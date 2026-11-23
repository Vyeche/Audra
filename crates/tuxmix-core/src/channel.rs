use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Identifies a specific channel on an RME device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum ChannelId {
    Input(usize),
    Playback(usize),
    Output(usize),
}

/// The type of an input channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
pub enum ChannelType {
    Mic,
    Instrument,
    Line,
    SPDIF,
    ADAT,
}

/// A single physical hardware input channel.
///
/// Each channel can be routed to every hardware output pair with
/// its own volume and pan — this is the submix (matrix) model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputChannel {
    pub id: usize,
    pub name: String,
    pub channel_type: ChannelType,
    /// Volume per output pair (0.0 – 1.0). Length = number of output pairs.
    pub volumes: Vec<f32>,
    /// Pan per output pair (-100 .. 100). Length = number of output pairs.
    pub pans: Vec<i8>,
    pub phantom: bool, // 48V
    pub pad: bool,
    pub sensitivity: Option<Sensitivity>,
    /// Preamp gain in dB (device-specific range — see `gain_max`).
    /// `None` for channel types with no gain control
    /// (Line/SPDIF/ADAT).
    pub gain: Option<u32>,
    /// Hardware-reported maximum for `gain`, read from the ALSA
    /// control's own range at attach time. `None` alongside `gain`.
    pub gain_max: Option<u32>,
    /// Phase Ø invert flag (bitwise-NOT of the crosspoint coefficient).
    /// `#[serde(default)]` so old scene JSON still loads.
    #[serde(default)]
    pub phase: bool,
    /// Ref-level code (Instr 3/4: +4dBu / -10dBV / Boost) as the raw
    /// Ref-level code (Instr 3/4): 0 = unset, 1 = +4dBu, 2 = -10dBV,
    /// 3 = Boost (see `usb::REF_*`; labeled cap_reflevel2.pcap).
    #[serde(default)]
    pub ref_level: u16,
    /// Hardware 3-band + low-cut DSP EQ. `Some` only for the 4 analog
    /// inputs (AN1-AN4 on the Babyface Pro FS — the only channels with
    /// an EQ strip); `None` for every other input type.
    /// `#[serde(default)]` so old scene JSON without EQ still loads.
    #[serde(default)]
    pub eq: Option<InputEq>,
    pub mute: bool,
    pub solo: bool,
    /// Trim (T), dB on the master curve (-65..+6, see
    /// `RmeDevice::set_trim`) — every hardware input has one (TotalMix
    /// shows a "T" button on all of them, analog and digital alike),
    /// unlike `gain` which only applies to Mic/Instrument. `set_trim`'s
    /// own trait default is a silent no-op (no register on the ALSA
    /// path), so this field is the only place a value set through it
    /// persists for the UI to read back — mirrored locally by whichever
    /// backend's `set_trim` override actually knows what to do with it,
    /// same as `gain`. `#[serde(default)]` so old scene JSON still loads.
    #[serde(default)]
    pub trim: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display)]
pub enum Sensitivity {
    Minus10dBV,
    Plus4dBu,
}

/// One EQ band's type (`mixer.c`'s `bf_eq_type_texts`: "Off"/"Bell"/
/// "Low Shelf"/"High Shelf" — enum items 0-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, Default)]
pub enum EqBandType {
    #[default]
    Off,
    Bell,
    LowShelf,
    HighShelf,
}

/// One of the 3 parametric bands in an [`InputEq`] strip.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EqBand {
    pub band_type: EqBandType,
    /// 20 – 20000 Hz.
    pub freq_hz: u16,
    /// Q factor, 0.05 – 10.0 (ALSA raw is Q x 100).
    pub q: f32,
    /// -24.0 – +24.0 dB (ALSA raw is dB x 10).
    pub gain_db: f32,
}

impl Default for EqBand {
    fn default() -> Self {
        Self {
            band_type: EqBandType::Off,
            freq_hz: 1000,
            q: 0.7,
            gain_db: 0.0,
        }
    }
}

/// Hardware 3-band parametric EQ + low cut on one analog input strip
/// (`eq.c`: 3 bands, a shared enable, and a low-cut filter).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct InputEq {
    pub enabled: bool,
    pub bands: [EqBand; 3],
    /// 20 – 20000 Hz, or 0 for a low cut that is switched off: the
    /// kernel driver's `EQ Low Cut Freq` takes 0 as "off" (there is no
    /// separate enable), and the protocol has a distinct off state.
    pub low_cut_freq_hz: u16,
    /// 6, 12, 18, or 24 dB/octave.
    pub low_cut_slope_db_oct: u8,
}

impl InputEq {
    /// Whether the low cut filters at all.
    pub fn low_cut_on(&self) -> bool {
        self.low_cut_freq_hz > 0
    }
}

impl Default for InputEq {
    fn default() -> Self {
        Self {
            enabled: false,
            bands: [EqBand::default(); 3],
            low_cut_freq_hz: 20,
            low_cut_slope_db_oct: 6,
        }
    }
}

/// A single software playback channel (from the computer to the device).
///
/// Same submix model: one volume + pan per hardware output pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackChannel {
    pub id: usize,
    pub name: String,
    /// Volume per output pair (0.0 – 1.0).
    pub volumes: Vec<f32>,
    /// Pan per output pair (-100 .. 100).
    pub pans: Vec<i8>,
    /// Stereo-split flag: the strip's L/R crosspoints are rewritten to
    /// 0x2000/0x0000 (split mono) instead of 0x1000/0x1000 (-6 dB pair).
    #[serde(default)]
    pub split: bool,
    /// Phase Ø invert (TotalMix's ØL/ØR on a playback channel): its
    /// crosspoints negated in every output. `#[serde(default)]` so old
    /// scene JSON still loads.
    #[serde(default)]
    pub phase: bool,
    pub mute: bool,
    pub solo: bool,
}

/// A physical hardware output (stereo pair) on the device.
///
/// Each output has a master volume, mute, and solo — the "bottom row"
/// in TotalMix. Inputs and playbacks route INTO these outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputChannel {
    pub id: usize,
    pub name: String,
    /// Master volume for this output (0.0 – 1.0).
    pub volume: f32,
    /// Loopback flag (the input signal is fed back into the playback
    /// path) — bReq 0x15, one mono channel per side (AN1/2 = ch 0/1).
    #[serde(default)]
    pub loopback: bool,
    pub mute: bool,
    pub solo: bool,
    /// CUE engaged — this output's own dedicated playback pair (`PB
    /// {pair+1}`) is exclusively monitored through the AN1/2 bus, every
    /// other playback pair muted there. See [`RmeDevice::set_cue`].
    /// Exclusive across every output (only one can be cued at a time,
    /// same as solo), not persisted in scenes — a momentary monitoring
    /// state, not a mix setting.
    #[serde(default, skip_serializing)]
    pub cue: bool,
}

impl InputChannel {
    pub fn new(id: usize, name: &str, channel_type: ChannelType, outputs: usize) -> Self {
        Self {
            id,
            name: name.to_string(),
            channel_type,
            volumes: vec![1.0; outputs],
            pans: vec![0; outputs],
            phantom: false,
            pad: false,
            sensitivity: None,
            gain: None,
            gain_max: None,
            phase: false,
            ref_level: 0,
            eq: None,
            mute: false,
            solo: false,
            trim: 0.0,
        }
    }

    /// Whether this input is a mono source, panned into each output pair:
    /// the four analog inputs.  The others are one side of a stereo pair.
    pub fn is_mono(&self) -> bool {
        matches!(
            self.channel_type,
            ChannelType::Mic | ChannelType::Instrument
        )
    }

    /// The linear levels this input sends into the left and right channel
    /// of every output pair, see [`side_levels`].
    pub fn side_levels(&self) -> Vec<[f32; 2]> {
        side_levels(self.is_mono(), self.id, &self.volumes, &self.pans)
    }
}

impl PlaybackChannel {
    pub fn new(id: usize, name: &str, outputs: usize) -> Self {
        Self {
            id,
            name: name.to_string(),
            volumes: vec![1.0; outputs],
            pans: vec![0; outputs],
            split: false,
            phase: false,
            mute: false,
            solo: false,
        }
    }

    /// The linear levels this playback channel sends into the left and
    /// right channel of every output pair, see [`side_levels`].
    pub fn side_levels(&self) -> Vec<[f32; 2]> {
        side_levels(false, self.id, &self.volumes, &self.pans)
    }
}

/// The linear left and right levels of a mono source at `volume` and
/// `pan` (-100 = left only, +100 = right only): the louder side carries the
/// volume and the other is attenuated by the pan.
pub fn pan_split(volume: f32, pan: i8) -> (f32, f32) {
    let p = (pan.clamp(-100, 100) as f32) / 100.0;
    if p <= 0.0 {
        (volume, volume * (1.0 + p))
    } else {
        (volume * (1.0 - p), volume)
    }
}

/// TotalMix's pan law for a mono source (Windows session 3, issue #13):
/// constant power, each side -3 dB at the centre and the whole level on
/// the side it's panned hard to (L = cos, R = sin over a quarter turn).
/// Only the centre and the two ends were captured; between them this is
/// the standard -3 dB law.
pub fn pan_split_power(volume: f32, pan: i8) -> (f32, f32) {
    let t = (f32::from(pan.clamp(-100, 100)) + 100.0) / 200.0 * std::f32::consts::FRAC_PI_2;
    (volume * t.cos(), volume * t.sin())
}

/// The inverse of [`pan_split_power`]: the level and pan two sides hold.
pub fn pan_join_power(l: f32, r: f32) -> (f32, i8) {
    let volume = (l * l + r * r).sqrt();
    if volume <= 0.0 {
        return (0.0, 0);
    }
    let t = r.max(0.0).atan2(l.max(0.0));
    let pan = (t / std::f32::consts::FRAC_PI_2 * 200.0 - 100.0).round().clamp(-100.0, 100.0) as i8;
    (volume, pan)
}

/// What channel `id` of a linked stereo pair sends its own side at fader
/// `volume` and the pair's balance `pan`: the left channel (even) takes
/// [`pan_split`]'s left, the right its right, so the side the balance
/// leans to stays at `volume` and the other is lowered to
/// (1 − |balance|) of it, the unit's own law (`tuxmix-usb`
/// `set_crosspoint_balance`).
pub fn balance_side(id: usize, volume: f32, pan: i8) -> f32 {
    let (l, r) = pan_split(volume, pan);
    if id.is_multiple_of(2) {
        l
    } else {
        r
    }
}

/// The linear levels a source channel sends into the left and right
/// channel of each output pair, from its per-pair volume and pan.  A mono
/// source is split by its pan.  A channel of a stereo pair (AS/ADAT inputs
/// and every playback channel) feeds only its own side, as the driver's
/// crosspoints are written: an even channel the left, an odd one the
/// right.
fn side_levels(mono: bool, id: usize, volumes: &[f32], pans: &[i8]) -> Vec<[f32; 2]> {
    volumes
        .iter()
        .enumerate()
        .map(|(pair, &v)| {
            if mono {
                let (l, r) = pan_split_power(v, pans.get(pair).copied().unwrap_or(0));
                [l, r]
            } else if id.is_multiple_of(2) {
                [v, 0.0]
            } else {
                [0.0, v]
            }
        })
        .collect()
}

impl OutputChannel {
    pub fn new(id: usize, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            volume: 1.0,
            loopback: false,
            mute: false,
            solo: false,
            cue: false,
        }
    }
}

/// A strip's level as the card should get it: its fader's, unless it is
/// muted or another strip's solo leaves it out, then silence. The model
/// keeps the fader's level either way. Every level write goes through it
/// (OpenCode's job 8: a fader moved on a muted strip, or a snapshot's
/// levels written before its mutes, brought the strip back into the
/// monitors).
pub fn heard_level(level: f32, mute: bool, solo: bool, any_solo: bool) -> f32 {
    if mute || (any_solo && !solo) {
        0.0
    } else {
        level
    }
}

/// A level that isn't a number (NaN, infinity) as silence, never as the
/// top: `NaN.min(2.0)` is 2.0, so a NaN reached the card at +6 dB, its
/// loudest (OpenCode's job 8).
pub fn safe_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume
    } else {
        0.0
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;

    #[test]
    fn a_muted_or_soloed_out_strip_is_silent_on_the_card() {
        assert_eq!(heard_level(0.8, false, false, false), 0.8);
        assert_eq!(heard_level(0.8, true, false, false), 0.0, "muted");
        assert_eq!(heard_level(0.8, false, false, true), 0.0, "another strip's solo");
        assert_eq!(heard_level(0.8, false, true, true), 0.8, "its own solo");
        assert_eq!(heard_level(0.8, true, true, true), 0.0, "muted beats soloed");
    }

    #[test]
    fn a_level_that_isnt_a_number_is_silence() {
        assert_eq!(safe_volume(f32::NAN), 0.0);
        assert_eq!(safe_volume(f32::INFINITY), 0.0);
        assert_eq!(safe_volume(f32::NEG_INFINITY), 0.0);
        assert_eq!(safe_volume(0.5), 0.5);
    }
}
