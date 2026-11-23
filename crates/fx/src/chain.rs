//! The processed mic's whole chain in one place (what the D panel's
//! "Audra processed mic" plays): noise removal → Auto-Tune → harsh S
//! sounds → the dynamics → the Cough key's fade.
//!
//! One copy, so what ships is what gets measured: the live loop
//! (`crates/app/src/dynamics_loop.rs`), the timing test below and the
//! offline renderer (`examples/voice_render.rs`, used by
//! `tools/checks/totalMix-voice-quality.py`) all run this. A second look's
//! catch: the order lived inline in the loop and again in a test, and a
//! third copy for measuring would have graded a twin.
//!
//! Noise removal and Auto-Tune each make the sound later (20 and about 17
//! ms), so switching one in or out mid-word is a jump in time: measured
//! (2026-11-10), it cut 17–18 ms of silence into the voice and clicked.
//! So each is switched with a crossfade: turned on, it warms up unheard
//! until its output is real, then fades in over 10 ms; turned off, it
//! fades out over 10 ms before it stops.
//!
//! The order: noise removal first, so RNNoise hears the room as it is,
//! before anything raises it (the compressor, Auto Level); Auto-Tune
//! before the dynamics, whose clamp and reset then cover it; the S sounds
//! tamed before the compressor raises them; Cough last, over everything.

use crate::autotune::AutoTune;
use crate::deesser::DeEsser;
use crate::denoise::Denoise;
use crate::{Dynamics, DynamicsParams};

/// The Cough key's fade: to silence in 8 ms, back in 15 (a second
/// opinion's numbers: mute 5–10 ms, unmute 10–20).
const COUGH_OUT_S: f32 = 0.008;
const COUGH_IN_S: f32 = 0.015;
/// The crossfade that switches a delaying stage in or out.
const SWITCH_S: f32 = 0.010;
/// The most samples processed at once (the dry copy's room).
const CHUNK: usize = 1024;

/// Where a delaying stage is in being switched.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Switch {
    Off,
    /// Running unheard: samples left until its output is real.
    Warming(usize),
    /// Fading in, 0 to 1.
    In(f32),
    On,
    /// Fading out, 0 to 1.
    Out(f32),
}

impl Switch {
    /// At a block's start: where it goes for `want`. `start` is called
    /// when it's turned on from rest (and gives the warm-up in samples).
    fn next(self, want: bool, start: impl FnOnce() -> usize) -> Switch {
        match (self, want) {
            (Switch::Off, true) => Switch::Warming(start()),
            (Switch::Warming(_), false) => Switch::Off,
            (Switch::In(t), false) => Switch::Out(1.0 - t),
            (Switch::On, false) => Switch::Out(0.0),
            (Switch::Out(t), true) => Switch::In(1.0 - t),
            (s, _) => s,
        }
    }

    /// The wet sound's share for the next sample, moving the state on.
    fn step(&mut self, by: f32) -> f32 {
        match *self {
            Switch::Off => 0.0,
            Switch::Warming(k) => {
                *self = if k <= 1 { Switch::In(0.0) } else { Switch::Warming(k - 1) };
                0.0
            }
            Switch::In(t) => {
                let t = t + by;
                *self = if t >= 1.0 { Switch::On } else { Switch::In(t) };
                t.min(1.0)
            }
            Switch::On => 1.0,
            Switch::Out(t) => {
                let t = t + by;
                *self = if t >= 1.0 { Switch::Off } else { Switch::Out(t) };
                (1.0 - t).max(0.0)
            }
        }
    }
}

pub struct VoiceChain {
    rate: f32,
    denoise: Denoise,
    noise_want: u8,
    noise_sw: Switch,
    tune: AutoTune,
    tune_want: bool,
    tune_sw: Switch,
    switch_step: f32,
    dry: [Vec<f32>; 2],
    deess: DeEsser,
    dynamics: Dynamics,
    cough: bool,
    open: f32,
    fade_out: f32,
    fade_in: f32,
}

impl VoiceChain {
    pub fn new(sample_rate: f32, params: DynamicsParams) -> Self {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        Self {
            rate,
            denoise: Denoise::new(rate),
            noise_want: 0,
            noise_sw: Switch::Off,
            tune: AutoTune::new(rate),
            tune_want: false,
            tune_sw: Switch::Off,
            switch_step: 1.0 / (SWITCH_S * rate).max(1.0),
            dry: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            deess: DeEsser::new(rate),
            dynamics: Dynamics::new(rate, params),
            cough: false,
            open: 1.0,
            fade_out: 1.0 / (COUGH_OUT_S * rate).max(1.0),
            fade_in: 1.0 / (COUGH_IN_S * rate).max(1.0),
        }
    }

    pub fn set_dynamics(&mut self, params: DynamicsParams) {
        self.dynamics.set(params);
    }

    /// Noise removal: 0 off, 1 light, 2 strong (switched in and out with
    /// a crossfade; Light and Strong swap without a break, the floor
    /// between them gliding).
    pub fn set_noise(&mut self, level: u8) {
        self.noise_want = level.min(2);
    }

    pub fn noise(&self) -> u8 {
        self.noise_want
    }

    /// Auto-Tune on or off; started afresh each time it's turned on, and
    /// switched in and out with a crossfade.
    pub fn set_tune(&mut self, on: bool) {
        self.tune_want = on;
    }

    /// Harsh S sounds: 0 off, 1 light, 2 strong.
    pub fn set_deess(&mut self, level: u8) {
        self.deess.set_level(level);
    }

    /// The Cough key held (silence) or let go.
    pub fn set_cough(&mut self, shut: bool) {
        self.cough = shut;
    }

    /// How late noise removal makes the sound now, in ms (0 while off).
    pub fn noise_ms(&self) -> f32 {
        self.denoise.latency() as f32 / self.rate * 1000.0
    }

    /// How late Auto-Tune makes the sound now, in ms (0 while off).
    pub fn tune_ms(&self) -> f32 {
        if self.tune_sw != Switch::Off {
            self.tune.delay_samples() / self.rate * 1000.0
        } else {
            0.0
        }
    }

    /// The note Auto-Tune pulls to now (MIDI number), while it's on.
    pub fn tune_note(&self) -> Option<i32> {
        if self.tune_sw != Switch::Off {
            self.tune.note()
        } else {
            None
        }
    }

    pub fn dynamics_mut(&mut self) -> &mut Dynamics {
        &mut self.dynamics
    }

    /// Runs a delaying stage over a block (at most `CHUNK`), its output
    /// mixed with the dry sound by the switch's state.
    fn switched(sw: &mut Switch, by: f32, dry: &mut [Vec<f32>; 2], left: &mut [f32], mut right: Option<&mut [f32]>, run: impl FnOnce(&mut [f32], Option<&mut [f32]>)) {
        let n = left.len();
        if *sw == Switch::On {
            run(left, right);
            return;
        }
        dry[0][..n].copy_from_slice(left);
        if let Some(r) = right.as_deref() {
            dry[1][..r.len().min(n)].copy_from_slice(&r[..r.len().min(n)]);
        }
        run(left, right.as_deref_mut());
        for i in 0..n {
            let g = sw.step(by);
            left[i] = dry[0][i] + (left[i] - dry[0][i]) * g;
            if let Some(r) = right.as_deref_mut().and_then(|r| r.get_mut(i)) {
                *r = dry[1][i] + (*r - dry[1][i]) * g;
            }
        }
    }

    /// Processes a block in place: `left`, and `right` for a linked pair.
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>) {
        if left.len() > CHUNK {
            let mut at = 0;
            while at < left.len() {
                let end = (at + CHUNK).min(left.len());
                let r = right.as_deref_mut().and_then(|r| r.get_mut(at..end.min(r.len())));
                self.process(&mut left[at..end], r);
                at = end;
            }
            return;
        }
        // Noise removal, switched with a crossfade.
        let (denoise, want) = (&mut self.denoise, self.noise_want);
        self.noise_sw = self.noise_sw.next(want > 0, || {
            denoise.set_level(want);
            denoise.latency()
        });
        if want > 0 && self.noise_sw != Switch::Off {
            self.denoise.set_level(want);
        } else if self.noise_sw == Switch::Off && self.denoise.level() > 0 {
            // Turned off while still warming up: never heard, just stopped.
            self.denoise.set_level(0);
        }
        if self.noise_sw != Switch::Off {
            let mut sw = self.noise_sw;
            Self::switched(&mut sw, self.switch_step, &mut self.dry, left, right.as_deref_mut(), |l, r| self.denoise.process(l, r));
            self.noise_sw = sw;
            if sw == Switch::Off {
                self.denoise.set_level(0);
            }
        }
        // Auto-Tune, the same way.
        let tune = &mut self.tune;
        self.tune_sw = self.tune_sw.next(self.tune_want, || {
            tune.reset();
            tune.delay_samples().ceil() as usize + 2
        });
        if self.tune_sw != Switch::Off {
            let mut sw = self.tune_sw;
            Self::switched(&mut sw, self.switch_step, &mut self.dry, left, right.as_deref_mut(), |l, r| self.tune.process(l, r));
            self.tune_sw = sw;
        }
        self.deess.process(left, right.as_deref_mut());
        self.dynamics.process(left, right.as_deref_mut());
        if self.cough || self.open < 1.0 {
            for (i, a) in left.iter_mut().enumerate() {
                self.open = if self.cough { (self.open - self.fade_out).max(0.0) } else { (self.open + self.fade_in).min(1.0) };
                *a *= self.open;
                if let Some(b) = right.as_deref_mut().and_then(|r| r.get_mut(i)) {
                    *b *= self.open;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The processed mic's whole chain against the clock (a second look:
    /// six stages in a 5.3 ms block, never timed): noise removal (two
    /// nets), Auto-Tune, the de-esser and the dynamics, on a stereo voice,
    /// in the loop's 256-sample blocks.
    #[test]
    fn the_whole_chain_keeps_up_with_time() {
        let sr = 48_000.0f32;
        let seconds = 10.0f32;
        let n = (sr * seconds) as usize;
        let voice: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / sr;
                let f0 = 140.0 + 15.0 * (t * 2.0).sin();
                (1..=12).map(|k| (std::f32::consts::TAU * f0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.15
            })
            .collect();
        let (mut l, mut r) = (voice.clone(), voice);
        let mut chain = VoiceChain::new(sr, DynamicsParams { auto_on: true, ..DynamicsParams::default() });
        chain.set_noise(2);
        chain.set_tune(true);
        chain.set_deess(2);
        let mut worst = std::time::Duration::ZERO;
        let started = std::time::Instant::now();
        for (bl, br) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            let t = std::time::Instant::now();
            chain.process(bl, Some(br));
            worst = worst.max(t.elapsed());
        }
        let total = started.elapsed();
        let share = total.as_secs_f32() / seconds;
        eprintln!("the chain: {:.1}% of one core, worst block {:?} of 5.33 ms", share * 100.0, worst);
        assert!(share < 0.25, "the chain takes {:.0}% of real time", share * 100.0);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
    }

    /// A steady voice-like tone with each delaying stage switched in and
    /// out: never a gap, never a step bigger than the tone itself makes.
    #[test]
    fn switching_noise_removal_or_auto_tune_never_gaps_or_clicks() {
        let sr = 48_000.0f32;
        let tone: Vec<f32> = (0..(3.0 * sr) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                (1..=6).map(|k| (std::f32::consts::TAU * 147.0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.15
            })
            .collect();
        let own_step = tone.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        for stage in ["noise", "tune"] {
            let mut chain = VoiceChain::new(sr, crate::dynamics::natural());
            let mut y = tone.clone();
            for (b, block) in y.chunks_mut(256).enumerate() {
                let on = matches!(b, 40..=120 | 200..=260 | 300..=301);
                match stage {
                    "noise" => chain.set_noise(if on { 2 } else { 0 }),
                    _ => chain.set_tune(on),
                }
                chain.process(block, None);
            }
            let step = y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
            assert!(step < 2.0 * own_step, "{stage}: a step of {step:.3} (the tone's own {own_step:.3})");
            // No 2 ms of near silence anywhere after the start.
            let quiet = y[4_800..].windows(96).any(|w| w.iter().all(|v| v.abs() < 1e-3));
            assert!(!quiet, "{stage}: a gap");
            assert!(y.iter().all(|v| v.is_finite()));
        }
    }

    /// A take for the level tests: a voice-like tone (a fundamental that
    /// moves a little, and its harmonics), the room under it (a fan: noise
    /// low-passed at 300 Hz, with a 120 Hz hum), and an "s" (a hiss) from
    /// the middle on.
    fn take(seconds: f32, sr: f32, voice: f32, room: f32, s: f32) -> Vec<f32> {
        let mut seed = 0x51ed_270bu32;
        let mut lp = 0.0f32;
        let a = (-std::f32::consts::TAU * 300.0 / sr).exp();
        (0..(seconds * sr) as usize)
            .map(|i| {
                let t = i as f32 / sr;
                let f0 = 140.0 + 15.0 * (t * 2.0).sin();
                let v: f32 = (1..=12).map(|k| (std::f32::consts::TAU * f0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.15 * voice;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let w = seed as f32 / u32::MAX as f32 - 0.5;
                lp = w + (lp - w) * a;
                let n = (lp * 8.0 + 0.3 * (std::f32::consts::TAU * 120.0 * t).sin()) * room;
                let hiss = if t > seconds / 2.0 { w * s } else { 0.0 };
                v + n + hiss
            })
            .collect()
    }

    /// The largest step between two samples of `y` in `from..to`.
    fn worst_step(y: &[f32], from: usize, to: usize) -> f32 {
        y[from..to].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
    }

    /// Changing a level mid-take — the noise removal from Light to
    /// Strong, the de-esser from Light to Strong — must not step the
    /// signal: the largest step around the change may not be much bigger
    /// than the same take's own (job 4, 2026-11-11).
    #[test]
    fn changing_a_level_mid_word_doesnt_click() {
        let sr = 48_000.0f32;
        let block = 256usize;
        let at = 96; // the block the level changes in, in the "s"
        for (name, take) in [
            ("the voice alone", take(1.0, sr, 1.0, 0.0, 0.0)),
            ("the voice over the room", take(1.0, sr, 1.0, 0.02, 0.0)),
            ("the room alone", take(1.0, sr, 0.0, 0.02, 0.0)),
            ("a voice with an s", take(1.0, sr, 1.0, 0.0, 0.05)),
        ] {
            for stage in ["noise", "deess"] {
                // The same take twice: once the level changes, once it does not.
                let (mut changed, mut same) = (take.clone(), take.clone());
                let mut chain = VoiceChain::new(sr, crate::dynamics::natural());
                let mut steady = VoiceChain::new(sr, crate::dynamics::natural());
                for (b, (c, s)) in changed.chunks_mut(block).zip(same.chunks_mut(block)).enumerate() {
                    match stage {
                        "noise" => {
                            chain.set_noise(if b < at { 1 } else { 2 });
                            steady.set_noise(1);
                        }
                        _ => {
                            chain.set_deess(if b < at { 1 } else { 2 });
                            steady.set_deess(1);
                        }
                    }
                    chain.process(c, None);
                    steady.process(s, None);
                }
                // The noise removal works in 10 ms frames, so its change
                // lands in the frame after it; the window covers the change
                // and that frame.
                let from = at * block;
                let to = (from + 1024).min(changed.len());
                let (around, own) = (worst_step(&changed, from, to), worst_step(&same, from, to));
                eprintln!("{name}, {stage}: a step of {around:.5} at the change, {own:.5} in the same take without it");
                assert!(around <= 1.5 * own, "{name}, {stage}: a step of {around:.5} against {own:.5}");
            }
        }
    }

    #[test]
    fn cough_fades_out_and_back_on_both_sides() {
        let sr = 48_000.0f32;
        let mut chain = VoiceChain::new(sr, crate::dynamics::natural());
        let mut l = vec![0.25f32; 2048];
        let mut r = vec![0.25f32; 2048];
        chain.set_cough(true);
        chain.process(&mut l, Some(&mut r));
        // Silent within 8 ms (384 samples), never a jump to it.
        assert!(l[400..].iter().chain(&r[400..]).all(|v| v.abs() < 1e-6));
        assert!(l.windows(2).all(|w| (w[0] - w[1]).abs() < 0.25 / 300.0));
        chain.set_cough(false);
        let (mut l, mut r) = (vec![0.25f32; 2048], vec![0.25f32; 2048]);
        chain.process(&mut l, Some(&mut r));
        assert!((l[2000] - 0.25).abs() < 1e-3 && (r[2000] - 0.25).abs() < 1e-3);
        assert!(l.windows(2).all(|w| (w[0] - w[1]).abs() < 0.25 / 600.0));
    }
}
