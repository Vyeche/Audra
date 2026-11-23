//! Mackie Control (manual 24.2): a control surface with motor faders,
//! V-Pots, buttons and a display drives a block of eight strips. The
//! surface talks Mackie Control Universal over MIDI; Audra hears it through
//! its MIDI input (`midi`), the events of the chosen surface coming here
//! instead of going to the learned bindings.
//!
//! What the surface does (RME's mapping, with the protocol's own names
//! for the navigation keys accepted too):
//!
//! | On the surface | In Audra |
//! |---|---|
//! | Faders 1-8, master fader | The block's eight strips; Main |
//! | V-Pots 1-8, pressed | Pan; centre |
//! | Mute, Solo, Select 1-8 | Mute, solo, select (gang) |
//! | Rec 1-8 | Edit the mix of output pair 1-8 |
//! | Bank ◀▶, cursor ◀▶ | The block 8 strips left or right |
//! | Channel ◀▶, Rewind / Fast forward | The block 1 strip left or right |
//! | Cursor ▲▼ | The block to the row above or below |
//! | EQ / Plug-in / Instrument / Flip | Mute master / Solo master / Trim Gains / Speaker B |
//! | Pan | Mono (not available yet: says so) |
//! | Stop / Play / Record | Dim / Talkback / Recall |
//! | F1-F8 | Snapshot 1-8 |
//! | Undo | Undo |
//!
//! Every action is the on-screen control's own message, so the lock, undo,
//! the sounds and Channel Layout's remote hiding treat the surface like
//! the mouse. Feedback to the surface (motor faders, lights, rings, the
//! display, meters) is `feedback`.

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::midi::{MidiEvent, MidiSource};

/// One thing the surface did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McuIn {
    /// Fader 0-7, or 8 for the master, to 0..=16383.
    Fader(u8, u16),
    /// A fader touched (`true`) or let go.
    Touch(u8, bool),
    /// V-Pot 0-7 turned by this many steps (negative: anticlockwise).
    VPot(u8, i8),
    /// A button (its note number) pressed (`true`) or released.
    Button(u8, bool),
}

/// The Mackie Control event in `ev`, if it is one.
pub fn decode(ev: &MidiEvent) -> Option<McuIn> {
    match ev.source {
        MidiSource::Bend { channel } if channel <= 8 => Some(McuIn::Fader(channel, ev.value.min(16383))),
        MidiSource::Note { channel: 0, number } if (104..=112).contains(&number) => {
            Some(McuIn::Touch(number - 104, ev.value > 0))
        }
        MidiSource::Note { channel: 0, number } => Some(McuIn::Button(number, ev.value > 0)),
        MidiSource::Cc { channel: 0, number } if (16..=23).contains(&number) => {
            // Relative: bit 6 is the direction, the rest the step count.
            let steps = (ev.value & 0x3f) as i8;
            Some(McuIn::VPot(number - 16, if ev.value & 0x40 != 0 { -steps } else { steps }))
        }
        _ => None,
    }
}

/// The note numbers of the buttons Audra uses.
pub mod note {
    pub const REC: u8 = 0;
    pub const SOLO: u8 = 8;
    pub const MUTE: u8 = 16;
    pub const SELECT: u8 = 24;
    pub const VPOT_PRESS: u8 = 32;
    pub const PAN: u8 = 42;
    pub const PLUG_IN: u8 = 43;
    pub const EQ: u8 = 44;
    pub const INSTRUMENT: u8 = 45;
    pub const BANK_LEFT: u8 = 46;
    pub const BANK_RIGHT: u8 = 47;
    pub const CHANNEL_LEFT: u8 = 48;
    pub const CHANNEL_RIGHT: u8 = 49;
    pub const FLIP: u8 = 50;
    pub const F1: u8 = 54;
    pub const UNDO: u8 = 81;
    pub const REWIND: u8 = 91;
    pub const FAST_FORWARD: u8 = 92;
    pub const STOP: u8 = 93;
    pub const PLAY: u8 = 94;
    pub const RECORD: u8 = 95;
    pub const UP: u8 = 96;
    pub const DOWN: u8 = 97;
    pub const LEFT: u8 = 98;
    pub const RIGHT: u8 = 99;
}

/// What the surface is set to and where its block of eight strips is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mackie {
    /// Settings → MIDI → Mackie Control.
    pub on: bool,
    /// The surface, by its port's name (addresses change on replug).
    pub device: Option<String>,
    /// Send meter levels to the surface's display.
    pub meters: bool,
    /// The block: its row (0 inputs, 1 playback, 2 outputs) and the
    /// index of its first strip in that row.
    pub row: usize,
    pub offset: usize,
    /// Faders held by a hand (8 the master): their positions aren't sent
    /// back while held, or the motor would fight the hand.
    pub touched: [bool; 9],
    /// What the surface was last sent, and where to (`feed` sends only
    /// what changed; a new surface gets everything).
    pub last: Option<Frame>,
    pub sent_to: Option<(i32, i32)>,
    /// When meters were last sent (the surface lets them fall by itself,
    /// so a held level is sent again).
    pub meters_at: Option<std::time::Instant>,
    /// An extender (a second surface with eight more strips): its port's
    /// name, whether it stands left of the main surface, and its own held
    /// faders and last frame.
    pub extender: Option<String>,
    pub extender_left: bool,
    pub touched_ext: [bool; 9],
    pub last_ext: Option<Frame>,
    pub sent_to_ext: Option<(i32, i32)>,
}

/// Strips on one surface.
pub const STRIPS: usize = 8;

/// Which surface: the main one (with the master fader and the function
/// keys) or the extender (eight more strips).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Main,
    Extender,
}

/// Whether the extender is set and connected.
pub fn extender_connected(state: &TuxMix) -> bool {
    state.mackie.on && extender_port(state).is_some()
}

/// The block's width: 16 with an extender, else 8.
pub fn width(state: &TuxMix) -> usize {
    if extender_connected(state) {
        STRIPS * 2
    } else {
        STRIPS
    }
}

/// Where a surface's eight strips start in the block.
fn first_of(state: &TuxMix, unit: Unit) -> usize {
    match (extender_connected(state), state.mackie.extender_left, unit) {
        (false, _, _) => 0,
        (true, true, Unit::Main) | (true, false, Unit::Extender) => STRIPS,
        (true, true, Unit::Extender) | (true, false, Unit::Main) => 0,
    }
}

/// A row's strips as the surface sees them: the mixer's, without those
/// kept from remote control (manual 21.5.3).
pub fn row_strips(state: &TuxMix, row: usize) -> Vec<ChannelId> {
    crate::app::mixer_strip_ids(state)
        .into_iter()
        .filter(|&c| crate::app::row_of(c) == row && !crate::app::remote_hidden(state, c))
        .collect()
}

/// The block's strips (up to eight, or sixteen with an extender), its
/// offset kept inside the row.
pub fn bank(state: &TuxMix) -> Vec<ChannelId> {
    let row = row_strips(state, state.mackie.row);
    let w = width(state);
    let start = clamp_offset(state.mackie.offset, row.len(), w);
    row.into_iter().skip(start).take(w).collect()
}

/// A surface's eight strips of the block.
fn unit_strips(state: &TuxMix, unit: Unit) -> Vec<ChannelId> {
    bank(state).into_iter().skip(first_of(state, unit)).take(STRIPS).collect()
}

/// An offset that still shows a block: at most the last strip that can
/// start a full block (or 0 for a short row).
fn clamp_offset(offset: usize, len: usize, width: usize) -> usize {
    offset.min(len.saturating_sub(width))
}

/// Moves the block by `by` strips within its row.
pub fn shift(state: &mut TuxMix, by: i32) {
    let len = row_strips(state, state.mackie.row).len();
    let w = width(state);
    let now = clamp_offset(state.mackie.offset, len, w) as i32;
    state.mackie.offset = clamp_offset((now + by).max(0) as usize, len, w);
}

/// Moves the block to the row above (`-1`) or below, at its start.
pub fn change_row(state: &mut TuxMix, by: i32) {
    state.mackie.row = (state.mackie.row as i32 + by).clamp(0, 2) as usize;
    state.mackie.offset = 0;
}

/// The mix strip `cid`'s fader and pan edit (outputs: their own level).
fn mix_of(state: &TuxMix, cid: ChannelId) -> usize {
    match cid {
        ChannelId::Output(ch) => ch,
        _ => crate::app::route_of(state, cid),
    }
}

/// What the surface's event does: the messages to send through `update`.
/// Banking, touch and select change `state` here directly (they are views
/// and selection, not the mix).
#[cfg(test)]
pub fn handle(state: &mut TuxMix, input: McuIn) -> Vec<Message> {
    handle_unit(state, input, Unit::Main)
}

/// `handle` for either surface: an extender has strips only (its master
/// fader channel and function keys, if any, do nothing).
pub fn handle_unit(state: &mut TuxMix, input: McuIn, unit: Unit) -> Vec<Message> {
    let strip = |state: &TuxMix, i: u8| unit_strips(state, unit).get(i as usize).copied();
    let level = |v: u16| crate::widgets::fader::t_to_vol(v as f32 / 16383.0);
    match input {
        McuIn::Fader(8, _) if unit == Unit::Extender => Vec::new(),
        McuIn::Fader(8, v) => {
            let pair = crate::control_room::monitor_pair(state);
            crate::control_room::pair_channels(state, pair)
                .first()
                .map(|&ch| vec![Message::VolumeChanged(ChannelId::Output(ch), pair, level(v))])
                .unwrap_or_default()
        }
        McuIn::Fader(i, v) => strip(state, i)
            .map(|cid| vec![Message::VolumeChanged(cid, mix_of(state, cid), level(v))])
            .unwrap_or_default(),
        McuIn::Touch(i, on) => {
            let touched = match unit {
                Unit::Main => &mut state.mackie.touched,
                Unit::Extender => &mut state.mackie.touched_ext,
            };
            if let Some(t) = touched.get_mut(i as usize) {
                *t = on;
            }
            // Letting go ends the fader's undo step, as a mouse release does.
            match (on, strip(state, i)) {
                (false, Some(cid)) => vec![Message::RangeCleared(cid)],
                _ => Vec::new(),
            }
        }
        McuIn::VPot(i, steps) => match strip(state, i) {
            Some(cid) if !matches!(cid, ChannelId::Output(_)) => {
                let out = mix_of(state, cid);
                let pan = state.device.pan(cid, out).unwrap_or(0) as i32;
                let new = (pan + steps as i32 * 4).clamp(-100, 100) as i8;
                vec![Message::PanChanged(cid, out, new)]
            }
            _ => Vec::new(),
        },
        McuIn::Button(_, false) => Vec::new(),
        // An extender's strip buttons (solo, mute, select, V-Pot press).
        McuIn::Button(n, true) if unit == Unit::Extender => {
            if (note::SOLO..=39).contains(&n) {
                button(state, n, unit)
            } else {
                Vec::new()
            }
        }
        McuIn::Button(n, true) => button(state, n, unit),
    }
}

fn button(state: &mut TuxMix, n: u8, unit: Unit) -> Vec<Message> {
    use note::*;
    let strip = |state: &TuxMix, i: u8| unit_strips(state, unit).get(i as usize).copied();
    let w = width(state) as i32;
    match n {
        REC..=7 => {
            let pair = (n - REC) as usize;
            if pair < state.device.output_pair_count() {
                vec![Message::SelectOutput(pair)]
            } else {
                Vec::new()
            }
        }
        SOLO..=15 => strip(state, n - SOLO)
            .map(|c| vec![Message::Solo(c, !crate::app::channel_is_soloed(state, c))])
            .unwrap_or_default(),
        MUTE..=23 => strip(state, n - MUTE)
            .map(|c| vec![Message::Mute(c, !crate::app::channel_is_muted(state, c))])
            .unwrap_or_default(),
        SELECT..=31 => {
            if let Some(c) = strip(state, n - SELECT) {
                if !state.selected.remove(&c) {
                    state.selected.insert(c);
                }
            }
            Vec::new()
        }
        VPOT_PRESS..=39 => match strip(state, n - VPOT_PRESS) {
            Some(c) if !matches!(c, ChannelId::Output(_)) => vec![Message::PanReset(c, mix_of(state, c))],
            _ => Vec::new(),
        },
        PAN => {
            state.notice = Some((
                "Mono needs a newer Babyface driver (the README says how to install it).".into(),
                std::time::Instant::now(),
            ));
            Vec::new()
        }
        PLUG_IN => vec![Message::GlobalSoloToggle],
        EQ => vec![Message::GlobalMuteToggle],
        INSTRUMENT => vec![Message::SetTrimAll(!state.trim_all)],
        FLIP => vec![Message::SpeakerBToggle],
        BANK_LEFT | LEFT => {
            shift(state, -w);
            Vec::new()
        }
        BANK_RIGHT | RIGHT => {
            shift(state, w);
            Vec::new()
        }
        CHANNEL_LEFT | REWIND => {
            shift(state, -1);
            Vec::new()
        }
        CHANNEL_RIGHT | FAST_FORWARD => {
            shift(state, 1);
            Vec::new()
        }
        UP => {
            change_row(state, -1);
            Vec::new()
        }
        DOWN => {
            change_row(state, 1);
            Vec::new()
        }
        F1..=61 => vec![Message::SnapshotSlotPressed(n - F1 + 1)],
        UNDO => vec![Message::Undo],
        STOP => vec![Message::DimToggle],
        PLAY => vec![Message::TalkbackToggle],
        RECORD => vec![Message::RecallMain],
        _ => Vec::new(),
    }
}

/// "AN1 – ADAT7/8" for the block, and its row's name, for Settings and the
/// row heads.
pub fn describe(state: &TuxMix) -> String {
    let b = bank(state);
    let name = |c: ChannelId| crate::app::short_label(&crate::app::strip_display_name(state, c)).to_string();
    let row = ["Hardware inputs", "Software playback", "Hardware outputs"][state.mackie.row.min(2)];
    match (b.first(), b.last()) {
        (Some(&a), Some(&z)) => format!("{row}, {} \u{2013} {}", name(a), name(z)),
        _ => format!("{row}, nothing to control"),
    }
}

// ---- Feedback: what the surface shows ----------------------------------

/// Everything the surface shows, as last sent or as it should be now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    /// Fader positions 0..=16383, eight strips and the master.
    pub faders: [u16; 9],
    /// Button lights, one bit per note number.
    pub leds: u128,
    /// V-Pot rings (CC 48-55 values).
    pub rings: [u8; 8],
    /// The display's two rows, 56 characters each.
    pub lcd: [String; 2],
    /// The two-character assignment display ("IN", "PB", "OU").
    pub assign: [u8; 2],
    /// Meter levels 0-12.
    pub meters: [u8; 8],
}

/// One message to the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McuOut {
    /// A fader position (channel 0-8).
    Bend(u8, u16),
    /// A light on or off (its note number).
    Light(u8, bool),
    /// A control change on channel 1 (rings, the assignment display).
    Cc(u8, u8),
    /// System exclusive (the display).
    Sysex(Vec<u8>),
    /// A meter level: channel pressure, strip in the top four bits.
    Meter(u8),
}

/// A name in the display's six characters: as it is when it fits; else
/// without its slash ("ADAT34"), then without inner vowels ("Vcl tk"),
/// then cut.
fn fit6(text: &str) -> String {
    let ascii: String = text
        .replace('\u{221e}', "inf")
        .replace('\u{2212}', "-")
        .chars()
        .map(|ch| if ch.is_ascii_graphic() || ch == ' ' { ch } else { '?' })
        .collect();
    if ascii.len() <= 6 {
        return ascii;
    }
    let no_slash = ascii.replace('/', "");
    if no_slash.len() <= 6 {
        return no_slash;
    }
    let mut out = String::new();
    for (i, ch) in no_slash.chars().enumerate() {
        if i == 0 || !"aeiouAEIOU".contains(ch) {
            out.push(ch);
        }
    }
    out.chars().take(6).collect()
}

/// A display row's text as the surface takes it: ASCII only, 56 wide,
/// seven characters a strip (six and a space).
fn lcd_row(cells: &[String]) -> String {
    let mut row = String::new();
    for i in 0..STRIPS {
        let cell = cells.get(i).map(|c| fit6(c)).unwrap_or_default();
        row.push_str(&format!("{cell:<6} "));
    }
    row
}

/// A level in dBFS as the surface's meter step (0 to 12).
pub fn meter_step(level: f32) -> u8 {
    const STEPS: [f32; 12] = [-60.0, -50.0, -40.0, -30.0, -20.0, -14.0, -10.0, -8.0, -6.0, -4.0, -2.0, 0.0];
    if level <= 0.0 {
        return 0;
    }
    let db = 20.0 * level.log10();
    STEPS.iter().filter(|&&t| db >= t - 0.0001).count() as u8
}

/// A pan (-100..100) as a V-Pot ring in "from the centre" mode.
pub fn ring_for_pan(pan: i8) -> u8 {
    let pos = ((pan as f32 + 100.0) / 200.0 * 10.0).round() as u8 + 1;
    0x10 | pos.clamp(1, 11)
}

/// A letter or digit on the assignment display (the protocol's 7-segment
/// code: A-Z are 1-26, digits are 0x30-0x39).
fn seven_seg(c: char) -> u8 {
    match c.to_ascii_uppercase() {
        c @ 'A'..='Z' => c as u8 - b'A' + 1,
        c @ '0'..='9' => c as u8,
        _ => 0x20,
    }
}

/// The frame the main surface should show now.
#[cfg(test)]
pub fn frame(state: &TuxMix) -> Frame {
    frame_unit(state, Unit::Main)
}

/// The frame a surface should show now: its eight strips; the master
/// fader, the function keys' lights and the row on the main one only.
pub fn frame_unit(state: &TuxMix, unit: Unit) -> Frame {
    let bank = unit_strips(state, unit);
    let mut f = Frame::default();
    let pos = |v: f32| (crate::widgets::fader::vol_to_t(v) * 16383.0).round() as u16;
    let mut names = Vec::new();
    let mut values = Vec::new();
    for (i, &cid) in bank.iter().enumerate() {
        let out = mix_of(state, cid);
        let level = state.device.volume(cid, out).unwrap_or(0.0);
        f.faders[i] = pos(level);
        let mut light = |note: u8, on: bool| {
            if on {
                f.leds |= 1u128 << note;
            }
        };
        light(note::SOLO + i as u8, crate::app::channel_is_soloed(state, cid));
        light(note::MUTE + i as u8, crate::app::channel_is_muted(state, cid));
        light(note::SELECT + i as u8, state.selected.contains(&cid));
        if !matches!(cid, ChannelId::Output(_)) {
            f.rings[i] = ring_for_pan(state.device.pan(cid, out).unwrap_or(0));
        }
        names.push(crate::app::short_label(&crate::app::strip_display_name(state, cid)).to_string());
        values.push(if level > 0.0 { crate::app::db_number(level) } else { "-inf".into() });
        let (meters, idx) = match cid {
            ChannelId::Input(i) => (&state.input_meters, i),
            ChannelId::Playback(i) => (&state.playback_meters, i),
            ChannelId::Output(i) => (&state.output_meters, i),
        };
        f.meters[i] = meters.get(idx).map_or(0, |m| meter_step(m.frame().value));
    }
    if unit == Unit::Extender {
        f.lcd = [lcd_row(&names), lcd_row(&values)];
        return f;
    }
    // The master fader: Main.
    let pair = crate::control_room::monitor_pair(state);
    if let Some(&ch) = crate::control_room::pair_channels(state, pair).first() {
        if let Some(out) = state.device.outputs().get(ch) {
            f.faders[8] = pos(crate::control_room::displayed_level(state, ch, out.volume));
        }
    }
    let mut light = |note: u8, on: bool| {
        if on {
            f.leds |= 1u128 << note;
        }
    };
    if state.sel_out < 8 {
        light(note::REC + state.sel_out as u8, true);
    }
    light(note::EQ, crate::app::any_channel_muted(state));
    light(note::PLUG_IN, crate::app::any_channel_soloed(state));
    light(note::INSTRUMENT, state.trim_all);
    light(note::FLIP, state.control_room.speaker_b);
    light(note::STOP, state.control_room.is_dimmed());
    light(note::PLAY, state.control_room.talkback);
    if let Some(n) = state.active_snapshot.filter(|n| (1..=8).contains(n)) {
        light(note::F1 + n as u8 - 1, true);
    }
    f.lcd = [lcd_row(&names), lcd_row(&values)];
    let tag = ["IN", "PB", "OU"][state.mackie.row.min(2)];
    let mut chars = tag.chars();
    f.assign = [seven_seg(chars.next().unwrap_or(' ')), seven_seg(chars.next().unwrap_or(' '))];
    f
}

/// What to send to move the surface from `old` (`None`: a fresh surface,
/// send it all) to `new`. A held fader's position is left alone; meters
/// are sent by `feed`, which repeats them.
#[cfg(test)]
pub fn diff(old: Option<&Frame>, new: &Frame, touched: &[bool; 9]) -> Vec<McuOut> {
    diff_unit(old, new, touched, Unit::Main)
}

/// `diff` for either surface: an extender's display has its own device
/// id (0x15), and it has no master fader or row display.
#[allow(clippy::needless_range_loop)]
pub fn diff_unit(old: Option<&Frame>, new: &Frame, touched: &[bool; 9], unit: Unit) -> Vec<McuOut> {
    let mut out = Vec::new();
    let (faders, device) = match unit {
        Unit::Main => (9, 0x14),
        Unit::Extender => (8, 0x15),
    };
    for ch in 0..faders {
        if !touched[ch] && old.is_none_or(|o| o.faders[ch] != new.faders[ch]) {
            out.push(McuOut::Bend(ch as u8, new.faders[ch]));
        }
    }
    for note in 0..128u8 {
        let on = new.leds >> note & 1 == 1;
        let was = old.map(|o| o.leds >> note & 1 == 1);
        let ours = if unit == Unit::Main { is_lit_button(note) } else { note < 32 };
        if was != Some(on) && (on || was.is_some() || ours) {
            out.push(McuOut::Light(note, on));
        }
    }
    for i in 0..STRIPS {
        if old.is_none_or(|o| o.rings[i] != new.rings[i]) {
            out.push(McuOut::Cc(48 + i as u8, new.rings[i]));
        }
    }
    for (row, offset) in [(0usize, 0u8), (1, 56)] {
        if old.is_none_or(|o| o.lcd[row] != new.lcd[row]) {
            let mut bytes = vec![0xF0, 0x00, 0x00, 0x66, device, 0x12, offset];
            bytes.extend(new.lcd[row].bytes());
            bytes.push(0xF7);
            out.push(McuOut::Sysex(bytes));
        }
    }
    if unit == Unit::Main && old.is_none_or(|o| o.assign != new.assign) {
        out.push(McuOut::Cc(0x4B, new.assign[0]));
        out.push(McuOut::Cc(0x4A, new.assign[1]));
    }
    out
}

/// The buttons Audra lights, cleared on a fresh surface so nothing stays
/// lit from before.
fn is_lit_button(note: u8) -> bool {
    use note::*;
    note < 32 || matches!(note, PLUG_IN | EQ | INSTRUMENT | FLIP | STOP | PLAY) || (F1..F1 + 8).contains(&note)
}

/// Brings the surfaces up to date (on every tick): what changed since the
/// last frame, and the meters every 120 ms while any shows a level.
pub fn feed(state: &mut TuxMix) {
    let main = if state.mackie.on { surface_port(state) } else { None };
    let ext = if state.mackie.on { extender_port(state) } else { None };
    if main != state.mackie.sent_to {
        writer(Cmd::Target(Unit::Main, main));
        state.mackie.sent_to = main;
        state.mackie.last = None;
    }
    if ext != state.mackie.sent_to_ext {
        writer(Cmd::Target(Unit::Extender, ext));
        state.mackie.sent_to_ext = ext;
        state.mackie.last_ext = None;
    }
    let due = state
        .mackie
        .meters_at
        .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(120));
    let mut sent_meters = false;
    for (unit, connected) in [(Unit::Main, main.is_some()), (Unit::Extender, ext.is_some())] {
        if !connected {
            continue;
        }
        let now_frame = frame_unit(state, unit);
        let (last, touched) = match unit {
            Unit::Main => (state.mackie.last.as_ref(), &state.mackie.touched),
            Unit::Extender => (state.mackie.last_ext.as_ref(), &state.mackie.touched_ext),
        };
        let mut out = diff_unit(last, &now_frame, touched, unit);
        let changed = last.is_none_or(|o| o.meters != now_frame.meters);
        if state.mackie.meters && (changed || (due && now_frame.meters.iter().any(|&m| m > 0))) {
            for (i, &m) in now_frame.meters.iter().enumerate() {
                out.push(McuOut::Meter(((i as u8) << 4) | m));
            }
            sent_meters = true;
        }
        if !out.is_empty() {
            writer(Cmd::Send(unit, out));
        }
        match unit {
            Unit::Main => state.mackie.last = Some(now_frame),
            Unit::Extender => state.mackie.last_ext = Some(now_frame),
        }
    }
    if sent_meters {
        state.mackie.meters_at = Some(std::time::Instant::now());
    }
}

/// The surface's input port, while it is connected.
fn surface_port(state: &TuxMix) -> Option<(i32, i32)> {
    let name = state.mackie.device.as_deref()?;
    state.midi_ports.iter().find(|p| p.name == name).map(|p| (p.client, p.port))
}

/// Which surface an event from sequencer address `from` belongs to, if
/// any. A surface and its extender on one USB device are one client with
/// two ports, so the port decides there; otherwise the client does (a
/// device's read and write ports needn't share a number).
pub fn unit_for(state: &TuxMix, from: (i32, i32)) -> Option<Unit> {
    if !state.mackie.on {
        return None;
    }
    let main = surface_port(state);
    let ext = extender_port(state);
    if Some(from) == ext {
        return Some(Unit::Extender);
    }
    if Some(from) == main {
        return Some(Unit::Main);
    }
    let shared = matches!((main, ext), (Some(m), Some(e)) if m.0 == e.0);
    match (main, ext) {
        (Some(m), _) if !shared && m.0 == from.0 => Some(Unit::Main),
        (_, Some(e)) if !shared && e.0 == from.0 => Some(Unit::Extender),
        _ => None,
    }
}

/// The extender's input port, while it is connected.
fn extender_port(state: &TuxMix) -> Option<(i32, i32)> {
    let name = state.mackie.extender.as_deref()?;
    state.midi_ports.iter().find(|p| p.name == name).map(|p| (p.client, p.port))
}

/// What the writer thread is told.
pub enum Cmd {
    /// Send this surface's messages to this port from now on (`None`: to
    /// nothing).
    Target(Unit, Option<(i32, i32)>),
    Send(Unit, Vec<McuOut>),
}

static WRITER: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Cmd>>> = std::sync::OnceLock::new();

/// Hands `cmd` to the writer thread, starting it the first time. The
/// writer is its own sequencer client ("Audra Mackie"), since the reader's
/// blocks in its read.
fn writer(cmd: Cmd) {
    if cfg!(test) {
        return;
    }
    let tx = WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let _ = std::thread::Builder::new().name("audra-mackie".into()).spawn(move || {
            if let Err(e) = write_loop(rx) {
                log::warn!("Mackie Control: {e}");
            }
        });
        std::sync::Mutex::new(tx)
    });
    if let Ok(tx) = tx.lock() {
        let _ = tx.send(cmd);
    }
}

fn write_loop(rx: std::sync::mpsc::Receiver<Cmd>) -> Result<(), String> {
    use alsa::seq::{Addr, EvCtrl, EvNote, Event, EventType, PortCap, PortSubscribe, PortType, Seq};
    let seq = Seq::open(None, Some(alsa::Direction::Playback), false).map_err(|e| format!("no ALSA sequencer ({e})"))?;
    let name = std::ffi::CString::new("Audra Mackie").expect("no NUL");
    let _ = seq.set_client_name(&name);
    let client = seq.client_id().map_err(|e| e.to_string())?;
    // One port per surface, each subscribed to its own device.
    let mut ports = Vec::new();
    for port_name in ["Audra Mackie", "Audra Mackie extender"] {
        let n = std::ffi::CString::new(port_name).expect("no NUL");
        let port = seq
            .create_simple_port(&n, PortCap::READ | PortCap::SUBS_READ, PortType::MIDI_GENERIC | PortType::APPLICATION)
            .map_err(|e| format!("could not make the Mackie port ({e})"))?;
        ports.push(port);
    }
    let index = |unit: Unit| if unit == Unit::Main { 0 } else { 1 };
    let mut targets: [Option<Addr>; 2] = [None, None];
    for cmd in rx {
        match cmd {
            Cmd::Target(unit, t) => {
                let me = Addr { client, port: ports[index(unit)] };
                if let Some(old) = targets[index(unit)].take() {
                    let _ = seq.unsubscribe_port(me, old);
                }
                if let Some((c, p)) = t {
                    let dest = Addr { client: c, port: p };
                    if let Ok(sub) = PortSubscribe::empty() {
                        sub.set_sender(me);
                        sub.set_dest(dest);
                        let _ = seq.subscribe_port(&sub);
                    }
                    targets[index(unit)] = Some(dest);
                    log::info!("Mackie Control: {unit:?} sending to {c}:{p}");
                }
            }
            Cmd::Send(unit, list) => {
                let port = ports[index(unit)];
                for out in list {
                    let mut ev = match &out {
                        McuOut::Bend(ch, v) => Event::new(EventType::Pitchbend, &EvCtrl { channel: *ch, param: 0, value: *v as i32 - 8192 }),
                        McuOut::Light(note, on) => Event::new(
                            EventType::Noteon,
                            &EvNote { channel: 0, note: *note, velocity: if *on { 127 } else { 0 }, off_velocity: 0, duration: 0 },
                        ),
                        McuOut::Cc(n, v) => Event::new(EventType::Controller, &EvCtrl { channel: 0, param: *n as u32, value: *v as i32 }),
                        McuOut::Sysex(bytes) => Event::new_ext(EventType::Sysex, bytes.clone()),
                        McuOut::Meter(v) => Event::new(EventType::Chanpress, &EvCtrl { channel: 0, param: 0, value: *v as i32 }),
                    };
                    ev.set_source(port);
                    ev.set_subs();
                    ev.set_direct();
                    let _ = seq.event_output(&mut ev);
                }
                let _ = seq.drain_output();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, update};

    fn ev(source: MidiSource, value: u16) -> MidiEvent {
        MidiEvent::new(source, value)
    }

    #[test]
    fn the_protocol_decodes() {
        assert_eq!(decode(&ev(MidiSource::Bend { channel: 3 }, 16383)), Some(McuIn::Fader(3, 16383)));
        assert_eq!(decode(&ev(MidiSource::Bend { channel: 8 }, 0)), Some(McuIn::Fader(8, 0)));
        assert_eq!(decode(&ev(MidiSource::Bend { channel: 9 }, 0)), None);
        assert_eq!(decode(&ev(MidiSource::Note { channel: 0, number: 105 }, 127)), Some(McuIn::Touch(1, true)));
        assert_eq!(decode(&ev(MidiSource::Note { channel: 0, number: 16 }, 127)), Some(McuIn::Button(16, true)));
        assert_eq!(decode(&ev(MidiSource::Cc { channel: 0, number: 17 }, 0x03)), Some(McuIn::VPot(1, 3)));
        assert_eq!(decode(&ev(MidiSource::Cc { channel: 0, number: 17 }, 0x42)), Some(McuIn::VPot(1, -2)));
        assert_eq!(decode(&ev(MidiSource::Cc { channel: 0, number: 7 }, 64)), None);
    }

    #[test]
    fn faders_mutes_and_pans_reach_the_block() {
        let mut state = new(true, None, None);
        state.mackie.on = true;
        let first = bank(&state)[0];
        for m in handle(&mut state, McuIn::Fader(0, 16383)) {
            let _ = update(&mut state, m);
        }
        let out = crate::app::route_of(&state, first);
        assert!(state.device.volume(first, out).unwrap() >= 0.99, "the fader's top (the device caps it as it caps a drag)");
        for m in handle(&mut state, McuIn::Button(note::MUTE, true)) {
            let _ = update(&mut state, m);
        }
        assert!(crate::app::channel_is_muted(&state, first));
        let before = state.device.pan(first, out).unwrap();
        for m in handle(&mut state, McuIn::VPot(0, 5)) {
            let _ = update(&mut state, m);
        }
        assert!(state.device.pan(first, out).unwrap() > before);
        // A release does nothing; touch is remembered.
        assert!(handle(&mut state, McuIn::Button(note::MUTE, false)).is_empty());
        let _ = handle(&mut state, McuIn::Touch(0, true));
        assert!(state.mackie.touched[0]);
    }

    #[test]
    fn the_block_banks_within_its_row_and_changes_rows() {
        let mut state = new(true, None, None);
        let inputs = row_strips(&state, 0);
        assert_eq!(bank(&state), inputs.iter().copied().take(STRIPS).collect::<Vec<_>>());
        let _ = handle(&mut state, McuIn::Button(note::CHANNEL_RIGHT, true));
        assert_eq!(bank(&state).first().copied(), inputs.get(1).copied().filter(|_| inputs.len() > STRIPS).or(inputs.first().copied()));
        let _ = handle(&mut state, McuIn::Button(note::DOWN, true));
        assert_eq!(state.mackie.row, 1);
        assert_eq!(bank(&state).first().copied(), row_strips(&state, 1).first().copied());
        let _ = handle(&mut state, McuIn::Button(note::DOWN, true));
        let _ = handle(&mut state, McuIn::Button(note::DOWN, true));
        assert_eq!(state.mackie.row, 2, "stops at the outputs");
        // A channel kept from remote control leaves the block.
        let _ = handle(&mut state, McuIn::Button(note::UP, true));
        let _ = handle(&mut state, McuIn::Button(note::UP, true));
        let first = bank(&state)[0];
        let _ = update(&mut state, Message::SetStripRemote(first, false));
        assert!(!bank(&state).contains(&first));
    }

    #[test]
    fn a_fresh_surface_gets_everything_and_then_only_changes() {
        let mut state = new(true, None, None);
        state.mackie.on = true;
        let f = frame(&state);
        let first = diff(None, &f, &[false; 9]);
        assert_eq!(first.iter().filter(|o| matches!(o, McuOut::Bend(..))).count(), 9, "every fader");
        assert_eq!(first.iter().filter(|o| matches!(o, McuOut::Sysex(_))).count(), 2, "both display rows");
        assert!(diff(Some(&f), &f, &[false; 9]).is_empty(), "nothing changed, nothing sent");
        // Mute the first strip: its light, nothing else.
        let first_strip = bank(&state)[0];
        let _ = update(&mut state, Message::Mute(first_strip, true));
        let g = frame(&state);
        assert_eq!(
            diff(Some(&f), &g, &[false; 9]),
            vec![McuOut::Light(note::MUTE, true), McuOut::Light(note::EQ, true)],
            "its light, and the mute master's (something is muted)"
        );
        // A held fader isn't moved under the hand.
        let mut h = g.clone();
        h.faders[0] = 1234;
        assert!(diff(Some(&g), &h, &[true, false, false, false, false, false, false, false, false]).is_empty());
    }

    #[test]
    fn the_display_rings_and_meters_encode_as_the_protocol_wants() {
        let row = lcd_row(&["Vocal".into(), "AN2".into(), "\u{221e}x".into()]);
        assert_eq!(row.len(), 56);
        assert!(row.starts_with("Vocal  AN2    infx  "));
        assert_eq!(fit6("ADAT3/4"), "ADAT34");
        assert_eq!(fit6("Vocal take"), "Vcl tk");
        assert_eq!(fit6("AN1/2"), "AN1/2");
        assert!(row.is_ascii());
        assert_eq!(ring_for_pan(0), 0x16, "centre");
        assert_eq!(ring_for_pan(-100), 0x11);
        assert_eq!(ring_for_pan(100), 0x1B);
        assert_eq!(meter_step(0.0), 0);
        assert_eq!(meter_step(1.0), 12, "0 dBFS");
        assert_eq!(meter_step(0.51), 9, "-5.8 dBFS: the -6 step");
        assert_eq!(meter_step(0.5), 8, "-6.02 dBFS: just under it");
        assert_eq!(seven_seg('I'), 9);
        let state = new(true, None, None);
        let sysex = diff(None, &frame(&state), &[false; 9])
            .into_iter()
            .find_map(|o| match o {
                McuOut::Sysex(b) => Some(b),
                _ => None,
            })
            .unwrap();
        assert_eq!(&sysex[..7], &[0xF0, 0x00, 0x00, 0x66, 0x14, 0x12, 0x00]);
        assert_eq!(*sysex.last().unwrap(), 0xF7);
    }

    /// A simulated session with a surface and an extender on one device
    /// (one client, two ports), playback row (eleven strips here: enough
    /// for both).
    fn with_extender(left: bool) -> TuxMix {
        let mut state = new(true, None, None);
        state.mackie.on = true;
        state.mackie.device = Some("X: main".into());
        state.mackie.extender = Some("X: ext".into());
        state.mackie.extender_left = left;
        state.midi_ports = vec![
            crate::midi::MidiPort { name: "X: main".into(), client: 24, port: 0 },
            crate::midi::MidiPort { name: "X: ext".into(), client: 24, port: 1 },
        ];
        state
    }

    #[test]
    fn an_extender_widens_the_block_to_sixteen_and_drives_its_half() {
        for left in [false, true] {
            let mut state = with_extender(left);
            for _ in 0..3 {
                let _ = update(&mut state, Message::Tick);
            }
            let all: Vec<ChannelId> = crate::app::mixer_strip_ids(&state)
                .into_iter()
                .filter(|c| crate::app::row_of(*c) == 0)
                .collect();
            let b = bank(&state);
            assert_eq!(b.len(), all.len().min(16), "the whole block at once");
            // The extender's first fader moves strip 0 when it stands left,
            // strip 8 when right (when the row has that many).
            let ext_first = if left { 0 } else { 8 };
            if let Some(&target) = b.get(ext_first) {
                for m in handle_unit(&mut state, McuIn::Fader(0, 0), Unit::Extender) {
                    let _ = update(&mut state, m);
                }
                let out = crate::app::route_of(&state, target);
                assert_eq!(state.device.volume(target, out).ok(), Some(0.0), "left={left}");
            }
            // Routing by port: same client, two ports.
            assert_eq!(unit_for(&state, (24, 1)), Some(Unit::Extender));
            assert_eq!(unit_for(&state, (24, 0)), Some(Unit::Main));
            assert_eq!(unit_for(&state, (99, 0)), None);
        }
    }

    #[test]
    fn the_extender_has_strips_only_and_its_own_display_id() {
        let mut state = with_extender(false);
        assert!(handle_unit(&mut state, McuIn::Button(note::STOP, true), Unit::Extender).is_empty(), "no transport");
        assert!(handle_unit(&mut state, McuIn::Fader(8, 9000), Unit::Extender).is_empty(), "no master");
        let f = frame_unit(&state, Unit::Extender);
        let out = diff_unit(None, &f, &[false; 9], Unit::Extender);
        assert!(!out.iter().any(|o| matches!(o, McuOut::Bend(8, _))));
        let sysex = out.iter().find_map(|o| if let McuOut::Sysex(b) = o { Some(b.clone()) } else { None }).unwrap();
        assert_eq!(sysex[4], 0x15, "the extender's device id");
        assert!(!out.iter().any(|o| matches!(o, McuOut::Cc(0x4A | 0x4B, _))), "no row display");
        // Bank buttons move by the whole sixteen.
        assert_eq!(width(&state), 16);
    }

    #[test]
    fn without_a_connected_extender_the_block_stays_eight() {
        let mut state = with_extender(true);
        state.midi_ports.truncate(1);
        assert_eq!(width(&state), 8);
        assert_eq!(unit_for(&state, (24, 1)), Some(Unit::Main), "no extender: the surface's client is the surface");
        assert_eq!(unit_for(&state, (24, 0)), Some(Unit::Main));
    }

    #[test]
    fn the_function_keys_map_to_the_control_room_and_snapshots() {
        let mut state = new(true, None, None);
        assert!(matches!(handle(&mut state, McuIn::Button(note::STOP, true))[..], [Message::DimToggle]));
        assert!(matches!(handle(&mut state, McuIn::Button(note::PLAY, true))[..], [Message::TalkbackToggle]));
        assert!(matches!(handle(&mut state, McuIn::Button(note::F1 + 2, true))[..], [Message::SnapshotSlotPressed(3)]));
        assert!(matches!(handle(&mut state, McuIn::Button(note::REC + 1, true))[..], [Message::SelectOutput(1)]));
        assert!(handle(&mut state, McuIn::Button(note::PAN, true)).is_empty());
        assert!(state.notice.as_ref().is_some_and(|(t, _)| t.contains("Mono")));
    }
}
