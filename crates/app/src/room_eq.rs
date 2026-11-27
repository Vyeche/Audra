//! Room EQ for the speakers (issue #29, the owner's yes on 2026-11-24:
//! "Yes sounds like a good idea. Please try to implement."): up to eight
//! EQ bands, a delay and a level trim for each speaker, on what the
//! computer plays to Main.
//!
//! It runs on the computer, in the same loop as an output's Loudness
//! (`dynamics_loop::run_output`): apps play to the virtual speaker
//! ("Audra Main 1/2"), the loop shapes it and writes Main's playback
//! channels, so those channels have one writer. The unit's own inputs never
//! pass through it: a mic heard straight through the unit is not tuned, and
//! the page says so. The sound itself is `audra_fx::room_eq`: boosts held to
//! +6 dB in all, every change gliding, off untouched.
//!
//! The page is a card like Workspaces, opened from Main's Settings panel.

use audra_fx::room_eq::{
    side_response_db, BandKind, RoomBand, RoomEqParams, RoomSide, MAX_BOOST_DB, MAX_DELAY_MS, MAX_HZ, MAX_Q, MIN_CUT_DB, MIN_HZ,
    MIN_Q, ROOM_BANDS, TRIM_MIN_DB,
};
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, row, text, Space};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use std::time::{Duration, Instant};
use tuxmix_core::ChannelId;

use crate::app::{Message, TuxMix};
use crate::dynamics_loop::DynState;
use crate::theme;

/// The bands' frequencies on a fresh page: an octave apart from 50 Hz,
/// where rooms boom and speakers differ most.
pub const FRESH_HZ: [f32; ROOM_BANDS] = [50.0, 100.0, 200.0, 400.0, 800.0, 1600.0, 3200.0, 6400.0];
/// A fresh band's width: about an octave.
pub const FRESH_Q: f32 = 1.4;
/// How long a second press may come to switch Room EQ off while apps play
/// to its speaker (as Loudness asks, `dynamics_panel`).
pub const OFF_ARM_FOR: Duration = Duration::from_secs(5);
/// The card's width: a plot and three knobs side by side.
const CARD_W: f32 = 500.0;
const PLOT_H: f32 = 150.0;
/// The plot's span in dB: the deepest cut to a little over the cap.
const PLOT_DB: (f32, f32) = (-18.0, 9.0);
/// Sound travels about 34.3 cm in a millisecond.
const CM_PER_MS: f32 = 34.3;

/// Settings with nothing done: off, every band off at its fresh frequency.
pub fn fresh() -> RoomEqParams {
    let mut side = RoomSide::default();
    for (b, hz) in side.bands.iter_mut().zip(FRESH_HZ) {
        *b = RoomBand { on: false, kind: BandKind::Peak, freq_hz: hz, gain_db: 0.0, q: FRESH_Q };
    }
    RoomEqParams { on: false, sides: [side, side] }
}

/// Which speaker the page edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sides {
    /// Both at once (they start the same; the left's values are shown).
    #[default]
    Both,
    Left,
    Right,
}

impl Sides {
    fn edits(self) -> &'static [usize] {
        match self {
            Sides::Both => &[0, 1],
            Sides::Left => &[0],
            Sides::Right => &[1],
        }
    }

    /// The side whose values are shown.
    fn shown(self) -> usize {
        if self == Sides::Right { 1 } else { 0 }
    }
}

/// What the page asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum RoomMsg {
    Power(bool),
    Sides(Sides),
    /// The band the knobs edit.
    Pick(usize),
    BandOn(bool),
    Kind(BandKind),
    Freq(f32),
    Gain(f32),
    Q(f32),
    Delay(f32),
    Trim(f32),
    /// Every band off and the speakers' delay and level back to none (on
    /// stays as it is; it glides there).
    Flat,
}

/// Main's pair: its first output channel.
pub fn main_left(state: &TuxMix) -> usize {
    state.control_room.assign.main * 2
}

/// Whether Room EQ keeps output pair `l`'s loop (and its virtual speaker).
pub fn keeps(state: &TuxMix, l: usize) -> bool {
    state.room_eq.on && l == main_left(state)
}

/// What output pair `l`'s loop runs: the page's settings on Main, off
/// elsewhere (Main moved, or Room EQ off), so it glides out.
pub fn params_for(state: &TuxMix, l: usize) -> RoomEqParams {
    if keeps(state, l) {
        state.room_eq
    } else {
        RoomEqParams { on: false, ..state.room_eq }
    }
}

/// The virtual speaker apps pick, as the sound settings show it.
pub fn speaker_label(state: &TuxMix) -> String {
    format!("Audra {}", crate::app::strip_display_name(state, ChannelId::Output(main_left(state))))
}

/// Whether switching Room EQ off now only asks first: apps play to its
/// speaker and nothing else (Loudness) keeps it, so they would move to the
/// speakers untuned (the owner's pick for Loudness, 2026-11-10: "Keep it
/// while running").
pub fn off_asks(state: &TuxMix) -> bool {
    let l = main_left(state);
    let armed = state.room_eq_off_armed.is_some_and(|at| at.elapsed() < OFF_ARM_FOR);
    let voice = state.dyn_outs.get(&l).is_some_and(|o| o.on);
    state.room_eq.on && !armed && !voice && in_use_lately(state, l)
}

/// `dynamics_panel::speaker_in_use` (it asks pactl), its answer kept for a
/// moment: a press asks it for its sound and again in its handler, and the
/// two must agree (the review: asked twice, they could differ).
fn in_use_lately(state: &TuxMix, l: usize) -> bool {
    use std::sync::Mutex;
    static LAST: Mutex<Option<(usize, Instant, bool)>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((at_l, at, used)) = *last {
        if at_l == l && at.elapsed() < Duration::from_millis(500) {
            return used;
        }
    }
    let used = crate::dynamics_panel::speaker_in_use(state, l);
    *last = Some((l, Instant::now(), used));
    used
}

/// Handles the page's messages.
pub fn update(state: &mut TuxMix, msg: RoomMsg) {
    let band = state.room_eq_band.min(ROOM_BANDS - 1);
    let sides = state.room_eq_sides.edits();
    match msg {
        RoomMsg::Power(false) if off_asks(state) => {
            state.room_eq_off_armed = Some(Instant::now());
            return;
        }
        RoomMsg::Power(_) => state.room_eq_off_armed = None,
        RoomMsg::Sides(s) => {
            state.room_eq_sides = s;
            return;
        }
        RoomMsg::Pick(i) => {
            state.room_eq_band = i.min(ROOM_BANDS - 1);
            return;
        }
        _ => {}
    }
    let p = &mut state.room_eq;
    match msg {
        RoomMsg::Power(on) => p.on = on,
        RoomMsg::Sides(_) | RoomMsg::Pick(_) => {}
        RoomMsg::BandOn(on) => sides.iter().for_each(|&s| p.sides[s].bands[band].on = on),
        RoomMsg::Kind(k) => sides.iter().for_each(|&s| {
            let b = &mut p.sides[s].bands[band];
            // Back to a bell from a shelf (its width held at the shelf's
            // most): a bell's own width again (round 2: it stayed 0.71).
            if k == BandKind::Peak && b.kind != BandKind::Peak {
                b.q = FRESH_Q;
            }
            b.kind = k;
        }),
        RoomMsg::Freq(hz) => sides.iter().for_each(|&s| p.sides[s].bands[band].freq_hz = hz.clamp(MIN_HZ, MAX_HZ)),
        // Turning a band's gain switches it on: the knob is the band.
        RoomMsg::Gain(db) => sides.iter().for_each(|&s| {
            let b = &mut p.sides[s].bands[band];
            b.gain_db = db.clamp(MIN_CUT_DB, MAX_BOOST_DB);
            b.on = true;
        }),
        RoomMsg::Q(q) => sides.iter().for_each(|&s| p.sides[s].bands[band].q = q.clamp(MIN_Q, MAX_Q)),
        RoomMsg::Delay(ms) => sides.iter().for_each(|&s| p.sides[s].delay_ms = ms.clamp(0.0, MAX_DELAY_MS)),
        RoomMsg::Trim(db) => sides.iter().for_each(|&s| p.sides[s].trim_db = db.clamp(TRIM_MIN_DB, 0.0)),
        RoomMsg::Flat => {
            let on = p.on;
            *p = RoomEqParams { on, ..fresh() };
        }
    }
    state.room_eq = state.room_eq.sanitized();
    state.prefs_dirty = true;
}

/// The sound a page press makes (`sounds::for_message`): None for a knob's
/// drag, as faders are.
pub fn sound(state: &TuxMix, msg: &RoomMsg) -> Option<crate::sounds::Sound> {
    use crate::sounds::Sound;
    match msg {
        RoomMsg::Power(false) if off_asks(state) => Some(Sound::Arm),
        RoomMsg::Power(on) | RoomMsg::BandOn(on) => Some(if *on { Sound::On } else { Sound::Off }),
        RoomMsg::Sides(_) | RoomMsg::Pick(_) | RoomMsg::Kind(_) | RoomMsg::Flat => Some(Sound::Tap),
        _ => None,
    }
}

/// How the loop is doing, in plain words.
fn status(state: &TuxMix) -> (String, bool) {
    let speaker = speaker_label(state);
    if !state.room_eq.on {
        return ("Off: what the computer plays reaches your speakers as it is.".into(), false);
    }
    if state.device.is_mock() {
        return ("In the simulation nothing plays; your settings are kept.".into(), false);
    }
    let out = state.dyn_outs.get(&main_left(state));
    // Failed, the loop is cleared for its retry: its reason stays (the
    // review: it read "Starting…").
    if let Some(why) = out.filter(|o| o.run.is_none()).and_then(|o| o.last_error.as_ref()) {
        return (format!("Stopped: {why} Audra tries again every 10 seconds."), true);
    }
    let run = out.and_then(|o| o.run.as_ref()).map(|r| r.state());
    match run {
        Some(DynState::Running) => (format!("On: apps that play to \u{201c}{speaker}\u{201d} are tuned. Pick it as your output in the sound settings."), false),
        Some(DynState::Failed(why)) => (format!("Stopped: {why} Audra tries again every 10 seconds."), true),
        _ => ("Starting\u{2026}".into(), false),
    }
}

fn freq_words(hz: f32) -> String {
    if hz >= 1000.0 {
        let k = hz / 1000.0;
        if k >= 10.0 { format!("{k:.0}k") } else { format!("{k:.1}k") }
    } else {
        format!("{hz:.0}")
    }
}

fn kind_words(k: BandKind) -> &'static str {
    match k {
        BandKind::Peak => "Bell",
        BandKind::LowShelf => "Low shelf",
        BandKind::HighShelf => "High shelf",
    }
}

/// The page.
pub fn panel(state: &TuxMix) -> Element<'_, Message> {
    let scale = state.ui_scale;
    let p = state.room_eq;
    let shown = state.room_eq_sides.shown();
    let band_i = state.room_eq_band.min(ROOM_BANDS - 1);
    let b = p.sides[shown].bands[band_i];
    let small = |label: String, on: bool, message: Message| {
        let key = button(text(label).size(theme::TEXT_SM * scale).center())
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
            .on_press(message);
        if on { key.style(theme::toggle_button(true, theme::accent())) } else { key.style(theme::page_key(11.0 * scale)) }
    };
    let sm = |s: &str| text(s.to_string()).size(theme::TEXT_SM * scale);
    let header = row![
        text("Room EQ").font(theme::SERIF).color(theme::text_primary()).size(theme::TEXT_ROW * scale),
        Space::new().width(Length::Fill),
        crate::app::close_x(Message::ToggleRoomEq, scale),
    ]
    .align_y(iced::Alignment::Center);
    let intro = sm("Tunes your speakers to your room: EQ bands for booms and dips, and a delay and level for each speaker, so both arrive together and as loud.")
        .color(theme::text_sec());
    let armed = state.room_eq_off_armed.is_some_and(|at| at.elapsed() < OFF_ARM_FOR);
    let power = crate::app::switch_row("Room EQ for Main", p.on, Some(Message::RoomEq(RoomMsg::Power(!p.on))), scale);
    let (said, problem) = status(state);
    let mut body = column![header, intro, power, sm(&said).color(if problem { theme::attention() } else { theme::muted() })]
        .spacing(theme::SPACE_MD * scale)
        .width(Length::Fill);
    // Main split into two mono outputs: the loop has one channel, so what
    // apps play to the speaker reaches the left output alone (round 2: the
    // right speaker gets none of it, and the line hid the status).
    if p.on && crate::dynamics_panel::out_pair_of(state, main_left(state)).len() < 2 {
        body = body.push(
            sm("Main is split into two mono outputs: what apps play to the speaker reaches the left one only, tuned. Link Main as a stereo pair to tune both speakers.")
                .color(theme::attention()),
        );
    }
    if armed {
        body = body.push(
            sm(&format!("Apps are playing to \u{201c}{}\u{201d}: switched off, they move to your speakers untuned. Press again to switch it off.", speaker_label(state)))
                .color(theme::attention()),
        );
    }
    // The automatic turn-down, said (the review: the plot didn't show it).
    let boost = audra_fx::room_eq::max_boost_db(&p, 48_000.0) as f32;
    if p.on && boost > MAX_BOOST_DB + 0.05 {
        body = body.push(
            sm(&format!("Turned down {:.1} dB overall, so no frequency gets more than 6 dB louder.", boost - MAX_BOOST_DB)).color(theme::muted()),
        );
    }
    body = body.push(
        canvas::Canvas::new(Plot { params: p, selected: (shown, band_i), both: state.room_eq_sides == Sides::Both, scale })
            .width(Length::Fill)
            .height(Length::Fixed(PLOT_H * scale)),
    );
    // Which speaker the knobs edit.
    let sides = row![
        sm("Edit").color(theme::text_sec()),
        small("Both".into(), state.room_eq_sides == Sides::Both, Message::RoomEq(RoomMsg::Sides(Sides::Both))),
        small("Left".into(), state.room_eq_sides == Sides::Left, Message::RoomEq(RoomMsg::Sides(Sides::Left))),
        small("Right".into(), state.room_eq_sides == Sides::Right, Message::RoomEq(RoomMsg::Sides(Sides::Right))),
    ]
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Center);
    body = body.push(sides);
    // The bands, one chip each: lit when on, outlined when picked.
    let mut chips = row![].spacing(theme::SPACE_TIGHT * scale);
    for (i, band) in p.sides[shown].bands.iter().enumerate() {
        let lit = band.on && band.gain_db != 0.0;
        let words = if lit { format!("{}\n{:+.1}", freq_words(band.freq_hz), band.gain_db) } else { format!("{}\n\u{2014}", freq_words(band.freq_hz)) };
        let chip = button(text(words).size(theme::TEXT_SM * scale).center())
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_TIGHT * scale])
            .width(Length::Fill)
            .style(theme::toggle_button(i == band_i, if lit { theme::accent() } else { theme::text_sec() }))
            .on_press(Message::RoomEq(RoomMsg::Pick(i)));
        chips = chips.push(chip);
    }
    body = body.push(chips);
    // The picked band.
    let kinds = row![
        small(kind_words(BandKind::LowShelf).into(), b.kind == BandKind::LowShelf, Message::RoomEq(RoomMsg::Kind(BandKind::LowShelf))),
        small(kind_words(BandKind::Peak).into(), b.kind == BandKind::Peak, Message::RoomEq(RoomMsg::Kind(BandKind::Peak))),
        small(kind_words(BandKind::HighShelf).into(), b.kind == BandKind::HighShelf, Message::RoomEq(RoomMsg::Kind(BandKind::HighShelf))),
    ]
    .spacing(theme::SPACE_SM * scale);
    body = body.push(crate::app::switch_row_note(
        band_title(band_i),
        "Turning its gain switches it on",
        b.on,
        Some(Message::RoomEq(RoomMsg::BandOn(!b.on))),
        scale,
    ));
    body = body.push(kinds);
    let knob = |caption: &'static str, k: crate::widgets::knob::Knob<Message>| -> Element<'_, Message> {
        column![sm(caption).color(theme::text_sec()), crate::widgets::knob::knob(k)]
            .spacing(theme::SPACE_TIGHT * scale)
            .align_x(iced::Alignment::Center)
            .width(Length::Fill)
            .into()
    };
    let base = |value: f32, range: (f32, f32), label: String| crate::widgets::knob::Knob {
        readout: crate::widgets::knob::Readout::Below,
        tint: None,
        value,
        range,
        label,
        arc_from_center: false,
        home: None,
        interactive: true,
        log_scale: false,
        modifiers: state.modifiers,
        scale,
        on_change: Box::new(|_| Message::Tick),
        on_reset: Box::new(|| Message::Tick),
    };
    let fresh_hz = FRESH_HZ[band_i];
    let band_knobs = row![
        knob("Frequency", crate::widgets::knob::Knob {
            log_scale: true,
            on_change: Box::new(|v| Message::RoomEq(RoomMsg::Freq(v.round()))),
            on_reset: Box::new(move || Message::RoomEq(RoomMsg::Freq(fresh_hz))),
            ..base(b.freq_hz, (MIN_HZ, MAX_HZ), format!("{} Hz", freq_words(b.freq_hz)))
        }),
        knob("Gain", crate::widgets::knob::Knob {
            home: Some(0.0),
            on_change: Box::new(|v| Message::RoomEq(RoomMsg::Gain((v * 2.0).round() / 2.0))),
            on_reset: Box::new(|| Message::RoomEq(RoomMsg::Gain(0.0))),
            ..base(b.gain_db, (MIN_CUT_DB, MAX_BOOST_DB), format!("{:+.1} dB", b.gain_db))
        }),
        knob("Width (Q)", crate::widgets::knob::Knob {
            log_scale: true,
            on_change: Box::new(|v| Message::RoomEq(RoomMsg::Q((v * 10.0).round() / 10.0))),
            on_reset: Box::new(|| Message::RoomEq(RoomMsg::Q(FRESH_Q))),
            // A shelf's width stops where it would ring (`MAX_SHELF_Q`).
            ..base(b.q, (MIN_Q, if b.kind == BandKind::Peak { MAX_Q } else { audra_fx::room_eq::MAX_SHELF_Q }), format!("{:.1}", b.q))
        }),
    ]
    .spacing(theme::SPACE_MD * scale);
    body = body.push(band_knobs);
    // The speaker: when it arrives and how loud.
    let side = p.sides[shown];
    let which = match state.room_eq_sides {
        Sides::Both => "Both speakers",
        Sides::Left => "Left speaker",
        Sides::Right => "Right speaker",
    };
    body = body.push(crate::app::settings_head(which, scale));
    let speaker_knobs = row![
        knob("Delay", crate::widgets::knob::Knob {
            home: Some(0.0),
            on_change: Box::new(|v| Message::RoomEq(RoomMsg::Delay((v * 10.0).round() / 10.0))),
            on_reset: Box::new(|| Message::RoomEq(RoomMsg::Delay(0.0))),
            ..base(side.delay_ms, (0.0, MAX_DELAY_MS), format!("{:.1} ms", side.delay_ms))
        }),
        knob("Level", crate::widgets::knob::Knob {
            home: Some(0.0),
            on_change: Box::new(|v| Message::RoomEq(RoomMsg::Trim((v * 2.0).round() / 2.0))),
            on_reset: Box::new(|| Message::RoomEq(RoomMsg::Trim(0.0))),
            ..base(side.trim_db, (TRIM_MIN_DB, 0.0), format!("{:+.1} dB", side.trim_db))
        }),
    ]
    .spacing(theme::SPACE_MD * scale);
    body = body.push(speaker_knobs);
    body = body.push(
        sm(&format!(
            "Delay the nearer speaker by its extra closeness: 1 ms is about {CM_PER_MS:.0} cm. Level only turns a speaker down, so match the louder one to the quieter."
        ))
        .color(theme::muted()),
    );
    body = body.push(small("Make it flat".into(), false, Message::RoomEq(RoomMsg::Flat)));
    // Honest about what it can't do (the issue's own words).
    for line in [
        "Your mic, heard straight through the unit, isn't tuned: it never passes through the computer.",
        "Wherever Main's playback channel also plays (say you sent it to the Phones), it's tuned there too.",
        "Boosts are held to +6 dB in all, and every change glides. For a quick fix with no delay, the Main strip's own EQ runs on the unit.",
    ] {
        body = body.push(sm(line).color(theme::muted()));
    }
    crate::app::card(state, body.into(), Message::ToggleRoomEq, CARD_W)
}

fn band_title(i: usize) -> &'static str {
    ["Band 1", "Band 2", "Band 3", "Band 4", "Band 5", "Band 6", "Band 7", "Band 8"][i.min(ROOM_BANDS - 1)]
}

/// The correction curves: the left speaker's in the accent, the right's
/// in the cool hue, the cap at +6 dB dashed, the picked band's frequency
/// marked.
struct Plot {
    params: RoomEqParams,
    selected: (usize, usize),
    both: bool,
    scale: f32,
}

impl<M> canvas::Program<M> for Plot {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let screen = crate::widgets::eq_plot::SCREEN;
        let ink = crate::widgets::eq_plot::SCREEN_INK;
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), screen);
        let (lo, hi) = PLOT_DB;
        let x_of = |hz: f32| bounds.width * (hz / MIN_HZ).log10() / (MAX_HZ / MIN_HZ).log10();
        let y_of = |db: f32| bounds.height * (hi - db.clamp(lo, hi)) / (hi - lo);
        // Grid: every 6 dB, and 100 Hz, 1 kHz, 10 kHz.
        for db in [-18.0f32, -12.0, -6.0, 0.0, 6.0] {
            let y = y_of(db);
            let a = if db == 0.0 { 0.45 } else { 0.15 };
            frame.stroke(&Path::line(Point::new(0.0, y), Point::new(bounds.width, y)), Stroke::default().with_color(Color { a, ..ink }).with_width(s));
        }
        for hz in [100.0f32, 1000.0, 10000.0] {
            let x = x_of(hz);
            frame.stroke(&Path::line(Point::new(x, 0.0), Point::new(x, bounds.height)), Stroke::default().with_color(Color { a: 0.15, ..ink }).with_width(s));
        }
        // The cap, dashed.
        let y_cap = y_of(MAX_BOOST_DB);
        let mut x = 0.0;
        while x < bounds.width {
            frame.stroke(&Path::line(Point::new(x, y_cap), Point::new((x + 4.0 * s).min(bounds.width), y_cap)), Stroke::default().with_color(Color { a: 0.5, ..theme::attention() }).with_width(s));
            x += 7.0 * s;
        }
        // The picked band's frequency.
        let (side, band) = self.selected;
        let xb = x_of(self.params.sides[side].bands[band].freq_hz);
        frame.stroke(&Path::line(Point::new(xb, 0.0), Point::new(xb, bounds.height)), Stroke::default().with_color(Color { a: 0.35, ..theme::accent() }).with_width(s));
        // The curves: right first, so the left (or both) sits on top.
        let n = (bounds.width / (2.0 * s)).max(32.0) as usize;
        let curve = |side: &RoomSide| {
            Path::new(|b| {
                for k in 0..=n {
                    let t = k as f32 / n as f32;
                    let hz = MIN_HZ * (MAX_HZ / MIN_HZ).powf(t);
                    let db = side_response_db(side, hz, 48_000.0);
                    let pt = Point::new(bounds.width * t, y_of(db));
                    if k == 0 { b.move_to(pt) } else { b.line_to(pt) }
                }
            })
        };
        let dim = if self.params.on { 1.0 } else { 0.4 };
        if !self.both || self.params.sides[0] != self.params.sides[1] {
            frame.stroke(&curve(&self.params.sides[1]), Stroke::default().with_color(Color { a: dim, ..theme::eq_band()[2] }).with_width(2.0 * s));
        }
        frame.stroke(&curve(&self.params.sides[0]), Stroke::default().with_color(Color { a: dim, ..theme::accent() }).with_width(2.0 * s));
        let _ = Size::ZERO;
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_turns_a_band_on_and_both_sides_follow() {
        let mut state = crate::app::new(true, None, None);
        state.room_eq = fresh();
        update(&mut state, RoomMsg::Pick(2));
        update(&mut state, RoomMsg::Gain(-4.5));
        for s in 0..2 {
            let b = state.room_eq.sides[s].bands[2];
            assert!(b.on && b.gain_db == -4.5, "side {s}");
        }
        // One speaker alone.
        update(&mut state, RoomMsg::Sides(Sides::Right));
        update(&mut state, RoomMsg::Delay(2.5));
        assert_eq!(state.room_eq.sides[0].delay_ms, 0.0);
        assert_eq!(state.room_eq.sides[1].delay_ms, 2.5);
    }

    #[test]
    fn the_page_keeps_values_in_range() {
        let mut state = crate::app::new(true, None, None);
        state.room_eq = fresh();
        update(&mut state, RoomMsg::Gain(40.0));
        update(&mut state, RoomMsg::Trim(6.0));
        update(&mut state, RoomMsg::Delay(500.0));
        let s = state.room_eq.sides[0];
        assert_eq!(s.bands[0].gain_db, MAX_BOOST_DB);
        assert_eq!(s.trim_db, 0.0, "level only turns a speaker down");
        assert_eq!(s.delay_ms, MAX_DELAY_MS);
    }

    #[test]
    fn flat_keeps_on_and_clears_the_rest() {
        let mut state = crate::app::new(true, None, None);
        state.room_eq = fresh();
        update(&mut state, RoomMsg::Power(true));
        update(&mut state, RoomMsg::Gain(3.0));
        update(&mut state, RoomMsg::Flat);
        assert!(state.room_eq.on);
        assert!(state.room_eq.sides.iter().all(|s| s.bands.iter().all(|b| !b.on)));
    }

    #[test]
    fn only_main_runs_it() {
        let mut state = crate::app::new(true, None, None);
        state.room_eq = RoomEqParams { on: true, ..fresh() };
        let main = main_left(&state);
        assert!(keeps(&state, main));
        assert!(!keeps(&state, main + 2));
        assert!(!params_for(&state, main + 2).on);
    }
}
