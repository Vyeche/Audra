//! Room EQ for a pair of studio speakers (issue #29): per side up to 8
//! EQ bands, a delay and a level trim, for what the computer plays to them.
//! No allocation and no locks inside `process`.
//!
//! Speaker safety: nothing steps (gains, frequencies and Q glide, a delay
//! change crossfades, on and off crossfade), and the speakers never get
//! more than `MAX_BOOST_DB` louder at any frequency, during a glide too
//! (`RoomEq::cap_now`). Off is bit-for-bit untouched.
//!
//! Drafted by the local Qwen (Strata) to Claude's spec; checked, fixed
//! (the delay read the oldest sample, the cap lagged a glide by up to
//! 6 dB, a delay retarget mid-fade jumped, the cap's grid missed narrow
//! peaks) and tested by Claude.

use std::f64::consts::PI;

pub const ROOM_BANDS: usize = 8;
pub const MAX_BOOST_DB: f32 = 6.0;    // a band's gain is clamped to [MIN_CUT_DB, MAX_BOOST_DB]
pub const MIN_CUT_DB: f32 = -18.0;
pub const MAX_DELAY_MS: f32 = 20.0;   // delay clamped to [0, MAX_DELAY_MS]
pub const TRIM_MIN_DB: f32 = -12.0;   // trim clamped to [TRIM_MIN_DB, 0.0]: it only turns a side down
pub const MIN_HZ: f32 = 20.0;
pub const MAX_HZ: f32 = 20_000.0;
pub const MIN_Q: f32 = 0.3;
pub const MAX_Q: f32 = 10.0;

/// Glide rate for gain / trim / headroom going down, in dB per 32-sample
/// step (375 dB/s at 48 kHz).
const GLIDE_DB_PER_STEP: f32 = 0.25;
/// Going up, a fifth of that (75 dB/s): bands rising together add up, and
/// "Make it flat" over stacked cuts and a trim rose about 48 dB in 50 ms
/// (the review). Turning down quickly is never the risk.
const RISE_DB_PER_STEP: f32 = 0.05;
/// A shelf's Q at most: over about 0.71 the cookbook shelf overshoots into
/// a resonant bump past its own gain (the review: a -18 dB low shelf at
/// Q 10 peaked at +18.9 dB), so a shelf's width stops there.
pub const MAX_SHELF_Q: f32 = 0.71;
/// The cap aims this far under `MAX_BOOST_DB`, so the search's own small
/// misses between its points (round 5: up to 0.13 dB with two close
/// narrow peaks) stay under the line.
const CAP_MARGIN_DB: f64 = 0.15;

/// One glide step from `cur` toward `tgt` in dB: up by at most `rise`
/// (each riser's share of `RISE_DB_PER_STEP`), down by at most `down`.
fn glide_db(cur: f64, tgt: f64, rise: f64, down: f64) -> f64 {
    let d = tgt - cur;
    let maxd = if d > 0.0 { rise } else { down };
    if d.abs() <= maxd { tgt } else { cur + d.signum() * maxd }
}

/// `p` with nothing done but kept on: every band off, no delay, no trim.
/// Switching off, the settings glide here first (`RoomEq::glide_step`).
fn flat_of(p: &RoomEqParams) -> RoomEqParams {
    let mut f = *p;
    for side in f.sides.iter_mut() {
        side.delay_ms = 0.0;
        side.trim_db = 0.0;
        for b in side.bands.iter_mut() {
            b.on = false;
        }
    }
    f
}
/// Glide rate for frequency, in octaves (log2) per 32-sample step.
const GLIDE_OCT_PER_STEP: f64 = 0.05;
/// Glide rate for Q, multiplicative fraction per 32-sample step.
const GLIDE_Q_FRAC: f64 = 0.02;
/// How often (in samples) the glide / coefficient recompute runs.
const STEP_SAMPLES: usize = 32;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BandKind { #[default] Peak, LowShelf, HighShelf }

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomBand { pub on: bool, pub kind: BandKind, pub freq_hz: f32, pub gain_db: f32, pub q: f32 }

impl Default for RoomBand {
    fn default() -> Self {
        RoomBand { on: false, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 0.0, q: 1.0 }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomSide { pub bands: [RoomBand; ROOM_BANDS], pub delay_ms: f32, pub trim_db: f32 }

impl Default for RoomSide {
    fn default() -> Self {
        RoomSide { bands: [RoomBand::default(); ROOM_BANDS], delay_ms: 0.0, trim_db: 0.0 }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RoomEqParams { pub on: bool, pub sides: [RoomSide; 2] }

impl RoomEqParams {
    /// Every value clamped to its range; a NaN or infinite value becomes that field's default.
    pub fn sanitized(&self) -> Self {
        let mut out = self.clone();
        for side in out.sides.iter_mut() {
            for band in side.bands.iter_mut() {
                // kind is an enum: leave as-is (never NaN).
                band.freq_hz = clamp_f32(band.freq_hz, MIN_HZ, MAX_HZ, 1000.0);
                band.gain_db = clamp_f32(band.gain_db, MIN_CUT_DB, MAX_BOOST_DB, 0.0);
                let max_q = if band.kind == BandKind::Peak { MAX_Q } else { MAX_SHELF_Q };
                band.q = clamp_f32(band.q, MIN_Q, max_q, max_q.min(1.0));
            }
            side.delay_ms = clamp_f32(side.delay_ms, 0.0, MAX_DELAY_MS, 0.0);
            side.trim_db = clamp_f32(side.trim_db, TRIM_MIN_DB, 0.0, 0.0);
        }
        out
    }

    /// True when processing would change nothing: off, or every band off or at 0 dB,
    /// and every delay 0 and every trim 0.
    pub fn is_flat(&self) -> bool {
        if !self.on { return true; }
        for side in self.sides.iter() {
            if side.delay_ms != 0.0 || side.trim_db != 0.0 { return false; }
            for band in side.bands.iter() {
                if band.on && band.gain_db != 0.0 { return false; }
            }
        }
        true
    }
}

/// Clamp a value to [lo, hi]; NaN / infinite becomes `default`.
fn clamp_f32(v: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if v.is_nan() || v.is_infinite() { return default; }
    if v < lo { lo } else if v > hi { hi } else { v }
}

/// The magnitude response in dB of one side's bands at `freq_hz` (bands that are off or at
/// 0 dB contribute 0), plus its trim. Uses the same biquad coefficients as processing.
pub fn side_response_db(side: &RoomSide, freq_hz: f32, sample_rate: f32) -> f32 {
    let mut total = 0.0f64;
    for band in side.bands.iter() {
        if !band.on || band.gain_db == 0.0 { continue; }
        let c = coeffs(band.kind, band.freq_hz as f64, band.gain_db as f64, band.q as f64, sample_rate as f64);
        total += mag_db(&c, freq_hz as f64, sample_rate as f64);
    }
    (total + side.trim_db as f64) as f32
}

// ---------------------------------------------------------------------------
// Biquad coefficients (RBJ cookbook). Transposed Direct Form II.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Coeffs { b0: f64, b1: f64, b2: f64, a1: f64, a2: f64 }

fn coeffs(kind: BandKind, freq_hz: f64, gain_db: f64, q: f64, sample_rate: f64) -> Coeffs {
    let w0 = 2.0 * PI * freq_hz / sample_rate;
    let cosw = w0.cos();
    let sinw = w0.sin();
    // Cookbook Q form for all three kinds: alpha = sin(w0)/(2*q).
    let alpha = sinw / (2.0 * q);
    let a = 10f64.powf(gain_db / 40.0);

    let (b0, b1, b2, a0, a1, a2) = match kind {
        BandKind::Peak => {
            // peakingEQ
            (1.0 + alpha * a, -2.0 * cosw, 1.0 - alpha * a,
             1.0 + alpha / a, -2.0 * cosw, 1.0 - alpha / a)
        }
        BandKind::LowShelf => {
            // lowShelf (Q form)
            let sq = 2.0 * a.sqrt() * alpha;
            (a * ((a + 1.0) - (a - 1.0) * cosw + sq),
             2.0 * a * ((a - 1.0) - (a + 1.0) * cosw),
             a * ((a + 1.0) - (a - 1.0) * cosw - sq),
             (a + 1.0) + (a - 1.0) * cosw + sq,
             -2.0 * ((a - 1.0) + (a + 1.0) * cosw),
             (a + 1.0) + (a - 1.0) * cosw - sq)
        }
        BandKind::HighShelf => {
            // highShelf (Q form)
            let sq = 2.0 * a.sqrt() * alpha;
            (a * ((a + 1.0) + (a - 1.0) * cosw + sq),
             -2.0 * a * ((a - 1.0) + (a + 1.0) * cosw),
             a * ((a + 1.0) + (a - 1.0) * cosw - sq),
             (a + 1.0) - (a - 1.0) * cosw + sq,
             2.0 * ((a - 1.0) - (a + 1.0) * cosw),
             (a + 1.0) - (a - 1.0) * cosw - sq)
        }
    };

    Coeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

/// Magnitude (dB) of the biquad at `freq_hz`.
fn mag_db(c: &Coeffs, freq_hz: f64, sample_rate: f64) -> f64 {
    let w = 2.0 * PI * freq_hz / sample_rate;
    let cw = w.cos();
    let sw = w.sin();
    // Real/imag of numerator and denominator (z^-1 = cos - j sin).
    let nr = c.b0 + c.b1 * cw + c.b2 * (2.0 * cw * cw - 1.0);
    let ni = -(c.b1 * sw + c.b2 * 2.0 * cw * sw);
    let dr = 1.0 + c.a1 * cw + c.a2 * (2.0 * cw * cw - 1.0);
    let di = -(c.a1 * sw + c.a2 * 2.0 * cw * sw);
    let num = nr * nr + ni * ni;
    let den = dr * dr + di * di;
    if den <= 0.0 { return 0.0; }
    10.0 * (num / den).log10()
}

// ---------------------------------------------------------------------------
// Runtime state for one band: current values, coefficients and TDF-II state.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct BandState {
    on: bool,          // whether the filter is currently active
    kind: BandKind,
    freq_hz: f64,
    gain_db: f64,
    q: f64,
    coeffs: Coeffs,
    z1: f64,           // TDF-II state (input-delayed)
    z2: f64,           // TDF-II state (output-delayed)
}

impl BandState {
    fn from(band: &RoomBand) -> Self {
        let c = coeffs(band.kind, band.freq_hz as f64, band.gain_db as f64, band.q as f64, 1.0); // placeholder; recomputed in new()
        BandState {
            on: band.on, kind: band.kind, freq_hz: band.freq_hz as f64, gain_db: band.gain_db as f64, q: band.q as f64,
            coeffs: c, z1: 0.0, z2: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime state for one side: bands + delay line + trim/headroom gains.
// ---------------------------------------------------------------------------

struct SideState {
    bands: [BandState; ROOM_BANDS],
    // Delay ring buffer (preallocated).
    buf: Vec<f64>,
    cap: usize,
    write_idx: usize,
    // Current / target delay in samples and crossfade state.
    cur_d: f64,
    tgt_d: f64,
    dxfade: i64,       // remaining crossfade samples (0 = settled)
    dxfade_len: i64,   // total crossfade length
    // Gains (linear), current and target.
    cur_trim: f64,
    tgt_trim: f64,
    cur_head: f64,
    tgt_head: f64,
}

impl SideState {
    fn new(side: &RoomSide, sample_rate: f64) -> Self {
        let cap = ((MAX_DELAY_MS as f64 / 1000.0) * sample_rate) as usize + 2;
        let mut bands = [BandState::from(&side.bands[0]); ROOM_BANDS];
        for i in 0..ROOM_BANDS {
            bands[i] = BandState::from(&side.bands[i]);
            bands[i].coeffs = coeffs(side.bands[i].kind, side.bands[i].freq_hz as f64,
                                     side.bands[i].gain_db as f64, side.bands[i].q as f64, sample_rate);
        }
        let d0 = ((side.delay_ms as f64 / 1000.0) * sample_rate).round();
        SideState {
            bands,
            buf: vec![0.0f64; cap],
            cap,
            write_idx: 0,
            cur_d: d0, tgt_d: d0,
            dxfade: 0, dxfade_len: ((0.02 * sample_rate) as i64).max(1),
            cur_trim: db_to_lin(side.trim_db as f64),
            tgt_trim: db_to_lin(side.trim_db as f64),
            cur_head: 1.0, tgt_head: 1.0,
        }
    }

    fn reset(&mut self) {
        for b in self.bands.iter_mut() { b.z1 = 0.0; b.z2 = 0.0; }
        for s in self.buf.iter_mut() { *s = 0.0; }
        self.write_idx = 0;
    }

    /// Read the delay line with linear interpolation at fractional position `pos` samples
    /// behind the write head (pos >= 0).
    fn read_delay(&self, pos: f64) -> f64 {
        let cap = self.cap as f64;
        let p = pos.max(0.0);
        let i0 = p.floor() as usize;
        let frac = p - i0 as f64;
        let wi = self.write_idx as f64;
        // index of the sample `i` steps behind the write head (mod cap).
        let idx = |k: usize| -> usize {
            // The newest sample is one behind the write head (it was
            // written before the read).
            let mut v = wi - 1.0 - k as f64;
            while v < 0.0 { v += cap; }
            v as usize % self.cap
        };
        let a = self.buf[idx(i0)];
        let b = self.buf[idx(i0 + 1)];
        a * (1.0 - frac) + b * frac
    }

    fn write_delay(&mut self, x: f64) {
        self.buf[self.write_idx] = x;
        self.write_idx += 1;
        if self.write_idx >= self.cap { self.write_idx = 0; }
    }
}

fn db_to_lin(db: f64) -> f64 { 10f64.powf(db / 20.0) }

// ---------------------------------------------------------------------------
// The RoomEq engine.
// ---------------------------------------------------------------------------

pub struct RoomEq {
    sample_rate: f64,
    cur: RoomEqParams,   // current (gliding) parameters
    tgt: RoomEqParams,   // target parameters (sanitized)
    sides: [SideState; 2],
    headroom_db: f64,    // current applied headroom (0 or negative)
    tgt_headroom_db: f64,// target headroom
    on_cur: f64,         // dry/wet crossfade weight of processed signal (1 = fully wet)
    on_tgt: f64,         // 1 when on, 0 when off
    fade_len: i64,       // length of the on/off crossfade in samples
    step_count: usize,   // counts samples toward the next 32-sample glide step
    /// Whether the last call had a right side: in mono, side 1 is never
    /// processed, so its delay crossfade must not hold "flat" back (round
    /// 3: a mono loop with a right delay never finished switching off).
    stereo: bool,
    /// The per-step rates are set at 48 kHz: this scales them so they hold
    /// per second at any rate (round 4: at 96 kHz everything glided twice
    /// as fast, 7.5 dB in 50 ms).
    rate_k: f64,
}

impl RoomEq {
    pub fn new(sample_rate: f32, params: RoomEqParams) -> Self {
        let sr = sample_rate as f64;
        let tgt = params.sanitized();
        // Starting off, the settings start flat, so switching on glides up
        // to them (round 3: it faded straight into them, +6 dB in 50 ms).
        let cur = if tgt.on { tgt } else { flat_of(&tgt) };
        let sides = [SideState::new(&cur.sides[0], sr), SideState::new(&cur.sides[1], sr)];
        let mut eq = RoomEq {
            sample_rate: sr,
            cur, tgt,
            sides,
            headroom_db: 0.0, tgt_headroom_db: 0.0,
            on_cur: if cur.on { 1.0 } else { 0.0 },
            on_tgt: if cur.on { 1.0 } else { 0.0 },
            fade_len: ((0.05 * sr) as i64).max(1),
            step_count: 0,
            stereo: true,
            rate_k: 48_000.0 / sr,
        };
        eq.recompute_headroom();
        // Starting, the headroom is already what the settings need (the
        // draft left it at 0 dB, so a narrow pair of peaks played +12).
        // Started off, nothing plays through it yet: no headroom until on
        // (round 4: the first switch-on dipped by it).
        eq.headroom_db = if eq.tgt.on { eq.tgt_headroom_db } else { 0.0 };
        // Seed the side headroom gains to match.
        let h = db_to_lin(eq.headroom_db);
        for s in eq.sides.iter_mut() { s.cur_head = h; s.tgt_head = h; }
        eq
    }

    /// New target settings. Never a step: everything glides toward it.
    pub fn set(&mut self, params: RoomEqParams) {
        let tgt = params.sanitized();
        // A band that is switched on starts from 0 dB gain (current gain 0).
        for side in 0..2 {
            for i in 0..ROOM_BANDS {
                if !self.cur.sides[side].bands[i].on && tgt.sides[side].bands[i].on {
                    self.cur.sides[side].bands[i].gain_db = 0.0;
                    self.sides[side].bands[i].gain_db = 0.0;
                }
            }
        }
        self.tgt = tgt;
        self.on_tgt = if tgt.on { 1.0 } else { 0.0 };
        self.recompute_headroom();
    }

    /// Compute the target headroom from the current *target* params (max boost over 64 log-spaced freqs).
    fn recompute_headroom(&mut self) {
        let max_boost = max_boost_db_sides(&self.tgt, self.sample_rate, if self.stereo { 2 } else { 1 });
        let line = MAX_BOOST_DB as f64 - CAP_MARGIN_DB;
        self.tgt_headroom_db = if max_boost > line { line - max_boost } else { 0.0 };
    }

    /// The automatic headroom in dB currently applied (0 or negative).
    pub fn headroom_db(&self) -> f32 { self.headroom_db as f32 }

    /// Clears filter and delay state (silence in, silence out).
    pub fn reset(&mut self) {
        for s in self.sides.iter_mut() { s.reset(); }
    }

    /// In place. `right` None: mono, side 0 only.
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>) {
        let n = left.len();
        // Mono or stereo decides which sides the cap counts: worked out
        // again when it changes (round 5: a mono loop kept the two-sided
        // headroom from its start until the next edit).
        if self.stereo != right.is_some() {
            self.stereo = right.is_some();
            self.recompute_headroom();
        }

        // Safety: any NaN / infinite input -> reset and write silence for this whole call.
        let mut bad = false;
        for i in 0..n { if !left[i].is_finite() { bad = true; break; } }
        if !bad {
            if let Some(r) = right.as_deref() {
                for i in 0..n { if !r[i].is_finite() { bad = true; break; } }
            }
        }
        if bad {
            self.reset();
            for i in 0..n { left[i] = 0.0; }
            if let Some(r) = right.as_deref_mut() { for i in 0..n { r[i] = 0.0; } }
            return;
        }

        // Fully off and crossfade finished: leave buffers bit-for-bit untouched,
        // but keep the delay lines fed so switching on later is smooth.
        if self.on_cur == 0.0 && self.on_tgt == 0.0 {
            for i in 0..n {
                self.sides[0].write_delay(left[i] as f64);
                if let Some(r) = right.as_deref() { self.sides[1].write_delay(r[i] as f64); }
            }
            return;
        }

        for i in 0..n {
            // Glide / recompute every STEP_SAMPLES.
            self.step_count += 1;
            if self.step_count >= STEP_SAMPLES {
                self.step_count = 0;
                self.glide_step();
            }

            let wet0 = self.process_side(0, left[i] as f64);
            let out0 = (1.0 - self.on_cur) * left[i] as f64 + self.on_cur * wet0;
            left[i] = clamp_sample(out0);

            if let Some(r) = right.as_deref_mut() {
                let wet1 = self.process_side(1, r[i] as f64);
                let out1 = (1.0 - self.on_cur) * r[i] as f64 + self.on_cur * wet1;
                r[i] = clamp_sample(out1);
            }
        }
    }

    /// Process one sample through side `s`, including delay, bands, trim and headroom.
    fn process_side(&mut self, s: usize, x: f64) -> f64 {
        let sr = self.sample_rate;
        let st = &mut self.sides[s];

        // Feed the delay line with the raw input (delay is applied to the signal path).
        st.write_delay(x);

        // Read the delayed sample, crossfading between old and new delay if needed.
        let y = if st.dxfade > 0 {
            let t = (st.dxfade_len - st.dxfade) as f64 / st.dxfade_len as f64;
            let a = st.read_delay(st.cur_d);
            let b = st.read_delay(st.tgt_d);
            st.dxfade -= 1;
            if st.dxfade == 0 { st.cur_d = st.tgt_d; }
            a * (1.0 - t) + b * t
        } else {
            st.read_delay(st.cur_d)
        };

        // EQ bands (TDF-II). Skip inactive / 0 dB bands.
        let mut v = y;
        for b in st.bands.iter_mut() {
            if !b.on || b.gain_db == 0.0 { continue; }
            let c = &b.coeffs;
            let out = c.b0 * v + b.z1;
            b.z1 = c.b1 * v - c.a1 * out + b.z2;
            b.z2 = c.b2 * v - c.a2 * out;
            v = out;
        }

        // Trim and headroom gains.
        v *= st.cur_trim * st.cur_head;
        let _ = sr;
        v
    }

    /// One 32-sample glide step: move current toward target, recompute changed coefficients.
    /// Whether it has finished switching off: the sound passes untouched
    /// (a loop may go then; `dynamics_panel::ensure`).
    pub fn is_idle(&self) -> bool {
        self.on_cur == 0.0 && self.on_tgt == 0.0
    }

    /// Whether the gliding settings change nothing: every band off, no
    /// trim, no headroom, no delay, none crossfading.
    fn cur_is_flat(&self) -> bool {
        let sides = if self.stereo { 2 } else { 1 };
        self.headroom_db == 0.0
            && (0..sides).all(|s| {
                let st = &self.sides[s];
                self.cur.sides[s].trim_db == 0.0 && st.cur_d == 0.0 && st.dxfade == 0 && self.cur.sides[s].bands.iter().all(|b| !b.on)
            })
    }

    /// The gain a band is heading for this step: its target's, or 0 dB
    /// while it goes off or changes kind.
    fn heading_db(cur_b: &RoomBand, goal_b: &RoomBand) -> f64 {
        if goal_b.on && (cur_b.kind == goal_b.kind || !cur_b.on) { goal_b.gain_db as f64 } else { 0.0 }
    }

    /// One 32-sample glide step: move current toward target, recompute changed coefficients.
    fn glide_step(&mut self) {
        // Switching on, the settings stay flat until the crossfade is done:
        // a linear crossfade lands its level late, and rising bands under it
        // added up (4.5 dB in 50 ms).
        let off = self.on_tgt == 0.0 || self.on_cur < self.on_tgt;
        // Where the settings head: the page's, or flat while switching off,
        // so the speakers get their level back at the rise rate, not in the
        // 50 ms crossfade (round 2: 30 dB in 50 ms with a trim and a cut).
        let goal = if off { flat_of(&self.tgt) } else { self.tgt };
        let goal_head = if off { 0.0 } else { self.tgt_headroom_db };
        // One rise budget for everything rising now, shared: rising one at
        // a time each, bands, trims and the headroom stacked (round 2: eight
        // cuts and a trim rose 34 dB in 50 ms).
        // Per speaker (round 3: shared by both, a deep set took 4.2 s to
        // go flat, past `CLOSE_MAX`); a band whose frequency or width moves
        // counts too, since moving a cut away gives its level back.
        // Each changing parameter is its own riser (round 4: a band's
        // gain, frequency and width each took a full share in one step).
        let head_up = goal_head > self.headroom_db;
        let mut rising = [0usize; 2];
        for (s, n) in rising.iter_mut().enumerate() {
            *n += usize::from(head_up) + usize::from(goal.sides[s].trim_db > self.cur.sides[s].trim_db);
            for i in 0..ROOM_BANDS {
                let (c, g) = (&self.cur.sides[s].bands[i], &goal.sides[s].bands[i]);
                let now = if c.on { c.gain_db as f64 } else { 0.0 };
                let shaped = c.on && c.gain_db != 0.0 && g.on;
                *n += usize::from((c.on || g.on) && Self::heading_db(c, g) > now)
                    + usize::from(shaped && c.freq_hz != g.freq_hz)
                    + usize::from(shaped && c.q != g.q);
            }
        }
        let k = self.rate_k;
        let down = GLIDE_DB_PER_STEP as f64 * k;
        let rise_of = [RISE_DB_PER_STEP as f64 * k / rising[0].max(1) as f64, RISE_DB_PER_STEP as f64 * k / rising[1].max(1) as f64];
        let head_rise = if self.stereo { rise_of[0].min(rise_of[1]) } else { rise_of[0] };

        // On at once (the settings are flat then and rise after); off only
        // once they're flat, so the crossfade is between two equal sounds.
        if self.on_cur != self.on_tgt {
            // A glide step comes every STEP_SAMPLES, so the fade moves that
            // many samples' worth: 50 ms in all (the review: stepped by one
            // sample's worth, it took 1.6 s).
            let step = STEP_SAMPLES as f64 / self.fade_len as f64;
            if self.on_tgt > self.on_cur {
                self.on_cur = (self.on_cur + step).min(1.0);
            } else if self.cur_is_flat() {
                self.on_cur = (self.on_cur - step).max(0.0);
            }
        }

        if self.headroom_db != goal_head {
            self.headroom_db = glide_db(self.headroom_db, goal_head, head_rise, down);
        }

        let mut moved = false;
        for s in 0..2 {
            let rise = rise_of[s];
            let st = &mut self.sides[s];

            // A new delay starts its crossfade only once the last one has
            // finished, so a drag never moves the far end mid-fade (a jump).
            // Whole samples (about 7 mm each): read between two, the
            // delayed side lost up to 10 dB at the top (the review: 1.2 ms
            // is 57.6 samples, -5.5 dB at 16 kHz).
            let tgt_d = ((goal.sides[s].delay_ms as f64 / 1000.0) * self.sample_rate).round();
            if s == 1 && !self.stereo {
                // Not processed in mono: no crossfade to wait for.
                st.cur_d = tgt_d;
                st.tgt_d = tgt_d;
                st.dxfade = 0;
            } else if st.dxfade == 0 && st.cur_d != tgt_d {
                st.tgt_d = tgt_d;
                st.dxfade = st.dxfade_len;
            }

            if self.cur.sides[s].trim_db != goal.sides[s].trim_db {
                self.cur.sides[s].trim_db = glide_db(self.cur.sides[s].trim_db as f64, goal.sides[s].trim_db as f64, rise, down) as f32;
            }
            st.cur_trim = db_to_lin(self.cur.sides[s].trim_db as f64);
            st.tgt_trim = db_to_lin(goal.sides[s].trim_db as f64);
            st.tgt_head = db_to_lin(goal_head);

            for i in 0..ROOM_BANDS {
                let cur_b = &mut self.cur.sides[s].bands[i];
                let tgt_b = &goal.sides[s].bands[i];
                let bs = &mut st.bands[i];
                // A narrow low band settles slowly (its bandwidth f0/Q is a
                // few hertz), so what you hear lags its gain, then catches up
                // faster than the gain moved: its gain glides slower in step
                // (round 5: a -18 dB Q 10 cut at 20 Hz came back 6 dB in
                // 50 ms through the filter itself).
                // At least half speed (round 6: slower still, eight deep
                // cuts at 20-45 Hz took 7.3 s to switch off, past the close
                // limit, and the bass came back at once).
                let settle = (0.03 * PI * cur_b.freq_hz as f64 / (cur_b.q as f64).max(MIN_Q as f64)).clamp(0.5, 1.0);
                let rise_g = rise * settle;

                if !cur_b.on && tgt_b.on {
                    // Switched on: from 0 dB, its shape as set.
                    cur_b.on = true;
                    cur_b.kind = tgt_b.kind;
                    cur_b.freq_hz = tgt_b.freq_hz;
                    cur_b.q = tgt_b.q;
                    cur_b.gain_db = 0.0;
                } else if cur_b.on && !tgt_b.on {
                    // Going off: only its gain moves, to 0 dB, then it's off
                    // (round 2: with a kind change too it glided twice).
                    cur_b.gain_db = glide_db(cur_b.gain_db as f64, 0.0, rise_g, down) as f32;
                    if cur_b.gain_db == 0.0 {
                        cur_b.on = false;
                    }
                } else if cur_b.on {
                    if cur_b.kind != tgt_b.kind {
                        // To 0 dB, where the filter does nothing, then the
                        // new kind with its own width at once: a width still
                        // gliding from a narrow bell made a shelf ring past
                        // the cap (round 2: +6.76 dB at 19.6 kHz).
                        cur_b.gain_db = glide_db(cur_b.gain_db as f64, 0.0, rise_g, down) as f32;
                        if cur_b.gain_db == 0.0 {
                            cur_b.kind = tgt_b.kind;
                            cur_b.q = tgt_b.q;
                        }
                    }
                    // A band with gain moves only as fast as its share of
                    // the rise allows: moved by an octave fraction d, a
                    // band of gain g and width Q changes any frequency by
                    // at most about 3·g·Q·d dB, and a width change by a
                    // fraction f by about g·f (round 3: a -18 dB shelf
                    // dragged across gave 17.7 dB back in 50 ms).
                    // Near the top the digital filter's frequency is warped
                    // (an octave there moves its curve further), so the
                    // move slows by sin(w0)/w0 (round 4: a -18 dB bell from
                    // 20 to 14 kHz gave 8.4 dB in 50 ms at 44.1 kHz).
                    let g = (cur_b.gain_db as f64).abs();
                    let w0 = 2.0 * PI * cur_b.freq_hz as f64 / self.sample_rate;
                    let warp = (w0.sin() / w0).max(0.05);
                    let oct_cap = GLIDE_OCT_PER_STEP * k;
                    let q_cap = GLIDE_Q_FRAC * k;
                    let max_oct = if g > 0.0 { (rise * warp / (3.0 * g * (cur_b.q as f64).max(0.7))).min(oct_cap) } else { oct_cap };
                    let max_q = if g > 0.0 { (rise / g).min(q_cap) } else { q_cap };
                    // A far move at that pace would crawl (round 4: 72 s to
                    // sweep a deep notch across): instead its gain goes to
                    // 0 dB, it jumps there silent, and its gain comes back,
                    // as a kind change does.
                    // Judged at the larger of its gain now and its target's,
                    // so the choice can't flip as the gain dips and comes
                    // back (it hovered at -1.8 dB, never arriving).
                    let g_far = g.max((tgt_b.gain_db as f64).abs());
                    let far_oct = (rise * warp / (3.0 * g_far * (cur_b.q as f64).max(0.7))).min(oct_cap);
                    let far_q = (rise / g_far.max(1e-9)).min(q_cap);
                    let steps = (tgt_b.freq_hz as f64 / cur_b.freq_hz as f64).log2().abs() / far_oct
                        + (tgt_b.q as f64 / cur_b.q as f64).ln().abs() / far_q.ln_1p();
                    let far = g > 0.0 && steps > 0.5 * self.sample_rate / STEP_SAMPLES as f64;
                    if far {
                        cur_b.gain_db = glide_db(cur_b.gain_db as f64, 0.0, rise_g, down) as f32;
                        if cur_b.gain_db == 0.0 {
                            cur_b.freq_hz = tgt_b.freq_hz;
                            cur_b.q = tgt_b.q;
                        }
                    }
                    if !far && cur_b.freq_hz != tgt_b.freq_hz {
                        let cf = cur_b.freq_hz as f64;
                        let tf = tgt_b.freq_hz as f64;
                        let loct = (tf / cf).log2();
                        if loct.abs() <= max_oct { cur_b.freq_hz = tgt_b.freq_hz; }
                        else { cur_b.freq_hz = (cf * 2f64.powf(loct.signum() * max_oct)) as f32; }
                    }
                    // The width and the gain only once the kind is settled.
                    if cur_b.kind == tgt_b.kind && !far {
                        if cur_b.q != tgt_b.q {
                            let cq = cur_b.q as f64;
                            let ratio = tgt_b.q as f64 / cq;
                            if (ratio - 1.0).abs() <= max_q { cur_b.q = tgt_b.q; }
                            else { cur_b.q = (cq * (1.0 + (ratio - 1.0).signum() * max_q)) as f32; }
                        }
                        if cur_b.gain_db != tgt_b.gain_db {
                            cur_b.gain_db = glide_db(cur_b.gain_db as f64, tgt_b.gain_db as f64, rise_g, down) as f32;
                        }
                    }
                }

                if !cur_b.on {
                    bs.on = false;
                    continue;
                }
                bs.on = true;
                let changed = bs.kind != cur_b.kind
                    || bs.freq_hz != cur_b.freq_hz as f64
                    || bs.gain_db != cur_b.gain_db as f64
                    || bs.q != cur_b.q as f64;
                if changed {
                    moved = true;
                    bs.kind = cur_b.kind;
                    bs.freq_hz = cur_b.freq_hz as f64;
                    bs.gain_db = cur_b.gain_db as f64;
                    bs.q = cur_b.q as f64;
                    bs.coeffs = coeffs(cur_b.kind, cur_b.freq_hz as f64, cur_b.gain_db as f64, cur_b.q as f64, self.sample_rate);
                }
            }
        }
        // The cap holds during a glide too: bands rising together outrun
        // the headroom's own glide, so whenever the filters moved the
        // headroom is at least what the current settings need, at once (a
        // turn-down, never a jump up).
        if moved {
            self.cap_now();
        }
        let h = db_to_lin(self.headroom_db);
        for st in self.sides.iter_mut() {
            st.cur_head = h;
        }
    }

    /// Lowers the headroom at once to what the current (gliding) settings
    /// need to stay within `MAX_BOOST_DB`.
    fn cap_now(&mut self) {
        // In mono only the left side plays (round 4: right-side boosts
        // turned the left down).
        let need = MAX_BOOST_DB as f64 - CAP_MARGIN_DB - max_boost_db_sides(&self.cur, self.sample_rate, if self.stereo { 2 } else { 1 });
        if need < self.headroom_db {
            self.headroom_db = need;
        }
    }
}

/// The most either side's bands lift any frequency, in dB (trim left out).
/// 64 frequencies log-spaced over the audible range, then a closer look
/// (17 points over a grid step either way) around each high of the grid and
/// each lit band's own frequency, where a narrow peak tops out between grid
/// points. No allocation and each band's coefficients once: it runs in the
/// audio loop every 32 samples while a glide moves (round 2: a Vec per side
/// and coefficients per frequency took longer than the step).
pub fn max_boost_db(p: &RoomEqParams, sample_rate: f64) -> f64 {
    max_boost_db_sides(p, sample_rate, 2)
}

/// `max_boost_db` over the first `sides` sides (1 in mono, where the right
/// never plays).
pub fn max_boost_db_sides(p: &RoomEqParams, sample_rate: f64, sides: usize) -> f64 {
    // From 1 Hz right up to half the sample rate, and DC: a shelf at 20 Hz
    // lifts everything below it, and one at 20 kHz everything above, most
    // at the very ends (round 3: +12 dB at 5 Hz and 23 kHz went unseen;
    // round 4: stopping at 0.49 of the rate missed up to +14 dB at the top).
    const GRID: usize = 96;
    const LOW_HZ: f64 = 1.0;
    let top = 0.5 * sample_rate;
    let span = (top / LOW_HZ).log2();
    let step = span / (GRID - 1) as f64;
    let mut max_boost = 0.0f64;
    for side in p.sides.iter().take(sides) {
        let mut lit: [Option<Coeffs>; ROOM_BANDS] = [None; ROOM_BANDS];
        let mut centres = [0.0f64; ROOM_BANDS];
        let mut n = 0;
        for (i, b) in side.bands.iter().enumerate() {
            if b.on && b.gain_db != 0.0 {
                lit[i] = Some(coeffs(b.kind, b.freq_hz as f64, b.gain_db as f64, b.q as f64, sample_rate));
                centres[n] = (b.freq_hz as f64 / LOW_HZ).log2();
                n += 1;
            }
        }
        if n == 0 {
            continue;
        }
        let at = |oct: f64| -> f64 {
            let f = LOW_HZ * oct.clamp(0.0, span).exp2();
            lit.iter().flatten().map(|c| mag_db(c, f, sample_rate)).sum()
        };
        // A look at 17 points over a grid step either way, then the true
        // high near the best of them, found by golden-section search (round
        // 6: a second fixed look still missed eight narrow bells stacked
        // close near 20 kHz by 0.75 dB at 44.1 kHz; fixed points can always
        // fall between, the search can't).
        let closer = |oct: f64, max: &mut f64| {
            let (mut best, mut at_best) = (f64::NEG_INFINITY, oct);
            for k in -8..=8 {
                let o = oct + step * k as f64 / 8.0;
                let r = at(o);
                if r > best {
                    best = r;
                    at_best = o;
                }
            }
            let phi = 0.5 * (5f64.sqrt() - 1.0);
            let (mut a, mut b) = (at_best - step / 8.0, at_best + step / 8.0);
            let (mut c, mut d) = (b - phi * (b - a), a + phi * (b - a));
            let (mut fc, mut fd) = (at(c), at(d));
            for _ in 0..28 {
                if fc > fd {
                    b = d;
                    d = c;
                    fd = fc;
                    c = b - phi * (b - a);
                    fc = at(c);
                } else {
                    a = c;
                    c = d;
                    fc = fd;
                    d = a + phi * (b - a);
                    fd = at(d);
                }
            }
            *max = max.max(best).max(fc).max(fd);
        };
        max_boost = max_boost.max(lit.iter().flatten().map(|c| mag_db(c, 0.0, sample_rate)).sum());
        let mut grid = [0.0f64; GRID];
        for (k, g) in grid.iter_mut().enumerate() {
            *g = at(step * k as f64);
            max_boost = max_boost.max(*g);
        }
        for k in 0..GRID {
            let left = if k > 0 { grid[k - 1] } else { f64::NEG_INFINITY };
            let right = if k + 1 < GRID { grid[k + 1] } else { f64::NEG_INFINITY };
            if grid[k] >= left && grid[k] >= right {
                closer(step * k as f64, &mut max_boost);
            }
        }
        for &c in centres[..n].iter() {
            closer(c, &mut max_boost);
        }
    }
    max_boost
}

fn clamp_sample(v: f64) -> f32 {
    if v.is_nan() || v.is_infinite() { return 0.0; }
    if v < -1.0 { -1.0 } else if v > 1.0 { 1.0 } else { v as f32 }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn default_on_params() -> RoomEqParams {
        let mut p = RoomEqParams::default();
        p.on = true;
        p
    }

    // A simple deterministic pseudo-noise generator (no external crates).
    struct Noise { state: u32 }
    impl Noise {
        fn new() -> Self { Noise { state: 0x1234_5678 } }
        fn next(&mut self) -> f32 {
            // xorshift32
            let mut x = self.state;
            x ^= x << 13; x ^= x >> 17; x ^= x << 5;
            self.state = x;
            ((x as i32) as f32 / (i32::MAX as f32)) * 0.5
        }
    }

    fn rms(x: &[f32]) -> f64 {
        let mut s = 0.0f64;
        for &v in x { s += (v as f64) * (v as f64); }
        if x.is_empty() { 0.0 } else { (s / x.len() as f64).sqrt() }
    }

    fn db(gain: f64) -> f64 { if gain <= 0.0 { -200.0 } else { 20.0 * gain.log10() } }

    #[test]
    fn flat_is_identity() {
        let p = default_on_params(); // on, all bands off, delay 0, trim 0
        assert!(p.is_flat());
        let mut eq = RoomEq::new(SR, p);
        let n = SR as usize; // 1 s
        let mut input: Vec<f32> = Vec::with_capacity(n);
        let mut noise = Noise::new();
        for _ in 0..n { input.push(noise.next()); }
        let mut left = input.clone();
        eq.process(&mut left, None);
        // After the 50 ms fade is complete (1 s processed), output == input exactly.
        assert_eq!(left, input);
    }

    #[test]
    fn off_is_bit_identical() {
        let mut p = RoomEqParams::default();
        p.on = false;
        let mut eq = RoomEq::new(SR, p);
        let n = 1024;
        let mut noise = Noise::new();
        let input: Vec<f32> = (0..n).map(|_| noise.next()).collect();
        let mut left = input.clone();
        eq.process(&mut left, None);
        assert_eq!(left, input); // from the first sample
    }

    #[test]
    fn peak_response_matches() {
        let mut side = RoomSide::default();
        side.bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        let at_1k = side_response_db(&side, 1000.0, SR);
        assert!((at_1k - 6.0).abs() <= 0.05, "at 1k got {}", at_1k);
        let at_100 = side_response_db(&side, 100.0, SR);
        assert!(at_100.abs() <= 0.5, "at 100 got {}", at_100);
        let at_10k = side_response_db(&side, 10_000.0, SR);
        assert!(at_10k.abs() <= 0.5, "at 10k got {}", at_10k);
    }

    #[test]
    fn processing_matches_response() {
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        let mut eq = RoomEq::new(SR, p);

        // Reference RMS of the sine (no processing).
        let n = SR as usize;
        let mut ref_in: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n {
            ref_in.push(0.1 * (2.0 * PI * 1000.0 * i as f64 / SR as f64).sin() as f32);
        }
        let ref_rms = rms(&ref_in);

        // Drive the sine through the EQ; after 1 s of gliding, measure RMS gain.
        let mut left = ref_in.clone();
        eq.process(&mut left, None);
        let out_rms = rms(&left);
        let gain_db = db(out_rms / ref_rms);
        assert!((gain_db - 6.0).abs() <= 0.2, "measured gain {} dB", gain_db);
    }

    #[test]
    fn delay_is_exact() {
        let mut p = default_on_params();
        p.sides[0].delay_ms = 10.0; // no bands
        let mut eq = RoomEq::new(SR, p);

        // Set the delay and process 1 s of silence so the crossfade finishes.
        let n = SR as usize;
        let mut left = vec![0.0f32; n];
        eq.process(&mut left, None);

        // Now an impulse at sample 0 must come out exactly 480 samples later.
        let m = 1024;
        let mut buf = vec![0.0f32; m];
        buf[0] = 1.0;
        eq.process(&mut buf, None);

        // Find the peak (impulse) location.
        let mut peak_idx = 0usize;
        let mut peak_val = 0.0f32;
        for i in 0..m { if buf[i].abs() > peak_val { peak_val = buf[i].abs(); peak_idx = i; } }
        assert_eq!(peak_idx, 480, "impulse appeared at {} (expected 480)", peak_idx);
    }

    #[test]
    fn boost_is_capped() {
        let mut p = default_on_params();
        for b in 0..3 {
            p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        }
        let mut eq = RoomEq::new(SR, p);

        // Steady 1 kHz sine; measure RMS in 32-sample windows after the on-fade.
        let n = SR as usize;
        let mut left: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n {
            left.push(0.1 * (2.0 * PI * 1000.0 * i as f64 / SR as f64).sin() as f32);
        }
        let ref_rms = rms(&left);

        // Process in chunks so we can inspect windows during the glide.
        let mut max_gain_db = f64::NEG_INFINITY;
        let chunk = 512;
        let mut pos = 0usize;
        while pos < n {
            let end = (pos + chunk).min(n);
            // copy window into a working buffer
            let mut w: Vec<f32> = left[pos..end].to_vec();
            eq.process(&mut w, None);
            // write back so subsequent chunks see the same signal (steady sine anyway)
            for k in 0..w.len() { left[pos + k] = w[k]; }
            if pos > (SR as usize / 20) { // after on-fade (~50 ms)
                let g = db(rms(&w) / ref_rms);
                if g > max_gain_db { max_gain_db = g; }
            }
            pos = end;
        }

        assert!((eq.headroom_db() - (-12.0)).abs() <= 0.5, "headroom {}", eq.headroom_db());
        assert!(max_gain_db <= 6.2, "max gain {} dB", max_gain_db);
    }

    #[test]
    fn no_steps() {
        // A +6 dB band switched on over a steady 1 kHz sine, on a running
        // flat EQ: whole-cycle windows (48 samples at 48 kHz, so a window's
        // RMS doesn't swing with the phase) never rise by more than 0.5 dB.
        let mut eq = RoomEq::new(SR, default_on_params());
        let sine = |i: usize| 0.1 * (2.0 * PI * 1000.0 * i as f64 / SR as f64).sin() as f32;
        let mut warm: Vec<f32> = (0..4800).map(sine).collect();
        eq.process(&mut warm, None);
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        eq.set(p);
        let (mut prev, mut max_jump, mut last) = (None::<f64>, 0.0f64, 0.0f64);
        for w in 0..1000 {
            let mut block: Vec<f32> = (0..48).map(|i| sine(4800 + w * 48 + i)).collect();
            eq.process(&mut block, None);
            let g = db(rms(&block));
            if let Some(p) = prev {
                max_jump = max_jump.max(g - p);
            }
            prev = Some(g);
            last = g;
        }
        assert!(max_jump <= 0.5, "max window-to-window rise {max_jump} dB");
        // And it got there: +6 dB over the 0.1 sine's RMS (-23 dBFS).
        assert!((last - (db(0.1 / 2f64.sqrt()) + 6.0)).abs() < 0.2, "ended at {last}");
    }

    #[test]
    fn the_cap_holds_while_bands_glide_up_together() {
        // Three +6 dB bands switched on at once on a running, flat EQ: they
        // rise together faster than a headroom glide could follow.
        let mut eq = RoomEq::new(SR, default_on_params());
        let sine = |i: usize| 0.1 * (2.0 * PI * 1000.0 * i as f64 / SR as f64).sin() as f32;
        let mut warm: Vec<f32> = (0..4800).map(sine).collect();
        eq.process(&mut warm, None);
        let mut p = default_on_params();
        for b in 0..3 {
            p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        }
        eq.set(p);
        let reference = rms(&(0..4800).map(sine).collect::<Vec<_>>());
        let mut worst = f64::NEG_INFINITY;
        for w in 0..(SR as usize / 256) {
            let mut block: Vec<f32> = (0..256).map(|i| sine(4800 + w * 256 + i)).collect();
            eq.process(&mut block, None);
            worst = worst.max(db(rms(&block) / reference));
        }
        assert!(worst <= 6.3, "it reached {worst:.2} dB during the glide");
    }

    #[test]
    fn delay_zero_is_no_delay() {
        // On, with no band and no delay: the impulse comes out where it
        // went in (the draft read the oldest sample in the ring).
        let mut eq = RoomEq::new(SR, default_on_params());
        let mut buf = vec![0.0f32; 256];
        buf[10] = 0.5;
        eq.process(&mut buf, None);
        assert_eq!(buf[10], 0.5);
        assert!(buf.iter().enumerate().all(|(i, v)| i == 10 || *v == 0.0));
    }

    #[test]
    fn a_narrow_peak_between_grid_points_is_still_capped() {
        // Two narrow +6 dB peaks on the same frequency, off the grid.
        let mut p = default_on_params();
        for b in 0..2 {
            p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1234.0, gain_db: 6.0, q: 10.0 };
        }
        let eq = RoomEq::new(SR, p);
        // 12 dB of peaks held to the line: 6 dB under it, plus the margin.
        assert!((eq.headroom_db() as f64 + 6.0 + CAP_MARGIN_DB).abs() < 0.1, "headroom {}", eq.headroom_db());
    }

    #[test]
    fn a_band_switched_off_stops_playing() {
        // The review: three +6 dB bells, one switched off, kept playing
        // at +12 because the gain glide pulled it back up.
        let mut p = default_on_params();
        for b in 0..3 {
            p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        }
        let mut eq = RoomEq::new(SR, p);
        let sine = |i: usize| 0.1 * (2.0 * PI * 1000.0 * i as f64 / SR as f64).sin() as f32;
        let mut warm: Vec<f32> = (0..48_000).map(sine).collect();
        eq.process(&mut warm, None);
        p.sides[0].bands[0].on = false;
        p.sides[0].bands[1].on = false;
        eq.set(p);
        let mut worst = f64::NEG_INFINITY;
        let mut last = 0.0;
        for w in 0..1000 {
            let mut block: Vec<f32> = (0..48).map(|i| sine(48_000 + w * 48 + i)).collect();
            eq.process(&mut block, None);
            last = db(rms(&block) / (0.1 / 2f64.sqrt()));
            worst = worst.max(last);
        }
        assert!(worst <= 6.3, "it reached {worst:.2} dB");
        assert!((last - 6.0).abs() < 0.2, "one band left: +6, got {last:.2}");
        assert!(!eq.cur.sides[0].bands[0].on && !eq.cur.sides[0].bands[1].on, "the two are off");
    }

    #[test]
    fn shelves_stay_within_the_cap() {
        // The review: a -18 dB low shelf at Q 10 rang to +18.9 dB at
        // 168 Hz. A shelf's Q stops at MAX_SHELF_Q, and whatever the bands,
        // the response after the headroom never passes the cap.
        let mut p = default_on_params();
        for (b, kind) in [BandKind::LowShelf, BandKind::HighShelf, BandKind::LowShelf].into_iter().enumerate() {
            p.sides[0].bands[b] = RoomBand { on: true, kind, freq_hz: 200.0 * (b + 1) as f32, gain_db: if b == 0 { -18.0 } else { 6.0 }, q: 10.0 };
        }
        let p = p.sanitized();
        assert!(p.sides[0].bands.iter().take(3).all(|b| b.q <= MAX_SHELF_Q));
        let eq = RoomEq::new(SR, p);
        for k in 0..2000 {
            let hz = MIN_HZ * (MAX_HZ / MIN_HZ).powf(k as f32 / 1999.0);
            let r = side_response_db(&RoomSide { trim_db: 0.0, ..p.sides[0] }, hz, SR) + eq.headroom_db();
            assert!(r <= MAX_BOOST_DB + 0.05, "{r:.2} dB at {hz:.0} Hz");
        }
    }

    #[test]
    fn the_on_fade_takes_fifty_ms() {
        // The review: stepped once per 32 samples by one sample's worth,
        // it took 1.6 s.
        let mut p = default_on_params();
        p.on = false;
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: -12.0, q: 1.0 };
        let mut eq = RoomEq::new(SR, p);
        p.on = true;
        eq.set(p);
        let mut block = vec![0.0f32; 2600];
        eq.process(&mut block, None);
        assert_eq!(eq.on_cur, 1.0, "fully on after 2600 samples (54 ms)");
    }

    #[test]
    fn a_delay_keeps_the_treble() {
        // 1.2 ms at 48 kHz is 57.6 samples: rounded to 58, an impulse comes
        // out whole, not smeared over two samples (a treble cut).
        let mut p = default_on_params();
        p.sides[0].delay_ms = 1.2;
        let mut eq = RoomEq::new(SR, p);
        let mut buf = vec![0.0f32; 256];
        buf[0] = 0.5;
        eq.process(&mut buf, None);
        assert_eq!(buf[58], 0.5);
        assert_eq!(buf.iter().filter(|v| **v != 0.0).count(), 1);
    }

    /// Whole-cycle RMS (48 samples of a 1 kHz sine at 48 kHz) of what
    /// `eq` makes of a steady 0.1 sine, `blocks` windows on from sample
    /// `from`, in dB against the sine itself.
    fn window_levels(eq: &mut RoomEq, from: usize, blocks: usize) -> Vec<f64> {
        let sine = |i: usize| 0.1 * (2.0 * PI * 1000.0 * i as f64 / SR as f64).sin() as f32;
        (0..blocks)
            .map(|w| {
                let mut block: Vec<f32> = (0..48).map(|i| sine(from + w * 48 + i)).collect();
                eq.process(&mut block, None);
                db(rms(&block) / (0.1 / 2f64.sqrt()))
            })
            .collect()
    }

    #[test]
    fn switching_off_gives_the_level_back_at_the_rise_rate() {
        // Round 2: a -18 dB cut and a -12 dB trim came back 30 dB in 50 ms
        // through the off crossfade. Now the settings glide flat first.
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: -18.0, q: 1.0 };
        p.sides[0].trim_db = -12.0;
        let mut eq = RoomEq::new(SR, p);
        let _ = window_levels(&mut eq, 0, 200);
        p.on = false;
        eq.set(p);
        let levels = window_levels(&mut eq, 9600, 2000);
        // 50 ms is 50 windows: never more than 75 dB/s, so under 4 dB in
        // any 50 ms (with a little for the window).
        for w in 0..levels.len() - 50 {
            assert!(levels[w + 50] - levels[w] <= 4.5, "{:.1} dB in 50 ms at window {w}", levels[w + 50] - levels[w]);
        }
        assert!(eq.is_idle(), "off in the end");
        assert!(levels.last().unwrap().abs() < 0.1, "untouched in the end");
    }

    #[test]
    fn rises_share_one_budget() {
        // Round 2: "Make it flat" over eight cuts and a trim rose 34 dB in
        // 50 ms; the rises stack no more.
        let mut p = default_on_params();
        for b in 0..8 {
            p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: -18.0, q: 1.0 };
        }
        p.sides[0].trim_db = -12.0;
        let mut eq = RoomEq::new(SR, p);
        let _ = window_levels(&mut eq, 0, 100);
        eq.set(RoomEqParams { on: true, ..RoomEqParams::default() });
        let levels = window_levels(&mut eq, 4800, 3000);
        for w in 0..levels.len() - 50 {
            assert!(levels[w + 50] - levels[w] <= 4.5, "{:.1} dB in 50 ms at window {w}", levels[w + 50] - levels[w]);
        }
    }

    #[test]
    fn the_cap_holds_while_a_narrow_bell_becomes_a_shelf() {
        // Round 2: the width kept gliding down from the bell's after the
        // kind switched, and the shelf rang to +6.76 dB at 19.6 kHz.
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 12_000.0, gain_db: 6.0, q: 8.0 };
        let mut eq = RoomEq::new(SR, p);
        let _ = window_levels(&mut eq, 0, 50);
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::HighShelf, freq_hz: 12_000.0, gain_db: 6.0, q: MAX_SHELF_Q };
        eq.set(p);
        let mut block = vec![0.0f32; 32];
        for k in 0..3000 {
            eq.process(&mut block, None);
            let b = eq.cur.sides[0].bands[0];
            assert!(b.kind == BandKind::Peak || b.q <= MAX_SHELF_Q + 1e-6, "a shelf at Q {}", b.q);
            if k % 10 == 0 {
                // Checked on a dense grid of its own, not the cap's search.
                let side = RoomSide { trim_db: 0.0, ..eq.cur.sides[0] };
                for j in 0..1500 {
                    let hz = MIN_HZ * (MAX_HZ / MIN_HZ).powf(j as f32 / 1499.0);
                    let r = side_response_db(&side, hz, SR) as f64 + eq.headroom_db;
                    assert!(r <= MAX_BOOST_DB as f64 + 0.05, "{r:.2} dB at {hz:.0} Hz, step {k}");
                }
            }
        }
    }

    #[test]
    fn the_cap_check_fits_in_a_glide_step() {
        // It runs every 32 samples (667 µs at 48 kHz) while a glide moves;
        // round 2 measured 750-800 µs with 16 bands lit near 0 dB.
        let mut p = default_on_params();
        for s in 0..2 {
            for b in 0..ROOM_BANDS {
                p.sides[s].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 50.0 * 2f32.powi(b as i32), gain_db: -0.5, q: 1.4 };
            }
        }
        let t = std::time::Instant::now();
        let mut x = 0.0;
        for _ in 0..100 {
            x += max_boost_db(&p, SR as f64);
        }
        let each = t.elapsed().as_secs_f64() / 100.0 * 1e6;
        assert!(x.is_finite());
        assert!(each < 400.0, "{each:.0} µs a call");
    }

    #[test]
    fn the_cap_covers_below_20_hz_and_above_20_khz() {
        // Round 3: two +6 dB low shelves at 20 Hz lifted 5 Hz by 12 dB,
        // two high shelves at 20 kHz lifted 23 kHz by 12, unseen.
        for (sr, kind, hz) in [(48_000.0f32, BandKind::LowShelf, 20.0f32), (48_000.0, BandKind::HighShelf, 20_000.0), (96_000.0, BandKind::HighShelf, 20_000.0)] {
            let mut p = default_on_params();
            for b in 0..2 {
                p.sides[0].bands[b] = RoomBand { on: true, kind, freq_hz: hz, gain_db: 6.0, q: MAX_SHELF_Q };
            }
            let eq = RoomEq::new(sr, p);
            for k in 0..3000 {
                let f = 1.0 * (0.499 * sr).powf(k as f32 / 2999.0);
                let r = side_response_db(&RoomSide { trim_db: 0.0, ..p.sides[0] }, f, sr) + eq.headroom_db();
                assert!(r <= MAX_BOOST_DB + 0.05, "{r:.2} dB at {f:.1} Hz ({sr} Hz)");
            }
        }
    }

    #[test]
    fn moving_a_cut_gives_its_level_back_slowly() {
        // Round 3: a -18 dB low shelf dragged from 20 kHz to 20 Hz gave
        // 17.7 dB back at 1 kHz in 50 ms; a -18 dB bell moved off 1 kHz, 16.
        for (start, end, kind, q) in [(20_000.0f32, 20.0f32, BandKind::LowShelf, MAX_SHELF_Q), (1000.0, 4000.0, BandKind::Peak, 4.0)] {
            let mut p = default_on_params();
            p.sides[0].bands[0] = RoomBand { on: true, kind, freq_hz: start, gain_db: -18.0, q };
            let mut eq = RoomEq::new(SR, p);
            let _ = window_levels(&mut eq, 0, 100);
            p.sides[0].bands[0].freq_hz = end;
            eq.set(p);
            let levels = window_levels(&mut eq, 4800, 8000);
            for w in 0..levels.len() - 50 {
                assert!(levels[w + 50] - levels[w] <= 4.5, "{kind:?}: {:.1} dB in 50 ms at window {w}", levels[w + 50] - levels[w]);
            }
            assert!(eq.cur.sides[0].bands[0].freq_hz == end, "{kind:?} got there");
        }
    }

    #[test]
    fn a_mono_loop_with_a_right_delay_still_switches_off() {
        // Round 3: side 1's delay crossfade never counted down in mono, so
        // it never went idle (and off was never untouched).
        let mut p = default_on_params();
        p.sides[1].delay_ms = 3.0;
        let mut eq = RoomEq::new(SR, p);
        let mut buf = vec![0.1f32; 4800];
        eq.process(&mut buf, None);
        p.on = false;
        eq.set(p);
        for _ in 0..20 {
            let mut b = vec![0.1f32; 4800];
            eq.process(&mut b, None);
        }
        assert!(eq.is_idle());
        let mut b: Vec<f32> = (0..64).map(|i| i as f32 / 100.0).collect();
        let before = b.clone();
        eq.process(&mut b, None);
        assert_eq!(b, before, "off is untouched");
    }

    #[test]
    fn the_first_switch_on_glides_up() {
        // Round 3: a loop started with Room EQ off held the full settings,
        // and switching on faded straight into them.
        let mut p = default_on_params();
        p.on = false;
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
        let mut eq = RoomEq::new(SR, p);
        p.on = true;
        eq.set(p);
        let levels = window_levels(&mut eq, 0, 2000);
        for w in 0..levels.len() - 50 {
            assert!(levels[w + 50] - levels[w] <= 4.5, "{:.2} dB in 50 ms at {w}: {:?}", levels[w + 50] - levels[w], &levels[w..w + 51].iter().step_by(10).map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>());
        }
        assert!((levels.last().unwrap() - 6.0).abs() < 0.2);
    }

    #[test]
    fn the_cap_reaches_the_very_top_and_dc() {
        // Round 4: stopping at 0.49 of the rate missed stacked wide high
        // shelves: +14 dB at 22.05 kHz.
        for sr in [44_100.0f32, 48_000.0, 96_000.0] {
            for (kind, hz, n) in [(BandKind::HighShelf, 20_000.0f32, 8), (BandKind::HighShelf, 20_000.0, 2), (BandKind::LowShelf, 20.0, 8)] {
                let mut p = default_on_params();
                for b in 0..n {
                    p.sides[0].bands[b] = RoomBand { on: true, kind, freq_hz: hz, gain_db: 6.0, q: MIN_Q };
                }
                let eq = RoomEq::new(sr, p);
                let side = RoomSide { trim_db: 0.0, ..p.sides[0] };
                let mut worst = side_response_db(&side, 0.0, sr);
                for k in 0..=4000 {
                    worst = worst.max(side_response_db(&side, 0.5 * sr * (k as f32 / 4000.0), sr));
                }
                let r = worst + eq.headroom_db();
                assert!(r <= MAX_BOOST_DB + 0.05, "{r:.2} dB, {n} {kind:?} at {sr}");
            }
        }
    }

    #[test]
    fn rates_hold_per_second_at_96_khz() {
        // Round 4: the per-step rates doubled at 96 kHz (7.5 dB in 50 ms).
        let sr = 96_000.0f32;
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 1000.0, gain_db: -18.0, q: 1.0 };
        let mut eq = RoomEq::new(sr, p);
        let sine = |i: usize| 0.1 * (2.0 * PI * 1000.0 * i as f64 / sr as f64).sin() as f32;
        let mut warm: Vec<f32> = (0..9600).map(sine).collect();
        eq.process(&mut warm, None);
        p.on = false;
        eq.set(p);
        let levels: Vec<f64> = (0..3000)
            .map(|w| {
                let mut b: Vec<f32> = (0..96).map(|i| sine(9600 + w * 96 + i)).collect();
                eq.process(&mut b, None);
                db(rms(&b) / (0.1 / 2f64.sqrt()))
            })
            .collect();
        for w in 0..levels.len() - 50 {
            assert!(levels[w + 50] - levels[w] <= 4.5, "{:.1} dB in 50 ms at 96 kHz", levels[w + 50] - levels[w]);
        }
    }

    #[test]
    fn a_far_move_dips_and_arrives() {
        // Round 4: a -18 dB notch swept from 20 Hz to 20 kHz crawled for
        // 72 s. Now it goes to 0 dB, jumps, and comes back, within seconds,
        // never rising faster than the budget.
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 20.0, gain_db: -18.0, q: 10.0 };
        let mut eq = RoomEq::new(SR, p);
        let _ = window_levels(&mut eq, 0, 50);
        p.sides[0].bands[0].freq_hz = 1000.0;
        eq.set(p);
        let levels = window_levels(&mut eq, 2400, 6000);
        for w in 0..levels.len() - 50 {
            assert!(levels[w + 50] - levels[w] <= 4.5, "{:.1} dB in 50 ms", levels[w + 50] - levels[w]);
        }
        let b = eq.cur.sides[0].bands[0];
        assert!(b.freq_hz == 1000.0 && b.gain_db == -18.0, "arrived within 6 s: {b:?}");
    }

    #[test]
    fn a_narrow_low_cut_comes_back_slowly_through_the_filter() {
        // Round 5: a -18 dB Q 10 cut at 20 Hz, released, came back 6 dB in
        // 50 ms measured through the filter (it settles slowly). Windows of
        // one 20 Hz cycle (2400 samples, 50 ms), each against the one before.
        for (hz, q) in [(20.0f32, 10.0f32), (40.0, 10.0), (100.0, 10.0)] {
            let mut p = default_on_params();
            p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: hz, gain_db: -18.0, q };
            let mut eq = RoomEq::new(SR, p);
            let sine = |i: usize| 0.1 * (2.0 * PI * hz as f64 * i as f64 / SR as f64).sin() as f32;
            let cycle = (SR / hz) as usize;
            let win = cycle * (2400 / cycle).max(1);
            let mut warm: Vec<f32> = (0..96_000).map(sine).collect();
            eq.process(&mut warm, None);
            p.sides[0].bands[0].on = false;
            eq.set(p);
            let mut prev: Option<f64> = None;
            for w in 0..200 {
                let mut b: Vec<f32> = (0..win).map(|i| sine(96_000 + w * win + i)).collect();
                eq.process(&mut b, None);
                let l = db(rms(&b) / (0.1 / 2f64.sqrt()));
                if let Some(p) = prev {
                    assert!(l - p <= 4.5, "{hz} Hz Q {q}: {:.1} dB in 50 ms", l - p);
                }
                prev = Some(l);
            }
        }
    }

    #[test]
    fn two_close_narrow_peaks_near_the_top_stay_capped() {
        // Round 6: two +6 dB Q 10 bells 113 Hz apart at 18.9 kHz (44.1 kHz)
        // met between the search's points and played 6.48 dB.
        let sr = 44_100.0f32;
        let mut p = default_on_params();
        p.sides[0].bands[0] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 18_900.0, gain_db: 6.0, q: 10.0 };
        p.sides[0].bands[1] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 19_013.0, gain_db: 6.0, q: 10.0 };
        let eq = RoomEq::new(sr, p);
        let side = RoomSide { trim_db: 0.0, ..p.sides[0] };
        let mut worst = f32::NEG_INFINITY;
        for k in 0..=20_000 {
            worst = worst.max(side_response_db(&side, 15_000.0 + 7_050.0 * k as f32 / 20_000.0, sr));
        }
        assert!(worst + eq.headroom_db() <= MAX_BOOST_DB + 0.01, "{:.2} dB", worst + eq.headroom_db());
    }

    #[test]
    fn stacked_close_narrow_bells_near_the_top_stay_capped() {
        // Round 6 (Fable): eight +6 dB Q 10 bells, four at 19,975 Hz and
        // four at 19,945, played 6.75 dB at 44.1 kHz.
        for sr in [44_100.0f32, 48_000.0] {
            let mut p = default_on_params();
            for b in 0..8 {
                p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: if b < 4 { 19_975.0 } else { 19_945.0 }, gain_db: 6.0, q: 10.0 };
            }
            let eq = RoomEq::new(sr, p);
            let side = RoomSide { trim_db: 0.0, ..p.sides[0] };
            let mut worst = f32::NEG_INFINITY;
            for k in 0..=40_000 {
                worst = worst.max(side_response_db(&side, 17_000.0 + (0.5 * sr - 17_000.0) * k as f32 / 40_000.0, sr));
            }
            assert!(worst + eq.headroom_db() <= MAX_BOOST_DB + 0.01, "{:.2} dB at {sr}", worst + eq.headroom_db());
        }
    }

    #[test]
    fn random_sets_stay_within_the_cap() {
        // Random sets of up to eight bands (any kind, frequency, gain and
        // width, often stacked close), at 44.1, 48 and 96 kHz, each checked
        // on a dense grid from DC to half the rate after the headroom.
        let mut seed: u32 = 0x9e37_79b9;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        for sr in [44_100.0f32, 48_000.0, 96_000.0] {
            for _ in 0..150 {
                let mut p = default_on_params();
                let centre = MIN_HZ * (MAX_HZ / MIN_HZ).powf(rnd());
                for b in 0..1 + (rnd() * 8.0) as usize % 8 {
                    let kind = [BandKind::Peak, BandKind::Peak, BandKind::LowShelf, BandKind::HighShelf][(rnd() * 4.0) as usize % 4];
                    // Half the bands crowd one place, the rest anywhere.
                    let hz = if rnd() < 0.5 { centre * 2f32.powf((rnd() - 0.5) * 0.2) } else { MIN_HZ * (MAX_HZ / MIN_HZ).powf(rnd()) };
                    p.sides[0].bands[b] = RoomBand { on: true, kind, freq_hz: hz, gain_db: MIN_CUT_DB + (MAX_BOOST_DB - MIN_CUT_DB) * rnd(), q: MIN_Q + (MAX_Q - MIN_Q) * rnd() * rnd() };
                }
                let p = p.sanitized();
                let eq = RoomEq::new(sr, p);
                let side = RoomSide { trim_db: 0.0, ..p.sides[0] };
                let mut worst = side_response_db(&side, 0.0, sr);
                for k in 0..=3000 {
                    worst = worst.max(side_response_db(&side, (0.5 * sr).powf(k as f32 / 3000.0), sr));
                }
                let r = worst + eq.headroom_db();
                assert!(r <= MAX_BOOST_DB + 0.02, "{r:.3} dB at {sr}: {:?}", p.sides[0].bands);
            }
        }
    }

    #[test]
    fn deep_low_narrow_cuts_switch_off_within_the_close_limit() {
        // Round 6: eight -18 dB Q 10 cuts at 20-45 Hz took 7.3 s to go
        // flat (Opus), all eight at 20 Hz 10.4 s (Fable); the loop went at
        // its limit with the bass still held.
        for spread in [3.5f32, 0.0] {
        let mut p = default_on_params();
        for b in 0..8 {
            p.sides[0].bands[b] = RoomBand { on: true, kind: BandKind::Peak, freq_hz: 20.0 + spread * b as f32, gain_db: -18.0, q: 10.0 };
        }
        p.sides[0].trim_db = -12.0;
        let mut eq = RoomEq::new(SR, p);
        let mut warm = vec![0.0f32; 4800];
        eq.process(&mut warm, None);
        p.on = false;
        eq.set(p);
        let mut t = 0.0;
        while !eq.is_idle() && t < 10.0 {
            let mut b = vec![0.0f32; 480];
            eq.process(&mut b, None);
            t += 0.01;
        }
        assert!(t < 5.0, "idle after {t:.2} s (quitting waits up to 6 s)");
        }
    }

    #[test]
    fn nan_input_gives_silence() {
        let p = default_on_params();
        let mut eq = RoomEq::new(SR, p);
        let n = 64;
        let mut left: Vec<f32> = vec![0.5; n];
        left[10] = f32::NAN;
        let mut right: Vec<f32> = vec![0.5; n];
        eq.process(&mut left, Some(&mut right));
        for &v in left.iter() { assert_eq!(v, 0.0); }
        for &v in right.iter() { assert_eq!(v, 0.0); }
    }

    #[test]
    fn sanitized_clamps() {
        let mut p = RoomEqParams::default();
        p.on = true;
        p.sides[0].bands[0].gain_db = 40.0;   // -> 6
        p.sides[0].delay_ms = 100.0;          // -> 20
        p.sides[0].trim_db = 5.0;             // -> 0
        p.sides[0].bands[0].freq_hz = 5.0;    // -> 20
        p.sides[0].bands[0].q = f32::NAN;     // -> 1.0

        let s = p.sanitized();
        assert_eq!(s.sides[0].bands[0].gain_db, MAX_BOOST_DB);
        assert_eq!(s.sides[0].delay_ms, MAX_DELAY_MS);
        assert_eq!(s.sides[0].trim_db, 0.0);
        assert_eq!(s.sides[0].bands[0].freq_hz, MIN_HZ);
        assert_eq!(s.sides[0].bands[0].q, 1.0);
    }
}
