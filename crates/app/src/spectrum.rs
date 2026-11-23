//! The math under the EQ panel's live spectrum (`live_spectrum`): samples
//! to a Hann-windowed magnitude spectrum in dBFS, those bins onto the
//! plot's log-frequency columns, and a smoother that lets a peak fall back
//! gently. Written with the local model (its FFT's butterflies were wrong
//! and rewritten here).

/// Samples per analysis.
pub const FFT_N: usize = 2048;

/// The quietest level shown, dBFS.
pub const FLOOR_DB: f32 = -100.0;

/// The magnitude spectrum of the last FFT_N samples of `samples` (fewer: zero-padded at the front), Hann-windowed, in dBFS per bin: FFT_N/2 bins, bin k at k*rate/FFT_N Hz. Scaled so a full-scale sine reads about 0 dB at its bin (divide by the window's coherent gain: magnitude * 2 / sum(window)). Never below FLOOR_DB; non-finite input samples count as 0.
pub fn magnitudes_db(samples: &[f32]) -> Vec<f32> {
    let n = FFT_N;
    let mut re = vec![0.0f32; n];
    let mut im = vec![0.0f32; n];

    let mut sum_window = 0.0f32;
    let mut idx = 0usize;
    while idx < n {
        sum_window += hann_window(idx, n);
        idx += 1;
    }

    if !sum_window.is_finite() || sum_window <= 0.0 {
        return vec![FLOOR_DB; n / 2];
    }

    let pad = n.saturating_sub(samples.len());
    let start = samples.len().saturating_sub(n);

    let mut idx = 0usize;
    while idx < n {
        let w = hann_window(idx, n);
        let sample = if idx < pad {
            0.0
        } else {
            samples
                .get(start + (idx - pad))
                .copied()
                .unwrap_or(0.0)
        };

        re[idx] = if sample.is_finite() { sample * w } else { 0.0 };
        idx += 1;
    }

    fft(&mut re, &mut im);

    let scale = 2.0 / sum_window;
    let mut out = Vec::with_capacity(n / 2);

    for (re_k, im_k) in re.iter().zip(im.iter()).take(n / 2) {
        let mag = re_k.hypot(*im_k) * scale;

        let db = if mag.is_finite() && mag > 0.0 {
            let db = 20.0 * mag.log10();
            if db.is_finite() {
                if db < FLOOR_DB {
                    FLOOR_DB
                } else {
                    db
                }
            } else if db > 0.0 {
                f32::MAX
            } else {
                FLOOR_DB
            }
        } else {
            FLOOR_DB
        };

        out.push(db);
    }

    out
}

/// The bins on a log-frequency axis: `cols` columns spanning f_lo..f_hi Hz (log spaced, column i covers [f_lo*(f_hi/f_lo)^(i/cols), f_lo*(f_hi/f_lo)^((i+1)/cols)) ). Each column is the max of the bins whose frequency lies in its band; a band narrower than one bin takes the bin value linearly interpolated at the band's centre frequency (geometric mean of its edges). Bins past Nyquist don't exist: such columns are FLOOR_DB. `rate` is the sample rate in Hz.
pub fn columns(bins_db: &[f32], rate: f32, cols: usize, f_lo: f32, f_hi: f32) -> Vec<f32> {
    if cols == 0 {
        return Vec::new();
    }

    if !f_lo.is_finite()
        || !f_hi.is_finite()
        || f_lo <= 0.0
        || f_hi <= 0.0
        || f_lo >= f_hi
        || !rate.is_finite()
        || rate <= 0.0
        || bins_db.is_empty()
    {
        return vec![FLOOR_DB; cols];
    }

    let nyquist = rate * 0.5;
    let bin_width = rate / FFT_N as f32;

    if !bin_width.is_finite() || bin_width <= 0.0 {
        return vec![FLOOR_DB; cols];
    }

    let ratio = f_hi / f_lo;
    let log_ratio = ratio.ln();

    if !ratio.is_finite() || ratio <= 1.0 || !log_ratio.is_finite() || log_ratio <= 0.0 {
        return vec![FLOOR_DB; cols];
    }

    let mut out = Vec::with_capacity(cols);
    let mut i = 0usize;

    while i < cols {
        let lo = f_lo * (log_ratio * (i as f32 / cols as f32)).exp();
        let hi = f_lo * (log_ratio * ((i + 1) as f32 / cols as f32)).exp();

        if !lo.is_finite() || !hi.is_finite() || hi <= lo || lo >= nyquist || hi <= 0.0 {
            out.push(FLOOR_DB);
            i += 1;
            continue;
        }

        let band_width = hi - lo;

        if band_width < bin_width {
            let center = (lo * hi).sqrt();

            if !center.is_finite() || center < 0.0 || center >= nyquist {
                out.push(FLOOR_DB);
            } else {
                out.push(interpolate_bin(bins_db, center / bin_width));
            }
        } else {
            let mut max = FLOOR_DB;
            let mut found = false;
            let half_bin = bin_width * 0.5;
            // Only the bins whose half-bin span can meet the band.
            let first = ((lo / bin_width) - 0.5).floor().max(0.0) as usize;
            let last = (((hi / bin_width) + 0.5).ceil() as usize).min(bins_db.len());

            for (k, &db) in bins_db.iter().enumerate().take(last).skip(first) {
                let freq = k as f32 * bin_width;

                if freq + half_bin > lo && freq - half_bin < hi {
                    if db.is_finite() && db > max {
                        max = db;
                    }
                    found = true;
                }
            }

            if found {
                out.push(max);
            } else {
                let center = (lo * hi).sqrt();

                if !center.is_finite() || center < 0.0 || center >= nyquist {
                    out.push(FLOOR_DB);
                } else {
                    out.push(interpolate_bin(bins_db, center / bin_width));
                }
            }
        }

        i += 1;
    }

    out
}

/// Smoothing across frames, per column: a rise shows at once, a fall goes down at most `fall_db_per_s` per second.
pub struct Smoother {
    values: Vec<f32>,
}

impl Smoother {
    /// Makes a smoother with `cols` columns, all at FLOOR_DB.
    pub fn new(cols: usize) -> Self {
        Self {
            values: vec![FLOOR_DB; cols],
        }
    }

    /// Updates the smoother and returns the current values.
    pub fn step(&mut self, now: &[f32], dt_s: f32, fall_db_per_s: f32) -> &[f32] {
        if self.values.len() != now.len() {
            self.values.resize(now.len(), FLOOR_DB);
        }

        let dt = if dt_s.is_finite() && dt_s > 0.0 {
            dt_s
        } else {
            0.0
        };

        let fall = if fall_db_per_s.is_finite() && fall_db_per_s > 0.0 {
            fall_db_per_s
        } else {
            0.0
        };

        let mut max_fall = fall * dt;
        if !max_fall.is_finite() {
            max_fall = f32::MAX;
        }

        for (old, now_val) in self.values.iter_mut().zip(now.iter()) {
            let target = if now_val.is_finite() { *now_val } else { FLOOR_DB };
            let target = if target < FLOOR_DB { FLOOR_DB } else { target };

            let old_val = if old.is_finite() { *old } else { FLOOR_DB };

            if target >= old_val {
                *old = target;
            } else {
                let min_allowed = if max_fall >= f32::MAX {
                    FLOOR_DB
                } else {
                    (old_val - max_fall).max(FLOOR_DB)
                };

                *old = target.max(min_allowed);

                if *old < FLOOR_DB {
                    *old = FLOOR_DB;
                }
            }
        }

        self.values.as_slice()
    }

    /// Returns the current smoothed values.
    pub fn values(&self) -> &[f32] {
        self.values.as_slice()
    }
}

fn hann_window(n: usize, total: usize) -> f32 {
    if total == 0 {
        return 0.0;
    }

    0.5 * (1.0 - (std::f32::consts::TAU * n as f32 / total as f32).cos())
}

fn interpolate_bin(bins_db: &[f32], x: f32) -> f32 {
    if bins_db.is_empty() || !x.is_finite() {
        return FLOOR_DB;
    }

    let last = bins_db.len() as f32 - 1.0;

    if x <= 0.0 {
        let v = bins_db[0];
        return if v.is_finite() && v >= FLOOR_DB { v } else { FLOOR_DB };
    }

    if x >= last {
        let v = bins_db[bins_db.len() - 1];
        return if v.is_finite() && v >= FLOOR_DB { v } else { FLOOR_DB };
    }

    let k = x.floor() as usize;

    if k + 1 >= bins_db.len() {
        let v = bins_db[bins_db.len() - 1];
        return if v.is_finite() && v >= FLOOR_DB { v } else { FLOOR_DB };
    }

    let frac = x - k as f32;
    let a = if bins_db[k].is_finite() { bins_db[k] } else { FLOOR_DB };
    let b = if bins_db[k + 1].is_finite() { bins_db[k + 1] } else { FLOOR_DB };

    let v = a + (b - a) * frac;

    if !v.is_finite() || v < FLOOR_DB {
        FLOOR_DB
    } else {
        v
    }
}

/// An iterative radix-2 FFT in place (`re`, `im` the same power-of-two
/// length; anything else is left as it is).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n <= 1 || !n.is_power_of_two() || im.len() != n {
        return;
    }
    // Bit-reversed order.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    // The butterflies, a stage at a time, its twiddles worked out once.
    let mut len = 2usize;
    while len <= n {
        let half = len / 2;
        let step = -std::f32::consts::TAU / len as f32;
        let twiddles: Vec<(f32, f32)> = (0..half).map(|k| ((step * k as f32).cos(), (step * k as f32).sin())).collect();
        for start in (0..n).step_by(len) {
            for (k, &(wr, wi)) in twiddles.iter().enumerate() {
                let (a, b) = (start + k, start + k + half);
                let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: f32, amp: f32) -> Vec<f32> {
        (0..FFT_N)
            .map(|n| amp * (std::f32::consts::TAU * freq * n as f32 / rate).sin())
            .collect()
    }

    #[test]
    fn full_scale_sine_reads_zero_db() {
        let samples = sine(1000.0, 48000.0, 1.0);
        let bins = magnitudes_db(&samples);

        let mut peak = 0usize;
        for (i, &v) in bins.iter().enumerate() {
            if v > bins[peak] {
                peak = i;
            }
        }

        let expected_bin = (1000.0 * FFT_N as f32 / 48000.0).round() as usize;

        assert!((peak as f32 - expected_bin as f32).abs() <= 1.0);
        assert!((bins[peak] - 0.0).abs() <= 1.5);
    }

    #[test]
    fn quiet_sine_reads_minus_twenty_db() {
        let amp = 10.0f32.powf(-20.0 / 20.0);
        let samples = sine(1000.0, 48000.0, amp);
        let bins = magnitudes_db(&samples);

        let mut peak = 0usize;
        for (i, &v) in bins.iter().enumerate() {
            if v > bins[peak] {
                peak = i;
            }
        }

        assert!((bins[peak] - (-20.0)).abs() <= 1.5);
    }

    #[test]
    fn silence_is_floor() {
        let samples = vec![0.0; FFT_N];
        let bins = magnitudes_db(&samples);

        assert!(bins.iter().all(|&v| v == FLOOR_DB));
    }

    #[test]
    fn non_finite_samples_do_not_break_it() {
        let mut samples = vec![0.0; FFT_N];
        samples[0] = f32::NAN;
        samples[1] = f32::INFINITY;
        samples[2] = f32::NEG_INFINITY;

        let bins = magnitudes_db(&samples);

        assert!(bins.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn columns_peak_is_expected_band() {
        let samples = sine(1000.0, 48000.0, 1.0);
        let bins = magnitudes_db(&samples);
        let cols = columns(&bins, 48000.0, 120, 20.0, 20000.0);

        let ratio = 20000.0f32 / 20.0;
        let expected = ((1000.0f32 / 20.0).ln() / ratio.ln() * 120.0).floor() as usize;
        let far = ((100.0f32 / 20.0).ln() / ratio.ln() * 120.0).floor() as usize;

        let mut peak = 0usize;
        for (i, &v) in cols.iter().enumerate() {
            if v > cols[peak] {
                peak = i;
            }
        }

        assert_eq!(peak, expected);
        assert!(cols[expected] - cols[far] >= 40.0);
    }

    #[test]
    fn columns_past_nyquist_are_floor() {
        let samples = sine(1000.0, 8000.0, 1.0);
        let bins = magnitudes_db(&samples);
        let cols = columns(&bins, 8000.0, 10, 20.0, 8000.0);

        assert_eq!(cols[cols.len() - 1], FLOOR_DB);
    }

    #[test]
    fn smoother_rises_at_once_and_falls_slowly() {
        let mut s = Smoother::new(1);

        s.step(&[-10.0], 0.0, 40.0);
        assert_eq!(s.values()[0], -10.0);

        s.step(&[FLOOR_DB], 0.5, 40.0);
        assert!((s.values()[0] - (-30.0)).abs() <= 0.01);

        s.step(&[FLOOR_DB], 10.0, 40.0);
        assert_eq!(s.values()[0], FLOOR_DB);
    }
}
