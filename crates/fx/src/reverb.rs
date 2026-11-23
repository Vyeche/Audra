//! Reverb: an eight-line feedback delay network.
//!
//! The send is low-cut, held back by the pre-delay and smeared by four
//! allpasses (Smooth), then fed into eight delay lines of mutually prime
//! lengths whose outputs are mixed back through a Householder matrix (it
//! keeps the energy, so the decay is set by the line gains alone). Each
//! line loses `60 dB` over the reverb time and is damped above High Damp in
//! the loop, so the tail darkens as it decays, as a room's does. Odd lines
//! make the left, even the right; Width narrows them toward the middle;
//! High Cut and Volume close it. The Gated and Envelope types shape the
//! tail with an envelope that follows the send (Attack, Hold, Release).

use crate::filters::{AllPass, Delay, HighPass, LowPass};

/// The types, Audra's own (`docs/ui-spec.md` §7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ReverbKind {
    SmallRoom,
    MediumRoom,
    LargeRoom,
    /// Hard reflective walls: bright, dense early sound.
    Walls,
    /// Short and warm, the highs gone soon.
    Warm,
    /// One clear slap off a far wall.
    Slap,
    /// A wide, open space.
    Space,
    /// A long bright tail, its time and High Damp set by hand.
    Long,
    /// A tail cut off by a gate once the send stops (Attack, Hold, Release).
    Gated,
    /// A tail that swells and fades with the send (Attack, Hold, Release).
    Envelope,
}

impl std::fmt::Display for ReverbKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl ReverbKind {
    pub const ALL: [ReverbKind; 10] = [
        ReverbKind::SmallRoom,
        ReverbKind::MediumRoom,
        ReverbKind::LargeRoom,
        ReverbKind::Walls,
        ReverbKind::Warm,
        ReverbKind::Slap,
        ReverbKind::Space,
        ReverbKind::Long,
        ReverbKind::Gated,
        ReverbKind::Envelope,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ReverbKind::SmallRoom => "Small room",
            ReverbKind::MediumRoom => "Medium room",
            ReverbKind::LargeRoom => "Large room",
            ReverbKind::Walls => "Walls",
            ReverbKind::Warm => "Warm",
            ReverbKind::Slap => "Slap",
            ReverbKind::Space => "Space",
            ReverbKind::Long => "Long and bright",
            ReverbKind::Gated => "Gated",
            ReverbKind::Envelope => "Envelope",
        }
    }

    /// (reverb time in seconds at Room Scale 1, line-length scale, high
    /// damp in Hz) for the types that set them.
    fn shape(self) -> (f32, f32, f32) {
        match self {
            ReverbKind::SmallRoom => (0.45, 0.6, 9_000.0),
            ReverbKind::MediumRoom => (0.8, 1.0, 8_000.0),
            ReverbKind::LargeRoom => (1.4, 1.6, 7_000.0),
            ReverbKind::Walls => (0.7, 1.1, 14_000.0),
            ReverbKind::Warm => (0.7, 0.9, 4_500.0),
            ReverbKind::Slap => (0.35, 2.2, 9_000.0),
            ReverbKind::Space => (2.6, 2.4, 9_000.0),
            ReverbKind::Long => (2.0, 1.8, 12_000.0),
            ReverbKind::Gated => (1.5, 1.3, 9_000.0),
            ReverbKind::Envelope => (1.8, 1.4, 9_000.0),
        }
    }

    fn enveloped(self) -> bool {
        matches!(self, ReverbKind::Gated | ReverbKind::Envelope)
    }
}

/// The reverb's settings, in the manual's units and ranges (BF §21.6).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(default))]
pub struct ReverbParams {
    pub kind: ReverbKind,
    /// 0–999 ms.
    pub pre_delay_ms: f32,
    /// Before the reverb, 20–500 Hz.
    pub low_cut_hz: f32,
    /// After the reverb, 5–20 kHz.
    pub high_cut_hz: f32,
    /// Input diffusion, 0–100.
    pub smooth: f32,
    /// 100 stereo … 0 mono.
    pub width: f32,
    /// The wet level into the FX return, dB (−65 is off).
    pub volume_db: f32,
    /// The room types' size, 0.5–3.0.
    pub room_scale: f32,
    /// Long's reverb time, 0.1–4.9 s.
    pub time_s: f32,
    /// Long's damping, 5–20 kHz.
    pub high_damp_hz: f32,
    /// Gated and Envelope, 5–400 ms each.
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            kind: ReverbKind::MediumRoom,
            pre_delay_ms: 20.0,
            low_cut_hz: 100.0,
            high_cut_hz: 12_000.0,
            smooth: 60.0,
            width: 100.0,
            volume_db: 0.0,
            room_scale: 1.0,
            time_s: 2.0,
            high_damp_hz: 12_000.0,
            attack_ms: 10.0,
            hold_ms: 150.0,
            release_ms: 200.0,
        }
    }
}

impl ReverbParams {
    /// Every value brought into the manual's range.
    pub fn clamped(mut self) -> Self {
        self.pre_delay_ms = self.pre_delay_ms.clamp(0.0, 999.0);
        self.low_cut_hz = self.low_cut_hz.clamp(20.0, 500.0);
        self.high_cut_hz = self.high_cut_hz.clamp(5_000.0, 20_000.0);
        self.smooth = self.smooth.clamp(0.0, 100.0);
        self.width = self.width.clamp(0.0, 100.0);
        self.volume_db = self.volume_db.clamp(-65.0, 6.0);
        self.room_scale = self.room_scale.clamp(0.5, 3.0);
        self.time_s = self.time_s.clamp(0.1, 4.9);
        self.high_damp_hz = self.high_damp_hz.clamp(5_000.0, 20_000.0);
        self.attack_ms = self.attack_ms.clamp(5.0, 400.0);
        self.hold_ms = self.hold_ms.clamp(5.0, 400.0);
        self.release_ms = self.release_ms.clamp(5.0, 400.0);
        self
    }

    /// The reverb time the network decays over, in seconds.
    pub fn decay_s(&self) -> f32 {
        let (rt, _, _) = self.kind.shape();
        match self.kind {
            ReverbKind::Long => self.time_s,
            _ => (rt * self.room_scale).clamp(0.1, 8.0),
        }
    }
}

/// The eight lines' lengths at scale 1, in ms: mutually prime in samples
/// at 48 kHz, spread so their echoes never line up.
const LINE_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 47.3, 53.9, 59.3, 67.1];
/// The longest a line can grow (scale 3 × the longest × the largest type).
const MAX_LINE_MS: f32 = 67.1 * 3.0 * 2.4 + 1.0;
/// The input allpasses, ms.
const DIFFUSER_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.31];

pub struct Reverb {
    params: ReverbParams,
    sample_rate: f32,
    low_cut: [HighPass; 2],
    pre_delay: [Delay; 2],
    diffusers: [AllPass; 4],
    lines: [Delay; 8],
    lengths: [usize; 8],
    gains: [f32; 8],
    damp: [LowPass; 8],
    high_cut: [LowPass; 2],
    /// The wet level, fading to a new one over 50 ms (OpenCode's job 42: a
    /// preset load stepped it by up to 71 dB in one block).
    volume: crate::gain_ramp::GainRamp,
    /// The Gated/Envelope follower: the send's level, its hold count, and
    /// the gain it gives the tail.
    env_level: f32,
    env_hold: usize,
    env_gain: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32, params: ReverbParams) -> Self {
        let ms = |m: f32| ((m / 1000.0) * sample_rate).ceil() as usize;
        let mut r = Self {
            params,
            sample_rate,
            low_cut: [HighPass::new(100.0, sample_rate), HighPass::new(100.0, sample_rate)],
            pre_delay: [Delay::new(ms(1000.0)), Delay::new(ms(1000.0))],
            diffusers: DIFFUSER_MS.map(|d| AllPass::new(ms(d), 0.5)),
            lines: [0; 8].map(|_| Delay::new(ms(MAX_LINE_MS))),
            lengths: [1; 8],
            gains: [0.0; 8],
            damp: [0; 8].map(|_| LowPass::new(8_000.0, sample_rate)),
            high_cut: [LowPass::new(12_000.0, sample_rate), LowPass::new(12_000.0, sample_rate)],
            // At its level from the start: no fade in.
            volume: crate::gain_ramp::GainRamp::new(crate::db_to_gain(params.clamped().volume_db), crate::gain_ramp::GainRamp::length_for(sample_rate, 50.0)),
            env_level: 0.0,
            env_hold: 0,
            env_gain: 0.0,
        };
        r.set_params(params);
        r
    }

    pub fn params(&self) -> ReverbParams {
        self.params
    }

    /// Takes new settings. The lines keep what they hold, so a change is
    /// heard on the tail already ringing, without a click of silence.
    #[allow(clippy::needless_range_loop)]
    pub fn set_params(&mut self, params: ReverbParams) {
        let p = params.clamped();
        self.params = p;
        let sr = self.sample_rate;
        for f in &mut self.low_cut {
            f.set(p.low_cut_hz, sr);
        }
        for f in &mut self.high_cut {
            f.set(p.high_cut_hz, sr);
        }
        let (_, size, damp) = p.kind.shape();
        let damp = if p.kind == ReverbKind::Long { p.high_damp_hz } else { damp };
        let scale = size * if p.kind == ReverbKind::Long { 1.0 } else { p.room_scale };
        let decay = p.decay_s();
        for i in 0..8 {
            let len = ((LINE_MS[i] * scale / 1000.0) * sr).round().max(2.0) as usize;
            self.lengths[i] = len.min(self.lines[i].capacity());
            // −60 dB over the reverb time, this line's share of it.
            self.gains[i] = 10f32.powf(-3.0 * self.lengths[i] as f32 / (decay * sr));
            self.damp[i].set(damp, sr);
        }
        let g = 0.25 + 0.45 * p.smooth / 100.0;
        for d in &mut self.diffusers {
            d.gain = g;
        }
        self.volume.set(crate::db_to_gain(p.volume_db));
    }

    pub fn reset(&mut self) {
        self.low_cut.iter_mut().for_each(HighPass::clear);
        self.pre_delay.iter_mut().for_each(Delay::clear);
        self.diffusers.iter_mut().for_each(AllPass::clear);
        self.lines.iter_mut().for_each(Delay::clear);
        self.damp.iter_mut().for_each(LowPass::clear);
        self.high_cut.iter_mut().for_each(LowPass::clear);
        self.env_level = 0.0;
        self.env_hold = 0;
        self.env_gain = 0.0;
    }

    /// One stereo frame of send in, one of wet out.
    #[allow(clippy::needless_range_loop)]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let p = self.params;
        let sr = self.sample_rate;
        let pre = ((p.pre_delay_ms / 1000.0) * sr).round() as usize;
        let cut = [self.low_cut[0].process(l), self.low_cut[1].process(r)];
        let mut ins = [0.0; 2];
        for c in 0..2 {
            self.pre_delay[c].write(cut[c]);
            ins[c] = if pre == 0 { cut[c] } else { self.pre_delay[c].read(pre) };
        }
        // One diffused feed, a little of each side.
        let mut x = 0.5 * (ins[0] + ins[1]);
        for d in &mut self.diffusers {
            x = d.process(x);
        }
        // The lines, read, damped, mixed by Householder (each output less
        // a quarter of their sum), fed back with their gains plus the send
        // in alternating signs.
        let mut outs = [0.0f32; 8];
        for i in 0..8 {
            outs[i] = self.damp[i].process(self.lines[i].read(self.lengths[i]));
        }
        let sum: f32 = outs.iter().sum();
        for i in 0..8 {
            let mixed = outs[i] - 0.25 * sum;
            let feed = if i % 2 == 0 { x } else { -x };
            self.lines[i].write(self.gains[i] * mixed + feed * 0.35);
        }
        let mut wl = 0.0;
        let mut wr = 0.0;
        for i in 0..8 {
            let s = if (i / 2) % 2 == 0 { outs[i] } else { -outs[i] };
            if i % 2 == 0 {
                wl += s;
            } else {
                wr += s;
            }
        }
        wl *= 0.35;
        wr *= 0.35;
        // Width toward the middle.
        let w = p.width / 100.0;
        let (m, s) = (0.5 * (wl + wr), 0.5 * (wl - wr) * w);
        let (mut ol, mut or) = (m + s, m - s);
        ol = self.high_cut[0].process(ol);
        or = self.high_cut[1].process(or);
        let env = if p.kind.enveloped() { self.envelope(l, r) } else { 1.0 };
        let v = self.volume.advance();
        (ol * v * env, or * v * env)
    }

    /// The Gated/Envelope gain: follows the send's level, opens over
    /// Attack, stays for Hold after the send falls, closes over Release.
    fn envelope(&mut self, l: f32, r: f32) -> f32 {
        let p = self.params;
        let sr = self.sample_rate;
        let level = l.abs().max(r.abs());
        self.env_level = level.max(self.env_level * (-1.0 / (0.02 * sr)).exp());
        let open = self.env_level > 0.01;
        let step = |ms: f32| 1.0 / ((ms / 1000.0) * sr).max(1.0);
        if open {
            self.env_hold = ((p.hold_ms / 1000.0) * sr) as usize;
            self.env_gain = (self.env_gain + step(p.attack_ms)).min(1.0);
        } else if self.env_hold > 0 {
            self.env_hold -= 1;
        } else {
            let release = if p.kind == ReverbKind::Gated { step(p.release_ms) * 4.0 } else { step(p.release_ms) };
            self.env_gain = (self.env_gain - release).max(0.0);
        }
        self.env_gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// The output's energy in 10 ms blocks after an impulse.
    fn impulse_energy(params: ReverbParams, seconds: f32) -> Vec<f32> {
        let mut r = Reverb::new(SR, params);
        let n = (seconds * SR) as usize;
        let mut blocks = Vec::new();
        let mut acc = 0.0;
        for i in 0..n {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let (a, b) = r.process(x, x);
            acc += a * a + b * b;
            if (i + 1) % 480 == 0 {
                blocks.push(acc);
                acc = 0.0;
            }
        }
        blocks
    }

    #[test]
    fn the_pre_delay_holds_the_first_sound_back() {
        let params = ReverbParams { pre_delay_ms: 100.0, ..ReverbParams::default() };
        let mut r = Reverb::new(SR, params);
        let mut first = None;
        for i in 0..(SR as usize) {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let (a, b) = r.process(x, x);
            if first.is_none() && (a.abs() > 1e-6 || b.abs() > 1e-6) {
                first = Some(i);
            }
        }
        let first = first.expect("it sounds");
        assert!(first >= (0.1 * SR) as usize, "nothing before 100 ms: {first}");
    }

    #[test]
    fn a_longer_time_rings_longer() {
        let short = impulse_energy(ReverbParams { kind: ReverbKind::Long, time_s: 0.5, ..ReverbParams::default() }, 3.0);
        let long = impulse_energy(ReverbParams { kind: ReverbKind::Long, time_s: 3.0, ..ReverbParams::default() }, 3.0);
        // At 1.5 s the short one is gone and the long one still rings.
        let at = 150;
        assert!(long[at] > short[at] * 1000.0, "long {} vs short {}", long[at], short[at]);
    }

    #[test]
    fn the_decay_follows_the_reverb_time() {
        // −60 dB over 1 s: about −30 dB (1/1000 in energy) at 0.5 s.
        let e = impulse_energy(ReverbParams { kind: ReverbKind::Long, time_s: 1.0, pre_delay_ms: 0.0, ..ReverbParams::default() }, 1.0);
        let early: f32 = e[10..20].iter().sum();
        let late: f32 = e[60..70].iter().sum();
        let drop_db = 10.0 * (late / early).log10();
        // 0.5 s apart: about −30 dB, within a broad margin (damping adds).
        assert!(drop_db < -20.0 && drop_db > -60.0, "{drop_db} dB");
    }

    #[test]
    fn width_zero_is_mono() {
        let mut r = Reverb::new(SR, ReverbParams { width: 0.0, ..ReverbParams::default() });
        for i in 0..4800 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let (a, b) = r.process(x, 0.0);
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn every_type_stays_bounded_at_the_extremes() {
        for kind in ReverbKind::ALL {
            let params = ReverbParams { kind, room_scale: 3.0, time_s: 4.9, smooth: 100.0, volume_db: 6.0, ..ReverbParams::default() };
            let mut r = Reverb::new(SR, params);
            let mut peak = 0.0f32;
            // Five seconds of loud noise-like input.
            let mut seed = 1u32;
            for _ in 0..(5.0 * SR) as usize {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let x = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                let (a, b) = r.process(x, -x);
                assert!(a.is_finite() && b.is_finite(), "{kind:?}");
                peak = peak.max(a.abs()).max(b.abs());
            }
            assert!(peak < 20.0, "{kind:?} ran away: {peak}");
        }
    }

    #[test]
    fn the_gate_closes_after_the_send_stops() {
        let params = ReverbParams { kind: ReverbKind::Gated, hold_ms: 50.0, release_ms: 50.0, ..ReverbParams::default() };
        let mut r = Reverb::new(SR, params);
        // Half a second of signal, then silence.
        for i in 0..(0.5 * SR) as usize {
            let x = if i % 100 == 0 { 0.5 } else { 0.0 };
            r.process(x, x);
        }
        let mut late = 0.0f32;
        for i in 0..(0.5 * SR) as usize {
            let (a, b) = r.process(0.0, 0.0);
            if i > (0.2 * SR) as usize {
                late = late.max(a.abs()).max(b.abs());
            }
        }
        assert!(late < 1e-4, "gated shut: {late}");
    }
}
