//! Quick: a guided setup (2026-11-05, the critique: Quick was "the least
//! authored screen", two big strips in empty dark; the owner chose the
//! guided setup from three structures, `.impeccable/surfaces/crates-app.md`).
//!
//! Three numbered steps, each answering one question a podcaster or a
//! streamer has, each ticking off from the live state, and each still the
//! working control:
//!
//! 1. **Your mic:** which source, its mic preset, and 48V (two clicks, as
//!    everywhere).
//! 2. **Its level:** a large meter with the band a voice should peak in,
//!    the gain beside it, and a line that says what to do (too quiet, good,
//!    a little hot, too loud).
//! 3. **Hear it:** the mix you listen on and the send into it, with what
//!    stands in the way said (muted, the mix turned down, nothing sent).
//!
//! The counter stays at the right: the Main and Phones levels and the
//! Control Room keys are where they always are.

use iced::widget::{button, column, container, pick_list, row, rule, scrollable, text, Space};
use iced::{Alignment, Color, Element, Length, Task};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::theme;
use crate::widgets::fader::{self, MeterFrame};

/// The band a voice should peak in, in dBFS: room above for a laugh, far
/// enough above the noise below.
pub const BAND: (f32, f32) = (-18.0, -6.0);

/// The Quick setup's scroll, so the keyboard can bring the ring into view
/// (job 14): a step below the fold got its ring unseen.
pub const SCROLL_ID: &str = "quick-setup-scroll";

/// Where the setup scrolls to for the stop at `k` of `n`: the first at the
/// top, the last at the foot (the rule the D panel's own scroll uses).
pub fn scroll_offset(k: usize, n: usize) -> f32 {
    if n > 1 { k as f32 / (n - 1) as f32 } else { 0.0 }
}

/// Scrolls the Quick setup to where the keyboard is, so the ring is in
/// view. The keyboard's own step handlers call this with the setup's
/// scroll (job 14; Claude wires it into `app.rs`).
pub fn scroll_to_focus(state: &TuxMix) -> Task<Message> {
    let Some(crate::keyfocus::KeyFocus::Item(id)) = state.key_focus else { return Task::none() };
    let stops = crate::keyfocus::drawn_in(crate::keyfocus::Region::Quick);
    let Some(k) = stops.iter().position(|r| r.id == id) else { return Task::none() };
    iced::widget::operation::snap_to(SCROLL_ID, scrollable::RelativeOffset { x: 0.0, y: scroll_offset(k, stops.len()) })
}

/// Where a step stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Done: its badge is a tick.
    Done,
    /// Done in the simulation, where nothing is really shaped (step 4): the
    /// tick, as the Voice panel's ON is lit, but its words in plain ink,
    /// never the green of a job done (critique #37: Quick said a green
    /// "On." where the panel said nothing is shaped).
    DoneSimulated,
    /// Still to do, or waiting (no signal yet).
    Open,
    /// Something wants changing now (too loud).
    Warn,
}

/// What the level step reads from the held peak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Silent,
    Quiet,
    Good,
    Hot,
    TooLoud,
}

/// The level step's reading of a held peak (`peak`, linear).
pub fn level_of(peak: f32) -> Level {
    let db = 20.0 * peak.max(1e-6).log10();
    if db < -50.0 {
        Level::Silent
    } else if db < BAND.0 {
        Level::Quiet
    } else if db <= BAND.1 {
        Level::Good
    } else if db < fader::TM_HOT_DB {
        Level::Hot
    } else {
        Level::TooLoud
    }
}

/// The source's meters: its own, and its pair's other channel where the
/// pair is one stereo strip.
pub(crate) fn source_meters(state: &TuxMix, cid: ChannelId) -> (MeterFrame, Option<MeterFrame>) {
    let (meters, i, linked) = match cid {
        ChannelId::Input(i) => (&state.input_meters, i, state.device.input_pair_linked(i / 2)),
        ChannelId::Playback(i) => (&state.playback_meters, i, state.device.playback_linked(i / 2)),
        ChannelId::Output(i) => (&state.output_meters, i, state.device.output_linked(i / 2)),
    };
    let l = if linked { i & !1 } else { i };
    let left = meters.get(l).map_or(MeterFrame::still(0.0), |m| m.frame());
    let right = linked.then(|| meters.get(l + 1).map(|m| m.frame())).flatten();
    (left, right)
}

/// The loudest held peak of the source's meters (its value where none is
/// held).
pub(crate) fn held_peak(left: &MeterFrame, right: &Option<MeterFrame>) -> f32 {
    // The held peak for at most `WORDS_HOLD`, whatever Peak Hold is set to,
    // then the bar: steady between syllables, never "a little hot" for ten
    // seconds after the voice went quiet (OpenCode's job 63).
    let fresh = |m: &MeterFrame| m.peak > 0.0 && m.peak_at.is_none_or(|at| at.elapsed() < WORDS_HOLD);
    std::iter::once(left)
        .chain(right.iter())
        .map(|m| if fresh(m) { m.peak.max(m.value) } else { m.value })
        .fold(0.0, f32::max)
}

/// How long the level step's words keep to a held peak.
const WORDS_HOLD: std::time::Duration = std::time::Duration::from_millis(1500);

/// Where Quick's third step listens: the mix chosen there, else the Phones
/// for a mic (speakers and an open mic can howl), else the mix being
/// edited. Quick's choice is its own: it doesn't move the Mixer's mix.
pub fn listen_pair(state: &TuxMix) -> usize {
    if let Some(pair) = state.quick_listen {
        return pair;
    }
    let p = crate::app::strip_params(state, state.quick_channel, state.sel_out);
    match state.control_room.assign.phones_pairs().first() {
        Some(&phones) if p.has_48v => phones,
        _ => state.sel_out,
    }
}

/// Whether the source would play on the speakers as an open, hot mic: the
/// mix is Main or Main B, the source a mic input with 48V on or its gain at
/// 20 dB or more, and something is sent and not muted. A howl in the room.
pub fn speaker_risk(state: &TuxMix) -> bool {
    let pair = listen_pair(state);
    let assign = state.control_room.assign;
    let speakers = pair == assign.main || assign.main_b == Some(pair);
    let cid = state.quick_channel;
    let p = crate::app::strip_params(state, cid, pair);
    let hot = p.has_48v && (p.phantom || p.gain >= 20);
    speakers && hot && !p.mute && state.device.volume(cid, pair).unwrap_or(0.0) > 0.0
}

/// Every open, hot mic playing on the speakers, whatever the view (critique
/// #6: Quick said it, the Mixer showed the same state without a word):
/// a mic input with 48V on or its gain at 20 dB or more, not muted, sent
/// into Main or Main B, and not kept there on purpose ("It's meant to be
/// there", remembered). A linked pair counts once, as its left side. Each
/// with the speaker mix it plays in.
pub fn open_mics_in_speakers(state: &TuxMix) -> Vec<(ChannelId, usize)> {
    open_mics(state).into_iter().filter(|k| !state.open_mic_kept.contains(k)).collect()
}

/// The open, hot mics sent into the speakers, kept or not. The speakers'
/// own level is not asked: a mic routed into a Main at −∞ is armed, and
/// the warning matters more before the Main comes up than after (and the
/// counter doesn't jump under a hand dragging the Main's level).
fn open_mics(state: &TuxMix) -> Vec<(ChannelId, usize)> {
    let assign = state.control_room.assign;
    let speakers: Vec<usize> = std::iter::once(assign.main).chain(assign.main_b).collect();
    let mut found = Vec::new();
    for i in 0..state.device.inputs().len() {
        if i % 2 == 1 && state.device.input_pair_linked(i / 2) {
            continue;
        }
        let cid = ChannelId::Input(i);
        for &pair in &speakers {
            let p = crate::app::strip_params(state, cid, pair);
            let hot = p.has_48v && (p.phantom || p.gain >= 20);
            if hot && !p.mute && state.device.volume(cid, pair).unwrap_or(0.0) > 0.0 {
                found.push((cid, pair));
                break;
            }
        }
    }
    found
}

/// On the tick: "AN1 is out of Main" said no more once it isn't (Undo put
/// it back). A mic kept in the speakers on purpose stays kept, between
/// starts too, until Audra settings → App warns again (the owner's
/// choice, critique #7).
pub fn sweep_open_mics(state: &mut TuxMix) {
    // Which open mics are picking up sound now (critique #14: the warning
    // shows only when it can howl).
    let now = std::time::Instant::now();
    for (cid, _) in open_mics(state) {
        if let ChannelId::Input(i) = cid {
            if state.input_meters.get(i).is_some_and(|m| m.frame().peak > MIC_HEARD) {
                state.open_mic_heard.insert(cid, now);
            }
        }
    }
    // Opened, then gone (kept, taken out, muted): it comes back as a line.
    if state.open_mic_expanded && crate::sidebar::open_mic_shown(state).is_none() && state.open_mic_taken.is_none() {
        state.open_mic_expanded = false;
    }
    if state.open_mic_taken.is_none() {
        return;
    }
    let live: Vec<(ChannelId, usize)> = open_mics(state);
    if state.open_mic_taken.is_some_and(|(c, p, _)| live.contains(&(c, p))) {
        state.open_mic_taken = None;
    }
}

/// A mic heard above −40 dBFS is picking up the room; heard within
/// `MIC_HEARD_FOR`, it counts as live.
const MIC_HEARD: f32 = 0.01;
pub const MIC_HEARD_FOR: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether speaker pair `pair` plays now: its level up, not muted, and (for
/// Main) not dimmed.
/// The speakers on `pair` turned up and not muted, Dim or not: the mic is
/// still in them while dimmed, only quieter (`sidebar::open_mic_shown`).
pub fn speakers_up(state: &TuxMix, pair: usize) -> bool {
    let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
    let level = crate::control_room::user_output_level(state, ch);
    let muted = state.device.outputs().get(ch).is_some_and(|o| o.mute);
    level > 0.0 && !muted
}

/// Whether the placard warns of an open mic now (Quick's "All set" says
/// so rather than stand beside the warning unqualified).
pub fn any_open_mic_warned(state: &TuxMix) -> bool {
    crate::sidebar::open_mic_shown(state).is_some()
}

/// The three steps' states, from the live mix (also the tests' handle).
pub fn steps(state: &TuxMix) -> [Step; 3] {
    let cid = state.quick_channel;
    let p = crate::app::strip_params(state, cid, state.sel_out);
    let power = if !p.has_48v || p.phantom || matching_preset(state, cid).is_some_and(|m| !m.phantom) {
        Step::Done
    } else {
        Step::Open
    };
    let (l, r) = source_meters(state, cid);
    let level = match level_of(held_peak(&l, &r)) {
        Level::Good => Step::Done,
        Level::TooLoud => Step::Warn,
        _ => Step::Open,
    };
    let hear = if hear_problem(state).is_some() {
        Step::Open
    } else if speaker_risk(state) {
        Step::Warn
    } else {
        Step::Done
    };
    [power, level, hear]
}

/// On the tick (the livelier moments): a step newly done draws its tick in
/// and, the first time this session while Quick is on screen, plays one
/// soft note a step up the session's chain; a voice landing in the level
/// band brings the band up to its glow. The first look only notes where
/// the steps stand, so nothing draws in at start.
pub fn watch(state: &mut TuxMix) {
    // A screenshot holds its moment (`--open quick-lively`).
    if state.capture_burst_t.is_some() {
        return;
    }
    let now = std::time::Instant::now();
    // Opening: the meters still settling; the first look comes after.
    if state.settle_until.is_some_and(|t| now < t) {
        return;
    }
    let now_steps = steps(state);
    if let Some(prev) = state.quick_prev {
        // The last step done finishes Quick: the "All set" milestone's rise
        // says it, once, so this tick doesn't add a note of its own. With
        // Milestones off there's no rise, so the step chimes as the others
        // do (it finished in silence; the reviewer, 2026-11-14).
        let finishing = now_steps.iter().all(|s| *s == Step::Done)
            && state.milestone_cards
            && state.milestones_seeded
            && !crate::celebrate::reached(state, "quick-all-set");
        for i in 0..3 {
            // Once a session per step, with Quick on screen: the level
            // step comes and goes with every phrase (the band's glow says
            // that), its moment doesn't.
            if now_steps[i] == Step::Done && prev[i] != Step::Done && state.view == crate::app::View::Quick && !state.quick_rewarded[i] {
                state.quick_rewarded[i] = true;
                state.quick_done_at[i] = Some(now);
                // A guide under way plays its own notes.
                if state.guide.is_none() && !finishing {
                    let (step, _) = crate::celebrate::next_win(state, false);
                    if crate::celebrate::sounds_on(state) {
                        crate::sounds::play(crate::sounds::Sound::Chime(step.min(9)), state.ui_sound_db - 4.0);
                    }
                }
            }
        }
    }
    // The first look, all three already done: "All set" is kept quietly,
    // as the first run keeps every milestone already true (no shower for
    // the past; critique #8).
    if state.quick_prev.is_none()
        && now_steps.iter().all(|s| *s == Step::Done)
        && state.milestones_seeded
        && !crate::celebrate::reached(state, "quick-all-set")
    {
        state.milestones.push("quick-all-set".into());
        state.prefs_dirty = true;
    }
    // A done step folds and stays folded (`folded`); a warning opens it.
    for i in 0..3 {
        if now_steps[i] == Step::Warn {
            state.quick_folded[i] = false;
        } else if folded(state, i, now_steps[i]) {
            state.quick_folded[i] = true;
        }
    }
    state.quick_prev = Some(now_steps);
    let (l, r) = source_meters(state, state.quick_channel);
    let good = level_of(held_peak(&l, &r)) == Level::Good;
    match (good, state.quick_band_at) {
        (true, None) => {
            state.quick_band_at = Some(now);
            // The first good level of the session, in the Mixer: its strip
            // lights once, silently (critique #50's "you're live": what a
            // good level looks like, outside Quick setup).
            if !state.live_washed && state.view == crate::app::View::Mixer && state.animations {
                state.live_washed = true;
                state.live_wash_at = Some(now);
            }
        }
        (false, Some(_)) => state.quick_band_at = None,
        _ => {}
    }
}

/// The mic preset the source is set to now, if any.
pub(crate) fn matching_preset(state: &TuxMix, cid: ChannelId) -> Option<&crate::mic_presets::MicPreset> {
    let ChannelId::Input(idx) = cid else {
        return None;
    };
    state.mic_presets.iter().find(|m| crate::app::mic_preset_matches(state, idx, m))
}

/// The first step still to do: it carries the weight (the reviewer,
/// 2026-11-05: the done steps' green outweighed the one to do next).
fn first_open(steps: &[Step; 3]) -> Option<usize> {
    steps.iter().position(|s| *s != Step::Done)
}

/// What keeps the source from being heard in the mix, said plainly.
/// "Call on my speakers" is set and step 3 listens on the speakers: hearing
/// your own mic isn't the point there (the call app takes it straight from
/// the Babyface).
fn speaker_call(state: &TuxMix, pair: usize) -> bool {
    // While the mic is still out of the speakers (critique #109 A P3-36: a
    // raised send kept the tick and hid the howl warning).
    state.setup_done.as_ref().is_some_and(|d| d.setup == crate::setups::Setup::SpeakerCall)
        && pair == state.control_room.assign.main
        && state.device.volume(state.quick_channel, pair).unwrap_or(0.0) <= 0.0
}

fn hear_problem(state: &TuxMix) -> Option<String> {
    let cid = state.quick_channel;
    let pair = listen_pair(state);
    if speaker_call(state, pair) {
        return None;
    }
    let mix = place_words(state, pair);
    let p = crate::app::strip_params(state, cid, pair);
    // A mic and the speakers: never "raise the send" there (critique #104 A
    // P3-5, #103 A P2-1: it led back into the howl warning).
    if crate::hearfix::open_mic_on_speakers(state, cid, pair) && (p.mute || state.device.volume(cid, pair).unwrap_or(0.0) <= 0.0) {
        return Some(format!(
            "Your mic stays out of {mix}: an open mic there can howl. Listen on {} to hear it.",
            if state.plain_names { "your headphones" } else { "the Phones" }
        ));
    }
    if p.mute {
        // "Your mic (AN1)", as the setup's card (critique #86 A P3-3).
        let who = source_words(state, &p.name);
        let who = if who == p.name { who } else { format!("Your mic ({})", p.name) };
        return Some(format!("{who} is muted: unmute it below."));
    }
    if state.device.volume(cid, pair).unwrap_or(0.0) <= 0.0 {
        return Some(format!("Nothing is sent to {mix} yet: raise the send."));
    }
    let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
    if crate::control_room::user_output_level(state, ch) <= 0.0 {
        return Some(format!("The volume of {mix} is all the way down: raise it on the right."));
    }
    None
}

/// Where step 3 listens, in its sentences: the plain words first when they
/// are on, "your headphones (Phones 3/4)", as the setup's card and the tiles
/// say (critique #84 A P2-2, #85 A P2-1: the step's not-yet lines still
/// said "Phones 3/4" alone); else "Phones 3/4 (your headphones)".
fn place_words(state: &TuxMix, pair: usize) -> String {
    let (name, role) = (crate::sidebar::mix_name(state, pair), crate::sidebar::mix_role_of(state, pair));
    if state.plain_names { format!("{role} ({name})") } else { format!("{name} ({role})") }
}

/// The source in step 3's sentences: "your mic" for a mic input with the
/// plain words on, as step 1 and the card name it; else its code.
fn source_words(state: &TuxMix, name: &str) -> String {
    let is_mic = matches!(state.quick_channel, ChannelId::Input(i) if state.device.inputs().get(i).is_some_and(|c| c.channel_type == tuxmix_core::ChannelType::Mic));
    if state.plain_names && is_mic { "your mic".to_string() } else { name.to_string() }
}

/// About how tall the setup stands at scale 1, for its place on a tall
/// window: measured on build 849 (the "What are you doing?" cards and step
/// 4 grew it from 520; critique #35 found it sitting a third of the way
/// down a 1440x900 window with step 4's key below the fold).
const CONTENT_H: f32 = 800.0;

/// Under this much room under the top bar the whole page doesn't fit (at
/// 1440x900 step 4 just shows; at 1280x700 step 3 is cut off), so done
/// steps fold to one line (`folded`).
const FOLD_BELOW_H: f32 = 1000.0;

/// The page doesn't fit whole under the top bar (`FOLD_BELOW_H`).
fn is_short(state: &TuxMix) -> bool {
    let room = state.window_size.map_or(f32::MAX, |(_, h)| h - theme::TOP_BAR_H * state.ui_scale);
    room < FOLD_BELOW_H * state.ui_scale
}

/// Room the page needs across for two columns of steps (each about as
/// wide as the level meter and its knob need).
const TWO_COLUMNS_W: f32 = 880.0;

/// The steps in two columns, every one whole: the window is wide enough
/// (the owner, 2026-11-15, after seeing both: "Build both when screen is
/// larger"). Mic and level on the left, hear it and Voice on the right, so
/// the level meter stays in view while you talk. A narrower window keeps
/// one column, and folds done steps when it's short too (`folded`).
pub fn two_columns(state: &TuxMix) -> bool {
    let scale = state.ui_scale;
    let column = if state.sidebar_open { state.sidebar_width } else { crate::sidebar::COLLAPSED_WIDTH };
    let across = state.window_size.map_or(0.0, |(w, _)| w - (column + 2.0 * theme::SPACE_XXL) * scale);
    across >= TWO_COLUMNS_W * scale
}

/// Whether Quick setup's step `i` (0 to 2) shows as one line: a short
/// window, done (its tick drawn) or folded since (the level step comes and
/// goes with every phrase: once folded it stays, so the page doesn't jump
/// as you talk), never while it warns (too loud, a howl), and not once
/// opened again with Change.
pub fn folded(state: &TuxMix, i: usize, st: Step) -> bool {
    let short = is_short(state);
    if two_columns(state) {
        return false;
    }
    let settled = st == Step::Done && crate::motion::tick_flash(state, i) <= 0.0 && crate::motion::tick_drawn(state, i) >= 1.0;
    short && st != Step::Warn && !state.quick_unfolded.get(i).copied().unwrap_or(false) && (settled || state.quick_folded.get(i).copied().unwrap_or(false))
}

pub fn view(state: &TuxMix) -> Element<'_, Message> {
    let scale = state.ui_scale;
    let all = steps(state);
    let [s1, s2, s3] = all;
    let next = first_open(&all);
    let all_done = next.is_none();
    let head = column![
        text("Quick setup").font(theme::SERIF).size(theme::TEXT_BIG * scale).color(theme::text_primary()),
        text(if all_done {
            if any_open_mic_warned(state) {
                // Not "All set" beside "it can howl" unqualified (critique #7).
                if state.sidebar_open {
                    "All set where you listen, but a mic is open in your speakers too: see the warning at the right.".to_string()
                } else {
                    "All set where you listen, but a mic is open in your speakers too: open the column at the right to see it.".to_string()
                }
            } else {
                "All set: you'll hear yourself where you listen. The Mixer has the rest.".to_string()
            }
        } else {
            // What Audra is, on the page a first run opens on (critique
            // #94 A P3-6: said only in Help's About).
            "Audra runs your Babyface\u{2019}s mixer. Set up your mic, its level, and where you hear it, one step at a time.".to_string()
        })
        .size(theme::TEXT_SM * scale)
        // Green only for an "All set" with nothing to qualify it.
        .color(if all_done && !any_open_mic_warned(state) { theme::readable_on_floors(theme::signal()) } else { theme::text_sec() }),
    ]
    .spacing(theme::SPACE_TIGHT * scale);
    let rule = || rule::horizontal(1).style(theme::hairline_rule);
    // Its controls are the keyboard's stops, in the order they stand
    // (`keyfocus::reach`).
    let two = two_columns(state);
    let steps: Element<'_, Message> = crate::keyfocus::in_region(crate::keyfocus::Region::Quick, || {
        if two {
            let voice = if show_voice_step(state) { step_voice(state, all_done, scale) } else { voice_step_put_away(scale) };
            let left = column![step_mic(state, s1, next == Some(0), scale), rule(), step_level(state, s2, next == Some(1), scale)]
                .spacing(theme::SPACE_XL * scale)
                .width(Length::FillPortion(1));
            let right = column![step_hear(state, s3, next == Some(2), scale), rule(), voice]
                .spacing(theme::SPACE_XL * scale)
                .width(Length::FillPortion(1));
            return column![
                crate::setups::view(state),
                rule(),
                row![left, rule::vertical(1).style(theme::hairline_rule), right].spacing(theme::SPACE_XL * scale),
            ]
            .spacing(theme::SPACE_XL * scale)
            .into();
        }
        let col = column![
            // Ready-made setups first (the owner's pick from mockups).
            crate::setups::view(state),
            rule(),
            step_mic(state, s1, next == Some(0), scale),
            rule(),
            step_level(state, s2, next == Some(1), scale),
            rule(),
            step_hear(state, s3, next == Some(2), scale),
        ]
        .spacing(theme::SPACE_XL * scale);
        if show_voice_step(state) {
            col.push(rule()).push(step_voice(state, all_done, scale)).into()
        } else {
            // Put away with "Not now": one quiet line brings it back
            // (critique #37: the page just ended at step 3).
            col.push(rule()).push(voice_step_put_away(scale)).into()
        }
    });
    let body = column![
        head,
        rule(),
        steps,
    ]
    .spacing(theme::SPACE_XL * scale)
    .max_width(if two { 960.0 } else { 760.0 } * scale)
    .width(Length::Fill);
    // In the upper third of a tall window rather than pinned under the bar
    // (the reviewer, 2026-11-05: at 1902x1075 over half the height was
    // empty floor under it); a short one keeps the usual margin.
    let room = state.window_size.map_or(0.0, |(_, h)| h - theme::TOP_BAR_H * scale);
    let top = ((room - CONTENT_H * scale) / 3.0).max(theme::SPACE_XL * scale);
    iced::widget::mouse_area(
        container(
            scrollable(container(body).center_x(Length::Fill).padding(iced::Padding {
                top,
                right: theme::SPACE_XXL * scale,
                bottom: theme::SPACE_XL * scale,
                left: theme::SPACE_XXL * scale,
            }))
                .id(SCROLL_ID)
                // Marked: on a short window it's the sign there's more below.
                .style(theme::scrollable_marked)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(theme::root)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .on_press(Message::ClearSelection)
    .into()
}

/// Step 4 newly done by a press on the page (a sound, or the switch): its
/// tick draws in and its badge flashes, as steps 1 to 3 do. No note of its
/// own: the press has already made its sound (`sounds::for_message`).
pub fn voice_step_landed(state: &mut TuxMix, was_on: bool) {
    if !was_on && crate::dynamics_panel::power_of(state, state.quick_channel) && state.capture_burst_t.is_none() {
        state.quick_done_at[3] = Some(std::time::Instant::now());
    }
}

/// Step 4 put away: its number, its title and a Show key, in the quiet ink.
fn voice_step_put_away<'a>(scale: f32) -> Element<'a, Message> {
    let msg = Message::QuickVoiceSkip(false);
    row![
        text("4").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).color(theme::text_sec()),
        text("Make your voice sound good (optional)").size(theme::TEXT_SM * scale).color(theme::text_sec()),
        crate::keyfocus::reach_press(
            "Show step 4",
            button(text("Show").size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                .style(theme::toggle_button(false, theme::accent()))
                .on_press(msg.clone())
                .into(),
            Some(msg),
            theme::RADIUS_SM * scale,
        ),
    ]
    .spacing(theme::SPACE_MD * scale)
    .align_y(Alignment::Center)
    .into()
}

/// Whether step 4 shows: until it's put away with "Not now", and always
/// once done (its tick says the voice is shaped).
fn show_voice_step(state: &TuxMix) -> bool {
    !state.quick_voice_skipped || crate::dynamics_panel::power_of(state, state.quick_channel)
}

/// Step 4, optional (the owner's pick after critique #31, "A": beginners
/// start here and would never look behind a key marked "D"): the voice
/// for others, one press away. It isn't one of the three that make "All
/// set"; done when the processed mic is on for this mic.
/// Which of step 4's three sounds is set on `cid` now, by its key's name.
fn picked_sound(state: &TuxMix, cid: ChannelId) -> Option<&'static str> {
    let p = crate::dynamics_panel::params_of(state, cid);
    let presets = crate::dynamics_panel::presets_for(cid);
    // Any of the Voice panel's sounds, the first three by the names this
    // page gives them (OpenCode's job 60: "Singing" picked read as none).
    let i = presets.iter().position(|(_, q)| *q == p)?;
    Some(["Clear Calls", "Streamer", "Podcast"].get(i).copied().unwrap_or(presets[i].0))
}

fn step_voice(state: &TuxMix, after_the_rest: bool, scale: f32) -> Element<'_, Message> {
    // Right here on the page (critique #33's pick, "B"): three sounds, one
    // switch and the one thing to do in your call app; the full Voice panel
    // one click away. It used to send you off to the busy Mixer.
    let cid = state.quick_channel;
    let on = crate::dynamics_panel::power_of(state, cid);
    let mock = state.device.is_mock();
    let st = match (on, mock) {
        (true, true) => Step::DoneSimulated,
        (true, false) => Step::Done,
        (false, _) => Step::Open,
    };
    let mic = crate::dynamics_loop::SOURCE_LABEL;
    // The Voice panel's words in the simulation (critique #37): no call app
    // can hear a simulated voice.
    let words = if on && mock {
        "On, but this is the simulation: nothing is shaped.".to_string()
    } else if on && crate::app::plain_mic_looped(state, cid).is_none() && (0..state.device.output_pair_count()).any(|p| crate::app::input_looped(state, p * 2)) {
        // The looped mix carries the computer alone (Stream with Voice on):
        // both inputs, the voice shaped (the owner's pick).
        format!("On. For a call, pick \u{201c}{mic}\u{201d} as the mic. For a stream, add \u{201c}{mic}\u{201d} (your voice) and the mix to your apps (the computer); Cough silences your voice on it.")
    } else if on && (0..state.device.output_pair_count()).any(|p| crate::app::input_looped(state, p * 2)) {
        // A mix looped back (the Stream or Record setup) has the plain mic
        // and the computer: one or the other, never both, said before the
        // "no app yet" line (critique #104 and #105 A P2-1).
        format!("On. For a call, pick \u{201c}{mic}\u{201d} as the mic. For a stream, the mix to your apps still has your plain mic: press Stream again to send your shaped voice there instead.")
    } else if on && state.dyn_heard == Some(false) {
        format!("Voice is on, but no app uses it yet: pick \u{201c}{mic}\u{201d} as the mic in your call app.")
    } else if on && speaker_call(state, listen_pair(state)) {
        // On a speaker call nobody hears themselves (critique #109 A P3-35).
        format!("On. In your call app, pick \u{201c}{mic}\u{201d} as the mic.")
    } else if on {
        format!("On. In your call or streaming app (Zoom, OBS, Discord\u{2026}), pick \u{201c}{mic}\u{201d} as the mic. You still hear yourself straight.")
    } else if let Some(name) = picked_sound(state, cid) {
        // Picked but Voice off: its key wears the "set, waiting" look, said
        // here too (critique #59: it read as already on).
        format!("{name} picked: turn on Voice to use it.")
    } else {
        "Optional: pick a sound for calls and streams. Others hear it; you hear yourself straight.".to_string()
    };
    let p = crate::dynamics_panel::params_of(state, cid);
    let presets = crate::dynamics_panel::presets_for(cid);
    let sound = |name: &'static str, i: usize| -> Element<'_, Message> {
        let picked = presets.get(i).is_some_and(|(_, q)| *q == p);
        let msg = Message::QuickSound(i);
        container(crate::keyfocus::reach_press(
            &format!("Sound: {name}"),
            button(text(name).size(theme::TEXT_MD * scale).width(Length::Fill).center())
                .padding([theme::SPACE_MD * scale, theme::SPACE_LG * scale])
                .width(Length::Fill)
                // A third of the row each, they fall between pixels sideways:
                // a rim that reads wherever it lands (critique #56).
                .style(theme::split_safe_key(move |t: &iced::Theme, st| {
                    if picked && !on {
                        // Picked, Voice off: "set, waiting", as in the panel.
                        (theme::key_button_armed(theme::accent(), theme::RADIUS_SM))(t, st)
                    } else {
                        (theme::toggle_button(picked, theme::accent()))(t, st)
                    }
                }, picked && on))
                .on_press(msg.clone())
                .into(),
            Some(msg),
            theme::RADIUS_SM * scale,
        ))
        .width(Length::Fill)
        .into()
    };
    // Capsules, as the page's other one-off actions are (Undo, Show me in
    // the Mixer, Replace, Cancel; critique #90 A P3-9: square here).
    let key = |words: &'static str, msg: Message, _lit: bool| -> Element<'_, Message> {
        crate::keyfocus::reach_press(
            words,
            button(text(words).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                .style(theme::page_key(14.0 * scale))
                .on_press(msg.clone())
                .into(),
            Some(msg),
            14.0 * scale,
        )
    };
    // The sounds first, as they stand on the page, so Tab meets them in
    // that order (critique #37: the keys row was built, and reached, first).
    let sounds = row![sound("Clear Calls", 0), sound("Streamer", 1), sound("Podcast", 2)].spacing(theme::SPACE_MD * scale);
    // A switch says where it stands (critique #36: drawn as a button,
    // nothing said it was off), drawn as Audra's switches are (critique
    // #52: one of five on/off looks).
    let shaping = container(crate::app::switch_row("Voice: shapes how others hear you", on, Some(Message::QuickShape(!on)), scale)).max_width(340.0 * scale);
    let mut keys = row![key("Open Voice", Message::QuickOpenVoice, false)].spacing(theme::SPACE_MD * scale);
    if !on {
        keys = keys.push(key("Not now", Message::QuickVoiceSkip(true), false));
    }
    let controls = column![
        // The presets' own names, as the Voice panel says them (critique
        // #36: "Stream" and "Podcast" also named the cards above), under a
        // label that says what they are for (critique #37: they still read
        // as the setup cards' twins).
        text("A sound for others").size(theme::TEXT_SM * scale).color(theme::text_sec()),
        sounds,
        shaping,
        keys,
    ]
    .spacing(theme::SPACE_MD * scale);
    step(4, "Make your voice sound good", words, st, after_the_rest && !on, controls.into(), scale, (crate::motion::tick_drawn(state, 3), crate::motion::tick_flash(state, 3)), None)
}

/// One step: its badge (the number, or a tick once done), its title and
/// the line that says where it stands, and its controls under them.
#[allow(clippy::too_many_arguments)]
fn step<'a>(n: u8, title: &'a str, status: String, st: Step, next: bool, controls: Element<'a, Message>, scale: f32, (drawn, flash): (f32, f32), fold: Option<Message>) -> Element<'a, Message> {
    let d = 30.0 * scale;
    let badge: Element<'a, Message> = match st {
        // Just done: the tick draws in as the badge's field flashes and
        // settles (`motion::tick_drawn`).
        Step::Done | Step::DoneSimulated => container(crate::widgets::strip_icons::check_icon_drawn(theme::readable_on_floors(theme::signal()), scale, drawn))
            .width(Length::Fixed(d))
            .height(Length::Fixed(d))
            .style(badge_flash(theme::signal(), d, flash))
            .into(),
        Step::Warn => container(text(n.to_string()).font(theme::SANS_SEMI).size(theme::TEXT_MD * scale).center())
            .width(Length::Fixed(d))
            .height(Length::Fixed(d))
            .center(Length::Fixed(d))
            .style(badge_style(theme::clip(), d))
            .into(),
        Step::Open => {
            let badge = container(
                text(n.to_string())
                    .font(theme::SANS_SEMI)
                    .size(theme::TEXT_MD * scale)
                    // The step to do next: its disc filled with the ink and
                    // its number cut out of it, so it stands apart from a
                    // waiting step's ring by more than tone (critique #92 B:
                    // the two rings stood 1.68 apart on High contrast).
                    .color(if next { theme::bg_deep() } else { theme::text_sec() })
                    .center(),
            )
            .width(Length::Fixed(d))
            .height(Length::Fixed(d))
            .center(Length::Fixed(d))
            .style(move |t: &iced::Theme| {
                let mut style = theme::inset_box(d / 2.0)(t);
                // Waiting: the ring alone. Its graded fill read as a raised
                // key on Daylight (critique #93 A P3-10).
                style.background = next.then(|| iced::Background::Color(theme::text_primary()));
                style.shadow = iced::Shadow::default();
                style
            });
            // The step to do next: its number ringed in the ink. A step that
            // waits: a thin firm ring, so the four read as one list on every
            // theme (critique #90 B: its disc was 1.01-2.42 against the
            // page, gone on High contrast).
            {
                iced::widget::stack![
                    badge,
                    container(Space::new().width(Length::Fill).height(Length::Fill))
                        .width(Length::Fixed(d))
                        .height(Length::Fixed(d))
                        .style(move |_t: &iced::Theme| container::Style {
                            border: iced::Border {
                                color: if next { theme::text_primary() } else { theme::firm_edge() },
                                // 1.5 px waiting: at 1 px its diagonals read
                                // 2.59-2.96 (critique #91 B). The next step's
                                // disc is filled (above), which is what tells
                                // the two apart; its ring only rims the fill.
                                width: if next { 2.5 } else { 1.5 },
                                radius: (d / 2.0).into(),
                            },
                            ..container::Style::default()
                        }),
                ]
                .into()
            }
        }
    };
    let status_color = match st {
        Step::Done => theme::readable_on_floors(theme::signal()),
        Step::DoneSimulated => theme::text_sec(),
        Step::Warn => theme::readable_on_floors(theme::clip()),
        Step::Open if next => theme::text_primary(),
        Step::Open => theme::text_sec(),
    };
    let words = column![
        text(title).font(theme::SANS_SEMI).size(theme::TEXT_MD * scale).color(theme::text_primary()),
        text(status).size(theme::TEXT_SM * scale).color(status_color),
    ]
    .spacing(theme::SPACE_TIGHT * scale);
    // Folded (a done step on a short window): its words on one line with
    // a Change key, the controls put away until it's opened again.
    if let Some(open) = fold {
        let key = crate::keyfocus::reach_press(
            &format!("Change: {title}"),
            iced::widget::button(text("Change").size(theme::TEXT_SM * scale))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_LG * scale])
                // A rim that reads wherever it lands (critique #58 B: 1.86
                // to 2.73 on Daylight).
                .style(theme::split_safe_key(theme::page_key(11.0 * scale), false))
                .on_press(open.clone())
                .into(),
            Some(open),
            11.0 * scale,
        );
        return row![badge, container(words).width(Length::Fill), key]
            .spacing(theme::SPACE_LG * scale)
            .align_y(Alignment::Center)
            .into();
    }
    row![
        badge,
        column![words, controls]
        .spacing(theme::SPACE_LG * scale)
        .width(Length::Fill),
    ]
    .spacing(theme::SPACE_LG * scale)
    .align_y(Alignment::Start)
    .into()
}

/// A done or warning badge: a ring in its hue round a faint field of it.
fn badge_style(hue: Color, d: f32) -> impl Fn(&iced::Theme) -> container::Style {
    badge_flash(hue, d, 0.0)
}

/// `badge_style` with its field flashing brighter (`flash` 1 to 0) and a
/// glow of its hue round it, as a tick draws in.
fn badge_flash(hue: Color, d: f32, flash: f32) -> impl Fn(&iced::Theme) -> container::Style {
    let f = flash.clamp(0.0, 1.0);
    move |_theme| {
        // Flashing on a light floor, the hue at its readable depth, so the
        // flash isn't a pastel wash; at rest, as it always was.
        let deep = if f > 0.0 && !theme::dark_theme() { theme::readable_on_floors(hue) } else { hue };
        container::Style {
            shadow: iced::Shadow { color: Color { a: 0.55 * f, ..deep }, offset: iced::Vector::new(0.0, 0.0), blur_radius: 12.0 * f },
            background: Some(iced::Background::Color(Color { a: 0.14 + 0.3 * f, ..deep })),
            // A step toward the ink: on Daylight the warning's digit read
            // 4.34 (critique #87 B, the too-loud step).
            // High contrast: the ink at 7:1 and a whole rim at 4.5
            // (critiques #109-#111 B: the warning's "3" read 6.74, its ring
            // 4.17).
            text_color: Some(if theme::is_high_contrast() {
                theme::text_primary()
            } else {
                theme::blend(theme::readable_on_tint(hue, 0.14 + 0.3 * f), theme::text_primary(), 0.2)
            }),
            border: iced::Border {
                color: if theme::is_high_contrast() { theme::readable_on_floors(hue) } else { Color { a: 0.8, ..hue } },
                width: 1.5,
                radius: (d / 2.0).into(),
            },
            ..container::Style::default()
        }
    }
}

fn picker_label(scale: f32, words: &'static str) -> Element<'static, Message> {
    text(words).size(theme::TEXT_SM * scale).color(theme::text_sec()).into()
}

/// 1. The source, its mic preset and its 48V.
fn step_mic(state: &TuxMix, st: Step, next: bool, scale: f32) -> Element<'_, Message> {
    let cid = state.quick_channel;
    let p = crate::app::strip_params(state, cid, state.sel_out);
    let options = crate::app::quick_channel_options(state);
    let labels: Vec<String> = options.iter().map(|(_, l)| l.clone()).collect();
    let cids: Vec<ChannelId> = options.iter().map(|(c, _)| *c).collect();
    let current = options.iter().find(|(c, _)| *c == cid).map(|(_, l)| l.clone());
    let pick_source = {
        let (labels, cids) = (labels.clone(), cids.clone());
        move |label: String| {
            let i = labels.iter().position(|l| *l == label).unwrap_or(0);
            Message::QuickChannelSelected(cids[i])
        }
    };
    let source = pick_list(labels.clone(), current.clone(), pick_source.clone())
        .style(theme::pick_list)
        .menu_style(theme::menu)
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .text_size(theme::TEXT_MD * scale)
        .width(Length::Fixed(200.0 * scale));
    let source = crate::keyfocus::reach_pick("Source", source.into(), &labels, current.as_ref(), pick_source, theme::RADIUS_SM * scale);
    let mut controls = row![column![picker_label(scale, "Source"), source].spacing(theme::SPACE_TIGHT * scale)]
        .spacing(theme::SPACE_XL * scale)
        .align_y(Alignment::End);
    // Its mic preset, where it has a gain to set and presets are saved.
    if let (ChannelId::Input(idx), true) = (cid, p.has_gain && !state.mic_presets.is_empty()) {
        let names: Vec<String> = state.mic_presets.iter().map(|m| m.name.clone()).collect();
        let now = state
            .mic_presets
            .iter()
            .position(|m| crate::app::mic_preset_matches(state, idx, m))
            .map(|n| names[n].clone());
        let names_for = names.clone();
        let load = move |name: String| Message::MicPresetLoad(idx, names_for.iter().position(|n| *n == name).unwrap_or(0));
        let preset = pick_list(names.clone(), now.clone(), load.clone())
            .placeholder("Load a mic preset")
            .style(theme::pick_list)
            .menu_style(theme::menu)
            .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
            .text_size(theme::TEXT_MD * scale)
            .width(Length::Fixed(220.0 * scale));
        // The arrows load the preset before or after (its 48V still waits
        // for a click, its gain glides).
        let preset = crate::keyfocus::reach_pick("Mic preset", preset.into(), &names, now.as_ref(), load, theme::RADIUS_SM * scale);
        controls = controls.push(column![picker_label(scale, "Mic preset"), preset].spacing(theme::SPACE_TIGHT * scale));
    }
    let mut words = String::new();
    if let (ChannelId::Input(idx), true) = (cid, p.has_48v) {
        let armed = crate::app::phantom_armed(state, idx);
        let key = button(
            text(if !p.phantom && armed { "48V?" } else { "48V" })
                .font(theme::SANS_SEMI)
                .size(theme::TEXT_MD * scale)
                .center(),
        )
        .padding([0.0, theme::SPACE_LG * scale])
        .height(Length::Fixed(34.0 * scale))
        // Unlit, its rim held at 3:1 wherever it lands (carried since #58:
        // Daylight's read 2.02-2.44 on the Quick page), armed too, its
        // face a tint (critique #85 B: 1.89-3.12 then); lit, its own
        // edge says it.
        .style(theme::split_safe_key(
            theme::lamp_button(
                theme::phantom(),
                theme::Lamp { active: p.phantom, armed, waiting: armed, hue_label: true, ..theme::Lamp::default() },
            ),
            p.phantom,
        ))
        .on_press(Message::PhantomPress(idx));
        // Its two presses stay two from the keyboard.
        controls = controls.push(crate::keyfocus::reach_press("48V", key.into(), Some(Message::PhantomPress(idx)), 17.0 * scale));
        let preset = matching_preset(state, cid);
        words = match (p.phantom, armed, preset) {
            (true, _, Some(m)) => format!("48V is on for {}: your condenser mic has its power.", m.name),
            (true, _, None) => "48V is on: a condenser mic has its power.".into(),
            // Armed by a setup's press, not a first click (critique #85 A
            // P2-2: "again" had nothing to refer to).
            // How to tell, for a mic the app doesn't know (critique #94 A
            // P2-1: the first thing asked of a newcomer rested on
            // "condenser" and "dynamic", with no way to find out which).
            (false, true, _) if state.setup_arm == Some(idx) => "Speak first: if step 2\u{2019}s meter moves, leave 48V off. If not, click 48V? once.".into(),
            (false, true, _) => "Click 48V? again to switch phantom power on.".into(),
            (false, false, Some(m)) if !m.phantom => format!("{} needs no 48V: nothing to switch on.", m.name),
            (false, false, Some(m)) => format!("48V is off, and {} uses it: click 48V twice.", m.name),
            (false, false, None) => "48V is off. Speak: if the meter in step 2 moves, you're set. If it stays still, your mic needs 48V: click 48V twice.".into(),
        };
    }
    // An instrument or line input is not "your mic" (critique #97 A P2-3:
    // with IN3/4 as the source the page still spoke only of a mic).
    let is_mic = match cid {
        ChannelId::Input(i) => state.device.inputs().get(i).is_none_or(|c| c.channel_type == tuxmix_core::ChannelType::Mic),
        _ => false,
    };
    if words.is_empty() {
        words = if is_mic {
            format!("{}: no phantom power here, nothing to switch on.", p.name)
        } else {
            format!("{}: plug in and play. Nothing to switch on here (48V is for mics).", p.name)
        };
    }
    // The second mic's way in (critique #98 A P2-3: two people met a page
    // that knew one mic, and the Source list was the only door, unsaid).
    let other_mic = state.device.inputs().iter().enumerate().any(|(i, c)| c.channel_type == tuxmix_core::ChannelType::Mic && ChannelId::Input(i) != cid);
    if is_mic && other_mic {
        words.push_str(" A second mic? Pick it in Source and do these steps again.");
    }
    step(1, if is_mic { "Your mic" } else { "Your source" }, words, st, next, controls.into(), scale, (crate::motion::tick_drawn(state, 0), crate::motion::tick_flash(state, 0)), folded(state, 0, st).then_some(Message::QuickUnfold(0)))
}

/// "Too quiet" with the gain it takes, so nobody has to raise their voice
/// to reach the band (the owner, 2026-11-06, TLM 107 at +35 dB: "I need to
/// speak up louder for it to land in that spot"): the dB from the peak to
/// the band's middle, as the gain to set; at the top of its range, the
/// mic's own pad or the distance is what's left.
pub(crate) fn quiet_words(peak: f32, gain: u32, gain_max: u32, pad_on: bool, mic: bool) -> String {
    // Audra's own Pad first: `PAD_DB` back with one switch (the reviewer: the
    // owner's preset loads it, and "more gain" was the wrong first advice).
    if pad_on {
        return format!("Too quiet, and the Pad (\u{2212}{pad} dB) is on: switch it off first, in its Settings; that's {pad} dB back.", pad = crate::app::PAD_DB);
    }
    let own_pad = if mic { "the mic's own pad" } else { "the source's own level" };
    let peak_db = 20.0 * peak.max(1e-6).log10();
    let mid = (BAND.0 + BAND.1) / 2.0;
    let more = (mid - peak_db).round().max(1.0) as u32;
    let to = (gain + more).min(gain_max);
    if gain >= gain_max {
        format!("Too quiet, and the gain is at its top (+{gain_max} dB): check {own_pad}, or come a little closer.")
    } else if to < gain + more {
        format!("Too quiet: the gain all the way up (+{to} dB) gets closest; check {own_pad} too.")
    } else {
        format!("Too quiet: about {more} dB more gain (to +{to} dB) puts your peaks in the band (\u{2212}18 to \u{2212}6 dB), without raising your voice.")
    }
}

/// 2. The level against the band, and the gain that moves it.
fn step_level(state: &TuxMix, st: Step, next: bool, scale: f32) -> Element<'_, Message> {
    let cid = state.quick_channel;
    let p = crate::app::strip_params(state, cid, state.sel_out);
    let (l, r) = source_meters(state, cid);
    let level = level_of(held_peak(&l, &r));
    let gain_words = if p.has_gain { "turn the gain" } else { "set it at the source" };
    let words = match level {
        Level::Silent => "Speak as you will on the call or the stream; the meter's peaks should land in the band.".to_string(),
        Level::Quiet if p.has_gain => quiet_words(held_peak(&l, &r), p.gain, p.gain_max, p.has_pad && p.pad, p.has_48v),
        Level::Quiet => format!("Too quiet: {gain_words} up until your peaks reach the band."),
        Level::Good => "Good level: your peaks sit in the band.".to_string(),
        Level::Hot => format!("A little too loud: {gain_words} down a few dB."),
        Level::TooLoud => format!("Too loud, about to distort: {gain_words} down."),
    };
    let meter = fader::target_meter(l, r, BAND, scale, crate::motion::band_glow(state));
    let controls: Element<'_, Message> = match (cid, p.has_gain) {
        (ChannelId::Input(idx), true) => {
            let max = p.gain_max;
            let gain = crate::widgets::knob::knob(crate::widgets::knob::Knob {
                readout: crate::widgets::knob::Readout::Below,
                tint: None,
                value: p.gain as f32,
                range: (0.0, max as f32),
                // As the strips print it (critique #5: "35" here, "+35 dB"
                // there).
                label: format!("+{} dB", p.gain),
                arc_from_center: false,
                home: None,
                interactive: true,
                log_scale: false,
                modifiers: state.modifiers,
                // About 56px: the control this step asks for.
                scale: scale * 1.35,
                on_change: Box::new(move |v| Message::Gain(idx, (v.round() as u32).min(max))),
                on_reset: Box::new(move || Message::Gain(idx, 0)),
            });
            let gain_cell = column![gain, text("Gain").size(theme::TEXT_SM * scale).color(theme::text_sec())]
                .spacing(theme::SPACE_TIGHT * scale)
                .align_x(Alignment::Center);
            let value = p.gain as f32;
            // AutoSet (issue #26) beside the gain it guards: a key that says
            // what it does, lit while on.
            // A switch, as every yes/no choice in Audra is (critique #52: a
            // key named after its state was one of five on/off looks).
            let on = crate::autoset::is_on(state, idx);
            let autoset = container(crate::app::switch_row_note(
                "AutoSet",
                "Turns the gain down by itself if it gets too loud; never up",
                on,
                Some(Message::SetAutoSet(idx, !on)),
                scale,
            ))
            // Wide enough for its note on one line (critique #55: at 340 it
            // left "up" alone on a second line), close enough that the
            // switch stays by its label (it floated at 460; critique #53).
            .max_width(420.0 * scale);
            column![
                row![
                    container(meter).width(Length::Fill),
                    crate::keyfocus::reach_knob(
                        "Gain",
                        gain_cell.into(),
                        value,
                        (0.0, max as f32),
                        false,
                        Some(1.0),
                        move |v| Message::Gain(idx, (v.round() as u32).min(max)),
                        theme::RADIUS_SM * scale,
                    ),
                ]
                .spacing(theme::SPACE_XL * scale)
                .align_y(Alignment::Center),
                autoset,
            ]
            .spacing(theme::SPACE_MD * scale)
            .into()
        }
        _ => meter,
    };
    step(2, "Its level", words, st, next, controls, scale, (crate::motion::tick_drawn(state, 1), crate::motion::tick_flash(state, 1)), folded(state, 1, st).then_some(Message::QuickUnfold(1)))
}

/// 3. The mix it plays in, the send into it, and what stands in the way.
fn step_hear(state: &TuxMix, st: Step, next: bool, scale: f32) -> Element<'_, Message> {
    let cid = state.quick_channel;
    let pair = listen_pair(state);
    let p = crate::app::strip_params(state, cid, pair);
    let mixes: Vec<String> = (0..state.device.output_pair_count()).map(|i| crate::sidebar::mix_name(state, i)).collect();
    let mixes_for = mixes.clone();
    // Quick's own choice: the Mixer's mix stays where it is.
    let select = move |name: String| Message::QuickListen(mixes_for.iter().position(|m| *m == name).unwrap_or(0));
    let mix = pick_list(mixes.clone(), mixes.get(pair).cloned(), select.clone())
        .style(theme::pick_list)
        .menu_style(theme::menu)
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .text_size(theme::TEXT_MD * scale)
        .width(Length::Fixed(200.0 * scale));
    let mix = crate::keyfocus::reach_pick("Listen on", mix.into(), &mixes, mixes.get(pair), select, theme::RADIUS_SM * scale);
    let send = state.device.volume(cid, pair).unwrap_or(0.0);
    let fader = crate::widgets::level_fader::level_fader(crate::widgets::level_fader::LevelFader {
        t: fader::vol_to_t(send),
        default_t: fader::vol_to_t(1.0),
        dimmed: false,
        scale,
        modifiers: state.modifiers,
        on_change: Box::new(move |t| Message::VolumeChanged(cid, pair, fader::t_to_vol(t))),
        on_press: Box::new(move |t| Message::FaderPressed(cid, pair, fader::t_to_vol(t), None)),
        on_reset: Message::Reset(cid, pair, 1.0),
        on_release: Message::RangeCleared(cid),
    });
    // A wheel detent a step from the keyboard, as the counter's levels.
    let fader = crate::keyfocus::reach_knob(
        "Send",
        fader,
        fader::vol_to_t(send),
        (0.0, 1.0),
        false,
        Some(crate::app::NUDGE_STEP_T),
        move |t| Message::VolumeChanged(cid, pair, fader::t_to_vol(t)),
        theme::RADIUS_SM * scale,
    );
    let db = if send <= 0.0 { "\u{2212}\u{221e}".to_string() } else { format!("{} dB", crate::app::db_number(send)) };
    let controls = row![
        column![picker_label(scale, "Listen on"), mix].spacing(theme::SPACE_TIGHT * scale),
        column![
            row![
                picker_label(scale, "Send"),
                Space::new().width(Length::Fill),
                text(db).font(theme::MONO).size(theme::TEXT_SM * scale).color(theme::text_primary()),
            ],
            fader,
        ]
        .spacing(theme::SPACE_TIGHT * scale)
        .width(Length::Fill),
    ]
    .spacing(theme::SPACE_XL * scale)
    .align_y(Alignment::End);
    // "I can't hear it" (the owner's pick, 2026-11-21): one press fixes
    // what Audra holds back in this mix (a mute, a send at nothing, the
    // output down, Dim, a solo elsewhere), or says where else to look. In
    // the ink while a problem is said above it; it took the Unmute key's
    // place, which fixed one of those.
    // "Fix it:" only while the step says a problem: the press also sends the
    // computer's main sound, which the step line doesn't check, and "Fix it"
    // under a green tick read as a contradiction (critique #99 A P2-1).
    let can_fix = hear_problem(state).is_some() && !crate::hearfix::find(state, cid, pair).is_empty();
    let hear_key: Element<'_, Message> = crate::keyfocus::reach_press(
        "I can't hear it",
        button(text(if can_fix { "Fix it: I can\u{2019}t hear it" } else { "I can\u{2019}t hear it" }).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
            .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
            // Outlined in the lamp while there is something to fix, as the
            // comment above promised (critique #99 A P3-5: both states drew
            // the same key).
            .style(move |t: &iced::Theme, st: button::Status| {
                if can_fix { (theme::split_safe_key(theme::lamp_outline, false))(t, st) } else { (theme::page_key(14.0 * scale))(t, st) }
            })
            .on_press(Message::HearFix)
            .into(),
        Some(Message::HearFix),
        14.0 * scale,
    );
    // An open, hot mic on the speakers: the step stays a warning, with the
    // one press that moves it to the headphones.
    let risk = (hear_problem(state).is_none() && speaker_risk(state))
        || (crate::hearfix::open_mic_on_speakers(state, cid, pair) && hear_problem(state).is_some());
    let mut listen_key: Option<Element<'_, Message>> = None;
    if risk {
        if let Some(&phones) = state.control_room.assign.phones_pairs().first() {
            let listen = if state.plain_names { "Listen on your headphones" } else { "Listen on the Phones" };
            // On a line of its own under the two controls: beside them it
            // left the Send slider a 36 px stub, under a warning that says
            // "keep the send low" (critique #96 A P2-2).
            listen_key = Some(crate::keyfocus::reach_press(
                listen,
                button(text(listen).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
                    .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                    // The one safe press under the warning, lit as Fix it is
                    // (critique #103 A P3-4).
                    .style(theme::split_safe_key(theme::lamp_outline, false))
                    .on_press(Message::QuickListen(phones))
                    .into(),
                Some(Message::QuickListen(phones)),
                14.0 * scale,
            ));
        }
    }
    let keys = match listen_key {
        // Under the howl warning only its own key: "I can't hear it" there
        // took out the mic the step says is playing (critique #102 A P3-5).
        // And for someone without headphones, the speaker-call card
        // (critique #109 A P3-34).
        Some(key) => {
            let call = crate::keyfocus::reach_press(
                "No headphones: Call on my speakers",
                button(text("No headphones: Call on my speakers").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
                    .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                    .style(theme::page_key(14.0 * scale))
                    .on_press(Message::Setup(crate::setups::Setup::SpeakerCall))
                    .into(),
                Some(Message::Setup(crate::setups::Setup::SpeakerCall)),
                14.0 * scale,
            );
            row![key, call].spacing(theme::SPACE_MD * scale)
        }
        None => row![hear_key],
    };
    let controls: Element<'_, Message> = iced::widget::column![controls, keys].spacing(theme::SPACE_MD * scale).into();
    let words = hear_problem(state).unwrap_or_else(|| {
        // The plain words first when they are on, as on the setup's card
        // (critique #84 A P2-2: "Phones 3/4 (your headphones)" under a card
        // that said "your headphones").
        let place = place_words(state, pair);
        let who = source_words(state, &p.name);
        if speaker_call(state, pair) {
            "On a call on your speakers you needn\u{2019}t hear yourself: your call app takes your mic straight from the Babyface.".to_string()
        } else if risk {
            format!(
                "Speakers and an open mic can howl: {who} plays in {place}. Listen on {}, or keep the send low.",
                if state.plain_names { "your headphones" } else { "the Phones" }
            )
        } else {
            format!("You'll hear {who} in {place}.")
        }
    });
    step(3, "Hear it", words, st, next, controls, scale, (crate::motion::tick_drawn(state, 2), crate::motion::tick_flash(state, 2)), folded(state, 2, st).then_some(Message::QuickUnfold(2)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn done_steps_fold_on_a_short_window_stay_folded_and_open_with_change() {
        let mut state = crate::app::new(true, None, None);
        state.animations = false;
        // Narrow and short: one column, done steps folded.
        state.window_size = Some((1000.0, 700.0));
        assert!(!two_columns(&state));
        assert!(folded(&state, 1, Step::Done), "done, short: one line");
        assert!(!folded(&state, 1, Step::Open), "not done yet: whole");
        assert!(!folded(&state, 1, Step::Warn), "a warning always shows whole");
        // Folded once, a level that dips stays folded (no jumping as you talk).
        state.quick_folded[1] = true;
        assert!(folded(&state, 1, Step::Open));
        assert!(!folded(&state, 1, Step::Warn), "but too loud opens it");
        // Change opens it for the session.
        let _ = crate::app::update(&mut state, Message::QuickUnfold(1));
        assert!(!folded(&state, 1, Step::Done) && !state.quick_folded[1]);
        // A tall window shows every step whole.
        let mut tall = crate::app::new(true, None, None);
        tall.animations = false;
        tall.window_size = Some((1000.0, 1200.0));
        assert!(!folded(&tall, 0, Step::Done));
    }

    #[test]
    fn a_wide_window_shows_the_steps_in_two_columns_all_open() {
        // The owner, 2026-11-15: "Build both when screen is larger".
        let mut state = crate::app::new(true, None, None);
        state.animations = false;
        state.sidebar_open = true;
        for size in [(1280.0, 700.0), (1440.0, 900.0), (1920.0, 1080.0)] {
            state.window_size = Some(size);
            assert!(two_columns(&state), "{size:?}");
            assert!(!folded(&state, 0, Step::Done), "{size:?}: nothing folds in two columns");
        }
        // The column folded away leaves more room across.
        state.window_size = Some((1000.0, 700.0));
        assert!(!two_columns(&state));
        state.sidebar_open = false;
        assert!(two_columns(&state), "1000 px with the column folded: 924 across");
    }

    use super::*;
    use crate::app::{new, update};

    #[test]
    fn the_words_keep_a_held_peak_only_briefly() {
        // Job 63: with a 9.9 s Peak Hold the words lagged the voice.
        let mut f = MeterFrame::still(0.1);
        f.peak = 0.6;
        f.peak_at = Some(std::time::Instant::now());
        assert_eq!(held_peak(&f, &None), 0.6, "fresh: the held peak");
        f.peak_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(3));
        assert_eq!(held_peak(&f, &None), 0.1, "old: the bar");
    }

    #[test]
    fn too_quiet_says_how_much_gain_it_takes() {
        let db = |d: f32| 10f32.powf(d / 20.0);
        // The owner's: peaks at -30 dBFS with +35 dB of gain.
        assert_eq!(
            quiet_words(db(-30.0), 35, 65, false, true),
            "Too quiet: about 18 dB more gain (to +53 dB) puts your peaks in the band (\u{2212}18 to \u{2212}6 dB), without raising your voice."
        );
        assert!(quiet_words(db(-40.0), 60, 65, false, true).contains("all the way up (+65 dB)"));
        assert!(quiet_words(db(-30.0), 65, 65, false, true).contains("at its top"));
        // Audra's own Pad on: that first.
        assert!(quiet_words(db(-30.0), 35, 65, true, true).contains("Pad (\u{2212}11 dB) is on"));
    }

    #[test]
    fn the_setup_scrolls_to_the_first_and_the_last_stop() {
        // Job 14: a step below the fold got its ring unseen; the setup
        // scrolls to the stop the keyboard is on.
        assert_eq!(scroll_offset(0, 5), 0.0);
        assert_eq!(scroll_offset(4, 5), 1.0);
        assert_eq!(scroll_offset(0, 1), 0.0, "one stop: nowhere to scroll");
        // The setup's stops are the keyboard's, in the order they stand.
        let mut state = new(true, None, None);
        state.view = crate::app::View::Quick;
        let _ = crate::app::view(&state, iced::window::Id::unique());
        let stops = crate::keyfocus::drawn_in(crate::keyfocus::Region::Quick);
        let names: Vec<&str> = stops.iter().map(|r| r.name.as_str()).collect();
        assert!(names.len() > 3, "the setup's stops: {names:?}");
        assert!(names.contains(&"Sound: Clear Calls") && names.contains(&"Voice: shapes how others hear you"), "step 4's keys are stops: {names:?}");
    }

    #[test]
    fn the_level_reads_the_band() {
        let db = |d: f32| 10f32.powf(d / 20.0);
        assert_eq!(level_of(0.0), Level::Silent);
        assert_eq!(level_of(db(-30.0)), Level::Quiet);
        assert_eq!(level_of(db(-12.0)), Level::Good);
        assert_eq!(level_of(db(-4.0)), Level::Hot);
        assert_eq!(level_of(db(-1.0)), Level::TooLoud);
    }

    #[test]
    fn an_open_mic_in_the_speakers_is_found_in_any_view_and_answered() {
        let mut state = new(true, None, None);
        state.view = crate::app::View::Mixer;
        let main = state.control_room.assign.main;
        let phones = *state.control_room.assign.phones_pairs().first().unwrap();
        let an1 = ChannelId::Input(0);
        let _ = update(&mut state, Message::Phantom(0, true));
        let _ = update(&mut state, Message::VolumeChanged(an1, main, 0.5));
        let _ = update(&mut state, Message::VolumeChanged(an1, phones, 0.7));
        assert!(open_mics_in_speakers(&state).contains(&(an1, main)), "AN1 with 48V, live in Main");
        // "Take it out" moves that one send only: not the picked strips,
        // not a Fader group, not the Phones through Trim Gains.
        let in3 = ChannelId::Input(2);
        let _ = update(&mut state, Message::VolumeChanged(in3, main, 0.5));
        state.selected.insert(an1);
        state.selected.insert(in3);
        state.groups[0].members.push(an1);
        state.groups[0].members.push(in3);
        state.groups[0].fader_linked = true;
        let _ = update(&mut state, Message::TrimStripToggle(an1));
        let undo = state.undo_stack.len();
        let _ = update(&mut state, Message::OpenMicTakeOut(an1, main));
        assert_eq!(state.device.volume(an1, main).unwrap(), 0.0);
        assert!((state.device.volume(in3, main).unwrap() - 0.5).abs() < 1e-6, "the picked strip and the group stay");
        assert!((state.device.volume(an1, phones).unwrap() - 0.7).abs() < 1e-6, "the mic stays in the headphones");
        assert_eq!(state.undo_stack.len(), undo + 1, "one undo step");
        assert!(!open_mics_in_speakers(&state).iter().any(|(c, _)| *c == an1));
        let _ = update(&mut state, Message::Undo);
        crate::app::settle_glides(&mut state);
        assert!(state.device.volume(an1, main).unwrap() > 0.0, "Undo brings it back");
        sweep_open_mics(&mut state);
        assert!(state.open_mic_taken.is_none(), "and \"out of Main\" is said no more");
        // "It's meant to be there": that mic in that mix, kept, and
        // remembered between starts; Audra settings warns again.
        let _ = update(&mut state, Message::OpenMicKeep(an1, main));
        sweep_open_mics(&mut state);
        assert!(!open_mics_in_speakers(&state).iter().any(|(c, _)| *c == an1), "kept");
        let saved = crate::app::prefs_of(&state);
        let mut next = new(true, None, None);
        crate::app::apply_prefs(&mut next, saved);
        assert!(next.open_mic_kept.contains(&(an1, main)), "remembered at the next start");
        let _ = update(&mut state, Message::OpenMicForget);
        assert!(open_mics_in_speakers(&state).contains(&(an1, main)), "warned again");
        // Opened and answered from the keyboard: the answers close and the
        // ring moves on to a stop that is there.
        use crate::keyfocus::{CounterItem, KeyFocus};
        let _ = update(&mut state, Message::OpenMicExpand);
        assert!(state.open_mic_expanded);
        state.key_focus = Some(KeyFocus::Counter(CounterItem::OpenMicTakeOut));
        let _ = update(&mut state, Message::OpenMicTakeOut(an1, main));
        assert!(!state.open_mic_expanded, "taken out, the answers close");
        assert!(
            matches!(state.key_focus, Some(KeyFocus::Counter(CounterItem::Level(_) | CounterItem::OpenMicExpand))),
            "the ring left the gone key: {:?}",
            state.key_focus
        );
    }

    #[test]
    fn a_step_newly_done_draws_its_tick_in_and_the_first_look_draws_nothing() {
        let mut state = new(true, None, None);
        state.quick_channel = ChannelId::Input(0);
        state.view = crate::app::View::Quick;
        let _ = update(&mut state, Message::Phantom(0, true));
        watch(&mut state);
        assert!(state.quick_done_at.iter().all(Option::is_none), "already done at the first look: drawn, not drawing");
        let _ = update(&mut state, Message::Phantom(0, false));
        watch(&mut state);
        let _ = update(&mut state, Message::Phantom(0, true));
        watch(&mut state);
        assert!(state.quick_done_at[0].is_some(), "newly done: its tick draws in");
        assert!(state.quick_rewarded[0], "its moment, once this session");
        // Un-ticked and ticked again: no second moment.
        let at = state.quick_done_at[0];
        let _ = update(&mut state, Message::Phantom(0, false));
        watch(&mut state);
        let _ = update(&mut state, Message::Phantom(0, true));
        watch(&mut state);
        assert_eq!(state.quick_done_at[0], at, "once a session");
        // While the meters settle at opening, Quick doesn't look yet: a
        // level already in its band there is found, not newly done.
        let mut fresh = new(true, None, None);
        fresh.view = crate::app::View::Quick;
        fresh.settle_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(60));
        watch(&mut fresh);
        assert!(fresh.quick_prev.is_none(), "no first look while settling");
    }

    #[test]
    fn step_four_draws_its_tick_in_when_a_press_turns_voice_on() {
        let mut state = new(true, None, None);
        state.quick_channel = ChannelId::Input(0);
        state.view = crate::app::View::Quick;
        state.animations = true;
        let chain = state.chain;
        let _ = update(&mut state, Message::QuickSound(0));
        assert!(crate::dynamics_panel::power_of(&state, ChannelId::Input(0)), "a sound starts Voice");
        assert!(state.quick_done_at[3].is_some(), "step 4's tick draws in");
        assert!(crate::motion::tick_drawn(&state, 3) < 1.0);
        assert_eq!(state.chain, chain, "no note of its own: the press made its sound");
        // Another sound while on: nothing new to draw.
        let at = state.quick_done_at[3];
        let _ = update(&mut state, Message::QuickSound(1));
        assert_eq!(state.quick_done_at[3], at);
        // Off, then on with the switch: it draws again.
        let _ = update(&mut state, Message::QuickShape(false));
        state.quick_done_at[3] = None;
        let _ = update(&mut state, Message::QuickShape(true));
        assert!(state.quick_done_at[3].is_some());
        state.animations = false;
        assert_eq!(crate::motion::tick_drawn(&state, 3), 1.0, "at once with animations off");
    }

    #[test]
    fn a_quick_tick_under_a_guide_leaves_the_notes_to_the_guide() {
        let mut state = new(true, None, None);
        state.quick_channel = ChannelId::Input(0);
        state.view = crate::app::View::Quick;
        let _ = update(&mut state, Message::Phantom(0, false));
        watch(&mut state);
        let _ = update(&mut state, Message::GuideStart(3));
        state.view = crate::app::View::Quick;
        let chain = state.chain;
        let _ = update(&mut state, Message::Phantom(0, true));
        watch(&mut state);
        assert!(state.quick_done_at[0].is_some(), "the tick still draws in");
        assert_eq!(state.chain, chain, "no note of its own: the guide's plays");
        state.animations = true;
        assert!(crate::motion::tick_drawn(&state, 0) < 1.0);
        state.animations = false;
        assert_eq!(crate::motion::tick_drawn(&state, 0), 1.0, "at once with animations off");
    }

    #[test]
    fn the_steps_tick_off_from_the_live_mix() {
        let mut state = new(true, None, None);
        state.quick_channel = ChannelId::Input(0);
        // A mic listens on the Phones (PH3/4) unless told otherwise.
        assert_eq!(listen_pair(&state), 1);
        // 48V: off is open, on is done.
        let _ = update(&mut state, Message::Phantom(0, false));
        assert_eq!(steps(&state)[0], Step::Open);
        let _ = update(&mut state, Message::Phantom(0, true));
        assert_eq!(steps(&state)[0], Step::Done);
        // Hear it: nothing sent is open; a send is done; muted is open.
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 1, 0.0));
        assert_eq!(steps(&state)[2], Step::Open);
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 1, 0.5));
        assert_eq!(steps(&state)[2], Step::Done);
        let _ = update(&mut state, Message::Mute(ChannelId::Input(0), true));
        assert_eq!(steps(&state)[2], Step::Open);
    }

    #[test]
    fn all_set_found_at_the_first_look_is_kept_quietly_and_seen_in_quick_is_celebrated() {
        use crate::app::View;
        let db = |d: f32| 10f32.powf(d / 20.0);
        let ready = |view: View, level: bool| {
            let mut state = new(true, None, None);
            state.quick_channel = ChannelId::Input(0);
            state.view = view;
            state.notice = None;
            state.tips = true;
            let _ = update(&mut state, Message::Phantom(0, true));
            let _ = update(&mut state, Message::Mute(ChannelId::Input(0), false));
            let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 1, 0.5));
            if level {
                for m in state.input_meters.iter_mut().take(2) {
                    m.step(db(-12.0));
                }
            }
            // The first run's seeding, then "All set" not yet reached.
            crate::celebrate::check(&mut state);
            state.milestones.retain(|m| m != "quick-all-set");
            state.celebration = None;
            state
        };
        // (a) Already done at Quick's first look: kept, with no card.
        let mut a = ready(View::Quick, true);
        assert!(steps(&a).iter().all(|s| *s == Step::Done), "{:?}", steps(&a));
        watch(&mut a);
        crate::celebrate::check(&mut a);
        assert!(crate::celebrate::reached(&a, "quick-all-set"), "kept");
        assert!(a.celebration.is_none(), "quietly");
        // (b) The level arrives after the first look, in Quick: celebrated.
        let mut b = ready(View::Quick, false);
        watch(&mut b);
        assert_ne!(steps(&b)[1], Step::Done);
        for m in b.input_meters.iter_mut().take(2) {
            m.step(db(-12.0));
        }
        let chain = b.chain;
        watch(&mut b);
        assert!(b.quick_rewarded[1], "a step seen ticking");
        assert_eq!(b.chain, chain, "the last step leaves its note to the milestone's rise");
        crate::celebrate::check(&mut b);
        assert!(crate::celebrate::reached(&b, "quick-all-set"));
        assert_eq!(b.celebration.map(|c| c.reward), Some(crate::celebrate::Reward::Milestone("quick-all-set")));
        // (c) The same in the Mixer: not seen in Quick, not yet.
        let mut c = ready(View::Mixer, false);
        watch(&mut c);
        for m in c.input_meters.iter_mut().take(2) {
            m.step(db(-12.0));
        }
        watch(&mut c);
        crate::celebrate::check(&mut c);
        assert!(!c.quick_rewarded[1]);
        assert!(!crate::celebrate::reached(&c, "quick-all-set"));
        // (d) Milestones off: no rise will come, so the last step chimes
        // itself (it finished in silence; the reviewer, 2026-11-14).
        let mut d = ready(View::Quick, false);
        d.milestone_cards = false;
        watch(&mut d);
        for m in d.input_meters.iter_mut().take(2) {
            m.step(db(-12.0));
        }
        let chain = d.chain;
        watch(&mut d);
        assert!(d.quick_rewarded[1]);
        assert_eq!(d.chain, chain + 1, "its own chime, a step up the ladder");
    }

    #[test]
    fn an_open_mic_on_the_speakers_is_a_warning_not_a_tick() {
        let mut state = new(true, None, None);
        state.quick_channel = ChannelId::Input(0);
        let main = state.control_room.assign.main;
        let sel_before = state.sel_out;
        // Listening on Main (the speakers), 48V on, something sent.
        let _ = update(&mut state, Message::QuickListen(main));
        assert_eq!(state.sel_out, sel_before, "Quick's choice doesn't move the Mixer's mix");
        let _ = update(&mut state, Message::Phantom(0, true));
        let _ = update(&mut state, Message::Mute(ChannelId::Input(0), false));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), main, 0.5));
        assert!(speaker_risk(&state));
        assert_eq!(steps(&state)[2], Step::Warn);
        // On the Phones it is done.
        let phones = state.control_room.assign.phones_pairs()[0];
        let _ = update(&mut state, Message::QuickListen(phones));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), phones, 0.5));
        assert!(!speaker_risk(&state));
        assert_eq!(steps(&state)[2], Step::Done);
    }

    #[test]
    fn a_dynamic_mics_preset_finishes_the_first_step_without_48v() {
        // The reviewer (2026-11-05): with 48V the only way to tick it, a
        // dynamic mic could never finish.
        let mut state = new(true, None, None);
        state.quick_channel = ChannelId::Input(0);
        let _ = update(&mut state, Message::Phantom(0, false));
        assert_eq!(steps(&state)[0], Step::Open, "no preset: 48V off is still to do");
        state.mic_preset_name = "SM7B".into();
        let _ = update(&mut state, Message::MicPresetSave(0));
        assert!(state.mic_presets.iter().any(|m| m.name == "SM7B" && !m.phantom));
        assert_eq!(steps(&state)[0], Step::Done, "its preset says no 48V");
    }
}
