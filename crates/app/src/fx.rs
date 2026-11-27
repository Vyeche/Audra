//! Reverb and Echo in Audra (issue #9, phase D): the counter's "Reverb &
//! Echo" page, the FX send on AN1/2, the FX return on every output, Mute
//! FX, and the key that switches the card to Pro Audio.
//!
//! The unit leaves both effects to the computer (manual 21.6): it mixes an
//! FX send, Audra runs Reverb and Echo on it (`fx_loop`), and the wet
//! signal comes back as the FX return, which each output takes at its own
//! level. Three things must all be up before anything is heard: an effect
//! on, a send into it, and a return in the mix you listen to. The page
//! says which is missing and offers the one press that fixes it.
//!
//! Safety, since the return plays in the monitors: the driver starts every
//! return at off; the page's own keys set a return or a send at −12 dB,
//! never more; a reset of a return knob goes to off, never to 0 dB; a
//! snapshot or Undo that raises a return glides it up (`after_scene`);
//! Reset Mix only ever turns the FX down (`all_off`); Mute FX keeps your
//! levels and gives them back, also after a crash (`recover_mute`).

use std::time::Instant;

use audra_fx::{EchoKind, EchoParams, ReverbKind, ReverbParams};
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Element, Length, Task};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::fx_loop::FxLoopState;
use crate::theme;
use crate::widgets::knob::{knob, Knob, Readout};

/// Which effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Reverb,
    Echo,
}

impl Unit {
    fn word(self) -> &'static str {
        match self {
            Unit::Reverb => "Reverb",
            Unit::Echo => "Echo",
        }
    }

    /// The key's name, which also names its motion (`motion::Bloom::Key`).
    pub fn key_name(self) -> &'static str {
        match self {
            Unit::Reverb => "REVERB",
            Unit::Echo => "ECHO",
        }
    }
}

/// A preset of the user's: a name and one effect's settings.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Named<P> {
    pub name: String,
    pub params: P,
}

/// The user's own presets for each effect, kept in the prefs (the manual
/// keeps them in the workspace; Audra keeps them with its own settings,
/// as the EQ and mic presets are).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UserPresets {
    pub reverb: Vec<Named<ReverbParams>>,
    pub echo: Vec<Named<EchoParams>>,
}

/// How many presets of its own each effect keeps (the manual's 16 slots).
pub const USER_PRESETS: usize = 16;

/// What a preset menu loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetPick {
    /// The effect's own defaults.
    Default,
    /// A factory preset (`audra_fx::presets`), by index.
    Factory(usize),
    /// One of the user's, by index.
    User(usize),
    /// The hand-made settings a load replaced ("Your last settings").
    Kept,
}

/// What each effect's preset menu remembers this session: the preset last
/// loaded (so a knob turn reads "changed", not nothing), and the hand-made
/// settings a load replaced (so auditioning presets never loses them).
#[derive(Debug, Clone, Default)]
pub struct PresetMemory {
    pub reverb_loaded: Option<(PresetPick, String)>,
    pub echo_loaded: Option<(PresetPick, String)>,
    pub reverb_kept: Option<ReverbParams>,
    pub echo_kept: Option<EchoParams>,
}

/// The name the menu gives the kept settings.
const KEPT: &str = "Your last settings";

/// What AN1/2's FX send follows (BF §21.7): by default the channel's
/// highest submix fader, as a send after the fader would; or the Main
/// mix's; or nothing, a send that stays where its knob is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SendFollow {
    #[default]
    HighestSubmix,
    Main,
    Fixed,
}

impl SendFollow {
    pub const ALL: [SendFollow; 3] = [SendFollow::HighestSubmix, SendFollow::Main, SendFollow::Fixed];
}

impl std::fmt::Display for SendFollow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SendFollow::HighestSubmix => "its highest submix",
            SendFollow::Main => "the Main mix",
            SendFollow::Fixed => "nothing (a fixed send)",
        })
    }
}

/// The FX send's floor: at or under it nothing is sent (the knob's −∞).
pub const OFF_DB: f32 = -65.0;
/// What "Make it heard" sets a send and a return to: the owner's levels
/// from the Windows test (2026-11-10: "TotalMix-like"), send −10.5 dB and
/// return −1.5 dB. A return raised that far glides there (`RAMP_OVER_DB`).
pub const START_SEND_DB: f32 = -10.5;
pub const START_RETURN_DB: f32 = -1.5;
/// The return knob's top (the crosspoints' +6 dB).
const RETURN_TOP_DB: f32 = 6.0;
/// A return raised by more than this at once glides, as an output does.
pub const RAMP_OVER_DB: f32 = crate::app::RAMP_OVER_DB as f32;

/// The crosspoints' scale (1 = 0 dB, 0 = off) in dB, −65 for off.
pub fn vol_db(volume: f32) -> f32 {
    if volume <= 0.0 {
        OFF_DB
    } else {
        (20.0 * volume.log10()).max(OFF_DB)
    }
}

/// dB on the crosspoints' scale; −65 or under is off.
pub fn db_vol(db: f32) -> f32 {
    if db <= OFF_DB {
        0.0
    } else {
        10f32.powf(db.min(RETURN_TOP_DB) / 20.0)
    }
}

pub(crate) fn db_words(db: f32) -> String {
    if db <= OFF_DB {
        "\u{2212}\u{221e}".to_string()
    } else {
        format!("{db:.1}").replace('-', "\u{2212}")
    }
}

// ── Messages ──────────────────────────────────────────────────────

pub fn update(state: &mut TuxMix, message: Message) -> Task<Message> {
    // The user's hand on the effects (`watch_heard`).
    if matches!(
        message,
        Message::FxOn(..) | Message::FxSend(_) | Message::SourceFxSend(..) | Message::FxReturn(..) | Message::MuteFxToggle | Message::FxProAudio | Message::FxMakeHeard(_) | Message::FxTurnBackDown
    ) {
        state.fx_touched_at = Some(std::time::Instant::now());
    }
    match message {
        Message::FxOn(unit, on) => {
            match unit {
                Unit::Reverb => state.fx.reverb_on = on,
                Unit::Echo => state.fx.echo_on = on,
            }
            state.prefs_dirty = true;
            crate::app::sync_fx_loop(state);
        }
        Message::FxReverb(p) => {
            state.fx.reverb = p.clamped();
            state.prefs_dirty = true;
            crate::app::sync_fx_loop(state);
        }
        Message::FxEcho(p) => {
            state.fx.echo = p.clamped();
            state.prefs_dirty = true;
            crate::app::sync_fx_loop(state);
        }
        Message::FxSend(db) => {
            state.prefs_dirty = true;
            sync_send_follow(state);
            if let Err(e) = state.device.set_fx_send(db.clamp(OFF_DB, 0.0)) {
                crate::app::fail(state, "set_fx_send", &e);
            }
        }
        Message::SourceFxSend(cid, db) => {
            state.prefs_dirty = true;
            if let Err(e) = state.device.set_source_fx_send(cid, db.clamp(OFF_DB, 0.0)) {
                crate::app::fail(state, "set_source_fx_send", &e);
            }
        }
        Message::SetFxSendFollow(follow) => {
            state.fx_send_follow = follow;
            state.prefs_dirty = true;
            sync_send_follow(state);
        }
        Message::FxReturn(pair, volume) => set_return(state, pair, volume),
        // The send and this mix's return, whichever are off, at the
        // Windows test's levels: one press and one undo step.
        // What's heard comes in on the return's ramp from silence, never in
        // one write (the level audit: the return landed at -1.5 dB at once,
        // the doc's promised glide never happened); with only the send off,
        // the return dips and ramps back, so the reverb fades in either way.
        Message::FxMakeHeard(asked) => {
            // Never an open mic's reverb in the speakers (critique #108 A
            // P1-1): the headphones' return instead, where Quick setup
            // listens, and the notice says so.
            let assign = state.control_room.assign;
            let moved = open_mic_reverb_risk(state, asked);
            let pair = match (moved, assign.phones_pairs().first()) {
                (true, Some(&ph)) => ph,
                _ => asked,
            };
            // Every speakers' return an open mic's reverb would reach comes
            // down first, whichever mix was asked (critique #110 A P2-1: from
            // the headphones' mix the shared send went up into the speakers'
            // return unwarned).
            let speakers: Vec<usize> = std::iter::once(assign.main).chain(assign.main_b).filter(|&p| open_mic_reverb_risk(state, p) && user_return(state, p) > 0.0).collect();
            let speakers_down = !speakers.is_empty();
            for &sp in &speakers {
                set_return(state, sp, 0.0);
            }
            // No headphones to move it to: out of the speakers, said
            // (critique #110 A P3-38/P3-40: the key left it playing).
            if moved && pair == asked {
                let said = if speakers_down {
                    "Taken out of your speakers: an open mic\u{2019}s reverb there can howl. Assign headphones (Audra settings \u{2192} Monitor) to hear it there."
                } else {
                    "Not in your speakers: an open mic\u{2019}s reverb there can howl. Assign headphones (Audra settings \u{2192} Monitor) to hear it there."
                };
                state.notice = Some((said.to_string(), Instant::now()));
                return Task::none();
            }
            let mut send_raised = false;
            if send_db(state) <= OFF_DB {
                state.prefs_dirty = true;
                sync_send_follow(state);
                if let Err(e) = state.device.set_fx_send(START_SEND_DB) {
                    crate::app::fail(state, "set_fx_send", &e);
                } else {
                    send_raised = true;
                }
            }
            let now = Instant::now();
            let ret = user_return(state, pair);
            // Mute FX on: its kept level is set, nothing sounds (`set_return`;
            // the advisor: written to the unit, the reverb came back behind
            // a lit Mute FX key).
            let muted = state.control_room.fx_muted.as_ref().is_some_and(|kept| kept.iter().any(|(p, _)| *p == pair));
            if muted {
                if ret <= 0.0 {
                    set_return(state, pair, db_vol(START_RETURN_DB));
                }
                remember_made_heard(state, asked, pair, send_raised, ret <= 0.0);
                return Task::none();
            }
            // A return still gliding lands where it was going, not where it
            // had got to (Fable's review of issue #45).
            let to_db = if ret <= 0.0 { Some(START_RETURN_DB) } else if send_raised { Some(vol_db(target_return(state, pair))) } else { None };
            remember_made_heard(state, asked, pair, send_raised, ret <= 0.0);
            if let Some(to_db) = to_db {
                if let Err(e) = state.device.set_fx_return(pair, db_vol(-60.0)) {
                    crate::app::fail(state, "set_fx_return", &e);
                } else {
                    state.fx_return_ramps.insert(pair, (-60.0, to_db, now));
                }
            }
            // Say what the press did: the key goes once the effect is
            // heard, and went with no word (the owner, issue #41: "that
            // button went away which is strange").
            let mix = crate::sidebar::mix_said_s(state, pair);
            let what = match (send_raised, ret <= 0.0) {
                // Whose send (critique #93 A P3-9: two strips' FX keys lit
                // and the notice named neither): the one AN1 and AN2 share.
                (true, true) => format!("the mics\u{2019} FX send (AN1 and AN2 share it) is up and {mix} FX return is on"),
                (true, false) => "the mics\u{2019} FX send (AN1 and AN2 share it) is up".to_string(),
                (false, _) => format!("{mix} FX return is on"),
            };
            let why = match (moved, speakers_down) {
                (_, true) => " (and the speakers\u{2019} FX return off: an open mic\u{2019}s reverb there can howl)",
                (true, false) => " (not the speakers: an open mic\u{2019}s reverb there can howl)",
                _ => "",
            };
            // The sample's key only where the page shows it (critique #110 A:
            // the Kept state has none).
            let sample = if moved { "" } else { " Hear a sample now plays a clap through them;" };
            // Nothing raised, only the speakers taken out: not "Made it
            // heard" (critique #111 Fable A P3-2: it was heard already).
            if speakers_down && !send_raised && to_db.is_none() {
                let mix = crate::sidebar::mix_said(state, pair);
                crate::app::say_undoable(
                    state,
                    format!("Moved to {mix}: the speakers\u{2019} FX return is off (an open mic\u{2019}s reverb there can howl), and {mix} already had it. Undo puts it back as it was."),
                    "Put the speakers\u{2019} return back (Ctrl+Z)".to_string(),
                );
                return Task::none();
            }
            crate::app::say_undoable(state, format!("Made it heard: {what}{why}.{sample} Undo puts it back as it was."), "Put the send and return back (Ctrl+Z)".to_string());
        }
        // Make it heard's key pressed again: what it turned up, and only
        // that, goes back down (the owner, issue #45). Only ever down, at
        // once, as every turn-down is; never the speakers' return an open
        // mic's reverb was taken out of.
        Message::FxTurnBackDown => {
            let Some(m) = made_heard_up(state) else { return Task::none() };
            if !turn_down_ready(state) {
                return Task::none();
            }
            state.made_heard = None;
            let mix = crate::sidebar::mix_said_s(state, m.pair);
            let mut what = Vec::new();
            if m.ret && target_return(state, m.pair) > 0.0 {
                set_return(state, m.pair, 0.0);
                state.prefs_dirty = true;
                what.push(format!("{mix} FX return is off"));
            }
            if m.send && send_db(state) > OFF_DB {
                state.prefs_dirty = true;
                sync_send_follow(state);
                if let Err(e) = state.device.set_fx_send(OFF_DB) {
                    crate::app::fail(state, "set_fx_send", &e);
                } else {
                    what.insert(0, "the mics\u{2019} FX send is down".to_string());
                }
            }
            if !what.is_empty() {
                crate::app::say_undoable(
                    state,
                    format!("Turned back down: {}, as before Make it heard. Undo brings it back.", what.join(" and ")),
                    "Turn it up again (Ctrl+Z)".to_string(),
                );
            }
        }
        // The floating page's × (any page's, `sidebar::page_card_body`).
        Message::FxCardClose => crate::app::close_page_card(state),
        // Heard on the computer's own output, quietly (`sounds::fx_preview`),
        // whether or not the UI sounds are on: it was asked for. Not while
        // locked (on air).
        Message::FxPreview => {
            if !state.ui_locked {
                crate::sounds::play_fx_preview(crate::sounds::fx_preview(&state.fx), state.ui_sound_db);
                state.notice = Some(("Playing a clap as it is, then through the effects as they are set.".to_string(), std::time::Instant::now()));
            }
        }
        Message::FxAudition => {
            // Locked, nothing sounds on air (`sounds::set_quiet`).
            if let (Some(l), false) = (&state.fx_loop, state.ui_locked) {
                l.audition();
            }
        }
        Message::MuteFxToggle => {
            let on = state.control_room.fx_muted.is_none();
            set_mute(state, on);
        }
        Message::FxProAudio => {
            // `pactl` can block (PipeWire restarting): on its own thread,
            // not on the executor's.
            let (tx, rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(switch_to_pro_audio());
            });
            return Task::perform(
                async move { rx.await.unwrap_or_else(|_| Err("the switch didn't finish".to_string())) },
                Message::FxProAudioDone,
            );
        }
        Message::FxPresetLoad(unit, pick) => {
            load_preset(state, unit, pick);
            state.fx_naming = None;
        }
        Message::FxPresetSaveStart(unit) => {
            state.fx_naming = Some((unit, suggested_name(state, unit)));
            return Task::batch([
                iced::widget::operation::focus(NAME_INPUT_ID),
                iced::widget::operation::select_all(NAME_INPUT_ID),
            ]);
        }
        Message::FxPresetName(typed) => {
            if let Some((_, name)) = state.fx_naming.as_mut() {
                *name = crate::app::capped_name(typed);
            }
        }
        Message::FxPresetSave => save_preset(state),
        Message::FxPresetCancel => state.fx_naming = None,
        Message::FxPresetRemove(unit, i) => remove_preset(state, unit, i),
        Message::FxProAudioDone(result) => {
            let line = match result {
                Ok(()) => "Pro Audio is on: every channel of the unit is open. Reverb and Echo start in a moment.".to_string(),
                Err(why) => format!(
                    "Couldn't switch to Pro Audio ({why}). Choose it for the Babyface in Settings, Sound, or in pavucontrol, Configuration."
                ),
            };
            state.notice = Some((line, Instant::now()));
        }
        _ => {}
    }
    Task::none()
}

// ── Presets ───────────────────────────────────────────────────────

/// The preset name field's widget id, to put the keyboard in it.
pub const NAME_INPUT_ID: &str = "fx-preset-name";

/// The factory presets' names for `unit`.
fn factory_names(unit: Unit) -> Vec<&'static str> {
    match unit {
        Unit::Reverb => audra_fx::presets::reverbs().into_iter().map(|(n, _)| n).collect(),
        Unit::Echo => audra_fx::presets::echoes().into_iter().map(|(n, _)| n).collect(),
    }
}

fn user_names(state: &TuxMix, unit: Unit) -> Vec<String> {
    match unit {
        Unit::Reverb => state.fx_presets.reverb.iter().map(|p| p.name.clone()).collect(),
        Unit::Echo => state.fx_presets.echo.iter().map(|p| p.name.clone()).collect(),
    }
}

/// The preset `unit` is set to now, if its settings are exactly one's:
/// the user's first, then the factory's, then the defaults.
pub fn current_preset(state: &TuxMix, unit: Unit) -> Option<(PresetPick, String)> {
    match unit {
        Unit::Reverb => {
            let p = state.fx.reverb;
            if let Some(i) = state.fx_presets.reverb.iter().position(|n| n.params == p) {
                return Some((PresetPick::User(i), state.fx_presets.reverb[i].name.clone()));
            }
            if let Some((i, (name, _))) = audra_fx::presets::reverbs().into_iter().enumerate().find(|(_, (_, f))| *f == p) {
                return Some((PresetPick::Factory(i), name.to_string()));
            }
            if state.fx_memory.reverb_kept == Some(p) {
                return Some((PresetPick::Kept, KEPT.to_string()));
            }
            (p == ReverbParams::default()).then(|| (PresetPick::Default, "Default".to_string()))
        }
        Unit::Echo => {
            let p = state.fx.echo;
            if let Some(i) = state.fx_presets.echo.iter().position(|n| n.params == p) {
                return Some((PresetPick::User(i), state.fx_presets.echo[i].name.clone()));
            }
            if let Some((i, (name, _))) = audra_fx::presets::echoes().into_iter().enumerate().find(|(_, (_, f))| *f == p) {
                return Some((PresetPick::Factory(i), name.to_string()));
            }
            if state.fx_memory.echo_kept == Some(p) {
                return Some((PresetPick::Kept, KEPT.to_string()));
            }
            (p == EchoParams::default()).then(|| (PresetPick::Default, "Default".to_string()))
        }
    }
}

/// Loads a preset into `unit`; whether the effect is on stays as it is.
/// Settings that were no preset's (made or changed by hand) are kept as
/// "Your last settings" first, so a load never loses them; loading those
/// back swaps them with what is there.
pub fn load_preset(state: &mut TuxMix, unit: Unit, pick: PresetPick) {
    let hand_made = current_preset(state, unit).is_none();
    let name = match pick {
        PresetPick::Default => Some("Default".to_string()),
        PresetPick::Factory(i) => factory_names(unit).get(i).map(|n| n.to_string()),
        PresetPick::User(i) => user_names(state, unit).get(i).cloned(),
        PresetPick::Kept => Some(KEPT.to_string()),
    };
    let Some(name) = name else { return };
    let m = &mut state.fx_memory;
    match unit {
        Unit::Reverb => {
            let p = match pick {
                PresetPick::Default => Some(ReverbParams::default()),
                PresetPick::Factory(i) => audra_fx::presets::reverbs().get(i).map(|(_, p)| *p),
                PresetPick::User(i) => state.fx_presets.reverb.get(i).map(|n| n.params),
                PresetPick::Kept => m.reverb_kept,
            };
            let Some(p) = p else { return };
            if hand_made {
                m.reverb_kept = Some(state.fx.reverb);
            }
            state.fx.reverb = p.clamped();
            m.reverb_loaded = Some((pick, name));
        }
        Unit::Echo => {
            let p = match pick {
                PresetPick::Default => Some(EchoParams::default()),
                PresetPick::Factory(i) => audra_fx::presets::echoes().get(i).map(|(_, p)| *p),
                PresetPick::User(i) => state.fx_presets.echo.get(i).map(|n| n.params),
                PresetPick::Kept => m.echo_kept,
            };
            let Some(p) = p else { return };
            if hand_made {
                m.echo_kept = Some(state.fx.echo);
            }
            state.fx.echo = p.clamped();
            m.echo_loaded = Some((pick, name));
        }
    }
    state.prefs_dirty = true;
    crate::app::sync_fx_loop(state);
}

/// What the preset capsule says, and which entry it marks: the preset the
/// settings are exactly, or the one last loaded with ", changed" once a
/// knob has moved; `None` for settings that came from no preset.
fn preset_label(state: &TuxMix, unit: Unit) -> Option<(PresetPick, String)> {
    if let Some((pick, name)) = current_preset(state, unit) {
        return Some((pick, format!("Preset: {name}")));
    }
    let loaded = match unit {
        Unit::Reverb => state.fx_memory.reverb_loaded.clone(),
        Unit::Echo => state.fx_memory.echo_loaded.clone(),
    };
    loaded.map(|(pick, name)| (pick, format!("Preset: {name}, changed")))
}

fn has_kept(state: &TuxMix, unit: Unit) -> bool {
    match unit {
        Unit::Reverb => state.fx_memory.reverb_kept.is_some(),
        Unit::Echo => state.fx_memory.echo_kept.is_some(),
    }
}

/// What "Save as new" offers to call it: the user's preset it is set to,
/// else the factory one's name made the user's, else its type.
fn suggested_name(state: &TuxMix, unit: Unit) -> String {
    let from = current_preset(state, unit).or_else(|| match unit {
        Unit::Reverb => state.fx_memory.reverb_loaded.clone(),
        Unit::Echo => state.fx_memory.echo_loaded.clone(),
    });
    match from {
        Some((PresetPick::User(_), name)) => name,
        Some((PresetPick::Factory(_), name)) => format!("My {}", name.to_lowercase()),
        _ => match unit {
            Unit::Reverb => state.fx.reverb.kind.name().to_string(),
            Unit::Echo => format!("{} echo", state.fx.echo.kind.name()),
        },
    }
}

/// Keeps the effect as it is under the typed name; a name already used
/// (any case) is replaced.
fn save_preset(state: &mut TuxMix) {
    let Some((unit, typed)) = state.fx_naming.take() else { return };
    let mut name = typed.trim().to_string();
    let names = user_names(state, unit);
    let count = names.len();
    if name.is_empty() {
        // The first "Reverb N" no preset has, so an untitled save never
        // replaces one.
        name = (count + 1..)
            .map(|n| format!("{} {n}", unit.word()))
            .find(|candidate| !names.iter().any(|t| t.eq_ignore_ascii_case(candidate)))
            .unwrap_or_default();
    }
    let taken = user_names(state, unit).iter().position(|n| n.eq_ignore_ascii_case(&name));
    if taken.is_none() && count >= USER_PRESETS {
        state.notice = Some((format!("{} keeps {USER_PRESETS} presets of yours: remove one first.", unit.word()), Instant::now()));
        state.fx_naming = Some((unit, typed));
        if state.ui_sounds {
            crate::sounds::play(crate::sounds::Sound::Nope, state.ui_sound_db);
        }
        return;
    }
    match unit {
        Unit::Reverb => {
            let named = Named { name: name.clone(), params: state.fx.reverb };
            match taken {
                Some(i) => state.fx_presets.reverb[i] = named,
                None => state.fx_presets.reverb.push(named),
            }
        }
        Unit::Echo => {
            let named = Named { name: name.clone(), params: state.fx.echo };
            match taken {
                Some(i) => state.fx_presets.echo[i] = named,
                None => state.fx_presets.echo.push(named),
            }
        }
    }
    let verb = if taken.is_some() { "Replaced" } else { "Saved" };
    state.notice = Some((format!("{verb} \u{201c}{name}\u{201d} in {}'s presets.", unit.word()), Instant::now()));
    // "Kept", as an EQ or mic preset's save (critique #50: silent).
    if state.ui_sounds {
        crate::sounds::play(crate::sounds::Sound::Done, state.ui_sound_db);
    }
    state.prefs_dirty = true;
}

/// Removes one of the user's presets on the second choice within a few
/// seconds; the first asks (as a mic preset's removal does).
fn remove_preset(state: &mut TuxMix, unit: Unit, i: usize) {
    let Some(name) = user_names(state, unit).get(i).cloned() else { return };
    let armed = state
        .fx_remove_armed
        .is_some_and(|(u, at, when)| u == unit && at == i && when.elapsed() < crate::resetmix::ARM_FOR);
    if !armed {
        state.fx_remove_armed = Some((unit, i, Instant::now()));
        state.notice = Some((format!("Remove \u{201c}{name}\u{201d}? Choose it again to remove it."), Instant::now()));
        return;
    }
    state.fx_remove_armed = None;
    // The user's presets after it move up one: what was loaded is forgotten.
    match unit {
        Unit::Reverb => {
            state.fx_presets.reverb.remove(i);
            if matches!(state.fx_memory.reverb_loaded, Some((PresetPick::User(_), _))) {
                state.fx_memory.reverb_loaded = None;
            }
        }
        Unit::Echo => {
            state.fx_presets.echo.remove(i);
            if matches!(state.fx_memory.echo_loaded, Some((PresetPick::User(_), _))) {
                state.fx_memory.echo_loaded = None;
            }
        }
    }
    state.notice = Some((format!("Removed \u{201c}{name}\u{201d}."), Instant::now()));
    state.prefs_dirty = true;
}

/// One entry of a preset menu: what it does and its words. Two entries are
/// the same entry when they do the same thing, so the capsule's "Preset:
/// Close vocal" marks "Close vocal" in the open menu.
#[derive(Debug, Clone)]
struct MenuEntry {
    action: MenuAction,
    label: String,
}

impl PartialEq for MenuEntry {
    fn eq(&self, other: &Self) -> bool {
        self.action == other.action
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuAction {
    Load(PresetPick),
    SaveNew,
    Remove(usize),
}

impl std::fmt::Display for MenuEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// An effect's **Preset ▾**: the user's presets, the factory ones and the
/// defaults to load; save as new; remove the user's preset it is set to.
/// It names the preset the effect is set to, while it is.
fn preset_row(state: &TuxMix, unit: Unit, scale: f32) -> Element<'_, Message> {
    let current = current_preset(state, unit);
    let mut entries: Vec<MenuEntry> = Vec::new();
    if has_kept(state, unit) {
        entries.push(MenuEntry { action: MenuAction::Load(PresetPick::Kept), label: KEPT.into() });
    }
    entries.extend(user_names(state, unit)
        .into_iter()
        .enumerate()
        .map(|(i, name)| MenuEntry { action: MenuAction::Load(PresetPick::User(i)), label: name }));
    entries.extend(
        factory_names(unit)
            .into_iter()
            .enumerate()
            .map(|(i, name)| MenuEntry { action: MenuAction::Load(PresetPick::Factory(i)), label: name.to_string() }),
    );
    entries.push(MenuEntry { action: MenuAction::Load(PresetPick::Default), label: "Default".into() });
    entries.push(MenuEntry { action: MenuAction::SaveNew, label: "Save as new\u{2026}".into() });
    if let Some((PresetPick::User(i), name)) = &current {
        entries.push(MenuEntry { action: MenuAction::Remove(*i), label: format!("Remove \u{201c}{name}\u{201d}") });
    }
    let h = 24.0 * scale;
    // The preset in force reads as a value, in the ink (a placeholder's
    // grey read as an empty control), and is marked in the open menu.
    let shown = preset_label(state, unit).map(|(pick, label)| MenuEntry { action: MenuAction::Load(pick), label });
    let menu = crate::widgets::strip::hint(
        pick_list(entries, shown, move |entry: MenuEntry| match entry.action {
            MenuAction::Load(pick) => Message::FxPresetLoad(unit, pick),
            MenuAction::SaveNew => Message::FxPresetSaveStart(unit),
            MenuAction::Remove(i) => Message::FxPresetRemove(unit, i),
        })
        .placeholder("Preset")
        .style(theme::capsule_pick_list(h / 2.0))
        .menu_style(theme::menu)
        .padding([theme::SPACE_TIGHT * scale, theme::SPACE_LG * scale])
        .text_size(theme::TEXT_SM * scale)
        .width(Length::Fill),
        "Load a preset (yours, a factory one or the defaults), or save these settings as your own",
        scale,
    );
    let menu = crate::keyfocus::ring(menu, kb(state, CounterItem::FxPreset(unit)), h / 2.0);
    let Some((_, typed)) = state.fx_naming.as_ref().filter(|(u, _)| *u == unit) else {
        return menu;
    };
    let replaces = user_names(state, unit).iter().any(|n| n.eq_ignore_ascii_case(typed.trim()));
    // The row keeps the capsule's 24px: the field, the key and the ×.
    let field = iced::widget::text_input("Name to save as", typed)
        .id(NAME_INPUT_ID)
        .on_input(Message::FxPresetName)
        .on_submit(Message::FxPresetSave)
        .style(theme::text_input)
        .size(theme::TEXT_SM * scale)
        .line_height(iced::widget::text::LineHeight::Absolute(iced::Pixels(h - 2.0 * theme::SPACE_SM * scale)))
        .padding([theme::SPACE_SM * scale, theme::SPACE_SM * scale])
        .width(Length::Fill);
    let save = button(text(if replaces { "Replace" } else { "Save" }).size(theme::TEXT_SM * scale).center())
        .padding([0.0, theme::SPACE_MD * scale])
        .height(Length::Fixed(h))
        .style(theme::page_key(h / 2.0))
        .on_press(Message::FxPresetSave);
    let cancel = crate::widgets::strip::hint(
        button(container(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale)).center(Length::Fill))
            .padding(0)
            .width(Length::Fixed(h))
            .height(Length::Fixed(h))
            .style(theme::icon_button(false))
            .on_press(Message::FxPresetCancel),
        "Cancel (Esc)",
        scale,
    );
    column![
        menu,
        row![field, save, cancel].spacing(theme::SPACE_TIGHT * scale).align_y(iced::Alignment::Center),
    ]
    .spacing(theme::SPACE_SM * scale)
    .into()
}

// ── Sends and returns ─────────────────────────────────────────────

/// AN1/2's FX send knob in dB (−65 = nothing sent).
pub fn send_db(state: &TuxMix) -> f32 {
    state.device.settings().fx_send_db.unwrap_or(OFF_DB)
}

/// The strip's FX mini fader (`widgets::mini_fader`) runs the faders' taper,
/// so its travel reads as theirs does: the send's from off to 0 dB (its
/// top), the return's from off to +6 dB, as their knobs.
fn send_span() -> f32 {
    crate::widgets::fader::vol_to_t(1.0)
}

/// A send in dB as the mini fader's travel, 0 to 1.
pub fn send_t(db: f32) -> f32 {
    (crate::widgets::fader::vol_to_t(db_vol(db)) / send_span()).clamp(0.0, 1.0)
}

/// The mini fader's travel as a send in dB: off at the bottom.
pub fn t_send_db(t: f32) -> f32 {
    if t <= 0.0 {
        OFF_DB
    } else {
        vol_db(crate::widgets::fader::t_to_vol(t * send_span())).min(0.0)
    }
}

/// A return (the crosspoints' scale) as the mini fader's travel.
pub fn return_t(volume: f32) -> f32 {
    crate::widgets::fader::vol_to_t(volume)
}

/// A send's mini fader one step up from `t`: 6 dB louder (from off, from
/// -60 dB), the most one click may raise it (critique #49).
pub fn send_t_up(t: f32) -> f32 {
    send_t(t_send_db(t).max(-60.0) + crate::app::RAMP_OVER_DB as f32)
}

/// A return's mini fader one step up from `t`: 6 dB louder (from silence,
/// from -60 dB).
pub fn return_t_up(t: f32) -> f32 {
    let v = t_return_vol(t);
    let db = if v <= 0.0 { -60.0 } else { (20.0 * v.log10()).max(-60.0) };
    return_t(10f32.powf((db + crate::app::RAMP_OVER_DB as f32) / 20.0))
}

/// The mini fader's travel as a return: off at the bottom.
pub fn t_return_vol(t: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else {
        crate::widgets::fader::t_to_vol(t)
    }
}

/// What the send follows now, in dB on top of the knob: AN1/2's highest
/// submix level (or its Main mix level), a muted channel left out;
/// `NEG_INFINITY` where it is down in every one; 0 for a fixed send.
pub fn follow_trim(state: &TuxMix) -> f32 {
    let pairs: Vec<usize> = match state.fx_send_follow {
        SendFollow::Fixed => return 0.0,
        SendFollow::HighestSubmix => (0..state.device.output_pair_count()).collect(),
        SendFollow::Main => vec![crate::control_room::monitor_pair(state)],
    };
    let mut loudest = 0.0f32;
    for (i, input) in state.device.inputs().iter().enumerate().take(2) {
        if input.mute {
            continue;
        }
        for &pair in &pairs {
            loudest = loudest.max(state.device.volume(ChannelId::Input(i), pair).unwrap_or(0.0));
        }
    }
    if loudest <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * loudest.log10()
    }
}

/// The send the unit gets: the knob, following what it follows.
pub fn send_heard_db(state: &TuxMix) -> f32 {
    tuxmix_core::device::fx_send_on_unit(send_db(state), follow_trim(state))
}

/// Keeps the unit's send following its faders (on the tick, and when the
/// knob or the choice changes); writes only when the level moved.
pub fn sync_send_follow(state: &mut TuxMix) {
    if !in_panels(state) {
        return;
    }
    let trim = follow_trim(state);
    let moved = match state.fx_send_trim {
        None => true,
        Some(was) if was.is_finite() && trim.is_finite() => (was - trim).abs() > 0.05,
        Some(was) => was != trim,
    };
    if moved {
        state.fx_send_trim = Some(trim);
        if let Err(e) = state.device.set_fx_send_trim(trim) {
            log::warn!("FX send: couldn't follow the mix: {e}");
        }
    }
}

/// Your FX return into `pair`: the level Mute FX keeps while it holds the
/// pair silent, else what the unit has.
pub fn user_return(state: &TuxMix, pair: usize) -> f32 {
    state
        .control_room
        .fx_muted
        .as_ref()
        .and_then(|kept| kept.iter().find(|(p, _)| *p == pair).map(|&(_, v)| v))
        .unwrap_or_else(|| state.device.settings().fx_return_of(pair))
}

/// What Make it heard turned up (the owner, issue #45: "If I make a bunch
/// of changes I have no way of going back"): the mix it was pressed for,
/// the mix whose return it raised (the headphones' when an open mic kept it
/// out of the speakers), and whether it raised the send and that return.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MadeHeard {
    pub asked: usize,
    pub pair: usize,
    pub send: bool,
    pub ret: bool,
    /// When the press came: the key it becomes, under the same pointer,
    /// waits a moment, so a double-click doesn't undo itself (Opus's review).
    pub at: Instant,
}

/// How long "Turn it back down" waits after Make it heard before it acts.
const TURN_DOWN_ARM: std::time::Duration = std::time::Duration::from_millis(500);

/// A press adds to what an earlier one turned up into the same mix and
/// never forgets it: a second press that only moved the reverb out of the
/// speakers left nothing to turn back down (found by the test).
fn remember_made_heard(state: &mut TuxMix, asked: usize, pair: usize, send: bool, ret: bool) {
    if !(send || ret) {
        return;
    }
    let prev = state.made_heard.filter(|m| m.pair == pair);
    state.made_heard = Some(MadeHeard {
        asked: prev.map_or(asked, |m| m.asked),
        pair,
        send: send || prev.is_some_and(|m| m.send),
        ret: ret || prev.is_some_and(|m| m.ret),
        at: Instant::now(),
    });
}

/// What Make it heard turned up that is still up: its key reads "Turn it
/// back down" while so. Gone once those levels are down again however
/// they got there (Undo, a fader, the key), and with another mix.
pub fn made_heard_up(state: &TuxMix) -> Option<MadeHeard> {
    let m = state.made_heard?;
    let send_up = m.send && send_db(state) > OFF_DB;
    let ret_up = m.ret && target_return(state, m.pair) > 0.0;
    (send_up || ret_up).then_some(m)
}

/// Whether Make it heard's key, in its place, reads "Turn it back down"
/// for the mix being set: pressed there (or moved there from it), still
/// up, no other key of the page's own standing there (Turn on Reverb,
/// Release Mute FX, Make it heard, Switch to Pro Audio, the headphones'
/// keys), and no open mic's reverb in the speakers, now or behind Mute FX
/// (that press first, always).
pub fn turn_down_offer(state: &TuxMix) -> bool {
    let sel = state.sel_out;
    // Only where the page has no key of its own but the sample's (Fable's
    // review: it displaced Switch to Pro Audio, the headphones' keys and
    // Release Mute FX, and the placard then disagreed with the page).
    let page_free = || status(state).is_none_or(|(_, key, _, _)| key.is_none_or(|(label, _)| matches!(label, "Hear a sample" | "Hear a clap")));
    made_heard_up(state).is_some_and(|m| (sel == m.asked || sel == m.pair) && page_free())
        && mic_reverb_hazard(state).is_none()
        && mute_holds_mic_reverb(state).is_none()
}

/// Whether a "Turn it back down" press now would lower something: there
/// is something to, and the arm has passed. Else the press is no change
/// and opens no undo step (both reviewers, round 2: a double-click pushed
/// an empty step and took Make it heard's Undo key).
pub fn turn_down_ready(state: &TuxMix) -> bool {
    made_heard_up(state).is_some_and(|m| m.at.elapsed() >= TURN_DOWN_ARM)
}

pub(crate) const TURN_DOWN_TIP: &str = "Turns back down what Make it heard turned up (the FX send, the FX return), and nothing else: press Make it heard again to bring it back";

/// Your FX return into `pair`. While Mute FX holds the pair, it is kept
/// for later and the pair stays silent.
pub fn set_return(state: &mut TuxMix, pair: usize, volume: f32) {
    let volume = volume.clamp(0.0, 2.0);
    state.fx_return_ramps.remove(&pair);
    if let Some(kept) = state.control_room.fx_muted.as_mut() {
        if let Some(slot) = kept.iter_mut().find(|(p, _)| *p == pair) {
            slot.1 = volume;
            state.prefs_dirty = true;
            return;
        }
    }
    if let Err(e) = state.device.set_fx_return(pair, volume) {
        crate::app::fail(state, "set_fx_return", &e);
    }
}

/// The send and every return off (Reset Mix): a reset only turns down.
pub fn all_off(state: &mut TuxMix) {
    if !state.fx_available {
        return;
    }
    sends_and_returns_off(state);
}

/// Every send (AN1/2's and every source pair's) and every return off on
/// the unit, whether or not the effects run yet: how Audra starts (the
/// owner, 2026-11-10: the talking setup, nothing to or from Reverb and
/// Echo; the "Talking + reverb" snapshot brings them in).
pub fn sends_and_returns_off(state: &mut TuxMix) {
    let _ = state.device.set_fx_send(OFF_DB);
    for cid in SEND_PAIR_CHANNELS {
        if state.device.has_source_fx_send(cid) && state.device.settings().source_fx_send_of(cid) > OFF_DB {
            let _ = state.device.set_source_fx_send(cid, OFF_DB);
        }
    }
    for pair in 0..state.device.output_pair_count() {
        state.fx_return_ramps.remove(&pair);
        let _ = state.device.set_fx_return(pair, 0.0);
    }
    if let Some(kept) = state.control_room.fx_muted.as_mut() {
        kept.iter_mut().for_each(|slot| slot.1 = 0.0);
    }
}

// ── Mute FX ───────────────────────────────────────────────────────

/// The pairs Mute FX silences: Main, and Main B where there is one.
pub(crate) fn mute_pairs(state: &TuxMix) -> Vec<usize> {
    let assign = state.control_room.assign;
    let mut pairs = vec![assign.main];
    pairs.extend(assign.main_b);
    pairs
}

/// Mute FX on: the return off on Main (and Main B), your levels kept. Off:
/// they come back.
pub fn set_mute(state: &mut TuxMix, on: bool) {
    if on == state.control_room.fx_muted.is_some() {
        return;
    }
    if on {
        // A return still gliding is kept at where it was going, not where
        // the glide had got to.
        let kept: Vec<(usize, f32)> = mute_pairs(state)
            .into_iter()
            .map(|pair| (pair, target_return(state, pair)))
            .collect();
        for &(pair, _) in &kept {
            state.fx_return_ramps.remove(&pair);
            let _ = state.device.set_fx_return(pair, 0.0);
        }
        state.control_room.fx_muted = Some(kept);
    } else if let Some(kept) = state.control_room.fx_muted.take() {
        // A level the unit wouldn't take (not ready yet, no FX) stays kept,
        // and Mute FX with it, rather than being lost.
        // Back from silence on the ramp a scene's returns use, not in one
        // write (OpenCode's job 42): the first write, at -60 dB, also tells
        // whether the unit takes it.
        let mut failed = Vec::new();
        let now = Instant::now();
        for (pair, volume) in kept {
            let to_db = vol_db(volume);
            let first = if to_db > -60.0 + crate::app::RAMP_OVER_DB as f32 { db_vol(-60.0) } else { volume };
            if let Err(e) = state.device.set_fx_return(pair, first) {
                log::warn!("Mute FX: couldn't give pair {pair} its return back: {e}");
                failed.push((pair, volume));
            } else if first != volume {
                state.fx_return_ramps.insert(pair, (-60.0, to_db, now));
            }
        }
        if !failed.is_empty() {
            state.control_room.fx_muted = Some(failed);
            state.notice = Some((
                "Mute FX is still on: the unit didn't take the FX return back. Press Mute FX again once it's ready.".into(),
                Instant::now(),
            ));
        }
    }
    state.prefs_dirty = true;
}

/// Where your return into `pair` is going: a glide's end while one runs,
/// else your level.
fn target_return(state: &TuxMix, pair: usize) -> f32 {
    state
        .fx_return_ramps
        .get(&pair)
        .map_or_else(|| user_return(state, pair), |&(_, to_db, _)| db_vol(to_db))
}

/// Every pair's return as a scene should keep it: your levels, a glide's
/// end rather than its middle, and Mute FX's kept level rather than its 0.
pub fn returns_to_keep(state: &TuxMix) -> Vec<(usize, f32)> {
    (0..state.device.output_pair_count()).map(|pair| (pair, target_return(state, pair))).collect()
}

/// At start: a Mute FX the last run left on (a quit or a crash while it
/// was) gives your levels back, as Dim does.
pub fn recover_mute(state: &mut TuxMix, kept: Option<Vec<(usize, f32)>>) {
    if let Some(kept) = kept {
        state.control_room.fx_muted = Some(kept);
        set_mute(state, false);
    }
}

// ── Snapshots and Undo ────────────────────────────────────────────

/// What a scene's returns do once it is in: Mute FX's kept levels to
/// take, and the returns to glide up (pair, from and to in dB).
#[derive(Debug, Default)]
pub struct ScenePlan {
    kept: Vec<(usize, f32)>,
    glides: Vec<(usize, f32, f32)>,
}

/// The scene as the unit should receive it, decided before it is sent, so
/// the hardware never holds a level for a moment it shouldn't: a pair Mute
/// FX holds stays at 0 (the scene's level becomes the kept one), and a
/// return the scene raises by more than 6 dB stays where it is and glides
/// up afterwards (`finish_scene`). A scene from before the FX return (none
/// saved) leaves every return as it is, Mute FX's kept levels included.
pub fn prepare_scene(state: &TuxMix, scene: &tuxmix_core::Scene) -> (tuxmix_core::Scene, ScenePlan) {
    let mut scene = scene.clone();
    let mut plan = ScenePlan::default();
    // A send the scene raises far (Undo after Turn it back down, a
    // snapshot): what it feeds comes in on the returns' glide from silence,
    // as Make it heard brings it in, not in the send's one write (Fable's
    // review of issue #45).
    let cur = state.device.settings();
    let send_rises = scene.settings.fx_send_db.is_some_and(|to| to > cur.fx_send_db.unwrap_or(OFF_DB).max(-60.0) + RAMP_OVER_DB)
        || scene.settings.source_fx_send.iter().any(|(k, &to)| to > cur.source_fx_send.get(k).copied().unwrap_or(OFF_DB).max(-60.0) + RAMP_OVER_DB);
    if scene.settings.fx_return.is_empty() {
        if !send_rises {
            return (scene, plan);
        }
        // A scene from before the FX return leaves every return as it is;
        // raising a send, those levels come in on the glide too (Opus's and
        // Fable's round 2).
        for pair in 0..state.device.output_pair_count() {
            scene.settings.keep_fx_return(pair, target_return(state, pair));
        }
    }
    let muted: Vec<usize> = state.control_room.fx_muted.iter().flatten().map(|&(p, _)| p).collect();
    for pair in 0..state.device.output_pair_count() {
        let to = scene.settings.fx_return_of(pair);
        let now = state.device.settings().fx_return_of(pair);
        if muted.contains(&pair) {
            plan.kept.push((pair, to));
            scene.settings.keep_fx_return(pair, 0.0);
        } else if send_rises && vol_db(to) > -60.0 + RAMP_OVER_DB {
            plan.glides.push((pair, -60.0, vol_db(to)));
            scene.settings.keep_fx_return(pair, db_vol(-60.0).min(now));
        } else if vol_db(to) > vol_db(now) + RAMP_OVER_DB {
            plan.glides.push((pair, vol_db(now).max(-60.0), vol_db(to)));
            scene.settings.keep_fx_return(pair, now);
        }
    }
    (scene, plan)
}

/// After the scene is in: Mute FX keeps the scene's levels for its pairs,
/// and the raised returns start gliding.
pub fn finish_scene(state: &mut TuxMix, plan: ScenePlan, now: Instant) {
    state.fx_return_ramps.clear();
    for (pair, volume) in plan.kept {
        if let Some(slot) = state.control_room.fx_muted.as_mut().and_then(|k| k.iter_mut().find(|(p, _)| *p == pair)) {
            slot.1 = volume;
        }
    }
    for (pair, from, to) in plan.glides {
        state.fx_return_ramps.insert(pair, (from, to, now));
    }
}

/// The gliding returns' next step, on the tick.
pub fn step_return_ramps(state: &mut TuxMix, now: Instant) {
    let ramps: Vec<(usize, (f32, f32, Instant))> = state.fx_return_ramps.iter().map(|(&p, &r)| (p, r)).collect();
    let prev = crate::app::ramp_step_clock(state, crate::app::RampKind::FxReturn, now, !ramps.is_empty());
    for (pair, (from_db, to_db, since)) in ramps {
        // A late tick takes it no further than a regular step (the scout).
        let (t, since) = crate::app::capped_share(since, prev, now, crate::app::RAMP_FOR, to_db - from_db);
        state.fx_return_ramps.insert(pair, (from_db, to_db, since));
        let _ = state.device.set_fx_return(pair, db_vol(from_db + (to_db - from_db) * t));
        if t >= 1.0 {
            state.fx_return_ramps.remove(&pair);
        }
    }
}

// ── Meters ────────────────────────────────────────────────────────

/// The send and return meters' next step. The send is the unit's own FX
/// pair where the driver carries it; the simulation's is AN1/2 at the send
/// level, and it has no return, since nothing runs.
pub fn step_meters(state: &mut TuxMix, inputs: &[f32], ret: Option<[f32; 2]>, hold: std::time::Duration) {
    let send = if state.device.is_mock() {
        let g = db_vol(send_heard_db(state));
        [inputs.first().copied().unwrap_or(0.0) * g, inputs.get(1).copied().unwrap_or(0.0) * g]
    } else {
        state.device.fx_send_meters().unwrap_or([0.0; 2])
    };
    let ret = ret.unwrap_or([0.0; 2]);
    for (m, level) in state.fx_meters.iter_mut().zip([send[0], send[1], ret[0], ret[1]]) {
        m.step_elapsed(level);
        m.hold_peak(level, hold);
    }
}

/// What the page says above 48 kHz.
pub fn rate_too_high_words(rate: u32) -> String {
    let khz = rate as f32 / 1000.0;
    let khz = if khz.fract() == 0.0 { format!("{khz:.0}") } else { format!("{khz:.1}") };
    format!("Reverb and Echo work up to 48 kHz; the unit runs at {khz} kHz now. Set 44.1 or 48 kHz in the device card to use them.")
}

// ── Pro Audio ─────────────────────────────────────────────────────

/// The unit's card in PipeWire (`pactl list cards`): the card of the
/// Babyface sink the meters use (`alsa_output.<device>.<profile>` belongs
/// to `alsa_card.<device>`), else the first Babyface card.
fn babyface_card() -> Option<String> {
    let device = tuxmix_core::pw_meter::discover(tuxmix_core::pw_meter::TapKind::Sink).and_then(|sink| {
        let rest = sink.name.strip_prefix("alsa_output.")?;
        Some(rest.rsplit_once('.').map_or(rest, |(device, _)| device).to_string())
    });
    let out = std::process::Command::new("pactl")
        .args(["--format=json", "list", "cards"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let cards: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let names: Vec<&str> = cards
        .as_array()?
        .iter()
        .filter_map(|c| c.get("name")?.as_str())
        .filter(|name| name.contains("Babyface"))
        .collect();
    device
        .and_then(|d| names.iter().find(|n| n.strip_prefix("alsa_card.") == Some(d.as_str())))
        .or_else(|| names.first())
        .map(|n| n.to_string())
}

/// Switches the unit's card to Pro Audio, the profile that opens all its
/// channels (the FX pair among them). Only ever on the user's press.
fn switch_to_pro_audio() -> Result<(), String> {
    let card = babyface_card().ok_or_else(|| "PipeWire doesn't show the unit's card".to_string())?;
    let status = std::process::Command::new("pactl")
        .args(["set-card-profile", &card, "pro-audio"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("pactl: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("the card refused the profile".to_string())
    }
}

// ── The page ──────────────────────────────────────────────────────

/// The status line, the press that fixes it (its label and message), and
/// two flags: said in `attention`, and still blocking.
type Status = (String, Option<(&'static str, Message)>, bool, bool);

/// What keeps the effects from being heard in a mix, the first that
/// applies (the one "what you hear" model, critique #67's bigger step:
/// the page's line, the placard, Rec's reasons and the Mute FX line all
/// read it). `None`: on and nothing in the way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    /// Both effects off.
    Off,
    /// The driver has no FX channels.
    NoDriver,
    /// The card is on the stereo profile.
    NeedsProAudio,
    /// The driver opens 12 channels only.
    OldDriver,
    /// pw-record / pw-play missing.
    ToolsMissing,
    /// The unit runs above 48 kHz.
    RateTooHigh(u32),
    /// The loop is looking for the unit.
    Searching,
    /// No FX send up, and this mix's return off.
    NoSendNoReturn,
    /// No FX send up.
    NoSend,
    /// This mix's FX return off.
    NoReturn,
    /// AN1/2's send follows its faders, which are down.
    SendFollowsDown,
    /// Mute FX holds this mix's return.
    HeldByMute,
    /// The driver can't play the FX return back (issue #16).
    NoEngine,
    /// The speakers' return kept off on purpose: an open mic feeds the send,
    /// and its reverb there could howl (critique #108 A P1-1).
    KeptForMic,
}

/// `pair` is the speakers and an open mic (48V, or its gain at 20 dB or
/// more) feeds the shared AN1/2 send: its reverb there could howl.
pub fn open_mic_reverb_risk(state: &TuxMix, pair: usize) -> bool {
    let assign = state.control_room.assign;
    let speakers = pair == assign.main || assign.main_b == Some(pair);
    speakers
        && (0..2).any(|i| {
            let p = crate::app::strip_params(state, ChannelId::Input(i), pair);
            p.has_48v && (p.phantom || p.gain >= 20)
        })
}

/// The speakers (Main, Main B) where an open mic's reverb plays now: the
/// effect on, the mic's own send heard, that speakers' FX return up and not
/// held by Mute FX. Whatever mix is being edited (critique #110 A P2-1: the
/// guard read only the selected mix). Not a reason the effects aren't
/// heard: they are, and that is the hazard (critique #110 B: as a `Block`
/// it read as "not heard" and broke Mute FX's card and the flow map).
pub fn mic_reverb_hazard(state: &TuxMix) -> Option<usize> {
    if !state.fx.any_on() || send_heard_db(state) <= OFF_DB {
        return None;
    }
    let assign = state.control_room.assign;
    std::iter::once(assign.main)
        .chain(assign.main_b)
        .find(|&p| open_mic_reverb_risk(state, p) && user_return(state, p) > 0.0 && !held_by_mute(state, p))
}

/// The speakers where releasing Mute FX would bring an open mic's reverb
/// in: the hazard Mute FX is holding back now (critique #111 A P2-1: the
/// FX-sends placard's "Release Mute FX" led straight into it).
pub fn mute_holds_mic_reverb(state: &TuxMix) -> Option<usize> {
    if !state.fx.any_on() || send_heard_db(state) <= OFF_DB {
        return None;
    }
    let assign = state.control_room.assign;
    std::iter::once(assign.main)
        .chain(assign.main_b)
        .find(|&p| open_mic_reverb_risk(state, p) && user_return(state, p) > 0.0 && held_by_mute(state, p))
}

/// What keeps the effects from being heard in mix `pair` (see `Block`).
pub fn block_in(state: &TuxMix, pair: usize) -> Option<Block> {
    block_with(state, pair, true)
}

/// Whether Mute FX is what keeps the effects out of `pair`: held there, and
/// heard the moment it lets go. A held mix with no send up isn't silenced
/// by it (nothing would be heard anyway), so the Mute FX tip and line leave
/// it out.
pub fn silenced_by_mute(state: &TuxMix, pair: usize) -> bool {
    held_by_mute(state, pair) && block_with(state, pair, false).is_none()
}

/// What would stand in the way once Mute FX lets go of `pair` (None: the
/// effects would be heard). The page and the row say it beside Mute FX, so
/// Release isn't promised to bring back what nothing feeds (critique #71
/// A 2).
/// The effects that are on, as a sentence names them: "the Reverb", "the
/// Echo", or "Reverb and Echo" (both, or neither).
pub fn on_words(state: &TuxMix) -> &'static str {
    match (state.fx.reverb_on, state.fx.echo_on) {
        (true, false) => "Reverb",
        (false, true) => "Echo",
        _ => "Reverb and Echo",
    }
}

pub fn block_past_mute(state: &TuxMix, pair: usize) -> Option<Block> {
    block_with(state, pair, false)
}

fn held_by_mute(state: &TuxMix, pair: usize) -> bool {
    state.control_room.fx_muted.iter().flatten().any(|&(p, _)| p == pair)
}

/// The model, with or without the Mute FX hold. Mute FX comes first after
/// what the system lacks (OpenCode's job 71: checked after the send, a held
/// mix with no send up named the send and offered a press that only set
/// the level kept for later): it is the user's own switch, and its release
/// is the one press that changes what's heard.
fn block_with(state: &TuxMix, pair: usize, mind_mute: bool) -> Option<Block> {
    if !state.fx.any_on() {
        return Some(Block::Off);
    }
    let mock = state.device.is_mock();
    if !mock {
        if !state.fx_available {
            return Some(Block::NoDriver);
        }
        match state.fx_state {
            Some(FxLoopState::NeedsProAudio) => return Some(Block::NeedsProAudio),
            Some(FxLoopState::NeedsDriver) => return Some(Block::OldDriver),
            Some(FxLoopState::ToolsMissing) => return Some(Block::ToolsMissing),
            Some(FxLoopState::RateTooHigh(rate)) => return Some(Block::RateTooHigh(rate)),
            Some(FxLoopState::Searching) | None => return Some(Block::Searching),
            Some(FxLoopState::Running(_)) => {}
        }
    }
    if mind_mute && held_by_mute(state, pair) {
        return Some(Block::HeldByMute);
    }
    // Any source pair's send counts, not AN1/2's alone (every pair has one
    // since the 2026-11-09 driver).
    let other_send = SEND_PAIR_CHANNELS
        .iter()
        .any(|&c| state.device.has_source_fx_send(c) && state.device.settings().source_fx_send_of(c) > OFF_DB);
    let (no_send, no_return) = (send_db(state) <= OFF_DB && !other_send, user_return(state, pair) <= 0.0);
    match (no_send, no_return) {
        (true, true) => return Some(Block::NoSendNoReturn),
        (true, false) => return Some(Block::NoSend),
        // The speakers' return off on purpose for an open mic (with no send
        // up the plain "no send" words stay: critique #110 B).
        (false, true) if open_mic_reverb_risk(state, pair) => return Some(Block::KeptForMic),
        (false, true) => return Some(Block::NoReturn),
        _ => {}
    }
    if !other_send && send_heard_db(state) <= OFF_DB {
        return Some(Block::SendFollowsDown);
    }
    if !mock && !state.device.has_fx_engine() {
        return Some(Block::NoEngine);
    }
    None
}

/// Whether the effects are heard in mix `pair`: on, and nothing in the way.
pub fn heard_in(state: &TuxMix, pair: usize) -> bool {
    block_in(state, pair).is_none()
}

/// What stands between the user and hearing the effects, with the press
/// that fixes it: the first of these that applies. The flags: whether it is
/// something the system lacks (said in `attention`), and whether it still
/// keeps the effects from being heard (the On keys stay armed until not).
fn status(state: &TuxMix) -> Option<Status> {
    let pair = state.sel_out;
    // As a sentence names it (OpenCode's job 77).
    let mix = crate::sidebar::mix_said(state, pair);
    let mix_s = crate::sidebar::mix_said_s(state, pair);
    let mock = state.device.is_mock();
    // An open mic's reverb in the speakers comes first, whatever mix is
    // being edited (critique #110 A P2-1).
    if let Some(sp) = mic_reverb_hazard(state) {
        let what = on_words(state);
        let has_phones = state.control_room.assign.phones_pairs().first().is_some();
        return Some((
            // In the simulation nothing plays; the setting is still the
            // hazard it would be on the unit (critique #111 A P3-3).
            format!("Your open mic\u{2019}s {} {} in your speakers: it can howl. {}", what, if mock || !matches!(state.fx_state, Some(FxLoopState::Running(_))) { "would play" } else { "plays" }, if has_phones { "Move it to your headphones." } else { "Take it out of the speakers." }),
            Some((if has_phones { "Move it to the headphones" } else { "Take it out of the speakers" }, Message::FxMakeHeard(sp))),
            true,
            false,
        ));
    }
    Some(match block_in(state, pair) {
        // Both off: said quietly, with the snapshot that brings them in
        // (critique #21: the page was silent at Audra's dry start).
        Some(Block::Off) => {
            let snap = fx_snapshot(state);
            let line = match snap {
                Some(n) => format!(
                    "Both off, as Audra always starts. {} (snapshot {n}) brings them in, with its whole mix.",
                    crate::app::snapshot_title(state, Some(n as usize))
                ),
                None => "Both off, as Audra always starts. Turn on Reverb here; Echo has its own key below.".to_string(),
            };
            // A key in the block's held room in this state too (critique
            // #90 A P3-1: two lines over 55 px of nothing); pressed, "Make
            // it heard" takes its place under the pointer, the next step.
            (line, Some(snap.map_or(("Turn on Reverb", Message::FxOn(Unit::Reverb, true)), |n| ("Recall its snapshot", Message::SnapshotClicked(n)))), false, true)
        }
        Some(Block::NoDriver) => ("Needs a newer Babyface driver, which opens the unit's FX channels.".into(), None, true, true),
        // Kept out of the speakers on purpose; the headphones are where it
        // plays (critique #108 A P1-1).
        // "It plays in your headphones" only when it does (critique #109 A
        // P2-2); else the press that puts it there (Make it heard moves to
        // the headphones by itself).
        // The effect is the subject, not the mic (critique #110 P3-18,
        // #111 A: under "Open mic in your speakers" it read as the mic kept
        // out); in the simulation nothing plays (#110 P3-13).
        Some(Block::KeptForMic) => match state.control_room.assign.phones_pairs().first().copied() {
            Some(ph) if heard_in(state, ph) => (
                format!(
                    "{} kept out of your speakers: from an open mic it can howl there. {}",
                    on_words(state),
                    if mock { "Your headphones have it (the simulation plays nothing live)." } else { "It plays in your headphones." }
                ),
                Some(("Edit the headphones\u{2019} mix", Message::EditMix(ph))),
                false,
                false,
            ),
            Some(_) => (
                format!("{} kept out of your speakers: from an open mic it can howl there. Make it heard in your headphones instead.", on_words(state)),
                Some(("Make it heard in the headphones", Message::FxMakeHeard(pair))),
                false,
                true,
            ),
            None => (format!("{} kept out of your speakers: from an open mic it can howl there. Assign headphones to hear it there.", on_words(state)), None, false, true),
        },

        Some(Block::NeedsProAudio) => (
            "Needs the Pro Audio profile: the stereo one opens only two channels.".into(),
            Some(("Switch to Pro Audio", Message::FxProAudio)),
            true,
            true,
        ),
        Some(Block::OldDriver) => ("The Babyface driver here opens 12 channels; the FX pair needs a newer one.".into(), None, true, true),
        Some(Block::ToolsMissing) => ("Needs pw-record and pw-play, PipeWire's own tools.".into(), None, true, true),
        Some(Block::RateTooHigh(rate)) => (rate_too_high_words(rate), None, true, true),
        Some(Block::Searching) => ("Looking for the unit\u{2026}".into(), None, false, true),
        // The send and this mix's return, whichever are off, in one press.
        Some(b @ (Block::NoSendNoReturn | Block::NoSend | Block::NoReturn)) => {
            let line = match b {
                Block::NoSendNoReturn => format!("Not heard yet: no FX send is up, and {mix_s} FX return is off."),
                Block::NoSend => "Not heard yet: no FX send is up.".into(),
                _ => format!("Not heard in {mix} yet: the FX return there is off."),
            };
            // "In recordings" lit, and still this to press: say why (the
            // owner, issue #41: "surprising that I had to click Make it
            // heard even though In Recordings was clicked").
            let line = if [Unit::Reverb, Unit::Echo].iter().any(|&u| is_on(state, u) && rec_includes(state, u)) {
                format!("{line} Rec records an effect as you hear it, so it waits for this too.")
            } else {
                line
            };
            (line, Some(("Make it heard", Message::FxMakeHeard(pair))), false, true)
        }
        Some(Block::SendFollowsDown) => (
            match state.fx_send_follow {
                SendFollow::Main => "Reverb and Echo hear nothing yet: AN1/2 is down in the Main mix, and its FX send follows it.".into(),
                _ => "Reverb and Echo hear nothing yet: AN1/2 is down in every mix, and its FX send follows its faders.".into(),
            },
            None,
            false,
            true,
        ),
        // The mix's name kept on one line (critique #67 A 4: "Main / 1/2").
        Some(Block::HeldByMute) => (
            // The effect that's on by name (critique #72 A 6: "Reverb and
            // Echo" with only the Reverb on).
            crate::celebrate::no_orphan(&{
                let fx = on_words(state);
                match block_past_mute(state, pair) {
                    Some(Block::NoSend | Block::NoSendNoReturn) => format!("Mute FX is holding {fx} off {mix}, and no FX send is up yet either."),
                    Some(Block::NoReturn) => format!("Mute FX is holding {fx} off {mix}, and the FX return there is off too."),
                    Some(Block::SendFollowsDown) => format!("Mute FX is holding {fx} off {mix}, and AN1/2 is down, so nothing feeds it."),
                    // The driver too, so Release isn't the whole fix
                    // (OpenCode's job 73).
                    Some(Block::NoEngine) => format!("Mute FX is holding {fx} off {mix}, and this Babyface driver can't play them back anyway."),
                    // In the simulation, as every other line there says
                    // (OpenCode's job 73).
                    _ if mock => format!("Mute FX is holding {fx} off {mix}. (The\u{a0}simulation plays no effects.)"),
                    _ => format!("Mute FX is holding {fx} off {mix}."),
                }
            }),
            // Named as the placard's key (critique #72 A 8). Released, an
            // open mic's reverb would play in the speakers: the move first,
            // as the FX-sends placard does (critique #112 A P3-3 / P3-1).
            Some(match mute_holds_mic_reverb(state) {
                Some(sp) if state.control_room.assign.phones_pairs().is_empty() => ("Take it out of the speakers", Message::FxMakeHeard(sp)),
                Some(sp) => ("Move it to the headphones", Message::FxMakeHeard(sp)),
                None => ("Release Mute FX", Message::MuteFxToggle),
            }),
            false,
            true,
        ),
        // On the unit, everything set and the effects running: the Babyface
        // driver still can't play the FX return back (issue #16, measured
        // 2026-11-07: a tone into the return channels is silent), so say so
        // plainly rather than look ready; the On keys stay armed. Only on a
        // driver without the FX pair's arm (fixed by the driver's "FX Engine
        // Switch", 2026-11-09).
        Some(Block::NoEngine) => (
            "Reverb and Echo are running, but this Babyface driver can't play them back on the unit, so they aren't heard. Install the newer driver (see the README).".into(),
            None,
            true,
            true,
        ),
        None => {
            let rate = match state.fx_state {
                Some(FxLoopState::Running(rate)) => format!(" at {:.0} kHz", rate as f32 / 1000.0),
                _ => String::new(),
            };
            (
                if mock {
                    // What is on, and why nothing sounds (critique #88 A
                    // P3-4: a lit key over a dark Return meter read as
                    // broken).
                    let what = match (is_on(state, Unit::Reverb), is_on(state, Unit::Echo)) {
                        (true, true) => "Reverb and Echo are on",
                        (false, true) => "Echo is on",
                        _ => "Reverb is on",
                    };
                    format!("{what}. Nothing plays live in the simulation: Hear a sample plays a clap plain, then through them, as {mix} would.")
                } else {
                    format!("Running{rate}, into {mix}.")
                },
                // A clap through them, to hear what's on with no mic up (the
                // owner, 2026-11-06: "Cant tell if reverb is working").
                // Hidden while locked: it would sound on air.
                // In the simulation, a sample rendered by the same engine
                // (the owner's pick, 2026-11-21).
                (!state.ui_locked).then_some(if mock { ("Hear a sample", Message::FxPreview) } else { ("Hear a clap", Message::FxAudition) }),
                false,
                false,
            )
        }
    })
}

/// Whether the effects can be heard: on, and nothing in their way.
/// The snapshot slot (1-8) stored with both effects on, else with either:
/// what the page's "Recall its snapshot" recalls ("Talking + reverb").
fn fx_snapshot(state: &TuxMix) -> Option<u8> {
    let stored = |i: usize| state.stored_snapshots.get(i).copied().unwrap_or(false);
    (0..8)
        .find(|&i| stored(i) && state.snapshot_fx[i] == Some((true, true)))
        .or_else(|| (0..8).find(|&i| stored(i) && state.snapshot_fx[i].is_some_and(|(r, e)| r || e)))
        .map(|i| i as u8 + 1)
}

/// The page's key, for the tests: its label and its press.
#[cfg(test)]
pub(crate) fn page_key_for_tests(state: &TuxMix) -> Option<(&'static str, Message)> {
    status(state).and_then(|(_, key, _, _)| key)
}

pub fn heard(state: &TuxMix) -> bool {
    // A hazard is no win: no flare, no milestone, no guide tick (critique
    // #110 A P2-2).
    state.fx.any_on() && mic_reverb_hazard(state).is_none() && status(state).is_some_and(|(_, _, _, blocking)| !blocking)
}

/// How recently the user must have touched the effects (On, a send, a
/// return, Mute FX, Pro Audio) for their being heard to be their doing:
/// the loop can take a few seconds to come up after On.
const TOUCHED_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// On the tick: the moment the effects go from silent to heard by the
/// user's hand, their lit On keys flare (`motion::heard_flare`): lit means
/// heard, and this is the sound coming in. Not at start (the first look
/// only notes it), not on picking another mix (the sound didn't change),
/// not when the loop comes back up on its own.
pub fn watch_heard(state: &mut TuxMix) {
    // What Make it heard turned up, once down again however it got there,
    // is forgotten: a send raised by hand later isn't its to turn down
    // (Fable's review of issue #45).
    if state.made_heard.is_some() && made_heard_up(state).is_none() {
        state.made_heard = None;
    }
    // A screenshot holds its moment (`--open fx-heard`).
    if state.capture_burst_t.is_some() {
        return;
    }
    let now = (heard(state), state.sel_out);
    if let Some((was, mix)) = state.fx_seen {
        let touched = state.fx_touched_at.is_some_and(|at| at.elapsed() < TOUCHED_WITHIN);
        if now.0 && !was && now.1 == mix && touched {
            state.fx_heard_at = Some(std::time::Instant::now());
        }
    }
    state.fx_seen = Some(now);
}

/// The page's key for the next step, if there is one.
pub fn status_action(state: &TuxMix) -> Option<Message> {
    if turn_down_offer(state) {
        return Some(Message::FxTurnBackDown);
    }
    status(state).and_then(|(_, action, _, _)| action).map(|(_, message)| message)
}

/// Whether Rec's takes carry `unit` (Audra settings has the same two
/// switches).
pub fn rec_includes(state: &TuxMix, unit: Unit) -> bool {
    match unit {
        Unit::Reverb => state.record_reverb,
        Unit::Echo => state.record_echo,
    }
}

/// Whether a unit's In recordings key shows armed: included, but the take
/// wouldn't hold it now (by the Rec key's own rule, `app::take_fx`).
pub fn rec_key_armed(state: &TuxMix, unit: Unit) -> bool {
    rec_includes(state, unit) && !(is_on(state, unit) && crate::app::take_fx(state).is_ok())
}

pub fn is_on(state: &TuxMix, unit: Unit) -> bool {
    match unit {
        Unit::Reverb => state.fx.reverb_on,
        Unit::Echo => state.fx.echo_on,
    }
}

fn params_of(state: &TuxMix, unit: Unit) -> Vec<Param> {
    match unit {
        Unit::Reverb => reverb_params(state.fx.reverb),
        Unit::Echo => echo_params(state.fx.echo),
    }
}

/// How many knobs `unit` shows (its type decides).
pub fn knob_count(state: &TuxMix, unit: Unit) -> usize {
    params_of(state, unit).len()
}

/// Knob `i` of `unit` a step along its sweep (a fiftieth; on a frequency or
/// delay knob, along its log sweep), as the keyboard turns it.
pub fn knob_nudge(state: &TuxMix, unit: Unit, i: usize, dir: i32) -> Option<Message> {
    let p = params_of(state, unit).into_iter().nth(i)?;
    let (lo, hi) = p.range;
    let to = if p.log {
        let t = (p.value / lo).ln() / (hi / lo).ln() + dir as f32 / 50.0;
        lo * (hi / lo).powf(t.clamp(0.0, 1.0))
    } else {
        p.value + (hi - lo) * dir as f32 / 50.0
    }
    .clamp(lo, hi);
    Some((p.set)(to))
}

/// `unit`'s next type (or the one before), round the list.
pub fn type_step(state: &TuxMix, unit: Unit, dir: i32) -> Message {
    fn next<T: Copy + PartialEq>(all: &[T], now: T, dir: i32) -> T {
        let at = all.iter().position(|k| *k == now).unwrap_or(0) as i32;
        all[(at + dir).rem_euclid(all.len() as i32) as usize]
    }
    match unit {
        Unit::Reverb => {
            let p = state.fx.reverb;
            Message::FxReverb(ReverbParams { kind: next(&ReverbKind::ALL, p.kind, dir), ..p })
        }
        Unit::Echo => {
            let p = state.fx.echo;
            Message::FxEcho(EchoParams { kind: next(&EchoKind::ALL, p.kind, dir), ..p })
        }
    }
}

/// The preset after (or before) the one `unit` is set to, in the menu's
/// order; from the first or last when it is set to none.
pub fn preset_step(state: &TuxMix, unit: Unit, dir: i32) -> Option<Message> {
    let mut picks: Vec<PresetPick> = Vec::new();
    if has_kept(state, unit) {
        picks.push(PresetPick::Kept);
    }
    picks.extend((0..user_names(state, unit).len()).map(PresetPick::User));
    picks.extend((0..factory_names(unit).len()).map(PresetPick::Factory));
    picks.push(PresetPick::Default);
    let n = picks.len() as i32;
    // From the preset in force, or the one a knob turn changed.
    let at = preset_label(state, unit).and_then(|(pick, _)| picks.iter().position(|p| *p == pick));
    let to = match at {
        Some(at) => (at as i32 + dir).rem_euclid(n),
        None if dir > 0 => 0,
        None => n - 1,
    };
    Some(Message::FxPresetLoad(unit, picks[to as usize]))
}

use crate::keyfocus::CounterItem;

/// Whether the keyboard is on this item of the page.
fn kb(state: &TuxMix, item: CounterItem) -> bool {
    crate::keyfocus::counter_focused(state, item)
}

/// The counter's "Reverb & Echo" page.
pub fn page(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut col = column![].spacing(theme::SPACE_LG * scale).width(Length::Fill);
    if let Some((line, action, attention, _)) = status(state) {
        let words = text(line)
            .size(theme::TEXT_SM * scale)
            .color(if attention { theme::attention() } else { theme::text_sec() });
        let mut block = column![words].spacing(theme::SPACE_SM * scale);
        // Make it heard pressed and still up: its key stays in its place,
        // lit, and turns it back down (the owner, issue #45: "I think we
        // need a way to toggle the change"). A sample's key beside it.
        let turn_down = turn_down_offer(state);
        let action = if turn_down {
            let h = 30.0 * scale;
            let toggle = button(text("Turn it back down").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
                .padding([0.0, theme::SPACE_MD * scale])
                .height(Length::Fixed(h))
                .style(theme::key_button(true, 0.0, 0.0, h / 2.0))
                .on_press(Message::FxTurnBackDown);
            let toggle: Element<'_, Message> = iced::widget::tooltip(
                crate::keyfocus::ring(toggle.into(), kb(state, CounterItem::FxAction), h / 2.0),
                container(text(TURN_DOWN_TIP).size(theme::TEXT_SM * scale).color(theme::text_primary()))
                    .padding(theme::SPACE_SM * scale)
                    .style(crate::widgets::hint_panel),
                iced::widget::tooltip::Position::Bottom,
            )
            .gap(4.0 * scale)
            .delay(crate::widgets::hint_delay())
            .into();
            let mut keys = row![toggle].spacing(theme::SPACE_SM * scale).align_y(iced::Alignment::Center);
            if let Some((label @ ("Hear a sample" | "Hear a clap"), message)) = action {
                let sample: iced::widget::Button<'_, Message> = button(text(label).font(theme::SANS).size(theme::TEXT_SM * scale).center())
                    .padding([0.0, theme::SPACE_MD * scale])
                    .height(Length::Fixed(h))
                    .style(move |t: &iced::Theme, st| {
                        let mut style = theme::key_button(false, 0.0, 0.0, h / 2.0)(t, st);
                        style.border = theme::page_key(h / 2.0)(t, st).border;
                        style
                    })
                    .on_press(message);
                keys = keys.push(crate::widgets::strip::hint(sample, if label == "Hear a clap" { "Plays one clap through the effects, to hear what is on" } else { "Plays a clap on this computer, as it is and then through Reverb and Echo as they are set (quiet, about 3 seconds)" }, scale));
            }
            block = block.push(keys);
            None
        } else {
            action
        };
        if let Some((label, message)) = action {
            // The one thing to do next, as a key like the On keys (the
            // reviewer: a small capsule read as a tag).
            let h = 30.0 * scale;
            // A fix in the semi-bold; an everyday offer (both off: nothing
            // is wrong) in the regular weight (critique #91 A P3-4).
            let offer = matches!(label, "Turn on Reverb" | "Recall its snapshot" | "Hear a sample" | "Hear a clap");
            let key = button(text(label).font(if offer { theme::SANS } else { theme::SANS_SEMI }).size(theme::TEXT_MD * scale).center())
                .padding([0.0, theme::SPACE_XL * scale])
                .height(Length::Fixed(h))
                // The page's main action with its weakest rim (critique #89
                // B: 2.51-2.99 on Daylight): the firm rim, on the key's face.
                .style(move |t: &iced::Theme, s| {
                    let mut style = theme::key_button(false, 0.0, 0.0, h / 2.0)(t, s);
                    style.border = theme::page_key(h / 2.0)(t, s).border;
                    style
                })
                .on_press(message);
            // What the press does, on hover (the owner, 2026-11-17: "tooltip
            // is missing from button").
            let tip = match label {
                "Recall its snapshot" => "Loads that snapshot: its whole mix, with Reverb or Echo on as it was stored (Undo takes it back)",
                // Where the press lands (critique #111 Fable B: an open mic
                // on the speakers sends it to the headphones).
                "Make it heard" => block_in(state, state.sel_out).map_or("Turns up the FX send and this mix's FX return, in one press (Undo takes it back)", |b| crate::sidebar::make_heard_tip_in(state, state.sel_out, b)),
                "Move it to the headphones" => "Takes the speakers\u{2019} FX return down and plays the reverb in your headphones instead: an open mic\u{2019}s reverb in the speakers can howl (Undo takes it back)",
                "Take it out of the speakers" => "Takes the speakers\u{2019} FX return down: an open mic\u{2019}s reverb there can howl. Assign headphones to hear it there (Undo takes it back)",
                "Release Mute FX" if mute_holds_mic_reverb(state).is_some() => "Lets the effects back, an open mic\u{2019}s reverb into your speakers too, where it can howl: move it to the headphones first",
                "Release Mute FX" => "Lets the effects back into the mixes Mute FX is holding",
                "Switch to Pro Audio" => "Switches the sound card to its Pro Audio profile, which opens the FX channels",
                "Hear a clap" => "Plays one clap through the effects, to hear what is on",
                "Hear a sample" => "Plays a clap on this computer, as it is and then through Reverb and Echo as they are set (quiet, about 3 seconds)",
                "Turn on Reverb" => "Switches Reverb on; this key then becomes the next step, Make it heard",
                _ => "The next thing to do here",
            };
            // Its tip opens under the key, over the meters: over it stands
            // the sentence that explains the key, and the pointer rests here
            // after the first of the two presses (critique #93 A P3-1).
            let key: Element<'_, Message> = iced::widget::tooltip(
                crate::keyfocus::ring(key.into(), kb(state, CounterItem::FxAction), h / 2.0),
                container(text(tip).size(theme::TEXT_SM * scale).color(theme::text_primary()))
                    .padding(theme::SPACE_SM * scale)
                    .style(crate::widgets::hint_panel),
                iced::widget::tooltip::Position::Bottom,
            )
            .gap(4.0 * scale)
            .delay(crate::widgets::hint_delay())
            .into();
            block = block.push(key);
        }
        // Room held for three lines and the key, whatever it says now: the
        // press on Reverb grew this block and moved the Reverb key from
        // under the pointer, "FX sends on faders" landing there for a second
        // click (found walking the page key by key, 2026-11-18).
        let held = (3.0 * theme::TEXT_SM * 1.3 + theme::SPACE_SM + 30.0) * scale;
        // Standing on the room's foot, so the key keeps its place whether
        // two lines or three stand over it: "Turn on Reverb" pressed,
        // "Make it heard" came one line lower, a press on the key's upper
        // edge missing it (critique #92's captures).
        let block = row![iced::widget::Space::new().width(Length::Fixed(1.0)).height(Length::Fixed(held)), block].align_y(iced::Alignment::End);
        col = col.push(container(block).padding([0.0, theme::SPACE_SM * scale]));
    }
    col = col.push(meters(state, scale));
    // "Faders show FX sends" (the owner's pick from mockups, 2026-11-09):
    // the whole reverb mix on the faders, amber, until switched back. Only
    // where the effects are (critique #19).
    let on = state.fader_fx;
    let h = 28.0 * scale;
    // One label; the key's light says it's on, as every Audra key does.
    let key = button(text("FX sends on faders").size(theme::TEXT_SM * scale).center())
        .padding([0.0, theme::SPACE_LG * scale])
        .height(Length::Fixed(h))
        // Unlit, a rim that reads wherever it lands (critique #87 B: 1.90
        // to 2.60 top and bottom).
        .style(theme::split_safe_key(theme::key_button(on, 0.0, 0.0, h / 2.0), on))
        .on_press(Message::FaderFxToggle);
    if in_panels(state) { col = col.push(
        container(crate::widgets::strip::hint(
            crate::keyfocus::ring(key.into(), kb(state, CounterItem::FxFaders), h / 2.0),
            "Every input and playback fader sets its FX send, amber (output faders stay your volume); press again for the mix",
            scale,
        ))
        .padding([0.0, theme::SPACE_SM * scale]),
    ); }
    // The two effects as two groups: more room between them than inside.
    // Each effect a framed unit, as the Control Room's keys are one.
    col.push(
        column![
            crate::sidebar::section(None, unit_block(state, Unit::Reverb, scale), scale),
            crate::sidebar::section(None, unit_block(state, Unit::Echo, scale), scale),
        ]
        .spacing(theme::SPACE_XL * scale),
    )
    .into()
}

/// The page as a card beside the counter: its name and a close, then the
/// page.
pub fn card_body(state: &TuxMix) -> Element<'_, Message> {
    let scale = state.ui_scale;
    let close = crate::widgets::strip::hint(
        button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
            .padding(0)
            .width(Length::Fixed(20.0 * scale))
            .height(Length::Fixed(20.0 * scale))
            .style(theme::icon_button(false))
            .on_press(Message::FxCardClose),
        "Close (Esc)",
        scale,
    );
    column![
        row![
            text("Reverb & Echo").font(theme::SERIF).size(theme::TEXT_ROW * scale).color(theme::text_primary()).width(Length::Fill),
            close,
        ]
        .align_y(iced::Alignment::Center),
        page(state, scale),
    ]
    .spacing(theme::SPACE_LG * scale)
    .into()
}

/// The send going in and the return coming out, as the counter's level
/// meters.
fn meters(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let m = &state.fx_meters;
    let line = |label: &'static str, l: usize| {
        row![
            text(label)
                .size(theme::TEXT_SM * scale)
                .color(theme::muted())
                .width(Length::Fixed(44.0 * scale)),
            crate::widgets::fader::level_meter(m[l].frame(), Some(m[l + 1].frame()), scale),
        ]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center)
    };
    column![line("Send", 0), line("Return", 2)]
        .spacing(theme::SPACE_SM * scale)
        .padding([0.0, theme::SPACE_SM * scale])
        .into()
}

/// One knob of an effect: its name, where it is, its range and how it
/// reads, its default, and the message a turn sends.
struct Param {
    name: &'static str,
    /// What turning it does, in plain words, on hover (critique #27: the
    /// knobs said nothing, even on hover).
    tip: &'static str,
    value: f32,
    range: (f32, f32),
    log: bool,
    words: String,
    default: f32,
    set: Box<dyn Fn(f32) -> Message>,
}

fn ms(v: f32) -> String {
    format!("{v:.0} ms")
}

fn hz(v: f32) -> String {
    if v >= 9_950.0 {
        // "12 kHz", not "12.0 kHz": as wide as "100 Hz", so it fits its cell.
        format!("{:.0} kHz", v / 1000.0)
    } else if v >= 1000.0 {
        format!("{:.1} kHz", v / 1000.0)
    } else {
        format!("{v:.0} Hz")
    }
}

fn reverb_params(p: ReverbParams) -> Vec<Param> {
    let d = ReverbParams::default();
    let set = |f: fn(ReverbParams, f32) -> ReverbParams| -> Box<dyn Fn(f32) -> Message> { Box::new(move |v| Message::FxReverb(f(p, v))) };
    let mut v = vec![Param {
        name: "Pre-delay",
        tip: "A pause before the reverb starts: more keeps the voice clear of it",
        value: p.pre_delay_ms,
        range: (0.0, 999.0),
        log: false,
        words: ms(p.pre_delay_ms),
        default: d.pre_delay_ms,
        set: set(|p, v| ReverbParams { pre_delay_ms: v.round(), ..p }),
    }];
    if p.kind == ReverbKind::Long {
        v.push(Param {
            name: "Time",
            tip: "How long the reverb rings on",
            value: p.time_s,
            range: (0.1, 4.9),
            log: false,
            words: format!("{:.1} s", p.time_s),
            default: d.time_s,
            set: set(|p, v| ReverbParams { time_s: v, ..p }),
        });
        v.push(Param {
            name: "High damp",
            tip: "Softens the reverb's highs as it fades",
            value: p.high_damp_hz,
            range: (5_000.0, 20_000.0),
            log: true,
            words: hz(p.high_damp_hz),
            default: d.high_damp_hz,
            set: set(|p, v| ReverbParams { high_damp_hz: v, ..p }),
        });
    } else {
        v.push(Param {
            name: "Room",
            tip: "How big the room sounds",
            value: p.room_scale,
            range: (0.5, 3.0),
            log: false,
            words: format!("{:.2}\u{00d7}", p.room_scale),
            default: d.room_scale,
            set: set(|p, v| ReverbParams { room_scale: v, ..p }),
        });
    }
    if matches!(p.kind, ReverbKind::Gated | ReverbKind::Envelope) {
        for (name, value, default, f) in [
            ("Attack", p.attack_ms, d.attack_ms, (|p, v| ReverbParams { attack_ms: v.round(), ..p }) as fn(ReverbParams, f32) -> ReverbParams),
            ("Hold", p.hold_ms, d.hold_ms, |p, v| ReverbParams { hold_ms: v.round(), ..p }),
            ("Release", p.release_ms, d.release_ms, |p, v| ReverbParams { release_ms: v.round(), ..p }),
        ] {
            let tip = match name {
                "Attack" => "How fast the reverb opens",
                "Hold" => "How long it stays open",
                _ => "How fast it closes",
            };
            v.push(Param { name, tip, value, range: (5.0, 400.0), log: false, words: ms(value), default, set: set(f) });
        }
    }
    v.extend([
        Param {
            name: "Smooth",
            tip: "Evens out the reverb's reflections: higher is smoother",
            value: p.smooth,
            range: (0.0, 100.0),
            log: false,
            words: format!("{:.0}", p.smooth),
            default: d.smooth,
            set: set(|p, v| ReverbParams { smooth: v.round(), ..p }),
        },
        Param {
            name: "Low cut",
            tip: "Takes the rumble out of the reverb",
            value: p.low_cut_hz,
            range: (20.0, 500.0),
            log: true,
            words: hz(p.low_cut_hz),
            default: d.low_cut_hz,
            set: set(|p, v| ReverbParams { low_cut_hz: v.round(), ..p }),
        },
        Param {
            name: "High cut",
            tip: "Takes the hiss and sparkle off the reverb",
            value: p.high_cut_hz,
            range: (5_000.0, 20_000.0),
            log: true,
            words: hz(p.high_cut_hz),
            default: d.high_cut_hz,
            set: set(|p, v| ReverbParams { high_cut_hz: v, ..p }),
        },
        Param {
            name: "Width",
            tip: "How wide the reverb spreads: 0 keeps it in the middle",
            value: p.width,
            range: (0.0, 100.0),
            log: false,
            words: format!("{:.0}", p.width),
            default: d.width,
            set: set(|p, v| ReverbParams { width: v.round(), ..p }),
        },
        Param {
            name: "Volume",
            tip: "How loud the reverb comes back",
            value: p.volume_db,
            range: (OFF_DB, 6.0),
            log: false,
            words: format!("{} dB", db_words(p.volume_db)),
            default: d.volume_db,
            set: set(|p, v| ReverbParams { volume_db: v, ..p }),
        },
    ]);
    v
}

fn echo_params(p: EchoParams) -> Vec<Param> {
    let d = EchoParams::default();
    let set = |f: fn(EchoParams, f32) -> EchoParams| -> Box<dyn Fn(f32) -> Message> { Box::new(move |v| Message::FxEcho(f(p, v))) };
    vec![
        Param {
            name: "Delay",
            tip: "The time between repeats",
            value: p.delay_ms,
            range: (1.0, 2000.0),
            log: true,
            words: ms(p.delay_ms),
            default: d.delay_ms,
            set: set(|p, v| EchoParams { delay_ms: v.round(), ..p }),
        },
        Param {
            name: "Feedback",
            tip: "How many repeats: more keeps them coming",
            value: p.feedback,
            range: (0.0, 100.0),
            log: false,
            words: format!("{:.0}", p.feedback),
            default: d.feedback,
            set: set(|p, v| EchoParams { feedback: v.round(), ..p }),
        },
        Param {
            name: "High cut",
            tip: "Makes each repeat darker",
            value: p.high_cut_hz,
            range: (2_000.0, 20_000.0),
            log: true,
            words: hz(p.high_cut_hz),
            default: d.high_cut_hz,
            set: set(|p, v| EchoParams { high_cut_hz: v, ..p }),
        },
        Param {
            name: "Width",
            tip: "How wide the repeats spread: 0 keeps them in the middle",
            value: p.width,
            range: (0.0, 100.0),
            log: false,
            words: format!("{:.0}", p.width),
            default: d.width,
            set: set(|p, v| EchoParams { width: v.round(), ..p }),
        },
        Param {
            name: "Volume",
            tip: "How loud the echo comes back",
            value: p.volume_db,
            range: (OFF_DB, 6.0),
            log: false,
            words: format!("{} dB", db_words(p.volume_db)),
            default: d.volume_db,
            set: set(|p, v| EchoParams { volume_db: v, ..p }),
        },
    ]
}

/// How many knobs each row holds: at most four, the rows as even as they
/// can be (seven are four and three, five are three and two).
fn row_sizes(n: usize) -> Vec<usize> {
    let rows = n.div_ceil(4).max(1);
    (0..rows).map(|r| n / rows + usize::from(r < n % rows)).collect()
}

/// An effect: its On key and its type, then its knobs in rows of up to
/// four, in columns that line up.
fn unit_block(state: &TuxMix, unit: Unit, scale: f32) -> Element<'_, Message> {
    let on = match unit {
        Unit::Reverb => state.fx.reverb_on,
        Unit::Echo => state.fx.echo_on,
    };
    let key_h = 30.0 * scale;
    let key = button(text(unit.word()).font(theme::SANS_SEMI).size(theme::TEXT_MD * scale).center())
        .width(Length::Fixed(88.0 * scale))
        .height(Length::Fixed(key_h))
        .on_press(Message::FxOn(unit, !on));
    // Lit only once it can be heard: on but still silent, it shows armed
    // (set, waiting for sound), as the page's line says what's missing.
    // An open mic's reverb in the speakers plays: lit, though no win (no
    // flare: that keys on `heard`; critique #111 Fable B).
    let key = if on && !heard(state) && mic_reverb_hazard(state).is_none() {
        key.style(theme::key_button_armed(theme::accent(), key_h / 2.0))
    } else {
        // Off, its rim held at 3:1 (critique #87 B: the Reverb key 1.82 /
        // 2.16, the Echo key 2.40 / 2.68 on Daylight).
        key.style(theme::split_safe_key(
            theme::key_button(
                on,
                crate::motion::bloom(state, crate::motion::Bloom::Key(unit.key_name())).max(crate::motion::heard_flare(state)),
                crate::motion::cool(state, crate::motion::Bloom::Key(unit.key_name())),
                key_h / 2.0,
            ),
            on,
        ))
    };
    // The moment the sound comes in: the lit key throws its light round it.
    // Always wrapped (with no shadow at rest), so the key's place in the
    // tree never changes under a held press.
    let flare = if on && heard(state) { crate::motion::heard_flare(state) } else { 0.0 };
    let key: Element<'_, Message> = container(key).style(theme::key_halo(theme::accent(), flare, key_h / 2.0)).into();
    let key = crate::keyfocus::ring(key, kb(state, CounterItem::FxOn(unit)), key_h / 2.0);
    let key = crate::widgets::strip::hint(
        key,
        match (unit, on) {
            (Unit::Reverb, false) => "Switch Reverb on",
            (Unit::Reverb, true) if !heard(state) => "The Reverb is on, not heard yet: the page says what's missing",
        (Unit::Reverb, true) => "Switch Reverb off",
            (Unit::Echo, false) => "Switch Echo on",
            (Unit::Echo, true) if !heard(state) => "The Echo is on, not heard yet: the page says what's missing",
        (Unit::Echo, true) => "Switch Echo off",
        },
        scale,
    );
    let kind: Element<'_, Message> = match unit {
        Unit::Reverb => {
            let p = state.fx.reverb;
            pick_list(ReverbKind::ALL.to_vec(), Some(p.kind), move |kind| Message::FxReverb(ReverbParams { kind, ..p }))
                .style(theme::pick_list)
                .menu_style(theme::menu)
                .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
                .text_size(theme::TEXT_SM * scale)
                .width(Length::Fill)
                .into()
        }
        Unit::Echo => {
            let p = state.fx.echo;
            pick_list(EchoKind::ALL.to_vec(), Some(p.kind), move |kind| Message::FxEcho(EchoParams { kind, ..p }))
                .style(theme::pick_list)
                .menu_style(theme::menu)
                .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
                .text_size(theme::TEXT_SM * scale)
                .width(Length::Fill)
                .into()
        }
    };
    let kind = crate::keyfocus::ring(kind, kb(state, CounterItem::FxType(unit)), theme::RADIUS_SM * scale);
    // Named, so it doesn't read as a second preset list (critique #27).
    let kind = crate::widgets::strip::hint(
        row![text("Type").size(theme::TEXT_SM * scale).color(theme::text_sec()), kind]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        "The kind of space; which knobs show depends on it. A preset below sets this and every knob at once",
        scale,
    );
    let params = params_of(state, unit);
    let mut grid = column![].spacing(theme::SPACE_MD * scale);
    let mut params = params.into_iter().enumerate();
    let sizes = row_sizes(params.len());
    for size in sizes {
        // The last row stands at the right, so Width and Volume end every
        // unit in the same two columns, Volume at the far right, as a
        // hardware box keeps its output knob in one place.
        // A short first row stands at the right too, so the rows' ends
        // line up (critique #27: the Echo's three over two left holes).
        let gap = || iced::widget::Space::new().width(Length::FillPortion(1));
        let mut r = row![].width(Length::Fill);
        for _ in size..4 {
            r = r.push(gap());
        }
        for _ in 0..size {
            let Some((i, p)) = params.next() else { break };
            r = r.push(cell(state, p, kb(state, CounterItem::FxKnob(unit, i as u8)), is_on(state, unit), scale));
        }
        grid = grid.push(r);
    }
    // Whether Rec's takes carry this effect (the owner, 2026-11-15: "a
    // button to include in record on and off per Echo or Reverb"), at the
    // unit's foot, on the right: lit while included, as every Audra key.
    let in_rec = rec_includes(state, unit);
    let h = 28.0 * scale;
    // "In recordings", as Audra settings names the same switch (critique
    // #60 A 5: "takes" was jargon, and one switch had two names).
    // Armed (included, not heard yet), Rec's red dot leads its words, so
    // its outline isn't read as "panel open" (critique #61 A 4).
    // Lit by the Rec key's own rule (`app::take_fx`), so the two never
    // disagree (critique #64 A 1: lit "as you hear it" from the mix being
    // edited while the take, following Quick's listen mix, came out dry).
    let take = crate::app::take_fx(state);
    let armed = rec_key_armed(state, unit);
    let words = text("In recordings").size(theme::TEXT_SM * scale);
    let label: Element<'_, Message> = if armed {
        row![text("\u{25cf}").size(theme::TEXT_SM * scale).color(theme::readable_on_floors(theme::clip())), words]
            .spacing(theme::SPACE_TIGHT * scale)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        words.into()
    };
    // The words centred in the key's height (critique #72 A 5: they sat
    // at its top, 1-2 px under the rim and 12 px clear below).
    let rec_key = button(container(label).center_y(Length::Fill))
        .padding([0.0, theme::SPACE_LG * scale])
        .height(Length::Fixed(h))
        .on_press(Message::SetRecordFx(unit, !in_rec));
    // Lit only while it would go into a take; included with the effect
    // off or not heard yet, armed (set, waiting), as the effect's own key
    // shows it: lit, it outshone the Reverb key beside an effect that was
    // off (the capture of 2026-11-15).
    let rec_key = if armed {
        // A rim only, in every theme (critique #60 A 6: in light the armed
        // tint made it the brightest thing in an effect that was off).
        // Its rim split-safe (critique #60 B 1: 2.2-2.9 where it fell on a
        // half pixel).
        rec_key.style(theme::split_safe_key(move |t: &iced::Theme, st| button::Style { background: None, ..(theme::key_button_armed(theme::accent(), h / 2.0))(t, st) }, false))
    } else {
        rec_key.style(theme::key_button(in_rec, 0.0, 0.0, h / 2.0))
    };
    let name = if unit == Unit::Reverb { "Reverb" } else { "Echo" };
    let rec_key = crate::widgets::strip::hint_text(
        crate::keyfocus::ring(rec_key.into(), kb(state, CounterItem::FxRec(unit)), h / 2.0),
        match (in_rec, is_on(state, unit), take) {
            // What you hear never changes with it (the owner, #41: "I
            // toggled In Recordings, didn't appear to change anything").
            (false, _, _) => format!("Rec's recordings leave the {name} out; press to record it too. What you hear stays the same"),
            (true, false, _) => format!("Rec's recordings will hold the {name} once it's on; press to leave it out"),
            // On, not in a take yet: why (OpenCode's job 61: "as you hear
            // it" while nothing was heard; critique #64 A 1: say why).
            (true, true, Err(Some(_))) => format!("Rec's recordings will hold the {name} once you hear it; right now {}. Press to leave it out", crate::app::take_fx_why(state).unwrap_or_default()),
            (true, true, Err(None)) | (true, true, Ok(())) => format!("Rec's recordings hold the {name} as you hear it; press to leave it out of them. What you hear stays the same"),
        },
        scale,
    );
    column![
        row![key, kind].spacing(theme::SPACE_MD * scale).align_y(iced::Alignment::Center),
        preset_row(state, unit, scale),
        grid,
        row![iced::widget::Space::new().width(Length::Fill), rec_key],
    ]
    .spacing(theme::SPACE_MD * scale)
    .into()
}

fn cell(state: &TuxMix, p: Param, focused: bool, live: bool, scale: f32) -> Element<'static, Message> {
    let Param { name, tip, value, range, log, words, default, set } = p;
    let reset = set(default);
    let cell: Element<'static, Message> = column![
        knob(Knob {
            readout: Readout::Below,
            // An effect switched off keeps its settings, its arcs muted: lit
            // means it plays (critiques #21 and #22).
            tint: (!live).then(|| iced::Color { a: 0.35, ..theme::muted() }),
            value,
            range,
            label: words,
            arc_from_center: false,
            // The arc grows from the knob's default, so a lit arc means "you
            // changed this" (the owner's go-ahead, 2026-11-10, on the
            // recommendation: knobs with a real rest light only when moved).
            home: Some(default),
            interactive: true,
            log_scale: log,
            modifiers: state.modifiers,
            scale,
            on_change: set,
            on_reset: Box::new(move || reset.clone()),
        }),
        text(name).size(theme::TEXT_SM * scale).color(theme::text_sec()),
    ]
    .spacing(theme::SPACE_TIGHT * scale)
    .align_x(iced::Alignment::Center)
    .width(Length::Fill)
    .into();
    container(crate::widgets::strip::hint(crate::keyfocus::ring(cell, focused, theme::RADIUS_SM * scale), tip, scale))
        .width(Length::FillPortion(1))
        .into()
}

// ── In the strips' panels ─────────────────────────────────────────

/// A knob row in a strip's Settings panel, as Gain's: its words at the
/// left (with why, where it can't be turned), the knob at the right.
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel_knob(
    label: &'static str,
    why: Option<String>,
    db: f32,
    top: f32,
    on_change: Option<Box<dyn Fn(f32) -> Message>>,
    on_reset: Message,
    state: &TuxMix,
    scale: f32,
) -> Element<'static, Message> {
    let interactive = on_change.is_some();
    let mut words = column![text(label)
        .size(theme::TEXT_SM * scale)
        .color(if interactive { theme::text_primary() } else { theme::muted() })];
    if let Some(why) = why {
        words = words.push(text(why).size(theme::TEXT_SM * scale).color(theme::muted()));
    }
    container(
        row![
            container(words).width(Length::Fill),
            knob(Knob {
                readout: Readout::OnFace,
                tint: None,
                value: db,
                range: (OFF_DB, top),
                label: db_words(db),
                arc_from_center: false,
                home: None,
                interactive,
                log_scale: false,
                modifiers: state.modifiers,
                scale,
                on_change: on_change.unwrap_or_else(|| Box::new(move |_| Message::Tick)),
                on_reset: Box::new(move || on_reset.clone()),
            }),
        ]
        .align_y(iced::Alignment::Center)
        .width(Length::Fill),
    )
    .padding([theme::SPACE_TIGHT * scale, (theme::SPACE_SM + 2.0) * scale])
    .into()
}

/// Whether a strip's panel shows the FX rows: where the driver opens the
/// FX, and in the simulation.
pub fn in_panels(state: &TuxMix) -> bool {
    state.fx_available || state.device.is_mock()
}

/// AN1/2's FX send, the one the driver opens, on AN1's or AN2's panel:
/// said shared with the other where the pair is split, plain where it is
/// one strip.
pub fn send_row(state: &TuxMix, cid: ChannelId, scale: f32) -> Element<'static, Message> {
    let an12 = matches!(cid, ChannelId::Input(0 | 1));
    let db = send_db(state);
    let why = send_follow_words(state, cid);
    panel_knob(
        "FX send",
        Some(why),
        db,
        0.0,
        an12.then(|| Box::new(|v: f32| Message::FxSend(if v <= OFF_DB + 0.5 { OFF_DB } else { v })) as Box<dyn Fn(f32) -> Message>),
        Message::FxSend(OFF_DB),
        state,
        scale,
    )
}

/// What AN1/2's FX send follows, in the send row's words; the flow map's
/// line reads them too (critique #73 A 2: it said "AN1's loudest fader"
/// where the send moves with AN1's and AN2's, or with the Main fader).
pub fn send_follow_words(state: &TuxMix, cid: ChannelId) -> String {
    let with = if state.device.input_pair_linked(0) { 0 } else if matches!(cid, ChannelId::Input(1)) { 1 } else { 2 };
    // "Main" names the mix it follows, Main B while Speaker B plays
    // (OpenCode's job 74: "its Main fader" with Main B playing).
    let main = crate::sidebar::mix_name(state, crate::control_room::monitor_pair(state));
    match (with, state.fx_send_follow) {
        // One verb with the pair's line (critique #75 A 7).
        (0, SendFollow::HighestSubmix) => "Moves with its loudest fader".into(),
        (0, SendFollow::Main) => format!("Follows its fader in {main}"),
        (0, SendFollow::Fixed) => "At a fixed level".into(),
        // Said as one send for both (critique #58: "Shared with AN2;
        // follows its loudest fader" was hard to parse).
        (_, SendFollow::HighestSubmix) => "One send for AN1 and AN2; it moves with their loudest fader".into(),
        (_, SendFollow::Main) => format!("One send for AN1 and AN2; it follows their faders in {main}"),
        (_, SendFollow::Fixed) => "One send for AN1 and AN2, at a fixed level".into(),
    }
}

/// What a strip without a send of its own says (an older driver).
pub const NO_SEND_LINE: &str = "No FX send here yet: update the Babyface driver to get one";

/// One channel of each send pair beside AN1/2's, in strip order.
pub const SEND_PAIR_CHANNELS: [ChannelId; 11] = [
    ChannelId::Input(2),
    ChannelId::Input(4),
    ChannelId::Input(6),
    ChannelId::Input(8),
    ChannelId::Input(10),
    ChannelId::Playback(0),
    ChannelId::Playback(2),
    ChannelId::Playback(4),
    ChannelId::Playback(6),
    ChannelId::Playback(8),
    ChannelId::Playback(10),
];

/// The FX send of any source but AN1/2 (driver 2026-11-09): a fixed level,
/// shared with the pair's other channel. The "follows a fader" choice is
/// AN1/2's alone.
pub fn source_send_row(state: &TuxMix, cid: ChannelId, scale: f32) -> Element<'static, Message> {
    let db = state.device.settings().source_fx_send_of(cid);
    let linked = match cid {
        ChannelId::Input(i) => state.device.input_pair_linked(i / 2),
        ChannelId::Playback(i) => state.device.playback_linked(i / 2),
        ChannelId::Output(_) => false,
    };
    // A fixed level, unlike AN1/2's that can follow a fader (critique
    // #18), and the channel it's shared with, named.
    const PB_PARTNER: [&str; 12] = [
        "A fixed level, shared with AN2 PB",
        "A fixed level, shared with AN1 PB",
        "A fixed level, shared with PH4 PB",
        "A fixed level, shared with PH3 PB",
        "A fixed level, shared with AS2 PB",
        "A fixed level, shared with AS1 PB",
        "A fixed level, shared with ADAT4 PB",
        "A fixed level, shared with ADAT3 PB",
        "A fixed level, shared with ADAT6 PB",
        "A fixed level, shared with ADAT5 PB",
        "A fixed level, shared with ADAT8 PB",
        "A fixed level, shared with ADAT7 PB",
    ];
    let why = match cid {
        _ if linked => "A fixed level, for both sides of this pair",
        ChannelId::Playback(i) => PB_PARTNER.get(i).copied().unwrap_or("A fixed level"),
        ChannelId::Input(2) => "A fixed level, shared with IN4",
        ChannelId::Input(3) => "A fixed level, shared with IN3",
        ChannelId::Input(4) => "A fixed level, shared with AS2",
        ChannelId::Input(5) => "A fixed level, shared with AS1",
        ChannelId::Input(6) => "A fixed level, shared with ADAT4",
        ChannelId::Input(7) => "A fixed level, shared with ADAT3",
        ChannelId::Input(8) => "A fixed level, shared with ADAT6",
        ChannelId::Input(9) => "A fixed level, shared with ADAT5",
        ChannelId::Input(10) => "A fixed level, shared with ADAT8",
        _ => "A fixed level, shared with ADAT7",
    };
    panel_knob(
        "FX send",
        Some(why.into()),
        db,
        0.0,
        Some(Box::new(move |v: f32| Message::SourceFxSend(cid, if v <= OFF_DB + 0.5 { OFF_DB } else { v }))),
        Message::SourceFxSend(cid, OFF_DB),
        state,
        scale,
    )
}

/// An output's FX return: how much of Reverb and Echo it plays. A reset
/// turns it off, never up to 0 dB.
pub fn return_row(state: &TuxMix, pair: usize, scale: f32) -> Element<'static, Message> {
    let db = vol_db(user_return(state, pair));
    let muted = state.control_room.fx_muted.iter().flatten().any(|&(p, _)| p == pair);
    panel_knob(
        "FX return",
        Some(if muted { "Held off by Mute FX" } else { "Played in this output" }.into()),
        db,
        RETURN_TOP_DB,
        Some(Box::new(move |v: f32| Message::FxReturn(pair, db_vol(if v <= OFF_DB + 0.5 { OFF_DB } else { v })))),
        Message::FxReturn(pair, 0.0),
        state,
        scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, update};

    fn hw_return(state: &TuxMix, pair: usize) -> f32 {
        state.device.settings().fx_return_of(pair)
    }

    #[test]
    fn under_mute_fx_the_page_names_what_else_is_in_the_way() {
        // Critique #71 A 2: Mute FX on and no send up, the page promised the
        // room back on Release.
        let mut state = new(true, None, None);
        let pair = state.sel_out;
        state.fx.reverb_on = true;
        set_return(&mut state, pair, db_vol(-12.0));
        set_mute(&mut state, true);
        crate::app::settle_glides(&mut state);
        let (line, action, _, _) = status(&state).unwrap();
        assert!(line.replace('\u{a0}', " ").contains("no FX send is up yet either"), "{line}");
        assert!(matches!(action, Some((_, Message::MuteFxToggle))));
        // A send up: Mute FX alone is what holds them.
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), pair, 0.5));
        let _ = update(&mut state, Message::FxSend(-10.0));
        crate::app::settle_glides(&mut state);
        assert!(silenced_by_mute(&state, pair));
        let (line, _, _, _) = status(&state).unwrap();
        assert!(line.starts_with("Mute FX is holding") && !line.contains("either") && !line.contains("too"), "{line}");
    }

    #[test]
    fn the_page_says_what_is_missing_and_its_keys_stop_at_minus_12() {
        let mut state = new(true, None, None);
        // AN1 in a mix at 0 dB, so its send (following its fader) carries.
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, 1.0));
        let (line, action, attention, _) = status(&state).unwrap();
        assert!(line.starts_with("Both off"), "said quietly while both are off: {line}");
        assert!(!attention && action.as_ref().is_some_and(|(label, _)| *label == "Turn on Reverb"), "no snapshot with them yet: the key turns Reverb on");
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, true));
        assert!(state.fx.reverb_on && state.fx_loop.is_none(), "never a loop in the simulation");
        let (line, action, _, _) = status(&state).unwrap();
        assert!(line.contains("Not heard yet"), "{line}");
        assert!(!heard(&state), "on, but its key stays armed");
        // One press: the send and this mix's return, at the Windows levels
        // (the return gliding there).
        let (_, message) = action.unwrap();
        let _ = update(&mut state, message);
        assert!((send_db(&state) - START_SEND_DB).abs() < 1e-4);
        // The return starts from silence and ramps up (the level audit).
        assert!(vol_db(user_return(&state, state.sel_out)) < -50.0, "not at -1.5 dB in one write");
        crate::app::settle_glides(&mut state);
        assert!((vol_db(user_return(&state, state.sel_out)) - START_RETURN_DB).abs() < 0.01);
        assert!(status(&state).unwrap().1.is_none_or(|(label, _)| label == "Hear a sample"), "all set: only the simulation's sample is offered");
        assert!(heard(&state), "now its key lights");
    }

    #[test]
    fn a_return_resets_to_off_never_up() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxReturn(2, db_vol(-20.0)));
        assert!((vol_db(hw_return(&state, 2)) + 20.0).abs() < 0.01);
        let row_reset = Message::FxReturn(2, 0.0);
        let _ = update(&mut state, row_reset);
        assert_eq!(hw_return(&state, 2), 0.0);
    }

    #[test]
    fn mute_fx_holds_main_and_gives_your_level_back() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxOn(Unit::Echo, true));
        let _ = update(&mut state, Message::FxReturn(main, 0.5));
        let _ = update(&mut state, Message::FxReturn(1, 0.25));
        let _ = update(&mut state, Message::MuteFxToggle);
        assert_eq!(hw_return(&state, main), 0.0, "off on Main");
        assert_eq!(hw_return(&state, 1), 0.25, "the Phones keep theirs");
        assert_eq!(user_return(&state, main), 0.5, "the knob shows yours");
        // A turn while muted is kept for later, and Main stays silent.
        let _ = update(&mut state, Message::FxReturn(main, 0.4));
        assert_eq!(hw_return(&state, main), 0.0);
        let _ = update(&mut state, Message::MuteFxToggle);
        // Back from silence gliding, not in one write (OpenCode's job 42).
        assert!(hw_return(&state, main) < 0.1, "not at once: {}", hw_return(&state, main));
        crate::app::settle_glides(&mut state);
        assert!((hw_return(&state, main) - 0.4).abs() < 1e-4, "your level back");
    }

    #[test]
    fn a_crash_while_muted_gives_the_level_back_at_start() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxReturn(main, 0.5));
        let _ = update(&mut state, Message::MuteFxToggle);
        let saved = crate::app::prefs_of(&state);
        assert_eq!(saved.cr_fx_muted, Some(vec![(main, 0.5)]));
        let mut next = new(true, None, None);
        crate::app::apply_prefs(&mut next, saved);
        assert!(next.control_room.fx_muted.is_none());
        crate::app::settle_glides(&mut next);
        assert!((hw_return(&next, main) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a_snapshot_that_raises_a_return_glides_it_up() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxReturn(0, db_vol(-40.0)));
        let quiet = crate::control_room::capture_scene(&state);
        let _ = update(&mut state, Message::FxReturn(0, db_vol(-6.0)));
        let loud = crate::control_room::capture_scene(&state);
        crate::app::apply_mix_scene(&mut state, &quiet).unwrap();
        assert!(state.fx_return_ramps.is_empty(), "down at once");
        crate::app::apply_mix_scene(&mut state, &loud).unwrap();
        assert!((vol_db(hw_return(&state, 0)) + 40.0).abs() < 0.01, "not at once");
        let now = Instant::now();
        crate::app::tick_through(&mut state, step_return_ramps, now, now + crate::app::RAMP_FOR);
        assert!((vol_db(hw_return(&state, 0)) + 6.0).abs() < 0.01, "after the glide");
        assert!(state.fx_return_ramps.is_empty());
    }

    #[test]
    fn a_snapshot_stored_while_muted_keeps_your_level() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxReturn(main, 0.5));
        let _ = update(&mut state, Message::MuteFxToggle);
        let scene = crate::control_room::capture_scene(&state);
        assert_eq!(scene.settings.fx_return_of(main), 0.5, "yours, not the muted 0");
        let _ = update(&mut state, Message::MuteFxToggle);
        let _ = update(&mut state, Message::FxReturn(main, 0.0));
        crate::app::apply_mix_scene(&mut state, &scene).unwrap();
        let now = Instant::now();
        crate::app::tick_through(&mut state, step_return_ramps, now, now + crate::app::RAMP_FOR);
        assert_eq!(hw_return(&state, main), 0.5);
    }

    #[test]
    fn a_snapshot_from_before_the_fx_leaves_the_returns_alone() {
        let mut state = new(true, None, None);
        let mut old = crate::control_room::capture_scene(&state);
        old.settings.fx_return.clear();
        let _ = update(&mut state, Message::FxReturn(1, 0.5));
        crate::app::apply_mix_scene(&mut state, &old).unwrap();
        assert_eq!(hw_return(&state, 1), 0.5);
    }

    #[test]
    fn turning_a_return_knob_is_one_undo_step() {
        let mut state = new(true, None, None);
        let before = state.undo_stack.len();
        for i in 1..=20 {
            let _ = update(&mut state, Message::FxReturn(0, i as f32 / 40.0));
        }
        assert_eq!(state.undo_stack.len(), before + 1);
        let _ = update(&mut state, Message::Undo);
        assert_eq!(hw_return(&state, 0), 0.0, "the whole turn undone");
    }

    #[test]
    fn a_factory_preset_loads_names_itself_and_leaves_the_switch_alone() {
        let mut state = new(true, None, None);
        assert_eq!(current_preset(&state, Unit::Reverb).map(|(_, n)| n).as_deref(), Some("Default"));
        let _ = update(&mut state, Message::FxPresetLoad(Unit::Reverb, PresetPick::Factory(1)));
        assert!(!state.fx.reverb_on, "loading doesn't switch it on");
        assert_eq!(state.fx.reverb, audra_fx::presets::reverbs()[1].1);
        assert_eq!(current_preset(&state, Unit::Reverb).map(|(_, n)| n).as_deref(), Some("Close vocal"));
        // A turn of a knob: no preset any more.
        let p = state.fx.reverb;
        let _ = update(&mut state, Message::FxReverb(ReverbParams { smooth: 12.0, ..p }));
        assert!(current_preset(&state, Unit::Reverb).is_none());
    }

    #[test]
    fn saving_keeps_yours_and_the_same_name_replaces_it() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxPresetLoad(Unit::Echo, PresetPick::Factory(0)));
        let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Echo));
        assert_eq!(state.fx_naming.as_ref().map(|(_, n)| n.as_str()), Some("My slapback"));
        let _ = update(&mut state, Message::FxPresetName("Vox slap".into()));
        let _ = update(&mut state, Message::FxPresetSave);
        assert_eq!(state.fx_presets.echo.len(), 1);
        assert!(state.fx_naming.is_none());
        assert_eq!(current_preset(&state, Unit::Echo), Some((PresetPick::User(0), "Vox slap".into())));
        // Turned, then saved under the same name (any case): replaced.
        let p = state.fx.echo;
        let _ = update(&mut state, Message::FxEcho(EchoParams { feedback: 20.0, ..p }));
        let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Echo));
        let _ = update(&mut state, Message::FxPresetName("vox SLAP".into()));
        let _ = update(&mut state, Message::FxPresetSave);
        assert_eq!(state.fx_presets.echo.len(), 1);
        assert_eq!(state.fx_presets.echo[0].params.feedback, 20.0);
        // Esc gives up a name being typed.
        let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Reverb));
        let _ = update(&mut state, Message::EscapePressed);
        assert!(state.fx_naming.is_none());
        assert!(state.fx_presets.reverb.is_empty());
    }

    #[test]
    fn removing_one_of_yours_takes_two_choices() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        assert_eq!(state.fx_presets.reverb.len(), 1, "an empty name still saves");
        let _ = update(&mut state, Message::FxPresetRemove(Unit::Reverb, 0));
        assert_eq!(state.fx_presets.reverb.len(), 1, "the first choice only asks");
        let _ = update(&mut state, Message::FxPresetRemove(Unit::Reverb, 0));
        assert!(state.fx_presets.reverb.is_empty());
    }

    #[test]
    fn the_keyboard_walks_the_page_flips_presets_and_turns_knobs() {
        use crate::keyfocus::{self, CounterItem, KeyFocus};
        let mut state = new(true, None, None);
        state.sidebar_tab = Some(crate::sidebar::Tab::Snapshots);
        assert!(!keyfocus::counter_items(&state).contains(&CounterItem::FxOn(Unit::Reverb)), "only while the page is open");
        // The pages float at every height: the page is open as its card.
        state.sidebar_open = true;
        state.page_card = true;
        state.sidebar_tab = Some(crate::sidebar::Tab::Fx);
        let items = keyfocus::counter_items(&state);
        let n = knob_count(&state, Unit::Reverb) as u8;
        assert!(items.contains(&CounterItem::FxKnob(Unit::Reverb, n - 1)) && !items.contains(&CounterItem::FxKnob(Unit::Reverb, n)));
        // The arrows on the preset load the next one at once.
        state.key_focus = Some(KeyFocus::Counter(CounterItem::FxPreset(Unit::Reverb)));
        let m = keyfocus::left_right(&mut state, 1).flatten().unwrap();
        let _ = update(&mut state, m);
        assert_eq!(current_preset(&state, Unit::Reverb).map(|(_, n)| n).as_deref(), Some("Just a touch"));
        let m = keyfocus::left_right(&mut state, 1).flatten().unwrap();
        let _ = update(&mut state, m);
        assert_eq!(current_preset(&state, Unit::Reverb).map(|(_, n)| n).as_deref(), Some("Close vocal"));
        // A knob turns a step; the focus stays on it.
        let before = state.fx.reverb.pre_delay_ms;
        state.key_focus = Some(KeyFocus::Counter(CounterItem::FxKnob(Unit::Reverb, 0)));
        let m = keyfocus::adjust(&mut state, 1).unwrap();
        let _ = update(&mut state, m);
        assert!(state.fx.reverb.pre_delay_ms > before);
        assert_eq!(state.key_focus, Some(KeyFocus::Counter(CounterItem::FxKnob(Unit::Reverb, 0))));
        // Space on the On key switches the effect.
        state.key_focus = Some(KeyFocus::Counter(CounterItem::FxOn(Unit::Echo)));
        let m = keyfocus::press(&mut state).flatten().unwrap();
        let _ = update(&mut state, m);
        assert!(state.fx.echo_on);
    }

    #[test]
    fn auditioning_presets_never_loses_hand_made_settings() {
        let mut state = new(true, None, None);
        let mine = ReverbParams { smooth: 33.0, pre_delay_ms: 77.0, ..ReverbParams::default() };
        let _ = update(&mut state, Message::FxReverb(mine));
        assert!(preset_label(&state, Unit::Reverb).is_none(), "made by hand: no preset");
        // Flip forward twice, then back twice: the hand-made settings return.
        for dir in [1, 1, -1, -1] {
            let m = preset_step(&state, Unit::Reverb, dir).unwrap();
            let _ = update(&mut state, m);
        }
        assert_eq!(state.fx.reverb, mine);
        assert_eq!(preset_label(&state, Unit::Reverb).map(|(_, l)| l).as_deref(), Some("Preset: Your last settings"));
    }

    #[test]
    fn a_knob_turn_keeps_where_the_settings_came_from() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxPresetLoad(Unit::Echo, PresetPick::Factory(2)));
        let p = state.fx.echo;
        let _ = update(&mut state, Message::FxEcho(EchoParams { feedback: 12.0, ..p }));
        assert_eq!(preset_label(&state, Unit::Echo), Some((PresetPick::Factory(2), "Preset: Ping-pong, changed".into())));
        // The next preset is the one after Ping-pong, and the change is kept.
        let m = preset_step(&state, Unit::Echo, 1).unwrap();
        let _ = update(&mut state, m);
        assert_eq!(current_preset(&state, Unit::Echo).map(|(_, n)| n).as_deref(), Some("Dotted eighth at 120"));
        assert_eq!(state.fx_memory.echo_kept.map(|k| k.feedback), Some(12.0));
        // And a save starts from the preset it came from.
        let p = state.fx.echo;
        let _ = update(&mut state, Message::FxEcho(EchoParams { feedback: 13.0, ..p }));
        let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Echo));
        assert_eq!(state.fx_naming.as_ref().map(|(_, n)| n.as_str()), Some("My dotted eighth at 120"));
    }

    #[test]
    fn an_old_snapshot_while_muted_keeps_the_held_level() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxReturn(main, 0.25));
        let _ = update(&mut state, Message::MuteFxToggle);
        let mut old = crate::control_room::capture_scene(&state);
        old.settings.fx_return.clear();
        crate::app::apply_mix_scene(&mut state, &old).unwrap();
        assert_eq!(hw_return(&state, main), 0.0, "still held off");
        let _ = update(&mut state, Message::MuteFxToggle);
        crate::app::settle_glides(&mut state);
        assert!((hw_return(&state, main) - 0.25).abs() < 1e-4, "and given back");
    }

    #[test]
    fn the_unit_never_sees_a_muted_or_raised_return_from_a_scene() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxReturn(main, 0.5));
        let _ = update(&mut state, Message::FxReturn(3, db_vol(-40.0)));
        let mut scene = crate::control_room::capture_scene(&state);
        scene.settings.keep_fx_return(main, 2.0);
        scene.settings.keep_fx_return(3, 1.0);
        let _ = update(&mut state, Message::MuteFxToggle);
        let (sent, _) = prepare_scene(&state, &scene);
        assert_eq!(sent.settings.fx_return_of(main), 0.0, "Mute FX's pair stays at 0");
        assert!((vol_db(sent.settings.fx_return_of(3)) + 40.0).abs() < 0.01, "a raise starts where it is");
        crate::app::apply_mix_scene(&mut state, &scene).unwrap();
        assert_eq!(user_return(&state, main), 2.0, "the scene's level is the one kept");
    }

    #[test]
    fn a_new_main_lets_mute_fx_go() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxReturn(main, 0.5));
        let _ = update(&mut state, Message::MuteFxToggle);
        let assign = state.control_room.assign.with_main(2);
        let _ = update(&mut state, Message::SetAssign(assign));
        assert!(state.control_room.fx_muted.is_none());
        crate::app::settle_glides(&mut state);
        assert!((hw_return(&state, main) - 0.5).abs() < 1e-4, "the old Main has its return back");
    }

    #[test]
    fn a_glide_is_kept_at_its_end_by_mute_and_by_a_snapshot() {
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        let _ = update(&mut state, Message::FxReturn(main, db_vol(-50.0)));
        let mut loud = crate::control_room::capture_scene(&state);
        loud.settings.keep_fx_return(main, 1.0);
        crate::app::apply_mix_scene(&mut state, &loud).unwrap();
        // Half-way through the glide.
        let (_, _, since) = state.fx_return_ramps[&main];
        step_return_ramps(&mut state, since + crate::app::RAMP_FOR / 2);
        assert!(hw_return(&state, main) < 0.5);
        let stored = crate::control_room::capture_scene(&state);
        assert_eq!(stored.settings.fx_return_of(main), 1.0, "a snapshot keeps where it was going");
        let _ = update(&mut state, Message::MuteFxToggle);
        let _ = update(&mut state, Message::MuteFxToggle);
        crate::app::settle_glides(&mut state);
        assert!((hw_return(&state, main) - 1.0).abs() < 1e-4, "so does Mute FX");
    }

    #[test]
    fn a_level_the_unit_refuses_stays_kept() {
        let mut state = new(true, None, None);
        // A pair the simulation doesn't have: its write fails.
        recover_mute(&mut state, Some(vec![(9, 0.5)]));
        assert_eq!(state.control_room.fx_muted, Some(vec![(9, 0.5)]), "kept, not lost");
    }

    #[test]
    fn an_untitled_save_never_replaces_one() {
        let mut state = new(true, None, None);
        for _ in 0..2 {
            let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Reverb));
            let _ = update(&mut state, Message::FxPresetName(String::new()));
            let _ = update(&mut state, Message::FxPresetSave);
        }
        // Remove "Reverb 1"; the next untitled one must not be "Reverb 2".
        let _ = update(&mut state, Message::FxPresetRemove(Unit::Reverb, 0));
        let _ = update(&mut state, Message::FxPresetRemove(Unit::Reverb, 0));
        let _ = update(&mut state, Message::FxPresetSaveStart(Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetName(String::new()));
        let _ = update(&mut state, Message::FxPresetSave);
        let names: Vec<String> = state.fx_presets.reverb.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, vec!["Reverb 2".to_string(), "Reverb 3".to_string()]);
    }

    #[test]
    fn the_send_follows_the_highest_fader_or_main_or_nothing() {
        let mut state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        let mock = |state: &TuxMix| match &state.device {
            crate::app::DeviceHandle::Mock(d) => d.fx_send_on_unit(),
            _ => unreachable!(),
        };
        let _ = update(&mut state, Message::FxSend(-10.0));
        // AN1/2 at −6 dB in its loudest mix, lower elsewhere: the unit gets
        // the knob plus −6.
        for pair in 0..state.device.output_pair_count() {
            for i in 0..2 {
                let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(i), pair, 0.0));
            }
        }
        let _ = update(&mut state, Message::VolumeChanged(an1, 1, db_vol(-6.0)));
        let _ = update(&mut state, Message::VolumeChanged(an1, 0, db_vol(-20.0)));
        sync_send_follow(&mut state);
        assert!((mock(&state) + 16.0).abs() < 0.05, "knob −10 + highest −6: {}", mock(&state));
        assert_eq!(send_db(&state), -10.0, "the knob stays the knob");
        // Pulling the fader down takes the send down too; all down is off.
        let _ = update(&mut state, Message::VolumeChanged(an1, 1, db_vol(-30.0)));
        sync_send_follow(&mut state);
        assert!((mock(&state) + 30.0).abs() < 0.05, "now the Main mix's −20 is the highest");
        let _ = update(&mut state, Message::Mute(an1, true));
        sync_send_follow(&mut state);
        assert_eq!(mock(&state), OFF_DB, "muted: nothing sent");
        assert!(status(&state).is_none() || !state.fx.any_on());
        // Following the Main mix instead.
        let _ = update(&mut state, Message::Mute(an1, false));
        let _ = update(&mut state, Message::SetFxSendFollow(SendFollow::Main));
        let main = crate::control_room::monitor_pair(&state);
        let _ = update(&mut state, Message::VolumeChanged(an1, main, db_vol(-3.0)));
        sync_send_follow(&mut state);
        assert!((mock(&state) + 13.0).abs() < 0.05, "knob −10 + Main −3: {}", mock(&state));
        // A fixed send ignores the faders.
        let _ = update(&mut state, Message::SetFxSendFollow(SendFollow::Fixed));
        assert!((mock(&state) + 10.0).abs() < 0.05);
    }

    #[test]
    fn the_page_says_when_the_faders_hold_the_send() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, true));
        let _ = update(&mut state, Message::FxSend(-10.0));
        let pair = state.sel_out;
        let _ = update(&mut state, Message::FxReturn(pair, 1.0));
        for pair in 0..state.device.output_pair_count() {
            for i in 0..2 {
                let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(i), pair, 0.0));
            }
        }
        let (line, _, _, _) = status(&state).unwrap();
        assert!(line.contains("down in every mix"), "{line}");
    }

    #[test]
    fn the_send_knob_survives_a_restart_under_the_follow() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxSend(-10.0));
        let saved = crate::app::prefs_of(&state);
        assert_eq!(saved.fx_send_db, Some(-10.0));
        let mut next = new(true, None, None);
        crate::app::apply_prefs(&mut next, saved);
        assert_eq!(send_db(&next), -10.0);
    }

    #[test]
    fn every_source_pair_has_its_own_send_shared_by_its_two_channels() {
        let mut state = new(true, None, None);
        assert!(state.device.has_source_fx_send(ChannelId::Input(2)));
        assert!(state.device.has_source_fx_send(ChannelId::Playback(11)));
        assert!(!state.device.has_source_fx_send(ChannelId::Input(0)), "AN1/2 keeps its own");
        let _ = update(&mut state, Message::SourceFxSend(ChannelId::Input(3), -12.0));
        let s = state.device.settings();
        assert_eq!(s.source_fx_send_of(ChannelId::Input(2)), -12.0, "IN3 shares IN4's");
        assert_eq!(s.source_fx_send_of(ChannelId::Input(4)), OFF_DB, "AS1/2 untouched");
        assert_eq!(send_db(&state), OFF_DB, "AN1/2 untouched");
        // One undo step takes it back.
        let _ = update(&mut state, Message::Undo);
        assert_eq!(state.device.settings().source_fx_send_of(ChannelId::Input(3)), OFF_DB);
    }

    #[test]
    fn the_source_sends_survive_a_restart() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SourceFxSend(ChannelId::Playback(1), -18.0));
        let saved = crate::app::prefs_of(&state);
        assert_eq!(saved.source_fx_send.get("PB1"), Some(&-18.0));
        let mut next = new(true, None, None);
        crate::app::apply_prefs(&mut next, saved);
        assert_eq!(next.device.settings().source_fx_send_of(ChannelId::Playback(0)), -18.0);
    }

    #[test]
    fn the_panel_shows_the_send_and_the_keys_step_it() {
        let mut state = new(true, None, None);
        let pb = ChannelId::Playback(0);
        let _ = update(&mut state, Message::SourceFxSend(pb, -20.0));
        let _ = source_send_row(&state, pb, 1.0);
        let _ = source_send_row(&state, ChannelId::Input(7), 1.0);
        assert!(NO_SEND_LINE.contains("driver"));
        for cid in SEND_PAIR_CHANNELS {
            assert!(tuxmix_core::device::source_send_pair(cid).is_some(), "{cid:?}");
        }
    }

    #[test]
    fn on_a_short_window_the_page_is_a_card_that_esc_closes() {
        let mut state = new(true, None, None);
        state.window_size = Some((1600.0, 900.0));
        state.sidebar_tab = None;
        let _ = update(&mut state, Message::SidebarTab(crate::sidebar::Tab::Fx));
        assert!(crate::sidebar::pages_float(&state));
        assert_eq!(crate::sidebar::page_card_open(&state), Some(crate::sidebar::Tab::Fx));
        let _ = crate::app::view(&state, iced::window::Id::unique());
        let _ = update(&mut state, Message::EscapePressed);
        assert_ne!(state.sidebar_tab, Some(crate::sidebar::Tab::Fx));
        // A tall window floats it too (the owner's "chips everywhere").
        state.window_size = Some((1600.0, 1200.0));
        assert!(crate::sidebar::pages_float(&state));
    }

    #[test]
    fn the_keys_flare_the_moment_the_effects_are_first_heard() {
        let mut state = new(true, None, None);
        state.animations = true;
        watch_heard(&mut state);
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, true));
        watch_heard(&mut state);
        assert!(state.fx_heard_at.is_none(), "on but silent: no flare");
        let pair = state.sel_out;
        // AN1 up in this mix (the send follows its faders), then the send
        // and the return in one press.
        let _ = update(&mut state, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), pair, 1.0));
        let _ = update(&mut state, Message::FxMakeHeard(pair));
        watch_heard(&mut state);
        assert!(heard(&state) && state.fx_heard_at.is_some(), "heard: the keys flare");
        assert!(crate::motion::heard_flare(&state) > 0.0);
        let at = state.fx_heard_at;
        watch_heard(&mut state);
        assert_eq!(state.fx_heard_at, at, "once, not on every tick");
    }

    #[test]
    fn no_flare_at_start_on_another_mix_or_without_the_users_hand() {
        let mut state = new(true, None, None);
        state.animations = true;
        let pair = state.sel_out;
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, true));
        let _ = update(&mut state, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), pair, 1.0));
        let _ = update(&mut state, Message::FxMakeHeard(pair));
        // Already heard at the first look: noted, not flared.
        watch_heard(&mut state);
        assert!(state.fx_heard_at.is_none(), "not at start");
        // Long untouched, then heard again (the loop coming back up).
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, false));
        watch_heard(&mut state);
        state.fx_touched_at = std::time::Instant::now().checked_sub(TOUCHED_WITHIN * 2);
        state.fx.reverb_on = true;
        watch_heard(&mut state);
        assert!(state.fx_heard_at.is_none(), "not without the user's hand");
    }

    #[test]
    fn make_it_heard_is_one_press_and_one_undo() {
        let mut state = new(true, None, None);
        let pair = state.sel_out;
        let before = state.undo_stack.len();
        let _ = update(&mut state, Message::FxMakeHeard(pair));
        assert_eq!(state.undo_stack.len(), before + 1);
        assert!(send_db(&state) > OFF_DB && hw_return(&state, pair) > 0.0);
        let _ = update(&mut state, Message::Undo);
        assert_eq!(send_db(&state), OFF_DB);
        assert_eq!(hw_return(&state, pair), 0.0);
    }

    #[test]
    fn the_in_recordings_key_never_disagrees_with_the_rec_key() {
        // Critique #64 A 1: lit "as you hear it" while Rec said the take
        // would be dry (the simulation runs no FX loop).
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, true));
        assert!(rec_includes(&state, Unit::Reverb));
        assert!(crate::app::take_fx(&state).is_err());
        assert!(rec_key_armed(&state, Unit::Reverb), "armed while Rec says dry");
    }

    #[test]
    fn make_it_heard_raises_the_return_over_the_ramp_never_at_once() {
        // OpenCode's job 64 (b): each write of the return, tick by tick, a
        // late tick among them, rises no more than 6 dB on the one before.
        let mut state = new(true, None, None);
        let pair = state.sel_out;
        let _ = update(&mut state, Message::FxMakeHeard(pair));
        let (_, to_db, since) = *state.fx_return_ramps.get(&pair).expect("a ramp, not a jump");
        let mut last = hw_return(&state, pair);
        assert!(last <= db_vol(-59.0), "starts near silence: {last}");
        let mut now = since;
        for tick in 0..400 {
            // One tick a second late (a stall): still one regular step.
            now += std::time::Duration::from_millis(if tick == 3 { 1_000 } else { 20 });
            step_return_ramps(&mut state, now);
            let v = hw_return(&state, pair);
            assert!(20.0 * (v / last).log10() <= 6.0, "a step of {} dB at tick {tick}", 20.0 * (v / last).log10());
            last = v;
            if !state.fx_return_ramps.contains_key(&pair) {
                break;
            }
        }
        assert!(!state.fx_return_ramps.contains_key(&pair), "the ramp ends");
        assert!((last - db_vol(to_db)).abs() < 1e-4, "at its level");
    }

    #[test]
    fn a_log_knob_steps_along_its_sweep() {
        let state = new(true, None, None);
        // Echo's Delay (1–2000 ms, log): a step up from 375 ms is a few percent.
        let Some(Message::FxEcho(p)) = knob_nudge(&state, Unit::Echo, 0, 1) else { panic!() };
        assert!(p.delay_ms > 375.0 && p.delay_ms < 450.0, "{}", p.delay_ms);
    }

    #[test]
    fn the_lock_holds_the_send_and_returns_not_the_effects() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetUiLock(true));
        let _ = update(&mut state, Message::FxReturn(0, 1.0));
        let _ = update(&mut state, Message::FxSend(-6.0));
        assert_eq!(hw_return(&state, 0), 0.0);
        assert_eq!(send_db(&state), OFF_DB);
        let _ = update(&mut state, Message::FxOn(Unit::Reverb, true));
        assert!(state.fx.reverb_on, "the effect itself is not the mix");
    }

    #[test]
    fn clearing_effects_turns_the_fx_down() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::FxSend(-10.0));
        let _ = update(&mut state, Message::FxReturn(1, 1.0));
        crate::resetmix::apply(&mut state, crate::resetmix::ResetMix::ClearEffects);
        assert_eq!(send_db(&state), OFF_DB);
        assert_eq!(hw_return(&state, 1), 0.0);
    }

    #[test]
    fn the_knob_rows_stay_even() {
        assert_eq!(row_sizes(5), vec![3, 2]);
        assert_eq!(row_sizes(7), vec![4, 3]);
        assert_eq!(row_sizes(8), vec![4, 4]);
        assert_eq!(row_sizes(10), vec![4, 3, 3]);
        assert_eq!(row_sizes(4), vec![4]);
    }

    #[test]
    fn db_and_the_crosspoint_scale_agree_and_off_is_off() {
        assert_eq!(db_vol(OFF_DB), 0.0);
        assert_eq!(vol_db(0.0), OFF_DB);
        assert!((db_vol(0.0) - 1.0).abs() < 1e-6);
        assert!((vol_db(db_vol(START_RETURN_DB)) - START_RETURN_DB).abs() < 1e-4);
        assert!(db_vol(20.0) <= 2.0, "never past the crosspoints' top");
    }
}
