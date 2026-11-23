//! Auto-Tune for what others hear (the owner, 2026-11-10: "Auto tune
//! should be a toggle"): it nudges a voice onto the nearest note of the
//! chromatic scale. Run on the computer, in the processed mic's loop, so
//! others hear it and you don't (the round trip is too late for your own
//! ears).
//!
//! How: the pitch is found every 1024 samples (`pitch::detect`, YIN, the
//! tuner's own) and the median of the last 8 readings taken (about 170
//! ms, one vibrato's swing), so what's corrected is the note the voice is centred
//! on and a singer's vibrato is kept, not chased (measured on the EBU's
//! sung recordings, 2026-11-10: correcting each reading lagged the vibrato
//! and pulled the centre only a third of the way); a leap of more than
//! `LEAP_SEMITONES` starts the average afresh, so a new note is caught at
//! once. The nearest note is held until the voice moves well away from it,
//! so a voice between two notes doesn't flip between them; within 10 cents
//! of a note nothing is corrected; the correction glides there (a gentle
//! 60 ms, for talking as much as singing). The shift itself is a delay
//! line read at a slightly different speed by two taps, crossfaded; the
//! taps sit a whole number of pitch periods apart, so on a voice the
//! crossfade joins like with like and doesn't flutter. When the correction
//! is over, the taps hand over to one of them in 20 ms: left mixed, two
//! copies of the sound half a window apart filtered everything after like
//! a comb (measured: notches to −6.5 dB on what came next).
//!
//! As the rest of this crate: no allocation while processing (beyond the
//! detector's own), testable offline. Run it before `Dynamics`, whose
//! clamp and non-finite reset then cover it too.

use crate::pitch;

/// How often the pitch is looked for, in samples at the working rate.
const DETECT_EVERY: usize = 1024;
/// Samples the detector looks at, at the working rate.
const DETECT_LEN: usize = 2048;
/// Within this many cents of a note, nothing is corrected.
pub const DEADBAND_CENTS: f32 = 10.0;
/// A held note is kept until the voice is this far from it, in semitones.
const HOLD_SEMITONES: f32 = 0.75;
/// How long a note is held after the voice stops being found, in ms.
const HOLD_MS: f32 = 150.0;
/// How quickly the correction moves to a new target, in ms.
pub const GLIDE_MS: f32 = 60.0;
/// About half the shifter's window, in ms (rounded to whole periods).
const HALF_WINDOW_MS: f32 = 15.0;
/// The shortest delay a tap reads at, in ms.
const MIN_DELAY_MS: f32 = 2.0;
/// Readings averaged for the note the voice is centred on (8 x 1024
/// samples, about 170 ms: one swing of a 5–7 Hz vibrato).
const CENTRE_READINGS: usize = 8;
/// A reading this far from the average is a new note, in semitones (a
/// wide operatic vibrato swings about one semitone either side).
const LEAP_SEMITONES: f32 = 1.25;
/// How long the taps take to hand over to one at rest, in ms.
const SETTLE_MS: f32 = 20.0;
/// The glide counts as done this near "as it is" (under a cent): its
/// last tiny steps take hundreds of ms and nobody hears them.
const REST_OFF: f32 = 5e-4;

/// The working rate the detector runs at (about 48 kHz).
const WORK_RATE: f32 = 48_000.0;

/// One channel's (or one linked pair's) Auto-Tune.
pub struct AutoTune {
    rate: f32,
    /// The detector's samples: every `decim` input samples one, averaged.
    decim: usize,
    acc: f32,
    acc_n: usize,
    history: Vec<f32>,
    hist_pos: usize,
    hist_full: bool,
    scratch: Vec<f32>,
    since_detect: usize,
    /// The note held now (MIDI number), and samples since it was last found.
    note: Option<i32>,
    note_age: usize,
    target: f32,
    ratio: f32,
    /// How far the ratio still is from the target: kept on its own, as
    /// near 1 an f32 ratio can't take the glide's last tiny steps (it stuck
    /// at 1.00017, a drift for ever).
    off: f32,
    glide: f32,
    // The shifter: a delay line per side, two taps.
    bufs: [Vec<f32>; 2],
    mask: usize,
    w_pos: usize,
    /// Tap A's delay, in samples; tap B reads half a window away.
    d: f32,
    d_min: f32,
    w: f32,
    /// The last readings, in semitones (MIDI numbers), for the centre.
    recent: [f32; CENTRE_READINGS],
    recent_n: usize,
    recent_pos: usize,
    /// Whether the held note was chosen from a full set of readings.
    note_sure: bool,
    /// The hand-over to one tap at rest: how far along (0 to 1), tap A's
    /// gain when it began, and whether tap B is the one kept.
    settle: Option<(f32, f32, bool)>,
    settle_step: f32,
}

fn coeff(ms: f32, rate: f32) -> f32 {
    let samples = (ms.max(0.01) * 0.001 * rate).max(1.0);
    (-1.0 / samples).exp()
}

impl AutoTune {
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        let decim = (rate / WORK_RATE).round().max(1.0) as usize;
        // Room for the longest window: half a window of up to 15 ms plus
        // half a period of the lowest voice, twice, and the shortest delay.
        let len = ((rate * 0.06) as usize).next_power_of_two();
        let d_min = MIN_DELAY_MS * 0.001 * rate;
        let w = 2.0 * HALF_WINDOW_MS * 0.001 * rate;
        Self {
            rate,
            decim,
            acc: 0.0,
            acc_n: 0,
            history: vec![0.0; DETECT_LEN],
            hist_pos: 0,
            hist_full: false,
            scratch: vec![0.0; DETECT_LEN],
            since_detect: 0,
            note: None,
            note_age: 0,
            target: 1.0,
            ratio: 1.0,
            off: 0.0,
            glide: coeff(GLIDE_MS, rate),
            bufs: [vec![0.0; len], vec![0.0; len]],
            mask: len - 1,
            w_pos: 0,
            d: d_min + w / 2.0,
            d_min,
            w,
            recent: [0.0; CENTRE_READINGS],
            recent_n: 0,
            recent_pos: 0,
            note_sure: false,
            settle: None,
            settle_step: 1.0 / (SETTLE_MS * 0.001 * rate).max(1.0),
        }
    }

    /// The note it's pulling to now (MIDI number), if any.
    pub fn note(&self) -> Option<i32> {
        self.note
    }

    /// The speed the voice is read at now (1 = as it is).
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// How late the sound comes out, in samples: the middle of the delay
    /// line's window (about 17 ms), steady while the taps sweep (reading
    /// the moving tap made the delay line jitter: a second look's catch).
    pub fn delay_samples(&self) -> f32 {
        self.d_min + self.w / 2.0
    }

    /// Back to rest: nothing held, nothing shifted, the line silent. In
    /// place: no allocation, as it runs in the audio loop (DeepSeek's
    /// review: rebuilding it allocated there).
    pub fn reset(&mut self) {
        self.rest_correction();
        for b in &mut self.bufs {
            b.fill(0.0);
        }
        self.w_pos = 0;
        self.d_min = MIN_DELAY_MS * 0.001 * self.rate;
        self.w = 2.0 * HALF_WINDOW_MS * 0.001 * self.rate;
        self.d = self.d_min + self.w / 2.0;
        self.settle = None;
    }

    /// The correction back to rest, but the line keeps its sound: the
    /// note and the pitch readings go, the taps and the line stay where
    /// they are. For a broken sample, which should cost one silent
    /// sample and no more: emptying the line left about 17 ms of
    /// silence and then a jump. The hand-over between the two taps
    /// carries on: it is a change of taps, not a correction, and
    /// dropping it half way would step the gain.
    fn rest_correction(&mut self) {
        self.acc = 0.0;
        self.acc_n = 0;
        self.history.fill(0.0);
        self.hist_pos = 0;
        self.hist_full = false;
        self.since_detect = 0;
        self.note = None;
        self.note_age = 0;
        self.target = 1.0;
        self.ratio = 1.0;
        self.off = 0.0;
        self.recent_n = 0;
        self.recent_pos = 0;
        self.note_sure = false;
    }

    /// Processes a block in place: `left`, and `right` for a linked pair
    /// (one pitch, one correction for both).
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>) {
        for (i, slot) in left.iter_mut().enumerate() {
            let l = *slot;
            let r = right.as_ref().and_then(|r| r.get(i)).copied();
            if !l.is_finite() || r.is_some_and(|r| !r.is_finite()) {
                // A broken sample: the correction back to rest, and a zero
                // into the line in place of the bad sample, so the sound
                // carries on and only that one sample is silent. (The
                // line emptied left about 17 ms of silence and a jump.)
                self.rest_correction();
                let (out_l, out_r) = self.shift(
                    if l.is_finite() { l } else { 0.0 },
                    r.map(|r| if r.is_finite() { r } else { 0.0 }),
                );
                *slot = out_l;
                if let (Some(r_out), Some(slot_r)) = (out_r, right.as_mut().and_then(|r| r.get_mut(i))) {
                    *slot_r = r_out;
                }
                continue;
            }
            self.listen(r.map_or(l, |r| 0.5 * (l + r)));
            // Glide toward the target; exactly there once close.
            self.off *= self.glide;
            if self.off.abs() < 1e-7 {
                self.off = 0.0;
            }
            self.ratio = self.target + self.off;
            let (out_l, out_r) = self.shift(l, r);
            *slot = out_l;
            if let (Some(r_out), Some(slot_r)) = (out_r, right.as_mut().and_then(|r| r.get_mut(i))) {
                *slot_r = r_out;
            }
        }
    }

    /// One input sample into the detector; every `DETECT_EVERY` working
    /// samples a look for the pitch and a new target.
    fn listen(&mut self, x: f32) {
        self.acc += x;
        self.acc_n += 1;
        if self.acc_n < self.decim {
            return;
        }
        let v = self.acc / self.acc_n as f32;
        self.acc = 0.0;
        self.acc_n = 0;
        self.history[self.hist_pos] = v;
        self.hist_pos = (self.hist_pos + 1) % DETECT_LEN;
        if self.hist_pos == 0 {
            self.hist_full = true;
        }
        self.since_detect += 1;
        self.note_age = self.note_age.saturating_add(1);
        if self.since_detect < DETECT_EVERY || !self.hist_full {
            return;
        }
        self.since_detect = 0;
        let (head, tail) = self.history.split_at(self.hist_pos);
        self.scratch[..tail.len()].copy_from_slice(tail);
        self.scratch[tail.len()..].copy_from_slice(head);
        let work_rate = self.rate / self.decim as f32;
        match pitch::detect(&self.scratch, work_rate) {
            Some(hz) => {
                let now = 69.0 + 12.0 * (hz / 440.0).log2();
                let steps = self.centre(now);
                // The note is held only once it was chosen from a full set
                // of readings: chosen from the first, a vibrato's peak, it
                // held the note above (measured).
                let full = self.recent_n == CENTRE_READINGS;
                let n = match self.note {
                    Some(n) if self.note_sure && (steps - n as f32).abs() <= HOLD_SEMITONES => n,
                    _ => steps.round() as i32,
                };
                self.note_sure = full;
                self.note = Some(n);
                self.note_age = 0;
                let off = steps - n as f32;
                self.set_target(if off.abs() * 100.0 <= DEADBAND_CENTS { 1.0 } else { 2f32.powf(-off / 12.0) });
                self.fit_window(self.rate / hz);
            }
            None => {
                // A missed reading keeps the correction for the hold (a
                // dip to "as it is" for one reading wobbled; DeepSeek's
                // review); after it, or with no note at all, nothing.
                if self.note_age as f32 > HOLD_MS * 0.001 * work_rate {
                    self.note = None;
                    // Both: the median reads from the start of the ring
                    // (Qwen's review: the write position was left behind).
                    self.recent_n = 0;
                    self.recent_pos = 0;
                }
                if self.note.is_none() {
                    self.set_target(1.0);
                }
            }
        }
    }

    /// One reading in, the centre out: the median of the last readings,
    /// started afresh on a leap to a new note.
    fn centre(&mut self, now: f32) -> f32 {
        if self.recent_n > 0 {
            let avg = self.recent[..self.recent_n].iter().sum::<f32>() / self.recent_n as f32;
            if (now - avg).abs() > LEAP_SEMITONES {
                self.recent_n = 0;
                self.recent_pos = 0;
            }
        }
        self.recent[self.recent_pos] = now;
        self.recent_pos = (self.recent_pos + 1) % CENTRE_READINGS;
        self.recent_n = (self.recent_n + 1).min(CENTRE_READINGS);
        // The median: a vibrato's swings either side cancel, and a step to
        // the next note (a scale, a semitone: no leap) takes over once
        // half the readings are on it, instead of blending the two notes
        // as an average did for 170 ms (measured on sung scales).
        let mut v = self.recent;
        let v = &mut v[..self.recent_n];
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && v[j - 1] > v[j] {
                v.swap(j - 1, j);
                j -= 1;
            }
        }
        let k = v.len();
        if k % 2 == 1 {
            v[k / 2]
        } else {
            0.5 * (v[k / 2 - 1] + v[k / 2])
        }
    }

    /// A new target, the glide carrying on from where the ratio is now.
    fn set_target(&mut self, target: f32) {
        self.off = self.target + self.off - target;
        self.target = target;
    }

    /// The window to a whole number of periods (`period` samples) near
    /// `HALF_WINDOW_MS` a half, changed only while tap A carries the sound
    /// alone, so the change is never heard.
    fn fit_window(&mut self, period: f32) {
        // Never in the middle of the taps' hand-over, whose crossfade is
        // set for this window (Qwen's review).
        if !period.is_finite() || period <= 1.0 || self.settle.is_some() {
            return;
        }
        let half_want = HALF_WINDOW_MS * 0.001 * self.rate;
        let k = (half_want / period).round().max(1.0);
        let w_new = 2.0 * k * period;
        if (w_new - self.w).abs() < 1.0 {
            return;
        }
        let phase = (self.d - self.d_min) / self.w;
        let a = (std::f32::consts::PI * phase).sin().powi(2);
        if a < 0.97 {
            return;
        }
        let d_min_new = self.d - w_new / 2.0;
        let floor = MIN_DELAY_MS * 0.001 * self.rate;
        if d_min_new < floor || d_min_new + w_new + 2.0 >= self.bufs[0].len() as f32 {
            return;
        }
        self.d_min = d_min_new;
        self.w = w_new;
    }

    /// One sample through the delay lines and out of the two taps.
    fn shift(&mut self, l: f32, r: Option<f32>) -> (f32, Option<f32>) {
        let len = self.bufs[0].len();
        self.bufs[0][self.w_pos] = l;
        if let Some(r) = r {
            self.bufs[1][self.w_pos] = r;
        }
        // At rest (no correction, the glide done), the two taps hand over to
        // one of them: the one whose delay keeps the line shortest, its
        // window then centred on it. Until the hand-over ends the taps
        // stay still, and a correction starting meanwhile waits for it.
        let at_rest = self.target == 1.0 && self.off.abs() < REST_OFF;
        if self.settle.is_none() && at_rest {
            self.off = 0.0;
            self.ratio = 1.0;
            let g_a = (std::f32::consts::PI * (self.d - self.d_min) / self.w).sin().powi(2);
            if g_a < 0.999 {
                let d_b = self.other_tap();
                let home = MIN_DELAY_MS * 0.001 * self.rate + self.w / 2.0;
                let keep_b = d_b < self.d && d_b >= home - 0.5 || self.d < home - 0.5;
                let kept = if keep_b { d_b } else { self.d };
                if kept + self.w / 2.0 + 2.0 < len as f32 {
                    self.settle = Some((0.0, g_a, keep_b));
                }
            }
        }
        let (g_a, d_b) = if let Some((t, from, keep_b)) = self.settle {
            let t = (t + self.settle_step).min(1.0);
            let to = if keep_b { 0.0 } else { 1.0 };
            let g_a = from + (to - from) * t;
            let d_b = self.other_tap();
            self.settle = Some((t, from, keep_b));
            (g_a, d_b)
        } else {
            // Read faster (pitch up) by shortening the delay, slower by
            // lengthening it; past the window's end, back by a whole
            // window (where that tap's gain is nothing).
            self.d += 1.0 - self.ratio;
            if self.d < self.d_min {
                self.d += self.w;
            } else if self.d >= self.d_min + self.w {
                self.d -= self.w;
            }
            let phase = (self.d - self.d_min) / self.w;
            ((std::f32::consts::PI * phase).sin().powi(2), self.other_tap())
        };
        let g_b = 1.0 - g_a;
        let read = |buf: &[f32], delay: f32| {
            let at = (self.w_pos as f32 - delay).rem_euclid(len as f32);
            let i0 = at.floor() as usize & self.mask;
            let i1 = (i0 + 1) & self.mask;
            let frac = at - at.floor();
            buf[i0] + (buf[i1] - buf[i0]) * frac
        };
        let out_l = g_a * read(&self.bufs[0], self.d) + g_b * read(&self.bufs[0], d_b);
        let out_r = r.map(|_| g_a * read(&self.bufs[1], self.d) + g_b * read(&self.bufs[1], d_b));
        self.w_pos = (self.w_pos + 1) & self.mask;
        // The hand-over done: the kept tap becomes tap A, its window centred
        // on it (tap A's gain is then 1 by the window's own law).
        if let Some((t, _, keep_b)) = self.settle {
            if t >= 1.0 {
                if keep_b {
                    self.d = d_b;
                }
                self.d_min = self.d - self.w / 2.0;
                self.settle = None;
            }
        }
        (out_l, out_r)
    }

    /// Tap B's delay: half a window from tap A's, inside the window.
    fn other_tap(&self) -> f32 {
        let d_b = self.d + self.w / 2.0;
        if d_b >= self.d_min + self.w {
            d_b - self.w
        } else {
            d_b
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn tone(hz: f32, seconds: f32, amp: f32) -> Vec<f32> {
        // A voice-like tone: a fundamental and a few harmonics.
        (0..(seconds * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                let s: f32 = (1..=5).map(|k| (std::f32::consts::TAU * hz * k as f32 * t).sin() / k as f32).sum();
                amp * s / 2.3
            })
            .collect()
    }

    fn pitch_of(x: &[f32]) -> f32 {
        pitch::detect(&x[x.len() - 2048..], SR).expect("a pitch")
    }

    fn rms_db(x: &[f32]) -> f32 {
        let ms = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
        10.0 * ms.max(1e-12).log10()
    }

    #[test]
    fn an_in_tune_voice_comes_out_unchanged() {
        // A3, 220 Hz: in tune, so nothing moves; only the delay.
        let mut t = AutoTune::new(SR);
        let mut x = tone(220.0, 1.0, 0.5);
        let before = x.clone();
        t.process(&mut x, None);
        assert_eq!(t.ratio(), 1.0);
        assert!((pitch_of(&x) - 220.0).abs() < 1.0);
        // The same sound, a fixed delay later.
        let delay = (t.d) as usize;
        let tail = x.len() - 4800;
        let err: f32 = (tail..x.len()).map(|i| (x[i] - before[i - delay]).abs()).fold(0.0, f32::max);
        assert!(err < 0.02, "max difference {err}");
    }

    #[test]
    fn a_flat_note_is_pulled_onto_pitch() {
        // 143 Hz lies between C#3 (138.59) and D3 (146.83), nearer D3.
        let mut t = AutoTune::new(SR);
        let mut x = tone(143.0, 1.5, 0.5);
        t.process(&mut x, None);
        assert_eq!(t.note(), Some(50), "D3");
        let out = pitch_of(&x);
        assert!((out - 146.83).abs() < 146.83 * 0.004, "{out} Hz");
    }

    #[test]
    fn a_steady_correction_doesnt_flutter() {
        let mut t = AutoTune::new(SR);
        let mut x = tone(143.0, 2.0, 0.5);
        t.process(&mut x, None);
        // Loudness over 20 ms windows in the last second: steady.
        let tail = &x[x.len() - 48_000..];
        let levels: Vec<f32> = tail.chunks(960).map(rms_db).collect();
        let (lo, hi) = levels.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(hi - lo < 1.5, "ripple {:.2} dB", hi - lo);
    }

    #[test]
    fn noise_and_silence_are_left_alone() {
        let mut t = AutoTune::new(SR);
        let mut seed = 0x1234_5678u32;
        let mut noise: Vec<f32> = (0..48_000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.5
            })
            .collect();
        t.process(&mut noise, None);
        assert_eq!(t.ratio(), 1.0, "nothing to tune in noise");
        let mut t = AutoTune::new(SR);
        let mut silence = vec![0.0; 48_000];
        t.process(&mut silence, None);
        assert!(silence.iter().all(|v| *v == 0.0) && t.ratio() == 1.0);
    }

    #[test]
    fn a_voice_between_two_notes_doesnt_flip() {
        // Wavering ±10 cents round the C#3/D3 boundary (142.65 Hz) for 3 s.
        let mut t = AutoTune::new(SR);
        let mut changes = 0;
        let mut last = None;
        let mut phase = 0.0f32;
        let block = 1024;
        for b in 0..(3 * 48_000 / block) {
            let cents = 10.0 * (b as f32 * 0.3).sin();
            let hz = 142.65 * 2f32.powf(cents / 1200.0);
            let mut x: Vec<f32> = (0..block)
                .map(|_| {
                    phase += std::f32::consts::TAU * hz / SR;
                    0.5 * phase.sin()
                })
                .collect();
            t.process(&mut x, None);
            if t.note().is_some() && t.note() != last {
                if last.is_some() {
                    changes += 1;
                }
                last = t.note();
            }
        }
        assert_eq!(changes, 0, "the held note stays");
    }

    #[test]
    fn a_short_gap_keeps_the_correction() {
        // A 40 ms gap mid-note (a consonant, a breath): the pull stays.
        let mut t = AutoTune::new(SR);
        let mut x = tone(143.0, 1.0, 0.5);
        t.process(&mut x, None);
        let held = t.ratio();
        assert!(held > 1.02, "{held}");
        let mut gap = vec![0.0; 1920];
        t.process(&mut gap, None);
        assert!(t.ratio() > 1.02, "kept through the gap: {}", t.ratio());
        // A long silence lets it go.
        let mut quiet = vec![0.0; 48_000];
        t.process(&mut quiet, None);
        assert_eq!(t.ratio(), 1.0);
        assert_eq!(t.note(), None);
    }

    #[test]
    fn a_linked_pair_gets_one_correction() {
        let mut t = AutoTune::new(SR);
        let mut l = tone(143.0, 1.0, 0.5);
        let mut r: Vec<f32> = l.iter().map(|v| v * 0.5).collect();
        t.process(&mut l, Some(&mut r));
        for i in (l.len() - 2000)..l.len() {
            if l[i].abs() > 1e-3 {
                assert!((r[i] / l[i] - 0.5).abs() < 1e-3, "the image keeps its balance at {i}");
            }
        }
    }

    #[test]
    fn a_broken_sample_costs_one_silent_sample_and_no_more() {
        // A steady tone fills the line, then one broken sample, then the
        // tone again: the sound carries on, and only the one sample the
        // zero in the line makes is silent.
        let mut t = AutoTune::new(SR);
        let mut x = tone(143.0, 0.5, 0.5);
        t.process(&mut x, None);
        let mut bad = vec![f32::NAN];
        t.process(&mut bad, None);
        // The correction is at rest, and the note is gone.
        assert_eq!(t.ratio(), 1.0);
        assert_eq!(t.note(), None);
        // The tone again.
        let mut y = tone(143.0, 0.5, 0.5);
        t.process(&mut y, None);
        // Nothing broken comes out.
        assert!(y.iter().all(|v| v.is_finite()));
        // No hole: never more than one silent sample in a row.
        let mut run = 0usize;
        let mut worst = 0usize;
        for v in &y {
            if *v == 0.0 {
                run += 1;
                worst = worst.max(run);
            } else {
                run = 0;
            }
        }
        assert!(worst <= 1, "a run of {worst} silent samples");
        // The sound is still there, as loud as before.
        assert!((rms_db(&y) - rms_db(&x)).abs() < 1.0, "the sound carries on");
    }

    #[test]
    fn after_a_correction_the_taps_settle_and_filter_nothing() {
        // A flat note pulled up, then noise: once the correction is over,
        // the noise must come out as it went in, only later (left mixed,
        // the two taps made a comb: notches to -6.5 dB, measured).
        let mut t = AutoTune::new(SR);
        let mut x = tone(143.0, 1.3, 0.5);
        t.process(&mut x, None);
        let mut seed = 0x2468_ace1u32;
        let noise: Vec<f32> = (0..(SR as usize))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.2
            })
            .collect();
        let mut y = noise.clone();
        t.process(&mut y, None);
        assert!(t.settle.is_none(), "settled");
        let d = t.d;
        assert!((t.d - t.d_min - t.w / 2.0).abs() < 1e-3, "tap A centred");
        let (i0, frac) = (d.floor() as usize, d - d.floor());
        let err: f32 = (40_000..48_000)
            .map(|i| {
                let want = noise[i - i0] + (noise[i - i0 - 1] - noise[i - i0]) * frac;
                (y[i] - want).abs()
            })
            .fold(0.0, f32::max);
        assert!(err < 1e-3, "the noise a pure delay later: {err}");
    }

    #[test]
    fn a_vibrato_is_kept_and_its_centre_put_on_the_note() {
        // A3 + 40 cents, with a +-60 cent vibrato at 5.5 Hz: the centre is
        // pulled onto A3 (220 Hz), and the vibrato survives.
        let mut t = AutoTune::new(SR);
        let mut phase = 0.0f32;
        let n = (3.0 * SR) as usize;
        let mut x: Vec<f32> = (0..n)
            .map(|i| {
                let s = i as f32 / SR;
                let cents = 40.0 + 60.0 * (std::f32::consts::TAU * 5.5 * s).sin();
                phase += std::f32::consts::TAU * 220.0 * 2f32.powf(cents / 1200.0) / SR;
                (1..=5).map(|k| (k as f32 * phase).sin() / k as f32).sum::<f32>() * 0.2
            })
            .collect();
        t.process(&mut x, None);
        // Pitch every 10 ms over the last 2 s, in cents from A3.
        let cents: Vec<f32> = (1..200)
            .filter_map(|k| {
                let end = n - k * 480;
                pitch::detect(&x[end - 2048..end], SR).map(|hz| 1200.0 * (hz / 220.0).log2())
            })
            .collect();
        let mean = cents.iter().sum::<f32>() / cents.len() as f32;
        let (lo, hi) = cents.iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        assert!(mean.abs() < 12.0, "centre {mean:.1} cents from A3");
        assert!(hi - lo > 70.0, "vibrato kept: {:.0} cents wide", hi - lo);
    }

    #[test]
    fn a_higher_rate_works_too() {
        let sr = 96_000.0;
        let mut t = AutoTune::new(sr);
        let mut x: Vec<f32> = (0..(1.5 * sr) as usize).map(|i| 0.5 * (std::f32::consts::TAU * 143.0 * i as f32 / sr).sin()).collect();
        t.process(&mut x, None);
        assert_eq!(t.note(), Some(50));
        let out = pitch::detect(&x[x.len() - 4096..], sr).expect("a pitch");
        assert!((out - 146.83).abs() < 146.83 * 0.004, "{out} Hz");
    }
}
