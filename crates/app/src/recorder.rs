//! Recording to files (issue #28; the owner, 2026-11-10, comparing the
//! Babyface Pro FS with the Fireface UCX II / UFX III: "DURec (Direct USB
//! Recording): Not Available"). The Babyface has no port for a USB drive,
//! so the computer does DURec's job: one press records the chosen inputs to
//! a WAV file, another stops.
//!
//! One `pw-record` child reads every channel the unit's source has, pinned
//! to the unit as the meters and the FX loop are (never moved, never sent to
//! a fallback device, never remixed): unplugged, it ends, and the file ends
//! with it rather than carrying on from another mic. A thread keeps the
//! chosen channels as 24-bit PCM.
//!
//! The file is never overwritten (`create_new`), its header is refreshed
//! every second so a crash leaves a file that plays, and it is finished on
//! stop, on the unit going away and on drop. It stops itself, saying why,
//! before the WAV format's 4 GB limit or a nearly full disk.
//!
//! It needs the real unit: the UI keeps it inert in the simulation (and so
//! in every capture and probe run), since `discover` finds the real unit
//! whatever the UI is simulating.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tuxmix_core::pw_meter::{self, Monitor, TapKind};

/// Bytes a sample takes as `pw-record` hands it over (`s32`).
const IN_BYTES: usize = 4;
/// Bytes a sample takes in the file (24-bit PCM).
const OUT_BYTES: usize = 3;
/// Frames read at a time.
const BLOCK_FRAMES: usize = 1024;
/// How often the header's sizes are brought up to date.
const REFRESH: Duration = Duration::from_secs(1);
/// How often the disk's free room is looked at.
const DISK_EVERY: Duration = Duration::from_secs(5);
/// Under this much free room it stops, saying so.
const DISK_FLOOR: u64 = 256 * 1024 * 1024;
/// The WAV format's sizes are 32-bit: a part ends a little short of 4 GB,
/// and the take goes on in the next (`write_rolling`).
const WAV_LIMIT: u64 = u32::MAX as u64 - 1024 * 1024;

/// Where a take stands, for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum RecState {
    /// Looking for the unit and starting `pw-record`.
    Starting,
    /// Writing.
    Recording,
    /// Finished and saved; `why` when it stopped itself. `parts` is how
    /// many files it filled (more than one past the WAV format's 4 GB:
    /// `path` is the first, the rest "… part 2.wav" and on).
    /// `gaps`: holes of digital silence cut into the sound (`Gaps`).
    /// `with_fx`: the Reverb, and the Echo, were each added somewhere in
    /// it (OpenCode's job 61: the note named them from the switches at
    /// the end, not from the take) (on, the mic's
    /// send and the return both up, for at least one block).
    Saved { path: PathBuf, seconds: f32, why: Option<String>, parts: u32, gaps: u32, with_fx: (bool, bool) },
    /// It couldn't start (no file was written).
    Failed(String),
}

/// "Include Reverb & Echo" (the owner, 2026-11-15: Rec is Audra's own,
/// standing in for DURec, so it can record what you hear): the take's own
/// mic, fed at its FX send through a second `FxEngine` in the recorder's
/// thread, its wet output added at the level the listened mix's FX return
/// plays it. Run on the take's own samples, not the unit's FX send (which
/// carries every strip's send) nor the FX loop's output (another stream,
/// not sample-aligned, and it drops blocks when its queue backs up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetParams {
    pub fx: crate::fx_loop::FxSettings,
    /// The mic's FX send, linear (0 = off).
    pub send: f32,
    /// The listened mix's FX return beside the mic's level there, linear
    /// (`app::wet_balance`; 0 = off, held to 4 = +12 dB).
    pub ret: f32,
}

/// The app's side of a take with effects: it keeps `params` current while
/// the take runs (`set`); the recorder picks a change up on its next block.
pub struct WetFeed {
    params: Mutex<WetParams>,
    generation: AtomicU64,
}

impl WetFeed {
    pub fn new(params: WetParams) -> Arc<Self> {
        Arc::new(Self { params: Mutex::new(params), generation: AtomicU64::new(1) })
    }

    /// New levels or settings; nothing happens when they're the same.
    pub fn set(&self, params: WetParams) {
        if let Ok(mut p) = self.params.lock() {
            if *p != params {
                *p = params;
                self.generation.fetch_add(1, Ordering::Release);
            }
        }
    }
}

/// The wet path's state in the recorder's thread. Allocates only at start.
struct Wet {
    feed: Arc<WetFeed>,
    seen: u64,
    params: WetParams,
    engine: audra_fx::FxEngine,
    send: Vec<f32>,
    ret: Vec<f32>,
    /// Something was added: on, with the send and the return up (Strata's
    /// review: the note said "with Reverb & Echo" for a take with either
    /// at zero throughout).
    heard: (bool, bool),
}

impl Wet {
    fn new(feed: Arc<WetFeed>, rate: u32) -> Self {
        let params = feed.params.lock().map(|p| *p).unwrap_or(WetParams { fx: Default::default(), send: 0.0, ret: 0.0 });
        let mut engine = audra_fx::FxEngine::new(rate as f32);
        crate::fx_loop::apply(&mut engine, &params.fx);
        Wet { feed, seen: 0, params, engine, send: Vec::new(), ret: Vec::new(), heard: (false, false) }
    }

    /// The latest levels and settings, when they changed.
    fn refresh(&mut self) {
        let g = self.feed.generation.load(Ordering::Acquire);
        if g != self.seen {
            if let Ok(p) = self.feed.params.try_lock() {
                self.params = *p;
                crate::fx_loop::apply(&mut self.engine, &self.params.fx);
                self.seen = g;
            }
        }
    }

    /// `dry` (24-bit frames of `channels`, 1 or 2) with its effects added,
    /// as 24-bit stereo frames into `out`.
    fn mix(&mut self, dry: &[u8], channels: usize, out: &mut Vec<u8>) {
        self.refresh();
        let p = &self.params;
        let up = p.send > 0.0 && p.ret > 0.0;
        self.heard.0 |= up && p.fx.reverb_on;
        self.heard.1 |= up && p.fx.echo_on;
        mix_wet(dry, channels, &mut self.engine, &self.params, &mut self.send, &mut self.ret, out);
    }
}

/// The file's channels for a take of `channels`: stereo with effects (the
/// reverb is stereo), as picked without.
pub(crate) fn file_channels(channels: usize, wet: bool) -> usize {
    if wet { channels.max(2) } else { channels }
}

fn s24(b: &[u8]) -> f32 {
    (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0
}

/// A sample saturated into 24 bits, never wrapped (a wrapped sample is a
/// loud crack).
fn put_s24(x: f32, out: &mut Vec<u8>) {
    let x = if x.is_finite() { x } else { 0.0 };
    let v = (x * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
    out.extend_from_slice(&v.to_le_bytes()[0..3]);
}

/// Mixes `dry`'s effects into stereo 24-bit frames: the dry, a mono take on
/// both sides, plus the engine's wet at `params.ret`, fed at `params.send`.
/// With 3 or more channels each keeps its own and the wet goes to the
/// first two. `send` and `ret` are scratch, kept between calls.
pub(crate) fn mix_wet(
    dry: &[u8],
    channels: usize,
    engine: &mut audra_fx::FxEngine,
    params: &WetParams,
    send: &mut Vec<f32>,
    ret: &mut Vec<f32>,
    out: &mut Vec<u8>,
) {
    let ch = channels.max(1);
    let frames = dry.len() / (ch * OUT_BYTES);
    send.clear();
    for f in 0..frames {
        let at = f * ch * OUT_BYTES;
        let l = s24(&dry[at..at + 3]);
        let r = if ch > 1 { s24(&dry[at + 3..at + 6]) } else { l };
        send.push(l * params.send);
        send.push(r * params.send);
    }
    ret.clear();
    ret.resize(send.len(), 0.0);
    engine.process(send, ret);
    let ret_gain = params.ret.clamp(0.0, 4.0);
    let out_ch = file_channels(ch, true);
    for f in 0..frames {
        let at = f * ch * OUT_BYTES;
        for c in 0..out_ch {
            let d = s24(&dry[at + c.min(ch - 1) * OUT_BYTES..at + c.min(ch - 1) * OUT_BYTES + 3]);
            let w = if c < 2 { ret[2 * f + c] * ret_gain } else { 0.0 };
            put_s24(d + w, out);
        }
    }
}

struct Shared {
    state: Mutex<RecState>,
    frames: AtomicU64,
    rate: AtomicU64,
}

/// One take: the child and the writing thread.
pub struct Recorder {
    shared: Arc<Shared>,
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub path: PathBuf,
}

impl Recorder {
    /// Starts recording channels `channels` (the unit's capture channels:
    /// AN1 is 0) into a new file at `path`, from the source named `source`
    /// (Audra's processed mic, `dynamics_loop`) when given, else from the
    /// unit.
    /// With `wet`, the take includes Reverb & Echo (`WetFeed`), in stereo.
    pub fn start_from(source: Option<&'static str>, channels: Vec<usize>, path: PathBuf, wet: Option<Arc<WetFeed>>) -> Self {
        let shared = Arc::new(Shared { state: Mutex::new(RecState::Starting), frames: AtomicU64::new(0), rate: AtomicU64::new(0) });
        let child = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (shared, child, stop, path) = (shared.clone(), child.clone(), stop.clone(), path.clone());
            std::thread::Builder::new()
                .name("audra-recorder".into())
                .spawn(move || {
                    crate::priority::raise_this_thread("Rec");
                    run(&shared, &child, &stop, source, &channels, &path, wet)
                })
                .ok()
        };
        Self { shared, child, stop, worker, path }
    }

    pub fn state(&self) -> RecState {
        self.shared.state.lock().map(|s| s.clone()).unwrap_or(RecState::Failed("the recorder stopped".into()))
    }

    /// Seconds written so far.
    pub fn seconds(&self) -> f32 {
        let rate = self.shared.rate.load(Ordering::Relaxed);
        if rate == 0 {
            0.0
        } else {
            self.shared.frames.load(Ordering::Relaxed) as f32 / rate as f32
        }
    }

    /// Stops: the child ends, what it had read is written, the file is
    /// finished. Waits for that.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut c) = self.child.lock() {
            if let Some(child) = c.as_mut() {
                let _ = child.kill();
            }
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        if let Ok(mut c) = self.child.lock() {
            if let Some(mut child) = c.take() {
                let _ = child.wait();
            }
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop();
    }
}

fn set(shared: &Shared, s: RecState) {
    if let Ok(mut st) = shared.state.lock() {
        *st = s;
    }
}

fn run(shared: &Shared, child: &Mutex<Option<Child>>, stop: &AtomicBool, from: Option<&str>, channels: &[usize], path: &Path, wet: Option<Arc<WetFeed>>) {
    let found = match from {
        Some(name) => source_named(name),
        None => pw_meter::discover(TapKind::Source),
    };
    let Some(source) = found else {
        set(shared, RecState::Failed(if from.is_some() {
            "Voice isn't running (it's off, or still starting).".into()
        } else {
            "Audra can't find the unit to record from.".into()
        }));
        return;
    };
    if let Err(why) = check(&source, channels) {
        set(shared, RecState::Failed(why));
        return;
    }
    // Stopped while it was still looking: nothing written.
    if stop.load(Ordering::Relaxed) {
        set(shared, RecState::Failed("Stopped before it started.".into()));
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut wet = wet.map(|feed| Wet::new(feed, source.rate));
    let mut wav = match WavWriter::create(path, file_channels(channels.len(), wet.is_some()) as u16, source.rate) {
        Ok(w) => w,
        Err(e) => {
            let folder = path.parent().unwrap_or(path);
            set(shared, RecState::Failed(why_cant_write(folder, &e)));
            return;
        }
    };
    let Some(mut c) = Command::new("pw-record")
        .args(record_args(&source))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
    else {
        let _ = std::fs::remove_file(path);
        set(shared, RecState::Failed("pw-record couldn't be started (PipeWire's tools are needed).".into()));
        return;
    };
    // Its audio thread raised (rtkit's raise doesn't pass to a child).
    crate::priority::raise_child(c.id(), "Rec's pw-record");
    let Some(mut out) = c.stdout.take() else {
        let _ = c.kill();
        let _ = std::fs::remove_file(path);
        set(shared, RecState::Failed("pw-record gave no audio.".into()));
        return;
    };
    // Slack for the disk writes between reads, so a slow write doesn't
    // block pw-record and drop a buffer of the take (OpenCode's job 16).
    tuxmix_core::pw_meter::grow_pipe(&out);
    // Stored under the lock, then the stop flag looked at again: a stop
    // that came while it was starting (Rec pressed twice, Audra quitting)
    // found no child to kill and is waiting on this thread, so it is ended
    // here (critique #29's detector pass: the wait never ended, and the UI
    // with it).
    if let Ok(mut slot) = child.lock() {
        *slot = Some(c);
        if stop.load(Ordering::Relaxed) {
            if let Some(c) = slot.as_mut() {
                let _ = c.kill();
            }
        }
    }
    shared.rate.store(u64::from(source.rate), Ordering::Relaxed);
    set(shared, RecState::Recording);

    let width = source.channels * IN_BYTES;
    let mut block = vec![0u8; BLOCK_FRAMES * width];
    let mut have = 0usize;
    let mut picked = Vec::with_capacity(BLOCK_FRAMES * channels.len() * OUT_BYTES);
    let mut mixed = Vec::with_capacity(BLOCK_FRAMES * file_channels(channels.len(), true) * OUT_BYTES);
    let (mut refreshed, mut disk_at) = (Instant::now(), Instant::now());
    let mut why: Option<String> = None;
    let mut part = 1u32;
    let mut gaps = Gaps::default();
    // Read to the end of the pipe, stop or not: the child's last frames are
    // still in it after it is told to end.
    loop {
        match out.read(&mut block[have..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => have += n,
        }
        let whole = have / width * width;
        if whole == 0 {
            continue;
        }
        picked.clear();
        pick(&block[..whole], source.channels, channels, &mut picked);
        // Dropouts are looked for in the dry sound: a reverb tail would
        // fill the holes of silence they leave.
        gaps.feed(&picked, channels.len());
        let samples: &[u8] = match wet.as_mut() {
            Some(w) => {
                mixed.clear();
                w.mix(&picked, channels.len(), &mut mixed);
                &mixed
            }
            None => &picked,
        };
        if let Err(w) = write_rolling(&mut wav, path, &mut part, samples, WAV_LIMIT) {
            why = Some(w.into());
            break;
        }
        shared.frames.fetch_add((whole / width) as u64, Ordering::Relaxed);
        block.copy_within(whole..have, 0);
        have -= whole;
        if refreshed.elapsed() >= REFRESH {
            let _ = wav.refresh();
            refreshed = Instant::now();
        }
        if disk_at.elapsed() >= DISK_EVERY {
            if free_bytes(path).is_some_and(|b| b < DISK_FLOOR) {
                why = Some("the disk is nearly full".into());
                break;
            }
            disk_at = Instant::now();
        }
    }
    if why.is_some() {
        if let Ok(mut c) = child.lock() {
            if let Some(child) = c.as_mut() {
                let _ = child.kill();
            }
        }
    } else if !stop.load(Ordering::Relaxed) {
        // The pipe closed with nobody asking: the unit went away (the
        // child is pinned to it and doesn't follow to another device), or
        // the processed mic did (Dynamics switched off or stopped).
        why = Some(if from.is_some() { "Voice stopped" } else { "the unit went away" }.into());
    }
    let _ = wav.refresh();
    // On the disk before it's called saved, so a power cut after the take
    // can't take the header with it (OpenCode's job 17). Only here: a sync
    // every second could stall the writer past the pipe's slack and drop
    // audio from the take itself.
    let _ = wav.file.sync_all();
    let seconds = shared.frames.load(Ordering::Relaxed) as f32 / source.rate.max(1) as f32;
    let with_fx = wet.as_ref().map_or((false, false), |w| w.heard);
    set(shared, RecState::Saved { path: path.to_path_buf(), seconds, why, parts: part, gaps: gaps.count, with_fx });
}

/// Holes of exact digital silence cut into a take's sound: what a source
/// that fell behind leaves (the owner's Voice take on 2026-11-13 had nine,
/// each a whole PipeWire cycle, 21 to 128 ms). Counted where the sound is
/// cut off, not where it dies away: the millisecond before the silence is
/// still near as loud as the sound 8 ms before it (Cough's 8 ms fade, the
/// advisor's catch, ends in a few small samples), louder than -80 dBFS,
/// and the silence is exactly zero for 5 ms to half a second (real sound
/// is never exactly zero that long).
pub(crate) struct Gaps {
    run: usize,
    /// The peaks of the last frames before now (zeros not kept), newest at
    /// `at - 1`.
    ring: Vec<i32>,
    at: usize,
    seen: usize,
    pub(crate) count: u32,
}

impl Default for Gaps {
    fn default() -> Self {
        Gaps { run: 0, ring: vec![0; GAP_RING], at: 0, seen: 0, count: 0 }
    }
}

/// The shortest and longest gap counted, in frames at 48 kHz.
const GAP_MIN: usize = 240;
const GAP_MAX: usize = 24_000;
/// -80 dBFS in 24-bit steps.
const GAP_EDGE: i32 = 839;
/// Frames kept before a silence: 10 ms; the last 1 ms is compared with the
/// 1 ms that ended 8 ms before it.
const GAP_RING: usize = 480;
const GAP_LOOK: usize = 48;
const GAP_BACK: usize = 384;

impl Gaps {
    /// Looks at `samples`, 24-bit frames of `channels`.
    pub(crate) fn feed(&mut self, samples: &[u8], channels: usize) {
        for f in samples.chunks_exact(channels.max(1) * OUT_BYTES) {
            let peak = f.as_chunks::<OUT_BYTES>().0.iter().map(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8).abs()).max().unwrap_or(0);
            if peak == 0 {
                self.run += 1;
                continue;
            }
            if (GAP_MIN..=GAP_MAX).contains(&self.run) && self.cut_off() {
                self.count += 1;
            }
            self.run = 0;
            self.ring[self.at] = peak;
            self.at = (self.at + 1) % GAP_RING;
            self.seen += 1;
        }
    }

    /// Whether the sound before the silence was cut off rather than faded:
    /// its last millisecond loud enough, and at least 40% of the
    /// millisecond 8 ms before.
    fn cut_off(&self) -> bool {
        if self.seen < GAP_LOOK + GAP_BACK {
            return false;
        }
        let peak_over = |back: usize| (0..GAP_LOOK).map(|k| self.ring[(self.at + GAP_RING * 2 - back - 1 - k) % GAP_RING]).max().unwrap_or(0);
        let last = peak_over(0);
        let before = peak_over(GAP_BACK);
        last >= GAP_EDGE && last * 10 >= before * 4
    }
}

/// Writes `samples` (whole frames) to the take. When they wouldn't fit
/// under `limit`, this part is closed first and the take goes on in the
/// next file ("… part 2.wav"), so a take longer than the WAV format holds
/// keeps every sample: it used to stop at 4 GB, about four hours of two
/// channels, which a long stream reaches. No sync between parts: one could
/// stall the writer past the pipe's slack and drop audio at the seam.
fn write_rolling(wav: &mut WavWriter, first: &Path, part: &mut u32, samples: &[u8], limit: u64) -> Result<(), &'static str> {
    if wav.data_bytes > 0 && wav.data_bytes + samples.len() as u64 > limit {
        let _ = wav.refresh();
        let next = part_path(first, *part + 1);
        *wav = WavWriter::create(&next, wav.channels, wav.rate)
            .map_err(|_| "it filled the WAV format's 4 GB and the next part couldn't be made")?;
        *part += 1;
    }
    wav.write(samples).map_err(|_| "the file couldn't be written")
}

/// Part `n` of the take whose first file is `first`: "… AN1 part 2.wav"
/// beside it, numbered past a name already taken.
fn part_path(first: &Path, n: u32) -> PathBuf {
    let stem = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = first.parent().unwrap_or_else(|| Path::new("."));
    let plain = folder.join(format!("{stem} part {n}.wav"));
    if !plain.exists() {
        return plain;
    }
    (2..1000).map(|k| folder.join(format!("{stem} part {n} ({k}).wav"))).find(|p| !p.exists()).unwrap_or(plain)
}

/// The source named `name` (`pactl list sources short`: serial, name,
/// driver, "s32le 2ch 48000Hz", state).
fn source_named(name: &str) -> Option<Monitor> {
    let out = Command::new("pactl").args(["list", "sources", "short"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        if f.get(1) != Some(&name) {
            return None;
        }
        let index = f.first()?.trim().parse().ok()?;
        let spec = f.get(3)?;
        let channels = spec.split_whitespace().find_map(|t| t.strip_suffix("ch")?.parse().ok())?;
        let rate = spec.split_whitespace().find_map(|t| t.strip_suffix("Hz")?.parse().ok())?;
        Some(Monitor { name: name.to_string(), index, rate, channels, kind: TapKind::Source })
    })
}

/// Whether `source` carries every channel asked for.
fn check(source: &Monitor, channels: &[usize]) -> Result<(), String> {
    if channels.is_empty() {
        return Err("Nothing chosen to record.".into());
    }
    let need = channels.iter().max().copied().unwrap_or(0) + 1;
    if need > source.channels {
        return Err(format!(
            "Only {} channels are open to the computer, so this input can't be recorded: switch the unit's profile to Pro Audio in the sound settings.",
            source.channels
        ));
    }
    Ok(())
}

/// The child's arguments: every channel of the unit's source, pinned to it.
/// Pro Audio's nodes have positions `AUX0`…, a stereo profile's `FL,FR`.
pub(crate) fn record_args(source: &Monitor) -> Vec<String> {
    let mut args = vec![
        // The index, not the name: a name falls back to the default device
        // while the unit is suspended (`pw_meter`).
        "--target".to_string(),
        source.index.to_string(),
        "--format".into(),
        "s32".into(),
        "--rate".into(),
        source.rate.to_string(),
        "--channels".into(),
        source.channels.to_string(),
    ];
    if source.channels > 2 {
        args.extend(["--channel-map".into(), (0..source.channels).map(|i| format!("AUX{i}")).collect::<Vec<_>>().join(",")]);
    }
    args.extend([
        "--media-category".into(),
        "Capture".into(),
        "--media-role".into(),
        "Production".into(),
        "--properties".into(),
        "node.name=audra-recorder node.dont-reconnect=true node.dont-fallback=true node.dont-move=true stream.dont-remix=true".into(),
        "--raw".into(),
        "-".into(),
    ]);
    args
}

/// The chosen channels of whole `s32` frames of `width` channels, as
/// 24-bit little-endian samples, frame by frame.
pub(crate) fn pick(bytes: &[u8], width: usize, channels: &[usize], out: &mut Vec<u8>) {
    let frame = width * IN_BYTES;
    for f in bytes.chunks_exact(frame) {
        for &c in channels {
            let at = c * IN_BYTES;
            // The top three bytes of the 32-bit sample.
            out.extend_from_slice(&f[at + 1..at + 4]);
        }
    }
}

/// A 24-bit PCM WAV file being written.
pub(crate) struct WavWriter {
    file: File,
    channels: u16,
    rate: u32,
    data_bytes: u64,
}

impl WavWriter {
    /// A new file: an error if one is already there (never overwritten).
    pub(crate) fn create(path: &Path, channels: u16, rate: u32) -> std::io::Result<Self> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&header(channels, rate, 0))?;
        Ok(Self { file, channels, rate, data_bytes: 0 })
    }

    pub(crate) fn write(&mut self, samples: &[u8]) -> std::io::Result<()> {
        self.file.write_all(samples)?;
        self.data_bytes += samples.len() as u64;
        Ok(())
    }

    /// The header's sizes brought up to what's written.
    pub(crate) fn refresh(&mut self) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(self.channels, self.rate, self.data_bytes))?;
        self.file.seek(SeekFrom::End(0))?;
        self.file.flush()
    }
}

/// A 44-byte PCM WAV header for `data_bytes` of 24-bit samples.
pub(crate) fn header(channels: u16, rate: u32, data_bytes: u64) -> [u8; 44] {
    let data = data_bytes.min(u32::MAX as u64 - 36) as u32;
    let align = channels * OUT_BYTES as u16;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * u32::from(align)).to_le_bytes());
    h[32..34].copy_from_slice(&align.to_le_bytes());
    h[34..36].copy_from_slice(&24u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// Free bytes on the disk holding `path`'s folder.
fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let dir = path.parent()?;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` only writes into our own zeroed struct.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(c.as_ptr(), &mut s) == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
    }
}

/// Why the file couldn't be made, in plain words (job 18): the owner
/// isn't a programmer, and "Permission denied (os error 13)" says nothing
/// about what to do.
fn why_cant_write(folder: &Path, e: &std::io::Error) -> String {
    let folder = folder_words(folder);
    use std::io::ErrorKind::{AlreadyExists, NotADirectory, PermissionDenied, ReadOnlyFilesystem, StorageFull};
    match e.kind() {
        PermissionDenied | ReadOnlyFilesystem => format!(
            "the folder {folder} can't be written to. Pick another in Settings, or check its permissions."
        ),
        // A file where the folder should be: opening "folder/take.wav"
        // inside it fails with "not a directory".
        NotADirectory => format!("{folder} is a file, not a folder. Pick another folder in Settings."),
        // Only the take's own name: `free_path` picked a free one, so
        // another file took it in the moment between. The next try
        // gets "(2)".
        AlreadyExists => "a file took the take's name just as it started. Press Rec again.".to_string(),
        StorageFull => "the disk is full. Free some space, or pick another folder in Settings.".to_string(),
        _ => format!("Can't write {folder}: {e}."),
    }
}

/// A folder as words: the home folder as "~". Settings and Rec's tip use
/// it too.
pub(crate) fn folder_words(dir: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) => match dir.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => dir.display().to_string(),
        },
        None => dir.display().to_string(),
    }
}

/// The folder takes go to by default: `~/Music/Audra`.
pub fn default_folder() -> PathBuf {
    default_folder_from(std::env::var_os("HOME"), std::env::var_os("XDG_MUSIC_DIR"))
}

/// The folder itself, as a pure function of the two variables it comes
/// from, so a test never has to change the environment (other tests read
/// `HOME` at the same time).
///
/// With no home folder there is no "~", so the music folder is used when it
/// is set and absolute, and the temporary folder otherwise. Never a
/// relative path: that would put the takes wherever Audra was started from
/// (job 19).
fn default_folder_from(home: Option<std::ffi::OsString>, music: Option<std::ffi::OsString>) -> PathBuf {
    let home = home.map(PathBuf::from).filter(|h| h.is_absolute());
    if let Some(home) = home {
        return home.join("Music").join("Audra");
    }
    if let Some(music) = music.map(PathBuf::from).filter(|m| m.is_absolute()) {
        return music.join("Audra");
    }
    std::env::temp_dir().join("Audra")
}

/// The name of the small file a folder check writes and removes. No take
/// has this name (a take is "Audra … .wav").
const PROBE_NAME: &str = ".audra-write-test";

/// Whether takes can be written to `folder`, said in the words a take
/// would use (job 19): the folder is created if it isn't there, and a
/// probe file is really written and removed, so a read-only mount is
/// caught — a permissions check alone would pass it.
pub fn check_folder(folder: &Path) -> Result<(), String> {
    // A file where the folder should be: `create_dir_all` would say
    // "already exists", which is a take's own name, not this.
    if folder.is_file() {
        return Err(why_cant_write(folder, &std::io::Error::from(std::io::ErrorKind::NotADirectory)));
    }
    if let Err(e) = std::fs::create_dir_all(folder) {
        // A file in the way of the folder, or of a parent of it.
        if folder.is_file() {
            return Err(why_cant_write(folder, &std::io::Error::from(std::io::ErrorKind::NotADirectory)));
        }
        return Err(why_cant_write(folder, &e));
    }
    // A real write: the folder's own, in a file of its own.
    let probe = folder.join(PROBE_NAME);
    // A probe left behind by a crash is no reason to say the folder is
    // bad: it goes first.
    let _ = std::fs::remove_file(&probe);
    let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).and_then(|mut f| {
        f.write_all(b"audra")?;
        f.flush()?;
        Ok(())
    });
    let _ = std::fs::remove_file(&probe);
    match written {
        Ok(()) => Ok(()),
        Err(e) => Err(why_cant_write(folder, &e)),
    }
}

/// A take's file name: "Audra 2026-11-10 17-40-12 AN1.wav", by the local
/// time and what it records, with nothing in it a file name can't hold.
pub fn file_name(stamp: &str, what: &str) -> String {
    let what: String = what.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '-' } else { c }).collect();
    format!("Audra {stamp} {}.wav", what.trim())
}

/// A take's path in `folder` that no file holds yet: the plain name, then
/// "… (2).wav", "… (3).wav" and on. Names carry the time to the second,
/// so two takes in one second had one name and the second was refused
/// (OpenCode's job 17). `create_new` still guards the write itself.
pub fn free_path(folder: &Path, stamp: &str, what: &str) -> PathBuf {
    let base = file_name(stamp, what);
    let first = folder.join(&base);
    if !first.exists() {
        return first;
    }
    let stem = base.strip_suffix(".wav").unwrap_or(&base);
    (2..1000).map(|n| folder.join(format!("{stem} ({n}).wav"))).find(|p| !p.exists()).unwrap_or(first)
}

/// "2026-11-10 17-40-12", local time.
pub fn local_stamp() -> String {
    // SAFETY: `time` with a null pointer only returns the time, and
    // `localtime_r` writes the broken-down time into our own `tm`.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return format!("{now}");
        }
        format!(
            "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// "1:05", "12:40", "1:02:03".
pub fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(channels: usize) -> Monitor {
        Monitor { name: "alsa_input.usb-RME_Babyface".into(), index: 77, rate: 48_000, channels, kind: TapKind::Source }
    }

    /// 24-bit stereo frames of `v` (a fraction of full scale).
    fn frames(v: &[f32]) -> Vec<u8> {
        v.iter()
            .flat_map(|&x| {
                let i = (x * 8_388_607.0) as i32;
                let b = i.to_le_bytes();
                [b[0], b[1], b[2], b[0], b[1], b[2]]
            })
            .collect()
    }

    /// 24-bit mono frames of `v`.
    fn mono(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|&x| ((x * 8_388_607.0) as i32).to_le_bytes()[0..3].to_vec()).collect()
    }

    fn samples(b: &[u8]) -> Vec<i32> {
        b.chunks_exact(3).map(|c| i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8).collect()
    }

    fn wet(reverb: bool, send: f32, ret: f32) -> WetParams {
        let fx = crate::fx_loop::FxSettings { reverb_on: reverb, echo_on: false, ..Default::default() };
        WetParams { fx, send, ret }
    }

    fn run_mix(dry: &[u8], ch: usize, p: &WetParams) -> Vec<u8> {
        let mut engine = audra_fx::FxEngine::new(48_000.0);
        crate::fx_loop::apply(&mut engine, &p.fx);
        let (mut send, mut ret, mut out) = (Vec::new(), Vec::new(), Vec::new());
        mix_wet(dry, ch, &mut engine, p, &mut send, &mut ret, &mut out);
        out
    }

    #[test]
    fn a_take_with_no_effect_heard_is_the_plain_take_in_stereo() {
        // Effects off, the send off or the return off: the dry, sample for
        // sample, on both sides.
        let speech: Vec<f32> = (0..2048).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
        let dry = mono(&speech);
        for p in [wet(false, 1.0, 1.0), wet(true, 0.0, 1.0), wet(true, 1.0, 0.0)] {
            let out = samples(&run_mix(&dry, 1, &p));
            let d = samples(&dry);
            assert_eq!(out.len(), 2 * d.len());
            assert!(out.chunks_exact(2).zip(&d).all(|(f, x)| f[0] == *x && f[1] == *x), "{p:?}");
        }
        // A stereo take keeps each side its own.
        let st = frames(&speech);
        assert_eq!(run_mix(&st, 2, &wet(false, 1.0, 1.0)), st);
        assert_eq!(file_channels(1, true), 2);
        assert_eq!(file_channels(1, false), 1);
        assert_eq!(file_channels(2, true), 2);
    }

    #[test]
    fn reverb_adds_a_tail_after_the_voice_stops() {
        let mut v = vec![0.5f32; 480];
        v.extend(std::iter::repeat(0.0).take(48_000));
        let out = samples(&run_mix(&mono(&v), 1, &wet(true, 1.0, 1.0)));
        // Half a second after the burst ended, the room still rings.
        let late = &out[2 * 24_480..2 * 26_880];
        assert!(late.iter().any(|&x| x.abs() > 100), "a tail");
    }

    #[test]
    fn a_full_scale_take_with_a_hot_return_saturates_never_wraps() {
        // A wrapped sample is a loud crack: full-scale dry plus the return
        // at its +12 dB top is the sum held at full scale, sample for
        // sample, never wrapped round to the other side.
        let v: Vec<f32> = (0..9600).map(|i| if (i / 50) % 2 == 0 { 0.999 } else { -0.999 }).collect();
        let dry = mono(&v);
        let p = wet(true, 1.0, 4.0);
        let out = samples(&run_mix(&dry, 1, &p));
        // The same engine on its own, for the room.
        let mut engine = audra_fx::FxEngine::new(48_000.0);
        crate::fx_loop::apply(&mut engine, &p.fx);
        let d: Vec<f32> = samples(&dry).iter().map(|&x| x as f32 / 8_388_608.0).collect();
        let send: Vec<f32> = d.iter().flat_map(|&x| [x, x]).collect();
        let mut ret = vec![0.0; send.len()];
        engine.process(&send, &mut ret);
        let mut held = 0;
        for (i, f) in out.chunks_exact(2).enumerate() {
            for c in 0..2 {
                let want = ((d[i] + ret[2 * i + c] * 4.0) * 8_388_608.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                assert_eq!(f[c], want, "frame {i} side {c}");
                held += usize::from(want == 8_388_607 || want == -8_388_608);
            }
        }
        assert!(held > 0, "the test reaches full scale");
    }

    /// Writes target/rec-fx/{dry,wet}.wav: two seconds of a voice-like
    /// signal (bursts of a gliding tone at about -12 dBFS peaks), plain and
    /// with the default Reverb at the starting send and return, for
    /// `tools/checks/totalMix-rec-fx.py` to measure.
    #[test]
    #[ignore]
    fn write_a_take_with_effects_for_measuring() {
        let rate = 48_000usize;
        let v: Vec<f32> = (0..rate * 3)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let on = (t % 0.5) < 0.3 && t < 2.0;
                if on { 0.25 * (2.0 * std::f32::consts::PI * (180.0 + 60.0 * (t * 3.0).sin()) * t).sin() } else { 0.0 }
            })
            .collect();
        let dry = mono(&v);
        let p = wet(true, audra_fx::db_to_gain(crate::fx::START_SEND_DB), 1.0);
        let out = run_mix(&dry, 1, &p);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/rec-fx");
        let _ = std::fs::create_dir_all(&dir);
        for (name, data, ch) in [("dry.wav", &dry, 1u16), ("wet.wav", &out, 2u16)] {
            let path = dir.join(name);
            let _ = std::fs::remove_file(&path);
            let mut w = WavWriter::create(&path, ch, rate as u32).unwrap();
            w.write(data).unwrap();
            w.refresh().unwrap();
        }
    }

    #[test]
    fn gaps_are_counted_where_sound_stops_dead_not_where_it_fades() {
        // The owner's choppy take, 2026-11-13: whole cycles of zeros cut
        // into speech.
        let speech: Vec<f32> = (0..4800).map(|i| 0.1 * (i as f32 * 0.06).sin() + 0.02).collect();
        let mut take = speech.clone();
        take.extend(std::iter::repeat(0.0).take(1024));
        take.extend(&speech);
        take.extend(std::iter::repeat(0.0).take(3072));
        take.extend(&speech);
        let mut g = Gaps::default();
        // In two reads, a gap across them.
        let bytes = frames(&take);
        let (a, b) = bytes.split_at(6 * 5000);
        g.feed(a, 2);
        g.feed(b, 2);
        assert_eq!(g.count, 2);
        // Cough's 10 ms fade to silence and back, a long quiet, a 1 ms dip:
        // none counted.
        let mut calm = speech.clone();
        calm.extend((0..480).map(|i| 0.02 * (1.0 - i as f32 / 480.0)));
        calm.extend(std::iter::repeat(0.0).take(9600));
        calm.extend(&speech);
        calm.extend(std::iter::repeat(0.0).take(48_000));
        calm.extend(&speech);
        calm.extend(std::iter::repeat(0.0).take(48));
        calm.extend(&speech);
        let mut g = Gaps::default();
        g.feed(&frames(&calm), 2);
        assert_eq!(g.count, 0);
        // Cough's own fade (8 ms, linear, to exact zero) from speech at
        // about -16 dBFS, held 200 ms: not a dropout (the advisor's catch).
        let loud: Vec<f32> = (0..4800).map(|i| 0.16 * (i as f32 * 0.06).sin()).collect();
        let mut cough = loud.clone();
        cough.extend((0..384).map(|i| 0.16 * ((4800 + i) as f32 * 0.06).sin() * (1.0 - (i + 1) as f32 / 384.0)));
        cough.extend(std::iter::repeat(0.0).take(9600));
        cough.extend(&loud);
        let mut g = Gaps::default();
        g.feed(&frames(&cough), 2);
        assert_eq!(g.count, 0, "a Cough press isn't a dropout");
        // Silence at the very start of a take isn't a gap.
        let mut g = Gaps::default();
        let mut start = vec![0.0; 4800];
        start.extend(&speech);
        g.feed(&frames(&start), 2);
        assert_eq!(g.count, 0);
    }

    #[test]
    fn the_child_is_pinned_to_the_unit() {
        let args = record_args(&source(14));
        let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        assert_eq!(after("--target").as_deref(), Some("77"));
        assert_eq!(after("--channels").as_deref(), Some("14"));
        assert_eq!(after("--format").as_deref(), Some("s32"));
        assert!(after("--channel-map").unwrap().starts_with("AUX0,AUX1,"));
        let props = after("--properties").unwrap();
        for p in ["node.dont-reconnect=true", "node.dont-fallback=true", "node.dont-move=true", "stream.dont-remix=true"] {
            assert!(props.contains(p), "{p}");
        }
        assert_eq!(args.last().map(String::as_str), Some("-"));
        // A stereo profile: no AUX map (its positions are FL, FR).
        assert!(!record_args(&source(2)).iter().any(|a| a == "--channel-map"));
    }

    #[test]
    fn it_says_when_the_profile_lacks_the_channel() {
        assert!(check(&source(2), &[0, 1]).is_ok());
        let why = check(&source(2), &[2, 3]).unwrap_err();
        assert!(why.contains("Pro Audio"), "{why}");
        assert!(check(&source(14), &[]).is_err());
    }

    #[test]
    fn it_keeps_the_chosen_channels_as_24_bit() {
        // Two frames of three channels; keep channels 2 and 0.
        let s = |v: i32| v.to_le_bytes();
        let mut bytes = Vec::new();
        for frame in [[0x0102_0300, 0x7f00_0000, 0x0a0b_0c00], [-256, 0, 0x7fff_ff00]] {
            for v in frame {
                bytes.extend_from_slice(&s(v));
            }
        }
        let mut out = Vec::new();
        pick(&bytes, 3, &[2, 0], &mut out);
        assert_eq!(out, vec![0x0c, 0x0b, 0x0a, 0x03, 0x02, 0x01, 0xff, 0xff, 0x7f, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn the_header_says_24_bit_pcm_and_its_sizes() {
        let h = header(2, 48_000, 600);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 636);
        assert_eq!(u16::from_le_bytes(h[20..22].try_into().unwrap()), 1, "PCM");
        assert_eq!(u16::from_le_bytes(h[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 48_000 * 6);
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 6);
        assert_eq!(u16::from_le_bytes(h[34..36].try_into().unwrap()), 24);
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 600);
    }

    #[test]
    fn a_file_is_never_overwritten_and_ends_with_its_sizes() {
        let dir = std::env::temp_dir().join(format!("audra-rec-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("take.wav");
        let _ = std::fs::remove_file(&path);
        let mut w = WavWriter::create(&path, 1, 48_000).unwrap();
        w.write(&[1, 2, 3, 4, 5, 6]).unwrap();
        w.refresh().unwrap();
        assert!(WavWriter::create(&path, 1, 48_000).is_err(), "never overwritten");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On the real unit only (`cargo test -- --ignored live_recorder`): a
    /// second and a half of ADAT 8 (capture channel 11, an input with
    /// nothing in it; never the mic, nor the FX send, which can carry it)
    /// into a temporary folder; the file's header and length are checked,
    /// then it is deleted. Skipped on a 2-channel profile, whose two are
    /// AN1 and AN2.
    #[test]
    #[ignore]
    fn live_recorder_writes_a_file() {
        let Some(src) = pw_meter::discover(TapKind::Source) else {
            eprintln!("no unit: skipped");
            return;
        };
        if src.channels < 12 {
            eprintln!("{} channels open (not Pro Audio): skipped", src.channels);
            return;
        }
        let dir = std::env::temp_dir().join(format!("audra-live-rec-{}", std::process::id()));
        let path = dir.join("live.wav");
        let mut rec = Recorder::start_from(None, vec![11], path.clone(), None);
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(rec.state(), RecState::Recording, "it started");
        rec.stop();
        let RecState::Saved { seconds, why, .. } = rec.state() else { panic!("{:?}", rec.state()) };
        assert!(why.is_none(), "{why:?}");
        assert!(seconds > 0.8, "{seconds}");
        let bytes = std::fs::read(&path).unwrap();
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        assert_eq!(bytes.len(), 44 + data, "the header says what's there");
        assert_eq!(data % OUT_BYTES, 0);
        eprintln!("recorded {seconds:.2} s, {data} bytes at {} Hz; deleting", src.rate);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_take_in_the_same_second_gets_its_own_name() {
        // OpenCode's job 17: the second take was refused ("File exists").
        let dir = std::env::temp_dir().join(format!("audra-rec-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stamp = "2026-11-11 21-40-12";
        let first = free_path(&dir, stamp, "AN1");
        assert_eq!(first, dir.join("Audra 2026-11-11 21-40-12 AN1.wav"));
        std::fs::write(&first, b"take one").unwrap();
        let second = free_path(&dir, stamp, "AN1");
        assert_eq!(second, dir.join("Audra 2026-11-11 21-40-12 AN1 (2).wav"));
        std::fs::write(&second, b"take two").unwrap();
        assert_eq!(free_path(&dir, stamp, "AN1"), dir.join("Audra 2026-11-11 21-40-12 AN1 (3).wav"));
        assert_eq!(std::fs::read(&first).unwrap(), b"take one", "the first take is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_take_that_cant_start_says_why_in_plain_words() {
        // Job 18: the owner isn't a programmer; "Permission denied (os
        // error 13)" says nothing about what to do.
        use std::io::{Error, ErrorKind};
        // A folder outside the home, so the wording is about the folder
        // itself (the home is said as "~", in the next test).
        let folder = Path::new("/var/tmp/Audra");
        assert_eq!(
            why_cant_write(folder, &Error::from(ErrorKind::PermissionDenied)),
            "the folder /var/tmp/Audra can't be written to. Pick another in Settings, or check its permissions."
        );
        assert_eq!(
            why_cant_write(folder, &Error::from(ErrorKind::ReadOnlyFilesystem)),
            "the folder /var/tmp/Audra can't be written to. Pick another in Settings, or check its permissions."
        );
        assert_eq!(
            why_cant_write(folder, &Error::from(ErrorKind::NotADirectory)),
            "/var/tmp/Audra is a file, not a folder. Pick another folder in Settings."
        );
        assert_eq!(
            why_cant_write(folder, &Error::from(ErrorKind::AlreadyExists)),
            "a file took the take's name just as it started. Press Rec again."
        );
        assert_eq!(
            why_cant_write(folder, &Error::from(ErrorKind::StorageFull)),
            "the disk is full. Free some space, or pick another folder in Settings."
        );
        // Anything else keeps today's wording, with the error at the end.
        let other = Error::other("something else went wrong");
        assert_eq!(
            why_cant_write(folder, &other),
            "Can't write /var/tmp/Audra: something else went wrong."
        );
    }

    #[test]
    fn the_home_folder_is_said_as_a_tilde() {
        // Job 18: the notice stays short.
        let home = std::env::var_os("HOME").map(PathBuf::from).expect("a home folder");
        assert_eq!(folder_words(&home.join("Music").join("Audra")), "~/Music/Audra");
        assert_eq!(folder_words(Path::new("/var/tmp")), "/var/tmp", "another folder as it is");
    }

    #[test]
    fn names_and_times_read_plainly() {
        assert_eq!(file_name("2026-11-10 17-40-12", "AN1/2"), "Audra 2026-11-10 17-40-12 AN1-2.wav");
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(local_stamp().len(), 19);
    }

    #[test]
    fn a_take_past_the_limit_goes_on_in_a_second_part_with_every_sample() {
        // It stopped at the WAV format's 4 GB (about four hours of two
        // channels); it now goes on in "… part 2.wav". Here with a limit of
        // 80 bytes: 2 channels of 24-bit, 6 bytes a frame.
        let dir = std::env::temp_dir().join(format!("audra-rec-parts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("Audra 2026-11-12 10-00-00 AN1.wav");
        let mut wav = WavWriter::create(&first, 2, 48_000).unwrap();
        let mut part = 1;
        let block: Vec<u8> = (0..36u8).collect(); // 6 frames
        for _ in 0..3 {
            write_rolling(&mut wav, &first, &mut part, &block, 80).unwrap();
        }
        wav.refresh().unwrap();
        assert_eq!(part, 2, "one more part");
        let second = dir.join("Audra 2026-11-12 10-00-00 AN1 part 2.wav");
        let a = std::fs::read(&first).unwrap();
        let b = std::fs::read(&second).unwrap();
        assert_eq!(a.len() - 44, 72, "the first part holds what fit (two blocks; a third would pass 80)");
        assert_eq!(b.len() - 44, 36, "the second holds the rest: nothing lost");
        assert_eq!(u32::from_le_bytes(a[40..44].try_into().unwrap()), 72, "the first part's header says its size");
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()), 36, "so does the second's");
        assert_eq!(&a[44..80], &block[..], "the samples, in order");
        assert_eq!(&b[44..], &block[..]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_part_takes_a_free_name() {
        let dir = std::env::temp_dir().join(format!("audra-rec-partname-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("Audra x AN1.wav");
        assert_eq!(part_path(&first, 2), dir.join("Audra x AN1 part 2.wav"));
        std::fs::write(dir.join("Audra x AN1 part 2.wav"), b"x").unwrap();
        assert_eq!(part_path(&first, 2), dir.join("Audra x AN1 part 2 (2).wav"), "never over a file that's there");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_writable_folder_passes_and_leaves_no_probe_behind() {
        // Job 19: a folder picked in Settings is checked when it is
        // picked, so a take isn't refused at the next Rec.
        let dir = std::env::temp_dir().join(format!("audra-rec-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(check_folder(&dir).is_ok());
        assert!(!dir.join(PROBE_NAME).exists(), "the probe is gone again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_is_created_and_passes() {
        let dir = std::env::temp_dir().join(format!("audra-rec-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!dir.exists());
        assert!(check_folder(&dir).is_ok());
        assert!(dir.is_dir(), "it is there now");
        assert!(!dir.join(PROBE_NAME).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_where_the_folder_should_be_says_so() {
        let dir = std::env::temp_dir().join(format!("audra-rec-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("not a folder");
        std::fs::write(&file, b"x").unwrap();
        let why = check_folder(&file).unwrap_err();
        assert!(why.contains("is a file, not a folder"), "{why}");
        assert!(why.contains("Pick another folder in Settings"), "{why}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_folder_is_always_an_absolute_path() {
        use std::ffi::OsString;
        let os = |s: &str| Some(OsString::from(s));
        // A home folder: the usual place.
        assert_eq!(default_folder_from(os("/home/someone"), None), PathBuf::from("/home/someone/Music/Audra"));
        // No home folder, no music folder: the temporary folder.
        assert_eq!(default_folder_from(None, None), std::env::temp_dir().join("Audra"));
        // An empty home folder is no home folder.
        assert_eq!(default_folder_from(os(""), None), std::env::temp_dir().join("Audra"));
        // A relative music folder is no music folder.
        assert_eq!(default_folder_from(None, os("Music")), std::env::temp_dir().join("Audra"));
        // An absolute one is used.
        assert_eq!(default_folder_from(None, os("/srv/music")), PathBuf::from("/srv/music/Audra"));
        // Never a relative path, whatever comes in.
        for (home, music) in [(None, None), (os(""), os("Music")), (os("relative/home"), os("Music")), (os("/home/someone"), os("relative"))] {
            let folder = default_folder_from(home, music);
            assert!(folder.is_absolute(), "{folder:?}");
        }
    }
}
