//! TotalMix's EQ message, computed on the host (issue #15).
//!
//! TotalMix works out each EQ's filter coefficients itself and sends them
//! to the unit as one 64-byte message per channel on bulk OUT 0x0A
//! (`PROTOCOL.md`, "Windows session 3"; `docs/windows-file-analyses.md` §1):
//!
//! | Bytes | Meaning |
//! |---|---|
//! | 0 | an input's channel; on an output 0x02 (left) or 0x03 (right) |
//! | 1 | the low cut's stages, one bit each (0x03 = 12 dB/oct), while it's on |
//! | 2 | the channel: inputs from 0x00 (AN1), outputs from 0x10 (Main) |
//! | 3 | 0x80 while the EQ is on |
//! | words 1–12 | per band `[a1/a0, a2/a0, b1/b0, b2/b0]`, signed Q4.27, all zero for a flat band |
//! | word 13 | the product of every active band's b0/a0 |
//! | word 14 | the low cut's coefficient, 0x04000000 when it's off |
//!
//! The bands are the RBJ "Audio EQ Cookbook" biquads with one difference,
//! the one the Babyface driver's own comment found: α = sin(ω0/2Q), not
//! sin(ω0)/2Q. That is the Q that narrows above a few kHz in the captures,
//! and with it every captured band matches to the capture's rounding (see
//! the tests). This module is the reference for the driver too: its high
//! shelf's a1 has the opposite sign and its input strips address two
//! channels each (both noted in the project's tracker).

use crate::channel::{EqBandType, InputEq};

/// 1.0 in the message's fixed point (Q4.27).
const ONE: f64 = (1u32 << 27) as f64;
/// Word 14 with the low cut off.
pub const LOW_CUT_OFF: u32 = 0x0400_0000;

/// Which channel a message is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqChannel {
    /// An input, from 0 (AN1).
    Input(usize),
    /// An output channel, from 0 (Main left; `ChannelId::Output`).
    Output(usize),
}

impl EqChannel {
    /// Bytes 0 and 2 of its message.
    fn header(self) -> (u8, u8) {
        match self {
            EqChannel::Input(i) => (i as u8, i as u8),
            EqChannel::Output(o) => (0x02 + (o as u8 & 1), 0x10 + o as u8),
        }
    }
}

/// One band's four words and its b0/a0, or None for a band that changes
/// nothing (off, or at 0 dB), which the message carries as zeros.
pub fn band_coeffs(kind: EqBandType, freq_hz: f64, q: f64, gain_db: f64, fs: f64) -> Option<([f64; 4], f64)> {
    if kind == EqBandType::Off || gain_db == 0.0 || q <= 0.0 || freq_hz <= 0.0 {
        return None;
    }
    let a = 10f64.powf(gain_db / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * freq_hz / fs;
    let c = w0.cos();
    // TotalMix's alpha, held to pi/2 where sin() would fall again.
    let alpha = (w0 / (2.0 * q)).min(std::f64::consts::FRAC_PI_2).sin();
    let r = 2.0 * a.sqrt() * alpha;
    let (b0, b1, b2, a0, a1, a2) = match kind {
        EqBandType::Bell => (1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a),
        EqBandType::LowShelf => (
            a * ((a + 1.0) - (a - 1.0) * c + r),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - r),
            (a + 1.0) + (a - 1.0) * c + r,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - r,
        ),
        EqBandType::HighShelf => (
            a * ((a + 1.0) + (a - 1.0) * c + r),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - r),
            (a + 1.0) - (a - 1.0) * c + r,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - r,
        ),
        EqBandType::Off => return None,
    };
    Some(([a1 / a0, a2 / a0, b1 / b0, b2 / b0], b0 / a0))
}

/// Byte 1: one bit per one-pole stage of the low cut (6 dB/oct each).
pub fn low_cut_stages(slope_db_oct: u8) -> u8 {
    match slope_db_oct {
        12 => 0x03,
        18 => 0x07,
        24 => 0x0F,
        _ => 0x01,
    }
}

/// Word 14: the low cut's coefficient, as the driver fitted it at 48 kHz
/// (`bf_eq_lc_freq_raw`, cap_eq9, 0.003 %) and scaled to `fs`. The slope's
/// factor keeps the -3 dB point where the knob says; it is Windows'
/// k_n (0.654 / 0.527 / 0.457) relative to 12 dB/oct's.
pub fn low_cut_word(freq_hz: f64, slope_db_oct: u8, fs: f64) -> u32 {
    if freq_hz <= 0.0 {
        return LOW_CUT_OFF;
    }
    let f = freq_hz
        * match slope_db_oct {
            6 => 1.5267,
            18 => 0.8061,
            24 => 0.6977,
            _ => 1.0,
        };
    let at48 = 11508.0 * f * 11656.0 / (11656.0 + f);
    (at48 * 48_000.0 / fs).round().clamp(1.0, LOW_CUT_OFF as f64 - 1.0) as u32
}

fn q27(x: f64) -> i32 {
    (x * ONE).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

/// The whole message for `ch` with `eq` at sample rate `fs`. With the EQ
/// off it is TotalMix's "off" message: flat bands, unity, no low cut.
pub fn eq_block(ch: EqChannel, eq: &InputEq, fs: u32) -> [u8; 64] {
    let fs = f64::from(fs.max(8_000));
    let (b0_byte, ch_byte) = ch.header();
    let mut b = [0u8; 64];
    b[0] = b0_byte;
    b[2] = ch_byte;
    let mut gain = 1.0;
    let mut low_cut = LOW_CUT_OFF;
    if eq.enabled {
        b[3] = 0x80;
        for (slot, band) in eq.bands.iter().enumerate() {
            let Some((w, g)) = band_coeffs(band.band_type, f64::from(band.freq_hz), f64::from(band.q), f64::from(band.gain_db), fs) else {
                continue;
            };
            for (k, x) in w.iter().enumerate() {
                let at = 4 + slot * 16 + k * 4;
                b[at..at + 4].copy_from_slice(&q27(*x).to_le_bytes());
            }
            gain *= g;
        }
        if eq.low_cut_on() {
            b[1] = low_cut_stages(eq.low_cut_slope_db_oct);
            low_cut = low_cut_word(f64::from(eq.low_cut_freq_hz), eq.low_cut_slope_db_oct, fs);
        }
    }
    b[0x34..0x38].copy_from_slice(&q27(gain).to_le_bytes());
    b[0x38..0x3C].copy_from_slice(&low_cut.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::EqBand;

    const FS: f64 = 44_100.0;

    /// TotalMix's words from `cap_s4` (Windows, 44.1 kHz), with each step's
    /// settings read off its screenshot (`tools/windows/eq_analysis`).
    const CASES: &[(EqBandType, f64, f64, f64, [f64; 4])] = &[
        (EqBandType::LowShelf, 100.0, 5.0, 1.2, [-1.989619, 0.989771, -1.986114, 0.986383]),
        (EqBandType::HighShelf, 5000.0, 5.0, 1.2, [-1.039321, 0.509898, -1.287497, 0.585758]),
        (EqBandType::LowShelf, 105.0, 6.0, 1.5, [-1.991486, 0.991643, -1.987903, 0.988217]),
        (EqBandType::Bell, 1000.0, 1.5, 1.0, [-1.858401, 0.877424, -1.837176, 0.855982]),
        (EqBandType::Bell, 1050.0, 1.5, 1.3, [-1.878568, 0.899787, -1.860991, 0.882011]),
        (EqBandType::HighShelf, 5000.0, 4.0, 1.2, [-1.066922, 0.517270, -1.265416, 0.578078]),
        (EqBandType::Bell, 105.0, 6.0, 1.5, [-1.992741, 0.992964, -1.985789, 0.986011]),
        (EqBandType::LowShelf, 80.0, 1.5, 1.0, [-1.989024, 0.989143, -1.988029, 0.988170]),
    ];

    #[test]
    fn every_captured_band_matches_totalmix() {
        for &(kind, f, g, q, want) in CASES {
            let (got, _) = band_coeffs(kind, f, q, g, FS).unwrap();
            for (a, b) in got.iter().zip(want) {
                assert!((a - b).abs() < 1e-6, "{kind:?} {f} Hz {g:+} dB Q{q}: {got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn word_13_is_the_product_of_the_bands_gains() {
        let g = |i: usize| {
            let (kind, f, g, q, _) = CASES[i];
            band_coeffs(kind, f, q, g, FS).unwrap().1
        };
        // Captured: 1.580469 (bands 1 and 3), 1.598772 (all three), 1.000495.
        assert!((g(0) * g(1) - 1.580469).abs() < 1e-6);
        assert!((g(2) * g(3) * g(1) - 1.598772).abs() < 1e-6);
        assert!((g(7) - 1.000495).abs() < 1e-6);
    }

    #[test]
    fn low_cut_matches_the_capture_and_the_driver() {
        // Windows, 44.1 kHz, 20 Hz: 0.001863 (12), 0.001502 (18), 0.001301 (24).
        for (slope, want) in [(12u8, 0.001863), (18, 0.001502), (24, 0.001301)] {
            let got = f64::from(low_cut_word(20.0, slope, FS)) / ONE;
            assert!((got - want).abs() / want < 0.003, "{slope}: {got} vs {want}");
        }
        assert_eq!(low_cut_stages(24), 0x0F);
        assert_eq!(low_cut_word(0.0, 12, 48_000.0), LOW_CUT_OFF);
    }

    #[test]
    fn messages_are_laid_out_as_totalmix_sends_them() {
        let mut eq = InputEq { enabled: true, low_cut_freq_hz: 0, ..Default::default() };
        eq.bands[2] = EqBand { band_type: EqBandType::HighShelf, freq_hz: 5000, q: 1.2, gain_db: 5.0 };
        // Phones right: 0x03, the low cut off, channel 0x13, on.
        let b = eq_block(EqChannel::Output(3), &eq, 44_100);
        assert_eq!(&b[..4], &[0x03, 0x00, 0x13, 0x80]);
        assert_eq!(&b[4..0x24], &[0u8; 0x20], "bands 1 and 2 flat");
        let a1 = i32::from_le_bytes(b[0x24..0x28].try_into().unwrap());
        assert!((f64::from(a1) / ONE + 1.039321).abs() < 1e-6, "band 3's a1/a0, cookbook sign");
        assert_eq!(u32::from_le_bytes(b[0x38..0x3C].try_into().unwrap()), LOW_CUT_OFF);
        // IN3 (input 2) on its own channel, as TotalMix's Instr 3/4 went.
        assert_eq!(&eq_block(EqChannel::Input(2), &eq, 48_000)[..4], &[0x02, 0x00, 0x02, 0x80]);
        // Off: TotalMix's off message exactly.
        let off = eq_block(EqChannel::Output(0), &InputEq::default(), 48_000);
        let mut want = [0u8; 64];
        want[0] = 0x02;
        want[2] = 0x10;
        want[0x37] = 0x08;
        want[0x3B] = 0x04;
        assert_eq!(off, want);
    }
}
