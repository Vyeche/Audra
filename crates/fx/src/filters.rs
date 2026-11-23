//! The small parts the effects are built from: a delay line, one-pole low-
//! and high-pass filters, and a Schroeder allpass.

/// A circular delay line with a fixed most length, read at any delay up to
/// it.
pub struct Delay {
    buf: Vec<f32>,
    at: usize,
}

impl Delay {
    pub fn new(max_samples: usize) -> Self {
        Self { buf: vec![0.0; max_samples.max(1) + 1], at: 0 }
    }

    /// The sample written `delay` samples ago (at least 1).
    pub fn read(&self, delay: usize) -> f32 {
        let n = self.buf.len();
        let d = delay.clamp(1, n - 1);
        self.buf[(self.at + n - d) % n]
    }

    pub fn write(&mut self, x: f32) {
        self.buf[self.at] = x;
        self.at = (self.at + 1) % self.buf.len();
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
    }

    pub fn capacity(&self) -> usize {
        self.buf.len() - 1
    }
}

/// One-pole low-pass: `cutoff_hz` at `sample_rate`.
pub struct LowPass {
    a: f32,
    z: f32,
}

impl LowPass {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let mut f = Self { a: 0.0, z: 0.0 };
        f.set(cutoff_hz, sample_rate);
        f
    }

    pub fn set(&mut self, cutoff_hz: f32, sample_rate: f32) {
        let c = cutoff_hz.clamp(10.0, sample_rate * 0.49);
        self.a = 1.0 - (-2.0 * std::f32::consts::PI * c / sample_rate).exp();
    }

    pub fn process(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        // Kept out of the denormal range, which is slow on some CPUs.
        if self.z.abs() < 1e-20 {
            self.z = 0.0;
        }
        self.z
    }

    pub fn clear(&mut self) {
        self.z = 0.0;
    }
}

/// One-pole high-pass: the input less its low-passed self.
pub struct HighPass {
    lp: LowPass,
}

impl HighPass {
    pub fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        Self { lp: LowPass::new(cutoff_hz, sample_rate) }
    }

    pub fn set(&mut self, cutoff_hz: f32, sample_rate: f32) {
        self.lp.set(cutoff_hz, sample_rate);
    }

    pub fn process(&mut self, x: f32) -> f32 {
        x - self.lp.process(x)
    }

    pub fn clear(&mut self) {
        self.lp.clear();
    }
}

/// A Schroeder allpass: smears an impulse into a burst without colouring
/// its spectrum (the reverb's input diffusion).
pub struct AllPass {
    line: Delay,
    delay: usize,
    pub gain: f32,
}

impl AllPass {
    pub fn new(delay: usize, gain: f32) -> Self {
        Self { line: Delay::new(delay), delay, gain }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let d = self.line.read(self.delay);
        let v = x + self.gain * d;
        self.line.write(v);
        d - self.gain * v
    }

    pub fn clear(&mut self) {
        self.line.clear();
    }
}
