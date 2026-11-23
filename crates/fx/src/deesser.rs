//! Harsh S sounds tamed, for what others hear (the owner's pick from
//! ChatGPT's list, 2026-11-10: a de-esser, "particularly useful with
//! close-miked condenser microphones" like the owner's TLM 107).
//!
//! Split-band, as a second opinion advised (checked): the voice is split
//! at `SPLIT_HZ` into a low part and the rest (the rest is the sound less
//! the low part, so with nothing turned down the two add back to exactly
//! the sound), and only the top part is turned down, only while an S pushes
//! it close to the whole voice's loudness: a voiced sound keeps its top
//! 15–25 dB under the whole, an S has nearly all its energy there. Light
//! turns it down 3 dB at most, Strong 6; it reacts in 1.5 ms and lets go
//! in 60, so it neither lisps nor dulls the words after an S. No delay
//! (IIR filters only), no allocation while processing.

/// Where the voice is split, Hz (the S band of most voices starts 4–7
/// kHz).
pub const SPLIT_HZ: f32 = 5_500.0;
/// The most Light and Strong turn the top down, dB.
pub const LIGHT_DB: f32 = 3.0;
pub const STRONG_DB: f32 = 6.0;
/// The top within this many dB of the whole counts as an S.
const S_WITHIN_DB: f32 = 9.0;
/// Under this level (dBFS) nothing is an S: breath and room stay alone.
const QUIET_DB: f32 = -50.0;
const ATTACK_MS: f32 = 1.5;
const RELEASE_MS: f32 = 60.0;

fn coeff(ms: f32, rate: f32) -> f32 {
    (-1.0 / (ms * 0.001 * rate).max(1.0)).exp()
}

fn to_db(v: f32) -> f32 {
    if v > 1e-7 {
        20.0 * v.log10()
    } else {
        -140.0
    }
}

/// A second-order low-pass (RBJ, Q 0.707): the low part of the split.
#[derive(Clone, Copy, Default)]
struct LowPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl LowPass {
    fn new(hz: f32, rate: f32) -> Self {
        let w = std::f32::consts::TAU * (hz / rate).min(0.49);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - c) / 2.0 / a0,
            b1: (1.0 - c) / a0,
            b2: (1.0 - c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn step(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// One channel's (or one linked pair's) de-esser.
pub struct DeEsser {
    level: u8,
    lp: [LowPass; 2],
    env_top: f32,
    env_all: f32,
    gain_db: f32,
    att: f32,
    rel: f32,
}

impl DeEsser {
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let lp = LowPass::new(SPLIT_HZ, rate);
        Self { level: 0, lp: [lp, lp], env_top: 0.0, env_all: 0.0, gain_db: 0.0, att: coeff(ATTACK_MS, rate), rel: coeff(RELEASE_MS, rate) }
    }

    /// 0 off, 1 light, 2 strong.
    pub fn set_level(&mut self, level: u8) {
        self.level = level.min(2);
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// How far the top is turned down now, dB (0 or less).
    pub fn reduction_db(&self) -> f32 {
        self.gain_db
    }

    fn reset(&mut self) {
        for f in &mut self.lp {
            f.z1 = 0.0;
            f.z2 = 0.0;
        }
        self.env_top = 0.0;
        self.env_all = 0.0;
        self.gain_db = 0.0;
    }

    /// Processes a block in place: `left`, and `right` for a linked pair
    /// (one detector, one turn-down for both).
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>) {
        if self.level == 0 {
            return;
        }
        let most = if self.level == 1 { LIGHT_DB } else { STRONG_DB };
        for (i, slot) in left.iter_mut().enumerate() {
            let l = *slot;
            let r = right.as_ref().and_then(|r| r.get(i)).copied();
            if !l.is_finite() || r.is_some_and(|r| !r.is_finite()) {
                self.reset();
                *slot = 0.0;
                if let Some(r) = right.as_mut().and_then(|r| r.get_mut(i)) {
                    *r = 0.0;
                }
                continue;
            }
            let low_l = self.lp[0].step(l);
            let top_l = l - low_l;
            let (low_r, top_r) = match r {
                Some(r) => {
                    let low = self.lp[1].step(r);
                    (low, r - low)
                }
                None => (0.0, 0.0),
            };
            // The detector: the top's level against the whole's.
            let top = top_l.abs().max(top_r.abs());
            let all = l.abs().max(r.map_or(0.0, f32::abs));
            let follow = |env: &mut f32, x: f32, att: f32, rel: f32| {
                let c = if x > *env { att } else { rel };
                *env = x + (*env - x) * c;
            };
            follow(&mut self.env_top, top, self.att, self.rel);
            follow(&mut self.env_all, all, self.att, self.rel);
            let (top_db, all_db) = (to_db(self.env_top), to_db(self.env_all));
            let over = top_db - all_db + S_WITHIN_DB;
            let target = if top_db > QUIET_DB && over > 0.0 { -(over * 2.0).min(most) } else { 0.0 };
            let c = if target < self.gain_db { self.att } else { self.rel };
            self.gain_db = target + (self.gain_db - target) * c;
            let g = 10f32.powf(self.gain_db / 20.0);
            *slot = low_l + top_l * g;
            if let (Some(_), Some(slot_r)) = (r, right.as_mut().and_then(|r| r.get_mut(i))) {
                *slot_r = low_r + top_r * g;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// An "sss": noise high-passed over the split, at speech level.
    fn sss(seconds: f32) -> Vec<f32> {
        let mut seed = 0x2545_f491u32;
        let mut lp = LowPass::new(7_000.0, SR);
        (0..(seconds * SR) as usize)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let w = (seed as f32 / u32::MAX as f32 - 0.5) * 0.4;
                w - lp.step(w)
            })
            .collect()
    }

    /// A voiced sound: harmonics of 180 Hz up to 3 kHz.
    fn vowel(seconds: f32) -> Vec<f32> {
        (0..(seconds * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                (1..=16).map(|k| (std::f32::consts::TAU * 180.0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.12
            })
            .collect()
    }

    fn energy_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).max(1e-20).log10()
    }

    fn run(level: u8, x: &[f32]) -> Vec<f32> {
        let mut d = DeEsser::new(SR);
        d.set_level(level);
        let mut y = x.to_vec();
        for b in y.chunks_mut(256) {
            d.process(b, None);
        }
        y
    }

    #[test]
    fn off_changes_nothing() {
        let x = sss(0.3);
        assert_eq!(run(0, &x), x);
    }

    #[test]
    fn an_s_is_turned_down_by_about_its_amount() {
        let x = sss(1.0);
        for (level, most) in [(1u8, LIGHT_DB), (2, STRONG_DB)] {
            let y = run(level, &x);
            let drop = energy_db(&x[4_800..]) - energy_db(&y[4_800..]);
            assert!(drop > most * 0.6 && drop < most + 0.8, "level {level}: {drop:.1} dB");
        }
    }

    #[test]
    fn a_vowel_passes() {
        let x = vowel(1.0);
        let y = run(2, &x);
        let diff = (energy_db(&y[4_800..]) - energy_db(&x[4_800..])).abs();
        assert!(diff < 0.3, "{diff:.2} dB");
    }

    #[test]
    fn with_nothing_turned_down_the_split_adds_back_to_the_sound() {
        // A quiet vowel (under the detector's floor): exactly the sound.
        let x: Vec<f32> = vowel(0.2).iter().map(|v| v * 0.001).collect();
        let y = run(2, &x);
        assert!(x.iter().zip(&y).all(|(a, b)| (a - b).abs() < 1e-6));
    }

    #[test]
    fn a_pair_keeps_its_balance_and_broken_samples_are_safe() {
        let mut d = DeEsser::new(SR);
        d.set_level(2);
        let mut l = sss(0.5);
        let mut r: Vec<f32> = l.iter().map(|v| v * 0.5).collect();
        for (bl, br) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            d.process(bl, Some(br));
        }
        let i = l.len() - 100;
        assert!((r[i] - 0.5 * l[i]).abs() < 1e-5);
        let mut x = vec![f32::NAN, 0.3, f32::INFINITY, 0.2];
        d.process(&mut x, None);
        assert!(x.iter().all(|v| v.is_finite()));
    }
}
