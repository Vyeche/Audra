//! Learning Audra while using it (2026-11-05, the owner: "add a way to
//! learn the system while using it", and keep adding fun).
//!
//! Two things, both shown once and both behind one switch (Audra settings
//! → App, on by default):
//!
//! - **New tricks.** The first time a key or gesture is used, a small card
//!   at the window's foot names it and counts how many of the tricks have
//!   been found ("New trick: Space opens the Settings of the strip you're
//!   on · 5 of 18"). About's list ticks them off. Finding one is the
//!   reward; nothing is asked of anyone.
//! - **Tips.** Doing something the slow way a few times (the gear for
//!   Audra settings, a fader dragged back to 0 dB) brings one tip for the
//!   fast way, once, and never again once that trick is found.
//!
//! The card is not the notice: errors stay sticky there (`app::fail`),
//! and a tip must never stand in front of one.

use std::time::{Duration, Instant};

use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Element, Length};

use crate::app::{Message, TuxMix, View};
use crate::theme;

/// A trick: its id (kept in the prefs) and what it does, in a few words.
pub struct Trick {
    pub id: &'static str,
    pub words: &'static str,
}

pub const TRICKS: [Trick; 22] = [
    Trick { id: "tab", words: "Tab walks the strips and on into the right-hand column" },
    Trick { id: "space", words: "Space opens the Settings of the strip you're on" },
    Trick { id: "ms-keys", words: "M and S mute and solo the strip you're on" },
    Trick { id: "nudge", words: "\u{2191} and \u{2193} nudge the fader you're on" },
    Trick { id: "plus-minus", words: "+ and \u{2212} change the level the keyboard is on" },
    Trick { id: "reset", words: "A double-click (or a Reset key) puts a level back at 0 dB" },
    Trick { id: "select", words: "Ctrl or Shift + click picks strips that move together" },
    Trick { id: "alt-snapshot", words: "Alt+1 \u{2026} 8 recall a snapshot" },
    Trick { id: "step-mix", words: "[ and ] edit the previous and the next mix" },
    Trick { id: "views", words: "Ctrl+Tab steps Quick setup, Mixer and Matrix" },
    Trick { id: "settings-key", words: "F2 opens Audra settings" },
    Trick { id: "help-key", words: "F1 lists every key and gesture" },
    Trick { id: "zoom", words: "Ctrl + and Ctrl \u{2212} (or Ctrl+wheel) zoom" },
    Trick { id: "type-level", words: "Double-click a dB value to type a level" },
    Trick { id: "rename", words: "Double-click a strip's name to rename it" },
    Trick { id: "fold", words: "A strip folds to a narrow rail: double-click it, or press N" },
    Trick { id: "undo", words: "Ctrl+Z undoes, Ctrl+Shift+Z redoes" },
    Trick { id: "matrix-keys", words: "In the Matrix the arrow keys move from cell to cell" },
    Trick { id: "settings-keys", words: "Tab walks Audra settings; \u{2190} \u{2192} turn the one you're on" },
    Trick { id: "panel-keys", words: "Tab goes on into a strip's open EQ or Voice" },
    Trick { id: "quick-keys", words: "Tab walks the Quick setup's controls" },
    Trick { id: "fx-keys", words: "On Reverb & Echo, \u{2190} \u{2192} turn a knob or flip the presets as you listen" },
];

fn words_of(id: &str) -> &'static str {
    TRICKS.iter().find(|t| t.id == id).map_or("", |t| t.words)
}

/// What the card says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    /// A trick just found.
    Found(&'static str),
    /// A faster way to what was just done slowly.
    Tip(&'static str),
    /// What a strip's switch just did, the first time it's switched on
    /// (`note_first`).
    First(&'static str),
}

/// The first time a strip's switch is turned on, what it did, in plain
/// words (critique #56's "what that did"; Strata's list had the same
/// thought): once each ever, kept with the tips shown, behind Tips.
/// The past-0 dB card naming the mix's meter is up: the column rings that
/// meter while it shows (`sidebar::level`), so "watch the meter of the
/// mix you're on" points at one (critique #57's question, recommended yes).
pub fn names_mix_meter(state: &TuxMix) -> bool {
    matches!(state.trick_card, Some((Card::First(id), _)) if id == FIRST_ABOVE_0)
}

/// The past-0 card's words with the mix's meter named, as the column and
/// its strip ring it ("watch Main 1/2's meter, ringed now").
fn above0_words(state: &TuxMix) -> String {
    let name = crate::sidebar::mix_name(state, state.sel_out);
    first_words(FIRST_ABOVE_0).replace("watch the meter of the mix you're on", &format!("watch {name}'s meter (ringed now)"))
}

/// In recordings' line with the effect named, as the Rec key will name it
/// (the card can show away from the Reverb & Echo page: "this effect"
/// left you to guess which).
fn rec_fx_words(state: &TuxMix) -> String {
    match crate::app::take_fx(state) {
        Ok(()) => {
            let w = crate::app::take_fx_words(state);
            let key = if w == "Reverb & Echo" { "FX" } else { w };
            // And the way to a dry take (the owner's yes after critique #107
            // A P3-23: a voice-over take wants none).
            format!("Your next Rec take holds the {w} as you hear it. The Rec key says so: +\u{a0}{key}. For a dry take, switch In recordings off on the Reverb & Echo page.")
        }
        // Each "+ effect" kept on one line (non-breaking space).
        // "Then" pointed at nothing (critique #63 A 3).
        // On a short window, its first sentence only: five lines covered the
        // page keys above the card at 1280x700 (critique #65 B 4).
        Err(Some(_)) if crate::app::foot_compact(state) => format!("A Rec take holds the effects you hear, but right now {}, so takes have none.", crate::app::take_fx_why(state).unwrap_or_default()),
        Err(Some(_)) => format!("A Rec take holds the effects you hear, but right now {}, so takes have none. Once you can hear them, the Rec key says +\u{a0}Reverb, +\u{a0}Echo or +\u{a0}FX.", crate::app::take_fx_why(state).unwrap_or_default()),
        // Both switches off again while the card is up (it's redrawn each
        // frame): say so, not "holds this effect" (OpenCode's job 65).
        Err(None) => "With both In recordings switches off, Rec takes are your mic alone, as others hear it.".to_string(),
    }
}

/// The first amber meter's line with its input named, and the advice that
/// fits it: gain on an input that has one, its source otherwise (critique
/// #62 A 2: seven meters, none named, and "gain" on ADAT).
fn meter_hot_words(state: &TuxMix) -> String {
    let i = state.first_meter_in;
    let cid = tuxmix_core::ChannelId::Input(i);
    let name = crate::app::short_label(&crate::app::strip_display_name(state, cid)).to_string();
    use tuxmix_core::RmeDevice;
    let has_gain = state.device.inputs().get(i).is_some_and(|c| c.gain_max.is_some());
    // Where the gain is, too: the strip's one knob is its pan (critique
    // #91 A P3-7).
    let fix = if has_gain { "turn its gain down (click the dB number at the top of its strip)" } else { "turn it down at its source" };
    format!("{name}'s meter (ringed now) reached amber: it's getting loud. Red, from \u{2212}3 dB, is too loud: {fix}.")
}

/// The input whose meter the first amber line names, ringed on its strip
/// while the card shows (critique #63 A 6, as the past-0 card rings its
/// mix's meter).
pub fn names_hot_meter(state: &TuxMix) -> Option<tuxmix_core::ChannelId> {
    matches!(state.trick_card, Some((Card::First(id), _)) if id == FIRST_METER_HOT).then_some(tuxmix_core::ChannelId::Input(state.first_meter_in))
}

/// Whether pressing Mute FX now would silence something you'd hear: an
/// effect on, and a mix it holds with its FX level above 0.
fn mute_fx_silences(state: &TuxMix) -> bool {
    // Heard there now, by the one model (`fx::heard_in`).
    crate::fx::mute_pairs(state).iter().any(|&p| crate::fx::heard_in(state, p))
}

/// The first Mute FX line with Main named: it silences the effects there,
/// its FX key turns the mute colour, and the Phones keep theirs.
fn mute_fx_words(state: &TuxMix) -> String {
    // The mixes it really silences, Main B too, and the Phones only where
    // they keep theirs (OpenCode's job 69: assigned to Main, they don't).
    let held = crate::fx::mute_pairs(state);
    // The mixes where it took something out (their level kept above 0),
    // the effects that are on, and the Phones only where they really play
    // them (critique #67 A 1: it said all three without looking).
    let silenced: Vec<usize> = held.iter().copied().filter(|&p| crate::fx::silenced_by_mute(state, p)).collect();
    // One voice in the sentence (critique #87 A P3-1: "out of Main 1/2 …
    // your headphones keep theirs").
    let main = silenced
        .iter()
        .map(|&p| if state.plain_names && p == state.control_room.assign.main { "your speakers".to_string() } else { crate::sidebar::mix_name(state, p) })
        .collect::<Vec<_>>()
        .join(" and ");
    let fx = match (state.fx.reverb_on, state.fx.echo_on) {
        (true, false) => "Reverb",
        (false, true) => "Echo",
        _ => "Reverb & Echo",
    };
    // Which Phones keep the room: both, or the one by name (OpenCode's job
    // 70: split across Main and another pair, one loses it).
    let phones_all = state.control_room.assign.phones_pairs();
    let keeping: Vec<usize> = phones_all.iter().copied().filter(|&p| !held.contains(&p) && crate::fx::heard_in(state, p)).collect();
    let phones_keep = !keeping.is_empty();
    let who = if keeping.len() == phones_all.len() {
        if state.plain_names { "Your headphones keep" } else { "The Phones keep" }.to_string()
    } else {
        format!("{} keeps", keeping.iter().map(|&p| crate::sidebar::mix_name(state, p)).collect::<Vec<_>>().join(" and "))
    };
    let key = if silenced.len() > 1 { "their FX keys take" } else { "its FX key takes" };
    // Short on a short window: five lines covered the page keys (as the
    // In recordings line did, critique #65 B 4).
    if crate::app::foot_compact(state) {
        let phones = match (phones_keep, keeping.len() == phones_all.len()) {
            (false, _) => String::new(),
            (true, true) => if state.plain_names { ", and your headphones keep theirs" } else { ", and the Phones keep theirs" }.to_string(),
            (true, false) => format!(", and {who} theirs"),
        };
        return format!("Mute FX took {fx} out of {main}; your levels are kept{phones}.");
    }
    let phones = if phones_keep { format!(" {who} theirs, so you still hear the room {}.", if state.plain_names { "there" } else { "in your headphones" }) } else { String::new() };
    format!("Mute FX took {fx} out of {main} ({key} the mute colour); your levels are kept.{phones}")
}

/// A fader's first time past 0 dB (see `first_of`).
const FIRST_ABOVE_0: &str = "first:above0";
/// The same on an output's own fader: its meter does show it (post-fader).
const FIRST_ABOVE_0_OUT: &str = "first:above0-out";
/// A level past 0 dB by more than a fader's own step (about 0.03 dB).
const ABOVE_UNITY: f32 = 1.004;

fn first_of(message: &Message) -> Option<&'static str> {
    match message {
        Message::Phantom(_, true) => Some("first:48v"),
        Message::Pad(_, true) => Some("first:pad"),
        Message::PhaseChanged(_, true) => Some("first:phase"),
        Message::LoopbackChanged(_, true) => Some("first:loop"),
        Message::CueChanged(_, true) => Some("first:cue"),
        // Strata's list: "the mix sounds missing" after a solo.
        Message::Solo(_, true) => Some("first:solo"),
        // A fader past 0 dB (a level over 1.0) the first time: what that
        // means, and the meter to watch (a caution, so no glow).
        // An input's or playback's meter reads before its fader, so it won't
        // move: that card names the mix's meter (critique #57).
        Message::VolumeChanged(tuxmix_core::ChannelId::Output(_), _, v) if *v > ABOVE_UNITY => Some(FIRST_ABOVE_0_OUT),
        Message::VolumeChanged(_, _, v) | Message::MatrixVolume(_, _, v) if *v > ABOVE_UNITY => Some(FIRST_ABOVE_0),
        // Critique #61's teaching idea: tie the switch to the Rec key's
        // "+ Reverb", which people may miss.
        Message::SetRecordFx(_, true) => Some(FIRST_REC_FX),
        _ => None,
    }
}

/// The first time a side key rings (critique #60's teaching idea): EQ's
/// or D's panel about to open with its effect off.
const FIRST_RING: &str = "first:ring";

/// The first time the faders show FX sends (critique #60's teaching idea:
/// the chain from a send to what you hear).
const FIRST_FX_SENDS: &str = "first:fx-sends";

/// The first time an "In recordings" switch is turned on.
const FIRST_REC_FX: &str = "first:rec-fx";

/// The first time an input's meter reaches amber (critique #61's teaching
/// idea: until then only the first fader past 0 dB spoke of the red).
pub(crate) const FIRST_METER_HOT: &str = "first:meter-hot";

/// Where the meters' amber starts (`fader::METER_ZONES`): -6 dB.
const AMBER: f32 = 0.501;

/// Called with the inputs' fresh readings on each meter tick: the first
/// time one reaches amber on the unit, what amber and red mean. Not in the
/// simulation, whose made-up levels would bring it up in every capture,
/// nor in Quick setup, whose meter has words of its own.
pub fn note_meter(state: &mut TuxMix, in_levels: &[f32]) {
    if !state.device.is_mock() {
        meter_hot(state, in_levels);
    }
}

fn meter_hot(state: &mut TuxMix, in_levels: &[f32]) {
    let Some(hot) = in_levels.iter().position(|l| *l >= AMBER) else { return };
    // Not a press: it never takes the slot from another card, nor shows
    // where it can't be seen: behind the clipping card or a celebration
    // (ranked above it, `card_beneath`), or with the window hidden
    // (critique #62 A 3: spent unseen).
    if !state.tips
        || state.guide.is_some()
        || state.trick_card.is_some()
        // Only where the ring it names is drawn (critique #64 A 5: from
        // the Matrix "(ringed now)" pointed at nothing); Quick's meter has
        // words of its own.
        || state.view != View::Mixer
        || state.celebration.is_some()
        || state.window_hidden
        || state.window_id.is_none()
        || matches!(crate::assist::current(state), Some(crate::assist::Assist::Clipping(_)))
        || state.tips_shown.iter().any(|f| f == FIRST_METER_HOT)
    {
        return;
    }
    // Named and ringed by its strip: a linked pair's right side is its
    // left's strip.
    use tuxmix_core::RmeDevice;
    state.first_meter_in = if hot % 2 == 1 && state.device.input_pair_linked(hot / 2) { hot - 1 } else { hot };
    state.tips_shown.push(FIRST_METER_HOT.to_string());
    state.prefs_dirty = true;
    state.trick_card = Some((Card::First(FIRST_METER_HOT), Instant::now()));
}

/// The first time Mute FX goes on (critique #65's teaching idea): what
/// went quiet, and what didn't.
const FIRST_MUTE_FX: &str = "first:mute-fx";

fn first_ring(state: &TuxMix, message: &Message) -> Option<&'static str> {
    use crate::widgets::strip::FlyoutKind;
    // Decided before the press lands: off now means it's going on.
    // Only once it silences something you'd hear (critique #67 A 1: on a
    // fresh first press nothing was on, and every clause was false).
    if matches!(message, Message::MuteFxToggle) && state.control_room.fx_muted.is_none() {
        return mute_fx_silences(state).then_some(FIRST_MUTE_FX);
    }
    if matches!(message, Message::FaderFxToggle) && !state.fader_fx {
        return Some(FIRST_FX_SENDS);
    }
    let Message::ToggleFlyout(cid, kind) = message else { return None };
    if state.flyout_open.contains(&(*cid, *kind)) {
        return None; // closing
    }
    // Any side key's first ring (critique #61 A 3: the wrench, which
    // rings every time, is the one met first).
    let off = match kind {
        // Not from the FX key: the wrench stays plain then (job 63).
        FlyoutKind::Settings => !state.fx_opening,
        FlyoutKind::Eq => !crate::app::strip_params(state, *cid, state.sel_out).eq_enabled,
        FlyoutKind::Dynamics => !crate::dynamics_panel::lit(state, *cid),
        _ => return None,
    };
    off.then_some(FIRST_RING)
}

pub fn first_words(id: &str) -> &'static str {
    match id {
        // About the strip's keys only (critique #61 A 4: an outline means
        // other things elsewhere).
        FIRST_RING => "On a strip, an outlined key means its panel is open; a solid one means it's on.",
        // The FX control as drawn: a key on a short strip, a small fader on
        // a tall one (critique #61 A 2: no green fader showed at 900 px).
        FIRST_FX_SENDS => "The faders now set how much of each strip goes to Reverb & Echo (amber). You hear the effect through each output's FX return, set by the FX control on that output's strip.",
        FIRST_REC_FX => "Your next Rec take holds this effect as you hear it. The Rec key says what's included: + Reverb, + Echo or + FX.",
        FIRST_METER_HOT => "An input's meter reached amber: it's getting loud. Red, from \u{2212}3 dB, is too loud: turn that input's gain down.",
        "first:48v" => "48V powers a condenser mic through its cable. A dynamic mic doesn't need it.",
        "first:pad" => "PAD makes this input 11 dB quieter before the gain, for loud sources.",
        "first:phase" => "Phase flips the signal: for a mic that sounds thin with another one open.",
        // Led by what it's for (critique #59: both read circular).
        "first:loop" => "LOOP sends an output\u{2019}s mix back to the computer, to record it (a call, a stream): apps see it as the input of the same name.",
        "first:cue" => "Cue lets you listen to this output's mix in your headphones for a moment, to check it (where it plays: Audra settings, Monitor).",
        "first:solo" => "Solo lets you hear just the soloed strips; the mix itself doesn't change. Press S again to hear everything.",
        FIRST_ABOVE_0 => "Past 0 dB a fader adds level, up to +6 dB. This strip's meter shows what comes in, so watch the meter of the mix you're on: if it reaches the red, it's too loud, bring it back down.",
        FIRST_ABOVE_0_OUT => "Past 0 dB this output plays louder than its mix, up to +6 dB. Its meter shows it: if it reaches the red, it's too loud, bring it back down.",
        _ => "",
    }
}

/// Called once a press is decided (`app::update`): a switch's first time
/// on shows its card.
pub fn note_first(state: &mut TuxMix, message: &Message) {
    // Mute FX released while its first line is up: the line goes with it
    // (critique #68 A 3: redrawn, it lost its mix: "out of  (its FX key…").
    if matches!(message, Message::MuteFxToggle) && state.control_room.fx_muted.is_some() && matches!(state.trick_card, Some((Card::First(id), _)) if id == FIRST_MUTE_FX) {
        state.trick_card = None;
    }
    let Some(id) = first_of(message).or_else(|| first_ring(state, message)) else { return };
    // Not from a remote surface: the hand isn't on the screen.
    if !state.tips || state.from_remote || state.guide.is_some() || state.tips_shown.iter().any(|f| f == id) {
        return;
    }
    // A trick's card still up keeps its slot: this one waits for the next
    // time, unseen and so unmarked (Strata's review).
    if matches!(state.trick_card, Some((Card::Found(_), _))) {
        return;
    }
    state.tips_shown.push(id.to_string());
    state.prefs_dirty = true;
    state.trick_card = Some((Card::First(id), Instant::now()));
    // Sparks say "well done": not for a caution (past 0 dB), nor for 48V
    // (power into a mic that may not want it) or a first Solo (whose card
    // explains why the mix sounds missing). Critique #58, recommended.
    // PAD too (critique #59, recommended): it's a setting, not a win.
    // Nor for the ring's line: a way to read the keys, not a win.
    // Nor for In recordings: a setting put back, not a win.
    // Nor for Mute FX: a listening choice, not a win.
    if state.small_wins && ![FIRST_ABOVE_0, FIRST_ABOVE_0_OUT, "first:48v", "first:solo", "first:pad", FIRST_RING, FIRST_FX_SENDS, FIRST_REC_FX, FIRST_MUTE_FX].contains(&id) {
        if let Some(cid) = crate::motion::strip_of(message) {
            state.win_spark = Some((cid, Instant::now()));
        }
    }
}

/// How long a card stays (while not pointed at).
pub const CARD_FOR: Duration = Duration::from_secs(7);

/// The trick a message from the keyboard (or a gesture with a message of
/// its own) shows. Keyboard paths that share their message with a click
/// arrive wrapped in `Message::KeyShortcut` with their id instead.
pub fn trick_of(state: &TuxMix, message: &Message) -> Option<&'static str> {
    let on_strip = state.focused_strip.is_some() && state.key_focus.is_none();
    Some(match message {
        Message::FocusTab(_) if state.show_settings => "settings-keys",
        Message::FocusTab(_) if state.view == View::Quick => "quick-keys",
        // Tab on from an item of a strip's open EQ or Voice.
        Message::FocusTab(_) if state.view == View::Mixer && matches!(state.key_focus, Some(crate::keyfocus::KeyFocus::Item(_))) => "panel-keys",
        Message::FocusTab(_) if state.view == View::Mixer => "tab",
        Message::FocusTab(_) | Message::StripFocusStep(_) | Message::FaderNudge(_) if state.view == View::Matrix => "matrix-keys",
        Message::KeyLetter(' ') if on_strip => "space",
        Message::KeyLetter('m' | 's') if on_strip => "ms-keys",
        Message::FaderNudge(_) => "nudge",
        Message::KeyAdjust(_) => "plus-minus",
        Message::Reset(..) | Message::MatrixSet(_, _, _, true) => "reset",
        Message::StripClicked(_) if state.modifiers.control() || state.modifiers.shift() => "select",
        Message::StepMix(_) => "step-mix",
        Message::EditStart(..) => "type-level",
        Message::RenameStart(_) => "rename",
        Message::ToggleCollapse(_) => "fold",
        Message::KeyShortcut(id, _) => id,
        Message::WheelZoom(_) if state.modifiers.control() => "zoom",
        _ => return None,
    })
}

/// The slow ways, and the trick each is the fast way for: how many times
/// before the tip, and the tip's id. Only unwrapped (clicked) messages
/// count; their keyboard twins arrive as `KeyShortcut`.
fn slow_way(state: &TuxMix, message: &Message) -> Option<(&'static str, u8)> {
    match message {
        Message::ToggleSettings => Some(("settings-key", 3)),
        Message::SnapshotSlotPressed(_) => Some(("alt-snapshot", 3)),
        Message::SetView(v) if *v != state.view => Some(("views", 4)),
        Message::Undo | Message::Redo => Some(("undo", 3)),
        // A strip's Settings by its key or a right-click, not by Space.
        Message::ToggleFlyout(_, crate::widgets::strip::FlyoutKind::Settings) => Some(("space", 3)),
        // A fader dragged back to exactly 0 dB (it snaps there).
        Message::VolumeChanged(_, _, v) if (*v - 1.0).abs() < 1e-6 => Some(("reset", 2)),
        _ => None,
    }
}

/// Watches each message the user sends (`app::update`, before it runs):
/// a trick found for the first time shows its card, a slow way done often
/// enough shows its tip. Both are kept in the prefs.
pub fn note(state: &mut TuxMix, message: &Message) {
    if let Some(id) = trick_of(state, message) {
        if !state.tricks_found.iter().any(|f| f == id) {
            state.tricks_found.push(id.to_string());
            state.prefs_dirty = true;
            // While a guide is under way its own notes are the reward:
            // the find is counted, quietly.
            // Close together, the card's words grow (the reviewer,
            // 2026-11-14: every find read the same).
            bump_streak(state);
            // The card with Tips; the sparks and note with Small wins, each
            // its own switch (the owner's pick, 2026-11-16).
            if state.tips && state.guide.is_none() {
                state.trick_card = Some((Card::Found(id), Instant::now()));
            }
            if state.small_wins && state.guide.is_none() {
                // Lit where it happened: the strip the message acts on, or
                // the one the keyboard is on.
                if let Some(cid) = crate::motion::strip_of(message).or(state.focused_strip) {
                    state.win_spark = Some((cid, Instant::now()));
                }
                // A small win: one soft note, a step up the session's chain
                // (`celebrate::next_win`; a trick climbs the pitch but
                // doesn't count toward a run of milestones), under the
                // celebration sounds' switch and the top bar's speaker.
                // It rings when its card shows (`expire`), not behind a
                // notice with nothing to see.
                let (step, _) = crate::celebrate::next_win(state, false);
                if state.trick_card.is_some() {
                    state.trick_chime_due = Some(step);
                } else if crate::celebrate::sounds_on(state) {
                    // No card to wait for (Tips off): the note now.
                    crate::sounds::play(crate::sounds::Sound::Chime(step.min(9)), state.ui_sound_db - 4.0);
                }
            }
        }
        return;
    }
    if state.via_key {
        return;
    }
    let Some((id, times)) = slow_way(state, message) else {
        return;
    };
    if state.tricks_found.iter().any(|f| f == id) || state.tips_shown.iter().any(|f| f == id) {
        return;
    }
    let n = state.slow_counts.entry(id).or_insert(0);
    *n = n.saturating_add(1);
    if *n >= times && state.tips && state.guide.is_none() {
        state.tips_shown.push(id.to_string());
        state.prefs_dirty = true;
        state.trick_card = Some((Card::Tip(id), Instant::now()));
    }
}

/// How close together finds must be for the card's words to grow.
pub const STREAK_GAP: std::time::Duration = std::time::Duration::from_secs(120);

/// Counts a find into the run of finds close together.
fn bump_streak(state: &mut TuxMix) {
    let (n, last) = state.trick_streak;
    let close = last.is_some_and(|at| at.elapsed() < STREAK_GAP);
    state.trick_streak = (if close { n.saturating_add(1) } else { 1 }, Some(Instant::now()));
}

/// The card's lead for a find, by how many came close together.
pub fn found_kicker(streak: u8) -> &'static str {
    match streak {
        0 | 1 => "New trick:",
        2 => "Another one:",
        _ => "On a roll:",
    }
}

/// The newest segment's arrival on the card's ladder (as a milestone
/// ladder's newest lights, `celebrate::cascade`): how lit it is (0 to 1,
/// over `celebrate::SEG_LIGHT_MS`) and its bloom after (1 to 0, over
/// `celebrate::BLOOM_MS`). Lit and still with Animations off.
pub fn newest_arrival(state: &TuxMix) -> (f32, f32) {
    let Some((Card::Found(_), at)) = state.trick_card else { return (1.0, 0.0) };
    if !state.animations || state.notice.is_some() {
        return (1.0, 0.0);
    }
    let ms = at.elapsed().as_secs_f32() * 1000.0;
    let light = crate::motion::ease_out(ms / crate::celebrate::SEG_LIGHT_MS);
    let bloom = if ms < crate::celebrate::SEG_LIGHT_MS {
        0.0
    } else {
        1.0 - crate::motion::ease_out((ms - crate::celebrate::SEG_LIGHT_MS) / crate::celebrate::BLOOM_MS)
    };
    (light, bloom)
}

/// Whether the newest segment is still arriving (`motion::running`).
pub fn arriving(state: &TuxMix) -> bool {
    let (light, bloom) = newest_arrival(state);
    light < 1.0 || bloom > 0.0
}

/// Lets the card go once its time is up (`Message::Tick`).
pub fn expire(state: &mut TuxMix) {
    // Waiting behind a notice, a milestone's card or a clipping input's
    // card (the cards ranked above it), its time and its chime wait.
    let clipping = matches!(crate::assist::current(state), Some(crate::assist::Assist::Clipping(_)));
    if state.notice.is_some() || state.celebration.is_some() || clipping {
        if let Some((card, _)) = state.trick_card {
            state.trick_card = Some((card, Instant::now()));
        }
        return;
    }
    // Its card in sight: its chime now.
    if let Some(step) = state.trick_chime_due.take() {
        if state.trick_card.is_some() && crate::celebrate::sounds_on(state) {
            crate::sounds::play(crate::sounds::Sound::Chime(step.min(9)), state.ui_sound_db - 4.0);
        }
    }
    if !state.trick_card_held && state.trick_card.is_some_and(|(_, at)| at.elapsed() > CARD_FOR) {
        state.trick_card = None;
    }
}

/// How many tricks have been found, and of how many.
pub fn found_count(state: &TuxMix) -> (usize, usize) {
    let n = TRICKS.iter().filter(|t| state.tricks_found.iter().any(|f| f == t.id)).count();
    (n, TRICKS.len())
}

/// Whether trick `id` has been found (About ticks it).
pub fn found(state: &TuxMix, id: &str) -> bool {
    state.tricks_found.iter().any(|f| f == id)
}

/// The card at the window's foot, left of the notice: what was found or
/// what is faster, in one line, the tricks found as a row of lit
/// segments, a door to every key (F1) and its close. It waits while a
/// notice shows, so the two never stand side by side (the reviewer,
/// 2026-11-05).
pub fn card(state: &TuxMix) -> Option<Element<'_, Message>> {
    if state.notice.is_some() {
        return None;
    }
    let (card, _) = state.trick_card?;
    let scale = state.ui_scale;
    let (kicker, id, newest) = match card {
        Card::Found(id) => (found_kicker(state.trick_streak.0), id, Some(id)),
        Card::Tip(id) => ("Tip:", id, None),
        // No press brought the meter's line: it isn't "what that did".
        Card::First(id) if id == FIRST_METER_HOT => ("Good to know:", id, None),
        Card::First(id) => ("What that did:", id, None),
    };
    let words = match card {
        Card::Found(_) => words_of(id).to_string(),
        // The past-0 card names the meter it rings (critique #59: "the
        // mix you're on" left you to find which).
        Card::First(_) if id == FIRST_ABOVE_0 => above0_words(state),
        Card::First(_) if id == FIRST_REC_FX => rec_fx_words(state),
        Card::First(_) if id == FIRST_METER_HOT => meter_hot_words(state),
        Card::First(_) if id == FIRST_MUTE_FX => mute_fx_words(state),
        Card::First(_) => first_words(id).to_string(),
        Card::Tip(_) => match id {
            "space" => "Tab to a strip, then Space opens its Settings".to_string(),
            "reset" => "Double-click a fader to put it back at 0 dB".to_string(),
            _ => words_of(id).to_string(),
        },
    };
    let lamp = theme::readable_on_floors(theme::lamp());
    // Laid out for the counter's slot (`app::foot_card`): "New trick:" run
    // into the trick, as one sentence pulled to read (not an eyebrow over
    // it), the × at the right of its first line; then the ladder and the
    // way to every key.
    let line = iced::widget::rich_text::<(), Message, iced::Theme, iced::Renderer>([
        iced::widget::span(kicker).font(theme::SANS_SEMI).color(lamp),
        iced::widget::span(" "),
        iced::widget::span(words).color(theme::text_primary()),
    ])
    .size(theme::TEXT_SM * scale)
    .width(Length::Fill);
    // On a short window, the sentence and its × only (critique #7).
    // A first-time line is the sentence alone, as on a short window.
    if crate::app::foot_compact(state) || matches!(card, Card::First(_)) {
        let body = container(
            row![
                line,
                crate::widgets::strip::hint(
                    button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                        .padding(0)
                        .width(Length::Fixed(20.0 * scale))
                        .height(Length::Fixed(20.0 * scale))
                        .style(theme::icon_button(false))
                        .on_press(Message::TrickCardClose),
                    "Close (Esc; Help lists every key and gesture)",
                    scale,
                ),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        )
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .width(Length::Fill)
        .style(theme::trick_card);
        return Some(
            mouse_area(body)
                .on_enter(Message::TrickCardHeld(true))
                .on_exit(Message::TrickCardHeld(false))
                .into(),
        );
    }
    let body = container(
        column![
            row![
                line,
                crate::widgets::strip::hint(
                    button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                        .padding(0)
                        .width(Length::Fixed(20.0 * scale))
                        .height(Length::Fixed(20.0 * scale))
                        .style(theme::icon_button(false))
                        .on_press(Message::TrickCardClose),
                    "Close (Esc; tips can be switched off in Audra settings \u{2192} App)",
                    scale,
                ),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Start),
            row![
                ladder(state, newest, scale),
                // The count in words, as the guides' and milestones' cards
                // say theirs (critique #55: the pips alone, 1.7:1 unlit,
                // were the only way to see how many were left).
                text({
                    let (n, of) = found_count(state);
                    format!("{n} of {of}")
                })
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec()),
                iced::widget::Space::new().width(Length::Fill),
                button(text("All keys").size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                    .style(theme::page_key(11.0 * scale))
                    .on_press(Message::ToggleAbout),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(theme::SPACE_SM * scale),
    )
    .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
    .width(Length::Fill)
    .style(theme::trick_card);
    Some(
        mouse_area(body)
            .on_enter(Message::TrickCardHeld(true))
            .on_exit(Message::TrickCardHeld(false))
            .into(),
    )
}

/// The tricks found as a row of LED segments, one per trick, in the
/// meters' manner: unlit ghosts, lit in `signal`, the one just found in
/// the lamp's amber. A collection that fills slowly, and the count in
/// its tooltip.
pub fn ladder(state: &TuxMix, newest: Option<&'static str>, scale: f32) -> Element<'static, Message> {
    let lit: Vec<Option<bool>> = TRICKS
        .iter()
        .map(|t| {
            if Some(t.id) == newest {
                Some(true)
            } else if found(state, t.id) {
                Some(false)
            } else {
                None
            }
        })
        .collect();
    // Filled from the left, the newest at the end of the run (as
    // `celebrate::fill_from_left`): which trick is which is in Help.
    let mut lit = lit;
    lit.sort_by_key(|l| match l {
        Some(false) => 0,
        Some(true) => 1,
        None => 2,
    });
    let (n, of) = found_count(state);
    crate::widgets::strip::hint_text(
        iced::widget::canvas(Ladder { lit, scale, arrival: if newest.is_some() { newest_arrival(state) } else { (1.0, 0.0) } })
            .width(Length::Fixed(Ladder::width(scale)))
            .height(Length::Fixed(10.0 * scale)),
        format!("{n} of {of} tricks found"),
        scale,
    )
}

struct Ladder {
    /// Per trick: unlit (`None`), lit (`Some(false)`), just found (`Some(true)`).
    lit: Vec<Option<bool>>,
    scale: f32,
    /// The newest segment's light and bloom (`newest_arrival`).
    arrival: (f32, f32),
}

impl Ladder {
    const SEG_W: f32 = 4.0;
    const GAP: f32 = 2.0;
    fn width(scale: f32) -> f32 {
        (TRICKS.len() as f32 * (Self::SEG_W + Self::GAP) - Self::GAP) * scale
    }
}

/// The unlit pip and the newest one's lamp, each the first step toward
/// the ink that reads at 3:1 on the trick card's face.
fn pip_colors(theme: &iced::Theme) -> (iced::Color, iced::Color) {
    let ground = match theme::dialog(theme).background {
        Some(iced::Background::Color(c)) => c,
        _ => theme::surface(),
    };
    let firm = |from: iced::Color, toward: iced::Color| {
        (0..=20)
            .map(|k| theme::blend(from, toward, k as f32 / 20.0))
            .find(|c| theme::contrast(*c, ground) >= 3.0)
            .unwrap_or(toward)
    };
    (firm(theme::blend(ground, theme::text_sec(), 0.45), theme::text_sec()), firm(theme::lamp(), theme::text_primary()))
}

impl<M> iced::widget::canvas::Program<M> for Ladder {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        let mut frame = iced::widget::canvas::Frame::new(renderer, bounds.size());
        let s = self.scale;
        for (i, state) in self.lit.iter().enumerate() {
            let x = i as f32 * (Self::SEG_W + Self::GAP) * s;
            // Unlit firm enough to see (0.22 measured 1.7:1 on the dark card,
            // 0.32 2.25:1; critiques #55, #56;
            // the count beside the ladder says the number).
            // Solid, at 3:1 on the card itself (critique #57: 0.45 of the
            // secondary text read 2.30 on Daylight's white card, the raw
            // amber 2.03).
            let (ghost, newest) = pip_colors(_theme);
            let color = match state {
                // From the ghost up to the lamp as it arrives.
                Some(true) => theme::blend(ghost, newest, self.arrival.0),
                Some(false) => theme::signal(),
                None => ghost,
            };
            // Its bloom: a glow round it, as the toggles' bloom.
            if *state == Some(true) && self.arrival.1 > 0.0 {
                let pad = 2.0 * s * self.arrival.1;
                frame.fill(
                    &iced::widget::canvas::Path::rounded_rectangle(
                        iced::Point::new(x - pad, -pad),
                        iced::Size::new(Self::SEG_W * s + 2.0 * pad, bounds.height + 2.0 * pad),
                        (2.0 * s).into(),
                    ),
                    iced::Color { a: 0.45 * self.arrival.1, ..theme::lamp() },
                );
            }
            frame.fill(
                &iced::widget::canvas::Path::rounded_rectangle(
                    iced::Point::new(x, 0.0),
                    iced::Size::new(Self::SEG_W * s, bounds.height),
                    (1.0 * s).into(),
                ),
                color,
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, update};

    #[test]
    fn the_first_fx_sends_on_the_faders_says_where_the_effect_is_heard() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FaderFxToggle);
        assert!(state.fader_fx, "the mock has the effects' channels");
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_FX_SENDS)));
        assert!(first_words(FIRST_FX_SENDS).contains("FX return"));
    }

    #[test]
    fn the_first_mute_fx_says_what_went_quiet_and_what_didnt() {
        let mut state = new(true, None, None);
        // Critique #67 A 1: nothing on, nothing silenced: no line yet, and
        // the line stays for later.
        let _ = update(&mut state, Message::MuteFxToggle);
        assert!(state.trick_card.is_none(), "nothing to silence");
        let _ = update(&mut state, Message::MuteFxToggle);
        let main = state.control_room.assign.main;
        let phones = state.control_room.assign.phones_pairs()[0];
        heard_reverb(&mut state, main);
        let _ = update(&mut state, Message::MuteFxToggle);
        assert!(state.control_room.fx_muted.is_some());
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_MUTE_FX)));
        let words = mute_fx_words(&state);
        assert!(words.contains("took Reverb") && words.contains("your speakers"), "{words}");
        assert!(!words.contains("Phones") && !words.contains("headphones"), "the Phones play no room yet: {words}");
        crate::fx::set_return(&mut state, phones, crate::fx::db_vol(-12.0));
        assert!(mute_fx_words(&state).contains("headphones keep theirs"));
        assert!(state.win_spark.is_none(), "a listening choice, no sparks");
        // Released while the line is up: the line goes with it (critique
        // #68 A 3), and on again: once ever.
        let _ = update(&mut state, Message::MuteFxToggle);
        assert!(state.trick_card.is_none(), "released: the line goes");
        let _ = update(&mut state, Message::MuteFxToggle);
        assert!(state.trick_card.is_none());
    }

    /// Reverb on and heard in `pair`: AN1 up there, the send up, the
    /// return up (the one model, `fx::block_in`).
    fn heard_reverb(state: &mut TuxMix, pair: usize) {
        state.fx.reverb_on = true;
        let _ = update(state, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), pair, 0.5));
        let _ = update(state, Message::FxSend(-10.0));
        crate::fx::set_return(state, pair, crate::fx::db_vol(-12.0));
        crate::app::settle_glides(state);
        assert!(crate::fx::heard_in(state, pair), "{:?}", crate::fx::block_in(state, pair));
    }

    #[test]
    fn the_mute_fx_line_names_what_it_silences_and_no_more() {
        // OpenCode's job 69: Main B goes quiet too; Phones on Main don't
        // keep theirs.
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        heard_reverb(&mut state, main);
        crate::fx::set_return(&mut state, 2, crate::fx::db_vol(-12.0));
        state.control_room.assign.main_b = Some(2);
        crate::fx::set_mute(&mut state, true);
        let words = mute_fx_words(&state);
        assert!(words.contains("your speakers") && words.contains(&crate::sidebar::mix_name(&state, 2)), "{words}");
        let _ = main;
        assert!(words.contains("their FX keys"), "{words}");
        crate::fx::set_mute(&mut state, false);
        // Split Phones (OpenCode's job 70): one on Main, one free: the free
        // one by name, not "the Phones".
        state.control_room.assign.main_b = None;
        let free = state.control_room.assign.phones_pairs()[0];
        crate::fx::set_return(&mut state, free, crate::fx::db_vol(-12.0));
        state.control_room.assign.phones = [Some(main), Some(free)];
        crate::fx::set_mute(&mut state, true);
        let words = mute_fx_words(&state);
        assert!(words.contains(&format!("{} keeps theirs", crate::sidebar::mix_name(&state, free))), "{words}");
        crate::fx::set_mute(&mut state, false);
        state.control_room.assign.phones = [Some(main), None];
        crate::fx::set_mute(&mut state, true);
        let words = mute_fx_words(&state);
        assert!(!words.contains("Phones"), "{words}");
    }

    #[test]
    fn the_first_amber_meter_says_what_amber_and_red_mean_once() {
        let mut state = new(true, None, None);
        state.window_id = Some(iced::window::Id::unique());
        meter_hot(&mut state, &[0.3, 0.2]);
        assert!(state.trick_card.is_none(), "green: no line");
        meter_hot(&mut state, &[0.3, 0.6]);
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_METER_HOT)));
        // Critique #62 A 2: the input named, and its gain where it has one.
        let words = meter_hot_words(&state);
        assert!(words.starts_with("AN2") && words.contains("ringed now") && words.contains("\u{2212}3 dB") && words.contains("gain"), "{words}");
        assert_eq!(names_hot_meter(&state), Some(tuxmix_core::ChannelId::Input(1)), "its meter ringed while the card shows");
        assert!(state.win_spark.is_none(), "a caution, no sparks");
        // Once ever.
        state.trick_card = None;
        meter_hot(&mut state, &[0.9]);
        assert!(state.trick_card.is_none());
    }

    #[test]
    fn an_input_with_no_gain_is_told_to_turn_down_its_source() {
        let mut state = new(true, None, None);
        state.window_id = Some(iced::window::Id::unique());
        use tuxmix_core::RmeDevice;
        let i = state.device.inputs().iter().position(|c| c.gain_max.is_none()).expect("a line input with no gain");
        let mut levels = vec![0.0; i + 1];
        levels[i] = 0.7;
        meter_hot(&mut state, &levels);
        let words = meter_hot_words(&state);
        assert!(words.contains("at its source") && !words.contains("gain"), "{words}");
    }

    #[test]
    fn the_amber_line_is_never_spent_unseen() {
        // Critique #62 A 3: hidden, or behind a celebration, it waits.
        let mut state = new(true, None, None);
        meter_hot(&mut state, &[0.6]);
        assert!(state.trick_card.is_none(), "no window yet");
        state.window_id = Some(iced::window::Id::unique());
        state.window_hidden = true;
        meter_hot(&mut state, &[0.6]);
        assert!(state.trick_card.is_none(), "in the tray");
        assert!(!state.tips_shown.iter().any(|f| f == FIRST_METER_HOT), "kept for later");
        state.window_hidden = false;
        meter_hot(&mut state, &[0.6]);
        assert!(state.trick_card.is_some(), "seen now");
    }

    #[test]
    fn the_amber_line_waits_for_another_card_and_skips_quick_setup() {
        let mut state = new(true, None, None);
        state.window_id = Some(iced::window::Id::unique());
        state.trick_card = Some((Card::Tip("space"), Instant::now()));
        meter_hot(&mut state, &[0.6]);
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::Tip("space")), "never takes the slot");
        state.trick_card = None;
        state.view = View::Quick;
        meter_hot(&mut state, &[0.6]);
        assert!(state.trick_card.is_none(), "Quick's meter has its own words");
        state.view = View::Matrix;
        meter_hot(&mut state, &[0.6]);
        assert!(state.trick_card.is_none(), "no ring to point at in the Matrix");
        assert!(!state.tips_shown.iter().any(|f| f == FIRST_METER_HOT), "unseen, so not marked");
        // The simulation never brings it (its levels are made up).
        state.view = View::Mixer;
        note_meter(&mut state, &[0.9]);
        assert!(state.trick_card.is_none());
    }

    #[test]
    fn in_recordings_back_on_names_the_rec_key() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetRecordFx(crate::fx::Unit::Reverb, false));
        assert!(state.trick_card.is_none(), "off: no line");
        let _ = update(&mut state, Message::SetRecordFx(crate::fx::Unit::Reverb, true));
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_REC_FX)));
        assert!(first_words(FIRST_REC_FX).contains("+ Reverb"));
        assert!(state.win_spark.is_none());
        // The simulation's effects start off and run no loop: the card
        // says why, honestly.
        let words = rec_fx_words(&state);
        assert!(words.contains("right now") && words.contains("+\u{a0}Reverb"), "{words}");
    }

    #[test]
    fn the_in_recordings_line_never_claims_an_effect_with_both_switches_off() {
        // OpenCode's job 65: switched on, then off again while the card shows.
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetRecordFx(crate::fx::Unit::Reverb, false));
        let _ = update(&mut state, Message::SetRecordFx(crate::fx::Unit::Echo, false));
        let _ = update(&mut state, Message::SetRecordFx(crate::fx::Unit::Reverb, true));
        let _ = update(&mut state, Message::SetRecordFx(crate::fx::Unit::Reverb, false));
        let words = rec_fx_words(&state);
        assert!(words.contains("mic alone") && !words.contains("holds"), "{words}");
    }

    #[test]
    fn the_in_recordings_line_names_the_effect_a_take_will_hold() {
        let mut state = new(true, None, None);
        state.fx.reverb_on = true;
        state.fx.echo_on = false;
        let words = rec_fx_words(&state);
        // No loop here, so the "once you hear it" form; with both off, say so.
        state.fx.reverb_on = false;
        let off = rec_fx_words(&state);
        assert!(off.contains("Reverb and Echo are off") || off.contains("the Reverb is off"), "{off}");
        assert_ne!(words, off);
    }

    #[test]
    fn an_fx_key_opening_settings_brings_no_ring_line() {
        // Job 63: opened by its FX key, the wrench draws no ring.
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::OpenFx(tuxmix_core::ChannelId::Output(0)));
        assert_ne!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_RING)));
        assert!(!state.fx_opening, "the flag is let go");
    }

    #[test]
    fn the_first_outlined_side_key_says_what_the_outline_means() {
        use crate::widgets::strip::FlyoutKind;
        let mut state = new(true, None, None);
        let cid = tuxmix_core::ChannelId::Input(0);
        assert!(!crate::dynamics_panel::lit(&state, cid), "Voice starts off");
        let _ = update(&mut state, Message::ToggleFlyout(cid, FlyoutKind::Dynamics));
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_RING)));
        assert!(state.win_spark.is_none(), "a way to read the keys, no sparks");
        // Once ever: closed and opened again, no card.
        state.trick_card = None;
        let _ = update(&mut state, Message::ToggleFlyout(cid, FlyoutKind::Dynamics));
        let _ = update(&mut state, Message::ToggleFlyout(cid, FlyoutKind::Dynamics));
        assert!(state.trick_card.is_none());
    }

    #[test]
    fn a_key_found_shows_its_card_once_and_counts() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::KeyShortcut("settings-key", Box::new(Message::ToggleSettings)));
        assert!(state.show_settings, "the key still does its job");
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::Found("settings-key")));
        assert_eq!(found_count(&state).0, 1);
        state.trick_card = None;
        let _ = update(&mut state, Message::KeyShortcut("settings-key", Box::new(Message::ToggleSettings)));
        assert!(state.trick_card.is_none(), "found once, shown once");
    }

    #[test]
    fn the_slow_way_three_times_brings_one_tip_and_the_key_way_never_counts() {
        let mut state = new(true, None, None);
        // The keyboard's twin doesn't count as the slow way.
        for _ in 0..3 {
            let _ = update(&mut state, Message::KeyShortcut("undo", Box::new(Message::Undo)));
        }
        assert!(!state.slow_counts.contains_key("undo"));
        // Clicks do: the gear three times brings the F2 tip, once.
        for _ in 0..3 {
            let _ = update(&mut state, Message::ToggleSettings);
        }
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::Tip("settings-key")));
        state.trick_card = None;
        for _ in 0..6 {
            let _ = update(&mut state, Message::ToggleSettings);
        }
        assert!(state.trick_card.is_none(), "a tip shows once");
    }

    #[test]
    fn with_tips_off_tricks_are_still_found_but_nothing_shows() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetTips(false));
        let _ = update(&mut state, Message::StepMix(1));
        assert!(found(&state, "step-mix"));
        assert!(state.trick_card.is_none());
    }

    #[test]
    fn the_light_test_rises_holds_and_falls_without_a_clip() {
        let start = Instant::now() - Duration::from_millis(370);
        let peak = crate::motion::boot_level(start, 0);
        assert!(peak > 0.85 && peak < 0.9, "held near -1 dBFS: {peak}");
        let late = Instant::now() - Duration::from_millis(1000);
        assert_eq!(crate::motion::boot_level(late, 0), 0.0, "and back down");
        assert!(crate::motion::boot_level(Instant::now(), 3) == 0.0, "the later meters wait their turn");
    }

    #[test]
    fn finds_close_together_grow_the_cards_words_and_its_segment_lights_in() {
        let mut state = crate::app::new(true, None, None);
        state.tips = true;
        state.notice = None;
        state.animations = true;
        state.tricks_found.clear();
        let mut kickers = Vec::new();
        for _ in 0..3 {
            bump_streak(&mut state);
            kickers.push(found_kicker(state.trick_streak.0));
        }
        assert_eq!(kickers, ["New trick:", "Another one:", "On a roll:"]);
        // Long apart: back to the plain words.
        state.trick_streak.1 = Instant::now().checked_sub(STREAK_GAP + std::time::Duration::from_secs(1));
        bump_streak(&mut state);
        assert_eq!(found_kicker(state.trick_streak.0), "New trick:");
        // Its segment arrives: unlit as the card lands, lit and blooming
        // after, settled later; lit at once with Animations off.
        state.trick_card = Some((Card::Found(TRICKS[0].id), Instant::now()));
        assert!(newest_arrival(&state).0 < 0.2 && arriving(&state));
        state.trick_card = Some((Card::Found(TRICKS[0].id), Instant::now().checked_sub(std::time::Duration::from_millis(200)).unwrap()));
        let (light, bloom) = newest_arrival(&state);
        assert!(light == 1.0 && bloom > 0.0, "{light} {bloom}");
        state.trick_card = Some((Card::Found(TRICKS[0].id), Instant::now().checked_sub(std::time::Duration::from_secs(1)).unwrap()));
        assert!(!arriving(&state));
        state.animations = false;
        state.trick_card = Some((Card::Found(TRICKS[0].id), Instant::now()));
        assert_eq!(newest_arrival(&state), (1.0, 0.0));
    }

    #[test]
    fn small_wins_have_their_own_switch_apart_from_tips() {
        // The owner's pick, 2026-11-16: Tips off had taken the sparks and the
        // rising note with the cards.
        use crate::app::{new, Message};
        use tuxmix_core::ChannelId;
        let find = |tips: bool, wins: bool| {
            let mut state = new(true, None, None);
            state.notice = None;
            state.tips = tips;
            state.small_wins = wins;
            state.ui_sounds = true;
            state.celebration_sounds = true;
            state.tricks_found.clear();
            state.view = crate::app::View::Mixer;
            state.focused_strip = Some(ChannelId::Input(0));
            crate::sounds::TEST_HEARD.with(|h| h.borrow_mut().clear());
            note(&mut state, &Message::FaderNudge(1.0));
            let heard = crate::sounds::TEST_HEARD.with(|h| h.borrow().len());
            (state.trick_card.is_some(), state.win_spark.is_some(), heard > 0 || state.trick_chime_due.is_some())
        };
        assert_eq!(find(false, true), (false, true, true), "Tips off, small wins on: sparks and the note, no card");
        assert_eq!(find(true, false), (true, false, false), "Tips on, small wins off: the card in quiet");
        assert_eq!(find(true, true), (true, true, true));
    }

    #[test]
    fn a_milestone_takes_the_foot_from_a_waiting_trick_card_and_its_chime() {
        let mut state = crate::app::new(true, None, None);
        state.trick_card = Some((Card::Found(TRICKS[0].id), Instant::now()));
        state.trick_chime_due = Some(3);
        crate::celebrate::celebrate(&mut state, crate::celebrate::Reward::Milestone("first-snapshot"));
        assert!(state.trick_card.is_none() && state.trick_chime_due.is_none());
    }

    #[test]
    fn a_switch_turned_on_the_first_time_says_what_it_did_once() {
        let mut state = crate::app::new(true, None, None);
        state.tips = true;
        state.tips_shown.clear();
        state.trick_card = None;
        let _ = crate::app::update(&mut state, Message::Pad(0, true));
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First("first:pad")));
        assert!(!first_words("first:pad").is_empty());
        state.trick_card = None;
        let _ = crate::app::update(&mut state, Message::Pad(0, false));
        let _ = crate::app::update(&mut state, Message::Pad(0, true));
        assert!(state.trick_card.is_none(), "once ever");
        // Tips off: none.
        let mut quiet = crate::app::new(true, None, None);
        quiet.tips = false;
        quiet.tips_shown.clear();
        let _ = crate::app::update(&mut quiet, Message::Pad(0, true));
        assert!(quiet.trick_card.is_none());
    }

    #[test]
    fn powering_a_mic_says_what_it_did_without_sparks() {
        // Critique #58: sparks say "well done", wrong for 48V or a first Solo.
        let mut state = crate::app::new(true, None, None);
        state.tips = true;
        state.tips_shown.clear();
        state.trick_card = None;
        state.win_spark = None;
        note_first(&mut state, &Message::Phantom(0, true));
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First("first:48v")));
        assert!(state.win_spark.is_none());
        state.trick_card = None;
        note_first(&mut state, &Message::Pad(0, true));
        assert!(state.win_spark.is_none(), "nor PAD's");
        state.trick_card = None;
        note_first(&mut state, &Message::PhaseChanged(tuxmix_core::ChannelId::Input(0), true));
        assert!(state.win_spark.is_some(), "phase's first time still sparks");
    }

    #[test]
    fn trick_pips_read_on_the_card_in_every_theme() {
        // Critique #57: unlit 2.30:1 and the newest 2.03:1 on Daylight's card.
        let t = iced::Theme::Dark;
        for entry in theme::THEMES {
            theme::set_palette(entry.palette);
            let ground = match theme::dialog(&t).background {
                Some(iced::Background::Color(c)) => c,
                _ => theme::surface(),
            };
            let (ghost, newest) = super::pip_colors(&t);
            assert!(theme::contrast(ghost, ground) >= 3.0, "{}: unlit {:.2}", entry.name, theme::contrast(ghost, ground));
            assert!(theme::contrast(newest, ground) >= 3.0, "{}: newest {:.2}", entry.name, theme::contrast(newest, ground));
            assert!(theme::contrast(theme::signal(), ground) >= 3.0, "{}: lit", entry.name);
        }
        theme::set_palette(theme::THEMES[0].palette);
    }

    #[test]
    fn a_fader_past_0_db_says_so_once_and_doesnt_glow() {
        let mut state = crate::app::new(true, None, None);
        state.tips = true;
        state.tips_shown.clear();
        state.trick_card = None;
        state.win_spark = None;
        let cid = tuxmix_core::ChannelId::Input(0);
        let _ = crate::app::update(&mut state, Message::VolumeChanged(cid, 0, 1.0));
        assert!(state.trick_card.is_none(), "0 dB itself is no news");
        let _ = crate::app::update(&mut state, Message::VolumeChanged(cid, 0, 1.4));
        assert_eq!(state.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_ABOVE_0)));
        assert!(state.win_spark.is_none(), "a caution, not a win");
        assert!(names_mix_meter(&state), "the column rings the mix's meter while it shows");
        let named = above0_words(&state);
        assert!(named.contains(&crate::sidebar::mix_name(&state, state.sel_out)) && named.contains("ringed now"), "{named}");
        assert!(first_words(FIRST_ABOVE_0).contains("red") && first_words(FIRST_ABOVE_0).contains("mix you're on"));
        state.trick_card = None;
        let _ = crate::app::update(&mut state, Message::VolumeChanged(cid, 0, 0.5));
        let _ = crate::app::update(&mut state, Message::VolumeChanged(cid, 0, 1.6));
        assert!(state.trick_card.is_none(), "once ever");
        // Within a fader's own step of 0 dB: still no news (Strata's review).
        let mut near = crate::app::new(true, None, None);
        near.tips = true;
        near.tips_shown.clear();
        near.trick_card = None;
        let _ = crate::app::update(&mut near, Message::VolumeChanged(cid, 0, 1.003));
        assert!(near.trick_card.is_none());
        // A Matrix cell counts too.
        let _ = crate::app::update(&mut near, Message::MatrixVolume(cid, 0, 1.3));
        assert_eq!(near.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_ABOVE_0)));
        // From a remote surface: the hand isn't on the screen.
        let mut remote = crate::app::new(true, None, None);
        remote.tips = true;
        remote.tips_shown.clear();
        remote.trick_card = None;
        remote.from_remote = true;
        super::note_first(&mut remote, &Message::VolumeChanged(cid, 0, 1.5));
        assert!(remote.trick_card.is_none());
        // An output's own fader: its meter does show it, so the card says so.
        let mut out = crate::app::new(true, None, None);
        out.tips = true;
        out.tips_shown.clear();
        out.trick_card = None;
        let _ = crate::app::update(&mut out, Message::VolumeChanged(tuxmix_core::ChannelId::Output(0), 0, 1.3));
        assert_eq!(out.trick_card.map(|(c, _)| c), Some(Card::First(FIRST_ABOVE_0_OUT)));
        assert!(first_words(FIRST_ABOVE_0_OUT).contains("Its meter"));
    }

}
