//! Noise removal for what others hear (the owner's pick from ChatGPT's
//! list, 2026-11-10: "okay sounds good"): fans, air conditioning, hum and
//! hiss turned down under the voice. Xiph's RNNoise, in pure Rust
//! (`nnnoiseless`, BSD-3): a small neural net that finds the voice in each
//! 10 ms frame and turns the rest down.
//!
//! Measured with the EBU's speech (`tools/checks/totalMix-voice-quality.py`,
//! 2026-11-10): with fan noise or hiss 20 dB under the voice, the pauses
//! come out 8 dB (Light) or 18 dB (Strong) quieter and the voice no worse;
//! 10 dB under, the voice 6–7 dB cleaner; clean speech keeps its level
//! within 0.1 dB per band (the waveform changes about 20 dB under the
//! voice, mostly a phase-like change under 300 Hz, and the softest word
//! tails lose 3–4 dB). So in a quiet room (noise 30 dB under) it changes
//! the voice more than the noise: the mic check offers it only when the
//! room is close under the voice. Hiss with no voice ever is turned down
//! unevenly (1–12 dB by level).
//!
//! **Light** and **Strong** don't mix the raw sound back in, which would
//! bring the noise back too (a second opinion's caution, checked): they
//! limit how far the noise goes down, −8 dB and −18 dB, by blending the
//! frame RNNoise heard (aligned to its one-frame delay) under its output
//! at that level. A voice passes as it was. The floor glides from one
//! level's to the other's over 10 ms, so swapping them mid-word does not
//! step the room.
//!
//! RNNoise works on 480-sample frames at 48 kHz: our blocks (256) are
//! gathered into frames and the result handed back a frame later, so it
//! adds 959 samples, about 20 ms, while it's on (Off adds nothing). At any
//! other rate it stands aside. No allocation while processing.

use nnnoiseless::DenoiseState;

/// RNNoise's frame: 10 ms at 48 kHz.
const FRAME: usize = DenoiseState::FRAME_SIZE;
/// Its samples are 16-bit PCM in an `f32`.
const PCM: f32 = 32768.0;
/// The rate it's made for.
pub const RATE: f32 = 48_000.0;
/// How far Light and Strong turn the noise down at most, in dB.
pub const LIGHT_DB: f32 = -8.0;
pub const STRONG_DB: f32 = -18.0;
/// How long the floor takes to glide from one level's to the other's.
const FLOOR_MS: f32 = 10.0;

/// One side's frames: the samples gathered, the frame before (for the
/// blend), and what's ready to hand back.
struct Side {
    state: Box<DenoiseState<'static>>,
    gather: [f32; FRAME],
    before: [f32; FRAME],
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
    /// A ring of what's ready: the net's output and the dry signal it
    /// came from, side by side, so the floor can glide (a floor that
    /// steps is a click in the room between words). `FRAME - 1` waiting
    /// at the start, so a sample can always be handed back.
    ready_wet: [f32; 2 * FRAME],
    ready_dry: [f32; 2 * FRAME],
    head: usize,
    len: usize,
}

impl Side {
    fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            gather: [0.0; FRAME],
            before: [0.0; FRAME],
            frame_in: [0.0; FRAME],
            frame_out: [0.0; FRAME],
            ready_wet: [0.0; 2 * FRAME],
            ready_dry: [0.0; 2 * FRAME],
            head: 0,
            len: FRAME - 1,
        }
    }

    fn clear(&mut self) {
        // The frames start afresh. The net keeps what it learned of the
        // room: it is fresh when the engine is made, and a fresh one here
        // would mean allocating on the audio thread (the state holds nine
        // blocks of its own, so even a copy of it allocates).
        self.gather = [0.0; FRAME];
        self.before = [0.0; FRAME];
        self.ready_wet = [0.0; 2 * FRAME];
        self.ready_dry = [0.0; 2 * FRAME];
        self.head = 0;
        self.len = FRAME - 1;
    }

    /// One sample in at `filled` of the frame, one sample out.
    fn step(&mut self, x: f32, filled: usize, floor: f32) -> f32 {
        self.gather[filled] = if x.is_finite() { x } else { 0.0 };
        if filled + 1 == FRAME {
            for (dst, src) in self.frame_in.iter_mut().zip(self.gather.iter()) {
                *dst = src * PCM;
            }
            self.state.process_frame(&mut self.frame_out, &self.frame_in);
            // Its output is the frame before, cleaned: the noise under it
            // brought back up to the floor, the voice as it was. The wet
            // and the dry wait side by side, so the floor can glide.
            let tail = (self.head + self.len) % self.ready_wet.len();
            for k in 0..FRAME {
                let wet = self.frame_out[k] / PCM;
                let dry = self.before[k];
                self.ready_wet[(tail + k) % self.ready_wet.len()] = if wet.is_finite() { wet } else { 0.0 };
                self.ready_dry[(tail + k) % self.ready_dry.len()] = if dry.is_finite() { dry } else { 0.0 };
            }
            self.len += FRAME;
            self.before = self.gather;
        }
        let wet = self.ready_wet[self.head];
        let dry = self.ready_dry[self.head];
        self.head = (self.head + 1) % self.ready_wet.len();
        self.len -= 1;
        // The floor: how much of the dry is mixed back in, so the noise
        // goes down no further than the level allows.
        let out = wet + (dry - wet) * floor;
        if out.is_finite() { out } else { 0.0 }
    }
}

/// Noise removal for one channel or a linked pair (each side its own net).
pub struct Denoise {
    sides: [Side; 2],
    filled: usize,
    level: u8,
    active: bool,
    /// The floor now, and where it is going: Light and Strong differ only
    /// in the floor, and a floor that steps is a click in the room
    /// between words, so it glides over `FLOOR_MS`.
    floor: f32,
    floor_to: f32,
    floor_step: f32,
}

/// The floor for a level: how much of the dry is mixed back in, so the
/// noise goes down no further than the level allows.
fn floor_of(level: u8) -> f32 {
    match level {
        1 => 10f32.powf(LIGHT_DB / 20.0),
        2 => 10f32.powf(STRONG_DB / 20.0),
        _ => 0.0,
    }
}

impl Denoise {
    pub fn new(sample_rate: f32) -> Self {
        let rate = if sample_rate.is_finite() && sample_rate > 0.0 { sample_rate } else { 48_000.0 };
        Self {
            sides: [Side::new(), Side::new()],
            filled: 0,
            level: 0,
            active: (sample_rate - RATE).abs() < 1.0,
            floor: 0.0,
            floor_to: 0.0,
            floor_step: 1.0 / (FLOOR_MS * 0.001 * rate).max(1.0),
        }
    }

    /// 0 off, 1 light, 2 strong. Turned on, its frames start afresh (the
    /// net keeps what it learned of the room: building a new one here
    /// would allocate on the audio thread); Light and Strong swap without
    /// a break (only the floor differs; clearing there cut 20 ms of
    /// silence into the voice). The floor glides to the new level's, so
    /// the swap does not step the noise.
    pub fn set_level(&mut self, level: u8) {
        let level = level.min(2);
        if level != self.level {
            if self.level == 0 {
                for s in &mut self.sides {
                    s.clear();
                }
                self.filled = 0;
                // From silence the floor starts at the new level's: no step to
                // smooth, and the chain's crossfade covers the change.
                self.floor = floor_of(level);
            }
            self.level = level;
            self.floor_to = floor_of(level);
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// Whether it can work at this rate (48 kHz only).
    pub fn works_here(&self) -> bool {
        self.active
    }

    /// What it delays the sound by now, in samples (0 while off).
    pub fn latency(&self) -> usize {
        if self.active && self.level > 0 {
            2 * FRAME - 1
        } else {
            0
        }
    }

    /// Processes a block in place: `left`, and `right` for a linked pair.
    pub fn process(&mut self, left: &mut [f32], mut right: Option<&mut [f32]>) {
        if !self.active || self.level == 0 {
            return;
        }
        for (i, slot) in left.iter_mut().enumerate() {
            // The floor glides to its target, a step at a time: Light and
            // Strong must not step the noise (a step is a click between
            // words).
            let d = self.floor_to - self.floor;
            self.floor += if d.abs() <= self.floor_step { d } else { d.signum() * self.floor_step };
            let filled = self.filled;
            *slot = self.sides[0].step(*slot, filled, self.floor);
            if let Some(r) = right.as_mut().and_then(|r| r.get_mut(i)) {
                *r = self.sides[1].step(*r, filled, self.floor);
            }
            self.filled = (filled + 1) % FRAME;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counting allocator, so a test can show that a piece of work
    /// allocates nothing: the crate's rule is that nothing is allocated
    /// while processing.
    mod allocs {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            /// How many allocations this thread has made, while it counts.
            static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
        }

        pub struct Counting;

        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                count_one();
                System.alloc(layout)
            }
            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                System.dealloc(ptr, layout)
            }
            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                count_one();
                System.realloc(ptr, layout, new_size)
            }
        }

        fn count_one() {
            let _ = COUNT.try_with(|c| {
                if let Some(n) = c.get() {
                    c.set(Some(n + 1));
                }
            });
        }

        /// How many allocations `f` makes on this thread.
        pub fn count<T>(f: impl FnOnce() -> T) -> (T, usize) {
            COUNT.with(|c| c.set(Some(0)));
            let out = f();
            let n = COUNT.with(|c| c.get().unwrap_or(0));
            COUNT.with(|c| c.set(None));
            (out, n)
        }
    }

    #[global_allocator]
    static ALLOCATOR: allocs::Counting = allocs::Counting;

    /// The noise removal's switch must not allocate: it runs on the audio
    /// thread, where an allocation is a glitch.
    #[test]
    fn switching_the_noise_removal_on_allocates_nothing() {
        let mut d = Denoise::new(RATE);
        // The net is built when the engine is made; that is the start, not
        // the audio thread. Switching it on starts the frames afresh: no
        // net is built there, so nothing may be allocated.
        let ((), n) = allocs::count(|| d.set_level(1));
        assert_eq!(n, 0, "switching the noise removal on allocates");
        // Light to Strong and back: the same.
        let ((), n) = allocs::count(|| {
            d.set_level(2);
            d.set_level(1);
        });
        assert_eq!(n, 0, "changing the level allocates");
    }

    /// What the net's own frame costs: it allocates on every frame (the
    /// dependency's own doing, not ours). Kept as a note, not a rule:
    /// the crate's rule is about our own paths.
    #[test]
    fn what_the_nets_own_frame_costs() {
        let mut s = DenoiseState::new();
        let inp = vec![0.0f32; 480];
        let mut out = vec![0.0f32; 480];
        let (_, n) = allocs::count(|| s.process_frame(&mut out, &inp));
        eprintln!("the net's own frame: {n} allocations");
    }

    const SR: f32 = 48_000.0;

    fn voice(seconds: f32) -> Vec<f32> {
        (0..(seconds * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                let f0 = 140.0 + 15.0 * (t * 2.0).sin();
                (1..=12).map(|k| (std::f32::consts::TAU * f0 * k as f32 * t).sin() / k as f32).sum::<f32>() * 0.15
            })
            .collect()
    }

    /// A fan: noise low-passed at about 300 Hz with a 120 Hz hum (RNNoise
    /// turns this down 18–44 dB on its own).
    fn noise(seconds: f32, amp: f32) -> Vec<f32> {
        let mut seed = 0x1234_5678u32;
        let mut lp = 0.0f32;
        let a = (-2.0 * std::f32::consts::PI * 300.0 / SR).exp();
        (0..(seconds * SR) as usize)
            .map(|i| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let w = seed as f32 / u32::MAX as f32 - 0.5;
                lp = w + (lp - w) * a;
                lp * 8.0 * amp + 0.3 * amp * (std::f32::consts::TAU * 120.0 * i as f32 / SR).sin()
            })
            .collect()
    }

    fn energy_db(x: &[f32]) -> f32 {
        10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).max(1e-20).log10()
    }

    /// Runs `x` through at `level` in 256-sample blocks, as the loop does.
    fn run(level: u8, x: &[f32]) -> (Vec<f32>, usize) {
        let mut d = Denoise::new(SR);
        d.set_level(level);
        let mut y = x.to_vec();
        for block in y.chunks_mut(256) {
            d.process(block, None);
        }
        (y, d.latency())
    }

    #[test]
    fn off_changes_nothing() {
        let x = voice(0.5);
        let (y, lat) = run(0, &x);
        assert_eq!(y, x);
        assert_eq!(lat, 0);
    }

    #[test]
    fn a_voice_passes_a_frame_and_a_bit_later() {
        let x = voice(2.0);
        let (y, lat) = run(1, &x);
        assert_eq!(lat, 959);
        let (a, b) = (&x[20_000..80_000], &y[20_000 + lat..80_000 + lat]);
        assert!((energy_db(b) - energy_db(a)).abs() < 1.0, "{} vs {}", energy_db(b), energy_db(a));
        // The same sound, the delay later.
        let err = a.iter().zip(b).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
        assert!(err < 0.08, "{err}");
    }

    #[test]
    fn strong_turns_noise_down_but_no_further_than_its_limit() {
        let x = noise(3.0, 0.05);
        let (y, lat) = run(2, &x);
        let drop = energy_db(&y[48_000 + lat..]) - energy_db(&x[48_000..x.len() - lat]);
        assert!((STRONG_DB - 2.0..=STRONG_DB + 4.0).contains(&drop), "Strong: {drop:.1} dB");
    }

    #[test]
    fn light_turns_noise_down_no_more_than_8_db() {
        let x = noise(3.0, 0.05);
        let (y, lat) = run(1, &x);
        let drop = energy_db(&y[48_000 + lat..]) - energy_db(&x[48_000..x.len() - lat]);
        assert!((LIGHT_DB - 1.5..=LIGHT_DB + 2.0).contains(&drop), "Light: {drop:.1} dB");
    }

    #[test]
    fn broken_samples_and_other_rates_are_safe() {
        let mut d = Denoise::new(SR);
        d.set_level(2);
        let mut x = vec![f32::NAN; 1024];
        x[3] = f32::INFINITY;
        d.process(&mut x, None);
        assert!(x.iter().all(|v| v.is_finite()));
        let mut d = Denoise::new(44_100.0);
        d.set_level(2);
        let mut y = voice(0.2);
        let before = y.clone();
        d.process(&mut y, None);
        assert_eq!(y, before, "stands aside at 44.1 kHz");
        assert!(!d.works_here() && d.latency() == 0);
    }

    #[test]
    fn a_pair_keeps_its_balance() {
        let mut d = Denoise::new(SR);
        d.set_level(1);
        let mut l = voice(1.0);
        let mut r: Vec<f32> = l.iter().map(|v| v * 0.5).collect();
        for (bl, br) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            d.process(bl, Some(br));
        }
        let (el, er) = (energy_db(&l[24_000..]), energy_db(&r[24_000..]));
        assert!((el - er - 6.02).abs() < 1.0, "{el} {er}");
    }
}

