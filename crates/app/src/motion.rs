//! Motion (the owner, 2026-11-03: "add ... fun animations when moving
//! around"). Two, each short and each meaning something:
//!
//! - **A row opening.** While one row shows at a time, the row you open
//!   grows its faders into the room the others left, over `FLOOR_OPEN`,
//!   easing out. The row that closes folds at once: exits are quicker than
//!   entrances.
//! - **An LED warming up.** A toggle that lights (M, S, 48V, a Control Room
//!   key) flares wider and brighter than its resting glow, then settles,
//!   over `BLOOM`, like the unit's own LEDs.
//!
//! - **An LED cooling** (2026-11-03, the owner: "Ui should give more
//!   feedback ... subtle and fun"). A toggle that goes off fades from lit
//!   to rest over `COOL` instead of blinking out.
//! - **A breath.** A key waiting for its second click (48V?, LOOP?)
//!   breathes until the click comes or the wait runs out (`breath`).
//! - **The padlock's nudge.** A change the lock holds back shakes the top
//!   bar's padlock (`lock_nudge`), so the eye goes where the answer is.
//!
//! Nothing runs continuously: each finishes within half a second or ends
//! with its wait (an arm lasts `app::PHANTOM_ARM_FOR`), and the fast redraw
//! timer runs only while one is in flight (`running`). Audra settings'
//! "Animations" switch turns them off (the desktop gives iced no
//! reduced-motion signal to follow).

use std::time::{Duration, Instant};

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};

pub const FLOOR_OPEN: Duration = Duration::from_millis(260);
/// Store's sweep: each slot's coral edge comes up over `SWEEP_ONE`, the
/// next `SWEEP_STAGGER` later, so the edges run across the slots in
/// reading order (TotalMix flashes them; Audra lights them once).
pub const SWEEP_ONE: Duration = Duration::from_millis(140);
pub const SWEEP_STAGGER: Duration = Duration::from_millis(28);
/// The longest sweep: eight slots.
const SWEEP_ALL: Duration = Duration::from_millis(140 + 7 * 28);
pub const BLOOM: Duration = Duration::from_millis(220);
/// A light going out: a little longer than it took to come on, as a
/// filament cools slower than it heats.
pub const COOL: Duration = Duration::from_millis(280);
/// One breath of a key waiting for a second click.
pub const BREATH: Duration = Duration::from_millis(1300);
/// The padlock's shake.
pub const NUDGE: Duration = Duration::from_millis(420);
/// A strip's side panel (settings, EQ, trim) sliding out (2026-11-03, the
/// owner: "the panel just shoots out, almost no animation at all").
pub const PANEL_OPEN: Duration = Duration::from_millis(220);

/// A step just done in Quick: its tick drawn in, as by a pen.
pub const TICK_DRAW: Duration = Duration::from_millis(320);
/// Quick's level band coming up to its glow as a voice lands in it.
pub const BAND_GLOW: Duration = Duration::from_millis(300);
/// Reverb or Echo heard for the first time since it was silent: its On key
/// flares, longer than a press's bloom (the moment the sound comes in).
pub const HEARD_FLARE: Duration = Duration::from_millis(700);

/// How far Quick's step `i` has drawn its tick in: 1 when settled or with
/// animations off.
pub fn tick_drawn(state: &TuxMix, i: usize) -> f32 {
    pen(tick_progress(state, i))
}

/// How far through its `TICK_DRAW` Quick's step `i` is (1 when settled or
/// with animations off; a screenshot holds `capture_burst_t`).
fn tick_progress(state: &TuxMix, i: usize) -> f32 {
    match state.quick_done_at.get(i).copied().flatten() {
        Some(_) if state.animations && state.capture_burst_t.is_some() => state.capture_burst_t.unwrap_or(1.0),
        Some(at) if state.animations => progress(at, TICK_DRAW, Instant::now()),
        _ => 1.0,
    }
}

/// A pen's pace: easing in and out (cubic), so the stroke is seen being
/// drawn across its time rather than popping in.
pub fn pen(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// How bright Quick's step `i`'s badge flashes as its tick draws in: up
/// quickly, then settling as the tick lands (0 at rest).
pub fn tick_flash(state: &TuxMix, i: usize) -> f32 {
    let p = tick_progress(state, i);
    if p >= 1.0 {
        0.0
    } else if p < 0.15 {
        p / 0.15
    } else {
        let k = (p - 0.15) / 0.85;
        1.0 - k * k * (3.0 - 2.0 * k)
    }
}

/// How lit Quick's level band is: 0 out of it, up to 1 while a voice sits
/// in it (at once with animations off).
pub fn band_glow(state: &TuxMix) -> f32 {
    match state.quick_band_at {
        Some(at) if state.animations => ease_out(progress(at, BAND_GLOW, Instant::now())),
        Some(_) => 1.0,
        None => 0.0,
    }
}

/// How bright the effects' On keys flare as they're first heard: 1 the
/// moment, easing to 0.
pub fn heard_flare(state: &TuxMix) -> f32 {
    let p = match state.fx_heard_at {
        Some(_) if state.animations && state.capture_burst_t.is_some() => state.capture_burst_t.unwrap_or(1.0),
        Some(at) if state.animations => progress(at, HEARD_FLARE, Instant::now()),
        _ => return 0.0,
    };
    flare_curve(p)
}

/// The flare's envelope: held at its height for the first ~200ms (longer
/// than a press's whole bloom), then a slow, even fade.
pub fn flare_curve(p: f32) -> f32 {
    const HOLD: f32 = 0.28;
    if p >= 1.0 {
        0.0
    } else if p < HOLD {
        1.0
    } else {
        let k = (p - HOLD) / (1.0 - HOLD);
        1.0 - k * k * (3.0 - 2.0 * k)
    }
}

/// A gain readout lighting when the unit changed the gain (its wheel).
pub const GAIN_FLASH: Duration = Duration::from_millis(1100);

/// How long Start over stays outlined after Help's Show me.
pub const START_OVER_LIT: Duration = Duration::from_millis(1600);

/// How strongly Start over is outlined: 1 when Help's Show me lands there,
/// easing to 0.
pub fn phones_cap_lit(state: &TuxMix) -> f32 {
    state.phones_cap_lit_at.map_or(0.0, |at| 1.0 - ease_out(progress(at, START_OVER_LIT, Instant::now())))
}

pub fn start_over_lit(state: &TuxMix) -> f32 {
    state.start_over_lit_at.map_or(0.0, |at| 1.0 - ease_out(progress(at, START_OVER_LIT, Instant::now())))
}

/// How bright input `idx`'s gain readout is lit: 1 the moment the unit
/// changed its gain, easing to 0.
pub fn gain_flash(state: &TuxMix, idx: usize) -> f32 {
    state
        .gain_flash
        .get(&idx)
        .map_or(0.0, |&at| 1.0 - ease_out(progress(at, GAIN_FLASH, Instant::now())))
}

/// How far strip `cid`'s `kind` panel has slid out: 0 to 1 while it
/// opens, 1 when settled or with animations off. Closing is instant: exits
/// are quicker than entrances.
pub fn panel_reveal(state: &TuxMix, cid: ChannelId, kind: crate::widgets::strip::FlyoutKind) -> f32 {
    match state.panel_anim {
        Some((which, at)) if which == (cid, kind) && state.animations => {
            ease_out(progress(at, PANEL_OPEN, Instant::now()))
        }
        _ => 1.0,
    }
}

/// A Matrix patch's pulse (Fun pass 2): the patched cell blooms, and a
/// pulse runs out from it along its row and its column, a cell every
/// `PATCH_STEP`, fading as it goes.
pub const PATCH_STEP: Duration = Duration::from_millis(45);
pub const PATCH_FOR: Duration = Duration::from_millis(620);

/// A channel's row in the Matrix, as a distance: the inputs, then the
/// playback, then the outputs' row.
fn matrix_row(state: &TuxMix, cid: ChannelId) -> i32 {
    let ins = state.device.inputs().len() as i32;
    let pbs = state.device.playbacks().len() as i32;
    match cid {
        ChannelId::Input(i) => i as i32,
        ChannelId::Playback(i) => ins + i as i32,
        ChannelId::Output(_) => ins + pbs,
    }
}

/// For a screenshot: how far into Fun pass 2's pulses to hold them, in ms
/// (0: not held). Set by `main` for `--open patch-pulse` and `path-pulse`.
static FROZEN_PULSE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn freeze_pulses_at(ms: u64) {
    FROZEN_PULSE_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
}

/// The time since `at`, or the held time for a screenshot.
fn since(at: Instant) -> Duration {
    match FROZEN_PULSE_MS.load(std::sync::atomic::Ordering::Relaxed) {
        0 => at.elapsed(),
        ms => Duration::from_millis(ms),
    }
}

/// The time since `at`, held for a screenshot as the pulses are.
pub fn since_held(at: Instant) -> Duration {
    since(at)
}

/// How bright the patch pulse is at cell (`cid`, output pair `pair`) now.
pub fn matrix_pulse(state: &TuxMix, cid: ChannelId, pair: usize) -> f32 {
    let Some((from, from_pair, at)) = state.matrix_pulse else {
        return 0.0;
    };
    let elapsed = since(at);
    if elapsed >= PATCH_FOR {
        return 0.0;
    }
    let fade = 1.0 - (elapsed.as_secs_f32() / PATCH_FOR.as_secs_f32()).min(1.0);
    if cid == from && pair == from_pair {
        // The cell itself blooms.
        return fade;
    }
    let d = if cid == from {
        (pair as i32 - from_pair as i32).abs()
    } else if pair == from_pair {
        (matrix_row(state, cid) - matrix_row(state, from)).abs()
    } else {
        return 0.0;
    };
    let front = elapsed.as_secs_f32() / PATCH_STEP.as_secs_f32();
    ((1.0 - (d as f32 - front).abs()).max(0.0) * fade * 0.8).max(0.0)
}

/// The active path (Fun pass 2): when another mix is picked, the strips
/// that feed it light up in turn, a strip every `PATH_STEP`, over the
/// lamp coming up.
// 12 ms a strip, the wave done in 0.7 s (critique #50: at 30 ms it ran
// 1.14 s, the longest routine motion, again on every mix step).
pub const PATH_STEP: Duration = Duration::from_millis(12);
pub const PATH_FLARE: Duration = Duration::from_millis(420);
pub const PATH_ALL: Duration = Duration::from_millis(420 + 24 * 12);

/// How bright strip `cid`'s path flare is now: its place among the strips
/// feeding the mix delays it.
pub fn path_pulse(state: &TuxMix, cid: ChannelId) -> f32 {
    if !state.animations || state.path_quiet {
        return 0.0;
    }
    let Some((from, at)) = state.lamp_from else {
        return 0.0;
    };
    if from == state.sel_out || since(at) >= PATH_ALL || !crate::app::feeds_mix(state, cid, state.sel_out) {
        return 0.0;
    }
    let order = crate::app::mixer_strip_order(state);
    let place = order
        .iter()
        .filter(|&&c| crate::app::feeds_mix(state, c, state.sel_out))
        .position(|&c| c == cid)
        .unwrap_or(0);
    let delay = PATH_STEP * place as u32;
    let elapsed = since(at);
    if elapsed < delay {
        return 0.0;
    }
    flare_curve(((elapsed - delay).as_secs_f32() / PATH_FLARE.as_secs_f32()).min(1.0))
}

/// How long the first good level's strip glow runs.
pub const LIVE_WASH: Duration = Duration::from_millis(900);

/// How long a small win's sparks fly on its strip (`win_spark`).
pub const WIN_SPARK: Duration = Duration::from_millis(700);

/// A small win's sparks on the strip it happened on (`TuxMix::win_spark`):
/// the reward at the place of the action, the card being the receipt
/// (critique #55's question; the owner's "like Bejeweled"). A few of the
/// meters' squares fly up from the strip's head (`celebrate::StripSparks`),
/// not the lamp's glow, which says "this strip feeds the mix" (critique
/// #57, the owner's yes). 0..1 through `WIN_SPARK`, `None` when none fly.
/// A linked pair's strip sparks for either side.
pub fn win_spark(state: &TuxMix, cid: ChannelId) -> Option<f32> {
    let (on, at) = state.win_spark?;
    if !state.animations || since(at) >= WIN_SPARK {
        return None;
    }
    let shown = |c: ChannelId| match c {
        ChannelId::Input(i) if state.device.input_pair_linked(i / 2) => ChannelId::Input(i & !1),
        c => c,
    };
    if shown(on) != shown(cid) {
        return None;
    }
    Some((since(at).as_secs_f32() / WIN_SPARK.as_secs_f32()).min(1.0))
}

/// The strip a message acts on, where it acts on one (`win_spark`).
pub fn strip_of(message: &Message) -> Option<ChannelId> {
    match message {
        Message::Mute(cid, _) | Message::Solo(cid, _) | Message::PhaseChanged(cid, _) | Message::CueChanged(cid, _) | Message::ToggleCollapse(cid) => Some(*cid),
        Message::VolumeChanged(cid, _, _) => Some(*cid),
        Message::Phantom(i, _) | Message::Pad(i, _) => Some(ChannelId::Input(*i)),
        Message::LoopbackChanged(i, _) => Some(ChannelId::Output(*i)),
        _ => None,
    }
}

/// The first good level's glow on its strip (`TuxMix::live_wash_at`): the
/// Quick setup's input, once a session, in the Mixer.
pub fn live_wash(state: &TuxMix, cid: ChannelId) -> f32 {
    let Some(at) = state.live_wash_at else { return 0.0 };
    if !state.animations || since(at) >= LIVE_WASH {
        return 0.0;
    }
    // The strip shown for the input (a linked pair's left).
    let shown = match state.quick_channel {
        ChannelId::Input(i) if state.device.input_pair_linked(i / 2) => ChannelId::Input(i & !1),
        c => c,
    };
    if cid != shown {
        return 0.0;
    }
    flare_curve((since(at).as_secs_f32() / LIVE_WASH.as_secs_f32()).min(1.0))
}

/// What can bloom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bloom {
    Mute(ChannelId),
    Solo(ChannelId),
    Phantom(usize),
    /// A Control Room key, by its label.
    Key(&'static str),
    /// An output's CUE, by its pair (as `ControlRoom::cue`), and its LOOP,
    /// by its channel: they popped on and off beside M's bloom (the
    /// reviewer, 2026-11-14).
    Cue(usize),
    Loop(usize),
    /// The top bar's Rec key, flaring once when an In recordings switch
    /// goes on, so the card's "The Rec key says so" points at something
    /// that just moved (critique #62's idea).
    Rec,
}

/// A confident arrival: fast, then settling (quartic ease-out, close to
/// `cubic-bezier(0.16, 1, 0.3, 1)`).
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(4)
}

/// How far (0..1) a motion that started `at` and lasts `length` has come.
pub fn progress(at: Instant, length: Duration, now: Instant) -> f32 {
    (now.saturating_duration_since(at).as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0)
}

/// How bright `what`'s bloom is now: 1 the moment it lit, easing to 0.
pub fn bloom(state: &TuxMix, what: Bloom) -> f32 {
    state
        .blooms
        .get(&what)
        .map_or(0.0, |&at| 1.0 - ease_out(progress(at, BLOOM, Instant::now())))
}

/// How much light is still going out of `what` (`COOL`): 1 the moment it
/// went off, easing to 0.
pub fn cool(state: &TuxMix, what: Bloom) -> f32 {
    let Some(&at) = state.cools.get(&what) else { return 0.0 };
    match glide_of(&state.control_room, what, at) {
        Some(p) => 1.0 - p.clamp(0.0, 1.0),
        None => 1.0 - ease_out(progress(at, COOL, Instant::now())),
    }
}

/// Dim, Talkback, External Input and Listenback let go glide the sound back
/// over half a second: their light goes out with it, so the key is dark when
/// the sound is back, not 220 ms before (the scout). The glide is the key's
/// own: Dim's, or one its key started (`TargetEase::by`), begun after the
/// light started cooling (`at`), so another key's glide never lights it
/// (OpenCode's job 35). How far it has got now: read from the clock, as the
/// tick will step it, so the fade moves on every frame, not every other.
fn glide_of(cr: &crate::control_room::ControlRoom, what: Bloom, at: Instant) -> Option<f32> {
    let now = Instant::now();
    let ours = |since: Instant| since + Duration::from_millis(100) >= at;
    // Never behind where the tick has stepped it.
    let seen = |since: Instant, was: f32| was.max(crate::control_room::ease_progress(since, was, now));
    match what {
        Bloom::Key("DIM") => cr.dim_easing.filter(|e| ours(e.since)).map(|e| seen(e.since, e.progress)),
        // Cue let go glides its mix back (issue #37): its light goes with it.
        Bloom::Cue(_) => cr.easing.values().filter(|e| e.by == "CUE" && ours(e.since)).map(|e| seen(e.since, e.progress)).reduce(f32::min),
        Bloom::Key(key @ ("TALKBACK" | "EXT IN" | "LISTEN")) => {
            cr.easing.values().filter(|e| e.by == key && ours(e.since)).map(|e| seen(e.since, e.progress)).reduce(f32::min)
        }
        _ => None,
    }
}

/// When the breath's cycle began: the last key armed (`breath_from_rest`).
static BREATH_START: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Starts the breath from rest: a key just armed swells in from its
/// resting light, never appears mid-breath, sometimes fully swollen (the
/// reviewer, 2026-11-14: one phase for the whole app).
pub fn breath_from_rest() {
    if let Ok(mut start) = BREATH_START.lock() {
        *start = Some(Instant::now());
    }
}

/// A slow breath, 0 to 1 and back over `BREATH`, for a key waiting for its
/// second click. 0 with animations off.
pub fn breath() -> f32 {
    if !crate::widgets::animations() {
        return 0.0;
    }
    let t = BREATH_START
        .lock()
        .ok()
        .map(|mut s| *s.get_or_insert_with(Instant::now))
        .map(|start| start.elapsed())
        .map_or(0.0, |d| d.as_secs_f32());
    0.5 - 0.5 * (std::f32::consts::TAU * t / BREATH.as_secs_f32()).cos()
}

/// How far the padlock is shaken sideways now, in px at scale 1: a damped
/// shake over `NUDGE`, 0 when still.
pub fn lock_nudge(state: &TuxMix) -> f32 {
    let Some(at) = state.lock_nudge.filter(|_| state.animations) else { return 0.0 };
    let p = progress(at, NUDGE, Instant::now());
    if p >= 1.0 {
        return 0.0;
    }
    3.0 * (1.0 - p) * (1.0 - p) * (std::f32::consts::TAU * 3.0 * p).sin()
}

/// Whether something waits for a second click (so its key breathes, and
/// the end of the wait is heard on time).
fn waiting(state: &TuxMix) -> bool {
    let within = |at: Instant| at.elapsed() < crate::app::PHANTOM_ARM_FOR;
    // 48V's own wait (a mic preset's runs 12 s: the audit, it stopped
    // breathing after 4) and the mic preset's Remove, which wasn't tracked.
    state.phantom_armed.is_some_and(|(_, at)| at.elapsed() < state.phantom_arm_for)
        || state.mic_preset_remove_armed.is_some_and(|(_, at)| at.elapsed() < crate::resetmix::ARM_FOR)
        || state.loop_armed.is_some_and(|(_, at)| within(at))
        || state.reset_armed.is_some_and(|(_, at)| within(at))
        || state.workspace_armed.is_some_and(|(_, at)| within(at))
        || crate::app::link_armed(state)
        // The D panel's two "press again" waits (critique #34: neither
        // breathed nor was heard ending).
        || state.dyn_reset_armed.is_some_and(|(_, at)| at.elapsed() < crate::dynamics_panel::RESET_ARM_FOR)
        || state.dyn_off_armed.is_some_and(|(_, at)| at.elapsed() < crate::dynamics_panel::OFF_ARM_FOR)
}

/// How far the opening row has grown (1 when settled or not animating).
pub fn floor_growth(state: &TuxMix) -> f32 {
    state
        .floor_anim
        .map_or(1.0, |at| ease_out(progress(at, FLOOR_OPEN, Instant::now())))
}

/// How far slot `slot` (0-based, reading order) of a sweep that started
/// `at` has lit: 0 to 1.
/// Every mute, solo and 48V switch, lit or not, to compare after an Undo
/// or Redo (`show_changes`).
/// The Cue pair an output strip's CUE stands for (`ControlRoom::cue`).
pub fn cue_pair(state: &TuxMix, cid: ChannelId) -> Option<usize> {
    match cid {
        ChannelId::Output(idx) => Some(if state.device.outputs_one_per_pair() { idx } else { idx / 2 }),
        _ => None,
    }
}

fn looped(state: &TuxMix, i: usize) -> bool {
    state.device.outputs().get(i).is_some_and(|o| o.loopback)
}

pub fn toggles(state: &TuxMix) -> Vec<(Bloom, bool)> {
    let mut all = Vec::new();
    let channels = (0..state.device.inputs().len())
        .map(ChannelId::Input)
        .chain((0..state.device.playbacks().len()).map(ChannelId::Playback))
        .chain((0..state.device.outputs().len()).map(ChannelId::Output));
    for cid in channels {
        all.push((Bloom::Mute(cid), muted(state, cid)));
        all.push((Bloom::Solo(cid), crate::app::channel_is_soloed(state, cid)));
    }
    for (i, ch) in state.device.inputs().iter().enumerate() {
        all.push((Bloom::Phantom(i), ch.phantom));
    }
    for (i, ch) in state.device.outputs().iter().enumerate() {
        all.push((Bloom::Loop(i), ch.loopback));
    }
    let pairs = if state.device.outputs_one_per_pair() { state.device.outputs().len() } else { state.device.outputs().len().div_ceil(2) };
    for p in 0..pairs {
        all.push((Bloom::Cue(p), state.control_room.cue == Some(p)));
    }
    all
}

/// After an Undo or Redo: each switch it turned on blooms and each it turned
/// off cools, as a press would, so what came back is seen where it is (the
/// scout's motion list: Undo changed keys without a sign). With Animations
/// off, nothing.
pub fn show_changes(state: &mut TuxMix, before: &[(Bloom, bool)]) {
    if !state.animations {
        return;
    }
    let now = Instant::now();
    for (what, was) in toggles(state).into_iter().zip(before.iter().map(|(_, on)| *on)) {
        match (was, what.1) {
            (false, true) => {
                state.blooms.insert(what.0, now);
                state.cools.remove(&what.0);
            }
            (true, false) => {
                state.cools.insert(what.0, now);
                state.blooms.remove(&what.0);
            }
            _ => {}
        }
    }
}

/// How long a slot's flare takes to fade.
pub const SLOT_FLARE: Duration = Duration::from_millis(650);

/// How bright snapshot slot `n`'s flare is (1 to 8): 1 as it lands, 0 when
/// settled or with animations off.
pub fn slot_flare(state: &TuxMix, n: u8) -> f32 {
    match state.slot_landed {
        Some((m, at)) if m == n && state.animations => 1.0 - ease_out(progress(at, SLOT_FLARE, Instant::now())),
        _ => 0.0,
    }
}

pub fn sweep_level(at: Instant, slot: usize, now: Instant) -> f32 {
    ease_out(progress(at + SWEEP_STAGGER * slot as u32, SWEEP_ONE, now))
}

/// Whether a motion is in flight, so the fast redraw timer must run.
pub fn running(state: &TuxMix) -> bool {
    let now = Instant::now();
    state.floor_anim.is_some_and(|at| progress(at, FLOOR_OPEN, now) < 1.0)
        || state.blooms.values().any(|&at| progress(at, BLOOM, now) < 1.0)
        || state.cools.values().any(|&at| progress(at, COOL, now) < 1.0)
        || state.store_sweep.is_some_and(|at| progress(at, SWEEP_ALL, now) < 1.0)
        || (state.animations && state.slot_landed.is_some_and(|(_, at)| progress(at, SLOT_FLARE, now) < 1.0))
        || (state.animations && state.lock_nudge.is_some_and(|at| progress(at, NUDGE, now) < 1.0))
        || state.panel_anim.is_some_and(|(_, at)| progress(at, PANEL_OPEN, now) < 1.0)
        || state.gain_flash.values().any(|&at| progress(at, GAIN_FLASH, now) < 1.0)
        || state.start_over_lit_at.is_some_and(|at| progress(at, START_OVER_LIT, now) < 1.0)
        || state.phones_cap_lit_at.is_some_and(|at| progress(at, START_OVER_LIT, now) < 1.0)
        // The livelier moments: a Quick tick drawing in, its band coming
        // up to its glow, an effect's key flaring as it's first heard.
        || (state.animations
            && (state.quick_done_at.iter().flatten().any(|&at| progress(at, TICK_DRAW, now) < 1.0)
                || state.quick_band_at.is_some_and(|at| progress(at, BAND_GLOW, now) < 1.0)
                || state.fx_heard_at.is_some_and(|at| progress(at, HEARD_FLARE, now) < 1.0)))
        // Fun pass 2: a Matrix patch's pulse, the active path's flares.
        || (state.animations && state.matrix_pulse.is_some_and(|(_, _, at)| progress(at, PATCH_FOR, now) < 1.0))
        || (state.animations && state.lamp_from.is_some_and(|(_, at)| progress(at, PATH_ALL, now) < 1.0))
        || (state.animations && state.live_wash_at.is_some_and(|at| progress(at, LIVE_WASH, now) < 1.0))
        || (state.animations && state.win_spark.is_some_and(|(_, at)| progress(at, WIN_SPARK, now) < 1.0))
        // A signal-flow card's dots running.
        || (state.animations && crate::flow::drawn(state))
        // A discovery shimmer passing.
        || (state.animations && state.shimmer.is_some_and(|(_, _, at)| progress(at, SHIMMER_FOR, now) < 1.0))
        // A milestone's burst (`celebrate`).
        || crate::celebrate::sparking(state) || crate::guides::sparking(state)
        // A found trick's segment lighting on its card.
        || (state.trick_card.is_some() && crate::tricks::arriving(state))
        // A preset's gain coming up: not decoration, so with animations off too.
        || !state.gain_ramps.is_empty()
        || !state.level_ramps.is_empty()
        || !state.fader_glides.is_empty()
        // Dim, Talkback, Listenback or External Input easing back.
        || crate::control_room::is_easing(state)
        // Voice's status line glowing as an app begins to hear it.
        || (state.animations && crate::dynamics_panel::heard_flare(state) > 0.0)
        // A wait breathes only with Animations on, and its end is heard
        // only with sounds on; with neither, the tick's own check ends it
        // (the audit, 2026-11-05: a 4 s 48V? wait redrew 125 times a
        // second with nothing moving).
        || (waiting(state) && (state.animations || state.ui_sounds))
        // The past-0 card's ringed meters breathing (critique #59).
        || (state.animations && (crate::tricks::names_mix_meter(state) || crate::tricks::names_hot_meter(state).is_some()))
}

/// Drops settled motions (on each fast tick).
pub fn prune(state: &mut TuxMix) {
    let now = Instant::now();
    state.blooms.retain(|_, at| progress(*at, BLOOM, now) < 1.0);
    // A key whose light follows its glide stays while the glide runs (it
    // was dropped at 280 ms and popped out mid-glide).
    let cr = &state.control_room;
    state.cools.retain(|what, at| progress(*at, COOL, now) < 1.0 || glide_of(cr, *what, *at).is_some());
    if state.lock_nudge.is_some_and(|at| progress(at, NUDGE, now) >= 1.0) {
        state.lock_nudge = None;
    }
    if state.floor_anim.is_some_and(|at| progress(at, FLOOR_OPEN, now) >= 1.0) {
        state.floor_anim = None;
    }
    if state.panel_anim.is_some_and(|(_, at)| progress(at, PANEL_OPEN, now) >= 1.0) {
        state.panel_anim = None;
    }
    state.gain_flash.retain(|_, at| progress(*at, GAIN_FLASH, now) < 1.0);
    if state.store_sweep.is_some_and(|at| progress(at, SWEEP_ALL, now) >= 1.0) {
        state.store_sweep = None;
    }
}

/// Starts the motion `message` sets off, judged before it is applied.
/// A discovery shimmer (Fun pass 2): a control whose trick hasn't been
/// found yet catches a glint of the lamp's light the first time the
/// pointer comes onto its strip, once ever, so it says "try me" without a
/// word (the owner, 2026-11-07: "go for it").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shimmer {
    /// The narrow switch: a strip folds.
    Fold,
    /// The dB box: double-click to type a level.
    Level,
    /// The name: double-click to rename.
    Name,
}

impl Shimmer {
    /// The trick it points at (`tricks::TRICKS`); found, it never shines.
    pub fn trick(self) -> &'static str {
        match self {
            Shimmer::Fold => "fold",
            Shimmer::Level => "type-level",
            Shimmer::Name => "rename",
        }
    }
}

/// One glint's sweep, and the least time between two.
pub const SHIMMER_FOR: Duration = Duration::from_millis(900);
pub const SHIMMER_EVERY: Duration = Duration::from_secs(90);

/// Where strip `cid`'s glint is now, 0..1 across, if one is passing.
pub fn shimmer(state: &TuxMix, cid: ChannelId) -> Option<(Shimmer, f32)> {
    let (c, s, at) = state.shimmer?;
    let t = since(at).as_secs_f32() / SHIMMER_FOR.as_secs_f32();
    (c == cid && t < 1.0 && state.animations).then_some((s, t))
}

/// The pointer came onto strip `cid`: the next shimmer it can show, if
/// one is due (Tips and Animations on, not locked, the Mixer, none in the
/// last `SHIMMER_EVERY`), each kind once ever and never once its trick is
/// found.
fn start_shimmer(state: &mut TuxMix, cid: ChannelId, now: Instant) {
    if !state.tips || state.ui_locked || state.view != crate::app::View::Mixer {
        return;
    }
    if state.shimmer_last.is_some_and(|at| now.duration_since(at) < SHIMMER_EVERY) {
        return;
    }
    let folded = state.collapsed.contains(&cid);
    let next = [Shimmer::Fold, Shimmer::Level, Shimmer::Name].into_iter().find(|s| {
        let id = s.trick();
        (!folded || *s == Shimmer::Fold)
            && !state.tricks_found.iter().any(|t| t == id)
            && !state.shimmers_shown.iter().any(|t| t == id)
    });
    if let Some(s) = next {
        state.shimmer = Some((cid, s, now));
        state.shimmer_last = Some(now);
        state.shimmers_shown.push(s.trick().to_string());
        state.prefs_dirty = true;
    }
}

/// The Rec key's flare (`Bloom::Rec`), when `now` (its words just gained
/// "+ Reverb" and the like) and Animations are on.
pub fn flare_rec(state: &mut TuxMix, now: bool) {
    if now && state.animations {
        state.blooms.insert(Bloom::Rec, Instant::now());
    }
}

pub fn start(state: &mut TuxMix, message: &Message) {
    if !state.animations {
        return;
    }
    let now = Instant::now();
    if let Message::StripHovered(Some(cid)) = message {
        if !matches!(cid, ChannelId::Output(_)) {
            start_shimmer(state, *cid, now);
        }
    }
    let cr = &state.control_room;
    let lit = match message {
        Message::Mute(cid, true) => Some(Bloom::Mute(*cid)),
        Message::Solo(cid, true) => Some(Bloom::Solo(*cid)),
        Message::Phantom(idx, true) => Some(Bloom::Phantom(*idx)),
        Message::DimToggle if !cr.is_dimmed() => Some(Bloom::Key("DIM")),
        Message::TalkbackToggle if !cr.talkback => Some(Bloom::Key("TALKBACK")),
        Message::ExtInToggle if !cr.ext_in => Some(Bloom::Key("EXT IN")),
        Message::ListenbackToggle if !cr.listenback => Some(Bloom::Key("LISTEN")),
        Message::SpeakerBToggle if !cr.speaker_b => Some(Bloom::Key("SPEAKER B")),
        // Mono and the M/S check light up as the other keys do (critique #50).
        Message::MonoToggle if !cr.mono => Some(Bloom::Key("MONO")),
        Message::MsCheckToggle if cr.ms_check.is_none() => Some(Bloom::Key("Mid/Side")),
        Message::MuteFxToggle if cr.fx_muted.is_none() => Some(Bloom::Key("MUTE FX")),
        Message::FxOn(unit, true) => Some(Bloom::Key(unit.key_name())),
        Message::CueChanged(cid, true) => cue_pair(state, *cid).map(Bloom::Cue),
        // Arming (a second click wanted) starts the breath from rest.
        Message::PhantomArm(_) | Message::LinkArm(_) | Message::LoopArm(_) | Message::ResetMixArm(_) | Message::WorkspaceLoadArm(_) | Message::MicPresetRemove(_) => {
            breath_from_rest();
            None
        }
        Message::LoopbackChanged(i, true) if !looped(state, *i) => Some(Bloom::Loop(*i)),
        _ => None,
    };
    if let Some(what) = lit {
        state.blooms.insert(what, now);
        state.cools.remove(&what);
    }
    // And what goes out cools, if it was lit.
    let input = |i: usize| state.device.inputs().get(i);
    let out = match message {
        Message::Mute(cid, false) if muted(state, *cid) => Some(Bloom::Mute(*cid)),
        Message::Solo(cid, false) if crate::app::channel_is_soloed(state, *cid) => Some(Bloom::Solo(*cid)),
        Message::Phantom(idx, false) if input(*idx).is_some_and(|c| c.phantom) => Some(Bloom::Phantom(*idx)),
        Message::DimToggle if cr.is_dimmed() => Some(Bloom::Key("DIM")),
        Message::TalkbackToggle if cr.talkback => Some(Bloom::Key("TALKBACK")),
        Message::ExtInToggle if cr.ext_in => Some(Bloom::Key("EXT IN")),
        Message::ListenbackToggle if cr.listenback => Some(Bloom::Key("LISTEN")),
        Message::SpeakerBToggle if cr.speaker_b => Some(Bloom::Key("SPEAKER B")),
        Message::MonoToggle if cr.mono => Some(Bloom::Key("MONO")),
        Message::MsCheckToggle if cr.ms_check.is_some() => Some(Bloom::Key("Mid/Side")),
        Message::MuteFxToggle if cr.fx_muted.is_some() => Some(Bloom::Key("MUTE FX")),
        Message::FxOn(unit, false) => Some(Bloom::Key(unit.key_name())),
        Message::CueChanged(cid, false) => cue_pair(state, *cid).filter(|p| cr.cue == Some(*p)).map(Bloom::Cue),
        // Cue moved to another output: the one it left goes out.
        Message::CueChanged(cid, true) => cr.cue.filter(|p| Some(*p) != cue_pair(state, *cid)).map(Bloom::Cue),
        Message::LoopbackChanged(i, false) if looped(state, *i) => Some(Bloom::Loop(*i)),
        _ => None,
    };
    if let Some(what) = out {
        state.cools.insert(what, now);
        state.blooms.remove(&what);
    }
    // A slot lands: stored into, or a stored one recalled. Its key flares in
    // the lamp, so the "kept" or the recall's rise is seen where it happened
    // (the scout's motion list).
    let stored = |n: u8| state.stored_snapshots.get(usize::from(n).wrapping_sub(1)).copied().unwrap_or(false);
    let landed = match message {
        Message::SnapshotClicked(n) if state.snapshot_storing || stored(*n) => Some(*n),
        Message::SnapshotStoreTo(n) => Some(*n),
        _ => None,
    };
    if let Some(n) = landed {
        state.slot_landed = Some((n, now));
    }
    // Store: the waiting slots light in turn.
    let entering_store = matches!(message, Message::SnapshotStore if !state.snapshot_storing)
        || matches!(message, Message::LayoutStore if !state.layout_storing);
    if entering_store {
        state.store_sweep = Some(now);
    }
    if let Message::OpenFloor(floor) | Message::ToggleFloor(floor) = message {
        if !crate::app::floor_is_open(state, (*floor).min(2)) && crate::app::one_floor_open(state) {
            state.floor_anim = Some(now);
        }
    }
    // A patch in the Matrix: a crosspoint that was off coming on.
    if let Message::MatrixSet(cid, pair, v, _) | Message::MatrixVolume(cid, pair, v) = message {
        let was = state.device.volume(*cid, *pair).unwrap_or(0.0);
        if *v > 0.0 && was <= 0.0 {
            state.matrix_pulse = Some((*cid, *pair, now));
        }
    }
    // A strip's side panel slides out when it opens (not when it closes).
    if let Message::ToggleFlyout(cid, kind) = message {
        if !state.flyout_open.contains(&(*cid, *kind)) {
            state.panel_anim = Some(((*cid, *kind), now));
        }
    }
}

/// Whether channel `cid` is muted now.
fn muted(state: &TuxMix, cid: ChannelId) -> bool {
    match cid {
        ChannelId::Input(i) => state.device.inputs().get(i).is_some_and(|c| c.mute),
        ChannelId::Playback(i) => state.device.playbacks().get(i).is_some_and(|c| c.mute),
        ChannelId::Output(i) => state.device.outputs().get(i).is_some_and(|c| c.mute),
    }
}

/// How long the meters' light test at start runs, its last meter's
/// stagger included (`boot_level`).
pub const BOOT_FOR: Duration = Duration::from_millis(1200);

/// The level meter `i` shows `elapsed` into the light test: up to −1 dBFS
/// (never the clip), a breath held, and back down, each meter 25ms after
/// the one before it, so the light runs across the desk.
pub fn boot_level(at: Instant, i: usize) -> f32 {
    let ms = at.elapsed().as_secs_f32() * 1000.0 - i as f32 * 25.0;
    const TOP: f32 = 0.89;
    if ms <= 0.0 {
        0.0
    } else if ms < 320.0 {
        TOP * ease_out(ms / 320.0)
    } else if ms < 420.0 {
        TOP
    } else if ms < 820.0 {
        TOP * (1.0 - ease_out((ms - 420.0) / 400.0))
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_show_the_switches_they_change() {
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let an1 = ChannelId::Input(0);
        let _ = crate::app::update(&mut state, Message::Mute(an1, true));
        state.blooms.clear();
        state.cools.clear();
        let _ = crate::app::update(&mut state, Message::Undo);
        assert!(state.cools.contains_key(&Bloom::Mute(an1)), "the mute Undo took back cools");
        assert!(!state.blooms.contains_key(&Bloom::Mute(ChannelId::Input(1))), "nothing else");
        let _ = crate::app::update(&mut state, Message::Redo);
        assert!(state.blooms.contains_key(&Bloom::Mute(an1)), "Redo lights it again");
    }

    #[test]
    fn a_snapshot_slot_flares_where_it_lands() {
        // The scout's motion list: the store's "kept" and the recall's rise
        // are seen at the slot.
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let _ = crate::app::update(&mut state, Message::SnapshotClicked(5));
        assert_eq!(slot_flare(&state, 5), 0.0, "an empty slot recalls nothing: no flare");
        let _ = crate::app::update(&mut state, Message::SnapshotStore);
        let _ = crate::app::update(&mut state, Message::SnapshotClicked(3));
        assert!(slot_flare(&state, 3) > 0.9, "stored into: it flares");
        assert_eq!(slot_flare(&state, 2), 0.0, "only there");
        let _ = crate::app::update(&mut state, Message::SnapshotClicked(3));
        assert!(slot_flare(&state, 3) > 0.9, "recalled: it flares again");
        state.animations = false;
        assert_eq!(slot_flare(&state, 3), 0.0, "still with Animations off");
    }

    #[test]
    fn a_keys_light_stays_for_its_own_glide_only() {
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let _ = crate::app::update(&mut state, Message::DimToggle);
        let _ = crate::app::update(&mut state, Message::DimToggle);
        // 300 ms on, past the cool's own 280: kept while the glide runs.
        if let (Some(then), Some(mut ease)) = (Instant::now().checked_sub(Duration::from_millis(300)), state.control_room.dim_easing) {
            ease.since = then;
            state.control_room.dim_easing = Some(ease);
            state.cools.insert(Bloom::Key("DIM"), then);
            prune(&mut state);
            assert!(state.cools.contains_key(&Bloom::Key("DIM")), "not dropped mid-glide");
        }
        // Ext In switched off just now, Talkback let go with it: each key
        // follows its own glide, not one begun in the same moment
        // (OpenCode's job 35).
        let _ = crate::app::update(&mut state, Message::TalkbackToggle);
        state.cools.insert(Bloom::Key("EXT IN"), Instant::now());
        let _ = crate::app::update(&mut state, Message::TalkbackToggle);
        crate::control_room::step_easing(&mut state, Instant::now() + Duration::from_millis(250));
        let talkback = cool(&state, Bloom::Key("TALKBACK"));
        assert!(talkback < 0.95, "Talkback fades with its glide: {talkback}");
        assert!(cool(&state, Bloom::Key("EXT IN")) > 0.95, "Ext In isn't on Talkback's glide");
        crate::app::settle_glides(&mut state);
        // Ext In switched off a second ago; Talkback let go now: Ext In's
        // light doesn't come back with Talkback's glide.
        if let Some(earlier) = Instant::now().checked_sub(Duration::from_secs(1)) {
            state.cools.insert(Bloom::Key("EXT IN"), earlier);
            let _ = crate::app::update(&mut state, Message::TalkbackToggle);
            let _ = crate::app::update(&mut state, Message::TalkbackToggle);
            assert!(cool(&state, Bloom::Key("EXT IN")) < 0.01, "not another key's glide");
        }
    }

    #[test]
    fn dims_light_goes_out_with_its_glide() {
        // The scout: the key cooled in 280 ms while the sound took 500.
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let _ = crate::app::update(&mut state, Message::DimToggle);
        let _ = crate::app::update(&mut state, Message::DimToggle);
        assert!(state.control_room.dim_easing.is_some(), "gliding back");
        let start = cool(&state, Bloom::Key("DIM"));
        assert!(start > 0.95, "still lit as the glide starts: {start}");
        crate::control_room::step_easing(&mut state, Instant::now() + Duration::from_millis(300));
        let mid = cool(&state, Bloom::Key("DIM"));
        assert!(mid > 0.0 && mid < start, "part-way out with the sound part-way back: {mid}");
        crate::app::settle_glides(&mut state);
        // The glide's half second has gone by in the app (here, at once).
        if let Some(then) = Instant::now().checked_sub(Duration::from_millis(600)) {
            state.cools.insert(Bloom::Key("DIM"), then);
            assert!(cool(&state, Bloom::Key("DIM")) < 0.01, "out when the sound is back");
        }
    }

    #[test]
    fn the_ease_arrives_fast_and_settles() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.25) > 0.6, "most of the way in the first quarter");
        assert!(ease_out(0.5) < ease_out(0.75));
    }

    #[test]
    fn a_breath_stays_between_rest_and_full() {
        for _ in 0..20 {
            let b = breath();
            assert!((0.0..=1.0).contains(&b), "{b}");
        }
    }

    #[test]
    fn a_motion_ends_on_time() {
        let at = Instant::now();
        assert_eq!(progress(at, BLOOM, at), 0.0);
        assert_eq!(progress(at, BLOOM, at + BLOOM), 1.0);
        assert_eq!(progress(at, BLOOM, at + BLOOM * 3), 1.0);
    }

    #[test]
    fn many_switches_flipped_at_once_bloom_and_cool_like_undo() {
        // The reviewer, 2026-11-14: a recall or a master key flipped
        // switches with no light, while Undo's bloomed.
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let an1 = ChannelId::Input(0);
        let _ = crate::app::update(&mut state, Message::Mute(an1, true));
        state.blooms.clear();
        state.cools.clear();
        assert_eq!(crate::sounds::for_message(&state, &Message::GlobalMuteToggle), Some(crate::sounds::Sound::Off));
        let _ = crate::app::update(&mut state, Message::GlobalMuteToggle);
        assert!(state.cools.contains_key(&Bloom::Mute(an1)), "the master's clear cools AN1's M");
        assert_eq!(crate::sounds::for_message(&state, &Message::GlobalMuteToggle), Some(crate::sounds::Sound::On));
        let _ = crate::app::update(&mut state, Message::GlobalMuteToggle);
        assert!(state.blooms.contains_key(&Bloom::Mute(an1)), "brought back, it blooms");
        assert!(!state.blooms.contains_key(&Bloom::Mute(ChannelId::Input(1))), "nothing else");
    }

    #[test]
    fn the_rec_key_flares_only_when_its_words_gain_an_effect() {
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        // The simulation's take can't hold effects: the key's words don't
        // change, so no flare (critique #63 A 5).
        let _ = crate::app::update(&mut state, Message::SetRecordFx(crate::fx::Unit::Echo, false));
        let _ = crate::app::update(&mut state, Message::SetRecordFx(crate::fx::Unit::Echo, true));
        assert!(!state.blooms.contains_key(&Bloom::Rec));
        flare_rec(&mut state, true);
        assert!(bloom(&state, Bloom::Rec) > 0.5, "flaring now");
        state.blooms.clear();
        flare_rec(&mut state, false);
        assert!(!state.blooms.contains_key(&Bloom::Rec));
        state.animations = false;
        flare_rec(&mut state, true);
        assert!(!state.blooms.contains_key(&Bloom::Rec), "Animations off: none");
    }

    #[test]
    fn cue_and_loop_bloom_and_cool_as_their_neighbours_do() {
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let (a, b) = (ChannelId::Output(0), ChannelId::Output(2));
        let (pa, pb) = (cue_pair(&state, a).unwrap(), cue_pair(&state, b).unwrap());
        assert_ne!(pa, pb);
        let _ = crate::app::update(&mut state, Message::CueChanged(a, true));
        assert!(state.blooms.contains_key(&Bloom::Cue(pa)));
        let _ = crate::app::update(&mut state, Message::CueChanged(b, true));
        assert!(state.cools.contains_key(&Bloom::Cue(pa)), "the cue it left goes out");
        assert!(state.blooms.contains_key(&Bloom::Cue(pb)));
        assert_eq!(crate::sounds::for_message(&state, &Message::LoopbackChanged(0, true)), Some(crate::sounds::Sound::On));
        let _ = crate::app::update(&mut state, Message::LoopbackChanged(0, true));
        assert!(state.blooms.contains_key(&Bloom::Loop(0)));
        let _ = crate::app::update(&mut state, Message::LoopbackChanged(0, false));
        assert!(state.cools.contains_key(&Bloom::Loop(0)));
    }

    #[test]
    fn a_click_on_a_snapshot_slot_lights_what_its_recall_flips() {
        // A slot press is decided into a recall below the top of `update`:
        // the wrappers must see the recall, not the press (the advisor).
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        let an1 = ChannelId::Input(0);
        let _ = crate::app::update(&mut state, Message::Mute(an1, true));
        let _ = crate::app::update(&mut state, Message::SnapshotStore);
        let _ = crate::app::update(&mut state, Message::SnapshotSlotPressed(1));
        assert!(state.stored_snapshots[0], "stored from the slot");
        let _ = crate::app::update(&mut state, Message::Mute(an1, false));
        state.blooms.clear();
        state.cools.clear();
        let _ = crate::app::update(&mut state, Message::SnapshotSlotPressed(1));
        assert!(muted(&state, an1), "recalled");
        assert!(state.blooms.contains_key(&Bloom::Mute(an1)), "its M blooms as the recall brings it back");
    }

    #[test]
    fn a_small_win_lights_the_strip_it_happened_on() {
        let mut state = crate::app::new(true, None, None);
        state.animations = true;
        state.tips = true;
        state.tips_shown.clear();
        let _ = crate::app::update(&mut state, Message::PhaseChanged(ChannelId::Input(0), true));
        assert!(win_spark(&state, ChannelId::Input(0)).is_some(), "phase's first time sparks on AN1");
        assert_eq!(win_spark(&state, ChannelId::Input(2)), None, "not another strip");
        state.animations = false;
        assert_eq!(win_spark(&state, ChannelId::Input(0)), None, "Animations off: none");
    }
}
