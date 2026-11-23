//! Reset Mix (manual 21.10, "Offers several options to reset the mixer
//! state"). TotalMix keeps these in its Options menu, two clicks deep; here
//! they sit at the foot of Audra settings, and each one asks for a second
//! click within a few seconds (`ARM_FOR`) so a stray click mid-session can't
//! silence the monitoring. Every reset but the names is one undo step.
//!
//! Audra's readings where the manual is short:
//! - **Straight playback** rewrites the playback channels' routing only
//!   (playback 1/2 to output 1/2 and so on, at 0 dB); the inputs' routing
//!   is left alone, as "Clear all submixes" is the reset for that. The
//!   third row (output volumes) is not changed, as the manual says.
//! - **Total Reset** is straight playback with everything also on Main,
//!   every other routing cleared, channel effects off, every mute, solo,
//!   phase invert and loopback off, and every Control Room override
//!   released. It leaves the output volumes alone: a reset must never turn
//!   anything up.
//! - **Set inputs mono / outputs stereo (ADM)** is left out: ASIO Direct
//!   Monitoring is Windows-only.

use std::time::Duration;

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::TuxMix;

/// How long a reset stays armed after its first click.
pub const ARM_FOR: Duration = Duration::from_secs(4);

/// The manual's Reset Mix entries, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMix {
    StraightPlaybackToMain,
    StraightPlayback,
    ClearSubmixes,
    ClearEffects,
    OutputVolumes,
    ResetNames,
    AllMono,
    AllStereo,
    TotalReset,
    /// Audra's own, above TotalMix's nine (the owner's pick, 2026-11-21):
    /// a total reset and safe output volumes in one, for "start over".
    Fresh,
}

impl ResetMix {
    pub const ALL: [ResetMix; 9] = [
        ResetMix::StraightPlaybackToMain,
        ResetMix::StraightPlayback,
        ResetMix::ClearSubmixes,
        ResetMix::ClearEffects,
        ResetMix::OutputVolumes,
        ResetMix::ResetNames,
        ResetMix::AllMono,
        ResetMix::AllStereo,
        ResetMix::TotalReset,
    ];

    /// The button's label.
    pub fn label(self) -> &'static str {
        match self {
            ResetMix::StraightPlaybackToMain => "Straight playback, all to Main",
            ResetMix::StraightPlayback => "Straight playback",
            ResetMix::ClearSubmixes => "Clear all submixes",
            ResetMix::ClearEffects => "Clear channel effects",
            ResetMix::OutputVolumes => "Set output volumes",
            ResetMix::ResetNames => "Reset channel names",
            ResetMix::AllMono => "All channels mono",
            ResetMix::AllStereo => "All channels stereo",
            ResetMix::TotalReset => "Total reset",
            ResetMix::Fresh => "Start fresh",
        }
    }

    /// What it does, for the tooltip.
    pub fn detail(self) -> &'static str {
        match self {
            ResetMix::StraightPlaybackToMain => {
                "Computer audio plays out of each output at full level, and all of it in your speakers too. Output faders stay."
            }
            ResetMix::StraightPlayback => "Computer audio plays out of each output at full level. Output faders stay.",
            ResetMix::ClearSubmixes => "Turns every fader in every mix all the way down. Nothing plays until you raise them.",
            ResetMix::ClearEffects => "EQ and low cut off, their knobs and Width back to default. Reverb and Echo's send and returns off.",
            ResetMix::OutputVolumes => "Every output fader to 0 dB, Main and Main B to \u{2212}10 dB. What you listen on (your speakers and headphones) only ever comes down.",
            ResetMix::ResetNames => "Removes every name you gave a channel. Can't be undone.",
            ResetMix::AllMono => "Splits every stereo pair into two mono strips.",
            ResetMix::AllStereo => "Links every pair into one stereo strip.",
            ResetMix::TotalReset => {
                "Starts the mixer over: computer audio straight to each output and your speakers, mics and inputs silent, effects, mutes and loopback off. Output faders stay."
            }
            ResetMix::Fresh => {
                "A clean, safe mix: computer audio in your speakers and headphones, mics silent until you set them up again in Quick setup, effects and mutes off, your speakers down 10 dB and your headphones no louder than you had them. Your snapshots, names and My setup stay; Undo (Ctrl+Z) brings this mix back."
            }
        }
    }

    /// The notice once it is done.
    fn done(self) -> &'static str {
        match self {
            ResetMix::ResetNames => "Channel names reset.",
            ResetMix::StraightPlaybackToMain | ResetMix::StraightPlayback => {
                "Playback routed straight. Undo (Ctrl+Z) brings the old routing back."
            }
            ResetMix::ClearSubmixes => "Every submix cleared. Undo (Ctrl+Z) brings them back.",
            ResetMix::ClearEffects => "Channel effects cleared. Undo (Ctrl+Z) brings them back.",
            ResetMix::OutputVolumes => "Output volumes set. Undo (Ctrl+Z) brings them back.",
            ResetMix::AllMono => "Every pair split to mono. Undo (Ctrl+Z) links them again.",
            ResetMix::AllStereo => "Every pair linked. Undo (Ctrl+Z) splits them again.",
            ResetMix::TotalReset => "Mixer reset. Undo (Ctrl+Z) brings the mix back.",
            ResetMix::Fresh => "A fresh, safe mix. Quick setup puts your mic back in; Undo (Ctrl+Z) brings your old mix back.",
        }
    }

    /// Whether it changes the mix (and so is one undo step, and held by
    /// the lock). Only the names are not the mix.
    pub fn changes_mix(self) -> bool {
        self != ResetMix::ResetNames
    }
}

/// Every source channel: inputs, then playbacks.
fn sources(state: &TuxMix) -> Vec<ChannelId> {
    (0..state.device.inputs().len())
        .map(ChannelId::Input)
        .chain((0..state.device.playbacks().len()).map(ChannelId::Playback))
        .collect()
}

/// Applies `kind` and says so in the notice.
pub fn apply(state: &mut TuxMix, kind: ResetMix) {
    // A fader still gliding up would carry on past the reset.
    if kind.changes_mix() {
        state.fader_glides.clear();
    }
    match kind {
        ResetMix::StraightPlaybackToMain => straight_playback(state, true),
        ResetMix::StraightPlayback => straight_playback(state, false),
        ResetMix::ClearSubmixes => clear_submixes(state),
        ResetMix::ClearEffects => clear_effects(state),
        ResetMix::OutputVolumes => output_volumes(state),
        ResetMix::ResetNames => {
            state.labels.clear();
            state.prefs_dirty = true;
        }
        ResetMix::AllMono => set_all_linked(state, false),
        ResetMix::AllStereo => set_all_linked(state, true),
        ResetMix::TotalReset => total_reset(state),
        ResetMix::Fresh => {
            total_reset(state);
            output_volumes(state);
        }
    }
    state.reset_armed = None;
    // An Undo key on the notice, as every other change Undo takes back
    // (critique #100 A P3-4: Start fresh said "Undo (Ctrl+Z)" in words only).
    if kind.changes_mix() {
        crate::app::say_undoable(state, kind.done().into(), "Bring the mix back as it was (Ctrl+Z)".to_string());
    } else {
        state.notice = Some((kind.done().into(), std::time::Instant::now()));
    }
}

/// Playback channel `c` into output pair `p`: 0 dB where it goes, off
/// elsewhere, and a linked pair's balance back in the centre (a stereo
/// playback pair has a balance, one side lowered: straight is level).
fn straight_playback(state: &mut TuxMix, to_main: bool) {
    state.mirrors.clear();
    let main = state.control_room.assign.main;
    let pairs = state.device.output_pair_count();
    for c in 0..state.device.playbacks().len() {
        for p in 0..pairs {
            let on = p == c / 2 || (to_main && p == main);
            if on {
                // Up the gentle way: a rise of more than RAMP_OVER_DB
                // glides, as a click's does (OpenCode's job 9: up to 60 dB
                // in one step), without stopping the channel's glide into
                // its other mix, that channel alone (`level_gently_keeping_glides`).
                crate::app::level_gently_keeping_glides(state, ChannelId::Playback(c), p, 1.0, crate::app::GlideVia::OneChannel);
            } else {
                let _ = state.device.set_volume(ChannelId::Playback(c), p, 0.0);
            }
        }
    }
    for c in (0..state.device.playbacks().len()).step_by(2) {
        for p in 0..pairs {
            if state.device.playback_balance(c, p) != 0 {
                let _ = crate::app::write_send(state, p, |d| d.set_pan(ChannelId::Playback(c), p, 0));
            }
        }
    }
    crate::control_room::after_mix_change(state, None);
}

fn clear_submixes(state: &mut TuxMix) {
    state.mirrors.clear();
    let pairs = state.device.output_pair_count();
    for c in sources(state) {
        for p in 0..pairs {
            let _ = state.device.set_volume(c, p, 0.0);
        }
    }
    crate::control_room::after_mix_change(state, None);
}

/// EQ and low cut off with their knobs at default, every stereo pair's
/// Width back to +1.00 (as recorded), Reverb and Echo's send and returns
/// off. Needs RME's own protocol;
/// on a class-compliant connection there is nothing to clear.
fn clear_effects(state: &mut TuxMix) {
    if !state.device.protocol_features() {
        return;
    }
    let default = tuxmix_core::channel::InputEq::default();
    let with_eq: Vec<usize> = (0..state.device.inputs().len())
        .filter(|&i| state.device.inputs()[i].eq.is_some())
        .collect();
    for i in with_eq {
        let _ = state.device.set_eq_enabled(i, false);
        for (band, b) in default.bands.iter().enumerate() {
            let _ = state.device.set_eq_band_freq(i, band, b.freq_hz);
            let _ = state.device.set_eq_band_q(i, band, b.q);
            let _ = state.device.set_eq_band_gain(i, band, b.gain_db);
        }
        let _ = state.device.set_eq_low_cut_freq(i, 0);
        let _ = state.device.set_eq_low_cut_slope(i, default.low_cut_slope_db_oct);
        state.eq_low_cut_memory.remove(&i);
    }
    // Every pair's width back to as recorded (#7).
    let pairs = (0..state.device.inputs().len())
        .step_by(2)
        .map(ChannelId::Input)
        .chain((0..state.device.playbacks().len()).step_by(2).map(ChannelId::Playback))
        .collect::<Vec<_>>();
    for cid in pairs {
        if state.device.settings().source_width_of(cid) < 1.0 {
            let _ = state.device.set_source_width(cid, 1.0);
        }
    }
    // The FX send and every return off: a reset only ever turns down.
    crate::fx::all_off(state);
}

/// Every output fader to 0 dB; Main and Main B (Speaker B) to -10 dB, as
/// TotalMix sets them, but what is listened on (Main, Main B, the Phones)
/// only ever comes down (2026-11-04, the critique: Phones at -30 dB jumped
/// to 0 dB in the ears, past this module's own "a reset must never turn
/// anything up"). A dimmed Main takes the new level as the user's own and
/// stays dimmed.
fn output_volumes(state: &mut TuxMix) {
    let assign = state.control_room.assign;
    let ten_down = 10f32.powf(-10.0 / 20.0);
    for ch in 0..state.device.outputs().len() {
        let pair = crate::mixops::output_pair(state, ch);
        let main = pair == assign.main || Some(pair) == assign.main_b;
        let listened = crate::control_room::listened_on(state, ch);
        let target = if main { ten_down } else { 1.0 };
        let level = if listened {
            target.min(crate::control_room::user_output_level(state, ch))
        } else {
            target
        };
        let v = crate::control_room::compose_output_write(state, ch, level);
        // Up more than 6 dB on an output not listened on (a recording or a
        // second pair of speakers) glides, as every other rise does (the
        // local Qwen's review: straight to 0 dB in one write).
        let was = state.device.outputs().get(ch).map_or(0.0, |o| o.volume);
        let db = |x: f32| 20.0 * x.max(1e-6).log10();
        if db(v) > db(was) + crate::app::RAMP_OVER_DB as f32 {
            state.level_ramps.insert(ch, (db(was).max(-60.0), db(v), std::time::Instant::now()));
        } else {
            crate::app::write_output_volume(state, ch, v);
        }
    }
}

/// Links (`stereo`) or splits every input, playback and output pair, as
/// each strip's STEREO switch does.
fn set_all_linked(state: &mut TuxMix, stereo: bool) {
    for pair in 0..state.device.inputs().len() / 2 {
        if !stereo {
            crate::app::before_split(state, ChannelId::Input(pair * 2));
        }
        let _ = if stereo { crate::app::link_sources_gently(state, ChannelId::Input(pair * 2)) } else { state.device.set_input_pair_linked(pair, false) };
        if !stereo && state.center_pan_on_split {
            crate::app::center_split_pans(state, pair);
        }
    }
    for pair in 0..state.device.playbacks().len() / 2 {
        if !stereo {
            crate::app::before_split(state, ChannelId::Playback(pair * 2));
        }
        let _ = if stereo { crate::app::link_sources_gently(state, ChannelId::Playback(pair * 2)) } else { state.device.set_playback_linked(pair, false) };
    }
    for pair in 0..state.device.output_pair_count() {
        let _ = state.device.set_output_linked(pair, stereo);
    }
    crate::control_room::after_mix_change(state, None);
}

fn total_reset(state: &mut TuxMix) {
    // Letting go of Dim and the other overrides at once put Main back up by
    // the Dim amount in one write (the local Qwen's review): what rose glides
    // up from where it was.
    // Held as they let go (OpenCode's job 44): Main glides back up, and
    // the cells the overrides held down stay down until the reset below
    // silences them.
    let before = crate::app::output_levels(state);
    crate::control_room::release_holding(state, std::time::Instant::now());
    crate::app::glide_outputs_up_from(state, &before);
    clear_submixes(state);
    // Every cell is silent now: nothing is left to hold, and the
    // playbacks' glides below are the only writers of their cells.
    crate::control_room::drop_easing(state);
    straight_playback(state, true);
    clear_effects(state);
    for c in sources(state) {
        crate::app::write_mute(state, c, false);
    }
    for ch in 0..state.device.outputs().len() {
        crate::app::write_mute(state, ChannelId::Output(ch), false);
    }
    if state.device.protocol_features() {
        for i in 0..state.device.inputs().len() {
            let _ = state.device.set_phase(i, false);
        }
        for pair in 0..state.device.output_pair_count() {
            let _ = state.device.set_loopback(pair, false);
        }
    }
    crate::control_room::after_mix_change(state, None);
}
