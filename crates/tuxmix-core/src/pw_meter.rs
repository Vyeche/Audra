//! Per-channel meters from PipeWire: the sink monitor for playback, the
//! card's own source for the inputs.
//!
//! Capturing the input source instead of opening the card's capture PCM
//! directly matters: holding that PCM removes the input node from the
//! graph, and its fixed period is what made the card report 1024 frames
//! instead of the graph's quantum.
//!
//! The device exposes no playback-meter registers, and the kernel
//! driver's capture stream only carries a mixed-down playback tap (one
//! stereo pair for every PB channel), so per-PB levels have to come from
//! the host mixer. PipeWire exposes the card sink's monitor, which is
//! the mixed playback at the sink's own channel count: 2 channels in the
//! default stereo profile (PB1 only) and 12 channels in the "Pro Audio"
//! profile (all six PB pairs), with the same order as the device's
//! playback channels.
//!
//! `pw-record` is the capture child: it ships with PipeWire, needs no
//! build dependency, and can write raw S32LE to stdout. The sink is
//! discovered through `pactl --format=json list sinks` (PipeWire's
//! Pulse compatibility, present wherever `pactl` is), and the child is
//! told to capture the sink itself (`stream.capture.sink=true`), which
//! links it to that sink's monitor ports. The target is the sink's
//! Pulse index, not its name: with a name, a suspended target silently
//! falls back to the default sink instead (caught live: the Pro Audio
//! record linked to the laptop speakers while the Babyface was
//! suspended). If either tool or the Babyface sink is missing, the
//! meters simply stay unavailable and the rest of the app is unaffected.
//!
//! The tap is disabled in mock mode: the mock generates its own playback
//! levels and must not depend on the host's audio server.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// The Pro Audio profile's channel count: the 12 audio channels, then
/// the FX pair (the driver opens frame ch12/13 since 2026-11-05: the FX
/// send on capture, the FX return on playback). Anything past this is
/// not a Babyface layout and is ignored; a 12-channel node (an older
/// driver) still is one.
const MAX_CHANNELS: usize = 14;
/// Frames per read: at 48 kHz this is ~21 ms, enough for a smooth meter
/// and small enough to keep the mutex cheap.
const PERIOD_FRAMES: usize = 1024;
/// Room in a tap's pipe for several periods. The default 64 KB held barely
/// one 14-channel period (57 KB), so a reader a moment late made
/// `pw-record` block in its data thread, and the graph counted an xrun on
/// the meters (OpenCode's job 16). The FX loop grows its pipes the same way.
const PIPE_BYTES: i32 = 256 * 1024;

/// Grows `out`'s pipe to `PIPE_BYTES` (best effort: the default still
/// works, with less slack).
pub fn grow_pipe(out: &impl std::os::fd::AsRawFd) {
    // SAFETY: F_SETPIPE_SZ takes an int and touches no memory of ours; the
    // descriptor is open for the call.
    unsafe {
        libc::fcntl(out.as_raw_fd(), libc::F_SETPIPE_SZ, PIPE_BYTES);
    }
}
/// How long to wait before re-discovering the monitor after a failure
/// or after the capture child exits (profile switch, unplug, etc.).
const RETRY: Duration = Duration::from_secs(2);

/// Which PipeWire endpoint to capture: a sink's monitor (playback) or a
/// real source (the card's own inputs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapKind {
    Sink,
    Source,
}

/// A captured PipeWire endpoint: its Pulse name and index (the index is
/// the `pw-record` target), plus its rate and channel count. Public so the
/// FX loop finds the unit the same way the meters do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    pub name: String,
    pub index: u32,
    pub rate: u32,
    pub channels: usize,
    pub kind: TapKind,
}

/// The widest Babyface sink or source PipeWire shows right now, if any
/// (one `pactl` call; blocking, so not for the UI thread).
pub fn discover(kind: TapKind) -> Option<Monitor> {
    discover_monitor(kind)
}

/// Samples a scope keeps: a 2048-point spectrum and, at 192 kHz, two
/// periods of a low voice for the tuner.
pub const SCOPE_LEN: usize = 8192;

/// The last samples of one channel or a pair (averaged), for a spectrum
/// (Audra's EQ panel). With no channels it keeps nothing and costs
/// nothing.
#[derive(Default)]
struct Scope {
    channels: Vec<usize>,
    ring: Vec<f32>,
    pos: usize,
    rate: u32,
}

/// Owns the capture child and its reader thread. `drain()` is the only
/// thing the UI touches; the rest (discovery, restart, cleanup) happens
/// on the worker thread.
pub struct Tap {
    levels: Arc<Mutex<[f32; MAX_CHANNELS]>>,
    /// Sum of squares per channel and the frames it covers, since the last
    /// `drain_rms` (the RMS meters).
    energy: Arc<Mutex<Energy>>,
    /// The longest run of full-scale samples per channel since the last
    /// `drain_overs`.
    overs: Arc<Mutex<[u32; MAX_CHANNELS]>>,
    channels: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    handle: Option<JoinHandle<()>>,
    watch: Option<JoinHandle<()>>,
    scope: Arc<Mutex<Scope>>,
}

impl Tap {
    /// Start discovering and capturing a Babyface endpoint. Cheap: the
    /// actual work runs on a background thread, and a missing
    /// `pactl`/`pw-record` just leaves the meters at zero channels.
    pub fn start(kind: TapKind) -> Self {
        let levels = Arc::new(Mutex::new([0.0; MAX_CHANNELS]));
        let energy = Arc::new(Mutex::new(([0.0f64; MAX_CHANNELS], 0u64)));
        let overs = Arc::new(Mutex::new([0u32; MAX_CHANNELS]));
        let channels = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let wanted: Arc<Mutex<Option<Monitor>>> = Arc::new(Mutex::new(None));
        let scope = Arc::new(Mutex::new(Scope::default()));

        let handle = {
            let levels = Arc::clone(&levels);
            let energy = Arc::clone(&energy);
            let overs = Arc::clone(&overs);
            let channels = Arc::clone(&channels);
            let stop = Arc::clone(&stop);
            let child = Arc::clone(&child);
            let wanted = Arc::clone(&wanted);
            let scope = Arc::clone(&scope);
            std::thread::spawn(move || run(levels, energy, overs, channels, stop, child, wanted, scope))
        };
        let watch = {
            let stop = Arc::clone(&stop);
            let child = Arc::clone(&child);
            let wanted = Arc::clone(&wanted);
            std::thread::spawn(move || watch_monitor(wanted, child, stop, kind))
        };

        Self {
            levels,
            energy,
            overs,
            channels,
            stop,
            child,
            handle: Some(handle),
            watch: Some(watch),
            scope,
        }
    }

    /// The playback side: the sink's monitor.
    pub fn start_playback() -> Self {
        Self::start(TapKind::Sink)
    }

    /// The input side: the card's own source, so the app never holds the
    /// capture PCM itself.
    pub fn start_input() -> Self {
        Self::start(TapKind::Source)
    }

    /// How many channels the current endpoint has, 0 while none is being
    /// captured.
    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Relaxed)
    }

    /// A tap that never runs: mock mode, where the mock's own playback
    /// meters are authoritative.
    pub fn disabled() -> Self {
        Self {
            levels: Arc::new(Mutex::new([0.0; MAX_CHANNELS])),
            energy: Arc::new(Mutex::new(([0.0; MAX_CHANNELS], 0))),
            overs: Arc::new(Mutex::new([0; MAX_CHANNELS])),
            channels: Arc::new(AtomicUsize::new(0)),
            stop: Arc::new(AtomicBool::new(true)),
            child: Arc::new(Mutex::new(None)),
            handle: None,
            watch: None,
            scope: Arc::new(Mutex::new(Scope::default())),
        }
    }

    /// Which channels the scope keeps (one, or a pair averaged); none
    /// stops it. A change starts it afresh.
    pub fn scope_channels(&self, channels: &[usize]) {
        if let Ok(mut s) = self.scope.lock() {
            if s.channels != channels {
                s.channels = channels.to_vec();
                s.ring.clear();
                s.pos = 0;
            }
        }
    }

    /// The scope's samples, oldest first, and the rate they were taken at
    /// (empty until it has some).
    pub fn scope_recent(&self) -> (Vec<f32>, u32) {
        let Ok(s) = self.scope.lock() else { return (Vec::new(), 0) };
        let (head, tail) = s.ring.split_at(s.pos.min(s.ring.len()));
        let mut out = Vec::with_capacity(s.ring.len());
        out.extend_from_slice(tail);
        out.extend_from_slice(head);
        (out, s.rate)
    }

    /// Peak levels since the last call, plus the monitor's channel count
    /// (0 while no monitor is being captured). Levels past the channel
    /// count are always zero; the UI gates availability on the count.
    pub fn drain(&self) -> ([f32; MAX_CHANNELS], usize) {
        let channels = self.channels.load(Ordering::Relaxed);
        let Ok(mut peaks) = self.levels.lock() else {
            return ([0.0; MAX_CHANNELS], channels);
        };
        let out = *peaks;
        *peaks = [0.0; MAX_CHANNELS];
        (out, channels)
    }

    /// RMS level (0.0-1.0 of full scale) per channel over everything read
    /// since the last call, which resets it: the loudness the RMS meters
    /// show, where `drain` gives the peaks.
    pub fn drain_rms(&self) -> [f32; MAX_CHANNELS] {
        let Ok(mut e) = self.energy.lock() else {
            return [0.0; MAX_CHANNELS];
        };
        let (sumsq, frames) = *e;
        *e = ([0.0; MAX_CHANNELS], 0);
        rms_of(&sumsq, frames)
    }

    /// The longest run of consecutive full-scale samples per channel since
    /// the last call, which resets it (the meters' OVR, manual 21.7).
    pub fn drain_overs(&self) -> [u32; MAX_CHANNELS] {
        let Ok(mut o) = self.overs.lock() else {
            return [0; MAX_CHANNELS];
        };
        std::mem::replace(&mut *o, [0; MAX_CHANNELS])
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Killing the child closes its stdout, which unblocks the reader
        // thread even if it is waiting for the next period.
        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if let Some(watch) = self.watch.take() {
            let _ = watch.join();
        }
    }
}

/// Worker loop: wait for the watchdog's current monitor, spawn a capture
/// for it, read until the child exits or the app stops, then retry. If
/// the watchdog replaced the monitor while the child was reading, it
/// also killed the child, so the loop restarts immediately with the new
/// one.
#[allow(clippy::too_many_arguments)]
fn run(
    levels: Arc<Mutex<[f32; MAX_CHANNELS]>>,
    energy: Arc<Mutex<Energy>>,
    overs: Arc<Mutex<[u32; MAX_CHANNELS]>>,
    channels: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    slot: Arc<Mutex<Option<Child>>>,
    wanted: Arc<Mutex<Option<Monitor>>>,
    scope: Arc<Mutex<Scope>>,
) {
    while !stop.load(Ordering::Relaxed) {
        let Some(monitor) = wanted.lock().ok().and_then(|w| w.clone()) else {
            sleep_interruptible(&stop, Duration::from_millis(200));
            continue;
        };
        let Some(mut child) = spawn_record(&monitor) else {
            sleep_interruptible(&stop, RETRY);
            continue;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            sleep_interruptible(&stop, RETRY);
            continue;
        };
        grow_pipe(&stdout);
        // A profile switch between the clone above and the spawn is a
        // narrow race: the watchdog may have already killed the old
        // child and moved on, so re-check and drop this one instead of
        // leaving it unlinked forever.
        let still_wanted = wanted
            .lock()
            .ok()
            .is_some_and(|w| w.as_ref() == Some(&monitor));
        if !still_wanted {
            let _ = child.kill();
            let _ = child.wait();
            continue;
        }
        channels.store(monitor.channels, Ordering::Relaxed);
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(child);
        }
        if let Ok(mut s) = scope.lock() {
            s.rate = monitor.rate;
        }
        read_peaks(stdout, &levels, &energy, &overs, monitor.channels, &stop, &scope);
        channels.store(0, Ordering::Relaxed);
        if let Ok(mut slot) = slot.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let unchanged = wanted
            .lock()
            .ok()
            .is_some_and(|w| w.as_ref() == Some(&monitor));
        if unchanged {
            sleep_interruptible(&stop, RETRY);
        }
    }
}

/// Keep `wanted` in sync with what Pulse/PipeWire currently exposes. A
/// profile switch replaces the sink node (and its channel count), and
/// the old `pw-record` child stays alive but unlinked, so it has to be
/// killed for the reader loop to pick the new monitor up.
fn watch_monitor(
    wanted: Arc<Mutex<Option<Monitor>>>,
    slot: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
    kind: TapKind,
) {
    loop {
        let discovered = discover_monitor(kind);
        let changed = {
            let Ok(mut wanted) = wanted.lock() else {
                return;
            };
            if *wanted != discovered {
                *wanted = discovered;
                true
            } else {
                false
            }
        };
        if changed {
            // Killing closes stdout, which unblocks `read_peaks`.
            if let Ok(mut slot) = slot.lock() {
                if let Some(mut child) = slot.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
        sleep_interruptible(&stop, RETRY);
    }
}

/// Ask Pulse/PipeWire for its sinks or sources and pick the widest
/// Babyface endpoint. `pactl` prints one JSON object per endpoint; the
/// name carries the card and profile, and `sample_specification` is a
/// string like `"s32le 12ch 48000Hz"`.
fn discover_monitor(kind: TapKind) -> Option<Monitor> {
    let list = match kind {
        TapKind::Sink => "sinks",
        TapKind::Source => "sources",
    };
    let output = Command::new("pactl")
        .args(["--format=json", "list", list])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_monitor(&output.stdout, kind)
}

fn parse_monitor(json: &[u8], kind: TapKind) -> Option<Monitor> {
    let sinks: serde_json::Value = serde_json::from_slice(json).ok()?;
    let sinks = sinks.as_array()?;
    let mut best: Option<Monitor> = None;
    for sink in sinks {
        let Some(name) = sink.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        if !name.contains("Babyface") {
            continue;
        }
        // A source list also carries every sink's monitor; the input
        // side wants the real capture node, not the playback tap.
        if kind == TapKind::Source && name.ends_with(".monitor") {
            continue;
        }
        let Some(spec) = sink.get("sample_specification").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some((rate, channels)) = parse_spec(spec) else {
            continue;
        };
        if channels == 0 || channels > MAX_CHANNELS {
            continue;
        }
        let Some(index) = sink.get("index").and_then(|v| v.as_u64()) else {
            continue;
        };
        let candidate = Monitor {
            name: name.to_owned(),
            index: index as u32,
            rate,
            channels,
            kind,
        };
        if best
            .as_ref()
            .is_none_or(|b| candidate.channels > b.channels)
        {
            best = Some(candidate);
        }
    }
    best
}

/// `"s32le 12ch 48000Hz"` -> `(48000, 12)`. Unknown tokens are ignored;
/// a missing rate or channel count is a parse failure.
fn parse_spec(spec: &str) -> Option<(u32, usize)> {
    let mut rate = None;
    let mut channels = None;
    for token in spec.split_whitespace() {
        if let Some(n) = token.strip_suffix("ch") {
            channels = n.parse::<usize>().ok();
        } else if let Some(n) = token.strip_suffix("Hz") {
            rate = n.parse::<u32>().ok();
        }
    }
    Some((rate?, channels?))
}

fn spawn_record(monitor: &Monitor) -> Option<Child> {
    let mut cmd = Command::new("pw-record");
    cmd.arg("--target")
        // The index (PipeWire's Pulse object serial) is the reliable
        // target while the endpoint is suspended; a name falls back to
        // the default endpoint, which may be an entirely different card.
        .arg(monitor.index.to_string());
    if monitor.kind == TapKind::Sink {
        // Without this, the target would still record a source; the
        // property is what links the stream to the sink's monitor ports.
        cmd.arg("--properties").arg("stream.capture.sink=true");
    }
    cmd.arg("--format")
        .arg("s32")
        .arg("--rate")
        .arg(monitor.rate.to_string())
        .arg("--channels")
        .arg(monitor.channels.to_string())
        .arg("--media-category")
        .arg("Capture")
        .arg("--raw")
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// Read raw S32LE frames from the child and accumulate per-channel
/// peaks into `levels`. Returns on EOF/error (child exit, profile
/// switch) or when the app is stopping.
fn read_peaks(
    mut out: impl Read,
    levels: &Mutex<[f32; MAX_CHANNELS]>,
    energy: &Mutex<Energy>,
    overs: &Mutex<[u32; MAX_CHANNELS]>,
    channels: usize,
    stop: &AtomicBool,
    scope: &Mutex<Scope>,
) {
    if channels == 0 {
        return;
    }
    let frame_bytes = channels * 4;
    let mut buf = vec![0u8; PERIOD_FRAMES * frame_bytes];
    let mut run = [0u32; MAX_CHANNELS];
    while !stop.load(Ordering::Relaxed) {
        if out.read_exact(&mut buf).is_err() {
            return;
        }
        let longest = count_overs(&mut run, &buf, channels);
        if let Ok(mut o) = overs.lock() {
            for (slot, l) in o.iter_mut().zip(longest.iter()) {
                *slot = (*slot).max(*l);
            }
        }
        if let Ok(mut s) = scope.lock() {
            add_scope(&mut s, &buf, channels);
        }
        let mut local = [0f32; MAX_CHANNELS];
        add_peaks(&mut local, &buf, channels);
        let mut sumsq = [0f64; MAX_CHANNELS];
        let frames = add_energy(&mut sumsq, &buf, channels);
        if let Ok(mut e) = energy.lock() {
            for (slot, v) in e.0.iter_mut().zip(sumsq.iter()) {
                *slot += v;
            }
            e.1 += frames;
        }
        if let Ok(mut peaks) = levels.lock() {
            for (slot, value) in peaks.iter_mut().zip(local.iter()) {
                if *value > *slot {
                    *slot = *value;
                }
            }
        }
    }
}

/// The scope's channels of one interleaved S32LE buffer into its ring
/// (a pair averaged), as -1..1.
fn add_scope(s: &mut Scope, bytes: &[u8], channels: usize) {
    if s.channels.is_empty() || channels == 0 || s.channels.iter().any(|&c| c >= channels) {
        return;
    }
    let frame_bytes = channels * 4;
    let gain = 1.0 / (s.channels.len() as f32 * 2_147_483_648.0);
    for frame in bytes.chunks_exact(frame_bytes) {
        let sum: f32 = s
            .channels
            .iter()
            .map(|&c| i32::from_le_bytes([frame[c * 4], frame[c * 4 + 1], frame[c * 4 + 2], frame[c * 4 + 3]]) as f32)
            .sum();
        let v = sum * gain;
        if s.ring.len() < SCOPE_LEN {
            s.ring.push(v);
            s.pos = s.ring.len() % SCOPE_LEN;
        } else {
            let at = s.pos;
            s.ring[at] = v;
            s.pos = (at + 1) % SCOPE_LEN;
        }
    }
}

/// Fold one interleaved S32LE buffer into `local` (max absolute value
/// per channel, scaled to 0..=1). Split out so the conversion is
/// testable without a PipeWire server.
///
/// Each buffer's mean is subtracted per channel first. The sink monitor
/// of a suspended/idle sink carries a constant DC offset here (measured
/// live: about 0.07 FS on one channel and 0.28 FS on the other), which
/// otherwise lights the meter permanently. Removing the per-buffer mean
/// is a cheap DC blocker and also removes any very-low-frequency rumble
/// below the buffer rate, which is fine for a level meter.
fn add_peaks(local: &mut [f32; MAX_CHANNELS], bytes: &[u8], channels: usize) {
    let frame_bytes = channels * 4;
    if frame_bytes == 0 {
        return;
    }
    let frames = bytes.len() / frame_bytes;
    if frames == 0 {
        return;
    }
    let mut sums = [0f64; MAX_CHANNELS];
    for frame in bytes.chunks_exact(frame_bytes) {
        for (ch, sample) in frame.as_chunks::<4>().0.iter().enumerate() {
            if ch >= MAX_CHANNELS {
                break;
            }
            sums[ch] += i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f64;
        }
    }
    let means = sums.map(|sum| sum / frames as f64);
    for frame in bytes.chunks_exact(frame_bytes) {
        for (ch, sample) in frame.as_chunks::<4>().0.iter().enumerate() {
            if ch >= MAX_CHANNELS {
                break;
            }
            let value =
                i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f64 - means[ch];
            let level = (value.abs() / i32::MAX as f64) as f32;
            if level > local[ch] {
                local[ch] = level;
            }
        }
    }
}

/// Sum of squares per channel (in full scale, 1.0 = `i32::MAX`) and the
/// frame count it covers.
type Energy = ([f64; MAX_CHANNELS], u64);

/// Adds one interleaved S32LE buffer's squares into `sumsq`, after the
/// same per-buffer DC removal as `add_peaks`, and returns its frame count.
fn add_energy(sumsq: &mut [f64; MAX_CHANNELS], bytes: &[u8], channels: usize) -> u64 {
    let frame_bytes = channels * 4;
    if frame_bytes == 0 {
        return 0;
    }
    let frames = bytes.len() / frame_bytes;
    if frames == 0 {
        return 0;
    }
    let sample = |s: &[u8]| i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f64 / i32::MAX as f64;
    let mut sums = [0f64; MAX_CHANNELS];
    for frame in bytes.chunks_exact(frame_bytes) {
        for (ch, s) in frame.as_chunks::<4>().0.iter().enumerate().take(MAX_CHANNELS) {
            sums[ch] += sample(s);
        }
    }
    let means = sums.map(|sum| sum / frames as f64);
    for frame in bytes.chunks_exact(frame_bytes) {
        for (ch, s) in frame.as_chunks::<4>().0.iter().enumerate().take(MAX_CHANNELS) {
            let v = sample(s) - means[ch];
            sumsq[ch] += v * v;
        }
    }
    frames as u64
}

/// A sample at or above this magnitude counts as full scale (0.999).
const FULL_SCALE: u32 = (i32::MAX as f64 * 0.999) as u32;

/// Counts consecutive full-scale samples per channel through one
/// interleaved S32LE buffer, `run` carrying the count across buffers, and
/// returns each channel's longest run in it.
fn count_overs(run: &mut [u32; MAX_CHANNELS], bytes: &[u8], channels: usize) -> [u32; MAX_CHANNELS] {
    let mut longest = [0u32; MAX_CHANNELS];
    let frame_bytes = channels * 4;
    if frame_bytes == 0 {
        return longest;
    }
    for frame in bytes.chunks_exact(frame_bytes) {
        for (ch, s) in frame.as_chunks::<4>().0.iter().enumerate().take(MAX_CHANNELS) {
            let v = i32::from_le_bytes([s[0], s[1], s[2], s[3]]).unsigned_abs();
            run[ch] = if v >= FULL_SCALE { run[ch] + 1 } else { 0 };
            longest[ch] = longest[ch].max(run[ch]);
        }
    }
    longest
}

/// RMS per channel from a sum of squares over `frames`.
pub(crate) fn rms_of<const N: usize>(sumsq: &[f64; N], frames: u64) -> [f32; N] {
    if frames == 0 {
        return [0.0; N];
    }
    sumsq.map(|s| ((s / frames as f64).sqrt()) as f32)
}

/// Sleep in short steps so `Drop` does not wait the full retry delay.
fn sleep_interruptible(stop: &AtomicBool, total: Duration) {
    let mut left = total;
    while !stop.load(Ordering::Relaxed) && !left.is_zero() {
        let step = left.min(Duration::from_millis(100));
        std::thread::sleep(step);
        left -= step;
    }
}

#[cfg(test)]
mod pipe_tests {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    #[test]
    fn a_taps_pipe_grows_to_hold_several_periods() {
        // OpenCode's job 16: the default pipe held barely one 14-channel
        // period, and a reader a moment late made the graph drop a buffer.
        let mut fds = [0i32; 2];
        // SAFETY: pipe() writes two descriptors into the array it's given.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        // SAFETY: both descriptors were just opened and are owned here.
        let (read, _write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        super::grow_pipe(&read);
        // SAFETY: F_GETPIPE_SZ reads the size; the descriptor is open.
        let size = unsafe { libc::fcntl(read.as_raw_fd(), libc::F_GETPIPE_SZ) };
        let period = super::PERIOD_FRAMES * 14 * 4;
        assert!(size as usize >= 4 * period, "{size} bytes, a period is {period}");
    }
}

#[cfg(test)]
mod tests {
    use super::{add_energy, add_peaks, add_scope, count_overs, parse_monitor, parse_spec, rms_of, Scope, TapKind, MAX_CHANNELS, SCOPE_LEN};

    #[test]
    fn takes_the_fourteen_channel_node_the_fx_driver_opens() {
        // Since the driver opens the FX pair (frame ch12/13, 2026-11-05),
        // the Pro Audio node has 14 channels; the tap must not skip it.
        let json = br#"[
            {"index":2629,"name":"alsa_output.usb-RME_Babyface_Pro__x.pro-output-0",
             "sample_specification":"s32le 14ch 48000Hz"}
        ]"#;
        let monitor = parse_monitor(json, TapKind::Sink).expect("the 14-channel sink");
        assert_eq!(monitor.channels, 14);
        assert_eq!(parse_monitor(br#"[{"index":1,"name":"Babyface.x","sample_specification":"s32le 16ch 48000Hz"}]"#, TapKind::Sink).map(|m| m.channels), None, "past 14 is not a Babyface");
    }

    #[test]
    fn picks_the_widest_babyface_sink() {
        let json = br#"[
            {"index":11,"name":"alsa_output.pci-0.HiFi",
             "sample_specification":"s32le 2ch 48000Hz"},
            {"index":2628,"name":"alsa_output.usb-RME_Babyface_Pro__x.stereo-fallback",
             "sample_specification":"s32le 2ch 48000Hz"},
            {"index":2629,"name":"alsa_output.usb-RME_Babyface_Pro__x.pro-output-0",
             "sample_specification":"s32le 12ch 48000Hz"}
        ]"#;
        let monitor = parse_monitor(json, TapKind::Sink).expect("a Babyface sink");
        assert_eq!(monitor.index, 2629);
        assert_eq!(monitor.channels, 12);
        assert_eq!(monitor.rate, 48_000);
        assert!(monitor.name.ends_with("pro-output-0"));
    }

    #[test]
    fn ignores_sinks_that_are_not_babyface() {
        let json = br#"[
            {"index":11,"name":"alsa_output.pci-0.HiFi",
             "sample_specification":"s32le 2ch 48000Hz"},
            {"index":12,"name":"alsa_output.usb-Other",
             "sample_specification":"s32le 12ch 48000Hz"}
        ]"#;
        assert!(parse_monitor(json, TapKind::Sink).is_none());
    }

    #[test]
    fn picks_the_babyface_input_source_and_skips_monitors() {
        let json = br#"[
            {"index":11,"name":"alsa_input.pci-0.HiFi__Mic1__source",
             "sample_specification":"s32le 2ch 48000Hz"},
            {"index":234,"name":"alsa_output.usb-RME_Babyface_Pro__x.pro-output-0.monitor",
             "sample_specification":"s32le 12ch 48000Hz"},
            {"index":235,"name":"alsa_input.usb-RME_Babyface_Pro__x.pro-input-0",
             "sample_specification":"s32le 12ch 48000Hz"}
        ]"#;
        let source = parse_monitor(json, TapKind::Source).expect("a Babyface source");
        assert_eq!(source.index, 235);
        assert_eq!(source.channels, 12);
        assert_eq!(source.kind, TapKind::Source);
    }

    #[test]
    fn parses_the_sample_specification() {
        assert_eq!(parse_spec("s32le 2ch 48000Hz"), Some((48_000, 2)));
        assert_eq!(parse_spec("s32le 12ch 96000Hz"), Some((96_000, 12)));
        assert_eq!(parse_spec("garbage"), None);
    }

    #[test]
    fn folds_interleaved_samples_into_per_channel_peaks() {
        // Three frames: ch0 alternates +/-1000 (mean 0, peak 1000), ch1
        // is a constant 500 (pure DC, must be removed).
        let mut bytes = Vec::new();
        for (left, right) in [(1000i32, 500i32), (0, 500), (-1000, 500)] {
            bytes.extend_from_slice(&left.to_le_bytes());
            bytes.extend_from_slice(&right.to_le_bytes());
        }
        let mut peaks = [0.0f32; MAX_CHANNELS];
        add_peaks(&mut peaks, &bytes, 2);
        assert!((peaks[0] - 1000.0 / i32::MAX as f32).abs() < 1e-9);
        assert_eq!(peaks[1], 0.0);
        assert_eq!(peaks[2], 0.0);
    }

    #[test]
    fn a_sine_reads_3_db_under_its_peak_in_rms() {
        let mut bytes = Vec::new();
        let n = 4800;
        for i in 0..n {
            let v = (0.5 * (std::f64::consts::TAU * 1000.0 * i as f64 / 48000.0).sin() * i32::MAX as f64) as i32;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let mut sumsq = [0f64; MAX_CHANNELS];
        let frames = add_energy(&mut sumsq, &bytes, 1);
        let rms = rms_of(&sumsq, frames)[0];
        // 0.5 peak sine: RMS 0.354 (-3 dB under the peak).
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.005, "{rms}");
    }

    #[test]
    fn counts_consecutive_full_scale_samples_across_buffers() {
        let frame = |v: i32| v.to_le_bytes().to_vec();
        let buf = |vals: &[i32]| vals.iter().flat_map(|&v| frame(v)).collect::<Vec<u8>>();
        let mut run = [0u32; MAX_CHANNELS];
        // Two at full scale, a dip, three at full scale (negative counts too).
        let first = count_overs(&mut run, &buf(&[i32::MAX, i32::MAX, 1000, i32::MIN, i32::MIN + 1, i32::MAX]), 1);
        assert_eq!(first[0], 3);
        // The run carries into the next buffer.
        let second = count_overs(&mut run, &buf(&[i32::MAX, 0]), 1);
        assert_eq!(second[0], 4);
    }

    #[test]
    fn keeps_a_full_scale_alternating_signal() {
        let mut bytes = Vec::new();
        for value in [i32::MAX, i32::MIN, i32::MAX, i32::MIN] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut peaks = [0.0f32; MAX_CHANNELS];
        add_peaks(&mut peaks, &bytes, 1);
        assert!(peaks[0] > 0.99);
    }

    #[test]
    fn the_scope_keeps_its_channels_averaged_and_wraps() {
        let frame = |vals: [i32; 3]| vals.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let mut s = Scope { channels: vec![1, 2], ..Scope::default() };
        let half = i32::MAX / 2;
        let bytes: Vec<u8> = [frame([0, half, half]), frame([0, half, 0])].concat();
        add_scope(&mut s, &bytes, 3);
        assert_eq!(s.ring.len(), 2);
        assert!((s.ring[0] - 0.5).abs() < 1e-3 && (s.ring[1] - 0.25).abs() < 1e-3, "{:?}", s.ring);
        // Past its length it keeps the newest, in order.
        let many: Vec<u8> = (0..SCOPE_LEN + 3).flat_map(|_| frame([0, 0, 0])).collect();
        add_scope(&mut s, &many, 3);
        assert_eq!(s.ring.len(), SCOPE_LEN);
        // A channel past the stream's width keeps nothing.
        let mut off = Scope { channels: vec![5], ..Scope::default() };
        add_scope(&mut off, &bytes, 3);
        assert!(off.ring.is_empty());
    }

    /// On the real unit only (`cargo test -p tuxmix-core -- --ignored
    /// live_scope`): reads only (as the meters do), plays nothing. The
    /// input tap's scope on AN1 fills with samples at the unit's rate.
    #[test]
    #[ignore]
    fn live_scope_keeps_an_inputs_samples() {
        let tap = super::Tap::start_input();
        tap.scope_channels(&[0]);
        let started = std::time::Instant::now();
        let mut got = (Vec::new(), 0);
        while started.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(200));
            got = tap.scope_recent();
            if got.0.len() >= SCOPE_LEN {
                break;
            }
        }
        let peak = got.0.iter().fold(0f32, |m, v| m.max(v.abs()));
        eprintln!("scope: {} samples at {} Hz, peak {:.4}", got.0.len(), got.1, peak);
        assert_eq!(got.0.len(), SCOPE_LEN, "it fills");
        assert!(got.1 > 0 && got.0.iter().all(|v| v.is_finite()));
    }
}
