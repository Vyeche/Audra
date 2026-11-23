//! Echo: two delay lines with feedback, in three ways (BF §21.6):
//! independent left and right echoes, left and right with their feedback
//! crossed, or ping-pong (the send bounces from side to side).
//!
//! The feedback never reaches unity: at its top (100) each repeat is
//! still ~1 dB down and High Cut darkens it further, so an echo can ring
//! long but never climb.

use crate::filters::{Delay, LowPass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EchoKind {
    /// Left echoes left, right echoes right.
    Stereo,
    /// Each side's repeats feed the other.
    Cross,
    /// The send starts on the left and bounces between the sides.
    Pong,
}

impl std::fmt::Display for EchoKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl EchoKind {
    pub const ALL: [EchoKind; 3] = [EchoKind::Stereo, EchoKind::Cross, EchoKind::Pong];

    pub fn name(self) -> &'static str {
        match self {
            EchoKind::Stereo => "Stereo",
            EchoKind::Cross => "Cross",
            EchoKind::Pong => "Ping-pong",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(default))]
pub struct EchoParams {
    pub kind: EchoKind,
    /// 1–2000 ms.
    pub delay_ms: f32,
    /// 0–100: how much of each repeat comes round again.
    pub feedback: f32,
    /// 100 stereo … 0 mono.
    pub width: f32,
    /// The wet level into the FX return, dB (−65 is off).
    pub volume_db: f32,
    /// Darkens each repeat, 2–20 kHz.
    pub high_cut_hz: f32,
}

impl Default for EchoParams {
    fn default() -> Self {
        Self { kind: EchoKind::Stereo, delay_ms: 375.0, feedback: 35.0, width: 100.0, volume_db: -6.0, high_cut_hz: 8_000.0 }
    }
}

/// The feedback gain at Feedback 100: just under unity, so the echo can
/// never self-oscillate.
pub const FEEDBACK_MAX: f32 = 0.89;

impl EchoParams {
    pub fn clamped(mut self) -> Self {
        self.delay_ms = self.delay_ms.clamp(1.0, 2000.0);
        self.feedback = self.feedback.clamp(0.0, 100.0);
        self.width = self.width.clamp(0.0, 100.0);
        self.volume_db = self.volume_db.clamp(-65.0, 6.0);
        self.high_cut_hz = self.high_cut_hz.clamp(2_000.0, 20_000.0);
        self
    }

    /// The delay from a tempo: one beat at `bpm` (the manual's BPM entry).
    pub fn from_bpm(self, bpm: f32) -> Self {
        Self { delay_ms: 60_000.0 / bpm.clamp(30.0, 300.0), ..self }
    }
}

pub struct Echo {
    params: EchoParams,
    sample_rate: f32,
    lines: [Delay; 2],
    tone: [LowPass; 2],
    delay: usize,
    feedback: f32,
    /// The wet level, fading to a new one over 50 ms (job 42).
    volume: crate::gain_ramp::GainRamp,
}

impl Echo {
    pub fn new(sample_rate: f32, params: EchoParams) -> Self {
        let max = (2.0 * sample_rate).ceil() as usize + 1;
        let mut e = Self {
            params,
            sample_rate,
            lines: [Delay::new(max), Delay::new(max)],
            tone: [LowPass::new(8_000.0, sample_rate), LowPass::new(8_000.0, sample_rate)],
            delay: 1,
            feedback: 0.0,
            volume: crate::gain_ramp::GainRamp::new(crate::db_to_gain(params.clamped().volume_db), crate::gain_ramp::GainRamp::length_for(sample_rate, 50.0)),
        };
        e.set_params(params);
        e
    }

    pub fn params(&self) -> EchoParams {
        self.params
    }

    pub fn set_params(&mut self, params: EchoParams) {
        let p = params.clamped();
        self.params = p;
        self.delay = ((p.delay_ms / 1000.0) * self.sample_rate).round().max(1.0) as usize;
        self.feedback = FEEDBACK_MAX * p.feedback / 100.0;
        for t in &mut self.tone {
            t.set(p.high_cut_hz, self.sample_rate);
        }
        self.volume.set(crate::db_to_gain(p.volume_db));
    }

    pub fn reset(&mut self) {
        self.lines.iter_mut().for_each(Delay::clear);
        self.tone.iter_mut().for_each(LowPass::clear);
    }

    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let ol = self.tone[0].process(self.lines[0].read(self.delay));
        let or = self.tone[1].process(self.lines[1].read(self.delay));
        let fb = self.feedback;
        let (in_l, in_r) = match self.params.kind {
            EchoKind::Stereo => (l + fb * ol, r + fb * or),
            EchoKind::Cross => (l + fb * or, r + fb * ol),
            // The send enters on the left; each side feeds the other.
            EchoKind::Pong => (0.5 * (l + r) + fb * or, fb * ol),
        };
        self.lines[0].write(in_l);
        self.lines[1].write(in_r);
        let w = self.params.width / 100.0;
        let (m, s) = (0.5 * (ol + or), 0.5 * (ol - or) * w);
        let v = self.volume.advance();
        ((m + s) * v, (m - s) * v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn run(params: EchoParams, seconds: f32) -> Vec<(f32, f32)> {
        let mut e = Echo::new(SR, EchoParams { volume_db: 0.0, ..params });
        (0..(seconds * SR) as usize)
            .map(|i| {
                let x = if i == 0 { 1.0 } else { 0.0 };
                e.process(x, x)
            })
            .collect()
    }

    #[test]
    fn repeats_land_on_the_delay_and_fall() {
        let out = run(EchoParams { delay_ms: 100.0, feedback: 50.0, high_cut_hz: 20_000.0, ..EchoParams::default() }, 0.5);
        let d = 4800;
        // The first repeat at 100 ms, quieter ones after it.
        let first = out[d - 2..d + 40].iter().map(|s| s.0.abs()).fold(0.0, f32::max);
        let second = out[2 * d - 2..2 * d + 40].iter().map(|s| s.0.abs()).fold(0.0, f32::max);
        assert!(first > 0.3, "first repeat {first}");
        assert!(second < first && second > 0.05, "{second} after {first}");
        assert!(out[..d - 2].iter().all(|s| s.0.abs() < 1e-6), "nothing before the delay");
    }

    #[test]
    fn ping_pong_starts_left_and_answers_right() {
        let out = run(EchoParams { kind: EchoKind::Pong, delay_ms: 100.0, feedback: 60.0, high_cut_hz: 20_000.0, ..EchoParams::default() }, 0.5);
        let d = 4800;
        let peak = |from: usize, side: fn(&(f32, f32)) -> f32| out[from - 2..from + 40].iter().map(side).map(f32::abs).fold(0.0, f32::max);
        assert!(peak(d, |s| s.0) > 10.0 * peak(d, |s| s.1), "first bounce on the left");
        assert!(peak(2 * d, |s| s.1) > 10.0 * peak(2 * d, |s| s.0), "then the right");
    }

    #[test]
    fn full_feedback_never_climbs() {
        let out = run(EchoParams { delay_ms: 10.0, feedback: 100.0, high_cut_hz: 20_000.0, kind: EchoKind::Cross, ..EchoParams::default() }, 4.0);
        let early = out[..48_000].iter().map(|s| s.0.abs()).fold(0.0, f32::max);
        let late = out[out.len() - 48_000..].iter().map(|s| s.0.abs()).fold(0.0, f32::max);
        assert!(late < early * 0.5, "it rings down, never up: {early} then {late}");
    }

    #[test]
    fn a_new_level_fades_in_rather_than_steps() {
        // OpenCode's job 42: a preset load put the wet level from off to
        // +6 dB in one block. With a steady signal ringing in the lines, the
        // output now rises over 50 ms (2400 samples), sample by sample.
        let mut e = Echo::new(SR, EchoParams { delay_ms: 10.0, feedback: 0.0, high_cut_hz: 20_000.0, volume_db: -65.0, ..EchoParams::default() });
        for _ in 0..4800 {
            e.process(0.5, 0.5);
        }
        e.set_params(EchoParams { volume_db: 6.0, ..e.params() });
        let outs: Vec<f32> = (0..3000).map(|_| e.process(0.5, 0.5).0.abs()).collect();
        assert!(outs[0] < 0.01, "first sample barely up: {}", outs[0]);
        let full = outs[2999];
        assert!(full > 0.9, "there by the end: {full}");
        let biggest = outs.windows(2).map(|w| w[1] - w[0]).fold(0.0, f32::max);
        assert!(biggest < full / 2000.0, "no step: the biggest rise per sample is {biggest}");
    }

    #[test]
    fn a_tempo_sets_one_beat() {
        let p = EchoParams::default().from_bpm(120.0);
        assert!((p.delay_ms - 500.0).abs() < 1e-3);
    }
}
