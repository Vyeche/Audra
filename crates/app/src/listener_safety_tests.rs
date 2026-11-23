//! What the app writes to the card, in order: the listener-safety tests.
//!
//! Jobs 8 and 9 found levels written loud for a moment — a snapshot's
//! levels before its mutes, a group fader from silence, a remote's jump.
//! They are fixed, but only checked by reading the code and by end
//! states. The simulation device can log every level write (see
//! `tuxmix_core::mock::MockWrite`), so a test here can see the order of
//! the writes: the level that was loud, and the moment it was loud.
//!
//! The rule the app holds itself to: a level rises by no more than
//! `RAMP_OVER_DB` in one step; a bigger rise glides up over 300 ms.

use crate::app::{self, update, DeviceHandle, Message, TuxMix};
use std::time::{Duration, Instant};
use tuxmix_core::mock::MockWrite;
use tuxmix_core::{ChannelId, RmeDevice};

/// The app's own rule: a rise of more than this glides up instead.
const RAMP_OVER_DB: f32 = app::RAMP_OVER_DB as f32;

/// Where a write goes: an output's own master, or a channel's send into
/// an output pair.
#[derive(Clone, Copy, Debug)]
enum Spot {
    Output(usize),
    Send(ChannelId, usize),
}

/// The level a write put at a spot, if it is one.
fn level_at(w: &MockWrite, spot: Spot) -> Option<f32> {
    match (w, spot) {
        (MockWrite::OutputVolume(ch, v), Spot::Output(o)) => (*ch == o).then_some(*v),
        (MockWrite::Volume(c, o, v), Spot::Send(c2, o2)) => {
            (*c == c2 && *o == o2).then_some(*v)
        }
        (MockWrite::RawCrosspoint(c, o, v, _), Spot::Send(c2, o2)) => {
            (*c == c2 && *o == o2).then_some(*v)
        }
        _ => None,
    }
}

/// A state with a voice in the Main and in the Phones, and a second mic
/// in the Main, so the Control Room has a mix to take over (and something
/// to put back).
fn state_with_a_mix() -> TuxMix {
    let mut state = state();
    for (ch, out) in [(0usize, 0usize), (0, 1), (1, 0), (1, 1)] {
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(ch), out, 1.0));
    }
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Playback(0), 0, 1.0));
    state
}

/// What letting go of an override does now (the owner's pick,
/// 2026-11-11: "Glide back"): the level comes back up in steps. From what
/// the override held, no write falls and no step rises more than
/// `RAMP_OVER_DB` (silence counts as 60 dB under the level), and the last
/// lands exactly on the level that was there before the press, so none
/// is ever above it.
fn release_glides(name: &str, spot: Spot, before: f32, press: &[MockWrite], release: &[MockWrite]) {
    let held = press.iter().rev().find_map(|w| level_at(w, spot));
    let Some(held) = held else {
        panic!("{name}: the press wrote nothing at {spot:?} ({press:?})");
    };
    let levels: Vec<f32> = release.iter().filter_map(|w| level_at(w, spot)).collect();
    assert!(levels.len() > 2, "{name}: {} writes at {spot:?}, not a glide: {levels:?}", levels.len());
    let floor = db(before) - 60.0;
    let mut prev = held;
    let mut worst: f32 = 0.0;
    for &level in &levels {
        assert!(level + 1e-5 >= prev, "{name}: a write fell from {prev:.4} to {level:.4} on the way up");
        let rise = db(level) - db(prev).max(floor);
        worst = worst.max(rise);
        assert!(rise <= RAMP_OVER_DB + 0.01, "{name}: one step rose {rise:.1} dB ({prev:.4} to {level:.4})");
        prev = level;
    }
    let last = *levels.last().unwrap();
    assert!((last - before).abs() < 1e-4, "{name}: landed on {last:.4}, not the {before:.4} from before");
    eprintln!("{name}: {:.3} back to {before:.3} in {} steps, the biggest {worst:.1} dB", held, levels.len());
}

/// Steps every ease as the app's fast tick does, one every `every_ms`,
/// until each has landed.
fn settle_easing(state: &mut TuxMix, every_ms: u64) {
    let t0 = Instant::now();
    for i in 1..=80u64 {
        if !crate::control_room::is_easing(state) {
            return;
        }
        crate::control_room::step_easing(state, t0 + Duration::from_millis(every_ms * i));
    }
    panic!("an ease never landed");
}

/// The Control Room's Dim: pressing it turns the Main down at once, and
/// letting go of it glides the level back up.
#[test]
fn dim_release_glides_back_up_to_the_level_it_had() {
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::DimAmountChanged(-40.0));
    let main = state.control_room.assign.main;
    let spot = Spot::Output(main * 2);
    let before = state.device.outputs()[main * 2].volume;
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::DimToggle);
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::DimToggle);
    assert!(!state.control_room.is_dimmed(), "the key reads off at once");
    assert!(app::tick_interval(&state) <= Duration::from_millis(33), "the tick keeps its fast pace");
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    release_glides("Dim", spot, before, &press, &release);
}

/// Talkback: the talkback mic takes the Phones over, and letting go of
/// it glides the user's mix back.
#[test]
fn talkback_release_glides_the_mix_back() {
    let mut state = state_with_a_mix();
    // The talkback mic is AN1; AN2 in the Phones is what the talkback
    // puts out of the way, and what its release puts back.
    let phones = state.control_room.assign.phones_pairs()[0];
    let spot = Spot::Send(ChannelId::Input(1), phones);
    let before = state.device.volume(ChannelId::Input(1), phones).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::TalkbackToggle);
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::TalkbackToggle);
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    release_glides("Talkback", spot, before, &press, &release);
}

/// Listenback: the recording room's mic takes the Main over, and
/// letting go of it glides the user's mix back.
#[test]
fn listenback_release_glides_the_mix_back() {
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::ListenbackInputChanged(Some(1)));
    let spot = Spot::Send(ChannelId::Input(0), 0);
    let before = state.device.volume(ChannelId::Input(0), 0).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::ListenbackToggle);
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::ListenbackToggle);
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    release_glides("Listenback", spot, before, &press, &release);
}

/// External Input: the stereo input it plays takes the monitor pair
/// over, and letting go of it glides the user's mix back, from silence.
#[test]
fn external_input_release_glides_the_mix_back() {
    let mut state = state_with_a_mix();
    let monitor = state.control_room.assign.monitor(state.control_room.speaker_b);
    let spot = Spot::Send(ChannelId::Input(0), monitor);
    let before = state.device.volume(ChannelId::Input(0), monitor).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::ExtInToggle);
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::ExtInToggle);
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    release_glides("External Input", spot, before, &press, &release);
}

/// A late tick takes a longer glide, never a bigger step: Dim at its
/// deepest (-60 dB) let go with a tick only every 200 ms.
#[test]
fn a_late_tick_never_makes_a_dim_release_step_bigger() {
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::DimAmountChanged(-60.0));
    let main = state.control_room.assign.main;
    let spot = Spot::Output(main * 2);
    let before = state.device.outputs()[main * 2].volume;
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::DimToggle);
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::DimToggle);
    settle_easing(&mut state, 200);
    let release = writes_since(&state, mark);
    release_glides("Dim, late ticks", spot, before, &press, &release);
}

/// Dim pressed, let go, pressed again mid-glide and let go again: the
/// level Main comes back to is still the user's own, never one part-way
/// up (it lives in `dimmed_main` until the glide lands).
#[test]
fn dim_pressed_again_mid_glide_keeps_the_users_level() {
    let mut state = state_with_a_mix();
    let main = state.control_room.assign.main * 2;
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Output(main), main, 0.8));
    let user = state.device.outputs()[main].volume;
    let gain = vol(state.control_room.dim_db);
    let _ = update(&mut state, Message::DimToggle);
    let _ = update(&mut state, Message::DimToggle);
    let t0 = Instant::now();
    crate::control_room::step_easing(&mut state, t0 + Duration::from_millis(33));
    crate::control_room::step_easing(&mut state, t0 + Duration::from_millis(66));
    let part_way = state.device.outputs()[main].volume;
    assert!(part_way > user * gain && part_way < user, "part-way up: {part_way}");
    let p = app::strip_params(&state, ChannelId::Output(main), state.sel_out);
    assert!((p.vol - user).abs() < 1e-6, "the fader shows the user's level, not the glide");
    let _ = update(&mut state, Message::DimToggle);
    assert!(state.control_room.is_dimmed());
    assert!((state.device.outputs()[main].volume - user * gain).abs() < 1e-5, "down again at once");
    let _ = update(&mut state, Message::DimToggle);
    settle_easing(&mut state, 33);
    assert!((state.device.outputs()[main].volume - user).abs() < 1e-6, "back to the user's own level");
}

/// A snapshot, Undo, an Assign change or quit (`control_room::release`)
/// in the middle of a glide lands it at once on the exact level: nothing
/// is left part-way, with no tick to finish it after a quit.
#[test]
fn letting_go_of_everything_lands_a_glide_at_once() {
    let mut state = state_with_a_mix();
    let main = state.control_room.assign.main * 2;
    let user = state.device.outputs()[main].volume;
    let phones = state.control_room.assign.phones_pairs()[0];
    let an2 = ChannelId::Input(1);
    let before = state.device.volume(an2, phones).unwrap_or(0.0);
    let _ = update(&mut state, Message::DimToggle);
    let _ = update(&mut state, Message::TalkbackToggle);
    let _ = update(&mut state, Message::DimToggle);
    let _ = update(&mut state, Message::TalkbackToggle);
    crate::control_room::step_easing(&mut state, Instant::now() + Duration::from_millis(33));
    assert!(crate::control_room::is_easing(&state));
    crate::control_room::release(&mut state);
    assert!(!crate::control_room::is_easing(&state), "nothing left easing");
    assert!((state.device.outputs()[main].volume - user).abs() < 1e-6, "Main on the user's level");
    let hw = state.device.mock_hardware(an2, phones).unwrap().0;
    assert!((hw - before).abs() < 1e-6, "AN2 in the Phones on its level: {hw}");
}

/// A fader moved while a mix glides back is written eased, not at full
/// until the next tick, and the glide lands on the new level.
#[test]
fn a_fader_moved_mid_glide_joins_it() {
    let mut state = state_with_a_mix();
    let phones = state.control_room.assign.phones_pairs()[0];
    let an2 = ChannelId::Input(1);
    let _ = update(&mut state, Message::VolumeChanged(an2, phones, 0.5));
    let _ = update(&mut state, Message::TalkbackToggle);
    let _ = update(&mut state, Message::TalkbackToggle);
    crate::control_room::step_easing(&mut state, Instant::now() + Duration::from_millis(33));
    let _ = update(&mut state, Message::VolumeChanged(an2, phones, 0.9));
    let hw = state.device.mock_hardware(an2, phones).unwrap().0;
    assert!(hw < 0.9, "written eased after the move, not at full: {hw}");
    settle_easing(&mut state, 33);
    let hw = state.device.mock_hardware(an2, phones).unwrap().0;
    assert!((hw - 0.9).abs() < 1e-6, "landed on the new level: {hw}");
}

/// Recall pressed while dimmed, to a level under the user's own: Main
/// goes from the dimmed level to the Recall level, and never up to the
/// user's level on the way (it used to: Dim's release wrote it first).
#[test]
fn recall_while_dimmed_never_goes_up_to_the_users_level_first() {
    let mut state = state_with_a_mix();
    let main = state.control_room.assign.main * 2;
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Output(main), main, 1.0));
    let _ = update(&mut state, Message::RecallLevelChanged(-12.0));
    let _ = update(&mut state, Message::DimToggle);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::RecallMain);
    app::settle_glides(&mut state);
    let recall = vol(-12.0);
    let loudest = writes(&state)[mark..]
        .iter()
        .filter_map(|w| level_at(w, Spot::Output(main)))
        .fold(0.0f32, f32::max);
    assert!(loudest <= recall + 1e-4, "Main went up to {loudest:.3}, above the Recall level {recall:.3}");
    assert!((state.device.outputs()[main].volume - recall).abs() < 1e-4);
}

/// Cue: the cued output plays the AN1/2 bus, and letting go of it puts
/// the user's mix back.
#[test]
fn cue_release_puts_the_mix_back() {
    let mut state = state_with_a_mix();
    let target = state.control_room.assign.cue_target_for(state.control_room.speaker_b);
    // A source in the Phones' own mix and not in the Main's: Cue (the Main
    // on the Phones) silences it, and its release brings it back.
    let pb = ChannelId::Playback(2);
    let _ = update(&mut state, Message::VolumeChanged(pb, target, 1.0));
    let spot = Spot::Send(pb, target);
    let before = state.device.volume(pb, target).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::CueChanged(ChannelId::Output(0), true));
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::CueChanged(ChannelId::Output(0), false));
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    // Issue #37, the recommended pick: a glide, as Dim's.
    release_glides("Cue", spot, before, &press, &release);
}

/// Solo: soloing AN1 in the Main silences AN2 there; letting go glides AN2
/// back (issue #37).
#[test]
fn solo_release_glides_the_mix_back() {
    let mut state = state_with_a_mix();
    let main = state.sel_out;
    let spot = Spot::Send(ChannelId::Input(1), main);
    let before = state.device.volume(ChannelId::Input(1), main).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::Solo(ChannelId::Input(0), true));
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::Solo(ChannelId::Input(0), false));
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    release_glides("Solo", spot, before, &press, &release);
}

/// PFL (Live Mode): the Cue target plays AN1 alone; letting go glides its
/// own mix back (issue #37).
#[test]
fn pfl_release_glides_the_mix_back() {
    let mut state = state_with_a_mix();
    crate::control_room::set_live_mode(&mut state, true);
    let target = state.control_room.assign.cue_target_for(state.control_room.speaker_b);
    let spot = Spot::Send(ChannelId::Input(1), target);
    let before = state.device.volume(ChannelId::Input(1), target).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::Solo(ChannelId::Input(0), true));
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::Solo(ChannelId::Input(0), false));
    settle_easing(&mut state, 33);
    let release = writes_since(&state, mark);
    release_glides("PFL", spot, before, &press, &release);
}

/// The level a level in dB.
fn db(v: f32) -> f32 {
    20.0 * v.max(1e-6).log10()
}

/// A level from dB.
fn vol(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A state with the simulation device, its write log on.
fn state() -> TuxMix {
    let mut state = app::new(true, None, None);
    match &mut state.device {
        DeviceHandle::Mock(m) => m.log_writes(true),
        _ => panic!("the mock device"),
    }
    state
}

/// The writes so far.
fn writes(state: &TuxMix) -> Vec<MockWrite> {
    match &state.device {
        DeviceHandle::Mock(m) => m.writes().to_vec(),
        _ => Vec::new(),
    }
}

/// The writes since `from`, and they are not empty (a test that saw no
/// writes at all would pass for the wrong reason).
fn writes_since(state: &TuxMix, from: usize) -> Vec<MockWrite> {
    let all = writes(state);
    assert!(all.len() > from, "nothing was written: {all:?}");
    all[from..].to_vec()
}

/// Steps the glides the way the app's tick does: a step every 30 ms, so
/// a glide of 300 ms takes ten.
fn step_glides(state: &mut TuxMix, steps: u32) {
    let t0 = Instant::now();
    for i in 1..=steps {
        app::step_fader_glides(state, t0 + Duration::from_millis(30 * u64::from(i)));
    }
}

/// The biggest single-step rise of one send, in dB: every write for that
/// send against the one before it (`old` being the level before them).
fn biggest_rise(writes: &[MockWrite], cid: ChannelId, out: usize, old: f32) -> f32 {
    let mut prev = old;
    let mut worst: f32 = 0.0;
    let mut seen = false;
    for w in writes {
        if let MockWrite::Volume(c, o, v) = w {
            if *c == cid && *o == out {
                worst = worst.max(db(*v) - db(prev));
                prev = *v;
                seen = true;
            }
        }
    }
    // A test that saw no write for this send proves nothing (Qwen's review
    // of job 10): it fails instead of passing quietly.
    assert!(seen, "no write for {cid:?} into {out} among {writes:?}");
    worst
}

/// The rise the helper sees, so a test that passed because it saw nothing
/// cannot pass quietly.
#[test]
fn the_helper_sees_a_rise() {
    let writes = [
        MockWrite::Volume(ChannelId::Input(0), 0, vol(-30.0)),
        MockWrite::Volume(ChannelId::Input(0), 0, vol(0.0)),
    ];
    let rise = biggest_rise(&writes, ChannelId::Input(0), 0, vol(-30.0));
    assert!((rise - 30.0).abs() < 0.5, "the helper saw {rise:.1} dB");
}

/// A snapshot that raises a send by 30 dB never writes more than 6 dB
/// above the old level: the scene is capped before it reaches the card and
/// the rest glides up.
#[test]
fn a_snapshot_never_writes_a_send_loud_before_the_glide() {
    let mut state = state();
    let quiet = vol(-30.0);
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, quiet));
    let mut scene = state.device.capture_scene();
    scene.inputs[0].volumes[0] = vol(0.0); // 30 dB louder
    let before = writes(&state).len();
    app::apply_mix_scene(&mut state, &scene).expect("the scene applies");
    let after = writes_since(&state, before);
    let rise = biggest_rise(&after, ChannelId::Input(0), 0, quiet);
    eprintln!("a snapshot's send: the biggest step {rise:.2} dB (the rule is {RAMP_OVER_DB})");
    assert!(
        rise <= RAMP_OVER_DB + 0.5,
        "a write of {rise:.1} dB over the old level (the rule is {RAMP_OVER_DB})"
    );
    // And the glide brings it the rest of the way, in steps.
    step_glides(&mut state, 12);
    let all = writes_since(&state, before);
    let rise = biggest_rise(&all, ChannelId::Input(0), 0, quiet);
    eprintln!("a snapshot's send with the glide: the biggest step {rise:.2} dB");
    assert!(
        rise <= RAMP_OVER_DB + 0.5,
        "the glide stepped {rise:.1} dB at once"
    );
    let end = all
        .iter()
        .rev()
        .find_map(|w| match w {
            MockWrite::Volume(ChannelId::Input(0), 0, v) => Some(*v),
            _ => None,
        })
        .unwrap_or(0.0);
    assert!(end >= vol(-6.0), "the glide never brought the send up: {end:.3}");
}

/// A snapshot that mutes a strip never writes that strip's level before
/// its mute.
#[test]
fn a_snapshot_never_writes_a_strips_level_before_its_mute() {
    let mut state = state();
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, vol(-6.0)));
    let mut scene = state.device.capture_scene();
    scene.inputs[0].mute = true;
    let before = writes(&state).len();
    app::apply_mix_scene(&mut state, &scene).expect("the scene applies");
    let after = writes_since(&state, before);
    let mute_at = after
        .iter()
        .position(|w| matches!(w, MockWrite::Mute(ChannelId::Input(0), true)));
    let level_at = after
        .iter()
        .position(|w| matches!(w, MockWrite::Volume(ChannelId::Input(0), _, _)));
    // The order inside a scene is the backend's: the Babyface's adopts the
    // mutes before the levels (job 8). The stand-in device logs its scene
    // that way, so this checks that the app writes no level of its own for
    // the strip ahead of the scene's mute (Qwen's review of job 10). And it
    // requires both writes, so it can't pass on none.
    let mute_at = mute_at.unwrap_or_else(|| panic!("the mute was never written: {after:?}"));
    let level_at = level_at.unwrap_or_else(|| panic!("no level was written for the strip: {after:?}"));
    assert!(mute_at < level_at, "the level was written before the mute: {after:?}");
}

/// A group fader moved up from silence never writes a member more than
/// its travel allows: the members move by the fader's own travel, as a
/// ganged console fader does.
#[test]
fn a_group_fader_from_silence_never_flings_its_members() {
    let mut state = state();
    // Two strips, ganged on their faders, both routed nowhere to start.
    state.groups[0] = crate::sidebar::Group {
        members: vec![ChannelId::Input(0), ChannelId::Input(1)],
        mute_linked: false,
        solo_linked: false,
        fader_linked: true,
    };
    // Set apart, past the group: the lead at silence, the member at -20 dB
    // (through the group, moving the member would have moved the lead too).
    let _ = state.device.set_volume(ChannelId::Input(0), 0, 0.0);
    let _ = state.device.set_volume(ChannelId::Input(1), 0, vol(-20.0));
    let before = writes(&state).len();
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, vol(-20.0)));
    step_glides(&mut state, 12);
    let after = writes_since(&state, before);
    // The lead's own write, and the member's.
    let lead = after
        .iter()
        .filter_map(|w| match w {
            MockWrite::Volume(ChannelId::Input(0), 0, v) => Some(*v),
            _ => None,
        })
        .fold(0.0f32, f32::max);
    let member = after
        .iter()
        .filter_map(|w| match w {
            MockWrite::Volume(ChannelId::Input(1), 0, v) => Some(*v),
            _ => None,
        })
        .fold(0.0f32, f32::max);
    // The member moves by the lead's travel on the fader, as a ganged
    // console fader does: from silence to -20 dB is 43% of the travel, so
    // the member at -20 dB goes to about +3 dB. Passed in dB (silence
    // counted as -100), it was an 80 dB step and the member hit the +6 dB
    // top (2.0). Claude's rewrite of OpenCode's job 10 test: the lead had
    // been moved up with the member, so it never started at silence.
    use crate::widgets::fader::{t_to_vol, vol_to_t};
    // From the intended move (silence to -20 dB), not the logged lead
    // (Qwen's review).
    assert!((lead - vol(-20.0)).abs() < 1e-3, "the lead went where it was moved ({lead:.3})");
    let expected = t_to_vol((vol_to_t(vol(-20.0)) + vol_to_t(vol(-20.0))).min(1.0));
    eprintln!("a group from silence: the lead to {lead:.3}, the member to {member:.3} (by travel {expected:.3})");
    assert!((member - expected).abs() < 1e-3, "the member moves by the lead's travel ({member:.3}, expected {expected:.3})");
    assert!(member < 1.9, "not to the top in one step ({member:.3})");
}

/// A small nudge of a group's lead near the top lifts a member near the
/// bottom by many dB of travel: it glides there, never more than 6 dB a
/// write (issue #38, the recommended pick).
#[test]
fn a_group_member_near_the_bottom_glides_up_by_6_db_at_most() {
    let mut state = state();
    state.groups[0] = crate::sidebar::Group {
        members: vec![ChannelId::Input(0), ChannelId::Input(1)],
        mute_linked: false,
        solo_linked: false,
        fader_linked: true,
    };
    let quiet = vol(-58.0);
    let _ = state.device.set_volume(ChannelId::Input(0), 0, vol(-10.0));
    let _ = state.device.set_volume(ChannelId::Input(1), 0, quiet);
    let before = writes(&state).len();
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, vol(-2.0)));
    app::settle_glides(&mut state);
    let member: Vec<f32> = writes_since(&state, before)
        .iter()
        .filter_map(|w| match w {
            MockWrite::Volume(ChannelId::Input(1), 0, v) => Some(*v),
            _ => None,
        })
        .collect();
    use crate::widgets::fader::{t_to_vol, vol_to_t};
    let expected = t_to_vol(vol_to_t(quiet) + vol_to_t(vol(-2.0)) - vol_to_t(vol(-10.0)));
    assert!(db(expected) - db(quiet) > 12.0, "the nudge is many dB down there ({:.1} dB)", db(expected) - db(quiet));
    let mut prev = quiet;
    for &v in &member {
        assert!(db(v) - db(prev) <= RAMP_OVER_DB + 0.01, "the member rose {:.1} dB in one write", db(v) - db(prev));
        prev = v;
    }
    let last = *member.last().expect("the member moved");
    assert!((last - expected).abs() < 1e-3, "it lands where its travel says ({last:.4}, expected {expected:.4})");
}

/// A member gliding up from silence never takes its Trim Gains mixes up
/// with it: one write from -inf trims nothing, so its glide doesn't either
/// (the reviewer: the other mix came up about 16 dB, step by step).
#[test]
fn a_member_gliding_from_silence_leaves_its_other_mixes_alone() {
    let mut state = state();
    let (lead, member) = (ChannelId::Input(0), ChannelId::Input(1));
    state.groups[0] = crate::sidebar::Group { members: vec![lead, member], mute_linked: false, solo_linked: false, fader_linked: true };
    let _ = update(&mut state, Message::TrimStripToggle(member));
    assert!(app::trim_on(&state, member));
    let _ = state.device.set_volume(lead, 0, vol(-10.0));
    let _ = state.device.set_volume(member, 0, 0.0);
    let _ = state.device.set_volume(member, 1, vol(-10.0));
    let _ = update(&mut state, Message::VolumeChanged(lead, 0, vol(-4.0)));
    app::settle_glides(&mut state);
    assert!(state.device.volume(member, 0).unwrap_or(0.0) > 0.0, "the member came up in its mix");
    let other = state.device.volume(member, 1).unwrap_or(0.0);
    assert!((db(other) - -10.0).abs() < 0.1, "its other mix stays at -10 dB, not {:.1} dB", db(other));
}

/// A remote (OSC) that jumps up writes nothing more than 6 dB over the
/// old level in one step: the jump glides.
#[test]
fn a_remote_jump_up_glides_instead_of_stepping() {
    let mut state = state();
    let quiet = vol(-30.0);
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, quiet));
    let before = writes(&state).len();
    let _ = update(
        &mut state,
        Message::OscCommand(crate::osc::OscCommand::Volume(
            ChannelId::Input(0),
            0,
            vol(0.0),
        )),
    );
    step_glides(&mut state, 12);
    let after = writes_since(&state, before);
    let rise = biggest_rise(&after, ChannelId::Input(0), 0, quiet);
    eprintln!("a remote's jump: the biggest step {rise:.2} dB (the rule is {RAMP_OVER_DB})");
    assert!(
        rise <= RAMP_OVER_DB + 0.5,
        "the remote's jump wrote {rise:.1} dB at once (the rule is {RAMP_OVER_DB})"
    );
}

/// Reset Mix's straight playback glides its rises: a playback at −30 dB
/// comes up in steps, not in one.
#[test]
fn reset_mix_straight_playback_glides() {
    let mut state = state();
    let quiet = vol(-30.0);
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Playback(0), 0, quiet));
    let before = writes(&state).len();
    let _ = update(
        &mut state,
        Message::ResetMix(crate::resetmix::ResetMix::StraightPlayback),
    );
    step_glides(&mut state, 12);
    let after = writes_since(&state, before);
    let rise = biggest_rise(&after, ChannelId::Playback(0), 0, quiet);
    eprintln!("reset mix's straight playback: the biggest step {rise:.2} dB (the rule is {RAMP_OVER_DB})");
    assert!(
        rise <= RAMP_OVER_DB + 0.5,
        "the reset wrote {rise:.1} dB at once (the rule is {RAMP_OVER_DB})"
    );
}

/// The first tick after a busy start-up (or a stalled window) comes late:
/// a send recovered or recalled on its way up moves one regular step, never
/// the whole way (the scout, 2026-11-12: the RAMP_FOR glides took their
/// share straight from the clock).
#[test]
fn a_late_first_tick_never_lands_a_glide_in_one_step() {
    let mut state = state_with_a_mix();
    let main = state.control_room.assign.main * 2;
    let quiet = 10f32.powf(-60.0 / 20.0);
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Output(main), main, quiet));
    let since = Instant::now();
    state.level_ramps.insert(main, (-60.0, 0.0, since));
    // Two seconds late: by the clock the glide is long over.
    app::step_level_ramps(&mut state, since + Duration::from_secs(2));
    let db = 20.0 * state.device.outputs()[main].volume.max(1e-6).log10();
    assert!(db < -60.0 + 60.0 * app::RAMP_STEP_MAX + 0.01, "one regular step, not the way: {db} dB");
    assert!(!state.level_ramps.is_empty(), "still gliding");
    // Then a tick at a time: each step no bigger than a regular one.
    let mut last = db;
    let mut at = since + Duration::from_secs(2);
    for _ in 0..20 {
        at += Duration::from_millis(33);
        app::step_level_ramps(&mut state, at);
        let now = 20.0 * state.device.outputs()[main].volume.max(1e-6).log10();
        assert!(now - last <= 60.0 * app::RAMP_STEP_MAX + 0.01, "a step of {} dB", now - last);
        last = now;
    }
    assert!(state.level_ramps.is_empty() && last.abs() < 0.1, "it lands: {last}");

    // A preset's gain the same way.
    let before = state.device.inputs()[0].gain.unwrap_or(0);
    state.gain_ramps.insert(0, (before, before + 40, since));
    app::step_gain_ramps(&mut state, since + Duration::from_secs(2));
    let g = state.device.inputs()[0].gain.unwrap_or(0);
    assert!(g <= before + 5, "one regular step of the gain: {before} to {g}");
}

/// Replays the writes `press` makes, in order, and checks that after each
/// one no output plays louder than it did before the press or after it.
/// What an output plays: its master × the sum of every source's level into
/// its pair, as the hardware holds them (OpenCode's job 33: the loudest
/// alone understated two sources, and the Phones weren't watched).
fn no_loud_moment(name: &str, state: &mut TuxMix, press: impl FnOnce(&mut TuxMix)) {
    let one = state.device.outputs_one_per_pair();
    let pair_of = |c: usize| if one { c } else { c / 2 };
    let outs: Vec<usize> = (0..state.device.outputs().len()).collect();
    let pairs: Vec<usize> = (0..state.device.output_pair_count()).collect();
    let sources: Vec<ChannelId> = (0..state.device.inputs().len())
        .map(ChannelId::Input)
        .chain((0..state.device.playbacks().len()).map(ChannelId::Playback))
        .collect();
    let read = |state: &TuxMix| {
        let master: std::collections::HashMap<usize, f32> = outs.iter().map(|&c| (c, state.device.outputs()[c].volume)).collect();
        let sends: std::collections::HashMap<(ChannelId, usize), f32> = sources
            .iter()
            .flat_map(|&s| pairs.iter().map(move |&p| (s, p)))
            .map(|(s, p)| ((s, p), state.device.mock_hardware(s, p).map_or(0.0, |h| h.0)))
            .collect();
        (master, sends)
    };
    let loud = |master: &std::collections::HashMap<usize, f32>, sends: &std::collections::HashMap<(ChannelId, usize), f32>, c: usize| {
        let mix: f32 = sends.iter().filter(|((_, p), _)| *p == pair_of(c)).map(|(_, v)| *v).sum();
        master[&c] * mix
    };
    let (mut master, mut sends) = read(state);
    let before: Vec<f32> = outs.iter().map(|&c| loud(&master, &sends, c)).collect();
    let mark = writes(state).len();
    press(state);
    let (m_after, s_after) = read(state);
    let after: Vec<f32> = outs.iter().map(|&c| loud(&m_after, &s_after, c)).collect();
    for w in writes_since(state, mark) {
        match w {
            MockWrite::OutputVolume(c, v) if master.contains_key(&c) => {
                master.insert(c, v);
            }
            MockWrite::RawCrosspoint(s, p, v, _) | MockWrite::Volume(s, p, v) if sends.contains_key(&(s, p)) => {
                sends.insert((s, p), v);
            }
            _ => {}
        }
        for (k, &c) in outs.iter().enumerate() {
            let now = loud(&master, &sends, c);
            // A hair over: a cell an ease brings back from silence starts
            // 60 dB under its level (0.1 % of it), and a few add up.
            let ceiling = before[k].max(after[k]);
            assert!(now <= ceiling * 1.01 + 1e-6, "{name}: output {c} played {now:.4} mid-switch, above {ceiling:.4} (after {w:?})");
        }
    }
}

/// Main B on pair 2 (AS1/2), a mic and the computer in the Main and the
/// Phones.
fn with_main_b_and_a_mix() -> TuxMix {
    let mut state = state();
    let phones = state.control_room.assign.phones[0].unwrap_or(1);
    for (src, v) in [(ChannelId::Input(0), 0.7), (ChannelId::Playback(0), 0.5)] {
        let _ = state.device.set_volume(src, 0, v);
        let _ = state.device.set_volume(src, phones, v);
    }
    let assign = state.control_room.assign.with_main_b(Some(2));
    let _ = update(&mut state, Message::SetAssign(assign));
    let _ = update(&mut state, Message::DimAmountChanged(-40.0));
    state
}

/// Speaker B pressed while Dim is on, and again while it glides back with
/// Link on: no moment plays any output louder than before or after (the
/// scout, 2026-11-12: Dim was let go first, so the old Main came back to
/// full level while its mix still played, and the new Main played at full
/// level until Dim came back on).
#[test]
fn speaker_b_while_dimmed_never_plays_a_main_up_by_the_dim() {
    let mut state = with_main_b_and_a_mix();
    let _ = update(&mut state, Message::DimToggle);
    assert!(state.control_room.is_dimmed());
    no_loud_moment("Dim on", &mut state, |s| {
        let _ = update(s, Message::SpeakerBToggle);
    });
    assert!(state.control_room.is_dimmed(), "Dim stays on");
    no_loud_moment("Dim on, back again", &mut state, |s| {
        let _ = update(s, Message::SpeakerBToggle);
    });

    let mut state = with_main_b_and_a_mix();
    let _ = update(&mut state, Message::SpeakerBLinkToggle);
    let _ = update(&mut state, Message::DimToggle);
    let _ = update(&mut state, Message::DimToggle);
    crate::control_room::step_easing(&mut state, Instant::now() + Duration::from_millis(33));
    assert!(crate::control_room::is_easing(&state));
    no_loud_moment("Dim gliding back, Link on", &mut state, |s| {
        let _ = update(s, Message::SpeakerBToggle);
    });
    assert!(crate::control_room::is_easing(&state), "the glide carries on");

    let mut state = with_main_b_and_a_mix();
    no_loud_moment("Dim off", &mut state, |s| {
        let _ = update(s, Message::SpeakerBToggle);
    });
}

/// A Monitor setting changed while its override plays: what it raises
/// eases up, never in one write (OpenCode's job 41: Talkback's dim from
/// -60 dB to 0 dB while talking put the Phones' other sources up 60 dB).
#[test]
fn a_talkback_dim_raised_while_talking_eases_the_phones_up() {
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::TalkbackDimChanged(-60.0));
    let phones = state.control_room.assign.phones_pairs()[0];
    let spot = Spot::Send(ChannelId::Input(1), phones);
    let before = state.device.volume(ChannelId::Input(1), phones).unwrap_or(0.0);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::TalkbackToggle);
    let press = writes_since(&state, mark);
    let mark = writes(&state).len();
    let _ = update(&mut state, Message::TalkbackDimChanged(0.0));
    settle_easing(&mut state, 33);
    let raise = writes_since(&state, mark);
    release_glides("Talkback dim raised", spot, before, &press, &raise);
}

/// A glide longer than 60 dB (a strip from off to +6 dB) still steps no
/// more than 6 dB a tick (OpenCode's job 38: 0.099 of 66 dB was 6.5 dB).
#[test]
fn a_66_db_glide_never_steps_more_than_6_db() {
    let mut state = state();
    let an1 = ChannelId::Input(0);
    let _ = update(&mut state, Message::VolumeChanged(an1, 0, 0.0));
    let _ = update(&mut state, Message::RangeCleared(an1));
    let _ = update(&mut state, Message::MatrixSet(an1, 0, 2.0, false));
    assert!(state.fader_glides.contains_key(&(an1, 0)), "a jump to +6 dB glides");
    let mark = writes(&state).len();
    step_glides(&mut state, 30);
    let all = writes_since(&state, mark);
    let rise = biggest_rise(&all, an1, 0, 10f32.powf(-60.0 / 20.0));
    assert!(rise <= 6.0, "a step of {rise} dB");
}

/// A press on the screen mid-glide takes the level from it, as ever; only a
/// remote's stream of moves keeps the glide's clock (job 38).
#[test]
fn a_hand_on_the_screen_still_takes_a_level_from_its_glide() {
    let mut state = state();
    let an1 = ChannelId::Input(0);
    let _ = update(&mut state, Message::VolumeChanged(an1, 0, 0.001));
    let _ = update(&mut state, Message::RangeCleared(an1));
    let _ = update(&mut state, Message::MatrixSet(an1, 0, 0.6, false));
    let first = state.fader_glides.get(&(an1, 0)).copied().expect("gliding");
    let _ = update(&mut state, Message::MatrixSet(an1, 0, 0.7, false));
    let next = state.fader_glides.get(&(an1, 0)).copied().expect("gliding again");
    assert!(next.since > first.since, "a new glide, started now, not the old one retargeted");
    assert!((next.to - 0.7).abs() < 1e-6);
}

/// The overrides a Total Reset, a snapshot or Undo lets go of: Dim at
/// -40 dB and Talkback on, then each of a solo and the Cue.
fn with_overrides_on(extra: Option<Message>) -> TuxMix {
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::DimAmountChanged(-40.0));
    let _ = update(&mut state, Message::DimToggle);
    let _ = update(&mut state, Message::TalkbackToggle);
    match extra {
        // The PFL has no message of its own here: a strip's solo in PFL mode.
        Some(Message::ToggleSidebar) => {
            let _ = crate::control_room::set_pfl(&mut state, &[ChannelId::Input(0)], true, false);
            assert!(!state.control_room.pfl.is_empty());
        }
        Some(m) => {
            let _ = update(&mut state, m);
        }
        None => {}
    }
    assert!(state.control_room.is_dimmed() && state.control_room.talkback);
    state
}

/// The extras `with_overrides_on` is tried with.
fn extra_overrides() -> Vec<(&'static str, Option<Message>)> {
    vec![
        ("Dim and Talkback", None),
        ("and a solo", Some(Message::Solo(ChannelId::Input(0), true))),
        ("and the Cue", Some(Message::CueChanged(ChannelId::Output(2), true))),
        ("and the PFL", Some(Message::ToggleSidebar)),
        ("and Listenback", Some(Message::ListenbackToggle)),
        ("and External Input", Some(Message::ExtInToggle)),
    ]
}

/// A Total Reset with the overrides on: no moment plays any output louder
/// than before or after (OpenCode's job 44: letting go put Main back at
/// your level in one write before the glide took it down again, and
/// Talkback's, a solo's and the Cue's cells came back in one write before
/// the reset silenced them).
#[test]
fn a_total_reset_with_the_overrides_on_has_no_loud_moment() {
    use crate::resetmix::ResetMix;
    for (name, extra) in extra_overrides() {
        let mut state = with_overrides_on(extra);
        let _ = update(&mut state, Message::ResetMixArm(ResetMix::TotalReset));
        no_loud_moment(&format!("Total Reset, {name}"), &mut state, |s| {
            let _ = update(s, Message::ResetMix(ResetMix::TotalReset));
        });
        assert!(!state.control_room.is_dimmed() && !state.control_room.talkback, "{name}: let go");
    }
}

/// A snapshot or Undo with the overrides on: the same (OpenCode's job 44:
/// every scene lets go of them first, through the same writes).
#[test]
fn a_snapshot_or_undo_with_the_overrides_on_has_no_loud_moment() {
    for (name, extra) in extra_overrides() {
        let mut state = state_with_a_mix();
        let _ = update(&mut state, Message::SnapshotStoreTo(1));
        let mut on = with_overrides_on(extra.clone());
        std::mem::swap(&mut on.mock_snapshots, &mut state.mock_snapshots);
        on.stored_snapshots = state.stored_snapshots;
        no_loud_moment(&format!("snapshot, {name}"), &mut on, |s| {
            let _ = update(s, Message::SnapshotClicked(1));
        });
        assert!(!on.control_room.is_dimmed(), "{name}: let go");

        let mut state = with_overrides_on(extra);
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(1), 0, 0.5));
        no_loud_moment(&format!("Undo, {name}"), &mut state, |s| {
            let _ = update(s, Message::Undo);
        });
    }
}

/// No write raises an output's level or a cell more than `RAMP_OVER_DB`
/// over the one before it at the same spot (silence counts as 60 dB under
/// the new level), from `run`'s press through every glide after it.
fn no_step_over_6_db(name: &str, state: &mut TuxMix, run: impl FnOnce(&mut TuxMix)) {
    let step = 10f32.powf(app::RAMP_OVER_DB as f32 / 20.0);
    let pairs = state.device.output_pair_count();
    let mut outs: std::collections::HashMap<usize, f32> =
        state.device.outputs().iter().enumerate().map(|(c, o)| (c, o.volume)).collect();
    let mut cells: std::collections::HashMap<(ChannelId, usize), f32> = (0..state.device.inputs().len())
        .map(ChannelId::Input)
        .chain((0..state.device.playbacks().len()).map(ChannelId::Playback))
        .flat_map(|s| (0..pairs).map(move |p| (s, p)))
        .map(|(s, p)| ((s, p), state.device.mock_hardware(s, p).map_or(0.0, |h| h.0)))
        .collect();
    let mark = writes(state).len();
    run(state);
    for w in writes_since(state, mark) {
        let (spot, new, was) = match w {
            MockWrite::OutputVolume(c, v) => (format!("output {c}"), v, outs.insert(c, v)),
            MockWrite::RawCrosspoint(s, p, v, _) | MockWrite::Volume(s, p, v) => (format!("{s:?} in pair {p}"), v, cells.insert((s, p), v)),
            _ => continue,
        };
        // From silence a glide starts at -60 dB (`capped_share`'s floor).
        let was = was.unwrap_or(0.0).max(0.001);
        assert!(new <= was * step * 1.001 + 1e-6, "{name}: {spot} stepped up from {was:.4} to {new:.4} in one write");
    }
}

/// Every glide and ease run to its end.
fn settle_all(state: &mut TuxMix) {
    app::settle_glides(state);
    crate::control_room::settle_easing(state);
}

/// A snapshot of the mix with the Phones' cells silent: loading it, the
/// cells Talkback held down go down, never up to their own level first.
fn with_a_quiet_phones_snapshot(on: &mut TuxMix) {
    let mut state = state_with_a_mix();
    let phones = state.control_room.assign.phones_pairs()[0];
    for ch in 0..2 {
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(ch), phones, 0.0));
    }
    let _ = update(&mut state, Message::SnapshotStoreTo(1));
    std::mem::swap(&mut on.mock_snapshots, &mut state.mock_snapshots);
    on.stored_snapshots = state.stored_snapshots;
}

/// Letting go before a scene or a Total Reset, step by step: no level
/// rises more than 6 dB in one write, through the press and every glide
/// after it (OpenCode's job 44). Also a snapshot of quieter Phones, and
/// one pressed while Talkback eases back (`land_easing` put it up at once).
#[test]
fn letting_go_before_a_scene_never_steps_a_level_up_over_6_db() {
    use crate::resetmix::ResetMix;
    for (name, extra) in extra_overrides() {
        let mut state = with_overrides_on(extra.clone());
        let _ = update(&mut state, Message::ResetMixArm(ResetMix::TotalReset));
        no_step_over_6_db(&format!("Total Reset, {name}"), &mut state, |s| {
            let _ = update(s, Message::ResetMix(ResetMix::TotalReset));
            settle_all(s);
        });

        let mut state = with_overrides_on(extra.clone());
        with_a_quiet_phones_snapshot(&mut state);
        no_loud_moment(&format!("quiet Phones snapshot, {name}"), &mut state, |s| {
            let _ = update(s, Message::SnapshotClicked(1));
        });
        let mut state = with_overrides_on(extra.clone());
        with_a_quiet_phones_snapshot(&mut state);
        no_step_over_6_db(&format!("quiet Phones snapshot, {name}"), &mut state, |s| {
            let _ = update(s, Message::SnapshotClicked(1));
            settle_all(s);
        });

        let mut state = with_overrides_on(extra);
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(1), 0, 0.5));
        no_step_over_6_db(&format!("Undo, {name}"), &mut state, |s| {
            let _ = update(s, Message::Undo);
            settle_all(s);
        });
    }
    // Talkback let go by its key, still easing back, then a snapshot.
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::SnapshotStoreTo(1));
    let _ = update(&mut state, Message::TalkbackToggle);
    let _ = update(&mut state, Message::TalkbackToggle);
    crate::control_room::step_easing(&mut state, Instant::now() + Duration::from_millis(33));
    assert!(crate::control_room::is_easing(&state));
    no_loud_moment("snapshot mid-ease", &mut state, |s| {
        let _ = update(s, Message::SnapshotClicked(1));
    });
    no_step_over_6_db("after a snapshot mid-ease", &mut state, settle_all);
}

/// Where Main lands after a snapshot that let go of Dim: your level when
/// Settings keep the Main's, the snapshot's when they keep nothing.
#[test]
fn a_snapshot_that_lets_go_of_dim_lands_main_where_it_should() {
    use crate::control_room::SnapshotKeep;
    for (keep, want) in [(SnapshotKeep::Main, 0.8), (SnapshotKeep::Nothing, 0.5)] {
        let mut state = state_with_a_mix();
        let main = state.control_room.assign.main * 2;
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Output(main), main, 0.5));
        let _ = update(&mut state, Message::SnapshotStoreTo(1));
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Output(main), main, 0.8));
        state.snapshot_keep = keep;
        let _ = update(&mut state, Message::DimAmountChanged(-40.0));
        let _ = update(&mut state, Message::DimToggle);
        let _ = update(&mut state, Message::SnapshotClicked(1));
        assert!(!state.control_room.is_dimmed());
        settle_all(&mut state);
        let at = state.device.outputs()[main].volume;
        assert!((at - want).abs() < 1e-3, "{keep:?}: Main at {at}, not {want}");
    }
}

/// A fader moved in a pair an override holds down (the rest under a solo,
/// the Phones under Talkback): the cell never plays its own level first.
#[test]
fn a_fader_in_a_held_pair_never_plays_its_own_level_first() {
    let mut state = state_with_a_mix();
    let _ = update(&mut state, Message::Solo(ChannelId::Input(0), true));
    no_loud_moment("solo, another strip's fader", &mut state, |s| {
        let _ = update(s, Message::VolumeChanged(ChannelId::Input(1), 0, 0.9));
    });
    let mut state = state_with_a_mix();
    let phones = state.control_room.assign.phones_pairs()[0];
    let _ = update(&mut state, Message::TalkbackToggle);
    no_loud_moment("Talkback, a Phones fader", &mut state, |s| {
        let _ = update(s, Message::VolumeChanged(ChannelId::Input(1), phones, 0.9));
    });
    // Still reaches the card: held while Talkback is on, the new level once
    // it lets go (a fader that only moved the model would be stuck).
    let held = state.device.mock_hardware(ChannelId::Input(1), phones).unwrap().0;
    assert!(held > 0.0 && held < 0.9, "under Talkback's dim: {held}");
    let _ = update(&mut state, Message::TalkbackToggle);
    settle_all(&mut state);
    let hw = state.device.mock_hardware(ChannelId::Input(1), phones).map_or(state.device.volume(ChannelId::Input(1), phones).unwrap(), |h| h.0);
    assert!((hw - 0.9).abs() < 1e-6, "the new level once Talkback lets go: {hw}");
}

/// A workspace and Recall let go the same way (the advisor: the workspace
/// alone sets Live Mode between the let-go and `keep_levels`; Recall let
/// Talkback's Phones back in one write).
#[test]
fn a_workspace_or_recall_with_the_overrides_on_has_no_loud_moment() {
    for (name, extra) in extra_overrides() {
        let state = state_with_a_mix();
        let ws = crate::workspaces::capture(&state, "test");
        let mut on = with_overrides_on(extra.clone());
        no_loud_moment(&format!("workspace, {name}"), &mut on, |s| {
            crate::workspaces::apply(s, ws.clone());
        });
        let mut on = with_overrides_on(extra.clone());
        let ws = crate::workspaces::capture(&state, "test");
        no_step_over_6_db(&format!("workspace, {name}"), &mut on, |s| {
            crate::workspaces::apply(s, ws);
            settle_all(s);
        });
        let mut on = with_overrides_on(extra);
        no_step_over_6_db(&format!("Recall, {name}"), &mut on, |s| {
            let _ = update(s, Message::RecallMain);
            settle_all(s);
        });
    }
}

/// A scene the unit refuses, loaded while dimmed: Main still comes back to
/// your level, and the held cells to theirs (the advisor: the glides were
/// cleared before the scene was written, and the Dim's glide with them).
#[test]
fn a_refused_scene_still_brings_main_and_the_held_cells_back() {
    let mut state = with_overrides_on(None);
    let main = state.control_room.assign.main * 2;
    let user = state.control_room.dimmed_main.expect("dimmed")[0];
    let phones = state.control_room.assign.phones_pairs()[0];
    let an2 = ChannelId::Input(1);
    let want = state.device.volume(an2, phones).unwrap();
    let mut scene = crate::control_room::capture_scene(&state);
    scene.model = "Another unit".into();
    let levels = app::output_levels(&state);
    crate::control_room::release_holding(&mut state, Instant::now());
    assert!(app::apply_mix_scene_from(&mut state, &scene, &levels).is_err(), "refused");
    settle_all(&mut state);
    let at = state.device.outputs()[main].volume;
    assert!((at - user).abs() < 1e-3, "Main back at your level: {at}, not {user}");
    let hw = state.device.mock_hardware(an2, phones).map_or(want, |h| h.0);
    assert!((hw - want).abs() < 1e-6, "AN2 back in the Phones: {hw}, not {want}");
}

/// An Assign change (Settings → Monitor) with Dim and Talkback on, and
/// Main B leaving its role: no moment louder, no step over 6 dB (Main B
/// went from silent to its own mix in one write; Dim let go at once).
#[test]
fn an_assign_change_with_the_overrides_on_has_no_loud_moment() {
    for (name, extra) in extra_overrides() {
        let mut state = with_main_b_and_a_mix();
        let _ = update(&mut state, Message::DimToggle);
        let _ = update(&mut state, Message::TalkbackToggle);
        if let Some(m) = extra.filter(|m| !matches!(m, Message::ToggleSidebar)) {
            let _ = update(&mut state, m);
        }
        let to = state.control_room.assign.with_main_b(None);
        no_loud_moment(&format!("Assign, {name}"), &mut state, |s| {
            let _ = update(s, Message::SetAssign(to));
        });
        let mut state = with_main_b_and_a_mix();
        let _ = update(&mut state, Message::DimToggle);
        let _ = update(&mut state, Message::TalkbackToggle);
        no_step_over_6_db(&format!("Assign, {name}"), &mut state, |s| {
            let _ = update(s, Message::SetAssign(to));
            settle_all(s);
        });
        assert!(!state.control_room.is_dimmed());
    }
}

/// Quitting with the overrides on: let go step by step, nothing over 6 dB
/// in one write, and nothing left held when Audra is gone (OpenCode's job
/// 46: quit put Main back 40 dB at once, with no tick after it).
#[test]
fn quitting_with_the_overrides_on_glides_and_leaves_nothing_held() {
    for (name, extra) in extra_overrides() {
        let mut state = with_overrides_on(extra);
        let main = state.control_room.assign.main * 2;
        let user = state.control_room.dimmed_main.expect("dimmed")[0];
        let phones = state.control_room.assign.phones_pairs()[0];
        let an2 = ChannelId::Input(1);
        let want = state.device.volume(an2, phones).unwrap();
        no_step_over_6_db(&format!("quit, {name}"), &mut state, |s| app::let_go_before_quit(s, |_| {}));
        assert!(!state.control_room.is_dimmed() && !state.control_room.talkback, "{name}: let go");
        assert!(!crate::control_room::is_easing(&state) && state.level_ramps.is_empty(), "{name}: nothing part-way");
        assert!((state.device.outputs()[main].volume - user).abs() < 1e-3, "{name}: Main at your level");
        let hw = state.device.mock_hardware(an2, phones).map_or(want, |h| h.0);
        assert!((hw - want).abs() < 1e-6, "{name}: AN2 in the Phones at its level: {hw}");
    }
    // Nothing on: no wait.
    let mut state = state_with_a_mix();
    let t = Instant::now();
    app::let_go_before_quit(&mut state, |_| panic!("waited with nothing to glide"));
    assert!(t.elapsed() < Duration::from_millis(50));
}

/// A snapshot with another number of channels than the unit is refused
/// before anything is written, and the unit keeps its own channels
/// (OpenCode's job 40).
#[test]
fn a_scene_of_another_shape_is_refused_and_changes_nothing() {
    let mut state = state_with_a_mix();
    let inputs = state.device.inputs().len();
    let mut scene = crate::control_room::capture_scene(&state);
    scene.inputs.pop();
    let levels = app::output_levels(&state);
    let mark = writes(&state).len();
    let e = app::apply_mix_scene_from(&mut state, &scene, &levels).expect_err("refused");
    assert!(e.to_string().contains(&format!("{} inputs", inputs - 1)), "says why: {e}");
    assert_eq!(state.device.inputs().len(), inputs, "the unit's channels stay");
    assert_eq!(writes(&state).len(), mark, "nothing written");
}

/// The other Audra's saved mix, taken in with the overrides on: no moment
/// louder, no step over 6 dB, and no Undo step (OpenCode's job 49: let go
/// in one write and written raw).
#[test]
fn the_other_audras_mix_comes_in_gently_and_is_no_undo_step() {
    for (name, extra) in extra_overrides() {
        let mut state = state_with_a_mix();
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(1), 0, 0.25));
        let their = crate::control_room::capture_scene(&state);
        let mut on = with_overrides_on(extra.clone());
        let undo = on.undo_stack.len();
        no_loud_moment(&format!("sync, {name}"), &mut on, |s| app::sync_other_scene(s, &their));
        assert_eq!(on.undo_stack.len(), undo, "{name}: not an Undo step");
        let mut on = with_overrides_on(extra);
        no_step_over_6_db(&format!("sync, {name}"), &mut on, |s| {
            app::sync_other_scene(s, &their);
            settle_all(s);
        });
        assert!(!on.control_room.is_dimmed());
    }
}

// ── The one-press setups (the audit of 2026-11-17, target/safety-audit) ──

/// The first Phones pair and the Main, as a setup finds them.
fn phones_and_main(state: &TuxMix) -> (usize, usize) {
    let a = &state.control_room.assign;
    (a.phones_pairs().first().copied().expect("a Phones pair"), a.main)
}

/// With Trim Gains on, a setup sets the levels its card names and no
/// other: the computer raised 20 dB in the Phones took its send to the
/// speakers up 20 dB too (glided, and on no card).
#[test]
fn a_setup_with_trim_gains_on_leaves_the_other_mixes_alone() {
    let mut state = state();
    let (phones, main) = phones_and_main(&state);
    let computer = ChannelId::Playback(0);
    let _ = update(&mut state, Message::SetTrimAll(true));
    let _ = state.device.set_volume(computer, phones, vol(-30.0));
    let _ = state.device.set_volume(computer, main, vol(-20.0));
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Podcast));
    app::settle_glides(&mut state);
    let in_phones = state.device.volume(computer, phones).unwrap_or(0.0);
    assert!((db(in_phones) - -10.0).abs() < 0.2, "the computer at -10 dB in the Phones, not {:.1}", db(in_phones));
    let in_main = state.device.volume(computer, main).unwrap_or(0.0);
    assert!((db(in_main) - -20.0).abs() < 0.2, "the computer in the speakers stays at -20 dB, not {:.1} dB", db(in_main));
}

/// A setup moves the strips it names, not their fader group: the mic's
/// group member fell out of Main and rose in the Phones by the mic's travel.
#[test]
fn a_setup_moves_no_group_member() {
    let mut state = state();
    let (phones, main) = phones_and_main(&state);
    let (mic, member) = (ChannelId::Input(0), ChannelId::Input(1));
    state.groups[0] = crate::sidebar::Group { members: vec![mic, member], mute_linked: false, solo_linked: false, fader_linked: true };
    let _ = state.device.set_volume(mic, main, vol(-6.0));
    let _ = state.device.set_volume(mic, phones, vol(-20.0));
    let _ = state.device.set_volume(member, main, vol(-10.0));
    let _ = state.device.set_volume(member, phones, vol(-30.0));
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Podcast));
    app::settle_glides(&mut state);
    let mic_phones = state.device.volume(mic, phones).unwrap_or(0.0);
    assert!((db(mic_phones) - -3.0).abs() < 0.2, "the mic at -3 dB in the Phones, not {:.1}", db(mic_phones));
    let (m_main, m_phones) = (state.device.volume(member, main).unwrap_or(0.0), state.device.volume(member, phones).unwrap_or(0.0));
    assert!((db(m_main) - -10.0).abs() < 0.2, "the member stays at -10 dB in Main, not {:.1}", db(m_main));
    assert!((db(m_phones) - -30.0).abs() < 0.2, "the member stays at -30 dB in the Phones, not {:.1}", db(m_phones));
}

/// No tile steps a level up by more than 6 dB in one write, and a stereo
/// source lands on both its sides in every mix the setup names (Stream:
/// the Phones and the spare at once).
#[test]
fn no_setup_steps_a_level_up_over_6_db_and_stream_lands_in_both_mixes() {
    use crate::setups::Setup;
    for setup in [Setup::Podcast, Setup::Headphones, Setup::Record, Setup::Stream] {
        let mut state = state();
        let (phones, _) = phones_and_main(&state);
        let (mic, computer) = (ChannelId::Input(0), ChannelId::Playback(0));
        let before: Vec<(ChannelId, usize, f32)> =
            [mic, computer].iter().flat_map(|&c| (0..state.device.output_pair_count()).map(move |o| (c, o))).map(|(c, o)| (c, o, state.device.volume(c, o).unwrap_or(0.0))).collect();
        let from = writes(&state).len();
        let _ = update(&mut state, Message::Setup(setup));
        step_glides(&mut state, 12);
        app::settle_glides(&mut state);
        let w = writes_since(&state, from);
        for (c, o, old) in &before {
            if w.iter().any(|x| matches!(x, MockWrite::Volume(wc, wo, _) if wc == c && wo == o)) {
                // From the glide's floor: silence to -60 dB is no step to hear.
                let floor = |v: f32| v.max(vol(-60.0));
                let mut prev = floor(*old);
                for x in &w {
                    if let MockWrite::Volume(wc, wo, v) = x {
                        if wc == c && wo == o {
                            let rise = db(floor(*v)) - db(prev);
                            assert!(rise <= 6.1, "{setup:?}: {c:?} into {o} stepped up {rise:.1} dB");
                            prev = floor(*v);
                        }
                    }
                }
            }
        }
        if setup == Setup::Stream {
            let spare = state.setup_done.as_ref().expect("card").show;
            for out in [phones, spare] {
                let m = state.device.volume(mic, out).unwrap_or(0.0);
                assert!((db(m) - -3.0).abs() < 0.2, "Stream: the mic at -3 dB in {out}, not {:.1}", db(m));
                let c = state.device.volume(computer, out).unwrap_or(0.0);
                assert!((db(c) - -10.0).abs() < 0.2, "Stream: the computer at -10 dB in {out}, not {:.1}", db(c));
                let c2 = state.device.volume(ChannelId::Playback(1), out).unwrap_or(0.0);
                assert!((db(c2) - -10.0).abs() < 0.2, "Stream: the computer's right side at -10 dB in {out}, not {:.1}", db(c2));
            }
        }
    }
}

/// The mix a setup can touch, to compare before and after: every send of
/// the mic and the computer, and each output's loopback.
fn setup_ground(state: &TuxMix) -> Vec<f32> {
    let pairs = state.device.output_pair_count();
    let mut v: Vec<f32> = [ChannelId::Input(0), ChannelId::Playback(0), ChannelId::Playback(1)]
        .iter()
        .flat_map(|&c| (0..pairs).map(move |o| (c, o)))
        .map(|(c, o)| state.device.volume(c, o).unwrap_or(0.0))
        .collect();
    v.extend(state.device.outputs().iter().map(|o| if o.loopback { 1.0 } else { 0.0 }));
    v
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

/// The card's Undo takes back the whole setup, even with the undo stack
/// full: each part's step had pushed an older one out, and Undo took off
/// only the loopback.
#[test]
fn a_setups_undo_takes_back_all_of_it_with_a_full_undo_stack() {
    use crate::setups::Setup;
    for setup in [Setup::Record, Setup::Stream, Setup::Headphones] {
        let mut state = state();
        // Sixty separate steps: the stack is at its cap.
        for k in 0..60 {
            let _ = update(&mut state, Message::Mute(ChannelId::Input(3 + k % 4), k % 8 < 4));
        }
        app::settle_glides(&mut state);
        let before = setup_ground(&state);
        let depth = state.undo_stack.len();
        assert!(depth >= 50, "the stack is full: {depth}");
        let _ = update(&mut state, Message::Setup(setup));
        app::settle_glides(&mut state);
        assert_eq!(state.undo_stack.len(), depth, "{setup:?}: one step, on a full stack");
        assert!(!close(&before, &setup_ground(&state)), "{setup:?} changed the mix");
        let _ = update(&mut state, Message::SetupUndo);
        app::settle_glides(&mut state);
        assert!(close(&before, &setup_ground(&state)), "{setup:?}: all of it back: {before:?} / {:?}", setup_ground(&state));
    }
}

/// The card's Undo never pops a step that isn't the setup's: a Store over
/// a slot and a click on an empty slot push one without changing the mix.
#[test]
fn a_setups_undo_never_pops_another_step() {
    use crate::setups::Setup;
    for other in [Message::SnapshotStoreTo(1), Message::SnapshotClicked(8)] {
        let mut state = state();
        let _ = update(&mut state, Message::Setup(Setup::Podcast));
        app::settle_glides(&mut state);
        let after_setup = setup_ground(&state);
        let _ = update(&mut state, other.clone());
        let depth = state.undo_stack.len();
        let _ = update(&mut state, Message::SetupUndo);
        app::settle_glides(&mut state);
        // Either that step left the card fresh and Undo took the setup
        // back, or it didn't and Undo popped nothing.
        if state.undo_stack.len() != depth {
            assert!(!close(&after_setup, &setup_ground(&state)), "{other:?}: a step went and the setup stayed");
        }
    }
}

/// Under the lock the card's Undo is held, and the card stays.
#[test]
fn a_setups_undo_under_the_lock_keeps_its_card() {
    let mut state = state();
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Podcast));
    app::settle_glides(&mut state);
    let after = setup_ground(&state);
    state.ui_locked = true;
    let _ = update(&mut state, Message::SetupUndo);
    assert!(state.setup_done.is_some(), "the card stays");
    assert!(close(&after, &setup_ground(&state)));
}

/// The same tile twice, then the card's Undo: the setup goes, not just the
/// second press. And the 48V it left waiting goes with the Undo.
#[test]
fn the_same_tile_twice_then_undo_takes_the_setup_back_and_its_48v_arm() {
    let mut state = state();
    let _ = update(&mut state, Message::Phantom(0, false));
    app::settle_glides(&mut state);
    let before = setup_ground(&state);
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Podcast));
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Podcast));
    app::settle_glides(&mut state);
    assert!(state.phantom_armed.is_some(), "48V waits for a click");
    let _ = update(&mut state, Message::SetupUndo);
    app::settle_glides(&mut state);
    assert!(close(&before, &setup_ground(&state)), "the setup is back out");
    assert!(state.phantom_armed.is_none(), "and its 48V no longer waits");
}

/// Headphone mix leaves the user's copied mix on the clipboard: a later
/// Paste pasted Main's mix instead of what they had copied.
#[test]
fn headphone_mix_keeps_the_users_clipboard() {
    let mut state = state();
    let _ = update(&mut state, Message::MixCopy(ChannelId::Input(0)));
    let mine = format!("{:?}", state.mix_clip);
    assert_ne!(mine, "None");
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Headphones));
    app::settle_glides(&mut state);
    assert!(state.setup_done.is_some());
    assert_eq!(format!("{:?}", state.mix_clip), mine);
}

/// Record's mix goes to an optical pair nothing else uses: never the
/// headphone jack when no Phones is assigned, and not a pair that is
/// mirrored while another is free.
#[test]
fn record_picks_an_optical_pair_that_nothing_else_uses() {
    let mut state = state();
    state.control_room.assign.phones = Default::default();
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Record));
    let spare = state.setup_done.as_ref().expect("Record's card").show;
    assert!(spare >= 2, "an optical pair, not the analog jacks: {spare}");
    let mut state = self::state();
    state.mirrors.push((2, 3));
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Record));
    let spare = state.setup_done.as_ref().expect("Record's card").show;
    assert!(spare >= 4, "not a mirror's source or target while another is free: {spare}");
}

/// A pasted pan that swings a heard send to the other side never does it
/// at full level: one ear went from silence to full in one write.
#[test]
fn a_pasted_pan_never_puts_a_side_up_in_one_write() {
    let mut state = state();
    let (phones, main) = phones_and_main(&state);
    let mic = ChannelId::Input(0);
    let _ = state.device.set_volume(mic, main, vol(-6.0));
    let _ = state.device.set_pan(mic, main, 100);
    let _ = state.device.set_volume(mic, phones, vol(-6.0));
    let _ = state.device.set_pan(mic, phones, -100);
    let one = state.device.outputs_one_per_pair();
    let out = |pair: usize| ChannelId::Output(if one { pair } else { pair * 2 });
    let _ = update(&mut state, Message::MixCopy(out(main)));
    let _ = update(&mut state, Message::MixPaste(out(phones)));
    // Right after the paste the pan is over, and the send is down, gliding.
    assert_eq!(state.device.pan(mic, phones).ok(), Some(100));
    let now = state.device.volume(mic, phones).unwrap_or(1.0);
    assert!(db(now) < -40.0, "the send is down while its pan moves, not at {:.1} dB", db(now));
    app::settle_glides(&mut state);
    let landed = state.device.volume(mic, phones).unwrap_or(0.0);
    assert!((db(landed) - -6.0).abs() < 0.2, "and lands at -6 dB, not {:.1}", db(landed));
}

/// Class compliant, the unit has no loopback: Record and Stream say so and
/// change nothing, where the card claimed a loopback that never went on.
#[test]
fn record_and_stream_refuse_without_the_rme_mode() {
    for setup in [crate::setups::Setup::Record, crate::setups::Setup::Stream] {
        let mut state = state();
        state.device.set_protocol_features(false);
        let steps = state.undo_stack.len();
        let before = format!("{:?}", state.device.capture_scene());
        let _ = update(&mut state, Message::Setup(setup));
        assert!(state.setup_done.is_none(), "no card for a setup that did nothing");
        assert!(format!("{:?}", state.device.capture_scene()) == before, "nothing changed");
        assert_eq!(state.undo_stack.len(), steps, "and no undo step for nothing");
        let said = state.notice.as_ref().map(|n| n.0.clone()).unwrap_or_default();
        assert!(said.contains("RME mode"), "it says why: {said:?}");
    }
}

/// A My setup that can't be read changes nothing and leaves no undo step
/// (OpenCode's job 78: the step the press opened stayed, undoing nothing).
#[test]
fn a_my_setup_that_cannot_be_read_leaves_no_undo_step() {
    let mut state = state();
    state.mock_my_setup = None;
    let steps = state.undo_stack.len();
    let _ = update(&mut state, Message::MySetupRecall);
    assert_eq!(state.undo_stack.len(), steps);
    assert!(state.notice.as_ref().is_some_and(|n| n.0.contains("couldn't be read")));
}

/// Under the lock a 48V click changes nothing, so the setup card's waiting
/// 48V stays its own (OpenCode's job 78 asked whether the lock clears it).
#[test]
fn the_lock_keeps_a_setups_waiting_48v() {
    let mut state = state();
    let _ = update(&mut state, Message::Setup(crate::setups::Setup::Podcast));
    let (arm, fresh, steps) = (state.setup_arm, state.setup_fresh, state.setup_steps);
    assert!(arm.is_some(), "the demo mic's 48V is off, so the card asks");
    state.ui_locked = true;
    let _ = update(&mut state, Message::Phantom(arm.unwrap(), true));
    assert_eq!((state.setup_arm, state.setup_fresh, state.setup_steps), (arm, fresh, steps), "held by the lock: the card is as it was");
}

/// A double-click on Settings' "Reset Mix…" bar arms nothing: the list
/// scrolls into view under the pointer, and its first moment is deaf.
#[test]
fn a_reset_cannot_be_armed_as_its_list_opens() {
    let mut state = state();
    let _ = update(&mut state, Message::SettingsResetOpen(true));
    let _ = update(&mut state, Message::ResetMixArm(crate::resetmix::ResetMix::TotalReset));
    assert!(state.reset_armed.is_none(), "the press that came with the opening arms nothing");
    state.settings_reset_opened_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
    let _ = update(&mut state, Message::ResetMixArm(crate::resetmix::ResetMix::TotalReset));
    assert!(state.reset_armed.is_some(), "a moment later, a press arms as before");
}

/// A band that is off wakes as a Bell with the first turn of its gain, at
/// a low, middle or high place if it stood at the untouched 1 kHz; a band
/// already on keeps its type and place (critique #97: on an untouched
/// input the knob moved a number and nothing was drawn or heard).
#[test]
fn an_off_eq_band_wakes_with_its_first_gain_turn() {
    use tuxmix_core::channel::EqBandType;
    let mut state = state();
    let idx = 2; // IN3: an input whose bands start off
    let band = |state: &crate::app::TuxMix, b: usize| state.device.eq_of(idx).expect("an EQ").bands[b];
    let _ = state.device.set_eq_band_type(idx, 0, EqBandType::Off);
    let _ = state.device.set_eq_band_freq(idx, 0, 1000);
    let _ = update(&mut state, Message::EqBandGain(idx, 0, 0.0));
    assert_eq!(band(&state, 0).band_type, EqBandType::Off, "a turn to nothing wakes nothing");
    let _ = update(&mut state, Message::EqBandGain(idx, 0, 3.0));
    assert_eq!(band(&state, 0).band_type, EqBandType::Bell);
    assert_eq!(band(&state, 0).freq_hz, 120, "band 1 wakes in the lows");
    assert!((band(&state, 0).gain_db - 3.0).abs() < 0.11);
    // On already: its type and its place stay.
    let _ = state.device.set_eq_band_type(idx, 2, EqBandType::HighShelf);
    let _ = state.device.set_eq_band_freq(idx, 2, 5000);
    let _ = update(&mut state, Message::EqBandGain(idx, 2, -2.0));
    assert_eq!(band(&state, 2).band_type, EqBandType::HighShelf);
    assert_eq!(band(&state, 2).freq_hz, 5000);
}

/// Rec takes both mic inputs, each on its own channel, when the second is
/// in use; one when it is muted or sent nowhere (the owner's pick after
/// critique #98: a podcast guest on AN2 was never in the take).
#[test]
fn rec_takes_a_second_mic_that_is_in_use() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let n = state.device.output_pair_count();
    for p in 0..n {
        let _ = state.device.set_volume(ChannelId::Input(1), p, 0.0);
    }
    assert_eq!(app::take_target_for_tests(&state).map(|t| t.0), Some(vec![0]), "AN2 sent nowhere: AN1 alone");
    let _ = state.device.set_volume(ChannelId::Input(1), 1, 0.5);
    let (ch, name) = app::take_target_for_tests(&state).expect("a target");
    assert_eq!(ch, vec![0, 1]);
    assert_eq!(name, "AN1 + AN2");
    // Sent only to an output that is muted, it isn't in use (OpenCode's
    // job 81, finding 2).
    let out = ChannelId::Output(crate::assist::pair_ch_for_tests(&state, 1));
    let _ = update(&mut state, Message::Mute(out, true));
    assert_eq!(app::take_target_for_tests(&state).map(|t| t.0), Some(vec![0]), "AN2's only output muted: AN1 alone");
    let _ = update(&mut state, Message::Mute(out, false));
    // From AN2's side it is the same take, left AN1 (once AN1 is sent too).
    let _ = state.device.set_volume(ChannelId::Input(0), 1, 0.5);
    state.quick_channel = ChannelId::Input(1);
    assert_eq!(app::take_target_for_tests(&state).map(|t| t.0), Some(vec![0, 1]));
    // Muted, the guest is left out.
    let _ = update(&mut state, Message::Mute(ChannelId::Input(0), true));
    state.quick_channel = ChannelId::Input(1);
    assert_eq!(app::take_target_for_tests(&state).map(|t| t.0), Some(vec![1]), "AN1 muted: AN2 alone");
}

/// "I can't hear it": a muted mic sent nowhere comes back in one press, as
/// one undo step that takes it all back; with nothing in the way it changes
/// nothing and says where else to look.
#[test]
fn i_cant_hear_it_fixes_what_audra_holds_back() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let pair = crate::quick::listen_pair(&state);
    let mic = ChannelId::Input(0);
    let _ = update(&mut state, Message::Mute(mic, true));
    let _ = state.device.set_volume(mic, pair, 0.0);
    let steps = state.undo_stack.len();
    let _ = update(&mut state, Message::HearFix);
    app::settle_glides(&mut state);
    assert!(!crate::app::strip_params(&state, mic, pair).mute, "unmuted");
    assert!(db(state.device.volume(mic, pair).unwrap_or(0.0)) > -11.0, "sent at about -10 dB");
    assert_eq!(state.undo_stack.len(), steps + 1, "one undo step");
    assert!(state.notice.as_ref().is_some_and(|n| n.0.starts_with("Fixed:")), "{:?}", state.notice);
    let _ = update(&mut state, Message::Undo);
    app::settle_glides(&mut state);
    assert!(crate::app::strip_params(&state, mic, pair).mute, "Undo takes it all back");
    let _ = update(&mut state, Message::Redo);
    app::settle_glides(&mut state);
    // Nothing left in the way: no change, no undo step, and where to look.
    let steps = state.undo_stack.len();
    let _ = update(&mut state, Message::HearFix);
    if crate::hearfix::find(&state, mic, pair).is_empty() {
        assert_eq!(state.undo_stack.len(), steps, "no undo step for nothing");
        assert!(state.notice.as_ref().is_some_and(|n| n.0.starts_with("Nothing in Audra")), "{:?}", state.notice);
    }
}

/// "Start fresh": two presses, one undo step, mics out of every mix and no
/// output turned up; Undo brings the mix back.
#[test]
fn start_fresh_is_a_safe_total_reset_in_one_undo_step() {
    use crate::resetmix::ResetMix;
    let mut state = state();
    let mic = ChannelId::Input(0);
    let _ = state.device.set_volume(mic, 0, 0.7);
    let outs_before: Vec<f32> = state.device.outputs().iter().map(|o| o.volume).collect();
    let steps = state.undo_stack.len();
    let _ = update(&mut state, Message::ResetMixArm(ResetMix::Fresh));
    assert_eq!(state.undo_stack.len(), steps, "armed, nothing changed yet");
    let _ = update(&mut state, Message::ResetMix(ResetMix::Fresh));
    app::settle_glides(&mut state);
    assert_eq!(state.undo_stack.len(), steps + 1, "one undo step");
    assert_eq!(state.device.volume(mic, 0).unwrap_or(1.0), 0.0, "the mic is out of the mix");
    for (o, before) in state.device.outputs().iter().zip(outs_before) {
        assert!(o.volume <= before + 1e-6, "no output turned up: {} > {before}", o.volume);
    }
    let _ = update(&mut state, Message::Undo);
    app::settle_glides(&mut state);
    assert!((state.device.volume(mic, 0).unwrap_or(0.0) - 0.7).abs() < 0.01, "Undo brings the mix back");
}

/// The simulation's Reverb sample: about 3.6 s, never louder than its
/// set peak, the clap alone first, then a tail that rings well past the
/// plain clap's; nothing played in a test (`sounds::play_fx_preview`).
#[test]
fn the_reverb_sample_is_quiet_and_rings_after_its_plain_clap() {
    let mut state = state();
    state.fx.reverb_on = true;
    let s = crate::sounds::fx_preview(&state.fx);
    let secs = s.len() as f32 / 2.0 / 48_000.0;
    assert!((3.4..3.8).contains(&secs), "length {secs} s");
    assert!(s.iter().all(|x| x.is_finite()));
    let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let peak_db = 20.0 * peak.log10();
    assert!((peak_db - crate::sounds::PREVIEW_PEAK_DB).abs() < 0.1, "peak {peak_db} dBFS");
    let rms = |from: f32, to: f32| {
        let (a, b) = ((from * 48_000.0) as usize * 2, (to * 48_000.0) as usize * 2);
        (s[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt()
    };
    // The plain clap is gone 0.3 s in; the effects still ring 0.5 s after
    // theirs (it starts at 0.6 + 0.35 + 0.05 = 1.0 s).
    assert!(rms(0.35, 0.6) < 1e-4, "the plain clap ends: {}", rms(0.35, 0.6));
    assert!(rms(1.5, 1.8) > 10.0 * rms(0.35, 0.6).max(1e-6), "the reverb rings: {}", rms(1.5, 1.8));
    // And nothing reaches the speakers from a test.
    crate::sounds::play_fx_preview(s, 0.0);
}

/// Writes the Reverb sample to target/ideas/fx-sample.wav for measuring
/// (`cargo test -p audra write_the_reverb_sample -- --ignored`); never run
/// by default.
#[test]
#[ignore]
fn write_the_reverb_sample() {
    let mut state = state();
    state.fx.reverb_on = true;
    state.fx.echo_on = true;
    let s = crate::sounds::fx_preview(&state.fx);
    let data: Vec<u8> = s.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&48_000u32.to_le_bytes());
    w.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    let _ = std::fs::create_dir_all("../../target/ideas");
    std::fs::write("../../target/ideas/fx-sample.wav", w).expect("written");
}

/// "I can't hear it" on the speakers with an open mic in them: the mic comes
/// out of the speakers first, then the speakers come up (critique #101 A
/// P2-1: they never did; #99 A P3-13: never raised with the mic in them).
#[test]
fn i_cant_hear_it_takes_an_open_mic_out_of_the_speakers_before_raising_them() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let main = state.control_room.assign.main;
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), main, 0.5);
    let out = ChannelId::Output(crate::assist::pair_ch_for_tests(&state, main));
    let _ = update(&mut state, Message::Mute(out, true));
    let _ = update(&mut state, Message::HearFixOn(main));
    app::settle_glides(&mut state);
    assert_eq!(state.device.volume(ChannelId::Input(0), main).unwrap_or(1.0), 0.0, "the open mic is out of the speakers");
    assert!(!crate::assist::silenced(&state, main), "then the speakers come up");
    assert!(state.notice.as_ref().is_some_and(|n| n.0.contains("can howl")), "{:?}", state.notice);
}

/// After Start fresh (mics out of every mix, 48V kept) a press for the
/// speakers never sends the open mic into them (critique #101 A P1-1).
#[test]
fn i_cant_hear_it_never_sends_an_open_mic_into_the_speakers() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let main = state.control_room.assign.main;
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), main, 0.0);
    let _ = update(&mut state, Message::HearFixOn(main));
    app::settle_glides(&mut state);
    assert_eq!(state.device.volume(ChannelId::Input(0), main).unwrap_or(1.0), 0.0, "the mic stays out of the speakers");
}

/// Speakers turned far down (not silent) come up to -20 dB, never "turned
/// up" in words while they sit at -40 (critique #101 A P3-1).
#[test]
fn i_cant_hear_it_raises_an_output_turned_far_down() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let out = ChannelId::Output(crate::assist::pair_ch_for_tests(&state, phones));
    let _ = state.device.set_volume(out, phones, 0.01);
    let _ = update(&mut state, Message::HearFixOn(phones));
    app::settle_glides(&mut state);
    assert!(db(state.device.volume(out, phones).unwrap_or(0.0)) > -21.0, "up to about -20 dB");
}

/// A hot mic that isn't Quick setup's source is taken out of the speakers
/// too before they come up, muted or not (critique #102 A P2-1, P3-3).
#[test]
fn i_cant_hear_it_takes_every_hot_mic_out_of_the_speakers() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(1);
    let main = state.control_room.assign.main;
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), main, 0.5);
    let _ = update(&mut state, Message::Mute(ChannelId::Input(0), true));
    let out = ChannelId::Output(crate::assist::pair_ch_for_tests(&state, main));
    let _ = update(&mut state, Message::Mute(out, true));
    let _ = update(&mut state, Message::HearFixOn(main));
    app::settle_glides(&mut state);
    assert_eq!(state.device.volume(ChannelId::Input(0), main).unwrap_or(1.0), 0.0, "AN1 out of the speakers, though muted and not the source");
    assert_eq!(state.device.volume(ChannelId::Input(1), main).unwrap_or(1.0), 0.0, "AN2, a mic, not sent in");
    assert!(!crate::assist::silenced(&state, main), "then the speakers come up");
}

/// The headphones press lets go of a solo elsewhere; a hot mic the solo
/// held out of the speakers comes out of them first (critique #103 A P2-2).
#[test]
fn i_cant_hear_it_on_the_headphones_never_lets_a_hot_mic_back_into_the_speakers() {
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), main, 0.5);
    let _ = update(&mut state, Message::Solo(ChannelId::Input(2), true));
    let _ = update(&mut state, Message::HearFixOn(phones));
    app::settle_glides(&mut state);
    assert_eq!(state.device.volume(ChannelId::Input(0), main).unwrap_or(1.0), 0.0, "AN1 out of the speakers before the solo lets go");
}

/// Help's "Keep headphones gentle" opens the Mixer at the Phones' D panel,
/// where Volume protection is (critique #105 A P3-11).
#[test]
fn help_opens_the_headphones_loudness_cap() {
    let mut state = state();
    state.show_about = true;
    let _ = update(&mut state, Message::OpenPhonesCap);
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let cid = ChannelId::Output(crate::assist::pair_ch_for_tests(&state, phones));
    assert!(!state.show_about);
    assert!(state.flyout_open.contains(&(cid, crate::widgets::strip::FlyoutKind::Dynamics)), "{:?}", state.flyout_open);
}

/// With Voice on, Stream sends the shaped voice and keeps the plain mic out
/// of the looped mix, which carries the computer alone (the owner's pick
/// after critique #104 A P2-1/P2-2); the mic still plays in the Phones.
#[test]
fn stream_with_voice_on_keeps_the_plain_mic_out_of_the_looped_mix() {
    use crate::setups::Setup;
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let _ = update(&mut state, Message::QuickShape(true));
    assert!(crate::dynamics_panel::power_of(&state, ChannelId::Input(0)), "Voice on");
    let (phones, _) = phones_and_main(&state);
    let _ = update(&mut state, Message::Setup(Setup::Stream));
    step_glides(&mut state, 12);
    app::settle_glides(&mut state);
    let spare = state.setup_done.as_ref().expect("card").show;
    assert_eq!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(1.0), 0.0, "the plain mic out of the looped mix");
    assert!(state.device.volume(ChannelId::Playback(0), spare).unwrap_or(0.0) > 0.0, "the computer in it");
    assert!(state.device.volume(ChannelId::Input(0), phones).unwrap_or(0.0) > 0.0, "the mic still in the Phones");
    assert!(state.setup_done.as_ref().is_some_and(|d| d.why.contains("Audra processed mic")), "the card names both inputs");
}

/// "Make it heard" never puts an open mic's reverb in the speakers: with a
/// hot mic it raises the headphones' return instead (critique #108 A P1-1).
#[test]
fn make_it_heard_keeps_an_open_mics_reverb_out_of_the_speakers() {
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = update(&mut state, Message::FxMakeHeard(main));
    app::settle_glides(&mut state);
    for _ in 0..200 {
        let _ = update(&mut state, Message::Tick);
    }
    assert!(crate::fx::user_return(&state, main) <= 0.0, "no return in the speakers");
    assert!(crate::fx::user_return(&state, phones) > 0.0 || state.fx_return_ramps.contains_key(&phones), "the headphones' return instead");
}

/// Voice off after Stream-with-Voice puts the plain mic back in the mix to
/// your apps, so the stream never goes voiceless (critique #108 A P2-1).
#[test]
fn voice_off_puts_the_plain_mic_back_in_the_stream() {
    use crate::setups::Setup;
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let _ = update(&mut state, Message::QuickShape(true));
    let _ = update(&mut state, Message::Setup(Setup::Stream));
    step_glides(&mut state, 12);
    app::settle_glides(&mut state);
    let spare = state.setup_done.as_ref().expect("card").show;
    assert_eq!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(1.0), 0.0);
    let _ = update(&mut state, Message::QuickShape(false));
    app::settle_glides(&mut state);
    assert!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(0.0) > 0.0, "the plain mic back in the stream mix");
}

/// "Call on my speakers": the computer (the call) in the speakers, the mic
/// kept out of them (the owner's yes after critique #106 A P3-15).
#[test]
fn call_on_my_speakers_keeps_the_mic_out_and_plays_the_call() {
    use crate::setups::Setup;
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let main = state.control_room.assign.main;
    let _ = state.device.set_volume(ChannelId::Input(0), main, 0.5);
    let _ = update(&mut state, Message::Setup(Setup::SpeakerCall));
    step_glides(&mut state, 12);
    app::settle_glides(&mut state);
    assert_eq!(state.device.volume(ChannelId::Input(0), main).unwrap_or(1.0), 0.0, "the mic out of the speakers");
    assert!(state.device.volume(ChannelId::Playback(0), main).unwrap_or(0.0) > 0.5, "the call in the speakers");
    assert!(state.setup_done.as_ref().is_some_and(|d| d.setup == Setup::SpeakerCall));
}

/// With the speakers' FX return already up, an open mic's reverb is still
/// moved out of them: Make it heard takes the speakers' return down and
/// raises the headphones' (critique #109 A P2-1).
#[test]
fn make_it_heard_takes_an_open_mics_reverb_out_of_speakers_already_up() {
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = update(&mut state, Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
    assert!(crate::fx::user_return(&state, main) > 0.0);
    let _ = update(&mut state, Message::FxMakeHeard(main));
    for _ in 0..200 {
        let _ = update(&mut state, Message::Tick);
    }
    assert!(crate::fx::user_return(&state, main) <= 0.0, "the speakers' return taken down");
    assert!(crate::fx::user_return(&state, phones) > 0.0 || state.fx_return_ramps.contains_key(&phones), "the headphones' return up");
}

/// The reverb's states with an open mic and the speakers say what is true:
/// kept out while nothing plays there, heard in the headphones once Make it
/// heard has run, and "in your speakers" only when the return and the send
/// are both up (critique #109 A P2-1, P2-2).
#[test]
fn an_open_mics_reverb_states_follow_what_plays() {
    use crate::fx::Block;
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    // As after Quick setup: the mic in the headphones, out of the speakers.
    let _ = state.device.set_volume(ChannelId::Input(0), phones, 0.7);
    let _ = state.device.set_volume(ChannelId::Input(0), main, 0.0);
    let _ = update(&mut state, Message::FxOn(crate::fx::Unit::Reverb, true));
    // Nothing sent yet: the plain words, whose key moves it to the
    // headphones anyway (critique #110 B: "kept out" hid the no-send clause).
    assert_eq!(crate::fx::block_in(&state, main), Some(Block::NoSendNoReturn), "nothing up: the plain words");
    assert!(!crate::fx::heard_in(&state, phones), "and not heard in the headphones yet");
    let _ = update(&mut state, Message::FxMakeHeard(main));
    // The return fades in on the clock: land it where the fade ends.
    assert!(state.fx_return_ramps.contains_key(&phones), "the headphones' return fading in");
    let _ = update(&mut state, Message::FxReturn(phones, 10f32.powf(-1.5 / 20.0)));
    assert_eq!(crate::fx::block_in(&state, main), Some(Block::KeptForMic), "still kept out of the speakers");
    assert!(crate::fx::heard_in(&state, phones), "now heard in the headphones: {:?}, return {}, ramps {:?}", crate::fx::block_in(&state, phones), crate::fx::user_return(&state, phones), state.fx_return_ramps.get(&phones));
    let _ = update(&mut state, Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
    assert_eq!(crate::fx::mic_reverb_hazard(&state), Some(main), "raised by hand: a hazard, said");
    assert!(!crate::fx::heard(&state), "and no win: no milestone or flare for it");
    // The speakers' map says it too (OpenCode's job 84).
    let lane = crate::flow::fx_return_lane(&state, main, false).expect("a Reverb lane");
    assert!(lane.note.contains("can howl"), "the map names the hazard: {}", lane.note);
    // Seen from the headphones' mix as well (critique #110 A P2-1).
    state.sel_out = phones;
    assert_eq!(crate::fx::mic_reverb_hazard(&state), Some(main));
    assert!(crate::sidebar::fx_unheard(&state).is_some(), "the placard row says it whatever mix is edited");
}

/// Make it heard pressed for the headphones' mix while the speakers' return
/// is up and an open mic feeds the send: the speakers' return comes down
/// too (critique #110 A P2-1).
#[test]
fn make_it_heard_from_the_headphones_takes_the_speakers_reverb_down() {
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), phones, 0.7);
    let _ = update(&mut state, Message::FxOn(crate::fx::Unit::Reverb, true));
    let _ = update(&mut state, Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
    state.sel_out = phones;
    let _ = update(&mut state, Message::FxMakeHeard(phones));
    assert!(crate::fx::user_return(&state, main) <= 0.0, "the speakers' return down");
    assert_eq!(crate::fx::mic_reverb_hazard(&state), None);
}

/// Undo after Voice went off can take the plain mic back out of the mix to
/// your apps: said, with Redo, never left silent (critique #111 A P2-2,
/// both reviewers).
#[test]
fn undo_that_takes_the_voice_out_of_the_stream_says_so() {
    use crate::setups::Setup;
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let _ = update(&mut state, Message::QuickShape(true));
    let _ = update(&mut state, Message::Setup(Setup::Stream));
    step_glides(&mut state, 12);
    app::settle_glides(&mut state);
    let spare = state.setup_done.as_ref().expect("card").show;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::VolumeChanged(ChannelId::Playback(0), phones, 0.6));
    let _ = update(&mut state, Message::QuickShape(false));
    app::settle_glides(&mut state);
    assert!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(0.0) > 0.0, "Voice off: the plain mic back in the stream");
    state.notice = None;
    // Undo until the mic is out of the stream mix again.
    for _ in 0..4 {
        if state.device.volume(ChannelId::Input(0), spare).unwrap_or(0.0) <= 0.0 {
            break;
        }
        let _ = update(&mut state, Message::Undo);
        app::settle_glides(&mut state);
    }
    assert_eq!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(1.0), 0.0, "Undo took it out (the user's step stands)");
    let said = state.notice.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();
    assert!(said.contains("your stream won\u{2019}t hear you") && said.contains("Redo"), "said: {said:?}");
}

/// With the faders on FX sends and the page closed, an open mic's reverb in
/// the speakers is said on that placard too, with its press; and Mute FX's
/// release there doesn't lead into it (critique #111 A P2-1, both).
#[test]
fn the_fx_sends_placard_says_an_open_mics_reverb_in_the_speakers() {
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), phones, 0.7);
    let _ = update(&mut state, Message::FxOn(crate::fx::Unit::Reverb, true));
    let _ = update(&mut state, Message::FxMakeHeard(main));
    let _ = update(&mut state, Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
    assert_eq!(crate::fx::mic_reverb_hazard(&state), Some(main));
    state.fader_fx = true;
    state.sidebar_tab = None;
    let (head, _, _) = crate::sidebar::fx_sends_head(&state);
    assert!(head.contains("open mic"), "the head says it: {head}");
    assert!(crate::keyfocus::counter_items(&state).contains(&crate::keyfocus::CounterItem::FxUnheard), "its row is a keyboard stop");
    // Mute FX holding it: the second key moves it rather than releasing it.
    let _ = update(&mut state, Message::MuteFxToggle);
    assert_eq!(crate::fx::mic_reverb_hazard(&state), None, "held");
    assert_eq!(crate::fx::mute_holds_mic_reverb(&state), Some(main));
    assert!(matches!(crate::sidebar::fx_sends_second(&state), Some(Message::FxMakeHeard(p)) if p == main), "move, not release");
    let _ = update(&mut state, Message::FxMakeHeard(main));
    assert_eq!(crate::fx::mute_holds_mic_reverb(&state), None, "the speakers' kept return taken down");
    let _ = update(&mut state, Message::MuteFxToggle);
    assert_eq!(crate::fx::mic_reverb_hazard(&state), None, "released: nothing in the speakers");
}

/// The reverb guide's step 3 doesn't tick on the hazard (critique #111
/// Fable A P3-1), and "Edit the headphones' mix" leaves FX sends (#110
/// P3-7).
#[test]
fn the_hazard_is_no_guide_step_and_editing_the_headphones_leaves_fx_sends() {
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), phones, 0.7);
    let _ = update(&mut state, Message::FxOn(crate::fx::Unit::Reverb, true));
    let _ = update(&mut state, Message::FxSend(-10.0));
    let _ = update(&mut state, Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
    state.sel_out = main;
    assert_eq!(crate::fx::mic_reverb_hazard(&state), Some(main));
    let g = crate::guides::GUIDES.iter().position(|g| g.id == "reverb").expect("the reverb guide");
    let run = crate::guides::Run::new(g, &state);
    assert!(!(crate::guides::GUIDES[g].steps[2].done)(&state, &run), "step 3 not done on the hazard");
    state.fader_fx = true;
    let _ = update(&mut state, Message::EditMix(phones));
    assert!(!state.fader_fx && state.sel_out == phones, "the headphones' mix, its faders on the mix");
}

/// My setup brought back after Voice went off can take the plain mic out
/// of the mix to your apps: said, last, over "My setup is back." (critique
/// #112 A P3-1, Fable A P2-1).
#[test]
fn my_setup_that_takes_the_voice_out_of_the_stream_says_so() {
    use crate::setups::Setup;
    let mut state = state();
    state.quick_channel = ChannelId::Input(0);
    let _ = update(&mut state, Message::QuickShape(true));
    let _ = update(&mut state, Message::Setup(Setup::Stream));
    step_glides(&mut state, 12);
    app::settle_glides(&mut state);
    let spare = state.setup_done.as_ref().expect("card").show;
    let _ = update(&mut state, Message::MySetupSave);
    let _ = update(&mut state, Message::QuickShape(false));
    app::settle_glides(&mut state);
    assert!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(0.0) > 0.0, "Voice off: the plain mic back in the stream");
    let _ = update(&mut state, Message::MySetupRecall);
    app::settle_glides(&mut state);
    assert_eq!(state.device.volume(ChannelId::Input(0), spare).unwrap_or(1.0), 0.0, "My setup took it out (the user's press stands)");
    let said = state.notice.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();
    assert!(said.contains("your stream won\u{2019}t hear you"), "said: {said:?}");
}

/// While Mute FX holds an open mic's reverb, the Reverb & Echo page offers
/// the move, not a release into the speakers (critique #112 A P3-3, Fable A
/// P3-1).
#[test]
fn the_page_moves_an_open_mics_reverb_rather_than_releasing_it() {
    let mut state = state();
    let main = state.control_room.assign.main;
    let phones = *state.control_room.assign.phones_pairs().first().expect("a Phones pair");
    let _ = update(&mut state, Message::Gain(0, 30));
    let _ = state.device.set_volume(ChannelId::Input(0), phones, 0.7);
    let _ = update(&mut state, Message::FxOn(crate::fx::Unit::Reverb, true));
    let _ = update(&mut state, Message::FxMakeHeard(main));
    let _ = update(&mut state, Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
    let _ = update(&mut state, Message::MuteFxToggle);
    state.sel_out = main;
    let (label, press) = crate::fx::page_key_for_tests(&state).expect("a key");
    assert_eq!(label, "Move it to the headphones");
    assert!(matches!(press, Message::FxMakeHeard(p) if p == main));
}

#[test]
fn an_osc_command_for_a_channel_the_unit_hasnt_got_is_dropped() {
    // Security review: anyone on this computer can send OSC while it's on.
    // A locked mix used to echo `/input/9999/mute` back and crash on the
    // channel that isn't there; now it's dropped before anything sees it.
    use crate::osc::OscCommand;
    for locked in [false, true] {
        let mut state = state();
        state.ui_locked = locked;
        let before = writes(&state).len();
        for cmd in [
            OscCommand::Mute(ChannelId::Input(9999), true),
            OscCommand::Mute(ChannelId::Playback(usize::MAX), true),
            OscCommand::Solo(ChannelId::Output(9999), true),
            OscCommand::Volume(ChannelId::Input(0), 9999, 1.0),
            OscCommand::Pan(ChannelId::Playback(0), usize::MAX, 0),
            OscCommand::OutputVolume(9999, 1.0),
            OscCommand::Gain(9999, 30),
            OscCommand::Phantom(9999, true),
            OscCommand::Loopback(9999, true),
        ] {
            assert!(!app::osc_in_range(&state, &cmd), "{cmd:?}");
            let _ = update(&mut state, Message::OscCommand(cmd));
        }
        assert_eq!(writes(&state).len(), before, "nothing reached the card");
    }
    // A real channel still passes.
    let state = state();
    assert!(app::osc_in_range(&state, &OscCommand::Mute(ChannelId::Input(0), true)));
    assert!(app::osc_in_range(&state, &OscCommand::Volume(ChannelId::Input(0), 0, 0.5)));
}

#[test]
fn a_crafted_snapshot_cant_change_what_an_input_is_or_switch_loop_on() {
    // Security review (high): a snapshot or workspace someone sends could
    // call AN1 an instrument, so the USB backend wrote its gain in the
    // instrument's code (about 54 dB on the mic preamp for 9 on screen).
    // What an input is now always comes from the unit.
    use tuxmix_core::channel::ChannelType;
    let mut state = state();
    let ty = state.device.inputs()[0].channel_type;
    let max = state.device.inputs()[0].gain_max;
    let mut scene = state.device.capture_scene();
    scene.inputs[0].channel_type = if ty == ChannelType::Mic { ChannelType::Instrument } else { ChannelType::Mic };
    scene.inputs[0].gain_max = Some(999);
    scene.inputs[0].gain = Some(500);
    scene.inputs[0].id = 77;
    // LOOP and the clock, which wait for the user's own presses.
    let pairs = state.device.outputs().len() / 2;
    for o in scene.outputs.iter_mut() {
        o.loopback = true;
    }
    let clock = state.device.settings().clock_source.clone();
    scene.settings.clock_source = "Optical".into();
    // An EQ past anything the panel allows.
    if let Some(eq) = scene.inputs[0].eq.as_mut() {
        eq.bands[0].gain_db = 99.0;
        eq.bands[1].q = f32::NAN;
    }
    app::apply_mix_scene(&mut state, &scene).expect("the scene applies");
    step_glides(&mut state, 40);
    let a = &state.device.inputs()[0];
    assert_eq!(a.channel_type, ty, "the unit's own type");
    assert_eq!(a.gain_max, max, "the unit's own range");
    assert_eq!(a.id, 0);
    if let (Some(g), Some(m)) = (a.gain, max) {
        assert!(g <= m, "gain {g} over the unit's {m}");
    }
    if let Some(eq) = &a.eq {
        assert!(eq.bands[0].gain_db <= 20.0 && eq.bands[1].q.is_finite(), "{eq:?}");
    }
    assert!(state.device.outputs().iter().all(|o| !o.loopback), "LOOP only ever goes off from a file ({pairs} pairs)");
    assert_eq!(state.device.settings().clock_source, clock, "the clock stays the unit's");
}

#[test]
fn a_snapshot_with_the_wrong_number_of_mixes_is_refused_before_any_write() {
    // Security review: empty level lists crashed the mixer on the next
    // frame; too-long ones stopped a load half way.
    for (vols, pans) in [(0usize, 0usize), (99, 99)] {
        let mut state = state();
        let mut scene = state.device.capture_scene();
        scene.inputs[0].volumes = vec![0.5; vols];
        scene.inputs[0].pans = vec![0; pans];
        let before = writes(&state).len();
        assert!(app::apply_mix_scene(&mut state, &scene).is_err(), "{vols} mixes refused");
        assert_eq!(writes(&state).len(), before, "nothing written");
        // And the mixer still draws its strips (it indexed the lists).
        let _ = app::channel_is_muted(&state, ChannelId::Input(0));
        assert!(!state.device.inputs()[0].volumes.is_empty());
    }
}

#[test]
fn osc_cant_switch_48v_or_loop_on_by_itself() {
    // Security review: anyone on this computer can send OSC while it's on.
    // 48V (with its confirmation on, as it is by default) and LOOP wait for
    // the user's own presses; switching them off from OSC still works.
    use crate::osc::OscCommand;
    let mut state = state();
    assert!(state.confirm_48v);
    let mic = (0..state.device.inputs().len())
        .find(|&i| state.device.inputs()[i].channel_type == tuxmix_core::channel::ChannelType::Mic)
        .expect("a mic input");
    assert!(!state.device.inputs()[mic].phantom);
    let _ = update(&mut state, Message::OscCommand(OscCommand::Phantom(mic, true)));
    let _ = update(&mut state, Message::OscCommand(OscCommand::Phantom(mic, true)));
    assert!(!state.device.inputs()[mic].phantom, "48V stays off");
    assert!(state.notice.as_ref().is_some_and(|(t, _)| t.contains("48V")), "and the screen says why");
    let _ = update(&mut state, Message::OscCommand(OscCommand::Loopback(0, true)));
    assert!(!state.device.outputs()[0].loopback, "LOOP stays off");
    // With the confirmation switched off by the user, OSC may switch 48V.
    state.confirm_48v = false;
    let _ = update(&mut state, Message::OscCommand(OscCommand::Phantom(mic, true)));
    assert!(state.device.inputs()[mic].phantom);
    let _ = update(&mut state, Message::OscCommand(OscCommand::Phantom(mic, false)));
    assert!(!state.device.inputs()[mic].phantom, "off is always allowed");
}

#[test]
fn a_burst_of_small_osc_raises_glides_like_one_big_jump() {
    // Security review: 22 messages of +5.9 dB, with no tick between them,
    // took a send from silence to +6 dB at once; only one big jump glided.
    use crate::osc::OscCommand;
    let mut state = state();
    let cid = ChannelId::Input(0);
    let start = vol(-60.0);
    let _ = update(&mut state, Message::VolumeChanged(cid, 0, start));
    let before = writes(&state).len();
    let mut db = -60.0f32;
    for _ in 0..22 {
        db = (db + 5.9).min(6.0);
        let _ = update(&mut state, Message::OscCommand(OscCommand::Volume(cid, 0, vol(db))));
    }
    let burst = writes_since(&state, before);
    let rise = biggest_rise(&burst, cid, 0, start);
    assert!(rise <= RAMP_OVER_DB + 0.5, "the burst wrote {rise:.1} dB over the start at once");
    step_glides(&mut state, 40);
    let all = writes_since(&state, before);
    let rise = biggest_rise(&all, cid, 0, start);
    assert!(rise <= RAMP_OVER_DB + 0.5, "the glide stepped {rise:.1} dB at once");

    // The preamp gain the same way: 11 messages of +6 dB.
    let mut st = self::state();
    let mic = (0..st.device.inputs().len())
        .find(|&i| st.device.inputs()[i].gain.is_some())
        .expect("an input with gain");
    let _ = update(&mut st, Message::Gain(mic, 0));
    let mut g = 0;
    for _ in 0..11 {
        g += 6;
        let _ = update(&mut st, Message::OscCommand(OscCommand::Gain(mic, g)));
    }
    let now = st.device.inputs()[mic].gain.unwrap_or(0);
    assert!(now <= 12, "the burst raised the preamp to {now} dB at once (a glide takes it from there)");
}

#[test]
fn a_remote_cant_push_a_running_glides_target_into_one_big_write() {
    // The second security review (blocker): a glide nearly done, then ten
    // raises of its target in one frame; the next tick wrote 58 dB at once.
    // Now no glide write lands more than RAMP_STEP over the fader's level.
    use crate::osc::OscCommand;
    for via_midi in [false, true] {
        let mut state = state();
        let cid = ChannelId::Input(0);
        let start = vol(-60.0);
        let _ = update(&mut state, Message::VolumeChanged(cid, 0, start));
        let before = writes(&state).len();
        let send = |state: &mut TuxMix, db: f32| {
            if via_midi {
                state.from_remote = true;
                let _ = update(state, Message::VolumeChanged(cid, 0, vol(db)));
                state.from_remote = false;
            } else {
                let _ = update(state, Message::OscCommand(OscCommand::Volume(cid, 0, vol(db))));
            }
        };
        send(&mut state, -53.5);
        step_glides(&mut state, 9);
        let mut db = -53.5f32;
        for _ in 0..10 {
            db = (db + 5.9).min(6.0);
            send(&mut state, db);
        }
        step_glides(&mut state, 60);
        let all = writes_since(&state, before);
        let rise = biggest_rise(&all, cid, 0, start);
        assert!(rise <= RAMP_OVER_DB + 0.5, "{} wrote {rise:.1} dB at once", if via_midi { "MIDI" } else { "OSC" });
        // And it still gets there.
        let end = state.device.volume(cid, 0).unwrap();
        assert!((20.0 * end.log10() - 6.0).abs() < 0.6, "ended at {:.1} dB", 20.0 * end.log10());
    }
}

#[test]
fn small_osc_raises_spread_over_mixes_or_pair_sides_dont_add_up() {
    // The second security review: counted per level, six +5.9 dB messages
    // to six mixes with Trim Gains on took a send up 35 dB at once, and
    // alternating a linked output's two sides added 11.8 dB. Now one burst
    // has one 6 dB budget for every level together; the rest glides.
    use crate::osc::OscCommand;
    let db = |v: f32| 20.0 * v.max(1e-6).log10();
    let mut state = state();
    state.trim_all = true;
    let cid = ChannelId::Input(0);
    let mixes = state.device.inputs()[0].volumes.len();
    for out in 0..mixes {
        let _ = update(&mut state, Message::VolumeChanged(cid, out, vol(-40.0)));
    }
    for out in 0..mixes {
        let _ = update(&mut state, Message::OscCommand(OscCommand::Volume(cid, out, vol(-40.0 + 5.9 * (out as f32 + 1.0)))));
    }
    for out in 0..mixes {
        let now = db(state.device.volume(cid, out).unwrap());
        assert!(now <= -40.0 + RAMP_OVER_DB + 0.5, "mix {out} rose to {now:.1} dB at once");
    }
    // Main's two sides, alternately.
    let mut st = self::state();
    let _ = update(&mut st, Message::VolumeChanged(ChannelId::Output(0), 0, vol(-40.0)));
    let _ = update(&mut st, Message::VolumeChanged(ChannelId::Output(1), 1, vol(-40.0)));
    let mut level = -40.0f32;
    for i in 0..6 {
        level += 5.9;
        let _ = update(&mut st, Message::OscCommand(OscCommand::OutputVolume(i % 2, vol(level))));
    }
    for o in 0..2 {
        let now = db(st.device.outputs()[o].volume);
        assert!(now <= -40.0 + RAMP_OVER_DB + 0.5, "Main side {o} rose to {now:.1} dB at once");
    }
}

#[test]
fn my_setup_says_when_loop_stays_off() {
    // Opus's re-check: a recall never switches LOOP on (a file could), and
    // My setup's own "My setup is back." covered the sentence saying so.
    let mut state = state();
    let _ = state.device.set_loopback(0, true);
    let _ = update(&mut state, Message::MySetupSave);
    let _ = state.device.set_loopback(0, false);
    let _ = update(&mut state, Message::MySetupRecall);
    assert!(!state.device.outputs()[0].loopback, "LOOP stays off");
    let text = state.notice.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
    assert!(text.contains("My setup is back") && text.contains("LOOP stays off"), "{text}");
}
