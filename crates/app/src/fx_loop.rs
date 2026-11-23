//! The FX loop (issue #9, phase C): the unit's FX send through Audra's
//! Reverb and Echo and back into the unit's FX return.
//!
//! The Babyface Pro FS leaves Reverb and Echo to the host (manual 21.6).
//! The unit mixes an FX send from every channel's FX Send knob and hands it
//! to the computer on frame ch12/13 of its capture stream; whatever the
//! computer plays on ch12/13 comes back as the FX return, a mixer source
//! the outputs pick up through their "FX Return" faders. The driver opens
//! that pair since 2026-11-05 (phase A), so PipeWire's Pro Audio nodes
//! have 14 channels: the 12 audio channels, then the FX pair.
//!
//! The loop is two PipeWire children, like the meters (`pw_meter`): one
//! `pw-record` reads all 14 capture channels of the unit's source, and one
//! `pw-play` stream writes 14 channels to its sink, silent everywhere but
//! the FX pair, so it adds nothing to what other programs play. Between
//! them one thread runs `audra_fx::FxEngine`. Both children are pinned to
//! the unit: targeted by index, never moved, never sent to a fallback
//! device and never remixed, so the return can't end up in the laptop's
//! speakers if the unit goes away. If Audra quits or crashes, the pipes
//! close and both children exit on their own.
//!
//! It needs the Pro Audio profile (a stereo profile opens only 2 channels)
//! and the updated driver (an older one shows 12); `FxLoopState` says
//! which is missing, so the UI can explain it rather than stay silent.
//!
//! Latency: blocks of 256 frames, and a cushion queued ahead of `pw-play`
//! so it never waits on us: 512 frames to start, growing a block at a
//! time (to 2048) if the queue ever runs nearly dry, as when another
//! program makes the graph's buffer bigger. What is queued is measured
//! (`FIONREAD` on the pipe) and held to the cushion and one block: a block
//! that would queue more is dropped, so nothing builds up while `pw-play`
//! starts or stalls. About 16 ms at 48 kHz on top of the graph's own; a
//! reverb or echo carries that easily (it is added to the echo's delay).

use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use audra_fx::{EchoParams, FxEngine, ReverbParams};
use tuxmix_core::pw_meter::{self, Monitor, TapKind};

/// The fastest rate the unit carries the FX pair at.
pub const MAX_RATE: u32 = 48_000;

/// Channels in the Pro Audio frame with the FX pair open.
pub const FX_CHANNELS: usize = 14;
/// The FX pair's first channel: the send on capture, the return on playback.
pub const FX_PAIR: usize = 12;
/// The 12-channel layout of a driver that doesn't open the FX pair.
const AUDIO_CHANNELS: usize = 12;
/// Frames per block between the children.
const PERIOD_FRAMES: usize = 256;
/// Silence written ahead to `pw-play`, so it never waits on the loop.
const CUSHION_FRAMES: usize = 512;
/// The most the cushion grows to.
const CUSHION_MAX_FRAMES: usize = 2048;
/// The pipes' size: room for the largest cushion and a few blocks, so a
/// write never waits on a full pipe (Linux's default is 64 KiB).
const PIPE_BYTES: i32 = 256 * 1024;
/// How long to wait before looking for the unit again.
const RETRY: Duration = Duration::from_secs(2);
const BYTES: usize = std::mem::size_of::<f32>();

/// What the loop runs: Reverb and Echo with their settings. Saved in the
/// GUI's prefs, since the unit keeps none of it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct FxSettings {
    pub reverb: ReverbParams,
    pub echo: EchoParams,
    pub reverb_on: bool,
    pub echo_on: bool,
}


impl FxSettings {
    /// Whether anything needs the loop at all.
    pub fn any_on(&self) -> bool {
        self.reverb_on || self.echo_on
    }
}

/// Where the loop stands, for the UI to explain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FxLoopState {
    /// Looking for the unit, or between restarts.
    Searching,
    /// The card is in a stereo profile, which opens only 2 channels: the
    /// FX pair needs Pro Audio.
    NeedsProAudio,
    /// Pro Audio shows 12 channels: the driver doesn't open the FX pair.
    NeedsDriver,
    /// `pw-record` or `pw-play` couldn't be started.
    ToolsMissing,
    /// The unit runs faster than 48 kHz (this rate), where its USB frame
    /// carries no FX pair, whatever the profile.
    RateTooHigh(u32),
    /// Running at this sample rate.
    Running(u32),
}

/// The settings the UI changes and the loop reads, with a generation
/// counter so the loop locks only when something changed.
struct Shared {
    settings: Mutex<FxSettings>,
    generation: AtomicU64,
    /// Auditions asked for (`FxLoop::audition`): each new count is one clap.
    auditions: AtomicU64,
    /// Blocks pumped so far: the watchdog's sign that sound still comes
    /// (`stalled`).
    blocks: AtomicU64,
}

/// The two children, held so they can be killed from another thread:
/// killing one closes its pipe, which unblocks the loop.
#[derive(Default)]
struct Children {
    record: Option<Child>,
    play: Option<Child>,
}

impl Children {
    fn kill(&mut self) {
        for child in [self.record.take(), self.play.take()].into_iter().flatten() {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Owns the loop's threads and children; dropping it stops everything.
pub struct FxLoop {
    shared: Arc<Shared>,
    state: Arc<Mutex<FxLoopState>>,
    stop: Arc<AtomicBool>,
    children: Arc<Mutex<Children>>,
    worker: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
}

impl FxLoop {
    /// Starts looking for the unit and runs the loop once it is found.
    /// Cheap: discovery and the children live on background threads.
    pub fn start(settings: FxSettings) -> Self {
        let shared = Arc::new(Shared { settings: Mutex::new(settings), generation: AtomicU64::new(0), auditions: AtomicU64::new(0), blocks: AtomicU64::new(0) });
        let state = Arc::new(Mutex::new(FxLoopState::Searching));
        let stop = Arc::new(AtomicBool::new(false));
        let children = Arc::new(Mutex::new(Children::default()));
        let in_use: Arc<Mutex<Option<(Monitor, Monitor)>>> = Arc::new(Mutex::new(None));
        let worker = {
            let (shared, state, stop, children, in_use) =
                (Arc::clone(&shared), Arc::clone(&state), Arc::clone(&stop), Arc::clone(&children), Arc::clone(&in_use));
            std::thread::Builder::new()
                .name("audra-fx".into())
                .spawn(move || {
                    crate::priority::raise_this_thread("Reverb & Echo");
                    run(&shared, &state, &stop, &children, &in_use)
                })
                .ok()
        };
        let watch = {
            let (shared, state, stop, children) = (Arc::clone(&shared), Arc::clone(&state), Arc::clone(&stop), Arc::clone(&children));
            std::thread::Builder::new()
                .name("audra-fx-watch".into())
                .spawn(move || watch(&shared, &state, &in_use, &children, &stop))
                .ok()
        };
        Self { shared, state, stop, children, worker, watch }
    }

    /// New settings, heard from the next block on.
    pub fn set(&self, settings: FxSettings) {
        if let Ok(mut s) = self.shared.settings.lock() {
            if *s != settings {
                *s = settings;
                self.shared.generation.fetch_add(1, Ordering::Release);
            }
        }
    }

    /// One clap through Reverb and Echo, from the next block (`FxEngine::
    /// audition`): heard with no mic up.
    pub fn audition(&self) {
        self.shared.auditions.fetch_add(1, Ordering::Release);
    }

    pub fn state(&self) -> FxLoopState {
        self.state.lock().map(|s| *s).unwrap_or(FxLoopState::Searching)
    }
}

impl Drop for FxLoop {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut c) = self.children.lock() {
            c.kill();
        }
        for handle in [self.worker.take(), self.watch.take()].into_iter().flatten() {
            let _ = handle.join();
        }
    }
}

/// Whether the source and sink PipeWire shows can carry the loop, and at
/// what rate.
fn plan(source: Option<&Monitor>, sink: Option<&Monitor>) -> Result<u32, FxLoopState> {
    let (Some(source), Some(sink)) = (source, sink) else {
        return Err(FxLoopState::Searching);
    };
    // The rate first (issue #22): at 96 kHz the frame has fewer channels
    // and no FX pair, and the counts below would ask for Pro Audio or a
    // newer driver that are already there.
    let rate = source.rate.max(sink.rate);
    if rate > MAX_RATE {
        return Err(FxLoopState::RateTooHigh(rate));
    }
    let fewest = source.channels.min(sink.channels);
    if fewest < AUDIO_CHANNELS {
        return Err(FxLoopState::NeedsProAudio);
    }
    if fewest < FX_CHANNELS {
        return Err(FxLoopState::NeedsDriver);
    }
    if source.rate != sink.rate {
        // Mid-switch: the two nodes of one card always share a rate.
        return Err(FxLoopState::Searching);
    }
    Ok(source.rate)
}

/// `AUX0,…,AUX13`: the Pro Audio nodes' own positions, so PipeWire links
/// channel to channel and never remixes.
fn channel_map() -> String {
    (0..FX_CHANNELS).map(|i| format!("AUX{i}")).collect::<Vec<_>>().join(",")
}

/// The arguments both children share, for `endpoint`.
fn common_args(endpoint: &Monitor, node: &str) -> Vec<String> {
    vec![
        // The index, not the name: a name falls back to the default
        // device while the unit is suspended (`pw_meter`).
        "--target".into(),
        endpoint.index.to_string(),
        "--raw".into(),
        "--format".into(),
        "f32".into(),
        "--rate".into(),
        endpoint.rate.to_string(),
        "--channels".into(),
        FX_CHANNELS.to_string(),
        "--channel-map".into(),
        channel_map(),
        "--latency".into(),
        PERIOD_FRAMES.to_string(),
        "--media-role".into(),
        "Production".into(),
        // Pinned to the unit: never moved, never sent to a fallback device
        // if it goes away, never remixed onto other channels.
        "--properties".into(),
        format!(
            "node.name={node} node.dont-reconnect=true node.dont-fallback=true node.dont-move=true stream.dont-remix=true"
        ),
    ]
}

fn record_args(source: &Monitor) -> Vec<String> {
    let mut args = common_args(source, "audra-fx-send");
    args.extend(["--media-category".into(), "Capture".into(), "-".into()]);
    args
}

fn play_args(sink: &Monitor) -> Vec<String> {
    let mut args = common_args(sink, "audra-fx-return");
    args.extend(["--media-category".into(), "Playback".into(), "-".into()]);
    args
}

fn spawn(tool: &str, args: &[String], stdin: Stdio, stdout: Stdio) -> Option<Child> {
    let child = Command::new(tool).args(args).stdin(stdin).stdout(stdout).stderr(Stdio::null()).spawn().ok()?;
    // rtkit's raise doesn't pass to a child (`priority::raise_child`).
    crate::priority::raise_child(child.id(), "a Reverb & Echo helper");
    Some(child)
}

/// The worker: find the unit, start both children, run blocks until a pipe
/// closes (a child exited, the watchdog killed it, or Audra is stopping),
/// then look again.
fn run(
    shared: &Shared,
    state: &Mutex<FxLoopState>,
    stop: &AtomicBool,
    children: &Mutex<Children>,
    in_use: &Mutex<Option<(Monitor, Monitor)>>,
) {
    let set_state = |s| {
        if let Ok(mut st) = state.lock() {
            *st = s;
        }
    };
    while !stop.load(Ordering::Relaxed) {
        let source = pw_meter::discover(TapKind::Source);
        let sink = pw_meter::discover(TapKind::Sink);
        let rate = match plan(source.as_ref(), sink.as_ref()) {
            Ok(rate) => rate,
            Err(s) => {
                set_state(s);
                sleep_unless(stop, RETRY);
                continue;
            }
        };
        let (Some(source), Some(sink)) = (source, sink) else { continue };
        let Some(mut record) = spawn("pw-record", &record_args(&source), Stdio::null(), Stdio::piped()) else {
            set_state(FxLoopState::ToolsMissing);
            sleep_unless(stop, RETRY);
            continue;
        };
        let Some(mut play) = spawn("pw-play", &play_args(&sink), Stdio::piped(), Stdio::null()) else {
            let _ = record.kill();
            let _ = record.wait();
            set_state(FxLoopState::ToolsMissing);
            sleep_unless(stop, RETRY);
            continue;
        };
        let (Some(from), Some(to)) = (record.stdout.take(), play.stdin.take()) else {
            Children { record: Some(record), play: Some(play) }.kill();
            sleep_unless(stop, RETRY);
            continue;
        };
        let to_fd = std::os::fd::AsRawFd::as_raw_fd(&to);
        grow_pipe(std::os::fd::AsRawFd::as_raw_fd(&from));
        grow_pipe(to_fd);
        if let Ok(mut c) = children.lock() {
            *c = Children { record: Some(record), play: Some(play) };
        }
        if let Ok(mut u) = in_use.lock() {
            *u = Some((source, sink));
        }
        // Checked after the children are stored, so a stop that came
        // while they started still finds and kills them.
        if stop.load(Ordering::Relaxed) {
            break;
        }
        set_state(FxLoopState::Running(rate));
        let mut engine = FxEngine::new(rate as f32);
        let _ = pump(from, to, &mut engine, shared, stop, &|| queued_bytes(to_fd));
        if let Ok(mut u) = in_use.lock() {
            *u = None;
        }
        if let Ok(mut c) = children.lock() {
            c.kill();
        }
        set_state(FxLoopState::Searching);
        sleep_unless(stop, Duration::from_millis(300));
    }
    if let Ok(mut c) = children.lock() {
        c.kill();
    }
}

/// Blocks from the send through the engine into the return, until either
/// pipe closes or Audra stops. Allocates only before the first block.
/// Bytes waiting in a pipe, unread (`FIONREAD`); `None` where it can't be
/// told.
fn queued_bytes(fd: std::os::fd::RawFd) -> Option<usize> {
    let mut n: libc::c_int = 0;
    // SAFETY: FIONREAD writes one c_int through the pointer, which lives
    // for the call; `fd` is a pipe this thread holds open.
    let r = unsafe { libc::ioctl(fd, libc::FIONREAD, &mut n) };
    (r == 0 && n >= 0).then_some(n as usize)
}

/// Makes a pipe big enough for the cushion (best effort; the default size
/// still works, with a smaller cushion's worth of room).
fn grow_pipe(fd: std::os::fd::RawFd) {
    // SAFETY: F_SETPIPE_SZ takes an int and touches no memory of ours.
    unsafe {
        libc::fcntl(fd, libc::F_SETPIPE_SZ, PIPE_BYTES);
    }
}

/// Blocks from the send through the engine into the return, until either
/// pipe closes or Audra stops. `queued` says how much waits ahead of
/// `pw-play` (`None`: unknown, so nothing is dropped). Allocates only before
/// the first block.
fn pump(
    mut from: impl Read,
    mut to: impl Write,
    engine: &mut FxEngine,
    shared: &Shared,
    stop: &AtomicBool,
    queued: &dyn Fn() -> Option<usize>,
) -> std::io::Result<()> {
    let block = PERIOD_FRAMES * FX_CHANNELS * BYTES;
    let frame = FX_CHANNELS * BYTES;
    let mut input = vec![0u8; block];
    let mut output = vec![0u8; block];
    let silence = vec![0u8; block];
    let mut send = vec![0.0f32; PERIOD_FRAMES * 2];
    let mut ret = vec![0.0f32; PERIOD_FRAMES * 2];
    let mut cushion = CUSHION_FRAMES * frame;
    to.write_all(&vec![0u8; cushion])?;
    to.flush()?;
    let mut seen = u64::MAX;
    let mut clapped = shared.auditions.load(Ordering::Acquire);
    while !stop.load(Ordering::Relaxed) {
        from.read_exact(&mut input)?;
        let asked = shared.auditions.load(Ordering::Acquire);
        if asked != clapped {
            clapped = asked;
            engine.audition();
        }
        let generation = shared.generation.load(Ordering::Acquire);
        if generation != seen {
            if let Ok(s) = shared.settings.lock() {
                apply(engine, &s);
                seen = generation;
            }
        }
        shared.blocks.fetch_add(1, Ordering::Relaxed);
        take_send(&input, &mut send);
        engine.process(&send, &mut ret);
        put_return(&ret, &mut output);
        if let Some(q) = queued() {
            // Held to the cushion and a block: more would only be delay.
            if q > cushion + block {
                continue;
            }
            // Nearly dry: the graph takes more at a time than the cushion
            // covers. A block of silence now, and a bigger cushion.
            if q < block / 2 && cushion < CUSHION_MAX_FRAMES * frame {
                cushion += block;
                to.write_all(&silence)?;
            }
        }
        to.write_all(&output)?;
    }
    Ok(())
}

pub(crate) fn apply(engine: &mut FxEngine, s: &FxSettings) {
    // Switching one off and on again starts it clean, not mid-tail; back
    // on while it is still fading out, it carries on from there (a reset
    // then would cut the tail it is fading: OpenCode's job 61).
    if s.reverb_on && !engine.reverb_on && !engine.reverb_fading() {
        engine.reverb.reset();
    }
    if s.echo_on && !engine.echo_on && !engine.echo_fading() {
        engine.echo.reset();
    }
    engine.reverb_on = s.reverb_on;
    engine.echo_on = s.echo_on;
    if engine.reverb.params() != s.reverb {
        engine.reverb.set_params(s.reverb);
    }
    if engine.echo.params() != s.echo {
        engine.echo.set_params(s.echo);
    }
}

fn sample(bytes: &[u8], frame: usize, channel: usize) -> f32 {
    let at = (frame * FX_CHANNELS + channel) * BYTES;
    f32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The FX pair out of a 14-channel block, as interleaved stereo.
/// What the unit's FX send arrives short by, made up here so Reverb and
/// Echo come back at their knobs' levels, as TotalMix's do. The send's 0 dB
/// is 0x1000, half the crosspoints' unity (what TotalMix writes), and the
/// unit hands it over 6 dB down: measured on the unit (2026-11-10), a tone
/// at −33 dBFS RMS with its send at 0 dB arrived at −39.0 RMS, effects
/// channel on or off, one channel or both (TotalMix writes the returns
/// that much hotter instead). With it, an echo at send −1.5, volume −8 and
/// return −1.5 came back 8 dB under the dry beep, as on Windows
/// (`references/measurements/playback-fx-beep.wav`: the knobs exactly).
const SEND_MAKEUP: f32 = 2.0; // +6.02 dB

fn take_send(block: &[u8], send: &mut [f32]) {
    let frames = (block.len() / (FX_CHANNELS * BYTES)).min(send.len() / 2);
    for f in 0..frames {
        send[2 * f] = sample(block, f, FX_PAIR) * SEND_MAKEUP;
        send[2 * f + 1] = sample(block, f, FX_PAIR + 1) * SEND_MAKEUP;
    }
}

/// A 14-channel block that is silent everywhere but the FX pair.
fn put_return(ret: &[f32], block: &mut [u8]) {
    block.fill(0);
    let frames = (block.len() / (FX_CHANNELS * BYTES)).min(ret.len() / 2);
    for f in 0..frames {
        for (side, value) in [ret[2 * f], ret[2 * f + 1]].into_iter().enumerate() {
            let at = (f * FX_CHANNELS + FX_PAIR + side) * BYTES;
            block[at..at + BYTES].copy_from_slice(&value.to_ne_bytes());
        }
    }
}

/// Keeps an eye on what PipeWire shows: a profile switch or an unplug
/// replaces the nodes and leaves the children alive but unlinked, so
/// they are killed and the worker starts over with the new ones.
fn watch(
    shared: &Shared,
    state: &Mutex<FxLoopState>,
    in_use: &Mutex<Option<(Monitor, Monitor)>>,
    children: &Mutex<Children>,
    stop: &AtomicBool,
) {
    let kill = || {
        if let Ok(mut c) = children.lock() {
            c.kill();
        }
    };
    let mut last = None;
    while !stop.load(Ordering::Relaxed) {
        sleep_unless(stop, RETRY);
        // Running with no block since the last look: the unit's pipe
        // stopped without closing (a suspended device), and the return
        // would stay silent for good. The children go, and the loop starts
        // again, as when the endpoints change (job 42, as Voice's
        // watchdog does).
        let running = matches!(state.lock().map(|s| *s), Ok(FxLoopState::Running(_)));
        let (seen, stuck) = stalled(running, last, shared.blocks.load(Ordering::Relaxed));
        last = seen;
        if stuck {
            log::warn!("Reverb & Echo: no sound from the unit for {} s; starting again", RETRY.as_secs());
            kill();
            continue;
        }
        let current = in_use.lock().ok().and_then(|u| u.clone());
        let Some((source, sink)) = current else { continue };
        let now = (pw_meter::discover(TapKind::Source), pw_meter::discover(TapKind::Sink));
        if now != (Some(source), Some(sink)) {
            kill();
        }
    }
}

/// The watchdog's look, every `RETRY`: running, and not one block since
/// the last look, is a stall. The count to keep for the next look (none
/// when not running, or after a stall), and whether it stalled.
fn stalled(running: bool, last: Option<u64>, blocks: u64) -> (Option<u64>, bool) {
    match (running, last) {
        (false, _) => (None, false),
        (true, Some(b)) if b == blocks => (None, true),
        (true, _) => (Some(blocks), false),
    }
}

fn sleep_unless(stop: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() && !stop.load(Ordering::Relaxed) {
        let nap = left.min(step);
        std::thread::sleep(nap);
        left -= nap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The watchdog: a stall is two looks in a row, running, with no block
    /// between; the first look after a start or a stall only remembers.
    #[test]
    fn the_watchdog_sees_a_stall_only_while_running_and_still() {
        assert_eq!(stalled(true, None, 0), (Some(0), false), "the first look remembers");
        assert_eq!(stalled(true, Some(0), 5), (Some(5), false), "blocks came");
        assert_eq!(stalled(true, Some(5), 5), (None, true), "none came: stalled");
        assert_eq!(stalled(true, None, 5), (Some(5), false), "after a stall, a fresh start");
        assert_eq!(stalled(false, Some(5), 5), (None, false), "not running: nothing to watch");
    }

    /// The pump counts its blocks, for the watchdog.
    #[test]
    fn the_pump_counts_its_blocks() {
        let shared = Shared { settings: Mutex::new(FxSettings::default()), generation: AtomicU64::new(0), auditions: AtomicU64::new(0), blocks: AtomicU64::new(0) };
        let block = PERIOD_FRAMES * FX_CHANNELS * BYTES;
        let mut engine = FxEngine::new(48_000.0);
        let mut out = Vec::new();
        let _ = pump(std::io::Cursor::new(vec![0u8; block * 3]), &mut out, &mut engine, &shared, &AtomicBool::new(false), &|| None);
        assert_eq!(shared.blocks.load(Ordering::Relaxed), 3);
    }

    fn endpoint(kind: TapKind, channels: usize) -> Monitor {
        Monitor { name: "alsa_output.usb-RME_Babyface_Pro.pro-output-0".into(), index: 77, rate: 48_000, channels, kind }
    }

    fn block_of(frames: usize, each: impl Fn(usize, usize) -> f32) -> Vec<u8> {
        let mut b = Vec::with_capacity(frames * FX_CHANNELS * BYTES);
        for f in 0..frames {
            for c in 0..FX_CHANNELS {
                b.extend_from_slice(&each(f, c).to_ne_bytes());
            }
        }
        b
    }

    #[test]
    fn says_what_is_missing_before_it_runs() {
        let (src, sink) = (TapKind::Source, TapKind::Sink);
        assert_eq!(plan(None, Some(&endpoint(sink, 14))), Err(FxLoopState::Searching));
        assert_eq!(plan(Some(&endpoint(src, 2)), Some(&endpoint(sink, 2))), Err(FxLoopState::NeedsProAudio));
        assert_eq!(plan(Some(&endpoint(src, 12)), Some(&endpoint(sink, 12))), Err(FxLoopState::NeedsDriver));
        assert_eq!(plan(Some(&endpoint(src, 14)), Some(&endpoint(sink, 12))), Err(FxLoopState::NeedsDriver));
        assert_eq!(plan(Some(&endpoint(src, 14)), Some(&endpoint(sink, 14))), Ok(48_000));
        let mut other_rate = endpoint(sink, 14);
        other_rate.rate = 44_100;
        assert_eq!(plan(Some(&endpoint(src, 14)), Some(&other_rate)), Err(FxLoopState::Searching));
        // Issue #22: at 96 kHz the frame is smaller and has no FX pair; it
        // says the rate, not Pro Audio or the driver.
        let (mut fast_src, mut fast_sink) = (endpoint(src, 8), endpoint(sink, 8));
        fast_src.rate = 96_000;
        fast_sink.rate = 96_000;
        assert_eq!(plan(Some(&fast_src), Some(&fast_sink)), Err(FxLoopState::RateTooHigh(96_000)));
        assert!(crate::fx::rate_too_high_words(96_000).contains("up to 48 kHz"));
        assert!(crate::fx::rate_too_high_words(88_200).contains("88.2 kHz"));
    }

    #[test]
    fn the_children_are_pinned_to_the_unit_channel_for_channel() {
        let play = play_args(&endpoint(TapKind::Sink, 14));
        let after = |flag: &str| play.iter().position(|a| a == flag).map(|i| play[i + 1].clone());
        assert_eq!(after("--target").as_deref(), Some("77"));
        assert_eq!(after("--channels").as_deref(), Some("14"));
        assert_eq!(after("--format").as_deref(), Some("f32"));
        assert_eq!(after("--channel-map").as_deref(), Some("AUX0,AUX1,AUX2,AUX3,AUX4,AUX5,AUX6,AUX7,AUX8,AUX9,AUX10,AUX11,AUX12,AUX13"));
        let props = after("--properties").unwrap();
        for p in ["node.dont-reconnect=true", "node.dont-fallback=true", "node.dont-move=true", "stream.dont-remix=true"] {
            assert!(props.contains(p), "{p} in {props}");
        }
        assert!(play.contains(&"--raw".to_string()) && play.last().map(String::as_str) == Some("-"));
        let record = record_args(&endpoint(TapKind::Source, 14));
        assert!(record.windows(2).any(|w| w[0] == "--media-category" && w[1] == "Capture"));
    }

    #[test]
    fn takes_the_send_and_returns_only_the_fx_pair() {
        let block = block_of(4, |f, c| (c * 10 + f) as f32);
        let mut send = vec![0.0; 8];
        take_send(&block, &mut send);
        // The FX pair, made up by the send's measured shortfall (+9 dB).
        let want: Vec<f32> = [120.0, 130.0, 121.0, 131.0, 122.0, 132.0, 123.0, 133.0].iter().map(|v| v * SEND_MAKEUP).collect();
        assert_eq!(send, want);
        let mut out = vec![0xffu8; block.len()];
        put_return(&[0.5, -0.5, 0.25, -0.25, 0.0, 0.0, 1.0, -1.0], &mut out);
        for f in 0..4 {
            for c in 0..FX_PAIR {
                assert_eq!(sample(&out, f, c), 0.0, "frame {f} ch{c} stays silent");
            }
        }
        assert_eq!((sample(&out, 0, 12), sample(&out, 0, 13)), (0.5, -0.5));
        assert_eq!((sample(&out, 3, 12), sample(&out, 3, 13)), (1.0, -1.0));
    }

    #[test]
    fn the_mics_never_leak_into_the_return_and_the_send_rings() {
        // Loud signal on every audio channel; one click on the FX send.
        let frames = PERIOD_FRAMES * 40;
        let input = block_of(frames, |f, c| match c {
            0..=11 => if f % 2 == 0 { 0.9 } else { -0.9 },
            _ => if f == 10 { 1.0 } else { 0.0 },
        });
        let shared = Shared {
            settings: Mutex::new(FxSettings { reverb_on: true, ..FxSettings::default() }),
            generation: AtomicU64::new(1),
            auditions: AtomicU64::new(0),
            blocks: AtomicU64::new(0),
        };
        let mut engine = FxEngine::new(48_000.0);
        engine.reverb_on = false;
        let mut out = Vec::new();
        let r = pump(std::io::Cursor::new(input), &mut out, &mut engine, &shared, &AtomicBool::new(false), &|| None);
        assert!(r.is_err(), "ends when the send closes");
        let total = CUSHION_FRAMES + frames;
        assert_eq!(out.len(), total * FX_CHANNELS * BYTES, "the cushion, then one block out per block in");
        let mut wet = 0.0f32;
        for f in 0..total {
            for c in 0..FX_PAIR {
                assert_eq!(sample(&out, f, c), 0.0);
            }
            wet = wet.max(sample(&out, f, 12).abs()).max(sample(&out, f, 13).abs());
        }
        assert!(wet > 1e-3, "the reverb answers the click: {wet}");
        assert!(engine.reverb_on, "the settings reached the engine");
    }

    /// On the unit: the loop finds it and says what it needs, or runs.
    /// `cargo test -p audra live_fx_loop -- --ignored --nocapture`
    #[test]
    #[ignore = "needs the unit"]
    fn live_fx_loop_finds_the_unit() {
        let fx = FxLoop::start(FxSettings { reverb_on: true, ..FxSettings::default() });
        let mut state = fx.state();
        for _ in 0..60 {
            if state != FxLoopState::Searching {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            state = fx.state();
        }
        println!("FX loop: {state:?}");
        assert_ne!(state, FxLoopState::Searching, "the unit was found");
        if let FxLoopState::Running(_) = state {
            // It stays up: the children linked and the blocks flow.
            std::thread::sleep(Duration::from_secs(2));
            println!("FX loop after 2 s: {:?}", fx.state());
            assert!(matches!(fx.state(), FxLoopState::Running(_)), "still running");
        }
    }

    #[test]
    fn the_queue_ahead_of_pw_play_stays_short_and_grows_only_when_dry() {
        let shared = Shared { settings: Mutex::new(FxSettings::default()), generation: AtomicU64::new(0), auditions: AtomicU64::new(0), blocks: AtomicU64::new(0) };
        let mut engine = FxEngine::new(48_000.0);
        let block = PERIOD_FRAMES * FX_CHANNELS * BYTES;
        let cushion = CUSHION_FRAMES * FX_CHANNELS * BYTES;
        let blocks = 6;
        let input = block_of(PERIOD_FRAMES * blocks, |_, _| 0.0);
        // pw-play not reading yet: the queue is past the cushion and a
        // block, so every block is dropped and nothing builds up.
        let mut out = Vec::new();
        let full = cushion + 2 * block;
        let _ = pump(std::io::Cursor::new(input.clone()), &mut out, &mut engine, &shared, &AtomicBool::new(false), &|| Some(full));
        assert_eq!(out.len(), cushion, "only the cushion was written");
        // A queue nearly dry: each block brings a block of silence before it
        // (the cushion grows), until the cushion reaches its most.
        let mut out = Vec::new();
        let _ = pump(std::io::Cursor::new(input), &mut out, &mut engine, &shared, &AtomicBool::new(false), &|| Some(0));
        let grown = ((CUSHION_MAX_FRAMES - CUSHION_FRAMES) / PERIOD_FRAMES).min(blocks);
        assert_eq!(out.len(), cushion + (blocks + grown) * block);
    }

    #[test]
    fn stopping_ends_the_loop_at_the_next_block() {
        let shared = Shared { settings: Mutex::new(FxSettings::default()), generation: AtomicU64::new(0), auditions: AtomicU64::new(0), blocks: AtomicU64::new(0) };
        let mut engine = FxEngine::new(48_000.0);
        let input = block_of(PERIOD_FRAMES * 4, |_, _| 0.0);
        let mut out = Vec::new();
        let r = pump(std::io::Cursor::new(input), &mut out, &mut engine, &shared, &AtomicBool::new(true), &|| None);
        assert!(r.is_ok());
        assert_eq!(out.len(), CUSHION_FRAMES * FX_CHANNELS * BYTES, "only the cushion");
    }
}
