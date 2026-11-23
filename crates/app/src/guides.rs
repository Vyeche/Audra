//! Guided tasks: "How do I…" (2026-11-05, the owner's rewarding round:
//! "It should feel rewarding to use it … Like the game bejeweled", "The
//! sounds should be subtle").
//!
//! The thesis: a task taught by doing it. About lists a few things worth
//! doing; starting one puts its steps on a card at the window's foot, and
//! each step ticks itself as it's done, read from the mix itself (as
//! Quick's steps are), never from the card: Reverb switched on, the mic in
//! the Phones, a slot stored. A step ticks with one soft note, each a step
//! higher than the one before (the session's chain); the last lights the
//! card with the lamp and relights its steps in a sweep, and a milestone
//! reached in the same moment is named there, not shown twice.
//!
//! The rules it keeps:
//! - **The mix decides**: a live step un-ticks when it stops being true
//!   (the Phones turned back down); a step that is a thing done (a slot
//!   stored, a key pressed) stays done. A step's note plays once a run.
//! - **"Show me" only shows**: it opens the page or panel where the step
//!   is done (Reverb & Echo, a strip's Settings, Quick), never does it.
//! - **One card at a time**: a notice first, then a guide, then a
//!   milestone, then a trick. While a guide runs, a milestone waits for it
//!   to finish, and a trick found is counted without its card or note (the
//!   guide's own notes are the reward).

use std::time::{Duration, Instant};

use iced::widget::{button, canvas, column, container, mouse_area, row, text, Space};
use iced::{Element, Length};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::theme;

/// A guided task: its id (kept in the prefs once done), its title (About
/// and the card), its steps, and what Audra says when it's done.
pub struct Guide {
    pub id: &'static str,
    pub title: &'static str,
    pub done_title: &'static str,
    pub praise: &'static str,
    pub steps: &'static [Step],
}

/// A guide's or a milestone's line in the voice the plain-words switch
/// sets: "your headphones" and "computer audio" where the strips and Quick
/// setup say so (critique #85 A P3-5, #86 A P3-2: the headphone-mix guide
/// said "the Phones" over a strip labelled "your headphones").
pub fn voiced(state: &TuxMix, words: &'static str) -> &'static str {
    if !state.plain_names {
        return words;
    }
    match words {
        "Pick the Phones mix: press its output strip" => "Pick your headphones\u{2019} mix: press the Phones output strip",
        "Add the computer's sound: raise a playback fader (AN1/2 PB is where it plays)" => "Add computer audio: raise a playback fader (AN1/2 PB is where apps play)",
        "Turn the Phones up (the level at the right)" => "Turn your headphones up (the Phones level at the right)",
        "Mic and computer side by side in the Phones, turned up." => "Mic and computer side by side in your headphones, turned up.",
        "Your mic in the Phones: you hear yourself as you talk." => "Your mic in your headphones: you hear yourself as you talk.",
        "Send your mic into the Phones mix, the Phones turned up." => "Send your mic into your headphones\u{2019} mix, the headphones turned up.",
        _ => words,
    }
}

/// A step: its words, whether it is done (from the mix and the run), and
/// where "Show me" takes you, if anywhere (it may set what's shown, and
/// hand back a message to send).
pub struct Step {
    pub words: &'static str,
    pub done: fn(&TuxMix, &Run) -> bool,
    pub go: Option<fn(&mut TuxMix) -> Option<Message>>,
}

/// A guide under way: which, what it started from, what the user has done
/// since (things done, by name, as many times as done), which steps have
/// played their note, and when it was finished.
#[derive(Clone, Debug)]
pub struct Run {
    pub guide: usize,
    pub mic_presets: usize,
    pub seen: Vec<&'static str>,
    pub noted: Vec<bool>,
    pub finished: Option<Instant>,
    /// A milestone reached as it finished, named on its card.
    pub milestone: Option<&'static str>,
    /// The snapshot slot last stored to, so "press that slot again" wants
    /// that one (job 24).
    pub stored_slot: Option<u8>,
    /// Finished behind a notice: the rise's step, played when the card
    /// shows (as a milestone's notes wait for theirs; the Lock guide's rise
    /// played while "The mix is locked" hid its card).
    pub rise: Option<u8>,
    /// This finish completed every guide: its card says so, the meters'
    /// light test runs across the desk, the rise starts at its top.
    pub finale: bool,
}

impl Run {
    pub fn new(guide: usize, state: &TuxMix) -> Self {
        Run {
            guide,
            mic_presets: state.mic_presets.len(),
            seen: Vec::new(),
            noted: vec![false; GUIDES.get(guide).map_or(0, |g| g.steps.len())],
            finished: None,
            milestone: None,
            stored_slot: None,
            rise: None,
            finale: false,
        }
        .seeded(state)
    }

    /// What is already so as it begins: the mix locked counts as locked,
    /// so unlocking ticks the lock guide's last step.
    fn seeded(mut self, state: &TuxMix) -> Self {
        if state.ui_locked {
            self.seen.push("locked");
        }
        self
    }

    fn saw(&self, what: &str) -> bool {
        self.seen.contains(&what)
    }

    fn count(&self, what: &str) -> usize {
        self.seen.iter().filter(|s| **s == what).count()
    }

    /// `then` done after `first` was.
    fn saw_after(&self, first: &str, then: &str) -> bool {
        self.seen.iter().position(|s| *s == first).is_some_and(|i| self.seen[i + 1..].contains(&then))
    }
}

// "Show me" opens the page (never closes it, as a second press on its bar
// would).
fn fx_page(state: &mut TuxMix) -> Option<Message> {
    state.sidebar_open = true;
    state.show_settings = false;
    state.sidebar_tab = Some(crate::sidebar::Tab::Fx);
    state.page_card = crate::sidebar::pages_float(state);
    None
}

/// The Snapshots page open and the counter scrolled to it: the slots and
/// Store stand at its end, above the guide's own card.
fn snapshots_page(state: &mut TuxMix) -> Option<Message> {
    state.sidebar_open = true;
    state.show_settings = false;
    // Where the pages float, the keys and Store stand in the head: the
    // keyboard's ring lands on Store.
    if crate::sidebar::pages_float(state) {
        state.key_focus = Some(crate::keyfocus::KeyFocus::Counter(crate::keyfocus::CounterItem::Store));
        state.focused_strip = None;
        return None;
    }
    state.sidebar_tab = Some(crate::sidebar::Tab::Snapshots);
    Some(Message::ScrollCounterToEnd)
}

fn quick_view(state: &mut TuxMix) -> Option<Message> {
    state.show_settings = false;
    state.view = crate::app::View::Quick;
    None
}

fn mixer_view(state: &mut TuxMix) -> Option<Message> {
    state.show_settings = false;
    state.view = crate::app::View::Mixer;
    None
}

/// The Mixer with a row open (0 inputs, 1 playback, 2 outputs), where a
/// step's faders are.
fn row_of(state: &mut TuxMix, floor: usize) -> Option<Message> {
    let _ = mixer_view(state);
    Some(Message::OpenFloor(floor))
}

/// The mic's strip Settings (where its Mic preset is), opened in the Mixer,
/// the keyboard's ring on its Mic preset row and that row scrolled into
/// view (`app::panel_scroll_to_focus`): on a short panel it stands below
/// the fold.
fn mic_settings(state: &mut TuxMix) -> Option<Message> {
    let _ = mixer_view(state);
    let cid = mic(state);
    let kind = crate::widgets::strip::FlyoutKind::Settings;
    state.focused_strip = Some(cid);
    state.key_focus = Some(crate::keyfocus::KeyFocus::Panel(crate::keyfocus::PanelItem::MicPreset));
    (!state.flyout_open.contains(&(cid, kind))).then_some(Message::ToggleFlyout(cid, kind))
}

fn start_with(state: &mut TuxMix) -> Option<Message> {
    state.show_settings = true;
    state.settings_page = crate::app::SettingsPage::Mixer;
    None
}

/// The mic the guides mean: Quick's (AN1 unless chosen otherwise).
fn mic(state: &TuxMix) -> ChannelId {
    match state.quick_channel {
        ChannelId::Input(i) => ChannelId::Input(i),
        _ => ChannelId::Input(0),
    }
}

fn phones(state: &TuxMix) -> Option<usize> {
    state.control_room.assign.phones_pairs().first().copied()
}

fn phones_level(state: &TuxMix) -> f32 {
    let Some(pair) = phones(state) else { return 0.0 };
    let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
    crate::control_room::user_output_level(state, ch)
}

/// A source sent into the Phones, not muted.
fn into_phones(state: &TuxMix, cid: ChannelId) -> bool {
    let Some(pair) = phones(state) else { return false };
    let muted = match cid {
        ChannelId::Input(i) => state.device.inputs().get(i).is_none_or(|c| c.mute),
        ChannelId::Playback(i) => state.device.playbacks().get(i).is_none_or(|c| c.mute),
        ChannelId::Output(_) => true,
    };
    !muted && state.device.volume(cid, pair).unwrap_or(0.0) > 0.0
}

const REVERB: &[Step] = &[
    Step { words: "Switch Reverb on (Reverb & Echo, at the right)", done: |s, _| s.fx.reverb_on, go: Some(fx_page) },
    Step {
        // The unit's one FX send is AN1/2's (the driver's); "Make it heard"
        // on the page "Show me" opens sets it (critique #8: the step named a
        // knob that lives in the strip's Settings, not where it pointed).
        // With an open mic it is "Make it heard in the headphones" (critique
        // #109 B: the step named a key that wasn't on screen).
        // Short: three lines covered the sidebar's tabs at 1280x700
        // (critique #110 B); the key's label starts with these words.
        words: "Send your mic to it: press Make it heard",
        done: |s, _| crate::fx::send_db(s) > crate::fx::OFF_DB,
        go: Some(fx_page),
    },
    Step {
        // The return isn't a control on the page "Show me" opens; Make it
        // heard sets it, or the output's Settings do (job 24). With an open
        // mic on the speakers it comes back in the headphones, which counts
        // (critique #109 B: the step could never finish).
        words: "Bring it back into the mix you hear: Make it heard does it, or the output's Settings (FX return)",
        done: |s, _| {
            // Not on the hazard: the speakers' return up with an open mic is
            // no step done (critique #111 Fable A P3-1).
            (crate::fx::user_return(s, s.sel_out) > 0.0 && crate::fx::mic_reverb_hazard(s).is_none())
                || (crate::fx::open_mic_reverb_risk(s, s.sel_out)
                    && s.control_room.assign.phones_pairs().iter().any(|&ph| crate::fx::user_return(s, ph) > 0.0))
        },
        go: Some(fx_page),
    },
    // Nothing is heard in the simulation, so it never says so there (job
    // 24), as the voice guide's last step doesn't.
    Step { words: "Talk, and hear your voice in a room", done: |s, _| crate::fx::heard(s) && !s.device.is_mock(), go: Some(fx_page) },
];

const PHONES: &[Step] = &[
    Step {
        words: "Pick the Phones mix: press its output strip",
        done: |s, _| phones(s) == Some(s.sel_out),
        go: Some(|s| row_of(s, 2)),
    },
    Step {
        words: "Bring your mic up in it",
        done: |s, _| into_phones(s, mic(s)),
        go: Some(|s| row_of(s, 0)),
    },
    Step {
        words: "Add the computer's sound: raise a playback fader (AN1/2 PB is where it plays)",
        done: |s, r| r.saw("phones-playback-up") && (0..s.device.playbacks().len()).any(|i| into_phones(s, ChannelId::Playback(i))),
        go: Some(|s| row_of(s, 1)),
    },
    Step { words: "Turn the Phones up (the level at the right)", done: |s, _| phones_level(s) > 0.0, go: None },
];

const SNAPSHOT: &[Step] = &[
    Step {
        words: "Press Store, by the snapshot slots",
        done: |s, r| s.snapshot_storing || r.saw("stored"),
        go: Some(snapshots_page),
    },
    Step { words: "Press a slot: the mix is kept in it", done: |_, r| r.saw("stored"), go: Some(snapshots_page) },
    Step {
        // That slot, not any (job 24).
        words: "Change something, then press that slot again",
        done: |_, r| r.saw_after("stored", "recalled-stored"),
        go: Some(snapshots_page),
    },
];

/// "Show me" for the voice guide: the Mixer, with the mic's D panel open.
fn voice_panel(state: &mut TuxMix) -> Option<Message> {
    let _ = mixer_view(state);
    let cid = mic(state);
    let kind = crate::widgets::strip::FlyoutKind::Dynamics;
    state.focused_strip = Some(cid);
    (!state.flyout_open.contains(&(cid, kind))).then_some(Message::ToggleFlyout(cid, kind))
}

/// "Show me" for the voice guide's last step, which is done in another app:
/// the panel, and where to look in that app, said (the owner, 2026-11-13:
/// with the panel already open, "Show me" did nothing).
fn voice_app(state: &mut TuxMix) -> Option<Message> {
    let open = voice_panel(state);
    // Short and plain (critique #46: 40 words, and a confusing aside about
    // the simulation).
    state.notice = Some((
        format!("In your call app's audio settings, pick \u{201c}{}\u{201d} as the microphone. This step ticks once an app uses it.", crate::dynamics_loop::SOURCE_LABEL),
        std::time::Instant::now(),
    ));
    open
}

/// Voice on for the guides' mic.
fn voice_on(state: &TuxMix) -> bool {
    crate::dynamics_panel::power_of(state, mic(state))
}

/// Critique #34 asked what a podcaster would type into Help to find the
/// voice: the owner's pick, one name ("Voice") and this guide.
const VOICE: &[Step] = &[
    Step {
        words: "Open Voice: the D key on your mic's strip",
        done: |s, _| voice_on(s) || s.flyout_open.contains(&(mic(s), crate::widgets::strip::FlyoutKind::Dynamics)),
        go: Some(voice_panel),
    },
    Step { words: "Press ON, or pick a sound such as Clear Calls: Voice turns on", done: |s, _| voice_on(s), go: Some(voice_panel) },
    Step {
        words: "Press Fit to my voice and talk for ten seconds",
        done: |s, _| voice_on(s) && s.dyn_fit_report.is_some(),
        go: Some(voice_panel),
    },
    // Ticked only once an app really records from it (critique #35: the
    // guide praised "sounding good on calls" with no app listening).
    Step {
        words: "In your call app, pick \u{201c}Audra processed mic\u{201d} as the microphone",
        done: |s, _| voice_on(s) && s.dyn_heard == Some(true),
        go: Some(voice_app),
    },
];

const MIC: &[Step] = &[
    Step {
        words: "In Quick setup, tick your mic's three steps",
        done: |s, _| crate::quick::steps(s).iter().all(|st| *st == crate::quick::Step::Done),
        go: Some(quick_view),
    },
    Step {
        words: "Save it as a mic preset: its Settings, a name, Save",
        done: |s, r| s.mic_presets.len() > r.mic_presets,
        go: Some(mic_settings),
    },
    Step {
        words: "Have it load at start (Audra settings, Mixer)",
        done: |s, _| s.startup_mic_preset.is_some(),
        go: Some(start_with),
    },
];

const KEYS: &[Step] = &[
    Step {
        words: "Press Tab: the keyboard lands on a strip",
        done: |_, r| r.saw("tab"),
        go: Some(mixer_view),
    },
    Step { words: "\u{2191} and \u{2193} move its fader", done: |_, r| r.saw("nudge"), go: None },
    // M only (job 24: S presses counted too).
    Step { words: "M mutes it, and M again unmutes it", done: |_, r| r.count("m-key") >= 2, go: None },
    Step { words: "F1 lists every key and gesture", done: |_, r| r.saw("help-key"), go: None },
];

/// The output channel of mix `pair`, where its LOOP is.
fn looped(state: &TuxMix, pair: usize) -> bool {
    let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
    state.device.outputs().get(ch).is_some_and(|o| o.loopback)
}

/// A mix that isn't the speakers' or the Phones': free for a mix of its own.
fn spare_mix(state: &TuxMix) -> bool {
    let assign = state.control_room.assign;
    state.sel_out != assign.main && assign.main_b != Some(state.sel_out) && !assign.phones_pairs().contains(&state.sel_out)
}

const LOCK: &[Step] = &[
    Step {
        words: "Lock the mix: the padlock in the top bar",
        done: |s, r| s.ui_locked || r.saw_after("locked", "unlocked"),
        go: None,
    },
    Step {
        // A fader really tried (OpenCode's job 24: it also ticked on the
        // unlock, so two padlock clicks finished the guide).
        words: "Try a fader: it holds still, and says why",
        done: |_, r| r.saw("held"),
        go: Some(mixer_view),
    },
    Step { words: "Unlock it when you're done", done: |_, r| r.saw_after("locked", "unlocked"), go: None },
];

const TOGETHER: &[Step] = &[
    Step {
        words: "Ctrl- or Shift-click two strips: they're picked together",
        done: |s, r| s.selected.len() > 1 || r.saw("moved-together"),
        go: Some(mixer_view),
    },
    Step {
        words: "Move one of their faders: the other follows",
        done: |_, r| r.saw("moved-together"),
        go: None,
    },
    Step { words: "Press F (next to M and S, by Undo) to let them go", done: |_, r| r.saw_after("moved-together", "let-go"), go: None },
];

const RECORD: &[Step] = &[
    Step {
        words: "Pick an output you don't listen on: press its strip",
        done: |s, _| spare_mix(s),
        go: Some(|s| row_of(s, 2)),
    },
    Step {
        words: "Bring your mic up in its mix",
        done: |s, _| {
            let m = mic(s);
            spare_mix(s) && !crate::app::strip_params(s, m, s.sel_out).mute && s.device.volume(m, s.sel_out).unwrap_or(0.0) > 0.0
        },
        go: Some(|s| row_of(s, 0)),
    },
    Step {
        words: "Add the computer's sound: raise a playback fader (AN1/2 PB is where it plays)",
        done: |s, r| {
            spare_mix(s)
                && r.saw("mix-playback-up")
                && (0..s.device.playbacks().len()).any(|i| {
                    !s.device.playbacks()[i].mute && s.device.volume(ChannelId::Playback(i), s.sel_out).unwrap_or(0.0) > 0.0
                })
        },
        go: Some(|s| row_of(s, 1)),
    },
    Step {
        words: "Turn the output itself up: its own fader",
        done: |s, _| {
            let ch = if s.device.outputs_one_per_pair() { s.sel_out } else { s.sel_out * 2 };
            spare_mix(s) && crate::control_room::user_output_level(s, ch) > 0.0
        },
        go: Some(|s| row_of(s, 2)),
    },
    Step {
        words: "Press its LOOP twice: the input of the same name now records this mix, not its jack",
        done: |s, _| spare_mix(s) && looped(s, s.sel_out),
        go: Some(|s| row_of(s, 2)),
    },
];

pub const GUIDES: [Guide; 9] = [
    Guide {
        id: "reverb",
        title: "Add reverb to my voice",
        done_title: "The room is yours",
        praise: "Switched on, sent, brought back and heard, in the mix you're on.",
        steps: REVERB,
    },
    Guide {
        id: "phones",
        title: "Build a headphone mix",
        done_title: "Your own headphone mix",
        praise: "Mic and computer side by side in the Phones, turned up.",
        steps: PHONES,
    },
    Guide {
        id: "snapshot",
        title: "Keep a mix as a snapshot",
        done_title: "Store and recall, learned",
        praise: "Any mix one press away: eight slots, Alt and a number.",
        steps: SNAPSHOT,
    },
    Guide {
        id: "mic",
        title: "Set up my mic and keep it",
        done_title: "Ready at every start",
        praise: "Set in Quick setup, kept as a preset, loaded as Audra opens.",
        steps: MIC,
    },
    Guide {
        id: "voice",
        title: "Make my voice sound good on calls",
        done_title: "Sounding good on calls",
        praise: "Voice is on, fitted to you, and your call app hears it. You still hear yourself straight.",
        steps: VOICE,
    },
    Guide {
        id: "lock",
        // Not only for streams (critique #105 A P3-12: a class, little hands).
        title: "Lock the mix (a stream, a class, little hands)",
        done_title: "Steady hands",
        praise: "Locked while you're live, silent on air, a fader nudged says so, and unlocked at the end.",
        steps: LOCK,
    },
    Guide {
        id: "together",
        title: "Move strips together",
        done_title: "Two faders, one hand",
        praise: "Picked with Ctrl or Shift, moved as one, and let go with F.",
        steps: TOGETHER,
    },
    Guide {
        id: "record",
        title: "Record a mix of mic and computer",
        done_title: "A mix ready to record",
        praise: "Record from that input in your app, with its monitoring off, or it feeds back.",
        steps: RECORD,
    },
    Guide {
        id: "keys",
        title: "Mix from the keyboard",
        done_title: "The keys know the way",
        praise: "Tab to a strip, the arrows for its fader, M to mute, F1 for the rest.",
        steps: KEYS,
    },
];

/// How long the finished card stays (while not pointed at).
pub const DONE_FOR: Duration = Duration::from_secs(6);

pub fn done(state: &TuxMix, id: &str) -> bool {
    state.guides_done.iter().any(|g| g == id)
}

fn steps_done(state: &TuxMix, run: &Run) -> Vec<bool> {
    GUIDES[run.guide].steps.iter().map(|st| (st.done)(state, run)).collect()
}

/// Watches each message the user sends (`app::update`, before it runs,
/// beside `tricks::note`): the keys and gestures, by their trick's name.
pub fn note(state: &mut TuxMix, message: &Message) {
    if state.guide.as_ref().is_none_or(|r| r.finished.is_some()) {
        return;
    }
    if let Some(id) = crate::tricks::trick_of(state, message) {
        if let Some(run) = state.guide.as_mut() {
            run.seen.push(id);
            if id == "ms-keys" && matches!(message, Message::KeyLetter('m')) {
                run.seen.push("m-key");
            }
        }
    }
}

/// The same, once a slot press has become what it is (`app::update`,
/// after it decides): a slot stored, a slot recalled.
pub fn note_decided(state: &mut TuxMix, message: &Message) {
    let picked = state.selected.len();
    let locked = state.ui_locked;
    let in_pick = |cid: &ChannelId| state.selected.contains(cid);
    // Only a change the lock lets through counts as done.
    let moved_together = !locked && matches!(message, Message::VolumeChanged(cid, ..) if picked > 1 && in_pick(cid));
    // A playback fader raised by hand, and into which mix: the computer's
    // sound brought in (each output carries its own playback by default,
    // so a playback already up there says nothing).
    // Raised, not just moved: noted before the change applies, so the level
    // it had is still there to compare (pulling a fader down says nothing).
    let playback_up = match message {
        Message::VolumeChanged(cid @ ChannelId::Playback(_), out, v)
            if !locked && *v > state.device.volume(*cid, *out).unwrap_or(0.0) =>
        {
            Some(*out)
        }
        _ => None,
    };
    let (sel, phones_pair, spare) = (state.sel_out, phones(state), spare_mix(state));
    let Some(run) = state.guide.as_mut().filter(|r| r.finished.is_none()) else {
        return;
    };
    if moved_together {
        run.seen.push("moved-together");
    }
    if let Some(out) = playback_up {
        if Some(out) == phones_pair {
            run.seen.push("phones-playback-up");
        }
        // Into the spare mix being built, not whatever mix was on before.
        if out == sel && spare {
            run.seen.push("mix-playback-up");
        }
    }
    match message {
        Message::SnapshotStoreTo(slot) => {
            run.seen.push("stored");
            run.stored_slot = Some(*slot);
        }
        Message::SnapshotClicked(slot) => {
            run.seen.push("recalled");
            if run.stored_slot == Some(*slot) {
                run.seen.push("recalled-stored");
            }
        }
        Message::SetUiLock(true) => run.seen.push("locked"),
        Message::SetUiLock(false) => run.seen.push("unlocked"),
        // F with strips picked: they let go (a fader of a picked strip moved
        // while two or more are picked is noted above: the others came too).
        Message::FaderMaster if picked > 1 => run.seen.push("let-go"),
        _ => {}
    }
}

/// On the tick: each step newly done plays its note (once a run, a step
/// higher each); all done at once finishes the guide (kept in the prefs)
/// with a milestone's burst and rise; a finished card goes after its time.
pub fn tick(state: &mut TuxMix) {
    let Some(run) = state.guide.clone() else { return };
    if let Some(at) = run.finished {
        // Not yet shown, it takes a plain notice's place once that's read.
        if run.rise.is_some() && crate::celebrate::plain_notice_read(state) {
            state.notice = None;
        }
        if state.notice.is_some() {
            // Waiting, its burst holds at the start; shown already, held
            // settled, so a later notice doesn't replay it.
            let held = if run.rise.is_some() { Instant::now() } else { crate::celebrate::settled_at() };
            if let Some(r) = state.guide.as_mut() {
                r.finished = Some(held);
            }
        } else if let Some(step) = run.rise {
            // Out from behind the notice: its rise now, with the card.
            if let Some(r) = state.guide.as_mut() {
                r.rise = None;
            }
            if crate::celebrate::sounds_on(state) {
                crate::sounds::play(crate::sounds::Sound::Rise(step), state.ui_sound_db - 4.0);
            }
        } else if !state.guide_held && at.elapsed() > DONE_FOR {
            state.guide = None;
        }
        return;
    }
    let now = steps_done(state, &run);
    let mut noted = run.noted.clone();
    for (i, d) in now.iter().enumerate() {
        if *d && !noted[i] {
            noted[i] = true;
            // A step up the session's chain (`celebrate::next_win`), so a
            // guide after a milestone or a trick climbs on from it.
            let (step, _) = crate::celebrate::next_win(state, false);
            if crate::celebrate::sounds_on(state) {
                crate::sounds::play(crate::sounds::Sound::Chime(step.min(9)), state.ui_sound_db - 4.0);
            }
        }
    }
    let finished = now.iter().all(|d| *d);
    if let Some(r) = state.guide.as_mut() {
        r.noted = noted;
    }
    if finished {
        let id = GUIDES[run.guide].id;
        if !done(state, id) {
            state.guides_done.push(id.to_string());
        }
        // The last of them: a finale (the reviewer, 2026-11-14: milestones
        // and tricks had one, guides none). Once; the day kept for Help.
        let finale = state.guides_all_on.is_none() && GUIDES.iter().all(|g| done(state, g.id));
        if finale {
            state.guides_all_on = crate::recorder::local_stamp().get(..10).map(str::to_string);
            if state.animations {
                state.boot_sweep = Some(Instant::now());
            }
        }
        // A milestone reached in the same moment: said here, once.
        let milestone = crate::celebrate::fold_into_guide(state);
        if let Some(r) = state.guide.as_mut() {
            r.finished = Some(Instant::now());
            r.milestone = milestone;
            r.finale = finale;
        }
        state.prefs_dirty = true;
        state.guide_held = false;
        let (step, _) = crate::celebrate::next_win(state, true);
        let step = if finale { crate::celebrate::RISE_TOP } else { step.min(crate::celebrate::RISE_TOP) };
        if state.notice.is_some() {
            // Its card waits behind the notice: so do its notes.
            if let Some(r) = state.guide.as_mut() {
                r.rise = Some(step);
            }
        } else if crate::celebrate::sounds_on(state) {
            crate::sounds::play(crate::sounds::Sound::Rise(step), state.ui_sound_db - 4.0);
        }
    }
}

/// Whether the finished card's burst is flying (`motion::running`).
pub fn sparking(state: &TuxMix) -> bool {
    state.animations
        && state.notice.is_none()
        && state.guide.as_ref().and_then(|r| r.finished).is_some_and(|at| at.elapsed() < crate::celebrate::BURST_FOR)
}

/// Whether the guide card in the counter's slot offers "Show me" now (the
/// keyboard's stop for it, `keyfocus::CounterItem::GuideGo`).
pub fn show_me_here(state: &TuxMix) -> bool {
    if state.notice.is_some() || !state.sidebar_open {
        return false;
    }
    let Some(run) = state.guide.as_ref().filter(|r| r.finished.is_none()) else {
        return false;
    };
    let now = steps_done(state, run);
    now.iter().position(|d| !d).is_some_and(|i| GUIDES[run.guide].steps[i].go.is_some())
}

/// Whether a guide under way stands in the counter's slot (its × is a
/// keyboard stop).
pub fn under_way_here(state: &TuxMix) -> bool {
    state.notice.is_none() && state.sidebar_open && state.guide.as_ref().is_some_and(|r| r.finished.is_none())
}

pub fn update(state: &mut TuxMix, message: Message) -> Option<Message> {
    match message {
        Message::GuideStart(i) if i < GUIDES.len() => {
            state.guide = Some(Run::new(i, state));
            state.guide_held = false;
            state.show_about = false;
            // Its card takes the counter's slot: the counter open (folded to
            // its rail, the card would stand on the outputs' faders), and the
            // slot its own.
            state.sidebar_open = true;
            state.trick_card = None;
            state.celebration = None;
            // A guide that teaches on a page opens it (the counter gives
            // it room; the reverb guide pointed at a bar out of sight).
            let page = guide_page(state);
            let float = crate::sidebar::pages_float(state);
            // Where the pages float, the snapshots' keys are in the head
            // already; Reverb & Echo opens as its card.
            if let Some(tab) = page.filter(|&t| !(float && t == crate::sidebar::Tab::Snapshots)) {
                state.sidebar_tab = Some(tab);
                state.page_card = float;
            }
            // Started from the keyboard in About: the keyboard follows to
            // the card's "Show me" (or its ×), not left on a closed dialog.
            if matches!(state.key_focus, Some(crate::keyfocus::KeyFocus::Item(_))) {
                state.key_focus = if show_me_here(state) {
                    Some(crate::keyfocus::KeyFocus::Counter(crate::keyfocus::CounterItem::GuideGo))
                } else {
                    Some(crate::keyfocus::KeyFocus::Counter(crate::keyfocus::CounterItem::GuideStop))
                };
                state.focused_strip = None;
            }
            if page == Some(crate::sidebar::Tab::Snapshots) && !float {
                return Some(Message::ScrollCounterToEnd);
            }
        }
        Message::GuideStop => {
            state.guide = None;
            state.guide_held = false;
        }
        Message::GuideHeld(held) => state.guide_held = held,
        Message::GuideGo => {
            let run = state.guide.clone()?;
            let now = steps_done(state, &run);
            let next = now.iter().position(|d| !d)?;
            let go = GUIDES[run.guide].steps[next].go?;
            return go(state);
        }
        _ => {}
    }
    None
}

fn close_key(scale: f32, hint: &'static str, message: Message) -> Element<'static, Message> {
    crate::widgets::strip::hint(
        button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
            .padding(0)
            .width(Length::Fixed(20.0 * scale))
            .height(Length::Fixed(20.0 * scale))
            .style(theme::icon_button(false))
            .on_press(message),
        hint,
        scale,
    )
}

/// "Next for you" (Learning pass 1, the owner: the learning passes
/// "should guide the user where they need to go and what they are likely
/// needing to acheve"): one guide at a time, picked from what the mix
/// shows hasn't been done yet: no mic preset saved, then no snapshot kept,
/// then the reverb never heard, then the rest in Help's order. None
/// once all are done, while one is under way, or set aside this session.
pub fn next_for_you(state: &TuxMix) -> Option<usize> {
    if state.guide.is_some() {
        return None;
    }
    let index = |id: &str| GUIDES.iter().position(|g| g.id == id);
    let wanted = |id: &'static str| !done(state, id) && state.next_set_aside != Some(id);
    // The mic first (critique #15: a first run was told to add reverb
    // before any mic was set up), then a snapshot, then the reverb.
    let mut order: Vec<&'static str> = Vec::new();
    // Voice on but no app hearing it yet: the voice guide, whose last step
    // is picking "Audra processed mic" in the call app.
    if state.dyn_heard == Some(false) {
        order.push("voice");
    }
    if state.mic_presets.is_empty() {
        order.push("mic");
    }
    if !state.stored_snapshots.iter().any(|&s| s) {
        order.push("snapshot");
    }
    if state.fx_heard_at.is_none() {
        order.push("reverb");
    }
    order.extend(GUIDES.iter().map(|g| g.id));
    order.into_iter().find(|&id| wanted(id)).and_then(index)
}

/// Whether "Next for you" offers to keep the mic as a preset rather than
/// start the mic guide (issue #20, the owner's yes): the mic guide is next,
/// Quick setup's three checks have passed on its input, and no preset
/// holds that setup yet. Telling someone who has just set the mic up to
/// "set up my mic" contradicted Quick setup's ticks beside it.
pub fn keep_mic_offered(state: &TuxMix) -> bool {
    let mic = GUIDES.iter().position(|g| g.id == "mic");
    mic.is_some()
        && next_for_you(state) == mic
        && matches!(state.quick_channel, ChannelId::Input(_))
        && crate::quick::steps(state).iter().all(|s| *s == crate::quick::Step::Done)
        && crate::quick::matching_preset(state, state.quick_channel).is_none()
}

/// The name "Keep this as a mic preset" saves under: the input's name and
/// "mic" ("AN1 mic"), numbered past one already taken.
pub fn kept_mic_name(state: &TuxMix, idx: usize) -> String {
    let base = state.device.inputs().get(idx).map_or_else(|| "Mic".to_string(), |ch| format!("{} mic", ch.name));
    let taken = |name: &str| state.mic_presets.iter().any(|p| p.name.eq_ignore_ascii_case(name));
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base} {n}")).find(|name| !taken(name)).unwrap_or(base)
}

/// "Keep this as a mic preset" pressed: Quick setup's input saved as a new
/// mic preset, the mic guide marked done, a rising note (a real win, the
/// guides' finish), and a notice saying where it loads from.
pub fn keep_mic_preset(state: &mut TuxMix) {
    let ChannelId::Input(idx) = state.quick_channel else {
        return;
    };
    let name = kept_mic_name(state, idx);
    state.mic_preset_name = name.clone();
    crate::app::save_mic_preset(state, idx);
    if !done(state, "mic") {
        state.guides_done.push("mic".to_string());
    }
    state.prefs_dirty = true;
    let (step, _) = crate::celebrate::next_win(state, true);
    if crate::celebrate::sounds_on(state) {
        crate::sounds::play(crate::sounds::Sound::Rise(step.min(7)), state.ui_sound_db - 4.0);
    }
    // That rise was the win: the first-preset milestone it reached is kept
    // quietly, as a finished guide's are (the scout: a second rise played
    // on the next tick, behind the notice below).
    if state.milestones_seeded && !crate::celebrate::reached(state, "mic-preset") {
        state.milestones.push("mic-preset".into());
    }
    let input = state.device.inputs().get(idx).map_or_else(String::new, |ch| ch.name.clone());
    state.notice = Some((
        format!("{name} saved: load it from {input}'s Settings, or at every start (Audra settings, Mixer)."),
        Instant::now(),
    ));
}

/// The "Next for you" card in the counter's slot: one line, the guide's
/// title, Start and ×; or, the mic set up in Quick setup, "Keep this as a
/// mic preset" and Save (`keep_mic_offered`).
pub fn next_card(state: &TuxMix) -> Option<Element<'_, Message>> {
    if !state.tips {
        return None;
    }
    let i = next_for_you(state)?;
    let keep = keep_mic_offered(state);
    // In Quick setup, the mic guide is the page you're on (critique #35:
    // "Next for you: Set up my mic" stood beside its own steps).
    if state.view == crate::app::View::Quick && GUIDES[i].id == "mic" && !keep {
        return None;
    }
    let scale = state.ui_scale;
    let lamp = theme::readable_on_floors(theme::lamp());
    // The input named, not "this": the card shows on every page, and in
    // the Matrix "this" pointed at nothing (an earlier critique).
    let title: String = if keep {
        let name = crate::app::short_label(&crate::app::strip_display_name(state, state.quick_channel)).to_string();
        format!("Keep {name}'s setup as a mic preset")
    } else {
        GUIDES[i].title.to_string()
    };
    let line = iced::widget::rich_text::<(), Message, iced::Theme, iced::Renderer>([
        iced::widget::span("Next for you:").font(theme::SANS_SEMI).color(lamp),
        iced::widget::span(" "),
        iced::widget::span(title).color(theme::text_primary()),
    ])
    .size(theme::TEXT_SM * scale)
    .width(Length::Fill);
    let go = if keep {
        crate::widgets::strip::hint(
            capsule("Save", scale, Message::KeepMicPreset),
            "Saves this input's gain, 48V, PAD and EQ as a mic preset, so it loads in one press next time (or at every start). Voice keeps its own settings, under its Preset",
            scale,
        )
    } else {
        capsule("Start", scale, Message::GuideStart(i))
    };
    Some(
        container(
            row![
                line,
                go,
                crate::widgets::strip::hint(
                    button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                        .padding(0)
                        .width(Length::Fixed(20.0 * scale))
                        .height(Length::Fixed(20.0 * scale))
                        .style(theme::icon_button(false))
                        .on_press(Message::NextSetAside(GUIDES[i].id)),
                    "Not now (Help lists every guide)",
                    scale,
                ),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        )
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .width(Length::Fill)
        .style(theme::trick_card)
        .into(),
    )
}

fn capsule(label: &'static str, scale: f32, message: Message) -> Element<'static, Message> {
    button(text(label).size(theme::TEXT_SM * scale))
        .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
        .style(theme::page_key(11.0 * scale))
        .on_press(message)
        .into()
}

/// The guides done as LED segments (the tricks' and milestones' manner):
/// lit in `signal`; on a finished card the guide just done lights last in
/// the lamp, with a bloom, after its steps' sweep.
fn ladder(state: &TuxMix, newest: Option<usize>, t: f32, scale: f32) -> Element<'static, Message> {
    use crate::celebrate::{Seg, Segments};
    let ms = crate::celebrate::burst_ms(t);
    let at = crate::celebrate::NEWEST_AT_MS;
    let lit = GUIDES
        .iter()
        .enumerate()
        .map(|(i, g)| {
            if Some(i) == newest {
                let k = ((ms - at) / crate::celebrate::SEG_LIGHT_MS).clamp(0.0, 1.0);
                let bloom = if ms < at { 0.0 } else { 1.0 - ((ms - at) / crate::celebrate::BLOOM_MS).clamp(0.0, 1.0) };
                Seg::Newest(k, bloom)
            } else if done(state, g.id) {
                Seg::Lit(1.0)
            } else {
                Seg::Ghost
            }
        })
        .collect();
    let lit = crate::celebrate::fill_from_left(lit);
    canvas(Segments { lit, scale })
        .width(Length::Fixed(Segments::width(GUIDES.len(), scale)))
        .height(Length::Fixed(Segments::height(scale)))
        .into()
}

/// How lit step `i`'s ring is `t` into the finish: the steps go dark and
/// relight in order, Store's sweep (each over 140ms, the next 28ms later).
fn sweep(i: usize, t: f32) -> f32 {
    let from = crate::celebrate::CASCADE_FROM_MS + i as f32 * crate::celebrate::SEG_STAGGER_MS;
    ((crate::celebrate::burst_ms(t) - from) / crate::celebrate::SEG_LIGHT_MS).clamp(0.0, 1.0)
}

/// The counter page a guide under way teaches on, while it is open: the
/// counter gives it room before its head (at 1280x700 the head alone left
/// "Press Store" out of sight).
pub(crate) fn teaching_page(state: &TuxMix) -> Option<crate::sidebar::Tab> {
    // Where the pages float, a page is a card and the snapshots' keys
    // stand in the head: no room is needed in the column.
    guide_page(state)
        .filter(|&tab| state.sidebar_tab == Some(tab))
        .filter(|_| !crate::sidebar::pages_float(state))
}

/// The counter page a guide under way teaches on, open or not (its start
/// opens it; closed again, its card's "Show me" opens it).
pub(crate) fn guide_page(state: &TuxMix) -> Option<crate::sidebar::Tab> {
    let run = state.guide.as_ref().filter(|r| r.finished.is_none())?;
    match GUIDES.get(run.guide)?.id {
        "reverb" => Some(crate::sidebar::Tab::Fx),
        "snapshot" => Some(crate::sidebar::Tab::Snapshots),
        _ => None,
    }
}

/// About how tall `card` draws, at scale 1, with the slot's padding: the
/// counter keeps that room for it under its head (`sidebar::foot_slot_h`).
/// A little over rather than under, so the zoom bar is never pushed off.
pub(crate) fn card_h(state: &TuxMix) -> f32 {
    let Some(run) = state.guide.as_ref() else { return 0.0 };
    let Some(g) = GUIDES.get(run.guide) else { return 0.0 };
    // Words at TEXT_SM run about 6.5px a character in the card's width.
    // (The slot lines up with the column's edges, critique #27: 20px less.)
    let per_line = ((state.sidebar_width - 76.0) / 6.5).max(20.0);
    let lines = |w: &str, less: f32| (w.chars().count() as f32 / (per_line - less)).ceil().max(1.0);
    const LINE: f32 = 15.0;
    // Slot padding, card padding, the head row.
    let frame = 8.0 + 12.0 + 18.0;
    if run.finished.is_some() {
        let milestone = if run.milestone.is_some() && !crate::app::short_window(state) { 4.0 + LINE } else { 0.0 };
        if crate::app::short_window(state) {
            // The title, the rings beside "More guides", the milestone.
            return frame + 4.0 + 32.0 + milestone;
        }
        let steps: f32 = g.steps.iter().map(|st| 4.0 + LINE * lines(voiced(state, st.words), 3.0)).sum();
        frame + 4.0 + LINE * lines(voiced(state, g.praise), 0.0) + steps + milestone + 4.0 + 24.0
    } else {
        let now = steps_done(state, run);
        if crate::app::short_window(state) {
            // The rings in the head; "Show me" beside the words (about
            // 11 characters' room).
            let next = now.iter().position(|d| !d).map_or(0.0, |i| {
                let st = &g.steps[i];
                let less = if st.go.is_some() { 11.0 } else { 0.0 };
                4.0 + (LINE * lines(voiced(state, st.words), less)).max(if st.go.is_some() { 22.0 } else { 0.0 })
            });
            // A little over, as the slot's estimates are (two lines beside
            // "Show me" drew 4px taller than counted).
            return frame + next + 4.0;
        }
        let next = now.iter().position(|d| !d).map_or(0.0, |i| {
            let st = &g.steps[i];
            4.0 + LINE * lines(voiced(state, st.words), 0.0) + if st.go.is_some() { 4.0 + 20.0 } else { 0.0 }
        });
        frame + 4.0 + 14.0 + next
    }
}

/// The card at the window's foot (where the tricks card stands): while
/// under way, the guide's name, how many steps are done, its steps (each
/// with a ring that ticks; the next one in the ink, with "Show me" where it
/// can take you). Once done it stays the guide's own card, lit: it lands
/// in the lamp's light (as a milestone's does), its name gives way to the
/// done title in the lamp's amber, its steps relight in a sweep, a
/// milestone reached in the same moment is named, and the guides' ladder
/// lights the one just done. On a short window, once done: the title, the
/// steps as rings beside "More guides", and the milestone if one.
pub fn card(state: &TuxMix) -> Option<Element<'_, Message>> {
    if state.notice.is_some() {
        return None;
    }
    let run = state.guide.as_ref()?;
    let g = GUIDES.get(run.guide)?;
    let scale = state.ui_scale;
    let finished_t = run.finished.map(|at| {
        if !state.animations {
            1.0
        } else if let Some(t) = state.capture_burst_t {
            t
        } else {
            (at.elapsed().as_secs_f32() / crate::celebrate::BURST_FOR.as_secs_f32()).min(1.0)
        }
    });
    let now = steps_done(state, run);
    let next = if finished_t.is_some() { None } else { now.iter().position(|d| !d) };
    let lamp = theme::readable_on_floors(theme::lamp());
    let head = if let (Some(t), true) = (finished_t, run.finale) {
        // The last of them: the biggest moment the cards have, the
        // milestones' burst at its fullest beside the title (critique #56:
        // the finale was quieter than a small milestone).
        let badge = iced::widget::canvas(crate::celebrate::Burst { t, scale: scale * 0.6, seed: 9, level: 3 })
            .width(Length::Fixed(crate::celebrate::BADGE * 0.6 * scale))
            .height(Length::Fixed(crate::celebrate::BADGE * 0.6 * scale));
        row![
            badge,
            text("Every guide done").font(theme::SERIF).size(theme::TEXT_ROW * scale).color(lamp),
            Space::new().width(Length::Fill),
            close_key(scale, "Close (Esc)", Message::GuideStop),
        ]
    } else if finished_t.is_some() {
        row![
            text(g.done_title).font(theme::SERIF).size(theme::TEXT_ROW * scale).color(lamp),
            Space::new().width(Length::Fill),
            close_key(scale, "Close (Esc)", Message::GuideStop),
        ]
    } else {
        let n = now.iter().filter(|d| **d).count();
        // On a short window the steps' rings stand in the head, for
        // "n of m done" (critique #8: the card took the page keys' room).
        let progress: Element<'_, Message> = if crate::app::short_window(state) {
            // The step it is on marked here too (critique #88 A P3-1).
            let at = now.iter().position(|d| !d);
            now.iter()
                .enumerate()
                .fold(row![].spacing(theme::SPACE_TIGHT * scale), |r, (i, d)| {
                    r.push(if at == Some(i) { crate::celebrate::mark_current(scale) } else { crate::celebrate::mark(*d, scale) })
                })
                .into()
        } else {
            text(format!("{n} of {} done", g.steps.len())).size(theme::TEXT_SM * scale).color(theme::text_sec()).into()
        };
        row![
            text(g.title).font(theme::SERIF).size(theme::TEXT_MD * scale).color(theme::text_primary()),
            Space::new().width(Length::Fill),
            progress,
            crate::keyfocus::ring(
                close_key(scale, "Stop this guide (Help has them all)", Message::GuideStop),
                crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::GuideStop),
                theme::RADIUS_SM * scale,
            ),
        ]
    }
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Center);
    let short = crate::app::short_window(state);
    let mut list = column![head].spacing(theme::SPACE_SM * scale);
    if finished_t.is_some() && !short && !run.finale {
        list = list.push(text(voiced(state, g.praise)).size(theme::TEXT_SM * scale).color(theme::text_primary()));
    }
    if let (Some(t), true) = (finished_t, short) {
        // Finished, on a short window: the steps as a row of rings
        // relighting in their sweep beside "More guides", so the card
        // leaves the Control Room its room (critique #7).
        let rings = (0..g.steps.len()).fold(row![].spacing(theme::SPACE_TIGHT * scale), |r, i| r.push(crate::celebrate::mark_lit(sweep(i, t), scale)));
        list = list.push(
            row![rings, Space::new().width(Length::Fill), capsule(if run.finale { "All guides" } else { "More guides" }, scale, Message::ToggleAbout)]
                .spacing(theme::SPACE_SM * scale)
                .align_y(iced::Alignment::Center),
        );
    } else if run.finale && finished_t.is_some() {
        // The finale: what it means and where to go on, not the last
        // guide's steps again.
        let (found, of) = crate::tricks::found_count(state);
        let n = GUIDES.len();
        list = list.push(text(format!("{n} of {n} guides: you know your way round the desk.")).size(theme::TEXT_SM * scale).color(theme::text_primary()));
        list = list.push(text(format!("Next: {found} of {of} tricks found. F1 lists every key and gesture.")).size(theme::TEXT_SM * scale).color(theme::text_sec()));
    } else if let Some(t) = finished_t {
        // Finished: every step, relighting in its sweep.
        for (i, st) in g.steps.iter().enumerate() {
            let words = text(voiced(state, st.words)).size(theme::TEXT_SM * scale).color(theme::text_sec()).width(Length::Fill);
            list = list.push(
                row![crate::celebrate::mark_lit(sweep(i, t), scale), words].spacing(theme::SPACE_SM * scale).align_y(iced::Alignment::Start),
            );
        }
    } else {
        // Under way, compact, so the counter keeps its room for the page the
        // guide is teaching (the reviewer: at 1600x900 a full list pushed
        // the Snapshots slots and Store out of view while the step said
        // "Press Store"): the steps as a row of rings that tick, then only
        // the next one, its words and "Show me".
        if !short {
            let rings = now.iter().enumerate().fold(row![].spacing(theme::SPACE_TIGHT * scale), |r, (i, d)| {
                r.push(if next == Some(i) { crate::celebrate::mark_current(scale) } else { crate::celebrate::mark(*d, scale) })
            });
            list = list.push(rings);
        }
        if let Some(i) = next {
            let st = &g.steps[i];
            let words = text(voiced(state, st.words)).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI).color(theme::text_primary()).width(Length::Fill);
            let show = st.go.is_some().then(|| {
                let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::GuideGo);
                crate::keyfocus::ring(capsule("Show me", scale, Message::GuideGo), focused, 11.0 * scale)
            });
            // On a short window "Show me" stands beside the words.
            let step: Element<'_, Message> = match (short, show) {
                (true, Some(show)) => row![words, show].spacing(theme::SPACE_SM * scale).align_y(iced::Alignment::Center).into(),
                (false, Some(show)) => column![words, show].spacing(theme::SPACE_SM * scale).width(Length::Fill).into(),
                (_, None) => words.into(),
            };
            list = list.push(step);
        }
    }
    let body: Element<'_, Message> = if let Some(t) = finished_t {
        // Not on a short window: the milestone shows in Help, and its line
        // took the page keys' room (critique #28).
        if let Some(m) = run.milestone.filter(|_| !short) {
            list = list.push(
                row![
                    text("Milestone:").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).color(lamp),
                    text(m).size(theme::TEXT_SM * scale).color(theme::text_primary()),
                ]
                .spacing(theme::SPACE_SM * scale),
            );
        }
        let n = GUIDES.iter().filter(|g| done(state, g.id)).count();
        let shown = if crate::celebrate::newest_lit(t) { n } else { n.saturating_sub(1) };
        if !short {
            list = list.push(
                row![
                    // All nine alike at the finale: no newest one left
                    // looking unfinished.
                    ladder(state, if run.finale { None } else { Some(run.guide) }, t, scale),
                    text(format!("{shown} of {} guides", GUIDES.len())).size(theme::TEXT_SM * scale).color(theme::text_sec()),
                    // None left after the last: Help lists what was learned.
                    capsule(if run.finale { "All guides" } else { "More guides" }, scale, Message::ToggleAbout),
                ]
                .spacing(theme::SPACE_SM * scale)
                .align_y(iced::Alignment::Center),
            );
        }
        let glow = 0.3 + 0.7 * (1.0 - t).powf(1.5);
        container(
            container(list)
                .padding([theme::SPACE_MD * scale, theme::SPACE_MD * scale])
                .width(Length::Fill)
                .style(theme::milestone_card(glow)),
        )
        .style(theme::lamp_halo(glow))
        .into()
    } else {
        container(list)
            .padding([theme::SPACE_MD * scale, theme::SPACE_MD * scale])
            .width(Length::Fill)
            .style(theme::trick_card)
            .into()
    };
    Some(mouse_area(body).on_enter(Message::GuideHeld(true)).on_exit(Message::GuideHeld(false)).into())
}

/// About's "How do I…": each guide with a ring ticked once done, and a key
/// that starts it; the guides done as a ladder by the head.
pub fn about_list(state: &TuxMix, scale: f32) -> Element<'static, Message> {
    let n = GUIDES.iter().filter(|g| done(state, g.id)).count();
    // Quick help first, under its own heading: the rescue rows aren't guides
    // and sat under the guides' heading and ladder (critiques #101 and #102 A).
    let mut list = column![text("Quick help").font(theme::SERIF).color(theme::text_primary()).size(theme::TEXT_MD * scale)]
        .spacing(theme::SPACE_SM * scale);
    // The rescue first: "I can't hear it" from anywhere, not only from Quick
    // setup's step 3 (critique #99 A P2-5: sound lost in the Mixer or the
    // Matrix had no way to it). The same press and notice as step 3's.
    // Two keys, where the sound is missing: one listening output assumed,
    // silent speakers were "fixed" in the headphones (critique #100 A P2-1).
    let assign = state.control_room.assign;
    // Apart enough to tell (critique #101 B: 3 px between the capsules).
    let mut keys = row![].spacing(theme::SPACE_MD * scale);
    keys = keys.push(crate::keyfocus::reach_press(
        "help-hearfix-speakers",
        // The speakers that play: Main B while Speaker B is on (#102 A P3-2).
        capsule("In my speakers", scale, Message::HearFixOn(crate::control_room::monitor_pair(state))),
        Some(Message::HearFixOn(crate::control_room::monitor_pair(state))),
        11.0 * scale,
    ));
    if let Some(&phones) = assign.phones_pairs().first() {
        keys = keys.push(crate::keyfocus::reach_press(
            "help-hearfix-phones",
            capsule("In my headphones", scale, Message::HearFixOn(phones)),
            Some(Message::HearFixOn(phones)),
            11.0 * scale,
        ));
    }
    list = list.push(
        row![
            text("Can\u{2019}t hear anything? Say where: one press brings back what Audra holds back (a mic is kept out of the speakers, where it could howl). Undo puts it back.")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_primary())
                .width(Length::Fill),
            keys,
        ]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center),
    );
    // And the way back to a clean mix (critique #100 A P3-10: Start fresh was
    // five scrolls down in Settings, with no way in from Help).
    list = list.push(
        row![
            text("Start over? Start fresh sets a clean, safe mix; snapshots and names stay.")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_primary())
                .width(Length::Fill),
            crate::keyfocus::reach_press(
                "help-start-over",
                capsule("Show me", scale, Message::OpenStartOver),
                Some(Message::OpenStartOver),
                11.0 * scale,
            ),
        ]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center),
    );
    // A loudness cap for headphones, where a parent looks first (critique
    // #105 A P3-11: it was three clicks deep in the Phones' D panel).
    list = list.push(
        row![
            text("Keep headphones gentle? Volume protection caps how loud apps (calls, videos) play in them.")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_primary())
                .width(Length::Fill),
            crate::keyfocus::reach_press(
                "help-phones-cap",
                capsule("Show me", scale, Message::OpenPhonesCap),
                Some(Message::OpenPhonesCap),
                11.0 * scale,
            ),
        ]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center),
    );
    // Room before the guides (critique #101 B: Show me 3 px above the
    // first Start, centres 23.5 px apart).
    list = list.push(Space::new().height(Length::Fixed(theme::SPACE_SM * scale)));
    list = list.push(
        row![
            // Not the tab's own words again (critique #35): what the page is for.
            text("Learn a task by doing it").font(theme::SERIF).color(theme::text_primary()).size(theme::TEXT_MD * scale),
            Space::new().width(Length::Fill),
            ladder(state, None, 1.0, scale),
            text(format!("{n} of {} guides", GUIDES.len())).size(theme::TEXT_SM * scale).color(theme::text_sec()),
        ]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center),
    );
    // A little room under the heading (critique #103 B: 4 px to the first
    // guide).
    list = list.push(Space::new().height(Length::Fixed(theme::SPACE_TIGHT * scale)));
    // Every one done: a lasting line, as the milestones' finale.
    if n == GUIDES.len() {
        let on = state.guides_all_on.as_deref().map_or(String::new(), |d| format!(", on {d}"));
        list = list.push(
            text(format!("All {} guides done{on}. You know your way round the desk.", GUIDES.len()))
                .font(theme::SANS_SEMI)
                .size(theme::TEXT_SM * scale)
                .color(theme::readable_on_floors(theme::lamp())),
        );
    }
    // The mic first, as "Next for you" offers it first on a first run
    // (critique #27: fourth of eight here); the rest as they stand.
    let shown = GUIDES.iter().enumerate().filter(|(_, g)| g.id == "mic").chain(GUIDES.iter().enumerate().filter(|(_, g)| g.id != "mic"));
    // Where to begin, marked: the guide "Next for you" would offer (critique
    // #56: nine of equal weight, none saying "start here").
    let suggested = next_for_you(state);
    let first_time = state.guides_done.is_empty();
    for (i, g) in shown {
        let d = done(state, g.id);
        let running = state.guide.as_ref().is_some_and(|r| r.guide == i && r.finished.is_none());
        let key: Element<'static, Message> = if running {
            text("Under way").size(theme::TEXT_SM * scale).color(theme::text_sec()).into()
        } else {
            // A stop for the keyboard in About (`keyfocus::Region::About`).
            crate::keyfocus::reach_press(
                g.id,
                capsule(if d { "Again" } else { "Start" }, scale, Message::GuideStart(i)),
                Some(Message::GuideStart(i)),
                11.0 * scale,
            )
        };
        list = list.push(
            row![
                crate::celebrate::mark(d, scale),
                {
                    let title = text(g.title).color(theme::text_primary()).size(theme::TEXT_SM * scale);
                    let el: Element<'static, Message> = if suggested == Some(i) && !running {
                        // A small tag in front of the title, outlined in the
                        // lamp, so it reads as a pointer and not as part of
                        // the guide's name (critique #57: set apart only by
                        // colour and weight, 6 px after "keep it").
                        let ink = theme::readable_on_floors(theme::lamp());
                        let tag = container(
                            text(if first_time { "Start here" } else { "Next" })
                                .font(theme::SANS_SEMI)
                                .size(theme::TEXT_SM * scale)
                                .color(ink)
                                .wrapping(iced::advanced::text::Wrapping::None),
                        )
                        .padding([0.0, theme::SPACE_MD * scale])
                        .style(move |_t: &iced::Theme| iced::widget::container::Style {
                            border: iced::Border { color: ink, width: 1.0, radius: (8.0 * scale).into() },
                            ..iced::widget::container::Style::default()
                        });
                        row![tag, title]
                            .spacing(theme::SPACE_SM * scale)
                            .align_y(iced::Alignment::Center)
                            .width(Length::Fill)
                            .into()
                    } else {
                        title.width(Length::Fill).into()
                    };
                    el
                },
                text(format!("{} steps", g.steps.len())).color(theme::text_sec()).size(theme::TEXT_SM * scale),
                key,
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        );
    }
    list.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_voiced_line_is_a_real_line_and_none_keeps_the_old_words() {
        // Critique #87 A P3-3: `voiced` matches whole strings, so an edited
        // guide line would fall back to "the Phones" with nothing to catch it.
        let mut state = crate::app::new(true, None, None);
        state.plain_names = true;
        let lines: Vec<&'static str> = GUIDES
            .iter()
            .flat_map(|g| g.steps.iter().map(|s| s.words).chain([g.praise]))
            .chain(crate::celebrate::MILESTONES.iter().flat_map(|m| [m.praise, m.how]))
            .collect();
        let changed = lines.iter().filter(|l| voiced(&state, l) != **l).count();
        assert!(changed >= 6, "every key in `voiced` matches a line: {changed}");
        for l in &lines {
            let v = voiced(&state, l);
            assert!(!["Phones mix", "the Phones up", "in the Phones", "into the Phones", "computer's sound"].iter().any(|old| v.contains(old)), "{v}");
        }
        state.plain_names = false;
        assert!(lines.iter().all(|l| voiced(&state, l) == *l));
    }
    use crate::app::{new, update};

    fn start(state: &mut TuxMix, id: &str) {
        let i = GUIDES.iter().position(|g| g.id == id).unwrap();
        let _ = update(state, Message::GuideStart(i));
    }

    #[test]
    fn a_snapshot_guide_runs_through_update_and_is_kept_once_done() {
        let mut state = new(true, None, None);
        start(&mut state, "snapshot");
        tick(&mut state);
        assert_eq!(state.guide.as_ref().map(|r| r.noted.clone()), Some(vec![false, false, false]));
        let _ = update(&mut state, Message::SnapshotStore);
        tick(&mut state);
        assert_eq!(state.guide.as_ref().map(|r| r.noted.clone()), Some(vec![true, false, false]), "Store is the first step");
        // The slot press becomes a store after Store, a recall otherwise.
        let _ = update(&mut state, Message::SnapshotSlotPressed(3));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_none() && r.noted[1]));
        let _ = update(&mut state, Message::SnapshotSlotPressed(3));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()), "stored, then recalled: done");
        assert!(done(&state, "snapshot"));
        assert!(card(&state).is_some());
    }

    #[test]
    fn a_live_step_unticks_when_it_stops_being_true() {
        let mut state = new(true, None, None);
        start(&mut state, "phones");
        let phones = phones(&state).expect("the unit has a Phones pair");
        let other = (0..8).find(|p| *p != phones).unwrap();
        let _ = update(&mut state, Message::SelectOutput(phones));
        let run = state.guide.clone().unwrap();
        assert!(steps_done(&state, &run)[0], "the Phones mix picked");
        let _ = update(&mut state, Message::SelectOutput(other));
        assert!(!steps_done(&state, &run)[0], "another mix picked: not any more");
    }

    #[test]
    fn the_keyboard_guide_counts_m_twice() {
        let mut state = new(true, None, None);
        start(&mut state, "keys");
        state.focused_strip = Some(ChannelId::Input(0));
        let _ = update(&mut state, Message::KeyLetter('m'));
        let run = state.guide.clone().unwrap();
        assert!(!steps_done(&state, &run)[2]);
        let _ = update(&mut state, Message::KeyLetter('m'));
        let run = state.guide.clone().unwrap();
        assert!(steps_done(&state, &run)[2], "M, and M again");
    }

    #[test]
    fn while_a_guide_runs_milestones_wait_and_tricks_keep_quiet() {
        let mut state = new(true, None, None);
        crate::celebrate::check(&mut state);
        start(&mut state, "keys");
        // A new first while the guide runs: kept for later.
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        crate::celebrate::check(&mut state);
        assert!(state.celebration.is_none(), "the milestone waits for the guide");
        // A trick found: counted, no card.
        state.focused_strip = Some(ChannelId::Input(0));
        let _ = update(&mut state, Message::KeyLetter('m'));
        assert!(state.tricks_found.iter().any(|f| f == "ms-keys"));
        assert!(state.trick_card.is_none());
        // The guide stopped: the milestone lands.
        let _ = update(&mut state, Message::GuideStop);
        crate::celebrate::check(&mut state);
        assert!(state.celebration.is_some());
    }

    #[test]
    fn show_me_only_shows() {
        let mut state = new(true, None, None);
        start(&mut state, "reverb");
        state.sidebar_open = false;
        let _ = update(&mut state, Message::GuideGo);
        assert!(state.sidebar_open && state.sidebar_tab == Some(crate::sidebar::Tab::Fx), "Reverb & Echo shown");
        let _ = update(&mut state, Message::GuideGo);
        assert_eq!(state.sidebar_tab, Some(crate::sidebar::Tab::Fx), "pressed again: still shown, never closed");
        assert!(!state.fx.reverb_on, "but not switched on");
    }

    #[test]
    fn a_milestone_reached_as_a_guide_finishes_is_said_on_its_card_once() {
        let mut state = new(true, None, None);
        crate::celebrate::check(&mut state);
        assert!(!crate::celebrate::reached(&state, "first-snapshot"));
        start(&mut state, "snapshot");
        let _ = update(&mut state, Message::SnapshotStore);
        let _ = update(&mut state, Message::SnapshotSlotPressed(2));
        let _ = update(&mut state, Message::SnapshotSlotPressed(2));
        tick(&mut state);
        assert_eq!(state.guide.as_ref().and_then(|r| r.milestone), Some("Your first snapshot"));
        assert!(crate::celebrate::reached(&state, "first-snapshot"), "kept");
        // When the guide's card goes, no second card for the same win.
        let _ = update(&mut state, Message::GuideStop);
        crate::celebrate::check(&mut state);
        assert!(state.celebration.is_none());
    }

    #[test]
    fn the_steps_relight_in_order_on_the_finish() {
        assert_eq!(sweep(0, 0.0), 0.0);
        assert!(sweep(0, 0.2) > sweep(1, 0.2), "in order");
        assert_eq!(sweep(3, 1.0), 1.0);
    }

    #[test]
    fn show_me_for_the_mic_preset_lands_on_its_row() {
        let mut state = new(true, None, None);
        if let Some(m) = mic_settings(&mut state) {
            let _ = update(&mut state, m);
        }
        let cid = mic(&state);
        assert!(state.flyout_open.contains(&(cid, crate::widgets::strip::FlyoutKind::Settings)), "its Settings open");
        assert!(crate::keyfocus::panel_focused(&state, cid, crate::keyfocus::PanelItem::MicPreset), "the ring on Mic preset");
    }

    #[test]
    fn the_voice_guides_last_show_me_says_where_to_go() {
        // The owner, 2026-11-13: "Show me" at the last step did nothing
        // (the panel was open already; the step is in another app).
        let mut state = new(true, None, None);
        start(&mut state, "voice");
        if let Some(m) = voice_panel(&mut state) {
            let _ = update(&mut state, m);
        }
        state.notice = None;
        let _ = voice_app(&mut state);
        let said = state.notice.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        assert!(said.contains("Audra processed mic") && said.contains("audio settings"), "{said}");
        assert!(state.flyout_open.contains(&(mic(&state), crate::widgets::strip::FlyoutKind::Dynamics)), "the panel stays open");
    }

    #[test]
    fn the_voice_guide_opens_d_picks_a_preset_and_fits() {
        // The owner's pick after critique #34: a guide a podcaster finds.
        use crate::dynamics_panel::DynMsg;
        let mut state = new(true, None, None);
        start(&mut state, "voice");
        tick(&mut state);
        assert_eq!(state.guide.as_ref().map(|r| r.noted.clone()), Some(vec![false, false, false, false]));
        if let Some(m) = voice_panel(&mut state) {
            let _ = update(&mut state, m);
        }
        let an1 = mic(&state);
        assert!(state.flyout_open.contains(&(an1, crate::widgets::strip::FlyoutKind::Dynamics)), "Show me opens D");
        tick(&mut state);
        assert_eq!(state.guide.as_ref().map(|r| r.noted.clone()), Some(vec![true, false, false, false]));
        let _ = update(&mut state, Message::Dyn(an1, DynMsg::Preset(0)));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.noted[1] && r.finished.is_none()), "a preset turns Voice on");
        // Ten seconds of talking in a quiet room, as Fit's own tests talk.
        let talk: Vec<(f32, f32)> = (0..300)
            .map(|i| if i % 3 == 0 { (-56.0, -50.0) } else { (-34.0 + (i % 19) as f32, -24.0 + (i % 19) as f32) })
            .collect();
        state.dyn_fit_report = crate::dynamics_panel::fit_from(&talk).ok();
        assert!(state.dyn_fit_report.is_some(), "a fit");
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.noted[2] && r.finished.is_none()), "fitted, but no app hears it yet");
        // An app records from "Audra processed mic" (on the unit, the
        // watch finds it; here the test says so).
        state.dyn_heard = Some(true);
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()), "an app hears it: done");
        assert!(done(&state, "voice"));
    }

    #[test]
    fn the_lock_guide_ticks_lock_a_held_fader_and_unlock() {
        let mut state = new(true, None, None);
        start(&mut state, "lock");
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, 0.5));
        // The padlock's shake is long over: the step stays done.
        state.lock_nudge = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        crate::motion::prune(&mut state);
        let run = state.guide.clone().unwrap();
        assert_eq!(steps_done(&state, &run), vec![true, true, false], "locked, and a fader held");
        let _ = update(&mut state, Message::SetUiLock(false));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()));
        // Unlocked, "The mix is locked" went with the lock, so the finished
        // card and its rise come at once (the reviewer, 2026-11-14: the rise
        // played while the stale notice hid the card).
        assert!(state.notice.is_none(), "{:?}", state.notice);
        assert!(card(&state).is_some());
        assert!(state.guide.as_ref().is_some_and(|r| r.rise.is_none()), "its notes played with it");
    }

    #[test]
    fn a_guide_finished_behind_a_notice_keeps_its_rise_for_its_card_and_never_replays() {
        let mut state = new(true, None, None);
        start(&mut state, "lock");
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, 0.5));
        state.lock_nudge = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        crate::motion::prune(&mut state);
        let _ = update(&mut state, Message::SetUiLock(false));
        // A notice that isn't the lock's, up as it finishes.
        state.notice = Some(("Something else.".into(), Instant::now()));
        crate::sounds::TEST_HEARD.with(|h| h.borrow_mut().clear());
        tick(&mut state);
        let rise = state.guide.as_ref().and_then(|r| r.rise);
        assert!(rise.is_some(), "its notes wait");
        assert!(card(&state).is_none(), "behind the notice");
        let heard = || crate::sounds::TEST_HEARD.with(|h| h.borrow().clone());
        assert!(!heard().iter().any(|s| matches!(s, crate::sounds::Sound::Rise(_))), "not yet: {:?}", heard());
        // Read for 1.5 s, the plain notice gives way, and the rise plays.
        state.notice = Some(("Something else.".into(), Instant::now().checked_sub(std::time::Duration::from_secs(2)).unwrap()));
        tick(&mut state);
        assert!(state.notice.is_none());
        assert!(state.guide.as_ref().is_some_and(|r| r.rise.is_none()));
        assert!(heard().contains(&crate::sounds::Sound::Rise(rise.unwrap())), "{:?}", heard());
        // Shown, then a new notice: held settled, so its burst won't replay.
        state.notice = Some(("Later.".into(), Instant::now()));
        tick(&mut state);
        let at = state.guide.as_ref().and_then(|r| r.finished).unwrap();
        assert!(at.elapsed() >= crate::celebrate::BURST_FOR, "settled, not restarted");
    }

    #[test]
    fn finishing_the_last_guide_is_a_finale_once() {
        let mut state = new(true, None, None);
        state.animations = true;
        state.notice = None;
        // Every guide but the Lock done.
        state.guides_done = GUIDES.iter().filter(|g| g.id != "lock").map(|g| g.id.to_string()).collect();
        start(&mut state, "lock");
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, 0.5));
        state.lock_nudge = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        crate::motion::prune(&mut state);
        let _ = update(&mut state, Message::SetUiLock(false));
        state.boot_sweep = None;
        crate::sounds::TEST_HEARD.with(|h| h.borrow_mut().clear());
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finale), "the card says so");
        assert!(state.guides_all_on.is_some(), "the day kept for Help");
        assert!(state.boot_sweep.is_some(), "the light test runs across the desk");
        let heard = crate::sounds::TEST_HEARD.with(|h| h.borrow().clone());
        assert!(heard.contains(&crate::sounds::Sound::Rise(crate::celebrate::RISE_TOP)), "from the top: {heard:?}");
        // Again later: no second finale.
        state.guide = None;
        start(&mut state, "lock");
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, 0.5));
        state.lock_nudge = Instant::now().checked_sub(std::time::Duration::from_secs(2));
        crate::motion::prune(&mut state);
        let _ = update(&mut state, Message::SetUiLock(false));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some() && !r.finale));
    }

    #[test]
    fn voice_with_no_app_suggests_the_voice_guide_next() {
        // The owner: "keep improving the fun, teaching users": Voice on with
        // no app hearing it, the voice guide (it ends on the call app's mic).
        let mut state = new(true, None, None);
        state.guides_done.clear();
        state.dyn_heard = Some(false);
        let voice = GUIDES.iter().position(|g| g.id == "voice");
        assert_eq!(next_for_you(&state), voice);
        state.dyn_heard = Some(true);
        assert_ne!(next_for_you(&state), voice, "an app hears it: something else");
    }

    #[test]
    fn two_padlock_clicks_dont_finish_the_lock_guide() {
        // OpenCode's job 24: "Try a fader" ticked on the unlock.
        let mut state = new(true, None, None);
        start(&mut state, "lock");
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::SetUiLock(false));
        tick(&mut state);
        let run = state.guide.clone().unwrap();
        assert_eq!(steps_done(&state, &run), vec![true, false, true], "no fader was tried");
        assert!(run.finished.is_none());
    }

    #[test]
    fn the_snapshot_guide_wants_the_slot_it_stored() {
        // Job 24: any slot's recall ticked "press that slot again".
        let mut state = new(true, None, None);
        start(&mut state, "snapshot");
        let _ = update(&mut state, Message::SnapshotStore);
        let _ = update(&mut state, Message::SnapshotSlotPressed(3));
        let run = state.guide.clone().unwrap();
        let mut other = run.clone();
        other.seen.push("recalled");
        assert!(!steps_done(&state, &other)[2], "another slot doesn't count");
        let _ = update(&mut state, Message::SnapshotSlotPressed(3));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()), "that slot does");
    }

    #[test]
    fn the_keys_guide_counts_m_not_s() {
        // Job 24: S presses counted toward "M mutes it, and M again".
        let mut state = new(true, None, None);
        start(&mut state, "keys");
        let mut run = state.guide.clone().unwrap();
        run.seen.extend(["ms-keys", "ms-keys"]);
        assert!(!steps_done(&state, &run)[2], "two S presses");
        run.seen.extend(["m-key", "m-key"]);
        assert!(steps_done(&state, &run)[2], "two M presses");
    }

    #[test]
    fn a_preset_clears_a_fits_report() {
        // Job 24: the voice guide's "fitted to you" stayed after a preset
        // replaced the fitted settings.
        let mut state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        let talk: Vec<(f32, f32)> = (0..300)
            .map(|i| if i % 3 == 0 { (-56.0, -50.0) } else { (-34.0 + (i % 19) as f32, -24.0 + (i % 19) as f32) })
            .collect();
        state.dyn_fit_report = crate::dynamics_panel::fit_from(&talk).ok();
        state.dyn_fit_said = Some("Fitted to your voice".into());
        let _ = update(&mut state, Message::Dyn(an1, crate::dynamics_panel::DynMsg::Preset(1)));
        assert!(state.dyn_fit_report.is_none() && state.dyn_fit_said.is_none());
    }

    #[test]
    fn the_lock_guide_started_while_locked_ends_on_unlock() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetUiLock(true));
        start(&mut state, "lock");
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, 0.5));
        let _ = update(&mut state, Message::SetUiLock(false));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()));
    }

    #[test]
    fn the_together_guide_sees_a_pick_move_as_one_and_let_go() {
        let mut state = new(true, None, None);
        start(&mut state, "together");
        state.selected.insert(ChannelId::Input(2));
        state.selected.insert(ChannelId::Input(4));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(2), 0, 0.4));
        let _ = update(&mut state, Message::FaderMaster);
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()), "picked, moved together, let go");
    }

    #[test]
    fn the_record_guide_wants_a_spare_mix_with_mic_and_computer_looped() {
        let mut state = new(true, None, None);
        start(&mut state, "record");
        let assign = state.control_room.assign;
        let spare = (0..4).find(|&p| p != assign.main && assign.main_b != Some(p) && !assign.phones_pairs().contains(&p)).unwrap();
        // A playback raised while still on Main doesn't count for the spare.
        let on_main = state.sel_out;
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Playback(0), on_main, 0.9));
        let _ = update(&mut state, Message::SelectOutput(spare));
        // Its own playback is in it by default: the computer's sound counts
        // only once raised by hand; pulling one down says nothing.
        let own = ChannelId::Playback(spare * 2);
        let _ = update(&mut state, Message::VolumeChanged(own, spare, 0.5));
        let run = state.guide.clone().unwrap();
        assert!(!steps_done(&state, &run)[2], "not ticked before anyone touches it");
        let _ = update(&mut state, Message::Mute(ChannelId::Input(0), false));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), spare, 0.5));
        let _ = update(&mut state, Message::Mute(ChannelId::Playback(0), false));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Playback(0), spare, 0.5));
        let run = state.guide.clone().unwrap();
        assert_eq!(steps_done(&state, &run)[..3], [true, true, true]);
        let ch = if state.device.outputs_one_per_pair() { spare } else { spare * 2 };
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Output(ch), spare, 0.8));
        let _ = update(&mut state, Message::LoopbackChanged(ch, true));
        tick(&mut state);
        assert!(state.guide.as_ref().is_some_and(|r| r.finished.is_some()), "looped: done");
    }

    #[test]
    fn the_card_waits_behind_a_notice() {
        let mut state = new(true, None, None);
        start(&mut state, "reverb");
        assert!(card(&state).is_some());
        state.notice = Some(("Something went wrong".into(), Instant::now()));
        assert!(card(&state).is_none());
    }

    #[test]
    fn keeping_the_mic_saves_a_named_preset_and_moves_next_for_you_on() {
        // Issue #20: once Quick setup's checks pass, "Next for you" saves
        // the mic in one press instead of asking to set it up again.
        let mut state = new(true, None, None);
        state.mic_presets.clear();
        state.guides_done.clear();
        state.quick_channel = ChannelId::Input(0);
        let mic = GUIDES.iter().position(|g| g.id == "mic");
        assert_eq!(next_for_you(&state), mic, "the mic guide comes first");
        assert_eq!(kept_mic_name(&state, 0), "AN1 mic");
        let _ = update(&mut state, Message::KeepMicPreset);
        assert_eq!(state.mic_presets.len(), 1);
        assert_eq!(state.mic_presets[0].name, "AN1 mic");
        assert!(done(&state, "mic"), "the mic guide is done");
        assert_ne!(next_for_you(&state), mic, "the card moves on");
        assert!(!keep_mic_offered(&state));
        assert!(state.undo_stack.is_empty(), "saving a preset is no undo step");
        // A second save is numbered, never over the first.
        assert_eq!(kept_mic_name(&state, 0), "AN1 mic 2");
    }
}
