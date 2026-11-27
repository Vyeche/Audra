//! Audra's Reverb and Echo (issue #9, phase B).
//!
//! The Babyface Pro FS computes Reverb and Echo on the host, not in its
//! DSP (manual 21.6): the unit mixes an FX send from the channels' FX Send
//! levels and hands it to the computer on frame ch12/13 of the IN stream,
//! and plays whatever comes back on ch12/13 of the OUT stream as the FX
//! return, a mixer source like any other. This crate is the part in
//! between: stereo in (the send), stereo out (the wet return). The
//! algorithms are ours (`docs/ui-spec.md` §7: "We ship our own set and our
//! own names"); they do not try to sound like RME's.
//!
//! No audio I/O here, and no allocation while processing: the engine is
//! built once per sample rate and then only runs, so it can sit in a
//! real-time loop and be tested offline.
//!
//! Safety comes first, since the return plays in the monitors: the echo's
//! feedback is capped below self-oscillation, a non-finite sample resets
//! the engine to silence, and the output is clamped to full scale.

pub mod autotune;
pub mod chain;
pub mod deesser;
pub mod denoise;
pub mod dynamics;
pub mod pitch;
pub mod echo;
pub mod gain_ramp;
mod filters;
pub mod presets;
pub mod reverb;
pub mod room_eq;

pub use chain::VoiceChain;
pub use dynamics::{Dynamics, DynamicsParams};
pub use echo::{Echo, EchoKind, EchoParams};
pub use reverb::{Reverb, ReverbKind, ReverbParams};

/// How long an effect takes to fade in or out when switched (seconds).
const SWITCH_FADE_S: f32 = 0.03;

/// How loud an audition's clap itself plays in the return, before its
/// reverb and echo: about -7 dBFS at its peak.
const CLAP_DRY: f32 = 0.5;

/// Reverb and Echo side by side on one send, their wet outputs summed into
/// the return.
pub struct FxEngine {
    pub reverb: Reverb,
    pub echo: Echo,
    pub reverb_on: bool,
    pub echo_on: bool,
    /// Each effect's output level, 0 to 1, gliding to its switch over
    /// `SWITCH_FADE_S` (OpenCode's job 61: switched off, a ringing tail
    /// fell to zero in one sample, a click in the return and in a take).
    reverb_level: f32,
    echo_level: f32,
    sample_rate: f32,
    /// Samples left of an audition's clap (`audition`), and its noise.
    clap_left: u32,
    clap_len: u32,
    clap_seed: u32,
}

impl FxEngine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            reverb: Reverb::new(sample_rate, ReverbParams::default()),
            echo: Echo::new(sample_rate, EchoParams::default()),
            reverb_on: true,
            echo_on: false,
            // Silent at birth, Reverb's switch on: the glide brings it in
            // over `SWITCH_FADE_S`, so a loop restarted (the unit plugged
            // back, a stall) fades the room in, never steps it (job 64).
            reverb_level: 0.0,
            echo_level: 0.0,
            sample_rate,
            clap_left: 0,
            clap_len: 0,
            clap_seed: 0x9E37_79B9,
        }
    }

    /// A short clap into the send, once: the return then carries only its
    /// reverb and echo, so what's switched on can be heard with no mic up
    /// (the owner, 2026-11-06: "Cant tell if reverb is working"). About
    /// 40 ms of noise falling away, peaking near -1 dBFS on the send: the
    /// wet return is far quieter (the reverb alone about -24 dBFS).
    pub fn audition(&mut self) {
        self.clap_len = (self.sample_rate * 0.04) as u32;
        self.clap_left = self.clap_len;
        self.clap_seed = 0x9E37_79B9;
    }

    /// The clap's next sample (0 when none is playing).
    fn clap(&mut self) -> f32 {
        if self.clap_left == 0 {
            return 0.0;
        }
        self.clap_left -= 1;
        // A cheap LCG: noise, no allocation, the same clap every time.
        self.clap_seed = self.clap_seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (self.clap_seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
        let t = 1.0 - self.clap_left as f32 / self.clap_len.max(1) as f32;
        noise * 0.9 * (-5.0 * t).exp()
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// The Reverb switched off with its tail still fading out.
    pub fn reverb_fading(&self) -> bool {
        !self.reverb_on && self.reverb_level > 0.0
    }

    /// The Echo switched off with its tail still fading out.
    pub fn echo_fading(&self) -> bool {
        !self.echo_on && self.echo_level > 0.0
    }

    /// Processes interleaved stereo `input` (the FX send) into interleaved
    /// stereo `output` (the FX return), frame by frame. Lengths must match;
    /// a trailing odd sample is left alone.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        let frames = input.len().min(output.len()) / 2;
        let step = 1.0 / (SWITCH_FADE_S * self.sample_rate).max(1.0);
        let glide = |level: f32, on: bool| if on { (level + step).min(1.0) } else { (level - step).max(0.0) };
        for f in 0..frames {
            let (l, r) = (input[2 * f], input[2 * f + 1]);
            // A non-finite send (a broken stream) is silence, not a seed.
            let (l, r) = if l.is_finite() && r.is_finite() { (l, r) } else { (0.0, 0.0) };
            let c = self.clap();
            let (l, r) = (l + c, r + c);
            // The clap itself rides the return too, then its room: heard,
            // it proves the return reaches the ears whatever the effects
            // make of it (the owner, on the unit: "I dont hear the
            // handclap"; the return path is issue #9's open question).
            let dry = c * CLAP_DRY;
            let mut out = (0.0, 0.0);
            // Switched off, an effect plays on, fading out, until silent.
            self.reverb_level = glide(self.reverb_level, self.reverb_on);
            if self.reverb_on || self.reverb_level > 0.0 {
                let (a, b) = self.reverb.process(l, r);
                out.0 += a * self.reverb_level;
                out.1 += b * self.reverb_level;
            }
            self.echo_level = glide(self.echo_level, self.echo_on);
            if self.echo_on || self.echo_level > 0.0 {
                let (a, b) = self.echo.process(l, r);
                out.0 += a * self.echo_level;
                out.1 += b * self.echo_level;
            }
            out.0 += dry;
            out.1 += dry;
            if !(out.0.is_finite() && out.1.is_finite()) {
                self.reset();
                out = (0.0, 0.0);
            }
            output[2 * f] = out.0.clamp(-1.0, 1.0);
            output[2 * f + 1] = out.1.clamp(-1.0, 1.0);
        }
    }

    /// Silences every tail at once.
    pub fn reset(&mut self) {
        self.reverb.reset();
        self.echo.reset();
    }
}

/// dB to a linear gain; at or under −65 dB, silence (the FX send's floor).
pub fn db_to_gain(db: f32) -> f32 {
    if db <= -65.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_an_effect_off_fades_its_tail_rather_than_cutting_it() {
        // OpenCode's job 61: a ringing tail cut in one sample is a click.
        let mut fx = FxEngine::new(48_000.0);
        let burst = vec![0.5f32; 2 * 4_800];
        let mut out = vec![0.0f32; 2 * 4_800];
        fx.process(&burst, &mut out);
        let silence = vec![0.0f32; 2 * 4_800];
        fx.process(&silence[..2 * 480], &mut out[..2 * 480]);
        let before = out[2 * 479].abs().max(out[2 * 479 + 1].abs());
        assert!(before > 1e-3, "the reverb rings: {before}");
        fx.reverb_on = false;
        let mut after = vec![0.0f32; 2 * 4_800];
        fx.process(&silence, &mut after);
        // The first sample after the switch is still about where it was:
        // no step down to zero.
        let first = after[0].abs().max(after[1].abs());
        assert!(first > before * 0.5, "no cut: {first} after {before}");
        assert!(fx.reverb_fading() || after[2 * 2_000..].iter().all(|s| *s == 0.0));
        // 30 ms on it is silent, and stays so.
        assert!(after[2 * 1_500..].iter().all(|s| *s == 0.0), "faded out by 30 ms");
        assert!(!fx.reverb_fading());
    }

    #[test]
    fn a_fresh_engine_starts_silent_and_fades_the_room_in() {
        // OpenCode's job 64: the loop makes a new engine on each restart;
        // at full from the first block, the room came back in one step.
        let mut fx = FxEngine::new(48_000.0);
        assert!(fx.reverb_on);
        assert_eq!(fx.reverb_level, 0.0, "silent at birth");
        let send = vec![0.5f32; 2 * 480];
        let mut out = vec![0.0f32; 2 * 480];
        fx.process(&send, &mut out);
        // 10 ms in: about a third of the way, not at full.
        assert!((0.3..0.37).contains(&fx.reverb_level), "{}", fx.reverb_level);
        let send = vec![0.5f32; 2 * 1_000];
        let mut out = vec![0.0f32; 2 * 1_000];
        fx.process(&send, &mut out);
        assert_eq!(fx.reverb_level, 1.0, "full by 30 ms");
    }

    #[test]
    fn switching_an_effect_on_brings_it_in_over_30_ms() {
        // Job 64's companion of the switch-off test.
        let mut fx = FxEngine::new(48_000.0);
        fx.echo_on = true;
        let send = vec![0.5f32; 2 * 720];
        let mut out = vec![0.0f32; 2 * 720];
        fx.process(&send, &mut out);
        assert!((0.45..0.55).contains(&fx.echo_level), "half way at 15 ms: {}", fx.echo_level);
        fx.process(&send, &mut out);
        assert_eq!(fx.echo_level, 1.0);
        // Never past full, the wet never past the clamp.
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn a_broken_send_never_reaches_the_monitors() {
        let mut fx = FxEngine::new(48_000.0);
        fx.echo_on = true;
        let input = [f32::NAN, 1.0, f32::INFINITY, -1.0, 0.5, 0.5];
        let mut out = [0.0; 6];
        fx.process(&input, &mut out);
        assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn silence_in_is_silence_out_once_the_tails_end() {
        let mut fx = FxEngine::new(48_000.0);
        fx.echo_on = true;
        let mut out = vec![0.0; 2 * 48_000];
        let mut input = vec![0.0; 2 * 48_000];
        input[0] = 1.0;
        input[1] = 1.0;
        fx.process(&input, &mut out);
        // Ten seconds of silence after the impulse: nothing left.
        let silence = vec![0.0; 2 * 480_000];
        let mut tail = vec![0.0; 2 * 480_000];
        fx.process(&silence, &mut tail);
        let last = &tail[tail.len() - 2 * 4800..];
        assert!(last.iter().all(|s| s.abs() < 1e-5), "the tails die away");
    }

    #[test]
    fn an_audition_rings_through_the_reverb_with_no_send() {
        let mut fx = FxEngine::new(48_000.0);
        fx.audition();
        let silence = vec![0.0f32; 2 * 48_000];
        let mut out = vec![0.0f32; 2 * 48_000];
        fx.process(&silence, &mut out);
        // Loud enough to hear in the return, never near full scale, and
        // half a second on, long after the clap, the tail still rings.
        let db = |v: f32| 20.0 * v.max(1e-9).log10();
        let peak = out.iter().map(|v| v.abs()).fold(0.0, f32::max);
        let tail: f32 = out[2 * 24_000..2 * 26_000].iter().map(|v| v.abs()).fold(0.0, f32::max);
        // The clap itself rides the return (about -7 dBFS), then the room.
        assert!(db(peak) > -10.0 && db(peak) < -3.0, "return peak {:.1} dBFS", db(peak));
        assert!(db(tail) > -66.0, "an audible tail: {:.1} dBFS", db(tail));
        // Only once: ten seconds on, nothing.
        let long = vec![0.0f32; 2 * 480_000];
        let mut later = vec![0.0f32; 2 * 480_000];
        fx.process(&long, &mut later);
        assert!(later[later.len() - 2 * 4800..].iter().all(|s| s.abs() < 1e-5), "one clap, not a loop");
    }
}

