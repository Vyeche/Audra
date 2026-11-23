//! Dynamics for one input (issue #27; the owner, 2026-11-10: "Hardware
//! Dynamics (Comp/Gate): Not Available" on the Babyface, "also would be
//! cool to see"). The Fireface UCX II and UFX III run these in the unit;
//! the Babyface can't, so Audra runs them on the computer.
//!
//! A gate (or, with a shallow range, an expander) turns the input down
//! while it is quieter than its threshold, so room noise and keyboard
//! clicks stay out between sentences; a compressor turns it down while it
//! is louder than its threshold, so loud and quiet talking come out closer;
//! then makeup gain brings the whole back up. One detector serves both
//! sides of a linked pair, so a stereo image never leans.
//!
//! Audra runs one on an input (what comes out is a virtual microphone for
//! apps; the monitors stay straight) and one on each output that has its
//! D on (what the computer plays there, on its way to the speakers or the
//! phones). Auto Level after both, as TotalMix has it: a slow amplifier
//! and a limiter that never lets the output past its headroom.
//!
//! As the rest of this crate: no allocation while processing, built once
//! per sample rate, testable offline. Safety first, since on an output
//! what comes out reaches ears: makeup is capped, Auto Level's gain is
//! capped, a non-finite sample resets to silence, and the output is
//! clamped to full scale.

/// The settings, as the UI shows them (dB, ms, a ratio).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct DynamicsParams {
    pub gate_on: bool,
    /// Under this level the gate turns the input down.
    pub gate_threshold_db: f32,
    /// How far it turns down when closed: about −60 is a gate, −10 a
    /// gentle expander.
    pub gate_range_db: f32,
    /// How steeply the expander turns down under its threshold: 1 does
    /// nothing, 10 is nearly a gate.
    pub exp_ratio: f32,
    pub comp_on: bool,
    /// Over this level the compressor turns the input down.
    pub threshold_db: f32,
    /// How much: at 3, 3 dB over the threshold comes out 1 dB over.
    pub ratio: f32,
    /// The width of the soft bend around the threshold.
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// The whole brought back up after the compressor (capped at
    /// `MAKEUP_MAX_DB`); with the compressor off it does nothing, since
    /// then it could only make the input louder and clip.
    pub makeup_db: f32,
    /// Auto Level: a slow amplifier plus a limiter, after the dynamics.
    pub auto_on: bool,
    /// The most Auto Level may amplify.
    pub auto_max_gain_db: f32,
    /// How far below full scale Auto Level keeps the output.
    pub auto_headroom_db: f32,
    /// How slowly Auto Level turns the amplification up.
    pub auto_rise_s: f32,
}

impl Default for DynamicsParams {
    fn default() -> Self {
        // "Talking": a gentle evening-out and a soft expander.
        Self {
            gate_on: true,
            gate_threshold_db: -55.0,
            gate_range_db: -15.0,
            exp_ratio: 10.0,
            comp_on: true,
            threshold_db: -22.0,
            ratio: 3.0,
            knee_db: 6.0,
            attack_ms: 5.0,
            release_ms: 150.0,
            makeup_db: 4.0,
            auto_on: false,
            auto_max_gain_db: 6.0,
            auto_headroom_db: 6.0,
            auto_rise_s: 3.0,
        }
    }
}

/// Makeup never past this: a slip of the knob can't blast the monitors.
pub const MAKEUP_MAX_DB: f32 = 12.0;
/// The gate opens this fast (2 ms: at 1 ms a deep gate opening on a low
/// note could click; a second reviewer's catch)…
const GATE_OPEN_MS: f32 = 2.0;
/// …stays open this long after the level drops…
const GATE_HOLD_MS: f32 = 60.0;
/// …and closes this gently.
const GATE_CLOSE_MS: f32 = 120.0;
/// The detector's level floor (silence), in dB.
const FLOOR_DB: f32 = -120.0;
/// Auto Level's fast fall, in ms.
const AUTO_FAST_MS: f32 = 3.0;
/// Auto Level's envelope fall, in ms.
const AUTO_ENV_MS: f32 = 300.0;
/// Auto Level's noise floor when the gate is off, in dBFS.
const AUTO_FLOOR_DB: f32 = -50.0;
/// How quickly the makeup, and Auto Level's gain when it's turned off,
/// move to a new value, in ms: a preset changed mid-word stepped the gain
/// by up to 6 dB at once, a click (measured, 2026-11-10).
const GLIDE_MS: f32 = 10.0;
/// How quickly the makeup rises to a new value, in ms (one pole: most of
/// the way in a third of a second, as the app's other rises glide). At
/// `GLIDE_MS` a preset, Fit or the Strength knob put a quiet passage up by
/// up to 12 dB in a few hundredths of a second, which the people on a call
/// heard swell (OpenCode's job 45: the compressor takes nothing away
/// below its threshold, so the makeup's rise is all heard). Falls keep
/// `GLIDE_MS`.
const RISE_MS: f32 = 100.0;

/// One channel's (or one linked pair's) dynamics.
pub struct Dynamics {
    params: DynamicsParams,
    sample_rate: f32,
    attack: f32,
    release: f32,
    gate_open: f32,
    gate_close: f32,
    hold_samples: u32,
    /// The compressor's gain change now, in dB (0 or less).
    comp_db: f32,
    /// The gate's gain change now, in dB (0 or less).
    gate_db: f32,
    /// Samples left of the gate's hold.
    hold: u32,
    /// The deepest turn-down since the meter last read it, in dB.
    meter_db: f32,
    /// Auto Level's amplifier gain now, in dB (signed).
    auto_db: f32,
    /// Auto Level's envelope of the processed level, in dB.
    env_db: f32,
    /// One-pole coefficient for Auto Level's fast fall.
    auto_fast: f32,
    /// One-pole coefficient for Auto Level's rise.
    auto_slow: f32,
    /// One-pole coefficient for Auto Level's envelope fall.
    auto_env: f32,
    /// The makeup now, in dB, gliding to the setting's.
    makeup_now: f32,
    /// One-pole coefficient for `GLIDE_MS`.
    glide: f32,
    /// One-pole coefficient for `RISE_MS`.
    rise: f32,
}

/// The one-pole coefficient for a time constant of `ms`.
fn coeff(ms: f32, sample_rate: f32) -> f32 {
    let samples = (ms.max(0.01) * 0.001 * sample_rate).max(1.0);
    (-1.0 / samples).exp()
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn gain_to_db(g: f32) -> f32 {
    if g > 1e-6 {
        20.0 * g.log10()
    } else {
        FLOOR_DB
    }
}

impl Dynamics {
    pub fn new(sample_rate: f32, params: DynamicsParams) -> Self {
        let mut d = Self {
            params,
            sample_rate,
            attack: 0.0,
            release: 0.0,
            gate_open: coeff(GATE_OPEN_MS, sample_rate),
            gate_close: coeff(GATE_CLOSE_MS, sample_rate),
            hold_samples: (GATE_HOLD_MS * 0.001 * sample_rate) as u32,
            comp_db: 0.0,
            gate_db: 0.0,
            hold: 0,
            meter_db: 0.0,
            auto_db: 0.0,
            env_db: FLOOR_DB,
            auto_fast: 0.0,
            auto_slow: 0.0,
            auto_env: 0.0,
            makeup_now: if params.comp_on && params.makeup_db.is_finite() { params.makeup_db.clamp(0.0, MAKEUP_MAX_DB) } else { 0.0 },
            glide: coeff(GLIDE_MS, sample_rate),
            rise: coeff(RISE_MS, sample_rate),
        };
        d.set(params);
        d
    }

    /// New settings, kept within safe ranges; the state carries on, so a
    /// knob turned while it runs never clicks.
    pub fn set(&mut self, p: DynamicsParams) {
        let p = DynamicsParams {
            ratio: if p.ratio.is_finite() { p.ratio.clamp(1.0, 20.0) } else { 1.0 },
            knee_db: if p.knee_db.is_finite() { p.knee_db.clamp(0.0, 24.0) } else { 0.0 },
            threshold_db: if p.threshold_db.is_finite() { p.threshold_db.clamp(-60.0, 0.0) } else { 0.0 },
            gate_threshold_db: if p.gate_threshold_db.is_finite() { p.gate_threshold_db.clamp(-90.0, -10.0) } else { -90.0 },
            gate_range_db: if p.gate_range_db.is_finite() { p.gate_range_db.clamp(-80.0, 0.0) } else { 0.0 },
            exp_ratio: if p.exp_ratio.is_finite() { p.exp_ratio.clamp(1.0, 10.0) } else { 10.0 },
            makeup_db: if p.makeup_db.is_finite() { p.makeup_db.clamp(0.0, MAKEUP_MAX_DB) } else { 0.0 },
            attack_ms: if p.attack_ms.is_finite() { p.attack_ms.clamp(0.1, 200.0) } else { 5.0 },
            release_ms: if p.release_ms.is_finite() { p.release_ms.clamp(10.0, 2000.0) } else { 150.0 },
            auto_max_gain_db: if p.auto_max_gain_db.is_finite() { p.auto_max_gain_db.clamp(0.0, 18.0) } else { 0.0 },
            auto_headroom_db: if p.auto_headroom_db.is_finite() { p.auto_headroom_db.clamp(0.0, 12.0) } else { 12.0 },
            auto_rise_s: if p.auto_rise_s.is_finite() { p.auto_rise_s.clamp(0.1, 9.9) } else { 3.0 },
            ..p
        };
        self.attack = coeff(p.attack_ms, self.sample_rate);
        self.release = coeff(p.release_ms, self.sample_rate);
        self.auto_fast = coeff(AUTO_FAST_MS, self.sample_rate);
        self.auto_slow = coeff(p.auto_rise_s * 1000.0, self.sample_rate);
        self.auto_env = coeff(AUTO_ENV_MS, self.sample_rate);
        self.params = p;
        if !p.auto_on {
            // Auto Level's gain glides back to nothing in `process`.
            self.env_db = FLOOR_DB;
        }
    }

    pub fn params(&self) -> DynamicsParams {
        self.params
    }

    /// Back to rest: the compressor and the gate at zero, Auto Level at
    /// rest, the makeup at its setting. In place: no allocation.
    pub fn reset(&mut self) {
        self.comp_db = 0.0;
        self.gate_db = 0.0;
        self.hold = 0;
        self.meter_db = 0.0;
        self.auto_db = 0.0;
        self.env_db = FLOOR_DB;
        self.makeup_now = if self.params.comp_on { self.params.makeup_db } else { 0.0 };
    }

    /// A broken sample: the sample itself is silenced and the detectors
    /// (the level followers) start afresh — but the gain stays where it
    /// is. Resetting the gain too stepped the output by 6–10 dB in one
    /// sample, which is a click (job 5).
    fn broken_sample(&mut self) {
        self.hold = 0;
        self.meter_db = 0.0;
        self.env_db = FLOOR_DB;
    }

    /// The compressor's static curve: the gain change for a level `x` dB,
    /// with a soft knee (0 or less).
    pub fn curve_db(&self, x: f32) -> f32 {
        let p = &self.params;
        let (t, r, w) = (p.threshold_db, p.ratio, p.knee_db);
        let over = x - t;
        let y = if 2.0 * over < -w {
            x
        } else if w > 0.0 && 2.0 * over.abs() <= w {
            x + (1.0 / r - 1.0) * (over + w / 2.0).powi(2) / (2.0 * w)
        } else {
            t + over / r
        };
        (y - x).min(0.0)
    }

    /// The steady output level (dB) for a steady input level (dB), for the
    /// panel's curve. Auto Level is not included.
    pub fn static_db(&self, x: f32) -> f32 {
        if !x.is_finite() {
            return FLOOR_DB;
        }
        let p = self.params;
        let mut out = x;
        if p.comp_on {
            out += self.curve_db(x);
        }
        if p.gate_on && x < p.gate_threshold_db {
            let expander = ((x - p.gate_threshold_db) * (p.exp_ratio - 1.0))
                .max(p.gate_range_db)
                .min(0.0);
            out += expander;
        }
        if p.comp_on {
            out += p.makeup_db;
        }
        out
    }

    /// Processes a block in place: `left`, and `right` for a linked pair
    /// (the same gain on both).
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>) {
        let p = self.params;
        let makeup_to = if p.comp_on { p.makeup_db } else { 0.0 };
        let ceiling_db = -p.auto_headroom_db;
        let ceiling_gain = db_to_gain(ceiling_db);
        let auto_floor = if p.gate_on { p.gate_threshold_db } else { AUTO_FLOOR_DB };

        // Every left sample, and the right where it has one (a shorter
        // right buffer never leaves the left's tail untouched).
        for (i, slot) in left.iter_mut().enumerate() {
            let l = *slot;
            let r = right.as_ref().and_then(|r| r.get(i)).copied().unwrap_or(0.0);
            if !l.is_finite() || !r.is_finite() {
                // A broken sample: that one sample silent, the detectors
                // afresh, the gain where it was.
                self.broken_sample();
                *slot = 0.0;
                if let Some(r) = right.as_mut().and_then(|r| r.get_mut(i)) {
                    *r = 0.0;
                }
                continue;
            }
            let level = gain_to_db(l.abs().max(r.abs()));

            // Compressor: toward the curve's gain change, quickly when it
            // must turn down more (attack), slowly when less (release).
            let want = if p.comp_on { self.curve_db(level) } else { 0.0 };
            let c = if want < self.comp_db { self.attack } else { self.release };
            self.comp_db = want + (self.comp_db - want) * c;

            // Gate: open at once over its threshold, held a moment after,
            // then closing gently to its range.
            let target = if !p.gate_on {
                0.0
            } else if level >= p.gate_threshold_db {
                self.hold = self.hold_samples;
                0.0
            } else if self.hold > 0 {
                self.hold -= 1;
                0.0
            } else {
                ((level - p.gate_threshold_db) * (p.exp_ratio - 1.0))
                    .max(p.gate_range_db)
                    .min(0.0)
            };
            let g = if target > self.gate_db { self.gate_open } else { self.gate_close };
            self.gate_db = target + (self.gate_db - target) * g;

            let change = self.comp_db + self.gate_db;
            self.meter_db = self.meter_db.min(change);
            let k = if makeup_to > self.makeup_now { self.rise } else { self.glide };
            self.makeup_now = makeup_to + (self.makeup_now - makeup_to) * k;
            let base_gain = db_to_gain(change + self.makeup_now);
            let base_l = l * base_gain;
            let base_r = r * base_gain;
            let y = base_l.abs().max(base_r.abs());

            let out_gain = if p.auto_on {
                let y_db = gain_to_db(y);
                if y_db > self.env_db {
                    self.env_db = y_db;
                } else {
                    self.env_db = y_db + (self.env_db - y_db) * self.auto_env;
                }

                let desired_db = (ceiling_db - self.env_db).min(p.auto_max_gain_db);
                if desired_db < self.auto_db {
                    self.auto_db = desired_db + (self.auto_db - desired_db) * self.auto_fast;
                } else if desired_db > self.auto_db && level >= auto_floor {
                    self.auto_db = desired_db + (self.auto_db - desired_db) * self.auto_slow;
                }

                let mut ag = db_to_gain(self.auto_db);
                if y > 0.0 && y * ag > ceiling_gain {
                    ag = ceiling_gain / y;
                }
                ag
            } else {
                // Turned off: its gain glides back to nothing.
                self.auto_db *= self.glide;
                if self.auto_db.abs() < 1e-4 {
                    self.auto_db = 0.0;
                }
                self.env_db = FLOOR_DB;
                db_to_gain(self.auto_db)
            };

            *slot = (base_l * out_gain).clamp(-1.0, 1.0);
            if let Some(r) = right.as_mut().and_then(|r| r.get_mut(i)) {
                *r = (base_r * out_gain).clamp(-1.0, 1.0);
            }
        }
    }

    /// The deepest turn-down since the last read, in dB (0 or less), for
    /// the panel's meter; reading it starts the next span.
    pub fn take_meter_db(&mut self) -> f32 {
        let m = self.meter_db;
        self.meter_db = 0.0;
        m
    }

    /// Auto Level's amplifier gain now, in dB (signed; positive amplifies).
    pub fn auto_gain_db(&self) -> f32 {
        self.auto_db
    }
}

/// Ready-made starting points for a voice, by name (the owner's pick from
/// ChatGPT's list, 2026-11-10: "Clear Calls" first, the everyday one).
/// Each ends in a limiter at −2 dBFS: Auto Level with no gain of its own.
/// The thresholds suit a typical voice at a sensible gain.
pub fn presets() -> [(&'static str, DynamicsParams); 5] {
    let limit = |p: DynamicsParams| DynamicsParams { auto_on: true, auto_max_gain_db: 0.0, auto_headroom_db: 2.0, auto_rise_s: 3.0, ..p };
    let voice = |gate_threshold_db, gate_range_db, exp_ratio, threshold_db, ratio, knee_db, attack_ms, release_ms, makeup_db| {
        limit(DynamicsParams {
            gate_on: true,
            gate_threshold_db,
            gate_range_db,
            exp_ratio,
            comp_on: true,
            threshold_db,
            ratio,
            knee_db,
            attack_ms,
            release_ms,
            makeup_db,
            ..DynamicsParams::default()
        })
    };
    [
        // Clear without sounding processed: light evening-out, a gentle
        // turn-down between words.
        ("Clear Calls", voice(-55.0, -12.0, 1.5, -22.0, 2.0, 6.0, 15.0, 120.0, 2.0)),
        // Steady through laughs and shouts.
        ("Streamer", voice(-50.0, -20.0, 2.0, -24.0, 3.0, 6.0, 10.0, 100.0, 4.0)),
        // Polished and close.
        ("Podcast", voice(-52.0, -15.0, 1.5, -24.0, 3.0, 6.0, 10.0, 120.0, 4.0)),
        // Lighter, slower, keeping the expression.
        ("Singing", voice(-58.0, -8.0, 1.2, -18.0, 2.0, 8.0, 20.0, 150.0, 2.0)),
        // Fuller, more held: broadcast-like (pairs well with an EQ preset
        // such as Vocal presence).
        ("Radio Voice", voice(-48.0, -20.0, 2.0, -26.0, 4.0, 3.0, 5.0, 80.0, 6.0)),
    ]
}

/// Everything off: the voice as it comes in ("Reset to Natural").
pub fn natural() -> DynamicsParams {
    DynamicsParams { gate_on: false, comp_on: false, auto_on: false, makeup_db: 0.0, ..DynamicsParams::default() }
}

/// More starting points for an input, after `presets()`.
pub fn more_presets() -> [(&'static str, DynamicsParams); 1] {
    [(
        // The owner, 2026-11-10: "add more punch and attack to drums". A
        // slow attack lets each hit's front through before the compressor
        // turns the rest down, so the hit stands out; a firm expander
        // keeps the room out between hits.
        "Drums",
        DynamicsParams {
            gate_threshold_db: -45.0,
            gate_range_db: -20.0,
            exp_ratio: 4.0,
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 3.0,
            attack_ms: 30.0,
            release_ms: 120.0,
            makeup_db: 5.0,
            ..DynamicsParams::default()
        },
    )]
}

/// Starting points for an output: what the computer plays there.
pub fn output_presets() -> [(&'static str, DynamicsParams); 3] {
    let off = DynamicsParams { gate_on: false, comp_on: false, makeup_db: 0.0, ..DynamicsParams::default() };
    [
        // Never past the headroom, never louder: TotalMix's "maximum
        // volume limiter at the hardware outputs".
        ("Volume protection", DynamicsParams { auto_on: true, auto_max_gain_db: 0.0, auto_headroom_db: 6.0, auto_rise_s: 3.0, ..off }),
        // Quiet parts brought up a little, loud ones held: videos and
        // calls at one level.
        ("Even out", DynamicsParams { auto_on: true, auto_max_gain_db: 6.0, auto_headroom_db: 6.0, auto_rise_s: 3.0, ..off }),
        // Loud scenes held down, quiet speech up: listening late.
        (
            "Night",
            DynamicsParams {
                comp_on: true,
                threshold_db: -30.0,
                ratio: 4.0,
                knee_db: 6.0,
                attack_ms: 10.0,
                release_ms: 200.0,
                makeup_db: 6.0,
                auto_on: true,
                auto_max_gain_db: 0.0,
                auto_headroom_db: 3.0,
                ..off
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn sine(db: f32, seconds: f32) -> Vec<f32> {
        let a = db_to_gain(db);
        (0..(seconds * SR) as usize).map(|i| a * (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / SR).sin()).collect()
    }

    fn peak_db(x: &[f32]) -> f32 {
        gain_to_db(x.iter().fold(0.0f32, |m, v| m.max(v.abs())))
    }

    /// A quiet passage under the compressor's threshold while the makeup
    /// goes from 0 to the top: no 10 ms rises more than 1.5 dB, and it gets
    /// there within a second; back down, it falls in a few hundredths
    /// (OpenCode's job 45).
    #[test]
    fn the_makeup_rises_slowly_on_a_quiet_passage_and_falls_quickly() {
        let quiet = DynamicsParams { gate_on: false, threshold_db: -10.0, ratio: 4.0, makeup_db: 0.0, ..DynamicsParams::default() };
        let mut d = Dynamics::new(SR, quiet);
        let mut x = sine(-40.0, 0.2);
        d.process(&mut x, None);
        d.set(DynamicsParams { makeup_db: MAKEUP_MAX_DB, ..quiet });
        let window = (SR / 100.0) as usize;
        let mut was = peak_db(&x[x.len() - window..]);
        for k in 0..100 {
            let mut w = sine(-40.0, 0.01);
            d.process(&mut w, None);
            let now = peak_db(&w);
            assert!(now - was < 1.5, "window {k}: {was:.2} to {now:.2} dB");
            was = now;
        }
        assert!((was - (-40.0 + MAKEUP_MAX_DB)).abs() < 0.2, "there after a second: {was:.2}");
        d.set(quiet);
        let mut w = sine(-40.0, 0.05);
        d.process(&mut w, None);
        let tail = peak_db(&w[w.len() - window..]);
        assert!((tail - (-40.0)).abs() < 0.5, "down in 50 ms: {tail:.2}");
    }

    fn comp_only(threshold: f32, ratio: f32) -> DynamicsParams {
        DynamicsParams { gate_on: false, threshold_db: threshold, ratio, knee_db: 0.0, makeup_db: 0.0, ..DynamicsParams::default() }
    }

    #[test]
    fn under_the_threshold_nothing_changes() {
        let mut d = Dynamics::new(SR, comp_only(-20.0, 4.0));
        let mut x = sine(-30.0, 0.5);
        let before = x.clone();
        d.process(&mut x, None);
        for (a, b) in x.iter().zip(&before) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn over_it_the_ratio_holds_once_settled() {
        // −8 dBFS peaks, 12 dB over a −20 threshold at 4:1: 3 dB over, so
        // −17 dBFS once the compressor has settled.
        let mut d = Dynamics::new(SR, comp_only(-20.0, 4.0));
        let mut x = sine(-8.0, 1.0);
        d.process(&mut x, None);
        let tail = &x[x.len() - 4800..];
        assert!((peak_db(tail) + 17.0).abs() < 0.6, "{}", peak_db(tail));
        assert!(d.take_meter_db() < -8.0, "the meter shows the turn-down");
        assert_eq!(d.take_meter_db(), 0.0, "reading it starts a new span");
    }

    #[test]
    fn a_ratio_of_one_is_no_compression() {
        let mut d = Dynamics::new(SR, comp_only(-40.0, 1.0));
        let mut x = sine(-6.0, 0.3);
        d.process(&mut x, None);
        assert!((peak_db(&x[x.len() - 2400..]) + 6.0).abs() < 0.1);
    }

    #[test]
    fn the_knee_bends_smoothly_through_the_threshold() {
        let d = Dynamics::new(SR, DynamicsParams { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0, ..DynamicsParams::default() });
        assert_eq!(d.curve_db(-30.0), 0.0);
        assert!(d.curve_db(-20.0) < 0.0 && d.curve_db(-20.0) > -1.0, "a little at the threshold itself");
        assert!((d.curve_db(-8.0) + 9.0).abs() < 1e-4, "the full ratio past the knee");
        // No step anywhere across it.
        let mut last = d.curve_db(-30.0);
        let mut x = -30.0;
        while x < -10.0 {
            x += 0.1;
            let now = d.curve_db(x);
            assert!((now - last).abs() < 0.2, "step at {x}");
            last = now;
        }
    }

    #[test]
    fn the_gate_turns_quiet_parts_down_and_opens_for_loud_ones() {
        let p = DynamicsParams { comp_on: false, gate_on: true, gate_threshold_db: -45.0, gate_range_db: -40.0, makeup_db: 0.0, ..DynamicsParams::default() };
        let mut d = Dynamics::new(SR, p);
        // Room noise at −60: after the hold and the close, 40 dB down.
        let mut quiet = sine(-60.0, 1.0);
        d.process(&mut quiet, None);
        assert!((peak_db(&quiet[quiet.len() - 4800..]) + 100.0).abs() < 1.0, "{}", peak_db(&quiet[quiet.len() - 4800..]));
        // A word at −20: open within a few milliseconds.
        let mut loud = sine(-20.0, 0.1);
        d.process(&mut loud, None);
        assert!((peak_db(&loud[480..]) + 20.0).abs() < 0.5, "{}", peak_db(&loud[480..]));
    }

    #[test]
    fn a_linked_pair_gets_one_gain() {
        let mut d = Dynamics::new(SR, comp_only(-20.0, 4.0));
        let mut l = sine(-6.0, 0.3);
        let mut r: Vec<f32> = l.iter().map(|v| v * 0.1).collect();
        d.process(&mut l, Some(&mut r));
        let i = l.len() - 100;
        if l[i].abs() > 1e-3 {
            assert!((r[i] / l[i] - 0.1).abs() < 1e-4, "the image keeps its balance");
        }
    }

    #[test]
    fn a_broken_sample_doesnt_jump_the_gain() {
        // A steady tone with the compressor turning it down, one NaN,
        // then the tone again: the gain must not jump. The broken sample
        // is silenced (a step of its own, allowed); after it the sound
        // carries on at the level it had, and the steps are the tone's own.
        let p = DynamicsParams {
            gate_on: false,
            comp_on: true,
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 0.0,
            makeup_db: 4.0,
            auto_on: false,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let mut steady = sine(-8.0, 0.5);
        d.process(&mut steady, None);
        let own = steady.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        // The same tone, with one broken sample in the middle.
        let mut y = sine(-8.0, 0.5);
        let at = 12_000;
        y[at] = f32::NAN;
        d.process(&mut y, None);
        assert_eq!(y[at], 0.0, "the broken sample is silenced");
        // The level after it is the level before it: the gain did not jump.
        let level = |x: &[f32]| peak_db(&x[at + 1..at + 240]);
        assert!(
            (level(&y) - level(&steady)).abs() < 0.6,
            "the level jumped: {:.1} dB against {:.1} dB",
            level(&y),
            level(&steady)
        );
        // And the steps after it are the tone's own.
        let after = y[at + 2..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        eprintln!(
            "a broken sample: the level after it {:.1} dB, the steady take {:.1} dB; a step of {after:.5} after it (the take's own {own:.5})",
            level(&y),
            level(&steady)
        );
        assert!(after <= own * 1.5, "a step of {after} after the broken sample (the take's own {own})");
    }

    #[test]
    fn broken_samples_and_wild_settings_stay_safe() {
        let mut d = Dynamics::new(SR, DynamicsParams { makeup_db: 60.0, ratio: f32::NAN, attack_ms: f32::INFINITY, ..DynamicsParams::default() });
        assert_eq!(d.params().makeup_db, MAKEUP_MAX_DB, "makeup capped");
        assert_eq!(d.params().ratio, 1.0);
        let mut x = vec![0.5, f32::NAN, 0.5, f32::INFINITY, 0.9];
        d.process(&mut x, None);
        assert!(x.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{x:?}");
        assert_eq!(x[1], 0.0);
        assert_eq!(x[3], 0.0);
    }

    #[test]
    fn makeup_needs_the_compressor_and_a_short_right_leaves_no_left_untouched() {
        let p = DynamicsParams { comp_on: false, gate_on: false, makeup_db: 12.0, ..DynamicsParams::default() };
        let mut d = Dynamics::new(SR, p);
        let mut x = vec![0.5; 8];
        d.process(&mut x, None);
        assert!(x.iter().all(|v| (*v - 0.5).abs() < 1e-6), "no makeup with the compressor off");
        let mut d = Dynamics::new(SR, DynamicsParams { gate_on: false, makeup_db: 6.0, ..DynamicsParams::default() });
        let (mut l, mut r) = (vec![0.01; 8], vec![0.01; 4]);
        d.process(&mut l, Some(&mut r));
        assert!(l[7] > 0.015, "the left's tail gets the makeup too");
    }

    #[test]
    fn the_presets_are_within_their_ranges() {
        for (name, p) in presets() {
            let d = Dynamics::new(SR, p);
            assert_eq!(d.params(), p, "{name} needs no clamping");
        }
    }

    #[test]
    fn expander_ratio_turns_down_by_the_ratio() {
        let p = DynamicsParams {
            comp_on: false,
            gate_on: true,
            gate_threshold_db: -40.0,
            gate_range_db: -80.0,
            exp_ratio: 2.0,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let mut x = sine(-50.0, 1.0);
        d.process(&mut x, None);
        let tail = &x[x.len() - 4800..];
        assert!((peak_db(tail) + 60.0).abs() < 1.0, "{}", peak_db(tail));
    }

    #[test]
    fn the_range_floors_the_expander() {
        let p = DynamicsParams {
            comp_on: false,
            gate_on: true,
            gate_threshold_db: -40.0,
            gate_range_db: -20.0,
            exp_ratio: 10.0,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let mut x = sine(-50.0, 1.0);
        d.process(&mut x, None);
        let tail = &x[x.len() - 4800..];
        assert!((peak_db(tail) + 70.0).abs() < 1.0, "{}", peak_db(tail));
    }

    #[test]
    fn auto_level_never_passes_its_ceiling() {
        let p = DynamicsParams {
            comp_on: false,
            gate_on: false,
            auto_on: true,
            auto_headroom_db: 6.0,
            auto_max_gain_db: 12.0,
            auto_rise_s: 0.5,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let limit = db_to_gain(-6.0) + 1e-6;

        let mut x = sine(-40.0, 2.0);
        d.process(&mut x, None);
        for v in &x {
            assert!(v.abs() <= limit, "{v}");
        }

        let mut y = sine(-1.0, 0.5);
        d.process(&mut y, None);
        for v in &y {
            assert!(v.abs() <= limit, "{v}");
        }
    }

    #[test]
    fn auto_level_gain_is_capped() {
        let p = DynamicsParams {
            comp_on: false,
            gate_on: false,
            auto_on: true,
            auto_headroom_db: 6.0,
            auto_max_gain_db: 6.0,
            auto_rise_s: 0.5,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let mut x = sine(-40.0, 8.0);
        d.process(&mut x, None);
        let tail = &x[x.len() - 4800..];
        assert!((peak_db(tail) + 34.0).abs() < 0.7, "{}", peak_db(tail));
        assert!(d.auto_gain_db() <= 6.0 + 1e-3, "{}", d.auto_gain_db());
    }

    #[test]
    fn auto_level_never_pumps_room_noise() {
        let p = DynamicsParams {
            comp_on: false,
            gate_on: false,
            auto_on: true,
            auto_max_gain_db: 12.0,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let mut x = sine(-60.0, 5.0);
        d.process(&mut x, None);
        assert!(d.auto_gain_db() <= 0.01, "{}", d.auto_gain_db());
    }

    #[test]
    fn a_broken_sample_leaves_the_gain_alone() {
        // A broken sample silences that one sample; the gain stays where
        // it was. Resetting it too stepped the output by 6–10 dB in one
        // sample (a click).
        let p = DynamicsParams {
            comp_on: false,
            gate_on: false,
            auto_on: true,
            auto_headroom_db: 6.0,
            auto_max_gain_db: 6.0,
            auto_rise_s: 0.5,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        };
        let mut d = Dynamics::new(SR, p);
        let mut x = sine(-40.0, 4.0);
        d.process(&mut x, None);
        let gain = d.auto_gain_db();
        assert!(gain > 5.0, "Auto Level is at its gain: {gain}");
        let mut y = vec![f32::NAN];
        d.process(&mut y, None);
        assert_eq!(y[0], 0.0, "the broken sample is silenced");
        assert_eq!(d.auto_gain_db(), gain, "the gain stays where it was");
    }

    #[test]
    fn static_db_draws_the_curve() {
        let d = Dynamics::new(SR, DynamicsParams {
            comp_on: true,
            gate_on: false,
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        });
        assert_eq!(d.static_db(-30.0), -30.0);
        assert!((d.static_db(-8.0) + 17.0).abs() < 1e-3, "{}", d.static_db(-8.0));

        let d = Dynamics::new(SR, DynamicsParams {
            comp_on: false,
            gate_on: true,
            gate_threshold_db: -40.0,
            gate_range_db: -80.0,
            exp_ratio: 2.0,
            makeup_db: 0.0,
            ..DynamicsParams::default()
        });
        assert!((d.static_db(-50.0) + 60.0).abs() < 1e-3, "{}", d.static_db(-50.0));
    }

    #[test]
    fn every_voice_preset_ends_in_the_limiter_and_natural_is_untouched() {
        for (name, p) in presets() {
            assert!(p.auto_on && p.auto_max_gain_db == 0.0 && p.auto_headroom_db == 2.0, "{name}: a −2 dBFS limiter, no gain of its own");
            let mut d = Dynamics::new(SR, p);
            let mut x = sine(-0.5, 1.0);
            d.process(&mut x, None);
            assert!(peak_db(&x) <= -2.0 + 1e-3, "{name} holds −2 dBFS: {}", peak_db(&x));
        }
        let mut d = Dynamics::new(SR, natural());
        let mut x = sine(-12.0, 0.3);
        let before = x.clone();
        d.process(&mut x, None);
        assert!(x.iter().zip(&before).all(|(a, b)| (a - b).abs() < 1e-6), "Natural changes nothing");
    }

    #[test]
    fn the_new_presets_are_within_their_ranges_and_outputs_never_get_louder_by_themselves() {
        for (name, p) in more_presets().into_iter().chain(output_presets()) {
            let d = Dynamics::new(SR, p);
            assert_eq!(d.params(), p, "{name} needs no clamping");
        }
        // Limit and Night amplify nothing on their own; Even out at most 6 dB.
        for (name, p) in output_presets() {
            assert!(p.auto_on, "{name}: an output preset keeps the limiter on");
            assert!(p.auto_max_gain_db <= 6.0, "{name}");
        }
        let limit = output_presets()[0].1;
        let mut d = Dynamics::new(SR, limit);
        let mut x = sine(-1.0, 0.5);
        d.process(&mut x, None);
        assert!(peak_db(&x) <= -6.0 + 1e-3, "Volume protection holds the output 6 dB under full scale: {}", peak_db(&x));
    }
}
