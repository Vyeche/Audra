//! A gain that fades to a new value over a fixed time, so a change of an
//! effect's level is never a step (OpenCode's job 42: a preset load stepped
//! the wet level by tens of dB). Written by the local Qwen to a spec;
//! checked and tested by Claude.

/// A gain that moves to a new value over a fixed number of samples, in a
/// straight line (linear in amplitude): a change of level is a short fade.
pub struct GainRamp {
    current: f32,
    target: f32,
    /// Where the ramp under way started: each value is computed from it, so
    /// rounding never piles up over thousands of samples (Claude: added
    /// steps drifted, and the last one came out 6 % too big).
    start: f32,
    step: f32,
    left: u32,
    length: u32,
}

impl GainRamp {
    pub fn new(gain: f32, length: u32) -> Self {
        let gain = if !gain.is_finite() || gain < 0.0 { 0.0 } else { gain };
        let length = if length == 0 { 1 } else { length };
        GainRamp {
            current: gain,
            target: gain,
            start: gain,
            step: 0.0,
            left: 0,
            length,
        }
    }

    pub fn set(&mut self, target: f32) {
        let target = if !target.is_finite() || target < 0.0 { 0.0 } else { target };
        if target == self.target {
            return;
        }
        self.target = target;
        self.start = self.current;
        self.left = self.length;
        self.step = (self.target - self.current) / self.length as f32;
    }

    pub fn advance(&mut self) -> f32 {
        if self.left > 0 {
            self.left -= 1;
            let done = (self.length - self.left) as f32;
            self.current = if self.left == 0 { self.target } else { self.start + self.step * done };
        }
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn length_for(sample_rate: f32, ms: f32) -> u32 {
        let samples = (sample_rate * ms / 1000.0).round() as i64;
        if samples < 1 {
            1
        } else {
            samples as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starts_at_gain() {
        let mut gr = GainRamp::new(0.5, 100);
        assert!((gr.advance() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_rise_straight_line() {
        let mut gr = GainRamp::new(0.0, 10);
        gr.set(1.0);
        for i in 1..=10 {
            let expected = i as f32 / 10.0;
            assert!((gr.advance() - expected).abs() < 1e-6, "i={}", i);
        }
        // 11th value should be exactly target
        assert!((gr.advance() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_fall_straight_line() {
        let mut gr = GainRamp::new(1.0, 10);
        gr.set(0.0);
        for i in 1..=10 {
            let expected = 1.0 - (i as f32 / 10.0);
            assert!((gr.advance() - expected).abs() < 1e-6, "i={}", i);
        }
        assert!((gr.advance() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_set_same_target_mid_ramp_no_restart() {
        let mut gr = GainRamp::new(0.0, 10);
        gr.set(1.0);
        // Advance 5 steps
        for _ in 0..5 {
            gr.advance();
        }
        // Set to same target again - should not restart
        gr.set(1.0);
        // Next value should continue the ramp: step is 0.1, current after 5 steps is ~0.5
        // After 6th step it should be ~0.6
        let val = gr.advance();
        assert!((val - 0.6).abs() < 1e-6);
    }

    #[test]
    fn test_new_target_mid_ramp_starts_from_current() {
        let mut gr = GainRamp::new(0.0, 10);
        gr.set(1.0);
        // Advance 5 steps: current should be ~0.5
        for _ in 0..5 {
            gr.advance();
        }
        // Now set to 0.0 - starts a new ramp from current (~0.5) over 10 samples
        gr.set(0.0);
        let val = gr.advance();
        // step = (0.0 - 0.5) / 10 = -0.05, so next = 0.5 - 0.05 = 0.45
        assert!((val - 0.45).abs() < 1e-6);
    }

    #[test]
    fn test_non_finite_and_negative_become_zero() {
        let gr = GainRamp::new(f32::NAN, 10);
        assert_eq!(gr.current(), 0.0);
        assert_eq!(gr.target(), 0.0);

        let mut gr2 = GainRamp::new(1.0, 10);
        gr2.set(-5.0);
        assert_eq!(gr2.target(), 0.0);

        let mut gr3 = GainRamp::new(1.0, 10);
        gr3.set(f32::INFINITY);
        assert_eq!(gr3.target(), 0.0);
    }

    #[test]
    fn test_length_for() {
        assert_eq!(GainRamp::length_for(48_000.0, 50.0), 2400);
        assert_eq!(GainRamp::length_for(48_000.0, 0.0), 1);
    }

    #[test]
    fn test_no_step_bigger_than_max() {
        let mut gr = GainRamp::new(0.0006, 2400);
        gr.set(2.0);
        let max_step = (2.0 - 0.0006) / 2400.0;
        let mut prev = gr.current();
        for _ in 0..2400 {
            let val = gr.advance();
            let step_size = (val - prev).abs();
            // f32 rounding near 2.0 is about 2e-7 (Claude: 1e-9 was tighter
            // than the type can hold).
            assert!(step_size <= max_step + 1e-6, "step {} > max {}", step_size, max_step);
            prev = val;
        }
    }
}
