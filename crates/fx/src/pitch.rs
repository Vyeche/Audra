//! The voice tuner's pitch (the owner, 2026-11-10: a voice tuner, "useful
//! when using the EQ and the microphone"): the pitch of a voice or an
//! instrument from its last samples, and the note it's nearest. YIN
//! compares the sound with delayed copies of itself and takes the first
//! delay where they match well: one period. Written with the local model
//! and corrected here.

/// The lowest and highest pitch looked for, Hz (a low male voice to a high soprano, a bass guitar's open E is ~41 Hz but voices start ~65).
pub const MIN_HZ: f32 = 60.0;
/// The lowest and highest pitch looked for, Hz (a low male voice to a high soprano, a bass guitar's open E is ~41 Hz but voices start ~65).
pub const MAX_HZ: f32 = 1100.0;
/// Quieter than this (peak, dBFS) nothing is looked for.
pub const QUIET_DB: f32 = -50.0;

/// The pitch of the last up to 2048 samples of `samples` (fewer is fine down to what MIN_HZ needs: at least 2 * rate / MIN_HZ samples, else None), Hz, by YIN (de Cheveigné & Kawahara 2002): the difference function, its cumulative mean normalised form, the first dip under the threshold 0.15 (then followed down to its local minimum), parabolic interpolation around it. None when the sound is too quiet (peak under QUIET_DB), unvoiced (no dip under the threshold), out of MIN_HZ..MAX_HZ, or the input is empty/non-finite-only (non-finite samples count as 0). `rate` in Hz; non-finite or <= 0 gives None.
pub fn detect(samples: &[f32], rate: f32) -> Option<f32> {
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }

    if samples.is_empty() {
        return None;
    }

    // At least 2048 samples, more where the rate needs them for the lowest
    // pitch (two periods of MIN_HZ: 3200 at 96 kHz).
    let want = ((2.0 * rate / MIN_HZ).ceil() as usize).max(2048);
    let tail = if samples.len() > want { &samples[samples.len() - want..] } else { samples };
    let available = tail.len();

    let rate64 = rate as f64;
    let min_samples = 2.0 * rate64 / MIN_HZ as f64;
    if !min_samples.is_finite() || (available as f64) < min_samples {
        return None;
    }

    let quiet_peak = 10.0f32.powf(QUIET_DB / 20.0);
    let peak = tail.iter().fold(0.0f32, |m, &x| {
        let a = if x.is_finite() { x.abs() } else { 0.0 };
        if a > m {
            a
        } else {
            m
        }
    });
    if peak < quiet_peak {
        return None;
    }

    let min_lag_f = rate64 / MAX_HZ as f64;
    let max_lag_f = rate64 / MIN_HZ as f64;
    if !min_lag_f.is_finite() || !max_lag_f.is_finite() {
        return None;
    }

    let mut min_lag = min_lag_f.floor() as usize;
    let max_lag = max_lag_f.floor() as usize;
    if min_lag < 1 {
        min_lag = 1;
    }
    if max_lag < min_lag || max_lag >= available {
        return None;
    }

    // At least one longest period (DeepSeek's review: at 96 kHz 1024
    // samples fell short of a 60 Hz period), at least 1024.
    let w = 1024usize.max(max_lag).min(available - max_lag);
    if w == 0 {
        return None;
    }

    // Use the most recent block that still allows every lag up to max_lag.
    let start = available - (w + max_lag);
    let window = &tail[start..start + w + max_lag];

    // YIN's difference function for every lag up to `max_lag`: its
    // normalising mean runs from lag 1 (the paper's), the dip is looked
    // for from `min_lag` on: the first under the threshold, followed down
    // to its bottom. (The local model's draft averaged from `min_lag`,
    // which lowers the curve and lets too early a dip through.)
    let mut diffs = Vec::with_capacity(max_lag + 1);
    diffs.push(0.0f32);
    let mut running_sum = 0.0f32;
    let mut best: Option<(usize, f32)> = None;
    for tau in 1..=max_lag {
        let d: f32 = window[..w]
            .iter()
            .zip(&window[tau..tau + w])
            .map(|(a, b)| {
                let a = if a.is_finite() { *a } else { 0.0 };
                let b = if b.is_finite() { *b } else { 0.0 };
                (a - b) * (a - b)
            })
            .sum();
        if !d.is_finite() {
            return None;
        }
        diffs.push(d);
        running_sum += d;
        if tau < min_lag {
            continue;
        }
        if !running_sum.is_finite() || running_sum <= 0.0 {
            return None;
        }
        let cmnd = d * tau as f32 / running_sum;
        match best {
            None if cmnd < 0.15 => best = Some((tau, cmnd)),
            Some((_, c)) if cmnd < c => best = Some((tau, cmnd)),
            Some(_) => break,
            None => {}
        }
    }
    let (best_tau, _) = best?;
    let y1 = diffs[best_tau];

    // Between whole lags: a parabola through the dip and its neighbours.
    let period = if best_tau > 0 && best_tau + 1 < diffs.len() {
        let y0 = diffs[best_tau - 1];
        let y2 = diffs[best_tau + 1];
        let denom = y0 - 2.0 * y1 + y2;

        if denom.is_finite() && denom.abs() > f32::EPSILON {
            let shift = 0.5 * (y0 - y2) / denom;
            if shift.is_finite() && shift.abs() <= 1.0 {
                best_tau as f32 + shift
            } else {
                best_tau as f32
            }
        } else {
            best_tau as f32
        }
    } else {
        best_tau as f32
    };

    if !period.is_finite() || period <= 0.0 {
        return None;
    }

    let hz = rate / period;
    if !hz.is_finite() || hz < MIN_HZ || hz > MAX_HZ {
        return None;
    }

    Some(hz)
}

/// The note nearest `hz` (A4 = 440 Hz, equal temperament) as a name with its octave ("A2", "C#4"; sharps, never flats), and how far `hz` is from it in cents (-50..=50; positive = above). None for non-finite or <= 0.
pub fn note_of(hz: f32) -> Option<(String, f32)> {
    if !hz.is_finite() || hz <= 0.0 {
        return None;
    }

    let ratio = hz / 440.0;
    if !ratio.is_finite() || ratio <= 0.0 {
        return None;
    }

    let steps = 12.0 * ratio.log2();
    if !steps.is_finite() {
        return None;
    }

    let n = steps.round() as i32;
    let cents = (steps - n as f32) * 100.0;
    if !cents.is_finite() {
        return None;
    }
    let cents = cents.clamp(-50.0, 50.0);

    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    // A4 is MIDI-like index 57 if C0 is 0.
    let index = 57 + n;
    let note = NAMES[index.rem_euclid(12) as usize];
    let octave = index.div_euclid(12);

    Some((format!("{note}{octave}"), cents))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: f32, hz: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                (std::f32::consts::TAU * hz * t).sin() * amp
            })
            .collect()
    }

    fn voice_like(rate: f32, hz: f32, n: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; n];

        for (i, slot) in v.iter_mut().enumerate() {
            let t = i as f32 / rate;
            let mut s = 0.0f32;
            for k in 1..=10 {
                s += (std::f32::consts::TAU * hz * k as f32 * t).sin() / k as f32;
            }
            *slot = s;
        }

        let peak = v.iter().fold(0.0f32, |m, &x| if x.abs() > m { x.abs() } else { m });
        if peak > 0.0 {
            let scale = 0.5 / peak;
            for x in v.iter_mut() {
                *x *= scale;
            }
        }

        v
    }

    fn noise(n: usize, amp: f32) -> Vec<f32> {
        let mut state = 0x1234_5678u32;
        let mut v = Vec::with_capacity(n);

        for _ in 0..n {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let x = state as f32 / u32::MAX as f32;
            v.push((x * 2.0 - 1.0) * amp);
        }

        v
    }

    #[test]
    fn sine_220() {
        let s = sine(48000.0, 220.0, 2048, 0.5);
        if let Some(hz) = detect(&s, 48000.0) {
            assert!((hz - 220.0).abs() < 1.0);
        } else {
            panic!("expected pitch");
        }
    }

    #[test]
    fn voice_like_110() {
        let s = voice_like(48000.0, 110.0, 2048);
        if let Some(hz) = detect(&s, 48000.0) {
            assert!((hz - 110.0).abs() < 1.0);
        } else {
            panic!("expected pitch");
        }
    }

    #[test]
    fn low_and_high_sines() {
        let low = sine(48000.0, 82.4, 2048, 0.5);
        if let Some(hz) = detect(&low, 48000.0) {
            assert!((hz - 82.4).abs() / 82.4 < 0.01);
        } else {
            panic!("expected low pitch");
        }

        let high = sine(48000.0, 880.0, 2048, 0.5);
        if let Some(hz) = detect(&high, 48000.0) {
            assert!((hz - 880.0).abs() / 880.0 < 0.01);
        } else {
            panic!("expected high pitch");
        }
    }

    #[test]
    fn quiet_is_none() {
        let silent = vec![0.0; 2048];
        assert!(detect(&silent, 48000.0).is_none());

        let amp = 10.0f32.powf(-60.0 / 20.0);
        let quiet = sine(48000.0, 220.0, 2048, amp);
        assert!(detect(&quiet, 48000.0).is_none());
    }

    #[test]
    fn noise_is_none() {
        let n = noise(2048, 0.5);
        assert!(detect(&n, 48000.0).is_none());
    }

    #[test]
    fn too_few_or_empty_is_none() {
        let short = sine(48000.0, 220.0, 100, 0.5);
        assert!(detect(&short, 48000.0).is_none());

        let empty: [f32; 0] = [];
        assert!(detect(&empty, 48000.0).is_none());
    }

    #[test]
    fn a_low_voice_at_a_higher_rate_too() {
        // 70 Hz at 96 kHz: a period of 1371 samples, longer than 1024.
        let s = sine(96000.0, 70.0, 4096, 0.5);
        let hz = detect(&s, 96000.0).expect("a pitch");
        assert!((hz - 70.0).abs() < 0.7, "{hz}");
    }

    #[test]
    fn a_higher_rate_finds_it_too() {
        let s = sine(96000.0, 110.0, 4096, 0.5);
        let hz = detect(&s, 96000.0).expect("a pitch at 96 kHz");
        assert!((hz - 110.0).abs() < 1.0, "{hz}");
    }

    #[test]
    fn note_names_and_cents() {
        if let Some((name, cents)) = note_of(440.0) {
            assert_eq!(name, "A4");
            assert!(cents.abs() < 0.01);
        } else {
            panic!("expected note");
        }

        if let Some((name, cents)) = note_of(261.63) {
            assert_eq!(name, "C4");
            assert!(cents.abs() < 1.0);
        } else {
            panic!("expected note");
        }

        if let Some((name, cents)) = note_of(452.0) {
            assert_eq!(name, "A4");
            assert!((cents - 46.4).abs() < 0.5);
        } else {
            panic!("expected note");
        }

        if let Some((name, cents)) = note_of(116.54) {
            assert_eq!(name, "A#2");
            assert!(cents.abs() < 1.0);
        } else {
            panic!("expected note");
        }
    }
}
