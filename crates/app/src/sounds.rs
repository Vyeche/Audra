//! UI sounds: short clicks and ticks for presses, detents and moving
//! around, synthesized here (no sound files) and played through PipeWire's
//! own `pw-play`, as the meters already use `pw-record`, so no audio crate
//! is added.
//!
//! **On by default** (the owner's choice, 2026-11-03, first off, then on once
//! they could hear them; the top bar's speaker silences them). In a mixer, UI
//! sounds go out of the default output, usually the interface being
//! monitored and recorded on. They play as a "Notification" stream named
//! "Audra UI sounds", so the desktop's sound settings can send them to
//! another device (laptop speakers, say) instead.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};

const RATE: f32 = 48_000.0;

/// The volume range of the Settings knob, in dB, and its default.
pub const VOLUME_RANGE: (f32, f32) = (-40.0, 0.0);
pub const DEFAULT_VOLUME_DB: f32 = -20.0;

/// What a sound says. They are made like a phone's UI sounds rather than
/// beeps (the owner, 2026-11-03: "Make the sounds less annoying kind of
/// like apple sounds iphone"): a few milliseconds of noise through a
/// resonance, with a little low body under it and the top rolled off,
/// so a click reads as a soft "tock" and not a tone, and repeating it
/// never grates. The one tonal sound, `Move`, is a single round pop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Something switched on: a small, bright tick.
    On,
    /// Something switched off: the same tick, lower and softer.
    Off,
    /// A fader crossing 0 dB or a pan crossing the centre: the faintest
    /// tick, like a detent under the finger.
    Detent,
    /// Moving to another place (a view, a row, a mix, a theme): one soft,
    /// round pop that glides down a little.
    Move,
    /// Any other press (a tab, a panel, a switch, a picker): the soft
    /// "tock" of a phone's keyboard, quieter than On and Off.
    Tap,
    /// Something now waits for a second click (48V?, Store, a Reset Mix
    /// or workspace Load arming): two little ticks, the second higher,
    /// like a safety catch let go.
    Arm,
    /// The wait ran out with no second click: the same two ticks the
    /// other way, the second lower, softer: the catch back on.
    Disarm,
    /// Something was kept (a snapshot or layout stored, a workspace, a
    /// preset, a theme or a file saved): two round notes rising a fifth,
    /// warm, like a message sent. The ear tells "kept" from a click.
    Done,
    /// Something was refused (the lock held a change back, the interface
    /// refused a write): two low round notes falling, muffled, so it
    /// says "look" without alarm.
    Nope,
    /// A step of a guide done: one soft, round note, a step higher on a
    /// pentatonic scale each time in a row (`step`), so a run of wins climbs
    /// without ever getting louder (the owner: "The sounds should be
    /// subtle").
    Chime(u8),
    /// A milestone: three soft notes rising from that step, close together,
    /// the warmest Audra makes; still quieter than a click.
    Rise(u8),
    /// A knob turned against the end of its travel (the gain at its top or
    /// bottom): a soft, low, muffled bump, like a pot's end stop.
    Stop,
    /// A snapshot recalled: two quick round notes rising a third, lighter
    /// than "kept": the mix you had comes back.
    Recall,
    /// An input clipped: one soft, low, muffled tick, quieter than the end
    /// stop, at most once every few seconds (`app::CLIP_SOUND_EVERY`), so
    /// it tells without nagging (the owner, 2026-11-07: subtle sounds).
    Clip,
    /// A strip folded (`true`) or unfolded: a soft pop gliding down as it
    /// closes and up as it opens, under the view's pop (it tapped both
    /// ways; 2026-11-15).
    Fold(bool),
    /// The helper cat poked: a short, soft "mew" rising then falling, a
    /// voice of a few harmonics through a vowel that opens and closes.
    Meow,
    /// The helper cat picked up: a low, soft purr, pulsing about 25 times
    /// a second.
    Purr,
    /// The helper cat popping up somewhere: a tiny chirp gliding up.
    Chirp,
}

/// The sounds the Look page's "Hear them" row plays, with their names.
pub const PREVIEW: [(&str, Sound); 7] = [
    ("Click", Sound::Tap),
    ("On", Sound::On),
    // Plain names (critique #52: "Kept", "Refused", "Recall").
    ("Saved", Sound::Done),
    ("Snapshot", Sound::Recall),
    ("Not allowed", Sound::Nope),
    ("Clip", Sound::Clip),
    ("Milestone", Sound::Rise(0)),
];

/// A sound centred on zero: a gentle low cut (one pole at 35 Hz) takes
/// out the one-way push a short tock's low body had (measured 2026-11-14:
/// tap, on, off, the end stop and the clip tick sat off zero, a soft thump
/// in a woofer), its last 2 ms faded to silence, its peak kept.
fn centred(x: Vec<f32>) -> Vec<f32> {
    let peak_in = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak_in <= 0.0 {
        return x;
    }
    let rc = 1.0 / (std::f32::consts::TAU * 35.0);
    let a = rc / (rc + 1.0 / RATE);
    let mut out = Vec::with_capacity(x.len());
    let (mut prev_x, mut prev_y) = (0.0f32, 0.0f32);
    for &v in &x {
        let y = a * (prev_y + v - prev_x);
        out.push(y);
        prev_x = v;
        prev_y = y;
    }
    let fade = ((0.002 * RATE) as usize).min(out.len());
    let n = out.len();
    for (k, v) in out[n - fade..].iter_mut().enumerate() {
        *v *= 1.0 - (k + 1) as f32 / fade as f32;
    }
    let peak_out = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak_out > 0.0 {
        let g = peak_in / peak_out;
        out.iter_mut().for_each(|v| *v *= g);
    }
    out
}

/// C major pentatonic, from C5: any step sounds in key with any other.
fn pentatonic(step: u8) -> f32 {
    const SCALE: [f32; 5] = [523.25, 587.33, 659.25, 783.99, 880.0];
    let s = step as usize % 10;
    SCALE[s % 5] * if s >= 5 { 2.0 } else { 1.0 }
}

/// A soft bell: the note, an octave whisper over it, a long gentle tail.
fn bell(freq: f32) -> Vec<f32> {
    let mut body = note(freq, freq, 0.22, 0.07);
    let shimmer = note(2.0 * freq, 2.0 * freq, 0.12, 0.03);
    for (v, w) in body.iter_mut().zip(shimmer) {
        *v += 0.18 * w;
    }
    body
}

impl Sound {
    /// The samples (mono, 48 kHz, peak under 1.0), each one centred on
    /// zero (`centred`).
    pub fn samples(self) -> Vec<f32> {
        centred(self.raw_samples())
    }

    fn raw_samples(self) -> Vec<f32> {
        match self {
            Sound::On => tock(Tock { ring: 2400.0, q: 3.0, decay: 0.0018, body: 260.0, body_level: 0.35, peak: 0.62 }),
            Sound::Off => tock(Tock { ring: 1350.0, q: 3.0, decay: 0.0022, body: 180.0, body_level: 0.4, peak: 0.52 }),
            Sound::Detent => tock(Tock { ring: 3400.0, q: 4.0, decay: 0.0007, body: 0.0, body_level: 0.0, peak: 0.3 }),
            Sound::Move => pop(),
            Sound::Fold(closing) => {
                if closing {
                    glide_pop(560.0, 420.0, 0.3)
                } else {
                    glide_pop(420.0, 560.0, 0.3)
                }
            }
            Sound::Tap => tock(Tock { ring: 1600.0, q: 2.5, decay: 0.0022, body: 210.0, body_level: 0.45, peak: 0.45 }),
            Sound::Arm => {
                let first = tock(Tock { ring: 1700.0, q: 3.0, decay: 0.0016, body: 0.0, body_level: 0.0, peak: 0.42 });
                let second = tock(Tock { ring: 2550.0, q: 3.0, decay: 0.0018, body: 0.0, body_level: 0.0, peak: 0.48 });
                let offset = (0.045 * RATE) as usize;
                let mut out = vec![0.0; offset + second.len()];
                for (i, v) in first.iter().enumerate() {
                    out[i] += v;
                }
                for (i, v) in second.iter().enumerate() {
                    out[offset + i] += v;
                }
                out
            }
            Sound::Disarm => {
                let first = tock(Tock { ring: 2300.0, q: 3.0, decay: 0.0016, body: 0.0, body_level: 0.0, peak: 0.36 });
                let second = tock(Tock { ring: 1500.0, q: 3.0, decay: 0.0016, body: 0.0, body_level: 0.0, peak: 0.28 });
                overlay(first, second, 0.05)
            }
            // "Kept" and "refused" level with a move, under the wins: by
            // momentary loudness (BS.1770, `tools/checks/totalMix-ui-sounds.py`)
            // "kept" was the loudest sound in Audra, over every reward, on
            // every save (2026-11-14: -4 dB and -3 dB).
            Sound::Done => finish(overlay(note(880.0, 900.0, 0.05, 0.016), note(1320.0, 1330.0, 0.055, 0.02), 0.058), 0.265),
            Sound::Nope => finish(overlay(note(392.0, 370.0, 0.05, 0.018), note(294.0, 277.0, 0.055, 0.02), 0.062), 0.28),
            Sound::Chime(step) => finish(bell(pentatonic(step)), 0.26),
            Sound::Stop => tock(Tock { ring: 620.0, q: 1.6, decay: 0.0026, body: 110.0, body_level: 0.7, peak: 0.34 }),
            // Bright enough to be heard: at 480 Hz over a 90 Hz body it was
            // the faintest sound as the ear hears it (A-weighted 5.6 dB
            // under the tap, below even the detent); now level with the tap
            // and brighter, so it is not mistaken for one (2026-11-14).
            Sound::Clip => tock(Tock { ring: 1000.0, q: 1.6, decay: 0.003, body: 160.0, body_level: 0.12, peak: 0.34 }),
            // E5 to G5, in the chimes' C pentatonic.
            Sound::Recall => finish(overlay(note(659.0, 664.0, 0.04, 0.014), note(784.0, 790.0, 0.045, 0.016), 0.05), 0.36),
            Sound::Meow => meow(),
            Sound::Purr => purr(),
            Sound::Chirp => chirp(),
            Sound::Rise(step) => {
                let three = overlay(
                    overlay(bell(pentatonic(step)), bell(pentatonic(step + 1)), 0.075),
                    bell(pentatonic(step + 2)),
                    0.15,
                );
                finish(three, 0.3)
            }
        }
    }
}

/// The cat's voice: harmonics of a pitch that follows `pitch(t)` (Hz),
/// each weighted by how near it is to a vowel resonance at `formant(t)`,
/// under `env(t)`; `len` seconds, peak `peak`.
fn voice(len: f32, peak: f32, pitch: impl Fn(f32) -> f32, formant: impl Fn(f32) -> f32, env: impl Fn(f32) -> f32) -> Vec<f32> {
    let n = (len * RATE) as usize;
    let mut phase = 0.0f32;
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let u = t / len;
            let f = pitch(u);
            phase += std::f32::consts::TAU * f / RATE;
            let fm = formant(u);
            let mut v = 0.0;
            for k in 1..=6 {
                let fk = f * k as f32;
                let w = (-((fk - fm) / 450.0).powi(2)).exp() + 0.25 / k as f32;
                v += w * (phase * k as f32).sin();
            }
            v * env(u)
        })
        .collect();
    let max = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if max > 0.0 {
        for v in &mut out {
            *v *= peak / max;
        }
    }
    out
}

/// A soft "mew": up from 620 Hz to 880 and down to 520, the vowel opening
/// from "m" to "ow" and closing again; 0.38 s.
fn meow() -> Vec<f32> {
    voice(
        0.38,
        // Level with a milestone's notes (measured: at 0.3 it was 10 dB
        // over them, the loudest sound in Audra).
        0.1,
        |u| if u < 0.3 { 620.0 + 260.0 * (u / 0.3) } else { 880.0 - 360.0 * ((u - 0.3) / 0.7) },
        |u| 900.0 + 700.0 * (std::f32::consts::PI * u).sin(),
        |u| (u / 0.06).min(1.0) * ((1.0 - u) / 0.3).min(1.0),
    )
}

/// A soft purr: a low voice at 120 Hz, its level pulsing 25 times a
/// second, fading in and out; 0.7 s.
fn purr() -> Vec<f32> {
    voice(
        0.7,
        0.15,
        |_| 120.0,
        |_| 360.0,
        |u| {
            let pulse = 0.55 + 0.45 * (std::f32::consts::TAU * 25.0 * u * 0.7).sin().abs();
            pulse * (u / 0.15).min(1.0) * ((1.0 - u) / 0.25).min(1.0)
        },
    )
}

/// A tiny chirp: 900 Hz gliding up to 1500, 0.13 s.
fn chirp() -> Vec<f32> {
    voice(0.13, 0.13, |u| 900.0 + 600.0 * u, |u| 1600.0 + 400.0 * u, |u| (u / 0.1).min(1.0) * ((1.0 - u) / 0.4).min(1.0))
}

/// `second` laid over `first`, `gap` seconds in.
fn overlay(first: Vec<f32>, second: Vec<f32>, gap: f32) -> Vec<f32> {
    let offset = (gap * RATE) as usize;
    let mut out = vec![0.0; (offset + second.len()).max(first.len())];
    for (i, v) in first.iter().enumerate() {
        out[i] += v;
    }
    for (i, v) in second.iter().enumerate() {
        out[offset + i] += v;
    }
    out
}

/// One round note (the `pop` family): a sine with a touch of its octave,
/// gliding from `f0` to `f1` Hz over `len` seconds, a 3 ms rise and an
/// exponential fall (time constant `tau`).
fn note(f0: f32, f1: f32, len: f32, tau: f32) -> Vec<f32> {
    let n = (len * RATE) as usize;
    let tau_2 = std::f32::consts::TAU;
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let time = i as f32 / RATE;
            let freq = f0 + (f1 - f0) * (time / len).min(1.0);
            phase += tau_2 * freq / RATE;
            let rise = (time / 0.003).min(1.0);
            let env = (rise * std::f32::consts::FRAC_PI_2).sin() * (-time / tau).exp();
            env * (phase.sin() + 0.15 * (2.0 * phase).sin())
        })
        .collect()
}

/// A "tock": the shape of one UI tick.
struct Tock {
    /// The resonance the noise rings through, in Hz, and how narrow it is.
    ring: f32,
    q: f32,
    /// How fast the ring dies away (exponential time constant, s).
    decay: f32,
    /// A low sine under it for weight (0 for none), and how loud.
    body: f32,
    body_level: f32,
    /// The finished sound's peak.
    peak: f32,
}

/// A deterministic noise source (xorshift), so a sound is the same every
/// time and the tests can check it.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// A two-pole band-pass (the RBJ cookbook's, 0 dB at the peak).
struct BandPass {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BandPass {
    fn new(freq: f32, q: f32) -> Self {
        let w = std::f32::consts::TAU * freq / RATE;
        let alpha = w.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * w.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Scales `samples` to `peak`, rolls the top off (one pole at about
/// 7 kHz, so nothing hisses) and fades the last 3 ms to silence.
fn finish(mut samples: Vec<f32>, peak: f32) -> Vec<f32> {
    let k = 1.0 - (-std::f32::consts::TAU * 7000.0 / RATE).exp();
    let mut lp = 0.0;
    for v in samples.iter_mut() {
        lp += k * (*v - lp);
        *v = lp;
    }
    let fade = (0.003 * RATE) as usize;
    let n = samples.len();
    for (i, v) in samples.iter_mut().enumerate().skip(n.saturating_sub(fade)) {
        let x = (n - i) as f32 / fade as f32;
        *v *= x * x;
    }
    let max = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if max > 0.0 {
        for v in samples.iter_mut() {
            *v *= peak / max;
        }
    }
    samples
}

fn tock(t: Tock) -> Vec<f32> {
    let n = (0.035 * RATE) as usize;
    let mut noise = Noise(0x9e37_79b9);
    let mut ring = BandPass::new(t.ring, t.q);
    let tau = std::f32::consts::TAU;
    let out = (0..n)
        .map(|i| {
            let time = i as f32 / RATE;
            // A soft 0.4 ms onset (a quarter sine), then the decay.
            let onset = (time / 0.0004).min(1.0);
            let onset = (onset * std::f32::consts::FRAC_PI_2).sin();
            let click = ring.run(noise.next()) * onset * (-time / t.decay).exp();
            let body = if t.body > 0.0 {
                t.body_level * onset * (-time / (t.decay * 2.5)).exp() * (tau * t.body * time).sin()
            } else {
                0.0
            };
            // The band-pass narrows the noise's level; bring it up to sit
            // with the body.
            3.0 * click + body
        })
        .collect();
    finish(out, t.peak)
}

/// One soft, round pop: a sine gliding from 640 to 470 Hz under a quick
/// rise and a short fall (gone in about 40 ms), with a breath of its octave.
/// A pop like `pop`, gliding from `from` to `to` Hz, its peak `peak`.
fn glide_pop(from: f32, to: f32, peak: f32) -> Vec<f32> {
    let n = (0.07 * RATE) as usize;
    let tau = std::f32::consts::TAU;
    let mut phase = 0.0f32;
    let out = (0..n)
        .map(|i| {
            let time = i as f32 / RATE;
            let glide = (time / 0.04).min(1.0);
            let freq = from + (to - from) * (1.0 - (1.0 - glide) * (1.0 - glide));
            phase += tau * freq / RATE;
            let rise = (time / 0.004).min(1.0);
            let env = (rise * std::f32::consts::FRAC_PI_2).sin() * (-time / 0.013).exp();
            env * (phase.sin() + 0.12 * (2.0 * phase).sin())
        })
        .collect();
    finish(out, peak)
}

fn pop() -> Vec<f32> {
    let n = (0.07 * RATE) as usize;
    let tau = std::f32::consts::TAU;
    let mut phase = 0.0f32;
    let out = (0..n)
        .map(|i| {
            let time = i as f32 / RATE;
            let glide = (time / 0.04).min(1.0);
            let freq = 640.0 - 170.0 * (1.0 - (1.0 - glide) * (1.0 - glide));
            phase += tau * freq / RATE;
            let rise = (time / 0.004).min(1.0);
            let env = (rise * std::f32::consts::FRAC_PI_2).sin() * (-time / 0.013).exp();
            env * (phase.sin() + 0.12 * (2.0 * phase).sin())
        })
        .collect();
    finish(out, 0.4)
}

/// A keep that writes to disk and so can fail: its "kept" is played after
/// the handler, as refused if it failed (`app::update`, `TuxMix::saving`).
pub fn keeps_on_disk(message: &Message) -> bool {
    matches!(message, Message::SnapshotStoreTo(_) | Message::WorkspaceStore(_) | Message::ThemeSaveCopy | Message::SnapshotSaveTo(Some(_)))
}

/// The sound a message makes, judged before it is applied (so a fader's
/// old level is still in the state): toggles click on or off, a fader or
/// pan crossing its detent ticks, and moving around chimes. `None` for
/// everything else, drags included.
pub fn for_message(state: &TuxMix, message: &Message) -> Option<Sound> {
    let on_off = |on: bool| Some(if on { Sound::On } else { Sound::Off });
    match message {
        Message::Mute(_, m) => on_off(*m),
        Message::Solo(_, s) => on_off(*s),
        Message::Phantom(_, on) => on_off(*on),
        Message::CueChanged(_, on) => on_off(*on),
        // LOOP's confirming press, as 48V's (it tapped; the reviewer).
        Message::LoopbackChanged(_, on) => on_off(*on),
        // The M and S masters are lamp keys like the strips': Off when the
        // press clears them, On when it brings them back, a tap when there
        // is nothing to do (it tapped either way; the reviewer).
        Message::GlobalMuteToggle => {
            let any = crate::app::all_channel_ids(state).into_iter().any(|c| crate::app::channel_is_muted(state, c));
            if any { Some(Sound::Off) } else if state.last_cleared_mutes.is_empty() { Some(Sound::Tap) } else { Some(Sound::On) }
        }
        Message::GlobalSoloToggle => {
            let any = crate::app::all_channel_ids(state).into_iter().any(|c| crate::app::channel_is_soloed(state, c));
            if any { Some(Sound::Off) } else if state.last_cleared_solos.is_empty() { Some(Sound::Tap) } else { Some(Sound::On) }
        }
        Message::SetUiLock(on) => on_off(*on),
        Message::DimToggle => on_off(!state.control_room.is_dimmed()),
        Message::TalkbackToggle => on_off(!state.control_room.talkback),
        Message::ExtInToggle => on_off(!state.control_room.ext_in),
        // On needs a Listenback mic: without one, refused (the scout: a Key
        // Command ticked On and nothing happened).
        Message::ListenbackToggle if !state.control_room.listenback && state.control_room.listenback_input.is_none() => Some(Sound::Nope),
        Message::ListenbackToggle => on_off(!state.control_room.listenback),
        Message::MonoToggle => on_off(!state.control_room.mono),
        // Keys that lit but said nothing (critique #50): Reverb & Echo's
        // On, Mute FX, the M/S check.
        Message::FxOn(_, on) => on_off(*on),
        Message::MuteFxToggle => on_off(state.control_room.fx_muted.is_none()),
        Message::MsCheckToggle => on_off(state.control_room.ms_check.is_none()),
        Message::SpeakerBToggle => on_off(!state.control_room.speaker_b),
        Message::RecallMain => Some(Sound::On),
        // A stored snapshot recalled rises; an empty slot or Store's
        // second press is a move or a keep.
        Message::SnapshotClicked(n) if !state.snapshot_storing && state.stored_snapshots.get((*n as usize).wrapping_sub(1)).copied().unwrap_or(false) => {
            Some(Sound::Recall)
        }
        Message::SnapshotClicked(_) => Some(Sound::Move),
        // The gain ticks at every 10 dB as it turns, and bumps at either
        // end of its travel (the owner's fun pass: detents you can hear).
        Message::Gain(idx, g) => {
            let ch = state.device.inputs().get(*idx)?;
            let (old, max) = (ch.gain? as f32, ch.gain_max? as f32);
            let new = *g as f32;
            // Turned into an end from near it; a reset (a double-click) or
            // a jump from far off ticks as a mark, not a bump.
            if old != new && ((new <= 0.0 && old <= 6.0) || (new >= max && old >= max - 6.0)) {
                return Some(Sound::Stop);
            }
            let ten = |v: f32| (v / 10.0).floor();
            (ten(old) != ten(new)).then_some(Sound::Detent)
        }
        // Waiting for a second click: the armed ticks; cancelling taps.
        Message::PhantomArm(_) | Message::LinkArm(_) | Message::LoopArm(_) | Message::ResetMixArm(_) | Message::WorkspaceLoadArm(_) => Some(Sound::Arm),
        Message::SnapshotStore if !state.snapshot_storing => Some(Sound::Arm),
        Message::LayoutStore if !state.layout_storing => Some(Sound::Arm),
        // Keeping something is heard as kept.
        Message::SnapshotStoreTo(_) | Message::WorkspaceStore(_) | Message::EqPresetSave(..) | Message::MicPresetSave(_) | Message::ThemeSaveCopy => {
            Some(Sound::Done)
        }
        Message::LayoutClicked(_) if state.layout_storing => Some(Sound::Done),
        // Recall's "Use Main now": the level kept.
        Message::RecallStoreCurrent => Some(Sound::Done),
        // A mic preset's Remove: the first press arms (it asks for a
        // second), the second takes it away.
        // A preset no longer there: the press does nothing, so it's quiet.
        Message::MicPresetRemove(n) if *n >= state.mic_presets.len() => None,
        Message::MicPresetRemove(n) => {
            let armed = state.mic_preset_remove_armed.is_some_and(|(i, at)| i == *n && at.elapsed() < crate::resetmix::ARM_FOR);
            Some(if armed { Sound::Off } else { Sound::Arm })
        }
        Message::SnapshotSaveTo(Some(_)) => Some(Sound::Done),
        // Reverb and Echo's presets as EQ's and the mic's: a pick and the
        // naming taps, Remove arms then goes with Off (they were silent;
        // the reviewer, 2026-11-14). Their save plays in `fx::save_preset`.
        Message::FxPresetLoad(..) | Message::FxPresetSaveStart(_) | Message::FxPresetCancel => Some(Sound::Tap),
        Message::FxPresetRemove(unit, i) => {
            let count = match unit {
                crate::fx::Unit::Reverb => state.fx_presets.reverb.len(),
                crate::fx::Unit::Echo => state.fx_presets.echo.len(),
            };
            if *i >= count {
                return None;
            }
            let armed = state.fx_remove_armed.is_some_and(|(u, at, when)| u == *unit && at == *i && when.elapsed() < crate::resetmix::ARM_FOR);
            Some(if armed { Sound::Off } else { Sound::Arm })
        }
        // A centred knob through its centre ticks like the pan.
        Message::EqBandGain(idx, band, gain) => {
            let old = eq_gain(state, *idx, *band)?;
            crosses(old, *gain, 0.0).then_some(Sound::Detent)
        }
        Message::SetView(v) if *v != state.view => Some(Sound::Move),
        Message::OpenFloor(f) if *f != state.open_floor => Some(Sound::Move),
        Message::ToggleFloor(_) => Some(Sound::Move),
        // Folding glides down, opening glides up.
        Message::ToggleCollapse(cid) => Some(Sound::Fold(!state.collapsed.contains(cid))),
        // Stepping through mixes (`[` and `]`, quick picks): the first pick
        // moves, the rest within 1.5 s only tick (the reviewer, 2026-11-14:
        // a pop on every step), as the light wave runs once (`path_quiet`).
        Message::SelectOutput(o) | Message::EditMix(o) if *o != state.sel_out => Some(
            if state.last_mix_pick.is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(1500)) { Sound::Detent } else { Sound::Move },
        ),
        Message::ToggleFlyout(cid, kind) => {
            on_off(!state.flyout_open.contains(&(*cid, *kind)))
        }
        Message::VolumeChanged(cid, out, v) | Message::MatrixVolume(cid, out, v) | Message::MatrixSet(cid, out, v, _) => {
            let old = level(state, *cid, *out)?;
            crosses(old, *v, 1.0).then_some(Sound::Detent)
        }
        Message::PanChanged(cid, out, p) => {
            let old = state.device.pan(*cid, *out).ok()?;
            crosses(old as f32, *p as f32, 0.0).then_some(Sound::Detent)
        }
        // A new theme and a workspace or reset that changes everything
        // are moves too.
        Message::SetTheme(i) if *i != state.theme_shown => Some(Sound::Move),
        Message::WorkspaceLoad(_) | Message::WorkspaceRestoreBefore | Message::ResetMix(_) => Some(Sound::Move),
        // Voice, Quick setup's step 4, Rec and the folded band (the owner,
        // 2026-11-12: "add subtle fun sounds throughout"; they were silent).
        // A knob's drag (`Param`) stays silent, as faders do.
        Message::Dyn(cid, m) => voice_sound(state, *cid, m),
        Message::AutoTune(_, on) => on_off(*on),
        Message::QuickSound(_) | Message::QuickOpenVoice => Some(Sound::Move),
        Message::QuickShape(on) => on_off(*on),
        Message::QuickVoiceSkip(_) | Message::BandMore(_) => Some(Sound::Tap),
        // Rec and a picked folder are heard after the fact (`record_toggle`,
        // `say_take`, `RecordFolderPicked`): judged here, Rec ticked "on"
        // when the take couldn't start, its stop tick swallowed the saved
        // take's notes, and a folder Rec can't use sounded "kept"
        // (OpenCode's job 31).
        Message::RecordToggle | Message::RecordFolderPicked(_) => None,
        // A ready-made setup: one move for the whole change (its steps'
        // own clicks fall inside the gap after it), the armed tick when it
        // leaves 48V waiting for your click, refused when it can't (the
        // scout: Podcast was silent, the others tapped by chance).
        Message::Setup(setup) => Some(match crate::setups::outcome(state, *setup) {
            Some(true) => Sound::Arm,
            Some(false) => Sound::Move,
            None => Sound::Nope,
        }),
        // Room EQ's page (#29): its switches tick, its picks tap, a
        // switch-off that only asks arms; its knobs' drags are silent.
        Message::RoomEq(m) => crate::room_eq::sound(state, m),
        // A problem fixed is a real win, and "Heads up" never fires in the
        // simulation (the scout).
        Message::AssistFix(_) => Some(Sound::Done),
        // Every other press taps: tabs, panels, switches, pickers, undo,
        // zoom. Drags (faders, knobs), typing and the background messages
        // stay silent, as they are not presses.
        message if is_press(message) => Some(Sound::Tap),
        _ => None,
    }
}

/// A switch row's press (`switched`: turning it on or off) ticks On or
/// Off where its message alone would tap or say nothing; a sound of its
/// own (48V's armed ticks, a refusal, a keep) is kept.
pub fn for_switch(switched: Option<bool>, sound: Option<Sound>) -> Option<Sound> {
    match (switched, sound) {
        (Some(on), None | Some(Sound::Tap)) => Some(if on { Sound::On } else { Sound::Off }),
        (_, sound) => sound,
    }
}

/// A Voice panel press: on and off tick, a new sound pops, Reset arms
/// with the catch and goes with Off, folds and choices tap; a knob's drag
/// is silent.
fn voice_sound(state: &TuxMix, cid: tuxmix_core::ChannelId, m: &crate::dynamics_panel::DynMsg) -> Option<Sound> {
    use crate::dynamics_panel::DynMsg;
    let on_off = |on: bool| Some(if on { Sound::On } else { Sound::Off });
    match m {
        // An output's ON off may only ask first (apps play there): heard in
        // the handler, once it knows (`dynamics_panel::update`), so the
        // check that asks pactl runs once.
        DynMsg::Power(false) if matches!(cid, tuxmix_core::ChannelId::Output(_)) => None,
        DynMsg::Power(on) | DynMsg::Dynamics(on) | DynMsg::AutoLevel(on) | DynMsg::Even(on) | DynMsg::Protect(on) | DynMsg::LowDelay(on) => on_off(*on),
        // A pick on a mic whose Voice runs on another only asks "Move your
        // shaped voice here?": the armed tick, waiting for the answer, not
        // the move's (OpenCode's job 31).
        DynMsg::Preset(_) | DynMsg::Yours if crate::dynamics_panel::asks_to_move(state, cid) => Some(Sound::Arm),
        DynMsg::Preset(_) | DynMsg::Yours | DynMsg::MoveHere | DynMsg::MoveVoice(true) => Some(Sound::Move),
        // Judged as the handler judges (the scout: an expired wait read as
        // still armed, so a first press played Off while only arming).
        DynMsg::Natural if !crate::dynamics_panel::reset_waiting(state, cid) => Some(Sound::Arm),
        DynMsg::Natural => Some(Sound::Off),
        DynMsg::Param(..) | DynMsg::MaxVolume(_) => None,
        _ => Some(Sound::Tap),
    }
}

/// Whether `message` is a press of something on screen (or a key that
/// stands for one), which taps.
fn is_press(message: &Message) -> bool {
    matches!(
        message,
        Message::SetView(_)
            | Message::SidebarTab(_)
            | Message::SidebarTabOpen(_)
            | Message::ToggleSettings
            | Message::SettingsPage(_)
            | Message::SettingsResetOpen(_)
            | Message::MonitorMoreOpen(_)
            | Message::ToggleDevicePanel
            | Message::ToggleAbout
            | Message::ToggleWorkspaces
            | Message::ToggleRoomEq
            | Message::ToggleSidebar
            | Message::SetRowMode(_)
            | Message::SetExclusiveSolo(_)
            | Message::SetLiveMode(_)
            | Message::SetTrimAll(_)
            | Message::SetFreeRouting(_)
            | Message::SetShowOriginalNames(_)
            | Message::SetSnapshotKeep(_)
            | Message::SetMixerPref(..)
            | Message::SetAnimations(_)
            | Message::SetKeyCommand(..)
            | Message::SetMidiEnabled(_)
            | Message::MidiLearn(_)
            | Message::MidiUnbind(_)
            | Message::LayoutClicked(_)
            | Message::LayoutStore
            | Message::HideStrip(_)
            | Message::SetStripShown(..)
            | Message::SetChannelView(_)
            | Message::OpenChannelLayout
            | Message::SetMatrixMono(_)
            | Message::PhantomArm(_)
            | Message::LinkArm(_)
            | Message::WorkspacesShowAll(_)
            | Message::SetMeterRms(_)
            | Message::SetLayoutRemote(_)
            | Message::SetMackie(_)
            | Message::SetMackieMeters(_)
            | Message::MackieDevice(_)
            | Message::MackieExtender(_)
            | Message::SetMackieExtenderLeft(_)
            | Message::SetStripRemote(..)
            | Message::SetRmsPlus3(_)
            | Message::SetThemeAuto(_)
            | Message::ThemesReload
            | Message::ThemesOpenFolder
            | Message::ThemeSaveCopy
            | Message::SetThemeDay(_)
            | Message::SetThemeNight(_)
            | Message::SetThemeDayFrom(_)
            | Message::SetThemeNightFrom(_)
            | Message::SnapshotRenameStart(_)
            | Message::SnapshotSaveFile
            | Message::SnapshotOpenFile
            | Message::SnapshotStore
            | Message::Pad(..)
            | Message::StereoLinkChanged(..)
            | Message::LoopbackChanged(..)
            | Message::EqEnabled(..)
            | Message::EqLowCutEnabled(..)
            | Message::PhaseChanged(..)
            | Message::An12Changed(_)
            | Message::RefLevel(..)
            | Message::Sensitivity(..)
            | Message::ResetMixArm(_)
            | Message::WorkspaceStore(_)
            | Message::WorkspaceLoadArm(_)
            | Message::ScrollRowTo(..)
            | Message::StoreScrollMark(..)
            | Message::RecallStoreCurrent
            | Message::SetAssign(_)
            | Message::GroupEditToggle
            | Message::GroupLinkToggle(..)
            | Message::GroupClear
            | Message::ClipReset(..)
            | Message::MixCopy(_)
            | Message::MixPaste(_)
            | Message::MixClear(_)
            | Message::MixMirror(_)
            | Message::MixMirrorStop(_)
            | Message::TrimStripToggle(_)
            | Message::NoTrimToggle(_)
            | Message::TalkbackOutputChanged(..)
            | Message::SetStripRoute(..)
            | Message::EqPresetLoad(..)
            | Message::EqPresetSave(..)
            | Message::MicPresetLoad(..)
            | Message::MicPresetSave(_)
            | Message::MicPresetRemove(_)
            | Message::ZoomIn
            | Message::ZoomOut
            | Message::ZoomReset
            | Message::Undo
            | Message::Redo
            | Message::GlobalMuteToggle
            | Message::GlobalSoloToggle
            | Message::SpeakerBLinkToggle
    )
}

fn eq_gain(state: &TuxMix, idx: usize, band: usize) -> Option<f32> {
    let eq = state.device.inputs().get(idx)?.eq.as_ref()?;
    eq.bands.get(band).map(|b| b.gain_db)
}

fn level(state: &TuxMix, cid: ChannelId, out: usize) -> Option<f32> {
    match cid {
        ChannelId::Output(ch) => state.device.outputs().get(ch).map(|o| o.volume),
        _ => state.device.volume(cid, out).ok(),
    }
}

/// Whether a move from `old` to `new` lands on or passes `mark`.
fn crosses(old: f32, new: f32, mark: f32) -> bool {
    old != new && (old - mark) * (new - mark) <= 0.0 && old != mark
}

/// Set once `pw-play` has failed to start, so a system without it logs
/// once and then stays quiet.
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// Locked (Lock User Interface): Audra makes no sound at all, its notes
/// play on the unit a stream may be capturing (critique #9: a refused
/// nudge put a "nope" on air at the moment the lock exists for). The
/// padlock's shake and the notice still say it.
static QUIET: AtomicBool = AtomicBool::new(false);

/// Kept in step with the lock, on the tick (`app`).
pub fn set_quiet(quiet: bool) {
    QUIET.store(quiet, Ordering::Relaxed);
}

/// On air: Rec is recording, an app records from Voice, or a mic is open
/// into speakers that are up. Kept on the tick (`app`). Then the win
/// notes and the clip tick hold back, the cards and sparks carrying the
/// moment silently; a press's own click still answers it (critique #50:
/// "On the air"'s notes played as the call began, the clip tick when the
/// mic was hottest, all into the room the mic is in).
static ON_AIR: AtomicBool = AtomicBool::new(false);

pub fn set_on_air(on: bool) {
    ON_AIR.store(on, Ordering::Relaxed);
}

/// When the last sound started (ms since the epoch): sounds closer than
/// `MIN_GAP_MS` are dropped, so a burst of messages is one click.
static LAST_MS: AtomicU64 = AtomicU64::new(0);
const MIN_GAP_MS: u64 = 35;
/// How long after the sound before it a win's notes start (a click and its
/// ring), so the two never sound at once.
const WIN_AFTER_MS: u64 = 180;
/// How long the sound before a win rings, when longer than that: "kept"
/// rings 0.42 s, and a rise right after a Fix it began on its tail
/// (OpenCode's job 34). A win before a win keeps the 180 ms climb.
static LAST_LEN_MS: AtomicU64 = AtomicU64::new(0);
/// How long a player may run before it counts as stuck and is ended.
const STALL_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);

#[cfg(test)]
thread_local! {
    /// What `play` was asked to play in this test thread (it plays nothing).
    pub static TEST_HEARD: std::cell::RefCell<Vec<Sound>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Plays `sound` at `volume_db`, in the background. Never blocks the UI.
pub fn play(sound: Sound, volume_db: f32) {
    // The tests drive `update` with sounds on (the default): never let a
    // test run click through the speakers.
    if cfg!(test) {
        #[cfg(test)]
        TEST_HEARD.with(|h| h.borrow_mut().push(sound));
        return;
    }
    // Nor a screenshot capture: its probes press keys, and the notes would
    // play on the owner's speakers (OpenCode's job 31).
    if crate::app::CAPTURE_RUN.load(Ordering::Relaxed) {
        return;
    }
    if UNAVAILABLE.load(Ordering::Relaxed) || QUIET.load(Ordering::Relaxed) {
        return;
    }
    if ON_AIR.load(Ordering::Relaxed) && matches!(sound, Sound::Chime(_) | Sound::Rise(_) | Sound::Clip | Sound::Meow | Sound::Purr | Sound::Chirp) {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let last = LAST_MS.load(Ordering::Relaxed);
    // A win's notes come on the tick after the click that made it: never
    // swallowed by that click's gap, and never on top of it either: they
    // wait for the click to ring out (`WIN_AFTER_MS`), so "done" and then
    // the rise play one after the other.
    let win = matches!(sound, Sound::Rise(_) | Sound::Chime(_));
    // Only against a sound that has started: a win waiting to play (its
    // time ahead of now) never swallows the clicks before it (the audit:
    // for up to 420 ms after a win every press was silent).
    if !win && last <= now && now - last < MIN_GAP_MS {
        return;
    }
    // Measured from the sound before's start, which may itself still be
    // waiting (a guide's steps ticking together), so they climb in turn.
    let samples = sound.samples();
    let ring = WIN_AFTER_MS.max(LAST_LEN_MS.load(Ordering::Relaxed));
    let wait = if win { (last + ring).saturating_sub(now) } else { 0 };
    LAST_MS.store(now + wait, Ordering::Relaxed);
    LAST_LEN_MS.store(if win { 0 } else { (samples.len() as f32 / RATE * 1000.0) as u64 }, Ordering::Relaxed);
    let gain = 10f32.powf(volume_db.clamp(VOLUME_RANGE.0, VOLUME_RANGE.1) / 20.0);
    let bytes: Vec<u8> = samples
        .into_iter()
        .flat_map(|s| (s * gain).to_le_bytes())
        .collect();
    std::thread::spawn(move || {
        if wait > 0 {
            std::thread::sleep(std::time::Duration::from_millis(wait));
        }
        let child = Command::new("pw-play")
            .args([
                "--raw",
                "--format=f32",
                "--rate=48000",
                "--channels=1",
                "--latency=20ms",
                "--media-role=Notification",
                "-P",
                "{ node.name = audra-ui-sounds node.description = \"Audra UI sounds\" }",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(&bytes);
                }
                // A sound lasts under 0.1 s. A player that hasn't finished
                // in 2 s is stuck (no output to drain into) and is ended,
                // so stalled players can't pile up one per click.
                let started = std::time::Instant::now();
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) if started.elapsed() < STALL_LIMIT => {
                            std::thread::sleep(std::time::Duration::from_millis(20));
                        }
                        _ => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                if !UNAVAILABLE.swap(true, Ordering::Relaxed) {
                    log::warn!("UI sounds are off: could not start pw-play ({e})");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    /// Writes every UI sound as a 16-bit WAV (48 kHz, mono, at its raw
    /// level) into target/ui-sounds/, for measuring and listening
    /// (`tools/checks/totalMix-ui-sounds.py`). Nothing plays. Run with
    /// `cargo test -p audra -- --ignored render_ui_sounds`.
    #[test]
    #[ignore]
    fn render_ui_sounds() {
        use super::Sound;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-sounds");
        std::fs::create_dir_all(&dir).unwrap();
        let all: Vec<(String, Sound)> = [
            ("on", Sound::On), ("off", Sound::Off), ("detent", Sound::Detent), ("move", Sound::Move),
            ("tap", Sound::Tap), ("arm", Sound::Arm), ("disarm", Sound::Disarm), ("done", Sound::Done),
            ("nope", Sound::Nope), ("stop", Sound::Stop), ("recall", Sound::Recall), ("clip", Sound::Clip),
        ]
        .into_iter()
        .map(|(n, s)| (n.to_string(), s))
        .chain((0..8).map(|k| (format!("chime{k}"), Sound::Chime(k))))
        .chain((0..8).map(|k| (format!("rise{k}"), Sound::Rise(k))))
        .chain([("fold".to_string(), Sound::Fold(true)), ("unfold".to_string(), Sound::Fold(false))])
        .chain([("meow".to_string(), Sound::Meow), ("purr".to_string(), Sound::Purr), ("chirp".to_string(), Sound::Chirp)])
        .collect();
        for (name, sound) in all {
            let x = sound.samples();
            let data: Vec<u8> = x.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
            let mut f = Vec::new();
            f.extend(b"RIFF");
            f.extend(&(36 + data.len() as u32).to_le_bytes());
            f.extend(b"WAVEfmt ");
            f.extend(&16u32.to_le_bytes());
            f.extend(&1u16.to_le_bytes());
            f.extend(&1u16.to_le_bytes());
            f.extend(&48_000u32.to_le_bytes());
            f.extend(&96_000u32.to_le_bytes());
            f.extend(&2u16.to_le_bytes());
            f.extend(&16u16.to_le_bytes());
            f.extend(b"data");
            f.extend(&(data.len() as u32).to_le_bytes());
            f.extend(&data);
            std::fs::write(dir.join(format!("{name}.wav")), f).unwrap();
        }
    }

    #[test]
    fn voice_quick_rec_and_the_band_make_their_sounds() {
        // The owner, 2026-11-12: "add subtle fun sounds throughout".
        use crate::app::{new, Message};
        use crate::dynamics_panel::{DynMsg, DynParam};
        use tuxmix_core::ChannelId;
        let state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Power(true))), Some(Sound::On));
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Power(false))), Some(Sound::Off));
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Preset(1))), Some(Sound::Move));
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::FineTune(true))), Some(Sound::Tap));
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Natural)), Some(Sound::Arm), "Reset arms first");
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Param(DynParam::CompRatio, 3.0))), None, "a knob's drag is silent");
        assert_eq!(for_message(&state, &Message::QuickSound(0)), Some(Sound::Move));
        assert_eq!(for_message(&state, &Message::QuickShape(true)), Some(Sound::On));
        assert_eq!(for_message(&state, &Message::BandMore(true)), Some(Sound::Tap));
        // Heard after the fact, where the press is handled (job 31).
        assert_eq!(for_message(&state, &Message::RecordToggle), None);
        assert_eq!(for_message(&state, &Message::RecordFolderPicked(None)), None);
        // A pick on IN3/4 while Voice runs on AN1 only asks: the armed tick.
        let mut elsewhere = new(true, None, None);
        elsewhere.dyn_input = Some(0);
        let in3 = ChannelId::Input(2);
        assert!(crate::dynamics_panel::asks_to_move(&elsewhere, in3));
        assert_eq!(for_message(&elsewhere, &Message::Dyn(in3, DynMsg::Preset(1))), Some(Sound::Arm));
        assert_eq!(for_message(&elsewhere, &Message::Dyn(in3, DynMsg::Yours)), Some(Sound::Arm));
        assert_eq!(for_message(&elsewhere, &Message::Dyn(an1, DynMsg::Preset(1))), Some(Sound::Move), "on its own mic: a move");
    }

    #[test]
    fn voice_reset_is_heard_as_its_handler_judges_it_and_its_wait_ends() {
        // The scout: an expired wait still read as armed, so a first press
        // played Off while only arming, and the wait ended silently.
        use crate::app::{new, update, Message};
        use crate::dynamics_panel::DynMsg;
        use tuxmix_core::ChannelId;
        let an1 = ChannelId::Input(0);
        let mut state = new(true, None, None);
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Natural)), Some(Sound::Arm));
        let _ = update(&mut state, Message::Dyn(an1, DynMsg::Natural));
        assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Natural)), Some(Sound::Off), "the second press resets");
        // The wait runs out.
        let long_ago = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(10));
        if let (Some((k, _)), Some(at)) = (state.dyn_reset_armed, long_ago) {
            state.dyn_reset_armed = Some((k, at));
            assert_eq!(for_message(&state, &Message::Dyn(an1, DynMsg::Natural)), Some(Sound::Arm), "expired: arms again");
            let _ = update(&mut state, Message::Tick);
            assert!(state.dyn_reset_armed.is_none(), "the tick lets the wait go (and Disarm is heard)");
        }
    }

    #[test]
    fn a_setup_and_fix_it_are_heard_as_one_move_and_a_win() {
        // The scout: Podcast was silent, the others tapped by chance.
        use crate::app::{new, Message};
        let state = new(true, None, None);
        for setup in [crate::setups::Setup::Podcast, crate::setups::Setup::Record] {
            let heard = for_message(&state, &Message::Setup(setup));
            assert!(matches!(heard, Some(Sound::Move | Sound::Arm | Sound::Nope)), "{setup:?}: {heard:?}");
            let want = match crate::setups::outcome(&state, setup) {
                Some(true) => Sound::Arm,
                Some(false) => Sound::Move,
                None => Sound::Nope,
            };
            assert_eq!(heard, Some(want));
        }
        assert_eq!(for_message(&state, &Message::AssistFix(crate::assist::Assist::Clipping(0))), Some(Sound::Done));
    }

    #[test]
    fn listenback_without_a_mic_is_refused_and_says_why() {
        use crate::app::{new, update, Message};
        let mut state = new(true, None, None);
        state.control_room.listenback_input = None;
        assert_eq!(for_message(&state, &Message::ListenbackToggle), Some(Sound::Nope));
        let _ = update(&mut state, Message::ListenbackToggle);
        assert!(!state.control_room.listenback);
        assert!(state.notice.as_ref().is_some_and(|(t, _)| t.contains("Listenback needs a mic")));
    }

    #[test]
    fn a_pick_asks_to_move_only_from_another_mic() {
        use crate::app::new;
        use tuxmix_core::ChannelId;
        let mut state = new(true, None, None);
        assert!(!crate::dynamics_panel::asks_to_move(&state, ChannelId::Input(0)), "Voice off: no question");
        state.dyn_input = Some(0);
        assert!(!crate::dynamics_panel::asks_to_move(&state, ChannelId::Input(0)), "its own mic");
        assert!(!crate::dynamics_panel::asks_to_move(&state, ChannelId::Output(0)), "outputs never ask");
    }

    use super::*;

    #[test]
    fn every_sound_is_short_and_never_clips() {
        for sound in [Sound::On, Sound::Off, Sound::Detent, Sound::Move, Sound::Tap, Sound::Arm, Sound::Disarm, Sound::Stop] {
            let s = sound.samples();
            assert!(!s.is_empty() && s.len() as f32 / RATE <= 0.1, "{sound:?}");
            // Soft: well under full scale, before the volume setting.
            assert!(s.iter().all(|v| v.abs() <= 0.65), "{sound:?}");
            // Starts and ends near silence: no pop.
            assert!(s[0].abs() < 0.05 && s[s.len() - 1].abs() < 0.01, "{sound:?}");
            // Over within a few milliseconds of its peak, like a tap (the
            // disarm's peak is its first tick, so its second comes later).
            if sound == Sound::Disarm {
                continue;
            }
            let peak_at = s.iter().enumerate().fold((0, 0.0f32), |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m }).0;
            let late: f32 = s[peak_at..].iter().skip((0.03 * RATE) as usize).map(|v| v.abs()).fold(0.0, f32::max);
            assert!(late < 0.05, "{sound:?} rings on: {late}");
        }
        // The ticks are noise through a resonance, not a pitched beep: no
        // single frequency carries them (the old ones were sines).
        let tap = Sound::Tap.samples();
        let tone = |f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in tap.iter().enumerate() {
                let ph = std::f32::consts::TAU * f * i as f32 / RATE;
                re += v * ph.cos();
                im += v * ph.sin();
            }
            (re * re + im * im).sqrt()
        };
        let energy: f32 = tap.iter().map(|v| v * v).sum::<f32>().sqrt() * (tap.len() as f32).sqrt();
        assert!(tone(1320.0) < 0.5 * energy, "no strong 1320 Hz beep left in the tap");
    }

    #[test]
    fn kept_and_refused_are_two_short_notes_the_ear_tells_apart() {
        for sound in [Sound::Done, Sound::Nope] {
            let s = sound.samples();
            assert!(s.len() as f32 / RATE <= 0.12, "{sound:?} is short");
            assert!(s.iter().all(|v| v.abs() <= 0.45), "{sound:?} is soft");
            assert!(s[0].abs() < 0.05 && s[s.len() - 1].abs() < 0.01, "{sound:?} has no pop");
        }
        // Pitched, unlike the clicks: Done rises and sits well above Nope.
        let energy_at = |s: &[f32], f: f32| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in s.iter().enumerate() {
                let ph = std::f32::consts::TAU * f * i as f32 / RATE;
                re += v * ph.cos();
                im += v * ph.sin();
            }
            (re * re + im * im).sqrt()
        };
        let (done, nope) = (Sound::Done.samples(), Sound::Nope.samples());
        assert!(energy_at(&done, 1320.0) > 3.0 * energy_at(&done, 300.0), "Done is high");
        assert!(energy_at(&nope, 300.0) > 3.0 * energy_at(&nope, 1320.0), "Nope is low");
        // The disarm is the arm's mirror: its second tick is the softer.
        let disarm = Sound::Disarm.samples();
        let half = (0.05 * RATE) as usize;
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak(&disarm[half..]) < peak(&disarm[..half]), "the catch goes back on, quieter");
    }

    /// Writes every sound as raw f32 (48 kHz, mono) into the folder in
    /// `AUDRA_SOUND_DIR`, to listen to them with `pw-play`.
    #[test]
    #[ignore]
    fn write_the_sounds() {
        let Some(dir) = std::env::var_os("AUDRA_SOUND_DIR") else { return };
        let dir = std::path::PathBuf::from(dir);
        for (name, sound) in [
            ("tap", Sound::Tap),
            ("on", Sound::On),
            ("off", Sound::Off),
            ("detent", Sound::Detent),
            ("move", Sound::Move),
            ("arm", Sound::Arm),
            ("disarm", Sound::Disarm),
            ("done", Sound::Done),
            ("nope", Sound::Nope),
            ("stop", Sound::Stop),
            ("recall", Sound::Recall),
        ] {
            let bytes: Vec<u8> = sound.samples().into_iter().flat_map(|v| v.to_le_bytes()).collect();
            std::fs::write(dir.join(format!("{name}.raw")), bytes).unwrap();
        }
    }

    #[test]
    fn a_fader_ticks_only_when_it_crosses_unity() {
        assert!(crosses(0.5, 1.2, 1.0));
        assert!(crosses(1.2, 1.0, 1.0), "landing on the mark ticks");
        assert!(!crosses(1.0, 1.2, 1.0), "leaving the mark does not tick again");
        assert!(!crosses(0.2, 0.5, 1.0));
        assert!(!crosses(0.5, 0.5, 1.0));
    }

    #[test]
    fn a_save_that_fails_is_heard_as_refused_not_as_saved() {
        // The reviewer, 2026-11-14: "kept" was judged before the handler.
        let heard = |path: std::path::PathBuf| {
            let mut state = crate::app::new(true, None, None);
            state.ui_sounds = true;
            TEST_HEARD.with(|h| h.borrow_mut().clear());
            let _ = crate::app::update(&mut state, Message::SnapshotSaveTo(Some(path)));
            TEST_HEARD.with(|h| h.borrow().clone())
        };
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target"));
        assert_eq!(heard(dir.join("no-such-folder").join("snap.json")), vec![Sound::Nope]);
        let good = dir.join("audra-save-sound-test.json");
        assert_eq!(heard(good.clone()), vec![Sound::Done]);
        let _ = std::fs::remove_file(good);
    }

    #[test]
    fn switches_tick_on_and_off() {
        // The owner, 2026-11-24: "switches dont make sounds when i toggle
        // them". A switch row's press ticks as a lamp key does.
        let heard = |state: &mut crate::app::TuxMix, message: Message| {
            TEST_HEARD.with(|h| h.borrow_mut().clear());
            let _ = crate::app::update(state, message);
            TEST_HEARD.with(|h| h.borrow().clone())
        };
        let switched = |on: bool, m: Message| Message::Switched(on, Box::new(m));
        let mut state = crate::app::new(true, None, None);
        state.ui_sounds = true;
        // One that only tapped, and one that said nothing.
        assert_eq!(heard(&mut state, switched(true, Message::SetMeterRms(true))), vec![Sound::On]);
        assert_eq!(heard(&mut state, switched(false, Message::SetMeterRms(false))), vec![Sound::Off]);
        assert_eq!(heard(&mut state, switched(false, Message::SetTips(false))), vec![Sound::Off]);
        assert_eq!(heard(&mut state, switched(true, Message::SetTips(true))), vec![Sound::On]);
        // The sounds' own switch: Off as they go, one On as they come back.
        assert_eq!(heard(&mut state, switched(false, Message::SetUiSounds(false))), vec![Sound::Off]);
        assert_eq!(heard(&mut state, switched(true, Message::SetUiSounds(true))), vec![Sound::On]);
        // Celebration sounds' own rise as it comes on, with no tick on top.
        assert_eq!(heard(&mut state, switched(false, Message::SetCelebrationSounds(false))), vec![Sound::Off]);
        assert_eq!(heard(&mut state, switched(true, Message::SetCelebrationSounds(true))), vec![Sound::Rise(0)]);
        assert!(state.switch_press.is_none(), "nothing left over for the next press");
        // A sound of its own is kept: 48V's armed ticks, a refusal.
        assert_eq!(for_switch(Some(true), Some(Sound::Arm)), Some(Sound::Arm));
        assert_eq!(for_switch(Some(false), Some(Sound::Nope)), Some(Sound::Nope));
        assert_eq!(for_switch(None, Some(Sound::Tap)), Some(Sound::Tap), "not a switch: as before");
    }

    #[test]
    fn stepping_through_mixes_moves_once_then_ticks() {
        let mut state = crate::app::new(true, None, None);
        state.sel_out = 0;
        assert_eq!(for_message(&state, &Message::SelectOutput(1)), Some(Sound::Move), "the first pick");
        state.last_mix_pick = Some(std::time::Instant::now());
        assert_eq!(for_message(&state, &Message::SelectOutput(1)), Some(Sound::Detent), "a quick next step");
        state.last_mix_pick = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(2));
        assert_eq!(for_message(&state, &Message::SelectOutput(1)), Some(Sound::Move), "a pick a while later");
    }

    #[test]
    fn reverb_and_echo_presets_are_heard_as_eq_and_mic_presets_are() {
        use crate::fx::Unit;
        let mut state = crate::app::new(true, None, None);
        assert_eq!(for_message(&state, &Message::FxPresetSaveStart(Unit::Reverb)), Some(Sound::Tap));
        assert_eq!(for_message(&state, &Message::FxPresetRemove(Unit::Reverb, 0)), None, "nothing there");
        state.fx_presets.reverb.push(crate::fx::Named { name: "Mine".into(), params: state.fx.reverb });
        assert_eq!(for_message(&state, &Message::FxPresetRemove(Unit::Reverb, 0)), Some(Sound::Arm), "first press asks");
        let _ = crate::app::update(&mut state, Message::FxPresetRemove(Unit::Reverb, 0));
        assert_eq!(for_message(&state, &Message::FxPresetRemove(Unit::Reverb, 0)), Some(Sound::Off), "the second removes");
    }

    #[test]
    fn a_fold_glides_down_and_an_unfold_glides_up() {
        let mut state = crate::app::new(true, None, None);
        let an1 = tuxmix_core::ChannelId::Input(0);
        assert_eq!(for_message(&state, &Message::ToggleCollapse(an1)), Some(Sound::Fold(true)));
        let _ = crate::app::update(&mut state, Message::ToggleCollapse(an1));
        assert_eq!(for_message(&state, &Message::ToggleCollapse(an1)), Some(Sound::Fold(false)));
        assert_ne!(Sound::Fold(true).samples(), Sound::Fold(false).samples());
    }
}

/// The simulation's sample of Reverb and Echo (the owner's pick,
/// 2026-11-21: without the unit the page could only say "the simulation
/// plays no effects"). A clap as it is, a short gap, then the same clap
/// through the effects as they are set, rendered offline by Audra's own
/// engine (`audra_fx::FxEngine`, the one that runs with the unit), at
/// 48 kHz stereo. Its loudest sample stands at `PREVIEW_PEAK_DB` before
/// the UI sounds' volume.
pub fn fx_preview(settings: &crate::fx_loop::FxSettings) -> Vec<f32> {
    const RATE_HZ: f32 = 48_000.0;
    let frames = |s: f32| (s * RATE_HZ) as usize;
    let mut out: Vec<f32> = Vec::new();
    // As it is: the engine with both off passes the clap alone.
    let mut dry = audra_fx::FxEngine::new(RATE_HZ);
    dry.reverb_on = false;
    dry.echo_on = false;
    dry.audition();
    let mut buf = vec![0.0f32; 2 * frames(0.6)];
    dry.process(&vec![0.0; buf.len()], &mut buf);
    out.extend_from_slice(&buf);
    out.extend(std::iter::repeat_n(0.0, 2 * frames(0.35)));
    // Through the effects: each rendered on its own (its level glides in
    // first, then the clap), less the clap the engine adds, so the Reverb
    // can be raised by `PREVIEW_WET_DB` and the Echo kept as it is: as the
    // engine leaves it the room sat about 15 dB under the clap, and raised
    // together the echo's repeats came out louder than the clap
    // (measured, tools/checks/totalMix-fxsample.py).
    let render = |reverb: bool, echo: bool| -> Vec<f32> {
        let mut e = audra_fx::FxEngine::new(RATE_HZ);
        e.reverb.set_params(settings.reverb);
        e.echo.set_params(settings.echo);
        e.reverb_on = reverb;
        e.echo_on = echo;
        let mut lead = vec![0.0f32; 2 * frames(0.05)];
        e.process(&vec![0.0; lead.len()], &mut lead);
        e.audition();
        let mut buf = vec![0.0f32; 2 * frames(PREVIEW_TAIL_S)];
        e.process(&vec![0.0; buf.len()], &mut buf);
        buf
    };
    let clap = render(false, false);
    let reverb = if settings.reverb_on { render(true, false) } else { clap.clone() };
    let echo = if settings.echo_on { render(false, true) } else { clap.clone() };
    let wet_gain = 10f32.powf(PREVIEW_WET_DB / 20.0);
    let mut tail: Vec<f32> = clap.iter().zip(reverb.iter().zip(&echo)).map(|(c, (r, e))| c + (r - c) * wet_gain + (e - c)).collect();
    // A short fade at the end, so a long tail stops without a click.
    let fade = 2 * frames(0.15);
    let n = tail.len();
    for (k, s) in tail[n - fade..].iter_mut().enumerate() {
        *s *= 1.0 - (k / 2) as f32 / (fade / 2) as f32;
    }
    out.extend_from_slice(&tail);
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let to = 10f32.powf(PREVIEW_PEAK_DB / 20.0) / peak;
        for s in &mut out {
            *s *= to;
        }
    }
    out
}

/// How far the Reverb is raised against the clap in the sample, in dB.
pub const PREVIEW_WET_DB: f32 = 12.0;
/// The sample's loudest sample, in dBFS, before the UI sounds' volume.
pub const PREVIEW_PEAK_DB: f32 = -12.0;
/// How long the effects ring after the clap in the sample, in seconds.
pub const PREVIEW_TAIL_S: f32 = 2.6;

/// Plays `fx_preview`'s sample once, quietly, through the computer's
/// output, under the same rules as every UI sound: never from a test or a
/// capture run, not when sounds are unavailable or Audra is quiet on air.
pub fn play_fx_preview(samples: Vec<f32>, volume_db: f32) {
    if cfg!(test) || crate::app::CAPTURE_RUN.load(Ordering::Relaxed) {
        return;
    }
    if UNAVAILABLE.load(Ordering::Relaxed) || QUIET.load(Ordering::Relaxed) || ON_AIR.load(Ordering::Relaxed) {
        return;
    }
    let gain = 10f32.powf(volume_db.clamp(VOLUME_RANGE.0, VOLUME_RANGE.1) / 20.0);
    let bytes: Vec<u8> = samples.into_iter().flat_map(|s| (s * gain).to_le_bytes()).collect();
    std::thread::spawn(move || {
        let child = Command::new("pw-play")
            .args([
                "--raw",
                "--format=f32",
                "--rate=48000",
                "--channels=2",
                "--latency=40ms",
                "--media-role=Notification",
                "-P",
                "{ node.name = audra-fx-sample node.description = \"Audra Reverb sample\" }",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&bytes);
            }
            // About 3.5 s of sound; a player still going after 8 s is stuck.
            let started = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    _ if started.elapsed() > std::time::Duration::from_secs(8) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    _ => std::thread::sleep(std::time::Duration::from_millis(50)),
                }
            }
        }
    });
}
