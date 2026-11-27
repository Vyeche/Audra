//! The D panel (issue #27). The owner, 2026-11-10: "dynmics should be a
//! menu called D on the side", with TotalMix FX's Dynamics panel as the
//! reference: a round ON key and "Dynamics" with Preset at the right; the
//! amplitude display (the curve, a white dot for the level now, light-blue
//! marks for the turn-down); Gain, Attack and Release beside it; Thresh.
//! and Ratio for the compressor (red) and the expander (green); then Auto
//! Level's ON key with Max. Gain, Headroom and Rise Time. "All should show
//! a D": every hardware input, and every output, the Phones and Main too.
//! What it is for, in the owner's words: "sculpt the sound, like add more
//! punch and attack to drums and control the dynamics of incoming and
//! outgoing signals".
//!
//! The Babyface has no dynamics in the unit, so Audra runs them on the
//! computer (`dynamics_loop`), and the panel says where they act:
//! - on an input, on what apps hear ("Audra processed mic"), one input at
//!   a time; you still hear yourself straight, with no delay;
//! - on an output (the owner's pick, "Computer sound"), on what the
//!   computer plays to its virtual speaker ("Audra Phones 3/4"); the unit's
//!   own inputs still reach the output at once.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use audra_fx::DynamicsParams;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, Space};
use iced::{Color, Element, Length, Task};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::dynamics_loop::{DynLoop, DynState};
use crate::theme;
use crate::widgets::knob::{knob, Knob, Readout};
use crate::widgets::strip::{self, hint, FlyoutKind};

/// The compressor's red and the expander's green, as TotalMix colors their
/// knobs and the curve's two ends (`dyn_plot` draws the same).
const COMP_RED: Color = Color::from_rgb(0xe0 as f32 / 255.0, 0x4a as f32 / 255.0, 0x5a as f32 / 255.0);
const EXP_GREEN: Color = Color::from_rgb(0x5c as f32 / 255.0, 0xc8 as f32 / 255.0, 0x6a as f32 / 255.0);

/// The D panel's scroll, so the keyboard can bring the ring into view (job
/// 13): the panel holds 27-odd controls and the keyboard walks them all,
/// most of them below the fold.
pub const PANEL_SCROLL_ID: &str = "dyn-panel-scroll";

/// The panel's stops, in the order they were drawn last: the names the
/// keyboard registered them under (`keyfocus::drawn_in`, so nothing has
/// to be noted by hand, the Auto-Tune switch from `live_spectrum.rs`
/// included).
fn stops_of(cid: ChannelId) -> Vec<String> {
    crate::keyfocus::drawn_in(crate::keyfocus::Region::Dyn(cid)).into_iter().map(|r| r.name).collect()
}

/// Where the panel scrolls to for the stop at `k` of `n`: the first at the
/// top, the last at the foot (the rule the Settings panel's own scroll
/// uses, `app::panel_scroll_to_focus`).
pub fn scroll_offset(k: usize, n: usize) -> f32 {
    if n > 1 { k as f32 / (n - 1) as f32 } else { 0.0 }
}

/// Scrolls the D panel to where the keyboard is, so the ring is in view
/// (job 13). Called from the keyboard's step and press handlers in `app.rs`.
pub fn scroll_to_focus(state: &TuxMix) -> Task<Message> {
    let Some(crate::keyfocus::KeyFocus::Item(id)) = state.key_focus else { return Task::none() };
    // The open D panel holding that stop (two can be open at once, so not
    // by the focused strip).
    let open = state.flyout_open.iter().filter(|(_, k)| *k == FlyoutKind::Dynamics).map(|(c, _)| *c);
    let mut asked = None;
    for cid in open {
        if let Some(k) = stop_at(cid, id) {
            return iced::widget::operation::snap_to(PANEL_SCROLL_ID, scrollable::RelativeOffset { x: 0.0, y: scroll_offset(k, stops_of(cid).len()) });
        }
        if keep_answer_id(state, cid) == Some(id) {
            asked = Some(cid);
        }
    }
    // The move question's answer was just raised and isn't drawn yet: the
    // card stands at the panel's top, under its header.
    if asked.is_some() && state.dyn_move_ask.is_some() {
        return iced::widget::operation::snap_to(PANEL_SCROLL_ID, scrollable::RelativeOffset { x: 0.0, y: 0.0 });
    }
    Task::none()
}

/// The panel's stop the keyboard is on, by the id it holds: its place
/// among the panel's stops.
fn stop_at(cid: ChannelId, id: u64) -> Option<usize> {
    crate::keyfocus::drawn_in(crate::keyfocus::Region::Dyn(cid)).iter().position(|r| r.id == id)
}

/// One output's D: a virtual speaker apps play to, shaped on its way to the
/// output.
pub struct OutDyn {
    pub on: bool,
    pub params: DynamicsParams,
    pub run: Option<DynLoop>,
    pub retry_at: Option<Instant>,
    pub last_error: Option<String>,
    /// When the loop was told to fade out before it goes (`CLOSE_FADE`).
    pub closing_at: Option<Instant>,
    /// The playback channels the running loop writes: a link or split of
    /// the pair restarts it (faded) on the new ones (round 3: it stayed mono
    /// after Main was linked).
    pub channels: Vec<usize>,
}

/// How long a loop that's no longer wanted keeps running, faded out (its
/// Loudness and Room EQ fade over 50 ms, then what waits in the queue
/// plays), before it goes and apps move to the speakers: dropped at once,
/// apps landed there as loud as the cuts, trim and headroom had held them
/// down (the Room EQ review, #29).
pub const CLOSE_FADE: Duration = Duration::from_millis(250);
/// On quit, the longest Audra waits for Room EQ to glide flat before the
/// loops go anyway (cuts give their level back at 75 dB/s per speaker,
/// narrow bass cuts at half that; the deepest allowed set takes under 5 s,
/// an ordinary one under 1 s; round 6). Otherwise a loop waits until flat.
pub const CLOSE_MAX: Duration = Duration::from_secs(6);

impl OutDyn {
    fn new(params: DynamicsParams) -> Self {
        Self { on: false, params, run: None, retry_at: None, last_error: None, closing_at: None, channels: Vec::new() }
    }

    /// As the prefs kept it.
    pub fn kept(on: bool, params: DynamicsParams) -> Self {
        Self { on, ..Self::new(params) }
    }
}

/// What the panel asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum DynMsg {
    /// The processed path on or off: the virtual mic, or the virtual
    /// speaker.
    Power(bool),
    /// Start the path as a pick does: at once, or, if the shaped voice runs
    /// on another input, ask first (`MoveVoice`).
    Start,
    /// The answer to "Move your shaped voice here?".
    MoveVoice(bool),
    /// The compressor and the expander (TotalMix's "Dynamics" key).
    Dynamics(bool),
    AutoLevel(bool),
    Param(DynParam, f32),
    Preset(usize),
    /// "Yours" in the preset menu: your own settings back (`keep_yours`).
    Yours,
    /// The folded Fine-tune line opened or closed (a mic's Voice panel).
    FineTune(bool),
    /// "Move it to …" on another mic's card: Voice runs here from now on.
    MoveHere,
    /// The simple section (the owner's pick after ChatGPT's notes: "Simple
    /// on top, More below"): voice strength 0 off, 1 gentle, 2 medium, 3
    /// firm.
    Strength(u8),
    /// Background noise: 0 off, 1 light, 2 strong.
    Noise(u8),
    /// Keep my voice even (Auto Level bringing quiet parts up).
    Even(bool),
    /// An output's volume protection (its limiter) on or off.
    Protect(bool),
    /// An output's maximum volume, dB under full scale.
    MaxVolume(f32),
    /// An output's evening-out: 0 off, 1 light, 2 strong.
    EvenOut(u8),
    /// Everything off: the sound as it comes in.
    Natural,
    /// The TotalMix knobs shown (More) or not.
    More(bool),
    /// "Fit to my voice": start listening (true) or stop.
    Fit(bool),
    /// Noise removal: 0 off, 1 light, 2 strong.
    Removal(u8),
    /// Lower delay on or off.
    LowDelay(bool),
    /// The de-esser: 0 off, 1 light, 2 strong.
    DeEss(u8),
}

/// One of the panel's knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynParam {
    CompThreshold,
    CompRatio,
    Gain,
    Attack,
    Release,
    ExpThreshold,
    ExpRatio,
    AutoMaxGain,
    AutoHeadroom,
    AutoRise,
}

impl DynParam {
    /// The knob's sweep, TotalMix's where the engine allows it.
    pub fn range(self) -> (f32, f32) {
        match self {
            DynParam::CompThreshold => (-60.0, 0.0),
            DynParam::ExpThreshold => (-90.0, -20.0),
            DynParam::CompRatio | DynParam::ExpRatio => (1.0, 10.0),
            DynParam::Gain => (0.0, audra_fx::dynamics::MAKEUP_MAX_DB),
            DynParam::Attack => (0.1, 200.0),
            DynParam::Release => (10.0, 1000.0),
            DynParam::AutoMaxGain => (0.0, 18.0),
            DynParam::AutoHeadroom => (0.0, 12.0),
            DynParam::AutoRise => (0.1, 9.9),
        }
    }

    pub fn get(self, p: &DynamicsParams) -> f32 {
        match self {
            DynParam::CompThreshold => p.threshold_db,
            DynParam::CompRatio => p.ratio,
            DynParam::Gain => p.makeup_db,
            DynParam::Attack => p.attack_ms,
            DynParam::Release => p.release_ms,
            DynParam::ExpThreshold => p.gate_threshold_db,
            DynParam::ExpRatio => p.exp_ratio,
            DynParam::AutoMaxGain => p.auto_max_gain_db,
            DynParam::AutoHeadroom => p.auto_headroom_db,
            DynParam::AutoRise => p.auto_rise_s,
        }
    }

    /// `p` with this knob at `v` (within its sweep).
    pub fn set(self, p: &mut DynamicsParams, v: f32) {
        let (lo, hi) = self.range();
        let v = if v.is_finite() { v.clamp(lo, hi) } else { self.get(&DynamicsParams::default()) };
        match self {
            DynParam::CompThreshold => p.threshold_db = v,
            DynParam::CompRatio => p.ratio = v,
            DynParam::Gain => p.makeup_db = v,
            DynParam::Attack => p.attack_ms = v,
            DynParam::Release => p.release_ms = v,
            DynParam::ExpThreshold => p.gate_threshold_db = v,
            DynParam::ExpRatio => p.exp_ratio = v,
            DynParam::AutoMaxGain => p.auto_max_gain_db = v,
            DynParam::AutoHeadroom => p.auto_headroom_db = v,
            DynParam::AutoRise => p.auto_rise_s = v,
        }
    }

    /// Where a double-click puts it back.
    fn home(self) -> f32 {
        self.get(&DynamicsParams::default())
    }

    /// Its value with its unit (critique #30: "5", "150", "3.0" said
    /// nothing).
    fn label(self, v: f32) -> String {
        match self {
            DynParam::CompThreshold | DynParam::ExpThreshold => format!("{v:.0} dB"),
            DynParam::CompRatio | DynParam::ExpRatio => format!("{v:.1}:1"),
            DynParam::Gain | DynParam::AutoMaxGain | DynParam::AutoHeadroom => format!("{v:.1} dB"),
            DynParam::Attack | DynParam::Release => format!("{v:.0} ms"),
            DynParam::AutoRise => format!("{v:.1} s"),
        }
    }

    /// Its name for the keyboard's words.
    fn name(self) -> &'static str {
        match self {
            DynParam::CompThreshold => "Compressor threshold",
            DynParam::CompRatio => "Compressor ratio",
            DynParam::Gain => "Gain",
            DynParam::Attack => "Attack",
            DynParam::Release => "Release",
            DynParam::ExpThreshold => "Expander threshold",
            DynParam::ExpRatio => "Expander ratio",
            DynParam::AutoMaxGain => "Auto Level max gain",
            DynParam::AutoHeadroom => "Auto Level headroom",
            DynParam::AutoRise => "Auto Level rise time",
        }
    }

    /// What it does, in plain words (the owner isn't an audio engineer).
    fn tip(self) -> &'static str {
        match self {
            DynParam::CompThreshold => "Compressor, dB: over this level it turns the sound down. Lower evens out more of it",
            DynParam::CompRatio => "How firmly loud parts are held back: 1 does nothing, 4 is firm, 10 nearly a wall",
            DynParam::Gain => "Brings the whole back up after the compressor turned it down (up to 12 dB)",
            DynParam::Attack => "How fast the compressor reacts, in ms: slow lets each hit's front through (punch), fast catches it",
            DynParam::Release => "How fast it lets go after a loud part, in ms",
            DynParam::ExpThreshold => "Expander, dB: under this level it turns the sound down: room noise, hum, the gaps between words or hits",
            DynParam::ExpRatio => "How firmly quiet parts are turned down: 1 does nothing, 10 nearly silence",
            DynParam::AutoMaxGain => "Auto Level, dB: the most it brings quiet parts up",
            DynParam::AutoHeadroom => "Auto Level, dB: how far under full scale it holds the loudest parts (the whole gets that much quieter)",
            DynParam::AutoRise => "Auto Level, seconds: how slowly it brings the level back up after a loud part",
        }
    }
}

/// Whether strip `cid` has a D: every hardware input and every output (not
/// the playbacks, which have none in TotalMix either).
pub fn has_d(state: &TuxMix, cid: ChannelId) -> bool {
    match cid {
        ChannelId::Input(i) => i < state.device.inputs().len(),
        ChannelId::Output(i) => !state.device.outputs_one_per_pair() && i < state.device.outputs().len(),
        ChannelId::Playback(_) => false,
    }
}

/// An output's channels: the pair while linked, else itself.
pub fn out_pair_of(state: &TuxMix, i: usize) -> Vec<usize> {
    let l = i & !1;
    if state.device.output_linked(l / 2) && state.device.outputs().len() > l + 1 {
        vec![l, l + 1]
    } else {
        vec![i]
    }
}

/// The key a channel's D is kept under: its pair's first channel.
fn left_of(state: &TuxMix, cid: ChannelId) -> Option<usize> {
    match cid {
        ChannelId::Input(i) => Some(crate::autoset::pair_of(state, i)[0]),
        ChannelId::Output(i) => Some(out_pair_of(state, i)[0]),
        ChannelId::Playback(_) => None,
    }
}

/// An output's settings when it has none of its own yet: Limit, which
/// never makes anything louder.
fn output_start() -> DynamicsParams {
    audra_fx::dynamics::output_presets()[0].1
}

/// The settings strip `cid`'s D shows: the input's (one set, shared by
/// whichever input has it) or the output's own.
pub fn params_of(state: &TuxMix, cid: ChannelId) -> DynamicsParams {
    match (cid, left_of(state, cid)) {
        (ChannelId::Output(_), Some(l)) => state.dyn_outs.get(&l).map_or_else(output_start, |o| o.params),
        _ => state.dyn_params,
    }
}

/// "Move your shaped voice here?", in the panel of the input a pick asked
/// to move it to (`start_from_pick`). Inputs share one set of settings,
/// so the pick already applies where the voice runs; the question is only
/// where it runs.
fn move_question(state: &TuxMix, cid: ChannelId, scale: f32) -> Option<Element<'_, Message>> {
    let l = left_of(state, cid)?;
    if !matches!(cid, ChannelId::Input(_)) || state.dyn_move_ask != Some(l) {
        return None;
    }
    let here = crate::app::short_label(&crate::app::strip_display_name(state, cid)).to_string();
    let there = state.dyn_input.map(|i| crate::app::short_label(&crate::app::strip_display_name(state, ChannelId::Input(i))).to_string())?;
    let card_ground = theme::blend(theme::surface(), theme::attention(), 0.10);
    let ink = theme::readable_on(theme::attention(), theme::text_primary(), &[card_ground]);
    let key = |words: String, msg: Message, lit: bool| -> Element<'_, Message> {
        crate::keyfocus::reach_press(
            &words,
            button(text(words.clone()).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
                .style(if lit { theme::key_button(true, 0.0, 0.0, theme::RADIUS_SM * scale) } else { theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale) })
                .on_press(msg.clone())
                .into(),
            Some(msg),
            theme::RADIUS_SM * scale,
        )
    };
    Some(
        container(
            column![
                text(format!("Move Voice to {here}?")).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).color(ink),
                text(format!("It runs on {there} now: one mic at a time. Your settings already apply there.")).size(theme::TEXT_SM * scale).color(theme::text_sec()),
                row![
                    key(format!("Move it to {here}"), Message::Dyn(cid, DynMsg::MoveVoice(true)), true),
                    key(format!("Keep it on {there}"), Message::Dyn(cid, DynMsg::MoveVoice(false)), false),
                ]
                .spacing(theme::SPACE_SM * scale),
            ]
            .spacing(theme::SPACE_SM * scale),
        )
        .padding([theme::SPACE_SM * scale, (theme::SPACE_SM + 2.0) * scale])
        .width(Length::Fill)
        .style(move |_t: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(card_ground)),
            border: iced::Border { color: theme::attention(), width: 1.0, radius: (theme::RADIUS_SM * scale).into() },
            ..iced::widget::container::Style::default()
        })
        .into(),
    )
}

/// How long Reset to Natural's first press waits for the second.
pub(crate) const RESET_ARM_FOR: Duration = Duration::from_secs(4);
/// How long "off" (with apps playing here) waits for its second press.
pub(crate) const OFF_ARM_FOR: Duration = Duration::from_secs(5);

/// How long a running mic loop may go without a new block before the
/// watchdog calls it stuck: room for a slow start (it says Running before
/// the first block), and far under "silent for good".
const STALL: Duration = Duration::from_secs(2);

/// The watchdog's step: the block count and when it last moved, and
/// whether it has been still for `STALL`.
pub(crate) fn stalled(watch: Option<(u64, Instant)>, blocks: u64, now: Instant) -> (Option<(u64, Instant)>, bool) {
    match watch {
        Some((b, at)) if b == blocks => (watch, now.duration_since(at) >= STALL),
        _ => (Some((blocks, now)), false),
    }
}

/// Whether Cough acts: only while the shaped voice runs, on whichever
/// input has it (never in the simulation, nor before ON). Control Room's
/// Cough key, the C key and the hold's release all ask this.
pub fn cough_live(state: &TuxMix) -> bool {
    #[cfg(test)]
    if COUGH_LIVE_FOR_TESTS.with(|c| c.get()) {
        return true;
    }
    state.dyn_input.is_some() && state.dyn_loop.as_ref().is_some_and(|l| l.state() == DynState::Running)
}

#[cfg(test)]
thread_local! {
    /// Tests stand in for a running voice, which the simulation never has.
    pub static COUGH_LIVE_FOR_TESTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The strip the one-time hint points at (critique #31, the owner's "B":
/// the first time a mic is set up, a small note points at its D key): the
/// Quick setup's mic, on the Mixer, with tips on, its voice not yet shaped,
/// until answered or D is opened. (Not "once Quick setup's first step is
/// done": that step reads the strip's settings, which ask this, round and
/// round: a stack overflow.)
pub fn d_hint_due(state: &TuxMix) -> Option<ChannelId> {
    let cid = state.quick_channel;
    let due = state.tips
        && !state.d_hint_done
        && state.view == crate::app::View::Mixer
        && matches!(cid, ChannelId::Input(_))
        && has_d(state, cid)
        && !power_of(state, cid);
    due.then_some(cid)
}

/// Whether the hint's card stands in the counter's slot, so its two
/// buttons are keyboard stops (OpenCode's job 12): nothing ahead of it
/// there (a notice, a guide, a clipping input, a reward, Store waiting on
/// a short window). The keyboard's own card, also ahead of it, steps
/// aside for these two stops (`keyfocus::focus_words` names neither).
pub fn hint_stops_here(state: &TuxMix) -> bool {
    d_hint_due(state).is_some()
        && state.sidebar_open
        && state.notice.is_none()
        && state.guide.is_none()
        && state.celebration.is_none()
        && !matches!(crate::assist::current(state), Some(crate::assist::Assist::Clipping(_)))
        && !(state.snapshot_storing && crate::app::short_window(state))
}

/// The hint's card in the column's foot: what D holds, Show me and Got it.
pub fn hint_card(state: &TuxMix) -> Option<Element<'_, Message>> {
    let cid = d_hint_due(state)?;
    let scale = state.ui_scale;
    let name = crate::app::short_label(&crate::app::strip_display_name(state, cid)).to_string();
    let lamp = theme::readable_on_floors(theme::lamp());
    let line = iced::widget::rich_text::<(), Message, iced::Theme, iced::Renderer>([
        iced::widget::span(format!("Voice on {name}")).font(theme::SANS_SEMI).color(lamp),
        // Its short form where the column is short of room (critique #59:
        // at 1280x700 the full sentence cut the page keys above it).
        // Folded, the strip shows no D key: the card says Show me
        // unfolds it (critique #60 A 8).
        iced::widget::span(if state.collapsed.contains(&cid) {
            " shapes how others hear you. Show me unfolds it and opens Voice."
        } else if crate::app::foot_compact(state) {
            ": its D key shapes how others hear you."
        } else {
            // The dot named once, where it's met (critique #61's teaching
            // idea; "dot", not "amber": Contrast's lamp is white).
            " (its D key, marked with a dot) shapes how others hear you: ready-made sounds, noise removal and Auto-Tune."
        })
        .color(theme::text_primary()),
    ])
    .size(theme::TEXT_SM * scale)
    .width(Length::Fill);
    // Stops of the counter's own (the card is drawn outside any region, so
    // `reach_press` left its buttons out of the keyboard: job 12).
    let key = |words: &'static str, msg: Message, stop: crate::keyfocus::CounterItem| -> Element<'_, Message> {
        crate::keyfocus::ring(
            button(text(words).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                // Split-safe rims (critique #60 B 19: 2.03-2.40, carried
                // since #57).
                .style(theme::split_safe_key(theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale), false))
                .on_press(msg)
                .into(),
            crate::keyfocus::counter_focused(state, stop),
            theme::RADIUS_SM * scale,
        )
    };
    use crate::keyfocus::CounterItem::{DHintDone, DHintShow};
    Some(
        container(
            column![
                line,
                row![key("Show me", Message::DHint(true), DHintShow), key("Got it", Message::DHint(false), DHintDone)].spacing(theme::SPACE_SM * scale)
            ]
                .spacing(theme::SPACE_SM * scale),
        )
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .width(Length::Fill)
        .style(theme::trick_card)
        .into(),
    )
}

/// Whether strip `cid`'s processed path is on (its virtual mic or speaker).
pub fn power_of(state: &TuxMix, cid: ChannelId) -> bool {
    match (cid, left_of(state, cid)) {
        (ChannelId::Input(_), Some(l)) => state.dyn_input == Some(l),
        (ChannelId::Output(_), Some(l)) => state.dyn_outs.get(&l).is_some_and(|o| o.on),
        _ => false,
    }
}

/// The strip's D key lit: its path on, as the panel's ON key (one power
/// for the whole path since critique #31's "B").
pub fn lit(state: &TuxMix, cid: ChannelId) -> bool {
    power_of(state, cid)
}

/// How far strip `cid`'s dynamics turn it down now (0 or less; 0 while
/// its loop isn't running), for the light blue marks on its meter.
pub fn reduction_now(state: &TuxMix, cid: ChannelId) -> f32 {
    if let Some(r) = state.capture_reduction.filter(|_| lit(state, cid)) {
        return r;
    }
    loop_of(state, cid).filter(|l| l.state() == DynState::Running).map_or(0.0, DynLoop::meter_db)
}

/// The starting points the panel's Preset offers.
pub fn presets_for(cid: ChannelId) -> Vec<(&'static str, DynamicsParams)> {
    match cid {
        ChannelId::Output(_) => audra_fx::dynamics::output_presets().to_vec(),
        _ => audra_fx::dynamics::presets().into_iter().chain(audra_fx::dynamics::more_presets()).collect(),
    }
}

/// The preset menu's name for your own settings: kept when a preset, Reset
/// to Natural or Fit replaced them, so trying presets never loses a voice
/// you tuned (the owner's pick after critique #34: "Yours"). Reverb's
/// "Your last settings" works the same way.
pub(crate) const YOURS: &str = "Yours";

/// "Yours" in the preset menu, after the presets' own indexes.
const YOURS_I: usize = usize::MAX;

/// Whose "Yours" strip `cid`'s panel shows (`TuxMix::dyn_yours`): the
/// inputs share one set of settings, so one; each output its own.
fn yours_key(state: &TuxMix, cid: ChannelId) -> Option<(bool, usize)> {
    match cid {
        ChannelId::Input(_) => Some((false, 0)),
        ChannelId::Output(_) => left_of(state, cid).map(|l| (true, l)),
        _ => None,
    }
}

/// What "Yours" keeps: the settings and, on a mic, the extras (noise
/// removal, harsh S sounds, Auto-Tune), so one press brings the whole
/// voice back (the owner, after critique #35: "Yes, all of it").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Yours {
    pub params: DynamicsParams,
    pub extras: Option<Extras>,
}

/// A mic's extras: noise removal and harsh S levels (0 off, 1 Light, 2
/// Strong), and Auto-Tune.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extras {
    pub noise: u8,
    pub deess: u8,
    pub tune: bool,
}

impl Extras {
    fn of(state: &TuxMix) -> Self {
        Extras { noise: state.nr_level, deess: state.deess_level, tune: state.tune_on }
    }

    fn any(self) -> bool {
        self.noise > 0 || self.deess > 0 || self.tune
    }
}

/// Strip `cid`'s "Yours", if a preset, a reset or a fit ever replaced its
/// own settings: the settings part.
pub(crate) fn yours_of(state: &TuxMix, cid: ChannelId) -> Option<DynamicsParams> {
    yours_set_of(state, cid).map(|y| y.params)
}

/// Strip `cid`'s "Yours", extras included.
pub(crate) fn yours_set_of(state: &TuxMix, cid: ChannelId) -> Option<Yours> {
    yours_key(state, cid).and_then(|k| state.dyn_yours.get(&k).copied())
}

/// Keeps strip `cid`'s settings (and a mic's extras) as "Yours" before
/// something replaces them, when they are your own: no preset's, not
/// Natural, and not "Yours" already (so a second preset, or a reset after
/// one, never pushes your voice out). A preset or Fit replaces only the
/// settings, so only settings of your own count; Reset (`reset`) turns the
/// extras off too, so a preset with extras of your own counts as well.
/// Says so once it keeps them (critique #35: it was silent).
fn keep_yours(state: &mut TuxMix, cid: ChannelId, reset: bool) {
    // Whatever replaces the settings, a fit's report no longer describes
    // them (OpenCode's job 24: the voice guide's "fitted to you" stayed
    // ticked after a preset or Reset).
    if matches!(cid, ChannelId::Input(_)) {
        state.dyn_fit_report = None;
        state.dyn_fit_said = None;
    }
    let p = params_of(state, cid);
    let extras = matches!(cid, ChannelId::Input(_)).then(|| Extras::of(state));
    let a_preset = presets_for(cid).iter().any(|(_, q)| *q == p);
    let natural = p == audra_fx::dynamics::natural();
    let own_extras = reset && extras.is_some_and(Extras::any);
    let mine = Yours { params: p, extras };
    if ((a_preset || natural) && !own_extras) || yours_set_of(state, cid) == Some(mine) {
        return;
    }
    if let Some(k) = yours_key(state, cid) {
        state.dyn_yours.insert(k, mine);
        state.prefs_dirty = true;
        state.notice = Some(("Your own settings wait at the top of the preset list, as Yours.".to_string(), Instant::now()));
    }
}

/// The running loop of strip `cid`, when its path is on and running.
fn loop_of(state: &TuxMix, cid: ChannelId) -> Option<&DynLoop> {
    match (cid, left_of(state, cid)) {
        (ChannelId::Input(_), Some(l)) if state.dyn_input == Some(l) => state.dyn_loop.as_ref(),
        (ChannelId::Output(_), Some(l)) => state.dyn_outs.get(&l).and_then(|o| o.run.as_ref()),
        _ => None,
    }
}

fn set_params(state: &mut TuxMix, cid: ChannelId, p: DynamicsParams) {
    match (cid, left_of(state, cid)) {
        (ChannelId::Input(_), _) => {
            state.dyn_params = p;
            if let Some(l) = state.dyn_loop.as_ref() {
                l.set(p);
            }
        }
        (ChannelId::Output(_), Some(l)) => {
            let o = state.dyn_outs.entry(l).or_insert_with(|| OutDyn::new(output_start()));
            o.params = p;
            if let Some(r) = o.run.as_ref() {
                r.set(p);
            }
        }
        _ => return,
    }
    state.prefs_dirty = true;
}

/// Whether the panel's settings are at work: on an input, while the shaped
/// voice runs on any input (they share one set); on an output, while its
/// own path is on. Not at work, a chosen setting shows the "set, waiting"
/// look (the owner's pick after critique #32, "B"), so coral means working.
pub fn settings_live(state: &TuxMix, cid: ChannelId) -> bool {
    match cid {
        // In the simulation nothing runs anywhere, so only the panel whose
        // ON is lit shows its picks lit (critique #34: another mic's panel
        // said "nothing is shaped" over solid coral).
        ChannelId::Input(_) if state.device.is_mock() => power_of(state, cid),
        ChannelId::Input(_) => state.dyn_input.is_some(),
        _ => power_of(state, cid),
    }
}

/// A pick starts the path at once (the owner's pick after critique #32,
/// "A": you hear it right away). But if the shaped voice runs on another
/// input, it asks before moving it there; only the round ON moves it
/// outright.
fn start_from_pick(state: &mut TuxMix, cid: ChannelId, from: Option<&Message>) {
    if asks_to_move(state, cid) {
        state.dyn_move_ask = left_of(state, cid);
        ask_from_the_keyboard(state, cid, from);
        return;
    }
    set_power(state, cid, true);
}

/// Whether the Voice panel whose waits are keyed `(output, left)` is open:
/// a wait that runs out is heard only over its own panel (OpenCode's job
/// 35: any Voice panel spoke for it).
pub fn panel_shown(state: &TuxMix, output: bool, left: usize) -> bool {
    state.flyout_open.iter().any(|&(cid, kind)| {
        kind == FlyoutKind::Dynamics && matches!(cid, ChannelId::Output(_)) == output && left_of(state, cid) == Some(left)
    })
}

/// Whether Reset to Natural on panel `cid` is waiting for its second press
/// (the press now would reset, not arm).
pub fn reset_waiting(state: &TuxMix, cid: ChannelId) -> bool {
    let key = (matches!(cid, ChannelId::Output(_)), left_of(state, cid).unwrap_or(0));
    state.dyn_reset_armed.is_some_and(|(k, at)| k == key && at.elapsed() < RESET_ARM_FOR)
}

/// Whether a pick on mic `cid` asks before moving Voice there: it runs on
/// another input (`start_from_pick`; its sound waits too, `sounds`).
pub fn asks_to_move(state: &TuxMix, cid: ChannelId) -> bool {
    match (cid, left_of(state, cid)) {
        (ChannelId::Input(_), Some(l)) => state.dyn_input.is_some_and(|other| other != l),
        _ => false,
    }
}

/// The keyboard raised the question: it lands on the card's safe answer,
/// "Keep it on …", so a second Space changes nothing (job 13). A mouse
/// press moves no focus.
fn ask_from_the_keyboard(state: &mut TuxMix, cid: ChannelId, from: Option<&Message>) {
    // The card stands in the panel: with it closed there is nothing to
    // point at.
    if !state.flyout_open.contains(&(cid, FlyoutKind::Dynamics)) {
        return;
    }
    let Some(from) = from else { return };
    if !from_the_keyboard(state, cid, from) {
        return;
    }
    let Some(id) = keep_answer_id(state, cid) else { return };
    state.key_focus = Some(crate::keyfocus::KeyFocus::Item(id));
}

/// Whether the keyboard sent this message: its focus is on a control that
/// sends exactly this one (Space or Enter, or the arrows).
fn from_the_keyboard(state: &mut TuxMix, cid: ChannelId, from: &Message) -> bool {
    if !matches!(state.key_focus, Some(crate::keyfocus::KeyFocus::Item(_))) {
        return false;
    }
    let mut sent = Vec::new();
    if let Some(Some(m)) = crate::keyfocus::press(state) {
        sent.push(m);
    }
    if let Some(Some(m)) = crate::keyfocus::left_right(state, 1) {
        sent.push(m);
    }
    if let Some(Some(m)) = crate::keyfocus::left_right(state, -1) {
        sent.push(m);
    }
    sent.iter().any(|m| same_panel_message(m, from, cid))
}

/// Whether two of the panel's messages are the same one, for this strip.
fn same_panel_message(a: &Message, b: &Message, cid: ChannelId) -> bool {
    match (a, b) {
        (Message::Dyn(c1, m1), Message::Dyn(c2, m2)) => *c1 == cid && *c2 == cid && m1 == m2,
        _ => false,
    }
}

/// The card's safe answer, "Keep it on …": the id of that control, so the
/// keyboard can be put on it.
fn keep_answer_id(state: &TuxMix, cid: ChannelId) -> Option<u64> {
    let there = state.dyn_input.map(|i| crate::app::short_label(&crate::app::strip_display_name(state, ChannelId::Input(i))).to_string())?;
    Some(crate::keyfocus::first_id(crate::keyfocus::Region::Dyn(cid), &format!("Keep it on {there}")))
}

fn set_power(state: &mut TuxMix, cid: ChannelId, on: bool) {
    // ON, or an answer, settles any question.
    state.dyn_move_ask = None;
    match (cid, left_of(state, cid)) {
        (ChannelId::Input(_), Some(l)) => {
            let want = if on { Some(l) } else { state.dyn_input.filter(|&i| i != l) };
            if want != state.dyn_input {
                state.dyn_input = want;
                // Started for the new input (or stopped) at the next tick.
                state.dyn_loop = None;
                state.dyn_retry_at = None;
                state.dyn_last_error = None;
            }
        }
        (ChannelId::Output(_), Some(l)) => {
            let room = crate::room_eq::keeps(state, l);
            let o = state.dyn_outs.entry(l).or_insert_with(|| OutDyn::new(output_start()));
            o.on = on;
            // The loop goes in `ensure`, faded out first (`CLOSE_FADE`);
            // with Room EQ keeping it, Loudness only fades out of the sound
            // (`DynLoop::set_voice`).
            if !on && !room {
                o.retry_at = None;
                o.last_error = None;
            }
        }
        _ => return,
    }
    state.prefs_dirty = true;
}

/// Handles the panel's messages for strip `cid`.
pub fn update(state: &mut TuxMix, cid: ChannelId, msg: DynMsg) {
    if !has_d(state, cid) {
        return;
    }
    // The message as it came, to tell a keyboard press from a mouse one
    // (the move question's answer, job 13).
    let full = Message::Dyn(cid, msg.clone());
    let mut p = params_of(state, cid);
    match msg {
        DynMsg::Power(false) if matches!(cid, ChannelId::Output(_)) => {
            // The owner's pick (2026-11-10, "Keep it while running"): the
            // speaker goes only when asked twice while apps play there,
            // since they would move to the speakers unshaped.
            let l = left_of(state, cid).unwrap_or(0);
            let armed = state.dyn_off_armed.is_some_and(|(at, when)| at == l && when.elapsed() < OFF_ARM_FOR);
            // Heard here, once the outcome is known (the scout: it ticked
            // Off when it only asked).
            let sound = |state: &TuxMix, s: crate::sounds::Sound| {
                if state.ui_sounds {
                    crate::sounds::play(s, state.ui_sound_db);
                }
            };
            if !armed && in_use(state, l) && !crate::room_eq::keeps(state, l) {
                state.dyn_off_armed = Some((l, Instant::now()));
                sound(state, crate::sounds::Sound::Arm);
                return;
            }
            state.dyn_off_armed = None;
            sound(state, crate::sounds::Sound::Off);
            set_power(state, cid, false);
        }
        DynMsg::Power(on) => set_power(state, cid, on),
        DynMsg::Start => start_from_pick(state, cid, Some(&full)),
        DynMsg::MoveVoice(yes) => {
            let asked = state.dyn_move_ask.take();
            if yes && asked.is_some() && asked == left_of(state, cid) {
                set_power(state, cid, true);
            }
        }
        DynMsg::Dynamics(on) => {
            // ON brings back the parts that were on when it went off (a
            // preset without an expander keeps none), else both.
            let key = (matches!(cid, ChannelId::Output(_)), left_of(state, cid).unwrap_or(0));
            if on {
                let (c, g) = state.dyn_parts.remove(&key).filter(|&(c, g)| c || g).unwrap_or((true, true));
                p.comp_on = c;
                p.gate_on = g;
            } else {
                if p.comp_on || p.gate_on {
                    state.dyn_parts.insert(key, (p.comp_on, p.gate_on));
                }
                p.comp_on = false;
                p.gate_on = false;
            }
            // Only the part: the round ON at the top is the one power
            // (critique #32: this started the whole path, unlike what the
            // panel and the wiki said).
            set_params(state, cid, p);
        }
        DynMsg::AutoLevel(on) => {
            p.auto_on = on;
            set_params(state, cid, p);
        }
        DynMsg::Param(which, v) => {
            which.set(&mut p, v);
            set_params(state, cid, p);
        }
        DynMsg::Strength(level) => {
            match level {
                0 => p.comp_on = false,
                _ => {
                    let (ratio, threshold, makeup) = match level {
                        1 => (2.0, -20.0, 2.0),
                        2 => (3.0, -24.0, 4.0),
                        _ => (4.5, -28.0, 6.0),
                    };
                    p.comp_on = true;
                    p.ratio = ratio;
                    p.threshold_db = threshold;
                    p.makeup_db = makeup;
                    p.knee_db = 6.0;
                }
            }
            set_params(state, cid, p);
            // Picking a sound starts it; "Off" never does (critique #32:
            // pressing Off turned the path on, and moved the processed mic
            // to this input).
            if level > 0 {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::Noise(level) => {
            match level {
                0 => p.gate_on = false,
                1 => {
                    p.gate_on = true;
                    p.exp_ratio = 1.5;
                    p.gate_range_db = -12.0;
                    p.gate_threshold_db = -55.0;
                }
                _ => {
                    p.gate_on = true;
                    p.exp_ratio = 3.0;
                    p.gate_range_db = -30.0;
                    p.gate_threshold_db = -48.0;
                }
            }
            set_params(state, cid, p);
            if level > 0 {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::Even(on) => {
            // The safety limit stays; on, it also brings quiet parts up.
            p.auto_on = true;
            p.auto_max_gain_db = if on { 6.0 } else { 0.0 };
            if p.auto_headroom_db < 1.0 {
                p.auto_headroom_db = 2.0;
            }
            set_params(state, cid, p);
            if on {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::Protect(on) => {
            p.auto_on = on;
            set_params(state, cid, p);
            if on {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::MaxVolume(db) => {
            p.auto_on = true;
            p.auto_headroom_db = db.clamp(0.0, 12.0);
            set_params(state, cid, p);
            // The fit's end is no press: the keyboard raised nothing here.
            start_from_pick(state, cid, None);
        }
        DynMsg::EvenOut(level) => {
            p.auto_on = true;
            match level {
                0 => {
                    p.comp_on = false;
                    p.auto_max_gain_db = 0.0;
                }
                1 => {
                    p.comp_on = false;
                    p.auto_max_gain_db = 6.0;
                }
                _ => {
                    p.comp_on = true;
                    p.ratio = 3.0;
                    p.threshold_db = -30.0;
                    p.makeup_db = 4.0;
                    p.attack_ms = 10.0;
                    p.release_ms = 200.0;
                    p.auto_max_gain_db = 6.0;
                }
            }
            set_params(state, cid, p);
            if level > 0 {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::Natural => {
            // Asked twice, as 48V is: it wipes a tuned voice, with no Undo,
            // and stands under "During the call" (critique #33).
            let key = (matches!(cid, ChannelId::Output(_)), left_of(state, cid).unwrap_or(0));
            if !state.dyn_reset_armed.is_some_and(|(k, at)| k == key && at.elapsed() < RESET_ARM_FOR) {
                state.dyn_reset_armed = Some((key, Instant::now()));
                return;
            }
            state.dyn_reset_armed = None;
            // Your settings, extras included, stay under Preset as "Yours".
            keep_yours(state, cid, true);
            // Everything off, the extras too (it said "everything off" and
            // left noise removal, the de-esser and Auto-Tune on).
            set_params(state, cid, audra_fx::dynamics::natural());
            if matches!(cid, ChannelId::Input(_)) {
                state.nr_level = 0;
                state.deess_level = 0;
                state.tune_on = false;
                state.prefs_dirty = true;
            }
        }
        DynMsg::MoveHere => set_power(state, cid, true),
        DynMsg::FineTune(open) => {
            state.dyn_fine_open = open;
            state.prefs_dirty = true;
        }
        DynMsg::More(open) => {
            state.dyn_more = open;
            state.prefs_dirty = true;
        }
        DynMsg::DeEss(level) => {
            state.deess_level = level.min(2);
            if let Some(l) = state.dyn_loop.as_ref() {
                l.set_deess(state.deess_level);
            }
            state.prefs_dirty = true;
            if level > 0 {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::LowDelay(on) => {
            state.low_delay = on;
            if let Some(l) = state.dyn_loop.as_ref() {
                l.set_low_delay(on);
            }
            state.prefs_dirty = true;
        }
        DynMsg::Removal(level) => {
            state.nr_level = level.min(2);
            if let Some(l) = state.dyn_loop.as_ref() {
                l.set_nr(state.nr_level);
            }
            state.prefs_dirty = true;
            if level > 0 {
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::Fit(start) => {
            state.dyn_fit_said = None;
            state.dyn_fit_report = None;
            state.dyn_fit = start.then(|| FitRun { cid, started: Instant::now(), readings: Vec::new() });
        }
        DynMsg::Preset(i) => {
            if let Some((_, preset)) = presets_for(cid).get(i) {
                keep_yours(state, cid, false);
                state.dyn_parts.remove(&(matches!(cid, ChannelId::Output(_)), left_of(state, cid).unwrap_or(0)));
                set_params(state, cid, *preset);
                // A preset is a pick, and starts it as one does (critique
                // #33: a setting did, a preset didn't).
                start_from_pick(state, cid, Some(&full));
            }
        }
        DynMsg::Yours => {
            let Some(mine) = yours_set_of(state, cid) else { return };
            // Settings tuned since a preset swap places with "Yours", so
            // going back never loses them either.
            keep_yours(state, cid, false);
            state.dyn_parts.remove(&(matches!(cid, ChannelId::Output(_)), left_of(state, cid).unwrap_or(0)));
            set_params(state, cid, mine.params);
            // The extras too (the loop takes them on the next tick,
            // `ensure`).
            if let (ChannelId::Input(_), Some(x)) = (cid, mine.extras) {
                state.nr_level = x.noise.min(2);
                state.deess_level = x.deess.min(2);
                state.tune_on = x.tune;
            }
            start_from_pick(state, cid, Some(&full));
        }
    }
}

/// How long Voice's status line glows when an app begins recording from it.
pub const HEARD_FLARE: Duration = Duration::from_millis(1400);

/// How far into its glow the status line is, 1 at the start, 0 when done.
pub fn heard_flare(state: &TuxMix) -> f32 {
    state.dyn_heard_at.map_or(0.0, |at| (1.0 - at.elapsed().as_secs_f32() / HEARD_FLARE.as_secs_f32()).clamp(0.0, 1.0))
}

/// The least time between two glows, so an app that opens and closes the
/// mic again and again doesn't keep the card pulsing (OpenCode job 30).
pub const HEARD_REST: Duration = Duration::from_secs(10);

/// Whether the watch's new answer is an app beginning to hear Voice: a
/// "yes" after a known "no", rested since the last glow. "Unknown" (the
/// first look after a start, or an app already listening when Audra
/// opened) is not a beginning (OpenCode job 30).
pub fn glow_begins(before: Option<bool>, heard: Option<bool>, last: Option<Instant>, now: Instant) -> bool {
    heard == Some(true) && before == Some(false) && last.is_none_or(|at| now.saturating_duration_since(at) >= HEARD_REST)
}

/// While Voice runs on the unit, whether an app records from it, from a
/// watch off the UI thread (critique #35: Voice said "others hear you"
/// with no app listening). Never in the simulation, where tests set
/// `dyn_heard` themselves.
pub fn watch_listeners(state: &mut TuxMix) {
    if state.device.is_mock() {
        state.dyn_app_watch = None;
        return;
    }
    let running = state.dyn_loop.as_ref().is_some_and(|l| l.state() == DynState::Running);
    if running {
        let watch = state.dyn_app_watch.get_or_insert_with(|| crate::dynamics_loop::AppWatch::start(crate::dynamics_loop::SOURCE_NAME));
        let heard = watch.heard();
        // An app has just begun recording from Voice: the status line
        // glows for a moment (the owner: "keep improving the fun").
        let now = Instant::now();
        if glow_begins(state.dyn_heard, heard, state.dyn_heard_at, now) {
            state.dyn_heard_at = Some(now);
        }
        state.dyn_heard = heard;
    } else {
        state.dyn_app_watch = None;
        state.dyn_heard = None;
    }
}

/// The loops run while their path is on and the real unit is there (never
/// in the simulation); stopped when off; after a failure tried again every
/// 10 s, said once per new reason.
pub fn ensure(state: &mut TuxMix) {
    let mock = state.device.is_mock();
    let want = state.dyn_input.filter(|_| !mock);
    match (want, state.dyn_loop.as_ref().map(DynLoop::state)) {
        (None, Some(_)) => {
            state.dyn_loop = None;
            state.dyn_watch = None;
        }
        (Some(i), None) => {
            if state.dyn_retry_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(10)) {
                let channels = crate::autoset::pair_of(state, i);
                // Started here, on the app's thread, so it outlives the
                // loop (a failure here shows as the loop's own, which then
                // tries to start one itself).
                let _ = crate::dynamics_loop::keep_virtual_mic();
                state.dyn_loop = Some(DynLoop::start(channels, state.dyn_params));
                state.dyn_watch = None;
            }
        }
        (Some(_), Some(DynState::Failed(why))) => {
            state.dyn_loop = None;
            state.dyn_retry_at = Some(Instant::now());
            if state.dyn_last_error.as_deref() != Some(why.as_str()) {
                state.notice = Some((format!("Voice stopped: {why} Audra tries again every 10 seconds."), Instant::now()));
                state.dyn_last_error = Some(why);
            }
        }
        (Some(_), Some(DynState::Running)) => {
            state.dyn_last_error = None;
            let blocks = state.dyn_loop.as_ref().map_or(0, DynLoop::blocks);
            let (watch, stuck) = stalled(state.dyn_watch, blocks, Instant::now());
            state.dyn_watch = watch;
            if stuck {
                // Running, but no sound for a while: the mic's pipe stopped
                // without closing (a suspended device). Said, and tried
                // again, as a failure is (OpenCode's job 2: silent for good,
                // with the panel saying all was well).
                let why = "no sound came from the unit for 2 seconds.".to_string();
                state.dyn_loop = None;
                state.dyn_watch = None;
                state.dyn_retry_at = Some(Instant::now());
                state.notice = Some((format!("Voice stopped: {why} Audra tries again every 10 seconds."), Instant::now()));
                state.dyn_last_error = Some(why);
            }
        }
        _ => {}
    }
    // Voice off: its virtual mic goes too, after the loop and its player
    // (the pair last, as `kill_all` does; kept across moves and restarts
    // while on, `dynamics_loop::VIRTUAL_MIC`).
    if want.is_none() {
        crate::dynamics_loop::end_virtual_mic();
    }
    // Cough lets go when the voice stops while it's held: the key goes
    // inert, its release never comes, and the next start would begin
    // silenced (Qwen's review of critique #32's fixes). (It no longer lets
    // go with the D panel closed: it lives in Control Room now.)
    if state.cough && !cough_live(state) {
        state.cough = false;
        state.cough_by_key = false;
        state.cough_by_mouse = false;
    }
    // A move question goes when its panel closes, or the voice it would
    // move stops (or is already there).
    if let Some(l) = state.dyn_move_ask {
        let panel_open = state.flyout_open.iter().any(|(c, k)| *k == FlyoutKind::Dynamics && matches!(c, ChannelId::Input(_)) && left_of(state, *c) == Some(l));
        if !panel_open || state.dyn_input.is_none_or(|i| i == l) {
            state.dyn_move_ask = None;
        }
    }
    if let Some(l) = state.dyn_loop.as_ref() {
        l.set_tune(state.tune_on);
        l.set_cough(state.cough);
        l.set_nr(state.nr_level);
        l.set_low_delay(state.low_delay);
        l.set_deess(state.deess_level);
    }
    // Red once: others hear the voice late enough to notice; said once per
    // crossing, with the fix (the owner: "It could even suggest it in the
    // banner that comes up").
    let late = state.dyn_loop.as_ref().filter(|l| l.state() == DynState::Running).map(|l| l.delay_ms());
    match late {
        Some(ms) if ms >= 125.0 && !state.low_delay && !state.delay_warned => {
            state.delay_warned = true;
            state.notice = Some((
                format!("Others hear you {ms:.0} ms late, enough to notice against video: Lower delay, in Voice, pauses the extras that cost time (noise removal, Auto-Tune) and keeps your voice's protection."),
                Instant::now(),
            ));
        }
        Some(ms) if ms < 80.0 => state.delay_warned = false,
        _ => {}
    }
    // Room EQ runs in Main's loop (issue #29): it needs the entry even
    // when Loudness was never opened there.
    if state.room_eq.on {
        let main = crate::room_eq::main_left(state);
        state.dyn_outs.entry(main).or_insert_with(|| OutDyn::new(output_start()));
    }
    let rooms: BTreeMap<usize, (bool, audra_fx::room_eq::RoomEqParams)> = state
        .dyn_outs
        .keys()
        .map(|&l| (l, (crate::room_eq::keeps(state, l), crate::room_eq::params_for(state, l))))
        .collect();
    let names: BTreeMap<usize, String> =
        state.dyn_outs.keys().map(|&l| (l, crate::app::strip_display_name(state, ChannelId::Output(l)))).collect();
    let pairs: BTreeMap<usize, Vec<usize>> = state.dyn_outs.keys().map(|&l| (l, out_pair_of(state, l))).collect();
    let mut said = None;
    for (&l, o) in state.dyn_outs.iter_mut() {
        let (room_keeps, room) = rooms.get(&l).copied().unwrap_or_default();
        let channels = pairs.get(&l).cloned().unwrap_or_else(|| vec![l]);
        let moved = o.run.is_some() && o.channels != channels;
        // An entry for a pair's right half, after the pair was linked: its
        // channels now lead with the left's, whose own entry writes them, so
        // this one closes and stays closed (round 4: a split Main's right
        // Loudness restarted onto Main's channels, a second writer to the
        // speakers that bypassed Room EQ).
        let orphan = channels.first().is_some_and(|&first| first != l);
        let want = (o.on || room_keeps) && !mock && !moved && !orphan;
        if want {
            o.closing_at = None;
        }
        match (want, o.run.as_ref().map(DynLoop::state)) {
            // Faded out first (its Loudness and Room EQ are told off just
            // below), then gone.
            (false, Some(_)) => {
                let since = *o.closing_at.get_or_insert_with(Instant::now);
                // Only once it's flat (or not running at all): no time limit
                // here, so a deep set's glide is never cut short (round 6:
                // at a limit, the bass came back as a step). Quitting keeps
                // its limit (`stop_all`).
                let flat = o.run.as_ref().is_none_or(DynLoop::idle);
                if since.elapsed() >= CLOSE_FADE && flat {
                    o.run = None;
                    o.closing_at = None;
                }
            }
            (true, None) => {
                if o.retry_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(10)) {
                    let name = names.get(&l).cloned().unwrap_or_default();
                    o.channels = channels.clone();
                    // Room EQ off at the start, then on just below: it glides
                    // up rather than starting at its full settings (round 4,
                    // after a restart).
                    let start_room = audra_fx::room_eq::RoomEqParams { on: false, ..room };
                    o.run = Some(DynLoop::start_output_with(channels, format!("audra_out_{l}"), format!("Audra {name}"), o.params, o.on, start_room));
                }
            }
            (true, Some(DynState::Failed(why))) => {
                o.run = None;
                o.retry_at = Some(Instant::now());
                if o.last_error.as_deref() != Some(why.as_str()) {
                    let what = if o.on { "Loudness" } else { "Room EQ" };
                    said = Some(format!("{what} on {} stopped: {why} Audra tries again every 10 seconds.", names.get(&l).map_or("an output", String::as_str)));
                    o.last_error = Some(why);
                }
            }
            (true, Some(DynState::Running)) => o.last_error = None,
            _ => {}
        }
        // What the running loop does: Loudness fades in or out of the
        // sound, Room EQ glides to the page's settings.
        // A loop on its way out (not wanted, or restarting on new channels)
        // fades both out first, whatever the settings say.
        if let Some(run) = o.run.as_ref() {
            if o.closing_at.is_some() {
                run.set_voice(false);
                run.set_room(audra_fx::room_eq::RoomEqParams::default());
            } else {
                run.set_voice(o.on);
                run.set_room(room);
            }
        }
    }
    if let Some(words) = said {
        state.notice = Some((words, Instant::now()));
    }
}

/// Whether an app plays through output `l`'s virtual speaker now.
/// Whether apps play to output pair `l`'s virtual speaker now (Room EQ's
/// switch-off asks first then, as Loudness's does).
pub(crate) fn speaker_in_use(state: &TuxMix, l: usize) -> bool {
    in_use(state, l)
}

fn in_use(state: &TuxMix, l: usize) -> bool {
    state.dyn_outs.get(&l).is_some_and(|o| o.run.as_ref().is_some_and(|r| r.state() == DynState::Running))
        && crate::dynamics_loop::sink_in_use(&format!("audra_out_{l}"))
}

/// What quitting would do to apps now, one line each, so it can ask first:
/// apps on an output's virtual speaker move to the speakers unshaped; apps
/// recording from the processed mic lose it (ChatGPT's note on beginners,
/// the owner's pick: a warning, not a separate service).
pub fn quit_worries(state: &TuxMix) -> Vec<String> {
    let main = crate::sidebar::mix_name(state, 0);
    let keys: Vec<usize> = state.dyn_outs.keys().copied().collect();
    let mut lines: Vec<String> = keys
        .into_iter()
        .filter(|&l| in_use(state, l))
        .map(|l| format!("Apps playing to \u{201c}{}\u{201d} move to {main} (your speakers), without its shaping.", device_label(state, ChannelId::Output(l))))
        .collect();
    let mic_running = state.dyn_loop.as_ref().is_some_and(|l| l.state() == DynState::Running);
    // The watch's answer when it has one, so quitting doesn't wait on
    // `pactl` (OpenCode's job 22); asked here only if it hasn't.
    let mic_used = mic_running && state.dyn_heard.unwrap_or_else(|| crate::dynamics_loop::source_in_use(crate::dynamics_loop::SOURCE_NAME));
    if mic_used {
        lines.push(format!("Apps recording from \u{201c}{}\u{201d} lose it and switch to another mic.", crate::dynamics_loop::SOURCE_LABEL));
    }
    lines
}


/// Every loop stopped (Audra quitting): the virtual mic and speakers go.
pub fn stop_all(state: &mut TuxMix) {
    state.dyn_loop = None;
    crate::dynamics_loop::end_virtual_mic();
    // Each output's loop faded out first (Loudness and Room EQ off), so
    // apps moving to the speakers don't land louder by the cuts and trim
    // (round 2 of the Room EQ review), waited for at most `CLOSE_MAX`.
    let off = audra_fx::room_eq::RoomEqParams::default();
    for o in state.dyn_outs.values() {
        if let Some(run) = o.run.as_ref() {
            run.set_voice(false);
            run.set_room(off);
        }
    }
    let started = Instant::now();
    while started.elapsed() < CLOSE_MAX && !state.dyn_outs.values().all(|o| o.run.as_ref().is_none_or(DynLoop::idle)) {
        std::thread::sleep(Duration::from_millis(20));
    }
    for o in state.dyn_outs.values_mut() {
        o.run = None;
    }
}

/// What the virtual device is called in apps.
fn device_label(state: &TuxMix, cid: ChannelId) -> String {
    match cid {
        ChannelId::Output(_) => {
            let l = left_of(state, cid).unwrap_or(0);
            format!("Audra {}", crate::app::strip_display_name(state, ChannelId::Output(l)))
        }
        _ => crate::dynamics_loop::SOURCE_LABEL.to_string(),
    }
}

/// The input level now, for the white dot: the loop's own reading while it
/// runs; else, on an input, the unit's meter (an output's virtual speaker
/// has no level until it runs).
fn level_now(state: &TuxMix, cid: ChannelId) -> Option<f32> {
    if let Some(l) = loop_of(state, cid).filter(|l| l.state() == DynState::Running) {
        return Some(l.level_db());
    }
    let ChannelId::Input(i) = cid else { return None };
    let peak = crate::autoset::pair_of(state, i)
        .into_iter()
        .filter_map(|c| state.input_meters.get(c))
        .map(|m| m.frame().value)
        .fold(0f32, f32::max);
    Some(if peak > 1e-6 { 20.0 * peak.log10() } else { -120.0 })
}

/// An output's way in: its own playback channel, and whether that is in
/// this output's mix (else nothing shaped is heard here) or in others too.
/// `(words, a problem)`.
fn route(state: &TuxMix, cid: ChannelId) -> Option<(String, bool)> {
    let ChannelId::Output(_) = cid else { return None };
    let l = left_of(state, cid)?;
    let pb = ChannelId::Playback(l);
    let pb_name = format!("{} PB", crate::app::short_label(&crate::app::strip_display_name(state, pb)));
    let here = l / 2;
    let level = state.device.volume(pb, here).unwrap_or(0.0);
    let mix = crate::sidebar::mix_name(state, here);
    if state.device.playbacks().get(l).is_some_and(|c| c.mute) {
        return Some((format!("{pb_name} is muted, so nothing shaped is heard here."), true));
    }
    if level <= 0.0 {
        return Some((format!("{pb_name} isn't in {mix}'s mix, so nothing shaped is heard here: raise it there."), true));
    }
    let others: Vec<String> = (0..state.device.output_pair_count())
        .filter(|&q| q != here && state.device.volume(pb, q).unwrap_or(0.0) > 0.0)
        .map(|q| crate::sidebar::mix_name(state, q))
        .collect();
    let db = 20.0 * level.log10();
    let at = if db.abs() < 0.05 { "0 dB".to_string() } else { format!("{db:+.1} dB") };
    // Checked on the unit (2026-11-10): when the virtual speaker goes, an
    // app still playing to it moves to the unit's first pair, unshaped; so
    // switching it off while apps play asks first.
    if state.dyn_off_armed.is_some_and(|(at, when)| at == l && when.elapsed() < OFF_ARM_FOR) {
        return Some((
            format!("Apps are playing here: off moves them to {}, without this. Press again to switch it off.", crate::sidebar::mix_said(state, 0)),
            true,
        ));
    }
    Some(if others.is_empty() {
        // In plain words (critique #95 A P2-1: the limit was hidden in
        // a strip's code, "Through PH3/4 PB (0 dB here)").
        (format!("Only computer audio sent here (at {at}) is shaped. Mics and inputs in this mix are not."), false)
    } else {
        (format!("Through {pb_name} ({at} here), which also plays in {}.", others.join(", ")), true)
    })
}

/// How long "Fit to my voice" listens.
pub const FIT_FOR: Duration = Duration::from_secs(10);

/// A "Fit to my voice" run: the readings so far, one a tick, (loudness,
/// peak) in dBFS, from the unit's meter of the input.
pub struct FitRun {
    pub cid: ChannelId,
    pub started: Instant,
    pub readings: Vec<(f32, f32)>,
}

/// What a fit found, in dBFS: the room between words, the voice's typical
/// loud parts, and the thresholds set from them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub room_db: f32,
    pub loud_db: f32,
    pub comp_threshold_db: f32,
    pub exp_threshold_db: f32,
    /// The mic check (ChatGPT's thresholds, checked): the voice's typical
    /// loudness (speech's median), its peaks (speech's 95th percentile),
    /// and the readings at or near full scale.
    pub speech_db: f32,
    pub peak_db: f32,
    pub clipped: usize,
}

/// The thresholds from ten seconds of talking (ChatGPT's answer to the
/// engineering questions, checked): the room is the quiet quarter of the
/// readings; speech is what stands 10 dB over it; the compressor starts
/// 4.5 dB under speech's 75th percentile (about 3 dB of turn-down at 3:1
/// on the louder words), the expander 8 dB over the room but at least 3 dB
/// under the quietest speech, so word endings stay. `Err` says why not.
pub fn fit_from(readings: &[(f32, f32)]) -> Result<Fit, &'static str> {
    let mut loud: Vec<f32> = readings.iter().map(|(r, _)| *r).filter(|r| r.is_finite()).collect();
    if loud.len() < 20 {
        return Err("Not enough to go on: try again and talk for the whole ten seconds.");
    }
    loud.sort_by(f32::total_cmp);
    let quarter = &loud[..loud.len() / 4];
    let room = quarter[quarter.len() / 2];
    let speech: Vec<f32> = loud.iter().copied().filter(|r| *r > room + 10.0).collect();
    if speech.len() < loud.len() / 5 {
        return Err("Didn't hear enough talking: try again, speaking as you will on the call.");
    }
    if speech.len() > loud.len() * 19 / 20 {
        return Err("Didn't hear the room between your words: pause now and then while it listens.");
    }
    let at = |q: f32| speech[((speech.len() - 1) as f32 * q) as usize];
    let loud_db = at(0.75);
    let quiet_speech = at(0.10);
    let comp = (loud_db - 4.5).clamp(-40.0, -8.0);
    let exp = (room + 8.0).min(quiet_speech - 3.0).clamp(-80.0, -20.0);
    let mut peaks: Vec<f32> = readings.iter().filter(|(r, _)| *r > room + 10.0).map(|(_, p)| *p).filter(|p| p.is_finite()).collect();
    peaks.sort_by(f32::total_cmp);
    let peak_db = peaks.get((peaks.len().saturating_sub(1) as f32 * 0.95) as usize).copied().unwrap_or(-120.0);
    let clipped = readings.iter().filter(|(_, p)| *p >= -0.5).count();
    Ok(Fit { room_db: room, loud_db, comp_threshold_db: comp, exp_threshold_db: exp, speech_db: at(0.5), peak_db, clipped })
}

/// Whether noise removal is worth it here, from how far the room sits
/// under the voice, not how loud it is (measured with the EBU's speech,
/// 2026-11-10, `tools/checks/totalMix-voice-quality.py`: with the noise 30
/// dB under the voice, noise removal changed the voice more than the noise
/// did; 20 dB under, the voice came out no worse and the pauses 8 dB
/// (Light) or 18 dB (Strong) quieter; 10 dB under, the voice 6–7 dB
/// cleaner). 0 not needed, 1 Light, 2 Strong.
pub fn noise_advice(fit: &Fit) -> u8 {
    let gap = fit.speech_db - fit.room_db;
    if fit.room_db < -60.0 || gap >= 24.0 {
        0
    } else if gap >= 15.0 {
        1
    } else {
        2
    }
}

/// The mic check's verdicts, in plain words: (what, verdict, a problem?).
pub fn verdicts(fit: &Fit) -> [(&'static str, &'static str, bool); 3] {
    let level = if fit.peak_db >= -3.0 || fit.clipped > 2 {
        ("Too loud: turn the gain down (wrench, Gain)", true)
    } else if fit.speech_db < -30.0 {
        ("Too quiet: turn the gain up (wrench, Gain)", true)
    } else {
        ("Good", false)
    };
    let room = match noise_advice(fit) {
        0 if fit.room_db < -55.0 => ("Excellent", false),
        0 => ("Good: well under your voice", false),
        1 => ("Noticeable: noise removal (Light) will help", true),
        _ => ("High: noise removal (Strong), or a quieter room", true),
    };
    let clip = if fit.clipped == 0 { ("None", false) } else { ("Some: turn the gain down a little", true) };
    [("Voice level", level.0, level.1), ("Background noise", room.0, room.1), ("Clipping", clip.0, clip.1)]
}

/// Readings the trace keeps: about 4 s at the tick's pace.
pub const TRACE_LEN: usize = 125;

/// One reading a tick for every open D panel (the owner, 2026-11-10: "As
/// I'm talking, it's moving around the wave, so it's easier to shape the
/// sound"): the input level and the turn-down, from the loop while it
/// runs, else, on an input, from the unit's meter (no turn-down yet).
pub fn trace_tick(state: &mut TuxMix) {
    fit_tick(state);
    let open: Vec<ChannelId> =
        state.flyout_open.iter().filter(|(_, k)| *k == FlyoutKind::Dynamics).map(|(c, _)| *c).filter(|&c| has_d(state, c)).collect();
    state.dyn_trace.retain(|c, _| open.contains(c));
    for cid in open {
        let running = loop_of(state, cid).filter(|l| l.state() == DynState::Running).map(|l| (l.level_db(), l.meter_db()));
        let reading = running.or_else(|| level_now(state, cid).map(|l| (l, 0.0))).unwrap_or((-120.0, 0.0));
        let t = state.dyn_trace.entry(cid).or_default();
        t.push_back(reading);
        while t.len() > TRACE_LEN {
            t.pop_front();
        }
    }
}

/// One reading a tick for a "Fit to my voice" run, and the fit once it
/// has listened `FIT_FOR`: the thresholds set and the path turned on, or
/// why not, said on the panel.
fn fit_tick(state: &mut TuxMix) {
    let Some(run) = state.dyn_fit.as_mut() else { return };
    let ChannelId::Input(i) = run.cid else { return };
    let (l, r) = crate::quick::source_meters(state, ChannelId::Input(i));
    let to_db = |v: f32| if v > 1e-6 { 20.0 * v.log10() } else { -120.0 };
    // The level now, not the held peak (held, one clip counted for a
    // second of ticks and lifted every peak after it).
    let peak = l.value.max(r.map_or(0.0, |r| r.value));
    // The loudness (RMS) where the meters measure it; else the peak less
    // the 10 dB a voice's peaks stand over its loudness.
    let rms = [l.rms, r.map_or(-1.0, |r| r.rms)].into_iter().fold(-1.0, f32::max);
    let loud = if rms >= 0.0 { rms } else { peak * 0.316 };
    let Some(run) = state.dyn_fit.as_mut() else { return };
    run.readings.push((to_db(loud), to_db(peak)));
    if run.started.elapsed() < FIT_FOR {
        return;
    }
    let Some(run) = state.dyn_fit.take() else { return };
    let cid = run.cid;
    match fit_from(&run.readings) {
        Ok(fit) => {
            let mut p = params_of(state, cid);
            p.comp_on = true;
            p.gate_on = true;
            p.threshold_db = fit.comp_threshold_db;
            p.gate_threshold_db = fit.exp_threshold_db;
            if p.ratio < 1.5 {
                p.ratio = 3.0;
            }
            keep_yours(state, cid, false);
            set_params(state, cid, p);
            // The warm "kept" notes: your voice, fitted (the owner: subtle
            // fun sounds). Not in the simulation, where nothing was fitted
            // to a real voice (the scout).
            if state.ui_sounds && !state.device.is_mock() {
                crate::sounds::play(crate::sounds::Sound::Done, state.ui_sound_db);
            }
            // The fit's end is no press: the keyboard raised nothing here.
            start_from_pick(state, cid, None);
            // In the panel's own words; the dB stay under More (critique
            // #34: it answered in compressor and expander dB).
            state.dyn_fit_said = Some(if state.device.is_mock() {
                "Fitted to your voice: on the unit, your loud words get evened out and the room goes quiet between words. (Figures under More.)".to_string()
            } else {
                format!(
                    "Fitted to your voice: your loud words get evened out (from {:.0} dB) and the room goes quiet between words (under {:.0} dB). Figures under More.",
                    fit.comp_threshold_db, fit.exp_threshold_db
                )
            });
            state.dyn_fit_report = Some(fit);
        }
        Err(why) => {
            state.dyn_fit_said = Some(why.to_string());
            state.dyn_fit_report = None;
        }
    }
}

/// The panel's one line of what it is doing.
fn status(state: &TuxMix, cid: ChannelId) -> String {
    let label = device_label(state, cid);
    if state.device.is_mock() {
        return "Needs the unit: this is the simulation, so nothing is shaped.".into();
    }
    if !power_of(state, cid) {
        if let (ChannelId::Input(_), Some(other)) = (cid, state.dyn_input) {
            let name = crate::app::strip_display_name(state, ChannelId::Input(other));
            return format!("Runs on {name} now (one input at a time): turning it on here moves it.");
        }
        return match cid {
            // Room EQ keeps the speaker (#29): it's there, untouched by this.
            ChannelId::Output(_) if crate::room_eq::keeps(state, left_of(state, cid).unwrap_or(usize::MAX)) => {
                format!("Off: \u{201c}{label}\u{201d} stays for Room EQ; Loudness doesn't shape it.")
            }
            ChannelId::Output(_) => format!("Off: \u{201c}{label}\u{201d} isn't there; apps play straight to the unit."),
            _ => "Off: apps hear this input as it is.".into(),
        };
    }
    let (run, last_error) = match (cid, left_of(state, cid)) {
        (ChannelId::Output(_), Some(l)) => (
            state.dyn_outs.get(&l).and_then(|o| o.run.as_ref()),
            state.dyn_outs.get(&l).and_then(|o| o.last_error.clone()),
        ),
        _ => (state.dyn_loop.as_ref(), state.dyn_last_error.clone()),
    };
    match run.map(|l| (l.state(), l.meter_db(), l.auto_db())) {
        Some((DynState::Running, m, a)) => {
            let doing = if m < -0.5 { format!("Turning down {:.0} dB now.", -m) } else { "Not turning down now.".into() };
            let mut auto = if params_of(state, cid).auto_on && a.abs() >= 0.5 { format!(" Auto Level {a:+.0} dB.") } else { String::new() };
            if matches!(cid, ChannelId::Input(_)) && state.tune_on {
                auto.push_str(" Auto-Tune on.");
            }
            match cid {
                ChannelId::Output(_) => format!("Apps playing to \u{201c}{label}\u{201d} are shaped. {doing}{auto}"),
                _ => format!("Apps get it as \u{201c}{label}\u{201d}. {doing}{auto}"),
            }
        }
        Some((DynState::Failed(why), ..)) => why,
        _ => last_error.unwrap_or_else(|| "Starting\u{2026}".to_string()),
    }
}

/// A preset as the menu lists it; `shown`, as the closed menu shows the
/// one in force ("Preset: Volume protection", as Reverb's "Preset:
/// Default"; critique #109 A P3-31: under the "Loudness" title the bare
/// name read as a second name for the panel, beside the switch of the
/// same name).
#[derive(Debug, Clone)]
struct PresetName {
    i: usize,
    name: &'static str,
    shown: bool,
}

impl PartialEq for PresetName {
    fn eq(&self, other: &Self) -> bool {
        self.i == other.i
    }
}

impl std::fmt::Display for PresetName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.shown {
            write!(f, "Preset: {}", self.name)
        } else {
            f.write_str(self.name)
        }
    }
}

/// The D panel of strip `cid`, `width` wide.
pub fn popover(state: &TuxMix, cid: ChannelId, width: f32, scale: f32) -> Element<'_, Message> {
    crate::keyfocus::in_region(crate::keyfocus::Region::Dyn(cid), || body(state, cid, width, scale))
}

fn body(state: &TuxMix, cid: ChannelId, width: f32, scale: f32) -> Element<'_, Message> {
    if !has_d(state, cid) {
        return container(Space::new()).into();
    }
    let p = params_of(state, cid);
    let power = power_of(state, cid);
    let label = device_label(state, cid);
    let modifiers = state.modifiers;
    let dyn_on = p.comp_on || p.gate_on;
    let send = move |m: DynMsg| Message::Dyn(cid, m);
    // The knobs a little smaller than the strip's, as the EQ's are, so the
    // panel fits one strip's height.
    let kscale = scale * 0.9;
    let gap = theme::SPACE_SM * scale;
    let on_d = 24.0 * scale;
    let live = settings_live(state, cid);

    // A part's round ON key inside More (the compressor and expander,
    // Auto Level): it switches just that part, lit while the part is on.
    // The header's ON is the power for the whole path (critique #31's
    // "B"), so these no longer start the path themselves (Qwen's review).
    let on_key = |key: &'static str, set: bool, toggle: Message, tip: &'static str| -> Element<'_, Message> {
        let face = button(text("ON").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
            .padding(0)
            .width(Length::Fixed(on_d))
            .height(Length::Fixed(on_d))
            .on_press(toggle.clone())
            // Set but not at work: the "waiting" look (critique #32's pick).
            .style(theme::part_key(set, live, on_d / 2.0));
        crate::keyfocus::reach_press(key, hint(face, tip, scale), Some(toggle), on_d / 2.0)
    };

    // One knob with its caption over it (TotalMix sets Gain, Attack and
    // Release so) or none (the Thresh./Ratio grid has its words at the
    // row's start).
    let knob_of = |which: DynParam, caption: Option<(&'static str, Color)>, tint: Option<Color>| -> Element<'_, Message> {
        let v = which.get(&p);
        let range = which.range();
        let k = knob(Knob {
            readout: Readout::Below,
            tint,
            value: v,
            range,
            label: which.label(v),
            arc_from_center: false,
            home: None,
            interactive: true,
            log_scale: matches!(which, DynParam::Attack | DynParam::AutoRise),
            modifiers,
            scale: kscale,
            on_change: Box::new(move |v| send(DynMsg::Param(which, v))),
            on_reset: Box::new(move || send(DynMsg::Param(which, which.home()))),
        });
        let mut col = column![].align_x(iced::Alignment::Center).spacing(theme::SPACE_HAIRLINE * scale);
        if let Some((words, color)) = caption {
            col = col.push(text(words).size(theme::TEXT_SM * scale).color(color));
        }
        col = col.push(hint(k, which.tip(), scale));
        crate::keyfocus::reach_knob(
            which.name(),
            container(col).width(Length::Fill).center_x(Length::Fill).into(),
            v,
            range,
            matches!(which, DynParam::Attack | DynParam::AutoRise),
            None,
            move |v| send(DynMsg::Param(which, v)),
            theme::RADIUS_SM * scale,
        )
    };

    let name = crate::app::user_label(state, cid)
        .map_or_else(|| crate::app::short_label(&crate::app::strip_display_name(state, cid)).to_string(), str::to_string);
    let presets = presets_for(cid);
    let yours = yours_of(state, cid);
    let mut names: Vec<PresetName> = presets.iter().enumerate().map(|(i, (n, _))| PresetName { i, name: n, shown: false }).collect();
    if yours.is_some() {
        names.insert(0, PresetName { i: YOURS_I, name: YOURS, shown: false });
    }
    let now = presets
        .iter()
        .enumerate()
        .find(|(_, (_, q))| *q == p)
        // A mic's Sound box shows the sound's name big, alone (critique #35's
        // "B"); an output's menu says it is a preset.
        .map(|(i, (n, _))| PresetName { i, name: n, shown: matches!(cid, ChannelId::Output(_)) })
        .or_else(|| (yours == Some(p)).then_some(PresetName { i: YOURS_I, name: YOURS, shown: matches!(cid, ChannelId::Output(_)) }));
    let pick_msg = |pick: PresetName| if pick.i == YOURS_I { DynMsg::Yours } else { DynMsg::Preset(pick.i) };
    let h = 24.0 * scale;
    // The round ON is the power for the whole path, where TotalMix's ON
    // sits (the owner's pick after critique #31, "B: one power button"):
    // lit while others hear the shaped sound; a press turns all of it on
    // or off. An output asks twice while apps play there (amber).
    let off_armed = matches!(cid, ChannelId::Output(_))
        && power
        && state.dyn_off_armed.is_some_and(|(at, when)| Some(at) == left_of(state, cid) && when.elapsed() < OFF_ARM_FOR);
    let master: Element<'_, Message> = {
        let msg = send(DynMsg::Power(!power));
        let words: &'static str = match (cid, power, off_armed) {
            (_, _, true) => "Apps still play here: press ON again to turn it off (they would move to your first output, unshaped)",
            (ChannelId::Output(_), true, _) => "On: what apps play here is shaped. Press to turn it all off",
            (ChannelId::Output(_), false, _) => "Off: press to shape what apps play here",
            (_, true, _) => "On: Voice shapes what apps hear from \u{201c}Audra processed mic\u{201d}. Press to turn it all off",
            // Voice runs on another mic: ON here moves it, as the card's
            // key does (critique #36: an unexplained third way).
            (ChannelId::Input(_), false, _) if state.dyn_input.is_some_and(|i| Some(i) != left_of(state, cid)) => {
                "Off here: Voice runs on another mic. Press to move it here"
            }
            (_, false, _) => "Off: press to shape your voice for others",
        };
        let face = button(text("ON").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
            .padding(0)
            .width(Length::Fixed(on_d))
            .height(Length::Fixed(on_d))
            .on_press(msg.clone());
        let face = if off_armed { face.style(theme::key_button_armed(theme::attention(), on_d / 2.0)) } else { face.style(theme::power_key(power, on_d / 2.0)) };
        crate::keyfocus::reach_press("Power", hint(face, words, scale), Some(msg), on_d / 2.0)
    };
    let header = row![
        master,
        crate::keyfocus::reach_press(
            "Back to Settings",
            hint(
                button(text("\u{2039}").size(theme::TEXT_MD * scale).center())
                    .padding(0)
                    .width(Length::Fixed(on_d * 0.75))
                    .height(Length::Fixed(on_d))
                    .style(theme::ghost_button)
                    .on_press(Message::ToggleFlyout(cid, FlyoutKind::Settings)),
                "Back to its Settings",
                scale,
            ),
            Some(Message::ToggleFlyout(cid, FlyoutKind::Settings)),
            theme::RADIUS_SM * scale,
        ),
        text(if matches!(cid, ChannelId::Input(_)) { format!("{name} Voice") } else { format!("{name} Loudness") }).font(theme::SANS_SEMI).size(theme::TEXT_MD * scale).wrapping(iced::advanced::text::Wrapping::None),
        Space::new().width(Length::Fill),
        crate::app::panel_close(cid, FlyoutKind::Dynamics, scale),
    ]
    .spacing(theme::SPACE_MD * scale)
    .align_y(iced::Alignment::Center);

    // Voice runs on another mic: a card, not its settings (critique #33's
    // pick, "B"). Settled as soon as it's moved, or turned on here. Before
    // the presets are built: built, they'd be keyboard stops.
    let runs_elsewhere = match (cid, state.dyn_input) {
        (ChannelId::Input(_), Some(i)) if !power && Some(i) != left_of(state, cid) => Some(i),
        _ => None,
    };
    if let Some(there) = runs_elsewhere {
        let col = column![header, other_mic_card(state, cid, there, scale)].spacing(gap);
        return panel_frame(col, width, scale);
    }

    // Where it acts, on the path's own switch (the virtual device there or
    // not; picking a sound turns it on too, "Off" never does), with what it
    // is doing under it, so both show without scrolling.
    let lead = match cid {
        ChannelId::Output(_) => format!("Shapes what apps play to \u{201c}{label}\u{201d}, on its way here through this output's own playback channel; other sound in this mix isn't shaped. Your mic and instruments still reach this output at once. The speaker stays while Audra runs. If it goes while apps play here (ON off, or quitting), they move to your first output, so Audra asks first."),
        _ => "Voice shapes what others hear in calls, recordings and streams: pick \u{201c}Audra processed mic\u{201d} as the mic in the app. You still hear yourself straight, with no delay.".to_string(),
    };
    // In the simulation nothing is shaped, so no "on" line in green over
    // "nothing is shaped" (critique #32).
    let mock = state.device.is_mock();
    // The shaped voice running on another input: this panel's settings
    // shape it there (one set), so not "Off" (critique #33: coral picks
    // under "Off: others hear this input as it is").
    let elsewhere = match (cid, state.dyn_input) {
        // (Not in the simulation: nothing is shaped there.)
        (ChannelId::Input(_), Some(i)) if Some(i) != left_of(state, cid) && !mock => {
            Some(crate::app::short_label(&crate::app::strip_display_name(state, ChannelId::Input(i))).to_string())
        }
        _ => None,
    };
    let first = match (cid, power, off_armed) {
        (_, false, false) if elsewhere.is_some() => format!("\u{25cf} Voice runs on {}: these settings shape it there", elsewhere.clone().unwrap_or_default()),
        (_, _, true) => "Apps still play here: press ON again to turn it off".to_string(),
        (_, true, _) if mock => "On, but this is the simulation: nothing is shaped".to_string(),
        (ChannelId::Output(_), true, _) => format!("\u{25cf} What apps play to \u{201c}{label}\u{201d} is shaped"),
        // Said of what stands under it (critique #95 A P1-1: a lit
        // "Volume protection" switch under "Off" read as protection at work).
        (ChannelId::Output(_), false, _) => "Off: nothing below is working yet. Apps play straight to the unit".to_string(),
        // Honest about the call (critique #35): green only once an app
        // records from it.
        // Said as known (OpenCode's job 22): an app having the mic isn't
        // proof it's heard (OBS with it muted), and before the first check
        // nothing is known.
        (_, true, _) if state.dyn_heard == Some(false) => "\u{25cf} Voice is ready, but no app uses it yet".to_string(),
        (_, true, _) if state.dyn_heard == Some(true) => "\u{25cf} Voice is on, and an app is using it".to_string(),
        (_, true, _) => "\u{25cf} Voice is on".to_string(),
        (_, false, _) => "Off: others hear this input as it is".to_string(),
    };
    let second = match (cid, power) {
        (_, false) if elsewhere.is_some() => format!("One mic at a time: press ON to move it to {}.", crate::app::short_label(&crate::app::strip_display_name(state, cid))),
        // A mic's on card has its own Turn off Voice key (critique #37:
        // "Press ON" still pointed at the 24 px circle).
        (ChannelId::Input(_), true) if mock => format!("On the unit, apps would hear it as \u{201c}{label}\u{201d}."),
        (_, true) if mock => "Press ON to turn it off.".to_string(),
        // Only apps that play to its own speaker are shaped, and none moves
        // there by itself (critique #106 A P2-1: it read as automatic).
        (ChannelId::Output(_), true) => format!("It shapes apps that play to \u{201c}{label}\u{201d}: pick that as the speaker in Zoom, Meet or your player."),
        (ChannelId::Output(_), false) => format!("Turn it on, then pick \u{201c}{label}\u{201d} as the speaker in Zoom, Meet or your player, in place of the Babyface: only apps playing there are shaped."),
        // A mix looped back to the computer already carries the plain mic
        // (critique #105 A P2-1: this said to pick it for a stream too).
        (ChannelId::Input(_), _) if crate::app::plain_mic_looped(state, cid).is_some() => {
            format!("Pick \u{201c}{label}\u{201d} in a call app. Streaming the mix to your apps? It already has your plain mic: don't add this as well.")
        }
        (_, true) if state.dyn_heard == Some(false) => {
            format!("Pick \u{201c}{label}\u{201d} as the microphone in your call or streaming app.")
        }
        (_, true) => format!("Apps get it as \u{201c}{label}\u{201d}."),
        // Not "Turn Voice on" right above the key that says it (critique #37).
        (_, false) => format!("Once it's on, pick \u{201c}{label}\u{201d} as the mic in your app."),
    };
    // Read on the card's own ground, not the panel's (critique #32: 4.0:1
    // in Solarized).
    let card_ground = theme::blend(theme::surface(), theme::text_sec(), 0.06);
    let unheard = power && !mock && matches!(cid, ChannelId::Input(_)) && state.dyn_heard == Some(false);
    let first_ink = if off_armed || unheard {
        theme::readable_on(theme::attention(), theme::text_primary(), &[card_ground])
    } else if (elsewhere.is_some() || (power && (state.dyn_heard == Some(true) || matches!(cid, ChannelId::Output(_))))) && !mock {
        theme::readable_on(theme::signal(), theme::text_primary(), &[card_ground])
    } else {
        theme::text_primary()
    };
    let mut card = column![
        text(first).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).color(first_ink),
        text(second).size(theme::TEXT_SM * scale).color(theme::text_sec()),
    ]
    .spacing(theme::SPACE_HAIRLINE);
    // What it is doing, when there's more to say than on or off.
    let doing = status(state, cid);
    // (In the simulation with ON lit, the first line has said it already.)
    if (power || !doing.starts_with("Off")) && !(mock && power) {
        card = card.push(text(doing).size(theme::TEXT_SM * scale).color(theme::text_sec()));
    }
    // The computer wouldn't give Voice its audio priority (rtkit said no):
    // said, so dropouts aren't a mystery (the owner's choppy take,
    // 2026-11-13).
    if power && !mock && crate::priority::REFUSED.load(std::sync::atomic::Ordering::Relaxed) {
        card = card.push(
            text("Audra couldn't get audio priority from the system, so Voice may break up while the computer is busy.")
                .size(theme::TEXT_SM * scale)
                .color(theme::readable_on(theme::attention(), theme::text_primary(), &[card_ground])),
        );
    }
    // A mic's plain way in while Voice is off (critique #36: "Press ON"
    // pointed at the panel's smallest key, a 24 px circle), and its way out
    // while on (critique #37: only half the switch was plain).
    // An output's too (critique #95 A P1-1: its only way on was the 24 px
    // circle its card called "ON").
    if elsewhere.is_none() && !off_armed {
        let (words, msg) = match (cid, power) {
            (ChannelId::Input(_), true) => ("Turn off Voice", send(DynMsg::Power(false))),
            (ChannelId::Input(_), false) => ("Turn on Voice", send(DynMsg::Power(true))),
            (_, true) => ("Turn it off", send(DynMsg::Power(false))),
            (_, false) => ("Turn it on", send(DynMsg::Power(true))),
        };
        card = card.push(crate::keyfocus::reach_press(
            words,
            button(text(words).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                .style(theme::split_safe_key(theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale), false))
                .on_press(msg.clone())
                .into(),
            Some(msg),
            theme::RADIUS_SM * scale,
        ));
    }
    if let Some((r, problem)) = route(state, cid) {
        card = card.push(text(r).size(theme::TEXT_SM * scale).color(if problem {
            theme::readable_on(theme::attention(), theme::text_primary(), &[card_ground])
        } else {
            theme::text_sec()
        }));
    }
    // A call app just began hearing Voice: the card's ground glows green
    // and fades (`heard_flare`), the moment you're on.
    let flare = if state.animations && matches!(cid, ChannelId::Input(_)) { heard_flare(state) } else { 0.0 };
    let glow_ground = theme::blend(card_ground, theme::signal(), 0.22 * flare);
    let power_row = strip::hint_text(
        container(card)
            .padding([theme::SPACE_SM * scale, (theme::SPACE_SM + 2.0) * scale])
            .width(Length::Fill)
            .style(move |_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(glow_ground)),
                border: iced::Border { color: theme::border(), width: 1.0, radius: theme::RADIUS_SM.into() },
                ..iced::widget::container::Style::default()
            }),
        lead,
        scale,
    );

    // The presets are a stop of the panel's own, so the keyboard can pick
    // one (job 13): the left and right arrows step through them. Built
    // after the status card, so Tab meets the card's key (above) first
    // (critique #37: Tab went to the Sound box, then back up).
    let picker = crate::keyfocus::reach_pick(
        "Preset",
        hint(
            pick_list(names.clone(), now.clone(), move |pick: PresetName| send(pick_msg(pick)))
                // Your own settings, live, say so (critique #35: a blank
                // "Preset").
                .placeholder(if now.is_none() { "Your own settings" } else { "Preset" })
                .style(theme::capsule_pick_list(h / 2.0))
                .menu_style(theme::menu)
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_LG * scale])
                // A mic's sound, big in its Sound box (critique #35's "B").
                .text_size(if matches!(cid, ChannelId::Input(_)) { theme::TEXT_MD } else { theme::TEXT_SM } * scale)
                .width(Length::Fill),
            match cid {
                ChannelId::Output(_) => "Starting points: Volume protection never gets louder, Even out brings quiet parts up a little, Night holds loud scenes down. Your own settings stay at the top as Yours when a preset or Reset replaces them",
                _ => "Starting points: Clear Calls for every day, Streamer, Podcast, Singing, Radio Voice, and Drums for punch. Your own settings stay at the top as Yours when a preset, Fit or Reset replaces them",
            },
            scale,
        ),
        &names,
        now.as_ref(),
        move |pick: PresetName| send(pick_msg(pick)),
        theme::RADIUS_SM * scale,
    );

    // Simple on top, the TotalMix knobs under More (the owner's pick).
    // Preset on its own row, as the EQ's (a long name wrapped in the header).
    // A mic: what others hear first, then the Sound box (the preset, what
    // it does, Fit), big (the owner's pick after critique #35, "B": the
    // main path started at a thin line that looked like every other row).
    // An output keeps its short panel.
    let mut col = match cid {
        ChannelId::Input(_) => {
            let sound = container(
                column![
                    row![text("Sound").size(theme::TEXT_SM * scale).color(theme::text_sec()), picker]
                        .spacing(theme::SPACE_SM * scale)
                        .align_y(iced::Alignment::Center),
                    text(sound_line(now.as_ref().map(|n| n.name))).size(theme::TEXT_SM * scale).color(theme::text_sec()),
                    fit_key(state, cid, scale),
                ]
                .spacing(theme::SPACE_SM * scale),
            )
            .padding([theme::SPACE_SM * scale, (theme::SPACE_SM + 2.0) * scale])
            .width(Length::Fill)
            .style(|_t: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(theme::surface_hi())),
                border: iced::Border { color: theme::resting_key_edge(), width: 1.0, radius: theme::RADIUS_SM.into() },
                ..iced::widget::container::Style::default()
            });
            column![header, power_row, sound].spacing(gap)
        }
        _ => column![header, picker, power_row].spacing(gap),
    };
    if let Some(ask) = move_question(state, cid, scale) {
        col = col.push(ask);
    }
    col = col.push(simple_section(state, cid, &p, scale));
    // Built only while More is open: its knobs and keys register with
    // the keyboard as they're built, and closed they were twelve stops
    // Tab walked with nothing on screen (OpenCode's job 14). The curve
    // isn't worked out for nothing either.
    if state.dyn_more {
        // The display: the steady curve, the dot, the turn-down.
        let d = audra_fx::Dynamics::new(48_000.0, p);
        // With Auto Level on, nothing comes out over its ceiling: the curve
        // flattens there, as the sound does.
        let ceiling = p.auto_on.then_some(-p.auto_headroom_db);
        let curve: Vec<f32> = (0..=60).map(|i| d.static_db(-60.0 + i as f32)).map(|v| ceiling.map_or(v, |c| v.min(c))).collect();
        let dot = level_now(state, cid).filter(|l| *l > -59.5);
        let reduction = loop_of(state, cid).filter(|l| l.state() == DynState::Running).map_or(0.0, |l| l.meter_db());
        let side = 108.0 * scale;
        let plot = crate::widgets::dyn_plot::dyn_plot(crate::widgets::dyn_plot::DynPlot {
            curve,
            exp_threshold: p.gate_on.then_some(p.gate_threshold_db),
            comp_threshold: p.comp_on.then_some(p.threshold_db),
            dot,
            reduction_db: reduction,
            ceiling,
            side,
            scale,
        });
        let row_word = |name: &'static str, unit: &'static str| {
            container(
                column![
                    text(name).size(theme::TEXT_SM * scale).color(theme::text_sec()),
                    text(unit).size(theme::TEXT_SM * scale).color(theme::muted()),
                ]
                .spacing(0),
            )
            .width(Length::Fixed(56.0 * scale))
        };
        // The parts' hues readable on the panel itself (critique #30: tuned for
        // key wells, Nord's "Comp." lost its red); grey while that part is off.
        let red = if p.comp_on { theme::readable_on(COMP_RED, theme::text_primary(), &[theme::surface()]) } else { theme::muted() };
        let green = if p.gate_on { theme::readable_on(EXP_GREEN, theme::text_primary(), &[theme::surface()]) } else { theme::muted() };
        // The knobs' arcs in the same readable hues (critique #31: in Daylight
        // the raw green arc was 2.12 on the panel, 1.35 against its groove).
        let comp_tint = p.comp_on.then_some(red);
        let exp_tint = p.gate_on.then_some(green);
        let grid = column![
            plot,
            row![
                row_word("Thresh.", "starts at"),
                knob_of(DynParam::CompThreshold, Some(("Comp.", red)), comp_tint),
                knob_of(DynParam::ExpThreshold, Some(("Exp.", green)), exp_tint),
            ]
            .spacing(gap)
            .align_y(iced::Alignment::End),
            row![
                row_word("Ratio", "how firm"),
                knob_of(DynParam::CompRatio, None, comp_tint),
                knob_of(DynParam::ExpRatio, None, exp_tint),
            ]
            .spacing(gap)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(gap)
        .width(Length::Fill);
        let side_col = column![
            knob_of(DynParam::Gain, Some(("Gain", theme::text_sec())), None),
            knob_of(DynParam::Attack, Some(("Attack", theme::text_sec())), None),
            knob_of(DynParam::Release, Some(("Release", theme::text_sec())), None),
        ]
        .spacing(gap)
        .width(Length::Fixed(64.0 * scale));

        let comp_head = row![
            on_key("Compressor and expander on", dyn_on, send(DynMsg::Dynamics(!dyn_on)), "Compressor and expander on or off: evens out loud and quiet, quiets the room"),
            text("Compressor and expander").font(theme::SANS_SEMI).size(theme::TEXT_MD * scale),
        ]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center);
        let auto_head = row![
            on_key("Auto Level on", p.auto_on, send(DynMsg::AutoLevel(!p.auto_on)), "Auto Level on or off: holds the loudest parts under a ceiling and brings quiet parts up slowly. \"Keep me even\" above turns it on with gentle settings"),
            text("Auto Level").font(theme::SANS_SEMI).size(theme::TEXT_MD * scale),
        ]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center);
        let auto_row = row![
            knob_of(DynParam::AutoMaxGain, Some(("Max. Gain", theme::text_sec())), None),
            knob_of(DynParam::AutoHeadroom, Some(("Headroom", theme::text_sec())), None),
            knob_of(DynParam::AutoRise, Some(("Rise Time", theme::text_sec())), None),
        ]
        .spacing(gap);

        // The last few seconds of the level against the thresholds, moving as
        // you talk, with how far it turns down hanging from the top.
        let trace = strip::hint_text(
            crate::widgets::dyn_plot::dyn_trace(crate::widgets::dyn_plot::DynTrace {
                points: state.dyn_trace.get(&cid).map(|t| t.iter().copied().collect()).unwrap_or_default(),
                len: TRACE_LEN,
                exp_threshold: p.gate_on.then_some(p.gate_threshold_db),
                comp_threshold: p.comp_on.then_some(p.threshold_db),
                ceiling,
                height: 40.0 * scale,
                scale,
            }),
            "The last 4 seconds of this sound, newest at the right. The red line is the compressor's threshold: set it just under your loud parts. The green line is the expander's: set it just over the room's noise. Light blue from the top: how far it turns down".to_string(),
            scale,
        );
        // The trace's two lines, said under it (it was hover-only).
        let red_ink = theme::readable_on(COMP_RED, theme::text_primary(), &[theme::surface()]);
        let green_ink = theme::readable_on(EXP_GREEN, theme::text_primary(), &[theme::surface()]);
        let legend = row![
            text("Red: just under your loud parts").size(theme::TEXT_SM * scale).color(red_ink).wrapping(iced::advanced::text::Wrapping::None),
            text("Green: over room noise").size(theme::TEXT_SM * scale).color(green_ink).wrapping(iced::advanced::text::Wrapping::None),
        ]
        .spacing(theme::SPACE_MD * scale);
        col = col.push(comp_head).push(trace).push(legend).push(row![grid, side_col].spacing(gap)).push(auto_head).push(auto_row);
    }
    let more = state.dyn_more;
    col = col.push(crate::keyfocus::reach_press(
        "More",
        button(
            text(if more { "\u{25be} Less: hide the detailed knobs" } else { "\u{25b8} More: the detailed knobs, the curve and the trace" })
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec()),
        )
        .padding([theme::SPACE_TIGHT * scale, (theme::SPACE_SM + 2.0) * scale])
        .style(theme::ghost_button)
        .on_press(send(DynMsg::More(!more)))
        .into(),
        Some(send(DynMsg::More(!more))),
        theme::RADIUS_SM * scale,
    ));
    // As the EQ panel: its own ground (`panel_frame`).
    panel_frame(col, width, scale)
}

/// The D panel's simple section: plain choices that set the knobs below
/// (the owner's pick after ChatGPT's notes, 2026-11-10: "Simple on top,
/// More below"). On an input: a level coach, voice strength, background
/// noise, keep my voice even, Cough and Reset to Natural. On an output:
/// volume protection, maximum volume, even out, Reset to Natural.
/// A mic Voice doesn't run on: where it runs, and the two things you'd
/// want, instead of live settings that would change that mic's sound by
/// surprise (the owner's pick after critique #33, "B": the settings are
/// one set, shared, so a tweak here changed what others heard from the
/// other mic).
fn other_mic_card(state: &TuxMix, cid: ChannelId, there: usize, scale: f32) -> Element<'_, Message> {
    let here = crate::app::short_label(&crate::app::strip_display_name(state, cid)).to_string();
    let there_name = crate::app::short_label(&crate::app::strip_display_name(state, ChannelId::Input(there))).to_string();
    let key = |words: String, msg: Message, lit: bool| -> Element<'_, Message> {
        crate::keyfocus::reach_press(
            &words,
            button(text(words.clone()).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
                .style(theme::key_button(lit, 0.0, 0.0, theme::RADIUS_SM * scale))
                .on_press(msg.clone())
                .into(),
            Some(msg),
            theme::RADIUS_SM * scale,
        )
    };
    let card_ground = theme::blend(theme::surface(), theme::text_sec(), 0.06);
    container(
        column![
            text(format!("Voice runs on {there_name}")).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale),
            text(format!("One mic at a time, with one set of settings: change them in {there_name}'s panel, or move Voice here to {here}."))
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec()),
            row![
                key(format!("Open {there_name}'s Voice"), Message::ToggleFlyout(ChannelId::Input(there), FlyoutKind::Dynamics), false),
                key(format!("Move it to {here}"), Message::Dyn(cid, DynMsg::MoveHere), false),
            ]
            .spacing(theme::SPACE_SM * scale),
        ]
        .spacing(theme::SPACE_SM * scale),
    )
    .padding([theme::SPACE_SM * scale, (theme::SPACE_SM + 2.0) * scale])
    .width(Length::Fill)
    .style(move |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(card_ground)),
        border: iced::Border { color: theme::border(), width: 1.0, radius: theme::RADIUS_SM.into() },
        ..iced::widget::container::Style::default()
    })
    .into()
}

/// The panel's own ground around its column: scrolling inside when the
/// window is short, the bar beside the content and marked, with an id so
/// the keyboard can bring the ring into view (job 13).
fn panel_frame<'a>(col: iced::widget::Column<'a, Message>, width: f32, scale: f32) -> Element<'a, Message> {
    // Its foot fades while rows wait below (`app::with_more_below`).
    container(crate::app::with_more_below(col, Some(PANEL_SCROLL_ID), scale))
    .padding(theme::SPACE_MD * scale)
    .width(Length::Fixed(width))
    .height(Length::Fill)
    .style(theme::panel)
    .clip(true)
    .into()
}

/// "Fit to my voice": ten seconds of talking set the thresholds. In the
/// Sound box at the top of a mic's Voice panel (the owner's pick after
/// critique #35, "B": the sound you picked, what it does and Fit, big).
fn fit_key<'a>(state: &'a TuxMix, cid: ChannelId, scale: f32) -> Element<'a, Message> {
    let send = move |m: DynMsg| Message::Dyn(cid, m);
    let listening = state.dyn_fit.as_ref().filter(|f| f.cid == cid).map(|f| FIT_FOR.saturating_sub(f.started.elapsed()).as_secs() + 1);
    crate::keyfocus::reach_press(
        "Fit to my voice",
        hint(
            button(
                text(match listening {
                    Some(left) => format!("Listening\u{2026} talk normally ({left} s)"),
                    None => "Fit to my voice".to_string(),
                })
                .size(theme::TEXT_SM * scale)
                .center(),
            )
            .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
            .style(theme::split_safe_key(theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale), false))
            .on_press(send(DynMsg::Fit(listening.is_none()))),
            "Talk for ten seconds as you will on the call, with a pause or two: Audra sets where the compressor and the expander start, from your voice and your room. Press again to stop",
            scale,
        ),
        Some(send(DynMsg::Fit(listening.is_none()))),
        12.0 * scale,
    )
}

/// What a mic's sound does, in a line, for the Sound box: the preset's
/// own character, or your settings.
fn sound_line(name: Option<&str>) -> &'static str {
    match name {
        Some("Clear Calls") => "Clear and natural for calls: a light evening-out, and quieter between words.",
        Some("Streamer") => "Steady through laughs and shouts.",
        Some("Podcast") => "Polished and close.",
        Some("Singing") => "Lighter and slower, keeping your expression.",
        Some("Radio Voice") => "Fuller and more held, like broadcast.",
        Some("Drums") => "Punch: each hit's front comes through, the room stays out.",
        Some(YOURS) => "Your own settings, as you left them.",
        _ => "Your own settings: change them under Fine-tune or More.",
    }
}

/// The folded Fine-tune line's summary: what is on, then how many are off
/// ("Strength Gentle · Quiet between words Light · 4 more off").
fn fine_summary(state: &TuxMix, p: &DynamicsParams) -> String {
    let level = |n: u8| ["off", "Light", "Strong"][usize::from(n.min(2))];
    let strength = if !p.comp_on { None } else if p.ratio < 2.5 { Some("Gentle") } else if p.ratio < 3.75 { Some("Medium") } else { Some("Firm") };
    let quiet = if !p.gate_on { 0 } else if p.exp_ratio <= 2.0 { 1 } else { 2 };
    let even = p.auto_on && p.auto_max_gain_db > 0.5;
    let mut on: Vec<String> = Vec::new();
    let mut off = 0;
    match strength {
        Some(s) => on.push(format!("Voice strength {s}")),
        None => off += 1,
    }
    // The rows' own names (OpenCode's job 29: "Strength" and "Harsh S"
    // were short forms of what the rows say).
    for (name, n) in [("Noise removal", state.nr_level), ("Harsh S sounds", state.deess_level), ("Quiet between words", quiet)] {
        if n > 0 {
            on.push(format!("{name} {}", level(n)));
        } else {
            off += 1;
        }
    }
    if even { on.push("Keep me even".into()) } else { off += 1 }
    if state.tune_on { on.push("Auto-Tune".into()) } else { off += 1 }
    match (on.is_empty(), off) {
        (true, _) => "All off".into(),
        (false, 0) => on.join(" \u{b7} "),
        (false, n) => format!("{} \u{b7} {n}\u{a0}more\u{a0}off", on.join(" \u{b7} ")),
    }
}

fn simple_section<'a>(state: &'a TuxMix, cid: ChannelId, p: &DynamicsParams, scale: f32) -> Element<'a, Message> {
    let send = move |m: DynMsg| Message::Dyn(cid, m);
    let live = settings_live(state, cid);
    let label_w = Length::Fill;
    let label = |words: &'static str| container(text(words).size(theme::TEXT_SM * scale).color(theme::text_sec())).width(label_w);
    // A row of choices, the one in force lit, with air between the keys
    // and in them (the owner: "the buttons are too close together").
    let choices = |key: &'static str, names: &[&'static str], now: usize, pick: &dyn Fn(usize) -> Message| -> Element<'a, Message> {
        let mut r = row![].spacing(theme::SPACE_MD * scale);
        for (i, name) in names.iter().enumerate() {
            let msg = pick(i);
            r = r.push(
                container(crate::keyfocus::reach_press(
                    &format!("{key}: {name}"),
                    button(text(*name).size(theme::TEXT_SM * scale).width(Length::Fill).center())
                        .padding([theme::SPACE_MD * scale, theme::SPACE_SM * scale])
                        .width(Length::Fill)
                        .style({
                            // A chosen "Off" is quiet, not coral (critique
                            // #31, the owner's "B").
                            let lit = i == now;
                            let off = lit && *name == "Off";
                            // Chosen but not at work: "set, waiting"
                            // (critique #32, the owner's "B").
                            let waiting = lit && !off && !live;
                            move |t: &iced::Theme, st| {
                                if off {
                                    theme::chosen_off(t, st)
                                } else if waiting {
                                    (theme::key_button_armed(theme::accent(), theme::RADIUS_XS))(t, st)
                                } else {
                                    // A rim that reads on any pixel (critique
                                    // #95 B: an unlit cell's top read 1.88).
                                    (theme::split_safe_key(theme::toggle_button(lit, theme::accent()), lit))(t, st)
                                }
                            }
                        })
                        .on_press(msg.clone())
                        .into(),
                    Some(msg),
                    theme::RADIUS_SM * scale,
                ))
                .width(Length::Fill),
            );
        }
        r.into()
    };
    // Its name over its keys, so the keys have the panel's whole width
    // (four keys beside a name left "Medium" touching its edges).
    let line = |l: &'static str, el: Element<'a, Message>, tip: &'static str| -> Element<'a, Message> {
        hint(column![label(l), el].spacing(theme::SPACE_TIGHT * scale), tip, scale)
    };
    let reset_key = (matches!(cid, ChannelId::Output(_)), left_of(state, cid).unwrap_or(0));
    let reset_armed = state.dyn_reset_armed.is_some_and(|(k, at)| k == reset_key && at.elapsed() < RESET_ARM_FOR);
    // One width for both words, on whole pixels, so the second press's
    // wording doesn't reflow the panel or split the key's edge over two
    // pixel columns (critique #34).
    let reset_face = button(text(if reset_armed { "Reset? Press again" } else { "Reset to Natural" }).size(theme::TEXT_SM * scale).center())
        .padding([theme::SPACE_SM * scale, theme::SPACE_SM * scale])
        .width(Length::Fixed((132.0 * scale).round()))
        .on_press(send(DynMsg::Natural));
    // Built where it stands (the panel's foot), so the keyboard's stops
    // follow the order they are drawn in (job 13).
    let natural = || {
        crate::keyfocus::reach_press(
            "Reset to Natural",
            hint(
                if reset_armed {
                    // Breathing while it waits, as 48V? does (critique #34).
                    reset_face.style(theme::key_button_armed_breathing(theme::attention(), theme::RADIUS_SM * scale))
                } else {
                    reset_face.style(theme::split_safe_key(theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale), false))
                },
                "Everything off, noise removal, harsh S sounds and Auto-Tune too: your sound as it comes in. Press twice. Your settings stay under Preset as Yours; noise removal, harsh S and Auto-Tune you turn on again yourself",
                scale,
            ),
            Some(send(DynMsg::Natural)),
            12.0 * scale,
        )
    };
    // A group's small heading, over a hairline (the serif of the panel
    // titles, a size down).
    let heading = |words: &'static str| -> Element<'a, Message> {
        column![
            container(Space::new().height(Length::Fixed(1.0)))
                .width(Length::Fill)
                .style(|_t: &iced::Theme| iced::widget::container::Style {
                    background: Some(iced::Background::Color(theme::border())),
                    ..iced::widget::container::Style::default()
                }),
            text(words).font(theme::SERIF).size(theme::TEXT_MD * scale).color(theme::text_sec()),
        ]
        .spacing(theme::SPACE_TIGHT * scale)
        .into()
    };
    let mut col = column![].spacing(theme::SPACE_LG * scale);
    match cid {
        ChannelId::Input(i) => {
            // Three groups, each under a small heading (the owner's pick
            // after critique #31, "B": 27 controls in one column had no
            // place to start).
            col = col.push(heading("Your level"));
            // The level coach: the Quick setup's meter, in its words too
            // (critique #33: "Just right" here, "Good level" there).
            let (l, r) = crate::quick::source_meters(state, ChannelId::Input(i));
            let words = match crate::quick::level_of(crate::quick::held_peak(&l, &r)) {
                crate::quick::Level::Silent => "Speak normally: your peaks should land in the band",
                crate::quick::Level::Quiet => "Too quiet: turn the gain up (wrench, Gain)",
                crate::quick::Level::Good => "Good level: your peaks sit in the band",
                crate::quick::Level::Hot => "A little too loud: turn the gain down a few dB",
                crate::quick::Level::TooLoud => "Too loud, about to distort: turn the gain down",
            };
            col = col.push(hint(
                column![
                    crate::widgets::fader::target_meter(l, r, crate::quick::BAND, scale, 0.0),
                    text(words).size(theme::TEXT_SM * scale).color(theme::text_sec()),
                ]
                .spacing(theme::SPACE_HAIRLINE),
                "Your level coming in: too quiet, a good level (in the band), a little hot, or too loud. It's set with the gain, before anything here",
                scale,
            ));
            // Fit to my voice now stands in the Sound box at the top
            // (`fit_key`); its words and the mic check stay here, under
            // the level they come from.
            let listening = state.dyn_fit.as_ref().filter(|f| f.cid == cid).map(|f| FIT_FOR.saturating_sub(f.started.elapsed()).as_secs() + 1);
            // Off, nothing here is running: its last listen's words and
            // verdicts wait until it's on (critique #55: "Fitted to your
            // voice" and the mic check read as if Voice were running).
            let voice_on = power_of(state, cid);
            if let Some(said) = state.dyn_fit_said.as_ref().filter(|_| listening.is_none() && voice_on) {
                col = col.push(text(said.clone()).size(theme::TEXT_SM * scale).color(theme::text_sec()));
            }
            // The mic check from the same listen (ChatGPT's "Audra Mic
            // Check", the owner's pick): three verdicts, and a fix where one
            // helps.
            if let Some(fit) = state.dyn_fit_report.filter(|_| listening.is_none() && voice_on) {
                let mut card = column![text("Mic check").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale)].spacing(theme::SPACE_HAIRLINE);
                for (what, verdict, problem) in verdicts(&fit) {
                    card = card.push(
                        row![
                            container(text(what).size(theme::TEXT_SM * scale).color(theme::text_sec())).width(Length::Fixed(112.0 * scale)),
                            text(verdict).size(theme::TEXT_SM * scale).color(if problem {
                                theme::readable_on(theme::attention(), theme::text_primary(), &[theme::surface()])
                            } else {
                                theme::text_primary()
                            }),
                        ]
                        .spacing(theme::SPACE_SM * scale),
                    );
                }
                let level = noise_advice(&fit);
                if level > 0 && state.nr_level == 0 {
                    card = card.push(crate::keyfocus::reach_press(
                        "Turn on noise removal",
                        button(text(if level == 2 { "Turn on noise removal (Strong)" } else { "Turn on noise removal (Light)" }).size(theme::TEXT_SM * scale).center())
                            .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                            .style(theme::split_safe_key(theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale), false))
                            .on_press(send(DynMsg::Removal(level)))
                            .into(),
                        Some(send(DynMsg::Removal(level))),
                        12.0 * scale,
                    ));
                }
                col = col.push(card);
            }
            // Off: one line for what comes with it, not nine controls for a
            // feature that isn't running (critique #55).
            if !voice_on {
                col = col.push(
                    text("Once Voice is on: Fine-tune (strength, noise removal, Auto-Tune), how late others hear you, and Reset to Natural.")
                        .size(theme::TEXT_SM * scale)
                        .color(theme::text_sec()),
                );
            } else {
            // The five sound rows and Auto-Tune fold into one line that
            // says what's set (the owner's pick after critique #35, "B":
            // about 25 controls at once, and the panel scrolled at
            // 1440x900). Built only while open, so closed they're no
            // keyboard stops.
            let fine = state.dyn_fine_open;
            col = col.push(crate::keyfocus::reach_press(
                "Fine-tune",
                hint(
                    button(
                        // Open, the rows under it say it all: the summary
                        // goes (critique #37: it sat above what it sums up).
                        {
                            let head = column![text(if fine { "\u{25be} Fine-tune" } else { "\u{25b8} Fine-tune" }).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale)];
                            if fine { head } else { head.push(text(fine_summary(state, p)).size(theme::TEXT_SM * scale).color(theme::text_sec())) }
                        }
                        .spacing(theme::SPACE_HAIRLINE),
                    )
                    .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
                    .width(Length::Fill)
                    .style(theme::split_safe_key(theme::key_button(false, 0.0, 0.0, theme::RADIUS_SM * scale), false))
                    .on_press(send(DynMsg::FineTune(!fine))),
                    if fine { "Hide the fine-tune rows" } else { "Voice strength, noise removal, harsh S sounds, quiet between words, keep me even and Auto-Tune" },
                    scale,
                ),
                Some(send(DynMsg::FineTune(!fine))),
                theme::RADIUS_SM * scale,
            ));
            if fine {
                let strength = if !p.comp_on { 0 } else if p.ratio < 2.5 { 1 } else if p.ratio < 3.75 { 2 } else { 3 };
                col = col.push(line(
                    "Voice strength",
                    choices("Voice strength", &["Off", "Gentle", "Medium", "Firm"], strength, &|k| send(DynMsg::Strength(k as u8))),
                    "How much loud and quiet parts are evened out: gentle sounds natural, firm sounds broadcast",
                ));
                // Noise removal (RNNoise): fans and hum under the voice.
                let at_48k = (state.device.settings().sample_rate as f32 - audra_fx::denoise::RATE).abs() < 1.0;
                col = col.push(line(
                    "Noise removal",
                    choices("Noise removal", &["Off", "Light", "Strong"], usize::from(state.nr_level.min(2)), &|k| send(DynMsg::Removal(k as u8))),
                    if at_48k {
                        "Turns steady background noise down under your voice: fans, air conditioning, hum, hiss (Light up to 8 dB, Strong up to 18). In a quiet room leave it off: the mic check says when it helps. Adds about 20 ms for others"
                    } else {
                        "Works at 48 kHz only: set the unit to 48 kHz (the device card) to use it"
                    },
                ));
                // Chosen but unable to run: said on screen, not only on hover
                // (OpenCode's job 3: the keys showed it on, the room stayed).
                if !at_48k && state.nr_level > 0 {
                    col = col.push(
                        text(format!(
                            "Noise removal works at 48 kHz only; the unit runs at {:.1} kHz, so it isn't removing anything.",
                            state.device.settings().sample_rate as f32 / 1000.0
                        ))
                        .size(theme::TEXT_SM * scale)
                        .color(theme::readable_on(theme::attention(), theme::text_primary(), &[theme::surface()])),
                    );
                }
                col = col.push(line(
                    "Harsh S sounds",
                    choices("Harsh S sounds", &["Off", "Light", "Strong"], usize::from(state.deess_level.min(2)), &|k| send(DynMsg::DeEss(k as u8))),
                    "Tames sharp S and SH sounds (a de-esser): only the very top of your voice is turned down, only while an S is there (Light up to 3 dB, Strong up to 6). No delay",
                ));
                let noise = if !p.gate_on { 0 } else if p.exp_ratio <= 2.0 { 1 } else { 2 };
                col = col.push(line(
                    "Quiet between words",
                    choices("Quiet between words", &["Off", "Light", "Strong"], noise, &|k| send(DynMsg::Noise(k as u8))),
                    "Turns the sound down in the gaps between your words (the room, a keyboard): the expander",
                ));
                let even = p.auto_on && p.auto_max_gain_db > 0.5;
                col = col.push(line(
                    "Keep me even",
                    choices("Keep me even", &["Off", "On"], usize::from(even), &|k| send(DynMsg::Even(k == 1))),
                    "Brings quiet parts up slowly and holds loud ones under a ceiling, so you stay at one level. It's TotalMix's Auto Level, whose knobs are under More",
                ));
                // Auto-Tune with what else shapes your voice (critique #30:
                // "Move it to D").
                col = col.push(crate::live_spectrum::autotune_switch(state, cid, 0.0, scale));
            }
            col = col.push(heading("During the call"));
            // Cough lives in Control Room, next to Talkback, and on the C
            // key (the owner's pick after critique #32, "B": at the panel's
            // foot it was out of sight on smaller screens, and gone with
            // the panel closed). Here, a pointer to it.
            // Said as it is: Cough is greyed until Voice runs (critique #35).
            let cough = if power_of(state, cid) {
                "Cough: hold C, or its key in Control Room, next to Talkback"
            } else {
                "Cough, once Voice is on: hold C on your keyboard, or Cough in Control Room"
            };
            col = col.push(text(cough).size(theme::TEXT_SM * scale).color(theme::text_sec()));
            // The delay others hear, with how it'd be noticed (the owner,
            // 2026-11-10: "let the user know that there is a delay… and how
            // bad it is, and how to have control over it"): people notice
            // a voice trailing the picture past about 125 ms and leading it
            // past 45 (ITU); webcams often trail the sound by 50–150 ms.
            let running = state.dyn_loop.as_ref().filter(|l| l.state() == DynState::Running && power_of(state, cid));
            // Not running: what it will be once on, estimated from the
            // parts switched on (measured: about 40 ms with the floor,
            // noise removal 20 more, Auto-Tune about 17), and said so.
            let estimate = 40.0 + if state.nr_level > 0 && !state.low_delay { 20.0 } else { 0.0 } + if state.tune_on && !state.low_delay { 17.0 } else { 0.0 };
            let ms = running.map_or(estimate, |l| l.delay_ms());
            // Audra's voice only ever trails the picture: noticed past about
            // 125 ms; amber from 80, before that.
            let (judge, color) = if ms < 80.0 {
                ("fine", theme::text_sec())
            } else if ms < 125.0 {
                ("may show on video", theme::attention())
            } else {
                ("trails the video", theme::clip())
            };
            // One short line, always (the suggestion lights the Lower delay
            // row instead of growing the line).
            let words = if running.is_some() {
                format!("Others hear you about {ms:.0} ms late \u{b7} {judge}")
            } else {
                // On but not running yet (starting, or the simulation):
                // not "once on" under a lit ON (critique #35).
                if power_of(state, cid) {
                    format!("Others will hear you about {ms:.0} ms late")
                } else {
                    format!("Once on, others hear you about {ms:.0} ms late")
                }
            };
            let suggest = running.is_some() && ms >= 80.0 && !state.low_delay;
            col = col.push(hint(
                text(crate::app::fit_label(&words, 290.0 * scale, scale))
                    .size(theme::TEXT_SM * scale)
                    .color(theme::readable_on(color, theme::text_primary(), &[theme::surface()]))
                    .wrapping(iced::advanced::text::Wrapping::None),
                "How much later others hear you than you hear yourself. People notice a voice trailing the picture past about 125 ms, and webcams are often 50 to 150 ms behind the sound already, so this rarely matters. If your voice ever trails your video in OBS: add a Render Delay filter to the camera (or a negative Sync Offset on the mic, if your OBS allows it). If it leads: a positive Sync Offset on the mic",
                scale,
            ));
            // Lower delay: pauses only the extras that cost time; the
            // dynamics and the limit (no delay, and your voice's
            // protection) stay (the owner: "clipping my voice should not be
            // considered… That one should be still there").
            let paused: Vec<&str> = [(state.nr_level > 0, "noise removal"), (state.tune_on, "Auto-Tune")].iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
            // A switch, as every yes/no choice in Audra is (critique #52:
            // on and off were drawn five ways); its "try it" a line under it.
            let toggle = send(DynMsg::LowDelay(!state.low_delay));
            let row = if suggest {
                crate::app::switch_row_note("Lower delay", "Try it: others hear you late", state.low_delay, Some(toggle), scale)
            } else {
                crate::app::switch_row("Lower delay", state.low_delay, Some(toggle), scale)
            };
            col = col.push(hint(
                row,
                "Brings the delay down when it's too high: pauses the extras that cost time (noise removal about 20 ms, Auto-Tune about 17) and keeps Audra's queue short. Your voice strength, quiet between words, harsh S sounds and the safety limit stay on: they add no delay",
                scale,
            ));
            if state.low_delay && !paused.is_empty() {
                col = col.push(text(format!("Paused for lower delay: {}.", paused.join(" and "))).size(theme::TEXT_SM * scale).color(theme::text_sec()));
            }
            // Reset at the panel's foot, as on an output, apart from the
            // controls used mid-call (critique #34: it sat beside Cough).
            col = col.push(natural());
            }
        }
        _ => {
            // Its meaning on the page, not only on hover (critique #53).
            col = col.push(hint(
                // While the panel is off the switch shows what is set, in
                // the quiet look of a control that isn't working (the same
                // finding): lit red under "Off", it said the ears were
                // covered.
                if power_of(state, cid) {
                    crate::app::switch_row_note("Volume protection", "Stops sudden loud sounds going past the maximum below", p.auto_on, Some(send(DynMsg::Protect(!p.auto_on))), scale)
                } else {
                    // What it is and where it stands, both (critique #96 A
                    // P3-1, P3-3: the knob sat at "on" over "Not working",
                    // and what the switch does was hover-only).
                    crate::app::switch_row_note(
                        "Volume protection",
                        if p.auto_on { "Stops sudden loud sounds. Set to on, and working once this panel is turned on (above)" } else { "Stops sudden loud sounds. Set to off; nothing here works until this panel is turned on (above)" },
                        p.auto_on,
                        None,
                        scale,
                    )
                },
                "Stops sudden loud sounds (an ad, a notification) from going past the maximum below",
                scale,
            ));
            let max_now = if p.auto_headroom_db < 4.0 { 0 } else if p.auto_headroom_db < 8.0 { 1 } else { 2 };
            // Outlined a moment after Help's "Keep headphones gentle" lands
            // here (critique #106 A P3-17), with the same padding either way.
            let lit = if matches!(cid, ChannelId::Output(_)) { crate::motion::phones_cap_lit(state) } else { 0.0 };
            col = col.push(
                iced::widget::container(line(
                    "Maximum volume",
                    choices("Maximum volume", &["\u{2212}2 dB", "\u{2212}6 dB", "\u{2212}10 dB"], max_now, &|k| send(DynMsg::MaxVolume([2.0, 6.0, 10.0][k]))),
                    "How far under full scale the loudest sound may go (lower: quieter peaks)",
                ))
                // Little height: at SPACE_SM all round it pushed Even out out
                // of view (critique #107 B).
                // Room under the keys (critique #108 B: they touched the outline).
                .padding(iced::Padding { top: theme::SPACE_TIGHT * scale, bottom: theme::SPACE_SM * scale, left: theme::SPACE_SM * scale, right: theme::SPACE_SM * scale })
                .style(move |_t: &iced::Theme| iced::widget::container::Style {
                    border: iced::Border { color: iced::Color { a: lit, ..theme::readable_on_floors(theme::lamp()) }, width: 2.0, radius: theme::RADIUS_MD.into() },
                    ..iced::widget::container::Style::default()
                }),
            );
            let even_now = if p.comp_on { 2 } else if p.auto_on && p.auto_max_gain_db > 0.5 { 1 } else { 0 };
            col = col.push(line(
                "Even out",
                choices("Even out", &["Off", "Light", "Strong"], even_now, &|k| send(DynMsg::EvenOut(k as u8))),
                "Brings quiet parts up and holds loud ones down, so videos and calls play at one level",
            ));
            col = col.push(natural());
        }
    }
    col.into()
}

/// `power_switch` with `pad_x` of room at its sides (0: its words and its
/// switch on the column's own edges, as the EQ panel's grid needs).
#[allow(clippy::too_many_arguments)]
pub(crate) fn power_switch_padded(key: &str, label: String, note: String, route: Option<(String, bool)>, on: bool, message: Message, pad_x: f32, scale: f32) -> Element<'static, Message> {
    let (w, h, k) = (30.0 * scale, 16.0 * scale, 12.0 * scale);
    let knob = container(Space::new().width(k).height(k)).style(theme::switch_knob(on, false, k / 2.0));
    let track = container(knob)
        .width(w)
        .height(h)
        .padding(iced::Padding::from([0.0, (h - k) / 2.0]))
        .align_x(if on { iced::Alignment::End } else { iced::Alignment::Start })
        .align_y(iced::Alignment::Center)
        .style(theme::switch_track(on, false, h / 2.0));
    // Its name in the bold of a heading: where it acts is the panel's first
    // fact (the second opinion: it blended in).
    let mut words = column![
        text(label).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale),
        text(note).size(theme::TEXT_SM * scale).color(theme::muted()),
    ]
    .spacing(theme::SPACE_HAIRLINE);
    if let Some((r, problem)) = route {
        words = words.push(text(r).size(theme::TEXT_SM * scale).color(if problem { theme::attention() } else { theme::muted() }));
    }
    let row = button(row![container(words).width(Length::Fill), track].spacing(theme::SPACE_MD * scale).align_y(iced::Alignment::Center))
        .padding([theme::SPACE_SM * scale, pad_x])
        .width(Length::Fill)
        .style(theme::list_row(true, None))
        .on_press(message.clone())
        .into();
    crate::keyfocus::reach_press(key, row, Some(message), theme::RADIUS_SM * scale)
}

/// The panel's width: whole strips, as wide as the EQ's needs.
pub fn panel_w(cid: ChannelId, scale: f32) -> f32 {
    let one = strip::full_width(cid) * scale + strip::side_band_extra(scale);
    let need = strip::EQ_FLYOUT_W * scale;
    let k = ((need + theme::SPACE_MD) / (one + theme::SPACE_MD)).ceil().max(1.0);
    k * one + (k - 1.0) * theme::SPACE_MD
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_glow_wants_a_real_beginning_and_a_rest() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        // A known "no" turning "yes": an app has begun.
        assert!(super::glow_begins(Some(false), Some(true), None, now));
        // The first answer, or an app already listening at start-up: no glow.
        assert!(!super::glow_begins(None, Some(true), None, now));
        // Still yes, or turning no: nothing new.
        assert!(!super::glow_begins(Some(true), Some(true), None, now));
        assert!(!super::glow_begins(Some(false), Some(false), None, now));
        // An app flicking the mic: no second glow inside the rest...
        assert!(!super::glow_begins(Some(false), Some(true), Some(now), now + Duration::from_secs(4)));
        // ...and one again once it has passed.
        assert!(super::glow_begins(Some(false), Some(true), Some(now), now + super::HEARD_REST));
    }

    use super::*;

    /// Readings like ten seconds of talking: a room at `room` dB a third of
    /// the time, words from −34 to −16 dB the rest.
    fn talk(room: f32) -> Vec<(f32, f32)> {
        (0..300)
            .map(|i| {
                if i % 3 == 0 {
                    (room + (i % 5) as f32 * 0.5, room + 6.0)
                } else {
                    let r = -34.0 + (i % 19) as f32;
                    (r, r + 10.0)
                }
            })
            .collect()
    }

    #[test]
    fn a_fit_sets_the_compressor_under_the_loud_words_and_the_expander_over_the_room() {
        let fit = fit_from(&talk(-56.0)).expect("a fit");
        assert!((fit.room_db + 55.5).abs() < 1.5, "room {}", fit.room_db);
        assert!(fit.loud_db > -24.0 && fit.loud_db < -18.0, "loud {}", fit.loud_db);
        assert!((fit.comp_threshold_db - (fit.loud_db - 4.5)).abs() < 1e-3);
        assert!(fit.exp_threshold_db <= fit.room_db + 8.0 + 1e-3, "over the room by 8 at most");
        assert!(fit.exp_threshold_db <= -34.0 - 3.0 + 1e-3, "under the quietest words");
    }

    #[test]
    fn a_noisy_room_keeps_the_expander_under_the_quiet_words() {
        // A room at −46: room + 8 = −38, but the quietest words (−34) less
        // 3 is −37; the lower wins, so the words stay.
        let fit = fit_from(&talk(-46.0)).expect("a fit");
        assert!(fit.exp_threshold_db <= -34.0 - 3.0 + 1e-3, "{}", fit.exp_threshold_db);
        assert!(fit.exp_threshold_db > fit.room_db, "still over the room");
    }

    #[test]
    fn the_mic_check_says_what_it_heard() {
        let fit = fit_from(&talk(-56.0)).expect("a fit");
        let v = verdicts(&fit);
        assert_eq!(v[0].1, "Good", "speech around −25 with peaks under −3: {fit:?}");
        assert_eq!(v[1].1, "Excellent");
        assert_eq!(v[2].1, "None");
        // A loud room and clipped peaks.
        let mut hot = talk(-44.0);
        for r in hot.iter_mut().step_by(7) {
            r.1 = 0.0;
        }
        let v = verdicts(&fit_from(&hot).expect("a fit"));
        assert!(v[0].2 && v[1].2 && v[2].2, "{v:?}");
    }

    #[test]
    fn noise_removal_is_offered_by_the_rooms_gap_under_the_voice() {
        // The same room, −46: under a quiet voice (about −25) it's 21 dB
        // under, worth Light; under a voice 8 dB louder it's 29 dB under,
        // where noise removal would change the voice more than the room.
        let fit = fit_from(&talk(-46.0)).expect("a fit");
        assert_eq!(noise_advice(&fit), 1, "{fit:?}");
        let loud: Vec<(f32, f32)> = talk(-46.0).into_iter().map(|(r, p)| if r > -40.0 { (r + 8.0, p + 8.0) } else { (r, p) }).collect();
        let fit = fit_from(&loud).expect("a fit");
        assert_eq!(noise_advice(&fit), 0, "{fit:?}");
        assert!(!verdicts(&fit)[1].2);
        // Close under the voice: Strong.
        let fit = fit_from(&talk(-36.0)).expect("a fit");
        assert_eq!(noise_advice(&fit), 2, "{fit:?}");
    }

    #[test]
    fn a_fit_says_why_not() {
        assert!(fit_from(&talk(-56.0)[..10]).is_err(), "too short");
        let room_only: Vec<(f32, f32)> = (0..300).map(|i| (-55.0 + (i % 3) as f32, -50.0)).collect();
        assert!(fit_from(&room_only).is_err(), "no talking");
        let talk_only: Vec<(f32, f32)> = (0..300).map(|i| (-30.0 + (i % 12) as f32, -18.0)).collect();
        assert!(fit_from(&talk_only).unwrap_err().contains("pause") || fit_from(&talk_only).is_err(), "no pauses");
    }

    // ── The panel's keyboard (job 13) ─────────────────────────────

    use crate::keyfocus::{first_id, KeyFocus, Region};

    /// A mock state with the D panel of `cid` open and drawn, so the
    /// keyboard's stops are the panel's.
    fn with_the_panel(cid: ChannelId) -> TuxMix {
        with_the_panel_fine(cid, false)
    }

    /// The panel drawn with its Fine-tune rows open (`fine`) or folded.
    fn with_the_panel_fine(cid: ChannelId, fine: bool) -> TuxMix {
        let mut state = crate::app::new(true, None, None);
        state.dyn_fine_open = fine;
        crate::app::open_flyout(&mut state, cid, FlyoutKind::Dynamics);
        crate::keyfocus::begin_frame(&state);
        let _ = popover(&state, cid, 320.0, 1.0);
        state
    }

    /// The same with Voice on for `cid`: Fine-tune, the call's rows and
    /// Reset show only then (critique #55).
    fn with_voice_on(cid: ChannelId, fine: bool) -> TuxMix {
        let mut state = with_the_panel_fine(cid, fine);
        let _ = crate::app::update(&mut state, Message::Dyn(cid, DynMsg::Power(true)));
        state.dyn_fine_open = fine;
        crate::keyfocus::begin_frame(&state);
        let _ = popover(&state, cid, 320.0, 1.0);
        state
    }

    #[test]
    fn voice_off_shows_one_line_for_what_comes_with_it() {
        let cid = ChannelId::Input(0);
        let _ = with_the_panel_fine(cid, false);
        let off = stops_of(cid);
        assert!(off.iter().any(|s| s == "Turn on Voice"));
        for name in ["Fine-tune", "Reset to Natural", "Lower delay"] {
            assert!(!off.iter().any(|s| s == name), "{name} waits for Voice: {off:?}");
        }
        let _ = with_voice_on(cid, false);
        let on = stops_of(cid);
        assert!(on.iter().any(|s| s == "Fine-tune") && on.iter().any(|s| s == "Reset to Natural"), "{on:?}");
    }

    #[test]
    fn another_mics_panel_shows_where_voice_runs_not_its_settings() {
        // The owner's pick after critique #33 ("B"): a tweak on AN2's live
        // settings changed what others heard from AN1.
        let an2 = ChannelId::Input(1);
        let mut state = crate::app::new(true, None, None);
        state.dyn_input = Some(0);
        crate::app::open_flyout(&mut state, an2, FlyoutKind::Dynamics);
        crate::keyfocus::begin_frame(&state);
        let _ = popover(&state, an2, 320.0, 1.0);
        let stops = stops_of(an2);
        assert!(stops.iter().any(|s| s == "Open AN1's Voice") && stops.iter().any(|s| s == "Move it to AN2"), "{stops:?}");
        assert!(!stops.iter().any(|s| s == "Preset" || s == "Fine-tune" || s == "Fit to my voice"), "no settings: {stops:?}");
        let _ = crate::app::update(&mut state, Message::Dyn(an2, DynMsg::MoveHere));
        assert_eq!(state.dyn_input, Some(1), "moved");
    }

    #[test]
    fn voice_off_has_a_plain_way_in() {
        // Critique #36: "Press ON" pointed at a 24 px circle.
        let cid = ChannelId::Input(0);
        let mut state = with_the_panel(cid);
        assert!(stops_of(cid).iter().any(|s| s == "Turn on Voice"), "off: the key is there");
        let _ = crate::app::update(&mut state, Message::Dyn(cid, DynMsg::Power(true)));
        crate::keyfocus::begin_frame(&state);
        let _ = popover(&state, cid, 320.0, 1.0);
        assert!(power_of(&state, cid));
        assert!(!stops_of(cid).iter().any(|s| s == "Turn on Voice"), "on: it goes");
    }

    #[test]
    fn the_fine_tune_rows_are_stops_only_while_open() {
        // The owner's pick after critique #35 ("B"): the five rows and
        // Auto-Tune fold into one line; folded, they're no stops.
        let cid = ChannelId::Input(0);
        let _ = with_voice_on(cid, false);
        let folded = stops_of(cid);
        assert!(folded.iter().any(|s| s == "Fine-tune"), "the fold is a stop: {folded:?}");
        assert!(!folded.iter().any(|s| s.starts_with("Voice strength")), "folded rows are no stops: {folded:?}");
        let _ = with_voice_on(cid, true);
        let open = stops_of(cid);
        let at = |name: &str| open.iter().position(|s| s == name).unwrap_or_else(|| panic!("{name}: {open:?}"));
        assert!(at("Fine-tune") < at("Voice strength: Medium"), "the rows open under it");
        assert!(at("Fit to my voice") < at("Fine-tune"), "Fit stands in the Sound box above");
    }

    #[test]
    fn the_offset_puts_the_first_a_middle_and_the_last_stop_where_they_belong() {
        // Job 13: the panel's scroll follows the keyboard's stop: the first
        // at the top, the last at the foot, one between halfway.
        assert_eq!(scroll_offset(0, 7), 0.0);
        assert_eq!(scroll_offset(3, 7), 0.5);
        assert_eq!(scroll_offset(6, 7), 1.0);
        // One stop alone: nowhere to scroll.
        assert_eq!(scroll_offset(0, 1), 0.0);
    }

    #[test]
    fn the_preset_list_is_among_the_panels_stops() {
        // Job 13: the presets were the one control the keyboard could not
        // reach (the list never registered).
        let cid = ChannelId::Input(0);
        let mut state = with_the_panel(cid);
        let stops = stops_of(cid);
        assert!(stops.iter().any(|s| s == "Preset"), "the preset list is a stop: {stops:?}");
        // And the keyboard can pick one: the right arrow steps to the next.
        state.key_focus = Some(KeyFocus::Item(first_id(Region::Dyn(cid), "Preset")));
        let before = state.dyn_params;
        let _ = crate::app::update(&mut state, Message::KeyAdjust(1));
        assert_ne!(state.dyn_params, before, "the arrow chose a preset");
    }

    #[test]
    fn a_keyboard_press_that_raises_the_question_lands_on_the_safe_answer() {
        // Job 13: the card's safe answer, so a second Space changes
        // nothing. A mouse press moves no focus.
        let an2 = ChannelId::Input(1);
        let mut state = with_voice_on(an2, true);
        // The shaped voice runs on AN1; a pick on AN2 asks before moving it.
        state.dyn_input = Some(0);
        state.key_focus = Some(KeyFocus::Item(first_id(Region::Dyn(an2), "Voice strength: Medium")));
        let _ = crate::app::update(&mut state, Message::KeyLetter(' '));
        assert_eq!(state.dyn_move_ask, Some(1), "the question stands");
        assert_eq!(
            state.key_focus,
            Some(KeyFocus::Item(first_id(Region::Dyn(an2), "Keep it on AN1"))),
            "the keyboard lands on the safe answer"
        );
        // From the mouse: the question stands, the keyboard stays where it
        // was (a click hands over to the mouse).
        let mut state = with_voice_on(an2, true);
        state.dyn_input = Some(0);
        state.key_focus = Some(KeyFocus::Item(first_id(Region::Dyn(an2), "Voice strength: Medium")));
        let _ = crate::app::update(&mut state, Message::PointerPressed);
        let _ = crate::app::update(&mut state, Message::Dyn(an2, DynMsg::Strength(2)));
        assert_eq!(state.dyn_move_ask, Some(1), "the question stands");
        assert_eq!(state.key_focus, None, "the mouse moves no focus");
    }

    #[test]
    fn the_panels_stops_are_listed_in_the_order_they_are_drawn() {
        // Job 13: the scroll follows the stop's place among the panel's
        // stops, so the list has to be the order they stand in.
        let cid = ChannelId::Input(0);
        let _ = with_voice_on(cid, true);
        let stops = stops_of(cid);
        assert!(stops.len() > 20, "the panel's stops: {stops:?}");
        let at = |name: &str| stops.iter().position(|s| s == name).unwrap_or_else(|| panic!("{name} is a stop: {stops:?}"));
        assert_eq!(at("Power"), 0, "the power is the panel's first stop");
        assert!(at("Preset") < at("Voice strength: Medium"), "the presets stand above the picks");
        assert!(at("Reset to Natural") > at("Voice strength: Medium"), "Reset stands at the foot");
    }

    #[test]
    fn the_ring_finds_its_stop_among_the_panels_stops() {
        // Job 13: the scroll follows the stop the keyboard is on, first
        // and last alike.
        let cid = ChannelId::Input(0);
        let _ = with_the_panel(cid);
        let stops = stops_of(cid);
        let first = first_id(Region::Dyn(cid), &stops[0]);
        let last = first_id(Region::Dyn(cid), &stops[stops.len() - 1]);
        assert_eq!(stop_at(cid, first), Some(0), "the first stop");
        assert_eq!(stop_at(cid, last), Some(stops.len() - 1), "the last stop");
        // A stop of another panel: none of this one's.
        assert_eq!(stop_at(cid, 0), None);
    }

    #[test]
    fn the_more_knobs_are_stops_only_while_more_is_open() {
        // OpenCode's job 14: closed, the twelve knobs and keys under More
        // still registered, and Tab walked them with nothing on screen.
        let cid = ChannelId::Input(0);
        let mut state = with_voice_on(cid, false);
        state.dyn_more = false;
        crate::keyfocus::begin_frame(&state);
        let _ = popover(&state, cid, 320.0, 1.0);
        let closed = stops_of(cid);
        for name in ["Compressor and expander on", "Auto Level on"] {
            assert!(!closed.iter().any(|s| s == name), "{name} hidden under More is no stop: {closed:?}");
        }
        state.dyn_more = true;
        crate::keyfocus::begin_frame(&state);
        let _ = popover(&state, cid, 320.0, 1.0);
        let open = stops_of(cid);
        let at = |name: &str| open.iter().position(|s| s == name).unwrap_or_else(|| panic!("{name} is a stop: {open:?}"));
        assert!(at("Auto Level on") > at("Reset to Natural") && at("Auto Level on") < at("More"), "in screen order");
        assert_eq!(open.len(), closed.len() + 12, "the twelve under More: {open:?}");
    }
}
