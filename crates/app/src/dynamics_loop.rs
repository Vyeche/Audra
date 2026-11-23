//! Dynamics live (issue #27, the owner's pick: "go with option 1"): you
//! keep hearing your mic straight through the unit, with no delay, and
//! what others hear is processed. The round trip through the computer is
//! about 25 ms on this unit (`round_trip.rs`), too late to hear your own
//! voice comfortably, but nothing for a recording or a stream.
//!
//! Three children, all `pw-*` tools:
//! - `pw-loopback` makes a private virtual pair: a sink Audra writes into
//!   (`audra_dynamics_in`) and a source apps can pick as a mic, "Audra
//!   processed mic" (`audra_processed_mic`). Both at the lowest priority,
//!   so neither ever becomes the default device; it ends with Audra
//!   (`PR_SET_PDEATHSIG`), so a crash leaves no virtual mic behind.
//! - `pw-record` reads the unit's channels, pinned to the unit as the FX
//!   loop and the recorder are.
//! - `pw-play` writes the processed mic (stereo) into the private sink, by
//!   its serial and pinned, never falling back to another device: nothing
//!   this loop makes can reach a speaker.
//!
//! Between them one thread runs `audra_fx::Dynamics`. What waits ahead of
//! `pw-play` is held to a cushion (blocks are dropped rather than queued)
//! while no app is listening and the virtual pair stands still. A dropped
//! block's first millisecond is kept and faded into the next block
//! written, so a drop is a gap in time and not a click.
//!
//! An output's D (the owner's pick, 2026-11-10: "Computer sound") runs the
//! other way: `pw-loopback` makes a virtual speaker apps play to ("Audra
//! Phones 3/4") with a private tap behind it, `pw-record` reads the tap,
//! and `pw-play` writes the processed sound into the unit's playback
//! channels for that output, silent on every other channel (as the FX
//! loop does), pinned to the unit. The unit's own inputs never pass
//! through it, so your mic still reaches your ears at once.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use std::sync::atomic::AtomicU8;
use audra_fx::{Dynamics, DynamicsParams, VoiceChain};
use std::sync::atomic::AtomicI32;
use tuxmix_core::pw_meter::{self, TapKind};

/// The private sink Audra writes the processed mic into.
pub const SINK_NAME: &str = "audra_dynamics_in";
/// The virtual mic apps pick.
pub const SOURCE_NAME: &str = "audra_processed_mic";
/// What apps show for it.
pub const SOURCE_LABEL: &str = "Audra processed mic";

/// The virtual mic's PipeWire names: the real ones, or a test's own (so a
/// live test never collides with the owner's running Audra).
#[derive(Clone, Copy, Debug)]
struct Names {
    sink: &'static str,
    source: &'static str,
    label: &'static str,
}

const MIC: Names = Names { sink: SINK_NAME, source: SOURCE_NAME, label: SOURCE_LABEL };
#[cfg(test)]
const TEST_SOURCE: &str = "audra_test_processed_mic";
#[cfg(test)]
const TEST_MIC: Names = Names { sink: "audra_test_dynamics_in", source: TEST_SOURCE, label: "Audra test mic" };
const BLOCK_FRAMES: usize = 256;
/// Blocks allowed to wait ahead of `pw-play`; more are dropped.
const CUSHION_BLOCKS: usize = FLOOR_BLOCKS as usize + 2;
/// Blocks of silence written ahead at the start, so the player always has
/// sound waiting when its cycle comes. With none, the owner's takes from
/// the processed mic had every other 5.3 ms block silent (2026-11-10, 2 of
/// 3 takes: "the sound came out flaky"); the live test
/// (`live_processed_mic_holes`) found the same in 10 of 10 takes with no
/// floor, none with 2 or 4, and 4 keeping 2 blocks waiting steadily.
const FLOOR_BLOCKS: u8 = 4;
/// The delay of the path through PipeWire and back without what waits in
/// Audra's own queue: the measured 25 ms round trip less its one block.
const PATH_MS: f32 = 20.0;
/// How often the panel's readings are published: about a UI tick, so the
/// D panel's trace of your voice scrolls smoothly.
const PUBLISH_MS: u64 = 30;

/// Where the loop stands, for the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum DynState {
    Starting,
    Running,
    Failed(String),
}

struct Shared {
    params: Mutex<DynamicsParams>,
    generation: AtomicU64,
    state: Mutex<DynState>,
    /// The deepest turn-down lately, in dB (f32 bits).
    meter: AtomicU32,
    /// The loudest input lately, in dBFS (f32 bits), for the panel's dot.
    level: AtomicU32,
    /// Auto Level's gain now, in dB (f32 bits).
    auto: AtomicU32,
    /// Auto-Tune on (an input's loop only), and the note it pulls to now
    /// (MIDI number, -1 for none).
    tune: AtomicBool,
    tune_note: AtomicI32,
    /// The Cough key held: the processed mic fades to silence.
    cough: AtomicBool,
    /// What waits ahead of `pw-play` now, in ms (f32 bits): the part of
    /// the delay that grows when the computer is busy.
    queue_ms: AtomicU32,
    /// Noise removal: 0 off, 1 light, 2 strong (an input's loop only), and
    /// the delay it adds now, in ms (f32 bits).
    nr: AtomicU8,
    nr_ms: AtomicU32,
    /// Lower delay (the owner, 2026-11-10): noise removal and Auto-Tune
    /// paused and the queue kept short; the dynamics and the limit kept.
    low: AtomicBool,
    /// Auto-Tune's delay now, in ms (f32 bits; 0 while it's off).
    tune_ms: AtomicU32,
    /// The de-esser: 0 off, 1 light, 2 strong (no delay: Lower delay keeps
    /// it).
    deess: AtomicU8,
    /// Blocks processed, and those dropped with nobody listening.
    blocks: AtomicU64,
    dropped: AtomicU64,
    /// Blocks of silence written ahead at the start: the floor the player
    /// always has waiting.
    floor: AtomicU8,
    /// What waited at the player when each block was written, in bytes:
    /// the least, the sum and the count (for the live tests).
    q_min: AtomicU64,
    q_sum: AtomicU64,
    q_n: AtomicU64,
}

/// Which way a loop runs.
#[derive(Clone, Debug)]
enum Path {
    /// The unit's input channels (one, or a linked pair) to the virtual mic,
    /// under its names; `tone`: a live test's tone in place of the sound.
    Input { channels: Vec<usize>, names: Names, tone: bool },
    /// A virtual speaker (`node`, shown as `label`) to the unit's playback
    /// channels (one, or a linked pair) of an output.
    Output { channels: Vec<usize>, node: String, label: String },
}

/// The running loop; dropping it stops it and takes the virtual mic (or
/// speaker) away.
pub struct DynLoop {
    shared: Arc<Shared>,
    children: Arc<Mutex<Vec<Child>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl DynLoop {
    /// Starts processing input channels `channels` (one, or a linked pair)
    /// of the unit.
    pub fn start(channels: Vec<usize>, params: DynamicsParams) -> Self {
        Self::start_path(Path::Input { channels, names: MIC, tone: false }, params)
    }

    /// A live test's loop: its own names, a steady tone in place of the
    /// processed sound (so a hole shows as silence), `floor` blocks ahead.
    #[cfg(test)]
    fn start_test(channels: Vec<usize>, params: DynamicsParams, floor: u8) -> Self {
        Self::start_path_floor(Path::Input { channels, names: TEST_MIC, tone: true }, params, floor)
    }

    /// The least and the mean bytes waiting at the player.
    #[cfg(test)]
    fn queue_stats(&self) -> (u64, f64) {
        let n = self.shared.q_n.load(Ordering::Relaxed).max(1);
        (self.shared.q_min.load(Ordering::Relaxed), self.shared.q_sum.load(Ordering::Relaxed) as f64 / n as f64)
    }

    /// Starts an output's: a virtual speaker named `node` (shown as
    /// `label`) into the unit's playback `channels` (one, or a linked
    /// pair).
    pub fn start_output(channels: Vec<usize>, node: String, label: String, params: DynamicsParams) -> Self {
        Self::start_path(Path::Output { channels, node, label }, params)
    }

    fn start_path(path: Path, params: DynamicsParams) -> Self {
        Self::start_path_floor(path, params, FLOOR_BLOCKS)
    }

    fn start_path_floor(path: Path, params: DynamicsParams, floor: u8) -> Self {
        let shared = Arc::new(Shared {
            params: Mutex::new(params),
            generation: AtomicU64::new(1),
            state: Mutex::new(DynState::Starting),
            meter: AtomicU32::new(0f32.to_bits()),
            level: AtomicU32::new((-120f32).to_bits()),
            auto: AtomicU32::new(0f32.to_bits()),
            tune: AtomicBool::new(false),
            tune_note: AtomicI32::new(-1),
            cough: AtomicBool::new(false),
            queue_ms: AtomicU32::new(0f32.to_bits()),
            nr: AtomicU8::new(0),
            nr_ms: AtomicU32::new(0f32.to_bits()),
            low: AtomicBool::new(false),
            tune_ms: AtomicU32::new(0f32.to_bits()),
            deess: AtomicU8::new(0),
            blocks: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            floor: AtomicU8::new(floor),
            q_min: AtomicU64::new(u64::MAX),
            q_sum: AtomicU64::new(0),
            q_n: AtomicU64::new(0),
        });
        let children = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (shared, children, stop) = (shared.clone(), children.clone(), stop.clone());
            std::thread::Builder::new()
                .name("audra-dynamics".into())
                .spawn(move || {
                    // The loop's own thread; its `pw-*` helpers are raised
                    // as they start (`spawn`).
                    crate::priority::raise_this_thread("Voice");
                    let result = match &path {
                        Path::Input { channels, names, tone } => run(&shared, &children, &stop, channels, *names, *tone),
                        Path::Output { channels, node, label } => run_output(&shared, &children, &stop, channels, node, label),
                    };
                    kill_all(&children);
                    if let Err(why) = result {
                        set_state(&shared, DynState::Failed(why));
                    }
                })
                .ok()
        };
        Self { shared, children, stop, worker }
    }

    pub fn set(&self, params: DynamicsParams) {
        if let Ok(mut p) = self.shared.params.lock() {
            *p = params;
        }
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn state(&self) -> DynState {
        self.shared.state.lock().map(|s| s.clone()).unwrap_or(DynState::Failed("stopped".into()))
    }

    /// The deepest turn-down lately, in dB (0 or less).
    pub fn meter_db(&self) -> f32 {
        f32::from_bits(self.shared.meter.load(Ordering::Relaxed))
    }

    /// Auto-Tune on or off (an input's loop; an output's ignores it).
    pub fn set_tune(&self, on: bool) {
        self.shared.tune.store(on, Ordering::Relaxed);
    }

    /// About how late others hear the processed sound, in ms: the path
    /// through PipeWire and back as measured on this unit (`round_trip.rs`:
    /// about 25 ms with one block waiting), plus what waits ahead of
    /// `pw-play` beyond that block now.
    pub fn delay_ms(&self) -> f32 {
        let queued = f32::from_bits(self.shared.queue_ms.load(Ordering::Relaxed));
        let nr = f32::from_bits(self.shared.nr_ms.load(Ordering::Relaxed));
        let tune = f32::from_bits(self.shared.tune_ms.load(Ordering::Relaxed));
        PATH_MS + queued + nr + tune
    }

    /// Lower delay on or off: noise removal and Auto-Tune paused (the
    /// extras that cost time: about 20 ms and 17), the queue held to two
    /// blocks; the compressor, the expander and the limit, which add no
    /// delay, never touched.
    pub fn set_low_delay(&self, on: bool) {
        self.shared.low.store(on, Ordering::Relaxed);
    }

    /// The de-esser: 0 off, 1 light, 2 strong (an input's loop).
    pub fn set_deess(&self, level: u8) {
        self.shared.deess.store(level.min(2), Ordering::Relaxed);
    }

    /// Noise removal: 0 off, 1 light, 2 strong (an input's loop).
    pub fn set_nr(&self, level: u8) {
        self.shared.nr.store(level.min(2), Ordering::Relaxed);
    }

    /// The Cough key: held, what others hear fades to silence (10 ms, no
    /// click), and back when let go.
    pub fn set_cough(&self, held: bool) {
        self.shared.cough.store(held, Ordering::Relaxed);
    }

    /// The note Auto-Tune pulls to now (MIDI number), while it's on.
    pub fn tune_note(&self) -> Option<i32> {
        let n = self.shared.tune_note.load(Ordering::Relaxed);
        (n >= 0).then_some(n)
    }

    /// The loudest input lately, in dBFS.
    pub fn level_db(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// Auto Level's gain now, in dB (positive amplifies).
    pub fn auto_db(&self) -> f32 {
        f32::from_bits(self.shared.auto.load(Ordering::Relaxed))
    }

    /// Blocks processed so far: the watchdog's sign that sound still comes
    /// (`dynamics_panel::stalled`).
    pub fn blocks(&self) -> u64 {
        self.shared.blocks.load(Ordering::Relaxed)
    }

    /// Blocks processed and dropped so far (a check on long runs).
    #[cfg(test)]
    pub fn stats(&self) -> (u64, u64) {
        (self.shared.blocks.load(Ordering::Relaxed), self.shared.dropped.load(Ordering::Relaxed))
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        kill_all(&self.children);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for DynLoop {
    fn drop(&mut self) {
        self.stop();
    }
}

fn set_state(shared: &Shared, s: DynState) {
    if let Ok(mut st) = shared.state.lock() {
        *st = s;
    }
}

/// The virtual mic ("Audra processed mic") Voice writes into, kept across
/// a move to another mic and a restart after a stall: a call recording it
/// stays on it and hears a moment of silence, never moved by the session
/// manager to the default source, the raw mic (OpenCode's job 47: it went
/// and came back with every loop). Started on the app's own thread
/// (`keep_virtual_mic`), which lives as long as Audra: the signal that
/// ends a child with Audra goes with the thread that started it, and a
/// loop's worker ends on every move.
static VIRTUAL_MIC: Mutex<Option<Child>> = Mutex::new(None);

/// Keeps `slot`'s child running: a new one from `start` when there is none
/// or it has exited. Whether one was started.
fn keep_child(slot: &Mutex<Option<Child>>, start: impl FnOnce() -> Option<Child>) -> Result<bool, ()> {
    let mut kept = slot.lock().map_err(|_| ())?;
    if let Some(c) = kept.as_mut() {
        if matches!(c.try_wait(), Ok(None)) {
            return Ok(false);
        }
        let _ = c.wait();
    }
    *kept = Some(start().ok_or(())?);
    Ok(true)
}

/// Ends `slot`'s child, if there is one.
fn end_child(slot: &Mutex<Option<Child>>) {
    let taken = slot.lock().ok().and_then(|mut k| k.take());
    if let Some(mut c) = taken {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Starts the virtual mic if it isn't running. On the app's own thread
/// (see `VIRTUAL_MIC`), before a Voice loop starts.
pub fn keep_virtual_mic() -> Result<(), String> {
    keep_child(&VIRTUAL_MIC, || {
        // It carries the processed mic to apps; `spawn` raises it.
        spawn("pw-loopback", &loopback_args_for(MIC), Stdio::null(), Stdio::null(), true)
    })
        .map(|_| ())
        .map_err(|_| "pw-loopback couldn't be started (PipeWire's tools are needed).".into())
}

/// Ends the virtual mic: Voice is off, or Audra stops.
pub fn end_virtual_mic() {
    end_child(&VIRTUAL_MIC);
}

/// The kept virtual mic is running.
fn virtual_mic_kept() -> bool {
    VIRTUAL_MIC.lock().ok().is_some_and(|mut k| k.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None))))
}

fn kill_all(children: &Mutex<Vec<Child>>) {
    if let Ok(mut c) = children.lock() {
        // pw-play and pw-record first, the virtual pair last.
        while let Some(mut child) = c.pop() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `pw-loopback`'s arguments: the private sink and the virtual mic, both
/// at the lowest priority so neither becomes a default device.
#[cfg(test)]
pub(crate) fn loopback_args() -> Vec<String> {
    loopback_args_for(MIC)
}

fn loopback_args_for(names: Names) -> Vec<String> {
    let Names { sink, source, label } = names;
    let low = "priority.session=1 priority.driver=1 node.virtual=true";
    vec![
        "--name".into(),
        "audra-processed-mic".into(),
        "--channels".into(),
        "2".into(),
        "--channel-map".into(),
        "[ FL, FR ]".into(),
        "--capture-props".into(),
        format!("media.class=Audio/Sink node.name={sink} node.description=\"Audra dynamics (internal)\" {low}"),
        "--playback-props".into(),
        format!("media.class=Audio/Source node.name={source} node.description=\"{}\" {low}", pw_text(label)),
    ]
}

/// The unit's channels in, pinned to the unit.
pub(crate) fn record_args(target: u32, rate: u32, channels: usize) -> Vec<String> {
    let mut a = vec![
        "--target".to_string(),
        target.to_string(),
        "--raw".into(),
        "--format".into(),
        "f32".into(),
        "--rate".into(),
        rate.to_string(),
        "--channels".into(),
        channels.to_string(),
    ];
    if channels > 2 {
        a.extend(["--channel-map".into(), (0..channels).map(|i| format!("AUX{i}")).collect::<Vec<_>>().join(",")]);
    }
    a.extend(pinned("audra-dynamics-in", "Capture"));
    a
}

/// The processed mic out, into the private sink by its serial, pinned.
pub(crate) fn play_args(target: u32, rate: u32) -> Vec<String> {
    let mut a = vec![
        "--target".to_string(),
        target.to_string(),
        "--raw".into(),
        "--format".into(),
        "f32".into(),
        "--rate".into(),
        rate.to_string(),
        "--channels".into(),
        "2".into(),
    ];
    a.extend(pinned("audra-dynamics-out", "Playback"));
    a
}

fn pinned(node: &str, category: &str) -> Vec<String> {
    vec![
        "--latency".into(),
        BLOCK_FRAMES.to_string(),
        "--media-category".into(),
        category.into(),
        "--media-role".into(),
        "Production".into(),
        "--properties".into(),
        format!("node.name={node} node.dont-reconnect=true node.dont-fallback=true node.dont-move=true stream.dont-remix=true"),
        "-".into(),
    ]
}

/// Whether any stream plays into the sink named `name` now (`pactl list
/// sink-inputs short`): an app on an output's virtual speaker.
pub(crate) fn sink_in_use(name: &str) -> bool {
    let Some(id) = sink_serial(name) else { return false };
    let Ok(out) = Command::new("pactl").args(["list", "sink-inputs", "short"]).output() else { return false };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| l.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()) == Some(id))
}

/// Whether any app records from the source named `name` now (`pactl list
/// source-outputs short`): an app on the processed mic.
pub(crate) fn source_in_use(name: &str) -> bool {
    let Some(id) = source_serial(name) else { return false };
    let Ok(out) = Command::new("pactl").args(["list", "source-outputs", "short"]).output() else { return false };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| l.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()) == Some(id))
}

/// Whether any app records from the processed mic, checked every 2 s on a
/// thread of its own (`pactl` takes a moment; never on the UI thread).
/// Critique #35: Voice said "others hear you" with no app listening. It
/// stops within 100 ms of being dropped, without being waited for.
pub struct AppWatch {
    heard: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
}

impl AppWatch {
    /// Starts watching the source named `source`.
    pub fn start(source: &'static str) -> Self {
        let heard = Arc::new(AtomicU8::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (h, s) = (Arc::clone(&heard), Arc::clone(&stop));
        let spawned = std::thread::Builder::new().name("audra-app-watch".into()).spawn(move || {
            while !s.load(Ordering::Relaxed) {
                h.store(if source_in_use(source) { 2 } else { 1 }, Ordering::Relaxed);
                for _ in 0..20 {
                    if s.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        if spawned.is_err() {
            log::warn!("Could not start the processed mic's app watch");
        }
        AppWatch { heard, stop }
    }

    /// An app records from it (`Some(true)`), none does (`Some(false)`), or
    /// not checked yet.
    pub fn heard(&self) -> Option<bool> {
        match self.heard.load(Ordering::Relaxed) {
            0 => None,
            1 => Some(false),
            _ => Some(true),
        }
    }
}

impl Drop for AppWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The serial of the sink named `name` (`pactl list sinks short`), once it
/// shows.
fn sink_serial(name: &str) -> Option<u32> {
    let out = Command::new("pactl").args(["list", "sinks", "short"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let mut f = l.split_whitespace();
        let id = f.next()?.parse().ok()?;
        (f.next()? == name).then_some(id)
    })
}

fn spawn(tool: &str, args: &[String], stdin: Stdio, stdout: Stdio, end_with_audra: bool) -> Option<Child> {
    let mut cmd = Command::new(tool);
    cmd.args(args).stdin(stdin).stdout(stdout).stderr(Stdio::null());
    if end_with_audra {
        // SAFETY: only an async-signal-safe `prctl` between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    let child = cmd.spawn().ok()?;
    // Its audio thread as high as Voice's: rtkit's raise doesn't pass to
    // a child (it sets reset-on-fork; checked 2026-11-13, the child of a
    // raised process ran at nice 0).
    crate::priority::raise_child(child.id(), "a Voice helper");
    Some(child)
}

fn queued_bytes(fd: std::os::fd::RawFd) -> Option<usize> {
    let mut n: libc::c_int = 0;
    // SAFETY: FIONREAD writes one c_int through the pointer, which lives
    // for the call; `fd` is a pipe this thread holds open.
    let r = unsafe { libc::ioctl(fd, libc::FIONREAD, &mut n) };
    (r == 0 && n >= 0).then_some(n as usize)
}

fn run(shared: &Shared, children: &Mutex<Vec<Child>>, stop: &AtomicBool, channels: &[usize], names: Names, tone: bool) -> Result<(), String> {
    let source = pw_meter::discover(TapKind::Source).ok_or("Audra can't find the unit.")?;
    let need = channels.iter().max().copied().unwrap_or(0) + 1;
    if channels.is_empty() || need > source.channels {
        return Err(format!(
            "Only {} channels are open to the computer: switch the unit's profile to Pro Audio in the sound settings.",
            source.channels
        ));
    }
    let push = |c: Child| {
        if let Ok(mut v) = children.lock() {
            v.push(c);
        }
    };
    // The virtual pair: the kept one for Voice (`keep_virtual_mic`), else
    // this loop's own, ended with it.
    let kept = names.source == MIC.source && virtual_mic_kept();
    if !kept {
        push(spawn("pw-loopback", &loopback_args_for(names), Stdio::null(), Stdio::null(), true).ok_or("pw-loopback couldn't be started (PipeWire's tools are needed).")?);
    }
    // Wait for the private sink to show, for its serial.
    let started = Instant::now();
    let sink = loop {
        if let Some(id) = sink_serial(names.sink) {
            break id;
        }
        if stop.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(4) {
            if kept {
                // Running but not there (PipeWire restarted under it): the
                // next try starts a fresh one.
                end_virtual_mic();
            }
            return Err("the virtual mic didn't appear.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // Each child kept as soon as it runs, so an early return still ends and
    // reaps it (`kill_all`).
    let mut rec = spawn("pw-record", &record_args(source.index, source.rate, source.channels), Stdio::null(), Stdio::piped(), false)
        .ok_or("pw-record couldn't be started.")?;
    let out = rec.stdout.take();
    push(rec);
    let mut out = out.ok_or("pw-record gave no audio.")?;
    let in_fd = out.as_raw_fd();
    let mut play = spawn("pw-play", &play_args(sink, source.rate), Stdio::piped(), Stdio::null(), false).ok_or("pw-play couldn't be started.")?;
    let inp = play.stdin.take();
    push(play);
    let mut inp = inp.ok_or("pw-play took no audio.")?;
    let fd = inp.as_raw_fd();
    set_state(shared, DynState::Running);

    let width = source.channels * 4;
    let stereo = channels.len() > 1;
    // The whole chain, as the offline renderer and tests run it
    // (audra_fx::chain).
    let mut chain = VoiceChain::new(source.rate as f32, shared.params.lock().map(|p| *p).unwrap_or_default());
    let mut seen = shared.generation.load(Ordering::Relaxed);
    let mut block = vec![0u8; BLOCK_FRAMES * width];
    let (mut l, mut r) = (vec![0f32; BLOCK_FRAMES], vec![0f32; BLOCK_FRAMES]);
    let mut outb = vec![0u8; BLOCK_FRAMES * 8];
    // The first millisecond of a dropped block, kept so the sound can run
    // on from it into the next block written (a crossfade, so the join is
    // not a click). Made once, here.
    let fade_n = ((source.rate / 1000).max(1) as usize).min(BLOCK_FRAMES);
    let (mut fade_l, mut fade_r) = (vec![0.0f32; fade_n], vec![0.0f32; fade_n]);
    let mut fade_pending = false;
    let (mut meter_at, mut meter, mut peak) = (Instant::now(), 0f32, 0f32);
    // The floor: blocks of silence written ahead, so the player always has
    // sound waiting when its cycle comes; the caps sit above the floor in
    // use (a test's too, Qwen's review).
    let floor = shared.floor.load(Ordering::Relaxed) as usize;
    let cushion_full = (floor + 2).max(CUSHION_BLOCKS) * outb.len();
    if floor > 0 && inp.write_all(&vec![0u8; floor * outb.len()]).is_err() {
        return Err("the virtual mic went away.".into());
    }
    let mut tone_n = 0u64;
    while !stop.load(Ordering::Relaxed) {
        if out.read_exact(&mut block).is_err() {
            if !stop.load(Ordering::Relaxed) {
                return Err("the unit went away.".into());
            }
            break;
        }
        let g = shared.generation.load(Ordering::Relaxed);
        if g != seen {
            seen = g;
            if let Ok(p) = shared.params.lock() {
                chain.set_dynamics(*p);
            }
        }
        deinterleave(&block, width, channels, &mut l, &mut r);
        peak = peak.max(block_peak(&l, if stereo { Some(&r) } else { None }));
        // Lower delay pauses the extras that cost time.
        let low = shared.low.load(Ordering::Relaxed);
        chain.set_noise(if low { 0 } else { shared.nr.load(Ordering::Relaxed) });
        shared.nr_ms.store(chain.noise_ms().to_bits(), Ordering::Relaxed);
        chain.set_tune(shared.tune.load(Ordering::Relaxed) && !low);
        chain.set_deess(shared.deess.load(Ordering::Relaxed));
        chain.set_cough(shared.cough.load(Ordering::Relaxed));
        if stereo {
            chain.process(&mut l, Some(&mut r));
        } else {
            chain.process(&mut l, None);
            r.copy_from_slice(&l);
        }
        shared.tune_note.store(chain.tune_note().unwrap_or(-1), Ordering::Relaxed);
        shared.tune_ms.store(chain.tune_ms().to_bits(), Ordering::Relaxed);
        if tone {
            // A live test's steady tone: never silent, so a hole shows.
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                let v = 0.1 * (tone_n as f32 * std::f32::consts::TAU * 440.0 / source.rate as f32).sin();
                tone_n += 1;
                *a = v;
                *b = v;
            }
        }
        meter = meter.min(chain.dynamics_mut().take_meter_db());
        if meter_at.elapsed() >= Duration::from_millis(PUBLISH_MS) {
            publish(shared, meter, peak, chain.dynamics_mut().auto_gain_db());
            meter = 0.0;
            peak = 0.0;
            meter_at = Instant::now();
        }
        shared.blocks.fetch_add(1, Ordering::Relaxed);
        let queued = queued_bytes(fd);
        if let Some(q) = queued {
            shared.q_min.fetch_min(q as u64, Ordering::Relaxed);
            shared.q_sum.fetch_add(q as u64, Ordering::Relaxed);
            shared.q_n.fetch_add(1, Ordering::Relaxed);
        }
        // Lower delay holds the queue nearer the floor (drops sooner); never
        // under it, or every block would go (the floor keeps 2–2.5 waiting).
        let cushion = if low { floor.max(2) * outb.len() } else { cushion_full };
        // What waits: ahead of `pw-play`, and behind us in `pw-record`'s
        // pipe when processing falls behind (the delay that really grows; a
        // second look's catch: only the first was counted).
        let backlog = queued_bytes(in_fd).unwrap_or(0) as f32 / width as f32;
        if let Some(q) = queued {
            let ms = ((q + outb.len()) as f32 / 8.0 + backlog) / source.rate as f32 * 1000.0;
            shared.queue_ms.store(ms.to_bits(), Ordering::Relaxed);
        }
        // Past the cushion (nobody listening, or catching up after a
        // stall): drop the block, so others never hear you later and later.
        // A catch-up's drop is a gap, by choice: the voice stays in step
        // with the video (OpenCode's job 2 asked).
        if queued.is_some_and(|q| q + outb.len() > cushion) {
            shared.dropped.fetch_add(1, Ordering::Relaxed);
            // Keep the first millisecond of the dropped block: the sound
            // runs on from there into the next block written, so a drop is
            // a gap in time and not a click. The first dropped block
            // counts, because it follows the last one written: the join
            // there is smooth.
            if !fade_pending {
                fade_l.copy_from_slice(&l[..fade_n]);
                fade_r.copy_from_slice(&r[..fade_n]);
                fade_pending = true;
            }
            continue;
        }
        // The dropped block's start fades into the block written now.
        if fade_pending {
            crossfade_in(&fade_l[..], &fade_r[..], &mut l[..], &mut r[..], fade_n);
            fade_pending = false;
        }
        interleave(&l, &r, &mut outb);
        if inp.write_all(&outb).is_err() {
            if !stop.load(Ordering::Relaxed) {
                return Err("the virtual mic went away.".into());
            }
            break;
        }
    }
    Ok(())
}

/// Fades the block that was dropped into the block that is written now,
/// over `n` samples: the dropped one's start runs out as the new one
/// comes in. Without it the sound jumps from the end of the last written
/// block to the start of the next one, which is a click on every drop.
fn crossfade_in(from_l: &[f32], from_r: &[f32], to_l: &mut [f32], to_r: &mut [f32], n: usize) {
    let n = n.min(from_l.len()).min(from_r.len()).min(to_l.len()).min(to_r.len());
    for i in 0..n {
        let w = if n > 1 { (n - 1 - i) as f32 / (n - 1) as f32 } else { 0.0 };
        to_l[i] = from_l[i] * w + to_l[i] * (1.0 - w);
        to_r[i] = from_r[i] * w + to_r[i] * (1.0 - w);
    }
}

/// What the panel reads, every `PUBLISH_MS`: the deepest turn-down, the
/// loudest input and Auto Level's gain.
fn publish(shared: &Shared, meter: f32, peak: f32, auto: f32) {
    shared.meter.store(meter.to_bits(), Ordering::Relaxed);
    let db = if peak > 1e-6 { 20.0 * peak.log10() } else { -120.0 };
    shared.level.store(db.to_bits(), Ordering::Relaxed);
    shared.auto.store(auto.to_bits(), Ordering::Relaxed);
}

/// The loudest sample of a block (both sides of a pair).
fn block_peak(l: &[f32], r: Option<&[f32]>) -> f32 {
    let m = l.iter().fold(0f32, |m, v| if v.is_finite() { m.max(v.abs()) } else { m });
    r.map_or(m, |r| r.iter().fold(m, |m, v| if v.is_finite() { m.max(v.abs()) } else { m }))
}

/// A name as it may go inside a PipeWire property's quotes: a quote or a
/// backslash would end the value and let the rest of a name (a strip's
/// label, which a shared workspace file can carry) add properties of its
/// own, say turn the private tap into a playback stream to a speaker. So
/// those and control characters go, and the length is capped.
pub(crate) fn pw_text(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '"' | '\\') && !c.is_control()).take(64).collect()
}

/// `pw-loopback`'s arguments for an output's virtual speaker `node` (shown
/// as `label`) and its private tap, both at the lowest priority so neither
/// becomes a default device. The tap is a source nothing links to by
/// itself: only Audra's `pw-record` reads it, so the sound can't reach a
/// speaker unprocessed.
pub(crate) fn out_loopback_args(node: &str, label: &str) -> Vec<String> {
    let low = "priority.session=1 priority.driver=1 node.virtual=true";
    vec![
        "--name".into(),
        node.replace('_', "-"),
        "--channels".into(),
        "2".into(),
        "--channel-map".into(),
        "[ FL, FR ]".into(),
        "--capture-props".into(),
        format!("media.class=Audio/Sink node.name={node} node.description=\"{}\" {low}", pw_text(label)),
        "--playback-props".into(),
        format!("media.class=Audio/Source node.name={node}_tap node.description=\"{} (internal)\" {low}", pw_text(label)),
    ]
}

/// The virtual speaker's tap in (stereo), pinned to it.
pub(crate) fn tap_record_args(target: u32, rate: u32, node: &str) -> Vec<String> {
    let mut a = vec![
        "--target".to_string(),
        target.to_string(),
        "--raw".into(),
        "--format".into(),
        "f32".into(),
        "--rate".into(),
        rate.to_string(),
        "--channels".into(),
        "2".into(),
    ];
    a.extend(pinned(&format!("{node}-read"), "Capture"));
    a
}

/// The processed sound out to the unit: every one of its playback
/// channels (silent but the output's), pinned to the unit.
pub(crate) fn unit_play_args(target: u32, rate: u32, channels: usize, node: &str) -> Vec<String> {
    let mut a = vec![
        "--target".to_string(),
        target.to_string(),
        "--raw".into(),
        "--format".into(),
        "f32".into(),
        "--rate".into(),
        rate.to_string(),
        "--channels".into(),
        channels.to_string(),
    ];
    if channels > 2 {
        a.extend(["--channel-map".into(), (0..channels).map(|i| format!("AUX{i}")).collect::<Vec<_>>().join(",")]);
    }
    a.extend(pinned(&format!("{node}-out"), "Playback"));
    a
}

/// The serial of the source named `name` (`pactl list sources short`),
/// once it shows.
fn source_serial(name: &str) -> Option<u32> {
    let out = Command::new("pactl").args(["list", "sources", "short"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let mut f = l.split_whitespace();
        let id = f.next()?.parse().ok()?;
        (f.next()? == name).then_some(id)
    })
}

/// An output's loop: the virtual speaker's tap in, processed, out on the
/// output's playback channels of the unit.
fn run_output(shared: &Shared, children: &Mutex<Vec<Child>>, stop: &AtomicBool, channels: &[usize], node: &str, label: &str) -> Result<(), String> {
    let sink = pw_meter::discover(TapKind::Sink).ok_or("Audra can't find the unit.")?;
    let need = channels.iter().max().copied().unwrap_or(0) + 1;
    if channels.is_empty() || need > sink.channels {
        return Err(format!(
            "Only {} channels are open to the computer: switch the unit's profile to Pro Audio in the sound settings.",
            sink.channels
        ));
    }
    let push = |c: Child| {
        if let Ok(mut v) = children.lock() {
            v.push(c);
        }
    };
    push(spawn("pw-loopback", &out_loopback_args(node, label), Stdio::null(), Stdio::null(), true).ok_or("pw-loopback couldn't be started (PipeWire's tools are needed).")?);
    let tap_name = format!("{node}_tap");
    let started = Instant::now();
    let tap = loop {
        if let Some(id) = source_serial(&tap_name) {
            break id;
        }
        if stop.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(4) {
            return Err("the virtual speaker didn't appear.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let mut rec = spawn("pw-record", &tap_record_args(tap, sink.rate, node), Stdio::null(), Stdio::piped(), false).ok_or("pw-record couldn't be started.")?;
    let out = rec.stdout.take();
    push(rec);
    let mut out = out.ok_or("pw-record gave no audio.")?;
    let mut play = spawn("pw-play", &unit_play_args(sink.index, sink.rate, sink.channels, node), Stdio::piped(), Stdio::null(), false)
        .ok_or("pw-play couldn't be started.")?;
    let inp = play.stdin.take();
    push(play);
    let mut inp = inp.ok_or("pw-play took no audio.")?;
    let fd = inp.as_raw_fd();
    set_state(shared, DynState::Running);

    let width = sink.channels * 4;
    let stereo = channels.len() > 1;
    let mut dyn_ = Dynamics::new(sink.rate as f32, shared.params.lock().map(|p| *p).unwrap_or_default());
    let mut seen = shared.generation.load(Ordering::Relaxed);
    let mut block = vec![0u8; BLOCK_FRAMES * 8];
    let (mut l, mut r) = (vec![0f32; BLOCK_FRAMES], vec![0f32; BLOCK_FRAMES]);
    // Silent everywhere but the output's channels, which every block fills.
    let mut outb = vec![0u8; BLOCK_FRAMES * width];
    // A dropped block's first millisecond, faded into the next block
    // written, as the processed mic does (OpenCode's job 7 asked).
    let fade_n = ((sink.rate / 1000).max(1) as usize).min(BLOCK_FRAMES);
    let (mut fade_l, mut fade_r) = (vec![0.0f32; fade_n], vec![0.0f32; fade_n]);
    let mut fade_pending = false;
    let (mut meter_at, mut meter, mut peak) = (Instant::now(), 0f32, 0f32);
    let cushion = CUSHION_BLOCKS * outb.len();
    // The same floor as the input's: silence written ahead, so the unit's
    // playback never finds nothing waiting (with none, the processed mic
    // lost every other block).
    if inp.write_all(&vec![0u8; FLOOR_BLOCKS as usize * outb.len()]).is_err() {
        return Err("the unit's playback went away.".into());
    }
    while !stop.load(Ordering::Relaxed) {
        if out.read_exact(&mut block).is_err() {
            if !stop.load(Ordering::Relaxed) {
                return Err("the virtual speaker went away.".into());
            }
            break;
        }
        let g = shared.generation.load(Ordering::Relaxed);
        if g != seen {
            seen = g;
            if let Ok(p) = shared.params.lock() {
                dyn_.set(*p);
            }
        }
        deinterleave(&block, 8, &[0, 1], &mut l, &mut r);
        if !stereo {
            // One channel out: both sides of what apps play, as one.
            for (a, b) in l.iter_mut().zip(&r) {
                *a = (*a + *b) * 0.5;
            }
        }
        peak = peak.max(block_peak(&l, if stereo { Some(&r) } else { None }));
        if stereo {
            dyn_.process(&mut l, Some(&mut r));
        } else {
            dyn_.process(&mut l, None);
        }
        meter = meter.min(dyn_.take_meter_db());
        if meter_at.elapsed() >= Duration::from_millis(PUBLISH_MS) {
            publish(shared, meter, peak, dyn_.auto_gain_db());
            meter = 0.0;
            peak = 0.0;
            meter_at = Instant::now();
        }
        shared.blocks.fetch_add(1, Ordering::Relaxed);
        let queued = queued_bytes(fd);
        if let Some(q) = queued {
            shared.q_min.fetch_min(q as u64, Ordering::Relaxed);
            shared.q_sum.fetch_add(q as u64, Ordering::Relaxed);
            shared.q_n.fetch_add(1, Ordering::Relaxed);
        }
        if queued.is_some_and(|q| q + outb.len() > cushion) {
            shared.dropped.fetch_add(1, Ordering::Relaxed);
            if !fade_pending {
                fade_l.copy_from_slice(&l[..fade_n]);
                fade_r.copy_from_slice(&r[..fade_n]);
                fade_pending = true;
            }
            continue;
        }
        if fade_pending {
            crossfade_in(&fade_l[..], &fade_r[..], &mut l[..], &mut r[..], fade_n);
            fade_pending = false;
        }
        place(&l, if stereo { Some(&r) } else { None }, channels, width, &mut outb);
        if inp.write_all(&outb).is_err() {
            if !stop.load(Ordering::Relaxed) {
                return Err("the unit went away.".into());
            }
            break;
        }
    }
    Ok(())
}

/// `l` (and `r`) into channels `channels[0]` (and `[1]`) of whole `f32`
/// frames, `width` bytes each; the other channels are left as they are
/// (silent).
pub(crate) fn place(l: &[f32], r: Option<&[f32]>, channels: &[usize], width: usize, out: &mut [u8]) {
    for (i, f) in out.chunks_exact_mut(width).enumerate().take(l.len()) {
        let c = channels[0] * 4;
        f[c..c + 4].copy_from_slice(&l[i].to_le_bytes());
        if let (Some(r), Some(&c)) = (r, channels.get(1)) {
            f[c * 4..c * 4 + 4].copy_from_slice(&r[i].to_le_bytes());
        }
    }
}

/// Channels `channels[0]` (and `[1]`) of whole `f32` frames, `width` bytes
/// each, into `l` (and `r`).
pub(crate) fn deinterleave(bytes: &[u8], width: usize, channels: &[usize], l: &mut [f32], r: &mut [f32]) {
    let sample = |f: &[u8], c: usize| f32::from_le_bytes([f[c * 4], f[c * 4 + 1], f[c * 4 + 2], f[c * 4 + 3]]);
    for (i, f) in bytes.chunks_exact(width).enumerate().take(l.len()) {
        l[i] = sample(f, channels[0]);
        if let Some(&c) = channels.get(1) {
            r[i] = sample(f, c);
        }
    }
}

/// `l` and `r` as interleaved stereo `f32` bytes.
pub(crate) fn interleave(l: &[f32], r: &[f32], out: &mut [u8]) {
    for (i, (a, b)) in l.iter().zip(r).enumerate() {
        out[i * 8..i * 8 + 4].copy_from_slice(&a.to_le_bytes());
        out[i * 8 + 4..i * 8 + 8].copy_from_slice(&b.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The virtual mic's keeping, with a harmless stand-in child: kept while
    /// it runs, a new one once it's gone, nothing left after the end.
    #[test]
    fn a_kept_child_is_kept_replaced_when_gone_and_ended() {
        let slot = Mutex::new(None);
        let sleep = || Command::new("sleep").arg("30").spawn().ok();
        assert_eq!(keep_child(&slot, sleep), Ok(true), "none yet: started");
        let pid = slot.lock().unwrap().as_ref().unwrap().id();
        assert_eq!(keep_child(&slot, sleep), Ok(false), "running: kept");
        assert_eq!(slot.lock().unwrap().as_ref().unwrap().id(), pid, "the same one");
        {
            let mut k = slot.lock().unwrap();
            let c = k.as_mut().unwrap();
            let _ = c.kill();
            let _ = c.wait();
        }
        assert_eq!(keep_child(&slot, sleep), Ok(true), "gone: a new one");
        assert_ne!(slot.lock().unwrap().as_ref().unwrap().id(), pid);
        end_child(&slot);
        assert!(slot.lock().unwrap().is_none(), "ended");
        end_child(&slot);
        assert_eq!(keep_child(&slot, || None), Err(()), "a start that fails says so");
    }

    /// Why the virtual mic is started on the app's thread (OpenCode's job
    /// 47): a child that ends with Audra ends with the thread that started
    /// it. Started in a loop's worker, it goes when the worker does; started
    /// on a thread that stays, it outlives a worker ending.
    #[test]
    fn a_child_ending_with_audra_ends_with_the_thread_that_started_it() {
        let sleep = || spawn("sleep", &["30".to_string()], Stdio::null(), Stdio::null(), true);
        let gone_within = |c: &mut Child, ms: u64| {
            let until = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < until {
                if !matches!(c.try_wait(), Ok(None)) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            false
        };
        // Started by a worker that then ends.
        let mut from_worker = std::thread::spawn(sleep).join().unwrap().expect("sleep runs");
        assert!(gone_within(&mut from_worker, 2000), "ended with its worker");
        // Kept from this thread, while a worker comes and goes.
        let slot = Mutex::new(None);
        assert_eq!(keep_child(&slot, sleep), Ok(true));
        std::thread::spawn(|| std::thread::sleep(Duration::from_millis(20))).join().unwrap();
        let kept = slot.lock().unwrap().as_mut().map(|c| gone_within(c, 300));
        assert_eq!(kept, Some(false), "still running after the worker ended");
        end_child(&slot);
    }

    #[test]
    fn the_virtual_pair_never_becomes_a_default_and_the_streams_are_pinned() {
        let lb = loopback_args().join(" ");
        assert!(lb.contains("media.class=Audio/Sink") && lb.contains("media.class=Audio/Source"));
        assert_eq!(lb.matches("priority.session=1").count(), 2, "both at the lowest priority");
        assert!(lb.contains(SOURCE_NAME) && lb.contains(SOURCE_LABEL));
        for args in [record_args(77, 48_000, 14), play_args(91, 48_000)] {
            let props = args.iter().skip_while(|a| *a != "--properties").nth(1).unwrap().clone();
            for p in ["node.dont-reconnect=true", "node.dont-fallback=true", "node.dont-move=true", "stream.dont-remix=true"] {
                assert!(props.contains(p), "{p}");
            }
        }
        assert!(play_args(91, 48_000).windows(2).any(|w| w[0] == "--target" && w[1] == "91"), "into the private sink by serial");
    }

    /// On the real unit only (`cargo test -- --ignored live_dynamics`):
    /// the loop on ADAT 8 (capture channel 11, an input with nothing in
    /// it; never the mic), the virtual mic appearing while it runs and gone
    /// once it stops. Nothing is played anywhere.
    #[test]
    #[ignore]
    fn live_dynamics_makes_and_removes_the_virtual_mic() {
        let has_source = || {
            let out = Command::new("pactl").args(["list", "sources", "short"]).output().unwrap();
            String::from_utf8_lossy(&out.stdout).contains(SOURCE_NAME)
        };
        assert!(!has_source(), "no virtual mic before");
        let mut l = DynLoop::start(vec![11], DynamicsParams::default());
        let started = Instant::now();
        while l.state() == DynState::Starting && started.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(l.state(), DynState::Running);
        std::thread::sleep(Duration::from_millis(800));
        assert!(has_source(), "apps can see the virtual mic");
        l.stop();
        std::thread::sleep(Duration::from_millis(500));
        assert!(!has_source(), "gone once stopped");
        eprintln!("virtual mic made and removed");
    }

    /// On the real unit only (`cargo test -- --ignored live_output_dynamics
    /// -- --nocapture`), with the owner's go-ahead ("Okay please going",
    /// 2026-11-10): an output's loop on ADAT 7/8 (nothing plugged in), in
    /// SILENCE only, since the owner's Phones mix plays ADAT 7/8 PB. A
    /// silent "app" (a plain stream, free to be moved) plays into the
    /// virtual speaker. Checks: the speaker and its tap appear; the loop's
    /// out links only to the unit's own playback; only the loop reads the
    /// tap; the defaults never change; blocks flow. Then the loop stops
    /// with the app still playing, to see where an app goes when its
    /// virtual speaker disappears (printed).
    #[test]
    #[ignore]
    fn live_output_dynamics_silent_check() {
        let get = |what: &str| String::from_utf8_lossy(&Command::new("pactl").arg(what).output().unwrap().stdout).trim().to_string();
        let (sink0, src0) = (get("get-default-sink"), get("get-default-source"));
        let unit = pw_meter::discover(TapKind::Sink).expect("the unit");
        let sinks = String::from_utf8_lossy(&Command::new("pactl").args(["list", "sinks", "short"]).output().unwrap().stdout).to_string();
        let unit_name = sinks
            .lines()
            .find_map(|l| {
                let mut f = l.split_whitespace();
                (f.next()?.parse::<u32>().ok()? == unit.index).then(|| f.next().map(str::to_string)).flatten()
            })
            .expect("the unit's sink name");
        let node = "audra_out_check";
        let mut l = DynLoop::start_output(vec![10, 11], node.into(), "Audra check speaker".into(), audra_fx::dynamics::output_presets()[0].1);
        let started = Instant::now();
        while l.state() == DynState::Starting && started.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(l.state(), DynState::Running);
        std::thread::sleep(Duration::from_millis(800));
        let speaker = sink_serial(node).expect("the virtual speaker");
        assert!(source_serial(&format!("{node}_tap")).is_some(), "its tap");
        assert!(!sink_in_use(node), "no app on it yet");
        // The silent app: zeros, a plain stream.
        let mut app = Command::new("pw-play")
            .args(["--target", &speaker.to_string(), "--raw", "--format", "f32", "--rate", "48000", "--channels", "2", "--properties", "node.name=audra-check-app", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut feed = app.stdin.take().unwrap();
        let quit = Arc::new(AtomicBool::new(false));
        let feeder = {
            let quit = quit.clone();
            std::thread::spawn(move || {
                let zeros = vec![0u8; 4800 * 8];
                while !quit.load(Ordering::Relaxed) && feed.write_all(&zeros).is_ok() {
                    std::thread::sleep(Duration::from_millis(90));
                }
            })
        };
        std::thread::sleep(Duration::from_secs(3));
        let links = || String::from_utf8_lossy(&Command::new("pw-link").arg("-l").output().unwrap().stdout).to_string();
        // (port, linked port) for every link, both ways.
        let pairs = |all: &str| -> Vec<(String, String)> {
            let mut out = Vec::new();
            let mut port = String::new();
            for line in all.lines() {
                if !line.starts_with(' ') {
                    port = line.trim().to_string();
                } else if let Some(other) = line.trim().strip_prefix("|->").or_else(|| line.trim().strip_prefix("|<-")) {
                    out.push((port.clone(), other.trim().to_string()));
                }
            }
            out
        };
        let now = pairs(&links());
        let ours: Vec<&(String, String)> = now.iter().filter(|(a, b)| a.contains("audra") || b.contains("audra")).collect();
        for (a, b) in &ours {
            eprintln!("link: {a} <-> {b}");
        }
        let out_links: Vec<_> = now.iter().filter(|(a, _)| a.starts_with(&format!("{node}-out:"))).collect();
        assert!(!out_links.is_empty(), "the loop's out is linked");
        assert!(out_links.iter().all(|(_, b)| b.starts_with(&format!("{unit_name}:"))), "only into the unit's playback: {out_links:?}");
        let tap_links: Vec<_> = now.iter().filter(|(a, _)| a.starts_with(&format!("{node}_tap:"))).collect();
        assert!(tap_links.iter().all(|(_, b)| b.starts_with(&format!("{node}-read:"))), "only the loop reads the tap: {tap_links:?}");
        assert!(sink_in_use(node), "Audra sees the app on it (what the off switch and quitting ask about)");
        let (blocks, dropped) = l.stats();
        let (q_min, q_mean) = l.queue_stats();
        eprintln!("blocks {blocks}, dropped {dropped}; queue at the unit min {q_min} B, mean {q_mean:.0} B; level {:.1} dBFS, turn-down {:.1} dB", l.level_db(), l.meter_db());
        assert!(blocks > 100, "blocks flow");
        assert_eq!((get("get-default-sink"), get("get-default-source")), (sink0.clone(), src0.clone()), "defaults unchanged");
        // The speaker goes with the app still playing: where does it go?
        l.stop();
        std::thread::sleep(Duration::from_millis(1500));
        let after = pairs(&links());
        let app_links: Vec<_> = after.iter().filter(|(a, _)| a.starts_with("audra-check-app:")).collect();
        eprintln!("after the speaker went, the silent app links to: {app_links:?}");
        quit.store(true, Ordering::Relaxed);
        let _ = app.kill();
        let _ = app.wait();
        let _ = feeder.join();
        std::thread::sleep(Duration::from_millis(300));
        assert!(sink_serial(node).is_none(), "the speaker is gone");
        assert_eq!((get("get-default-sink"), get("get-default-source")), (sink0, src0), "defaults unchanged after");
    }

    /// On the real unit only (`cargo test -- --ignored live_dynamics_end`):
    /// 30 s with the loop on ADAT 8 (nothing in it), a quiet tone fed into
    /// the private sink (heard nowhere: it only shows the path carries
    /// sound), and Rec's own code taking "Audra processed mic" into a
    /// temporary file. Checks, while it runs: the default sink and source
    /// never change, and no Audra node links to the unit's playback. After:
    /// the file carries the tone, and the blocks dropped are counted.
    #[test]
    #[ignore]
    fn live_dynamics_end_to_end_soak() {
        let get = |what: &str| String::from_utf8_lossy(&Command::new("pactl").arg(what).output().unwrap().stdout).trim().to_string();
        let (sink0, source0) = (get("get-default-sink"), get("get-default-source"));
        let mut l = DynLoop::start(vec![11], DynamicsParams::default());
        let started = Instant::now();
        while l.state() == DynState::Starting && started.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(l.state(), DynState::Running);
        let serial = sink_serial(SINK_NAME).expect("the private sink");
        let mut tone = Command::new("pw-play").args(play_args(serial, 48_000)).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let mut tin = tone.stdin.take().unwrap();
        let feeder = std::thread::spawn(move || {
            let mut n = 0u64;
            let mut block = vec![0u8; BLOCK_FRAMES * 8];
            loop {
                for f in 0..BLOCK_FRAMES {
                    let v = 0.03 * ((n as f32) * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin();
                    n += 1;
                    block[f * 8..f * 8 + 4].copy_from_slice(&v.to_le_bytes());
                    block[f * 8 + 4..f * 8 + 8].copy_from_slice(&v.to_le_bytes());
                }
                if tin.write_all(&block).is_err() {
                    return;
                }
            }
        });
        let dir = std::env::temp_dir().join(format!("audra-live-dyn-{}", std::process::id()));
        let path = dir.join("processed.wav");
        let mut rec = crate::recorder::Recorder::start_from(Some(SOURCE_NAME), vec![0], path.clone(), None);
        let mut bad_links = Vec::new();
        for _ in 0..30 {
            std::thread::sleep(Duration::from_secs(1));
            assert_eq!(get("get-default-sink"), sink0, "the default sink never changes");
            assert_eq!(get("get-default-source"), source0, "the default source never changes");
            let links = String::from_utf8_lossy(&Command::new("pw-link").arg("-l").output().unwrap().stdout).to_string();
            let mut port = String::new();
            for line in links.lines() {
                if !line.starts_with(' ') {
                    port = line.to_string();
                } else if line.contains("|->") || line.contains("|<-") {
                    let peer = line.trim_start_matches([' ', '|', '-', '<', '>']).trim();
                    let audra = port.contains("audra") || peer.contains("audra");
                    let unit_out = (port.contains("alsa_output") && port.contains("Babyface") && !port.contains("monitor"))
                        || (peer.contains("alsa_output") && peer.contains("Babyface") && !peer.contains("monitor"));
                    if audra && unit_out {
                        bad_links.push(format!("{port} {peer}"));
                    }
                }
            }
        }
        rec.stop();
        let (blocks, dropped) = l.stats();
        l.stop();
        let _ = tone.kill();
        let _ = tone.wait();
        let _ = feeder.join();
        assert!(bad_links.is_empty(), "links into the unit's playback: {bad_links:?}");
        let bytes = std::fs::read(&path).unwrap();
        let peak = bytes[44..]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|s| ((i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0).abs())
            .fold(0.0f32, f32::max);
        eprintln!(
            "recorded {} s from the processed mic, peak {:.4}; {blocks} blocks, {dropped} dropped",
            (bytes.len() - 44) / 3 / 48_000,
            peak
        );
        assert!(peak > 0.01, "the processed mic carries sound");
        assert_eq!((get("get-default-sink"), get("get-default-source")), (sink0, source0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On the real unit only, silent (`cargo test -- --ignored
    /// live_processed_mic_holes --nocapture`): the input loop under a
    /// test's own names, on ADAT 8 (nothing in it, never the mic), a steady
    /// tone in place of the processed sound, recorded by Rec's own code
    /// from the test's virtual mic, start to stop, many times over: the
    /// silent 5.3 ms holes the owner heard (2026-11-10: every other block
    /// silent in 2 of 3 takes) are counted. Nothing is played anywhere.
    /// `AUDRA_TEST_FLOORS` (e.g. "0,2,4") picks the floors tried,
    /// `AUDRA_TEST_CYCLES` how often each.
    #[test]
    #[ignore]
    fn live_processed_mic_holes() {
        let floors: Vec<u8> = std::env::var("AUDRA_TEST_FLOORS").unwrap_or_else(|_| FLOOR_BLOCKS.to_string()).split(',').filter_map(|f| f.trim().parse().ok()).collect();
        let cycles: usize = std::env::var("AUDRA_TEST_CYCLES").ok().and_then(|c| c.parse().ok()).unwrap_or(10);
        // Never a pass with nothing tried (Qwen's review).
        assert!(!floors.is_empty() && cycles > 0, "no floors or no takes");
        let mut worst = 0usize;
        for &floor in &floors {
            let mut holed = 0;
            for cycle in 0..cycles {
                let mut l = DynLoop::start_test(vec![11], DynamicsParams::default(), floor);
                l.set_low_delay(std::env::var("AUDRA_TEST_LOW").is_ok_and(|v| v == "1"));
                let started = Instant::now();
                while l.state() == DynState::Starting && started.elapsed() < Duration::from_secs(6) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                assert_eq!(l.state(), DynState::Running, "the test loop runs");
                std::thread::sleep(Duration::from_millis(300));
                let dir = std::env::temp_dir().join(format!("audra-live-holes-{}-{floor}-{cycle}", std::process::id()));
                let path = dir.join("take.wav");
                let mut rec = crate::recorder::Recorder::start_from(Some(TEST_SOURCE), vec![0], path.clone(), None);
                let secs: f32 = std::env::var("AUDRA_TEST_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2.5);
                std::thread::sleep(Duration::from_secs_f32(secs));
                rec.stop();
                let (blocks, dropped) = l.stats();
                let (q_min, q_mean) = l.queue_stats();
                l.stop();
                let bytes = std::fs::read(&path).unwrap_or_default();
                let samples: Vec<f32> = bytes.get(44..).unwrap_or_default().as_chunks::<3>().0.iter().map(|s| (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0).collect();
                // Skip the first 50 ms (the floor's own silence, Rec's start).
                let body = samples.get(2_400..).unwrap_or_default();
                // A missing or empty take is a failure, not "no holes".
                assert!(body.len() > 48_000, "the take is too short: {} samples", body.len());
                assert!(body.iter().fold(0.0f32, |m, v| m.max(v.abs())) > 0.05, "the take carries the tone");
                let (mut holes, mut run) = (0usize, 0usize);
                for v in body {
                    if v.abs() < 1e-6 {
                        run += 1;
                        if run == 48 {
                            holes += 1;
                        }
                    } else {
                        run = 0;
                    }
                }
                if holes > 0 {
                    holed += 1;
                }
                worst = worst.max(holes);
                eprintln!("floor {floor} take {cycle}: {:.1} s, {holes} holes; queue at the player min {q_min} B, mean {q_mean:.0} B; {blocks} blocks, {dropped} dropped", body.len() as f32 / 48_000.0);
                let _ = std::fs::remove_dir_all(&dir);
            }
            eprintln!("floor {floor}: {holed} of {cycles} takes had holes");
        }
        assert_eq!(worst, 0, "the processed mic has holes");
    }

    #[test]
    fn a_mono_mic_comes_out_on_both_sides() {
        // Two frames of three channels; the mic is channel 1.
        let mut bytes = Vec::new();
        for frame in [[0.0f32, 0.5, 0.0], [0.0, -0.25, 0.9]] {
            for v in frame {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let (mut l, mut r) = (vec![0.0; 2], vec![0.0; 2]);
        deinterleave(&bytes, 12, &[1], &mut l, &mut r);
        assert_eq!(l, vec![0.5, -0.25]);
        r.copy_from_slice(&l);
        let mut out = vec![0u8; 16];
        interleave(&l, &r, &mut out);
        assert_eq!(f32::from_le_bytes(out[4..8].try_into().unwrap()), 0.5);
        assert_eq!(f32::from_le_bytes(out[12..16].try_into().unwrap()), -0.25);
    }

    /// The join where a block was dropped: the dropped block's start fades
    /// into the block written next, so the step there is no bigger than
    /// the tones' own (job 7).
    #[test]
    fn the_drop_crossfade_joins_without_a_step() {
        let sr = 48_000.0f32;
        let n = 48;
        let tone = |hz: f32, phase: f32, len: usize| {
            // The phase is in cycles.
            (0..len).map(|i| 0.5 * (((i as f32) + phase * sr / hz) * std::f32::consts::TAU * hz / sr).sin()).collect::<Vec<f32>>()
        };
        // The dropped block's start (440 Hz, carrying on from the block
        // written before it) and the block written now (1 kHz, a quarter
        // cycle on: without the fade the join would jump).
        let a = tone(440.0, 0.0, n);
        let (mut bl, mut br) = (tone(1000.0, 0.25, n + 1), tone(1000.0, 0.25, n + 1));
        let own = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        // The sample written before the drop: the 440 Hz tone, one sample on.
        let before = -0.5 * (std::f32::consts::TAU * 440.0 / sr).sin();
        let limit = own(&tone(440.0, 0.0, n + 2)).max(own(&tone(1000.0, 0.25, n + 2)));
        // Without the fade, the join would jump by the whole difference
        // between the two tones.
        let jump = (bl[0] - before).abs();
        crossfade_in(&a, &a, &mut bl, &mut br, n);
        // The steps at the join: into the faded part and out of it.
        let step_in = (bl[0] - before).abs();
        let step_out = (bl[n] - bl[n - 1]).abs();
        eprintln!(
            "the join: without the fade a step of {jump:.5}, with it {step_in:.5} into the join and {step_out:.5} out of it (the tones' own {limit:.5})"
        );
        assert!(jump > limit, "the tones are far enough apart to show the jump");
        assert!(step_in <= limit, "a step of {step_in} into the join (the tones' own {limit})");
        assert!(step_out <= limit, "a step of {step_out} out of the join (the tones' own {limit})");
    }

    #[test]
    fn an_outputs_sound_lands_only_on_its_own_channels_and_its_speaker_never_becomes_a_default() {
        // Phones 3/4 of a 14-channel frame: channels 2 and 3, the rest silent.
        let (l, r) = (vec![0.25f32; 4], vec![-0.5f32; 4]);
        let mut out = vec![0u8; 4 * 14 * 4];
        place(&l, Some(&r), &[2, 3], 14 * 4, &mut out);
        for f in out.as_chunks::<{ 14 * 4 }>().0 {
            for c in 0..14 {
                let v = f32::from_le_bytes(f[c * 4..c * 4 + 4].try_into().unwrap());
                let want = match c {
                    2 => 0.25,
                    3 => -0.5,
                    _ => 0.0,
                };
                assert_eq!(v, want, "channel {c}");
            }
        }
        let a = out_loopback_args("audra_out_2", "Audra Phones 3/4").join(" ");
        assert_eq!(a.matches("priority.session=1").count(), 2, "{a}");
        assert!(a.contains("media.class=Audio/Sink node.name=audra_out_2 "), "{a}");
        // The tap is a source, not a stream: nothing links it to a speaker.
        assert!(a.contains("media.class=Audio/Source node.name=audra_out_2_tap "), "{a}");
        let p = unit_play_args(7, 48_000, 14, "audra_out_2").join(" ");
        assert!(p.contains("node.dont-fallback=true") && p.contains("node.dont-move=true") && p.contains("stream.dont-remix=true"), "{p}");
        assert!(p.contains("AUX13"), "every channel by its own position: {p}");
    }
}


#[cfg(test)]
mod pw_text_tests {
    #[test]
    fn a_name_cant_add_pipewire_properties() {
        let evil = "Speakers\" media.class=Stream/Output/Audio target.object=alsa_output \\x";
        let a = super::out_loopback_args("audra_out_2", evil).join(" ");
        // Two descriptions, each one quoted value: nothing gets out of the
        // quotes to become a property of its own.
        assert_eq!(a.matches('"').count(), 4, "{a}");
        let outside: String = a.split('"').step_by(2).collect();
        assert!(!outside.contains("Stream") && !outside.contains("target.object"), "{outside}");
        assert_eq!(super::pw_text("Main\u{7}\"ok\""), "Mainok");
        assert_eq!(super::pw_text(&"x".repeat(200)).len(), 64);
    }
}
