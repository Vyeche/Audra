//! Babyface Pro (FS) implementation of the [`RmeDevice`] trait.

use alsa::mixer::{MilliBel, Selem, SelemChannelId};
use alsa::Round;
use log::info;

use crate::channel::OutputChannel;
use crate::channel::*;
use crate::curves::{send_db_to_raw, send_raw_to_db};
use crate::device::{DeviceSettings, RmeDevice};
use crate::error::Error;
use crate::mixer::AlsaMixer;
use crate::profile::DeviceProfile;
use crate::profiles::babyface_pro::PROFILE;
use crate::scene::Scene;

// ── Helpers ────────────────────────────────────────────────────
//
// The ALSA control names/indices below were cross-checked 2026-08-28
// against the real kernel driver (`babyface-pro-linux/tools/kernel/
// mixer.c`) and live `amixer -c FS scontents` output on real hardware
// — not guessed. The previous grammar this file used
// (`"<Type>-<Name>-<Output>"`, `" 48V"`/`" PAD"`/`" Sens."` suffixes)
// never matched anything the kernel driver actually creates, so every
// crosspoint/phantom/pad/gain read-write was a silent no-op.

/// One entry per `mixer.c`'s `bf_sources[14]` crosspoint source, in the
/// same order (source index = position in this array = `src` in the
/// kernel's `index = out*14 + src`). The first 4 (AN1-AN4) are true
/// mono sources: their control's front-left/front-right ALSA channels
/// write the source's level into the output pair's left/right side, so
/// TotalMix-style volume+pan applies (see `input_crosspoint_slot`). The
/// other 10 are hardware-linked stereo pairs (AS1/2, the ADAT pairs,
/// PB1-PB6): front-left/front-right are each physical channel's OWN
/// level into the output pair, with no cross-routing — there is no
/// ALSA-level way to route e.g. AS1 into an output's right side, so pan
/// does not apply to those.
const BF_SOURCES: [&str; 14] = [
    "AN1", "AN2", "AN3", "AN4", "AS1/2", "ADAT3/4", "ADAT5/6", "ADAT7/8", "PB1", "PB2", "PB3",
    "PB4", "PB5", "PB6",
];

/// `mixer.c`'s `out_names[6]` — the master-volume/mute ALSA control
/// name per output pair, in `self.profile.outputs` order (both use the
/// canonical AN1/2, PH3/4, ... order — hardware-verified in `mixer.c`
/// not to need reordering).
const PAIR_LABELS: [&str; 6] = ["AN1/2", "PH3/4", "AS1/2", "ADAT3/4", "ADAT5/6", "ADAT7/8"];

/// Which ALSA control grammar the attached card speaks.
///
/// The Babyface Pro FS presents two USB personalities and they expose
/// *structurally different* mixers, not just differently-named ones:
///
/// |                    | [`Proprietary`](ControlGrammar::Proprietary) | [`ClassCompliant`](ControlGrammar::ClassCompliant) |
/// |---|---|---|
/// | crosspoints        | 84 = 14 sources × 6 output **pairs**         | 288 = 24 sources × 12 destination **channels** |
/// | disambiguated by   | selem `.index` (`output * 14 + src`)         | the name itself; `.index` is always 0 |
/// | a mono source's pan| two channels of one stereo selem             | two separate mono controls |
///
/// Verified by dumping both surfaces from the same physical card
/// (`docs/reference/class-compliant-controls.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlGrammar {
    /// The from-scratch `snd-usb-babyface-pro` driver (USB `2a39:3fc0`).
    /// The full feature set: EQ, front panel, phase, trim, loopback…
    Proprietary,
    /// Stock `snd-usb-audio` on the class-compliant personality (USB
    /// `2a39:3fb0`). Routing, preamps and clock work; the DSP features
    /// that live in RME's own protocol do not exist here.
    ClassCompliant,
}

/// A crosspoint source's name in the class-compliant grammar, for
/// `BF_SOURCES[src]` and — for the ten paired sources — which side.
///
/// The pairing is the same one [`input_crosspoint_slot`] already
/// encodes for the proprietary grammar; only the spelling differs.
fn cc_source_name(src: usize, right: bool) -> Option<&'static str> {
    Some(match (src, right) {
        (0, _) => "Mic-AN1",
        (1, _) => "Mic-AN2",
        (2, _) => "Line-IN3",
        (3, _) => "Line-IN4",
        (4, false) => "Line-AS1",
        (4, true) => "Line-AS2",
        (5, false) => "Line-ADAT3",
        (5, true) => "Line-ADAT4",
        (6, false) => "Line-ADAT5",
        (6, true) => "Line-ADAT6",
        (7, false) => "Line-ADAT7",
        (7, true) => "Line-ADAT8",
        (8, false) => "PCM-AN1",
        (8, true) => "PCM-AN2",
        (9, false) => "PCM-PH3",
        (9, true) => "PCM-PH4",
        (10, false) => "PCM-AS1",
        (10, true) => "PCM-AS2",
        (11, false) => "PCM-ADAT3",
        (11, true) => "PCM-ADAT4",
        (12, false) => "PCM-ADAT5",
        (12, true) => "PCM-ADAT6",
        (13, false) => "PCM-ADAT7",
        (13, true) => "PCM-ADAT8",
        _ => return None,
    })
}

/// A destination channel's name in the class-compliant grammar, for
/// output pair `out` (same order as [`PAIR_LABELS`]) and side.
fn cc_dest_name(out: usize, right: bool) -> Option<&'static str> {
    Some(match (out, right) {
        (0, false) => "AN1",
        (0, true) => "AN2",
        (1, false) => "PH3",
        (1, true) => "PH4",
        (2, false) => "AS1",
        (2, true) => "AS2",
        (3, false) => "ADAT3",
        (3, true) => "ADAT4",
        (4, false) => "ADAT5",
        (4, true) => "ADAT6",
        (5, false) => "ADAT7",
        (5, true) => "ADAT8",
        _ => return None,
    })
}

/// Maps a `PROFILE.inputs` index to its crosspoint source: which of
/// `BF_SOURCES` it is, and — for the non-mono (linked-pair) sources —
/// which single ALSA channel (front-left/front-right) is *this*
/// input's own level, since the pair's other channel belongs to the
/// sibling input (e.g. `AS1` is `BF_SOURCES[4]` front-left, `AS2` is
/// the same source's front-right).
fn input_crosspoint_slot(idx: usize) -> Option<(usize, SelemChannelId, bool)> {
    use SelemChannelId::{FrontLeft as L, FrontRight as R};
    Some(match idx {
        0 => (0, L, true),   // AN1 (Mic 1)
        1 => (1, L, true),   // AN2 (Mic 2)
        2 => (2, L, true),   // IN3 -> hardware source "AN3"
        3 => (3, L, true),   // IN4 -> hardware source "AN4"
        4 => (4, L, false),  // AS1
        5 => (4, R, false),  // AS2
        6 => (5, L, false),  // ADAT3
        7 => (5, R, false),  // ADAT4
        8 => (6, L, false),  // ADAT5
        9 => (6, R, false),  // ADAT6
        10 => (7, L, false), // ADAT7
        11 => (7, R, false), // ADAT8
        _ => return None,
    })
}

/// Same idea for `PROFILE.outputs`-derived playback channels: each of
/// the 6 `PBn` sources is one playback pair (`PCM <left>`/`PCM
/// <right>`), always linked (no mono PB source exists).
fn playback_crosspoint_slot(idx: usize) -> Option<(usize, SelemChannelId)> {
    if idx >= 12 {
        return None;
    }
    let src = 8 + idx / 2;
    let ch = if idx.is_multiple_of(2) {
        SelemChannelId::FrontLeft
    } else {
        SelemChannelId::FrontRight
    };
    Some((src, ch))
}

/// EQ strips exist only on the 4 analog inputs (`eq.c`'s `names[4] =
/// {"AN1","AN2","AN3","AN4"}`), which happen to be exactly
/// `BF_SOURCES[0..4]`.
fn eq_strip_name(idx: usize) -> Option<&'static str> {
    if idx < 4 {
        Some(BF_SOURCES[idx])
    } else {
        None
    }
}

/// One of the driver's input-EQ controls, or None once Audra computes the
/// EQ itself ("DSP Block"): then the driver's strips stay off and only the
/// model and Audra's message change.
fn eq_selem<'a>(mixer: &'a AlsaMixer, audra_eq: bool, name: &str, index: u32) -> Option<Selem<'a>> {
    if audra_eq {
        None
    } else {
        mixer.find_selem(name, index)
    }
}

/// `eq.c`'s `bf_eq_type_texts` enum-item order: Off/Bell/Low Shelf/High Shelf.
fn eq_band_type_to_enum(t: EqBandType) -> u32 {
    match t {
        EqBandType::Off => 0,
        EqBandType::Bell => 1,
        EqBandType::LowShelf => 2,
        EqBandType::HighShelf => 3,
    }
}

fn eq_band_type_from_enum(v: u32) -> EqBandType {
    match v {
        1 => EqBandType::Bell,
        2 => EqBandType::LowShelf,
        3 => EqBandType::HighShelf,
        _ => EqBandType::Off,
    }
}

/// `eq.c`'s `bf_eq_slope_texts` enum-item order: 6/12/18/24 dB/oct.
fn eq_slope_to_enum(slope_db_oct: u8) -> u32 {
    match slope_db_oct {
        12 => 1,
        18 => 2,
        24 => 3,
        _ => 0, // 6 dB/oct, and the fallback for any other value
    }
}

fn eq_slope_from_enum(v: u32) -> u8 {
    match v {
        1 => 12,
        2 => 18,
        3 => 24,
        _ => 6,
    }
}

/// A mono input's route into a stereo output pair is really *two*
/// independent ALSA volumes (one per crosspoint channel) — there is no
/// separate "pan" control on the hardware. TotalMix's single
/// volume+pan fader is a UI convenience over that pair, so this
/// decodes raw front-left/front-right volumes into the same
/// convention: the louder side sets volume, and how far the quieter
/// side is attenuated sets pan. `max` is the ALSA control's own
/// reported full-scale (`get_playback_volume_range().1`) — the real
/// hardware ranges differ per control (crosspoints top out at 0x2d41,
/// master volume at 0x4000), so this must never be hardcoded.
/// `encode_volume_pan` is the exact inverse, so round-tripping through
/// both is lossless (mod integer rounding).
fn decode_volume_pan(l_raw: i64, r_raw: i64, max: f32) -> (f32, i8) {
    pan_join(l_raw as f32 / max, r_raw as f32 / max)
}

/// A single-channel source's two sides into an output with Mono on: both
/// at their mean, (L+R)/2, as TotalMix writes it (PROTOCOL.md "MONO and
/// PHASE are crosspoint writes": 0 dB on one side became -6 dB on both).
fn mono_sides((l, r): (f32, f32), mono: bool) -> (f32, f32) {
    if mono {
        let m = (l + r) / 2.0;
        (m, m)
    } else {
        (l, r)
    }
}

/// TotalMix's stereo width on one channel's two sides (issue #7;
/// PROTOCOL.md "Width strip mapping": L = 0x1000·(1+w), R = 0x1000·(1-w)
/// for a source at 0 dB): each side keeps (1+w)/2 of itself and takes
/// (1-w)/2 of its mirror. 1 leaves it as it is, 0 is mono, -1 swaps left
/// and right; the two sides' sum never changes.
fn width_blend((l, r): (f32, f32), w: f32) -> (f32, f32) {
    if w >= 1.0 {
        return (l, r);
    }
    let (a, b) = ((1.0 + w) / 2.0, (1.0 - w) / 2.0);
    (a * l + b * r, a * r + b * l)
}

/// The inverse of [`width_blend`], where the sides still tell it: `None`
/// near mono (|w| < 0.1), where both sides hold nearly the same.
fn width_unblend((l, r): (f32, f32), w: f32) -> Option<(f32, f32)> {
    if w >= 1.0 {
        return Some((l, r));
    }
    if w.abs() < 0.1 {
        return None;
    }
    // [l'; r'] = [[a, b], [b, a]] [l; r], and a² - b² = w.
    let (a, b) = ((1.0 + w) / 2.0, (1.0 - w) / 2.0);
    Some((((a * l - b * r) / w).max(0.0), ((a * r - b * l) / w).max(0.0)))
}

/// One channel of a stereo source into an output: its own cell and its
/// crossed cell, under the pair's width `w`, then Mono (both sides at
/// their mean). Their sum is always `volume`.
fn stereo_cells(volume: f32, w: f32, mono: bool) -> (f32, f32) {
    mono_sides(width_blend((volume, 0.0), w), mono)
}

/// The inverse of [`pan_split`].
fn pan_join(l: f32, r: f32) -> (f32, i8) {
    let volume = l.max(r);
    let pan = if volume > 0.0 {
        (((r - l) / volume) * 100.0).round().clamp(-100.0, 100.0) as i8
    } else {
        0
    };
    (volume, pan)
}

/// Playback channel `idx`'s level on its own side of its pair, at the
/// pair's balance `pan` (`channel::balance_side`).
fn playback_side(idx: usize, volume: f32, pan: i8) -> f32 {
    crate::channel::balance_side(idx, volume, pan)
}

/// A level control's value as the model's linear volume (1.0 = 0 dB,
/// 2.0 = +6 dB), read through the control's own dB scale (its TLV) rather
/// than as a share of its raw range.  The driver's raw ranges are not all
/// amplitude scales: the output masters count the hardware's 0.5 dB steps.
/// The bottom of the range is silence.
fn tlv_volume(selem: &Selem, ch: SelemChannelId) -> Option<f32> {
    let raw = selem.get_playback_volume(ch).ok()?;
    if raw <= selem.get_playback_volume_range().0 {
        return Some(0.0);
    }
    let db = selem.ask_playback_vol_db(raw).ok()?.to_db();
    Some(10f32.powf(db / 20.0))
}

/// The raw value that holds the linear `volume`, through the control's dB
/// scale, rounded down so a level never lands louder than asked.
fn tlv_raw(selem: &Selem, volume: f32) -> alsa::Result<i64> {
    // Not a number is silence: `NaN.min(2.0)` is 2.0, the top (job 8).
    if !volume.is_finite() || volume <= 0.0 {
        return Ok(selem.get_playback_volume_range().0);
    }
    let mb = (2000.0 * volume.min(2.0).log10()).floor() as i64;
    selem.ask_playback_db_vol(MilliBel(mb), Round::Floor)
}

/// Sets one channel of a level control to the linear `volume`.
fn set_tlv_volume(selem: &Selem, ch: SelemChannelId, volume: f32) -> alsa::Result<()> {
    selem.set_playback_volume(ch, tlv_raw(selem, volume)?)
}

/// Whether writing the model's `volume` would leave the raw value the card
/// holds - the re-read test for a dB-converted control.  A write rounds to
/// the control's grid, so the card's level can sit up to one step below the
/// model's, and a re-read must not move a fader the user is holding there.
fn tlv_matches(selem: &Selem, ch: SelemChannelId, volume: f32) -> bool {
    match (tlv_raw(selem, volume), selem.get_playback_volume(ch)) {
        (Ok(want), Ok(have)) => want == have,
        _ => false,
    }
}

/// A crosspoint cell's unity on TotalMix's scale (Windows session 3, issue
/// #13): a stereo source at 0 dB, a mono source panned hard to its side.
const XP_UNITY: f32 = 8192.0;
/// The cell's top since the 2026-11-09 driver, TotalMix's +6 dB (0x4000).
const XP_TOP: f32 = 16384.0;

/// The raw cell for a linear level on TotalMix's scale (1.0 = 0 dB), held
/// to the cell's top; written as is, not through the controls' dB labels
/// (their 0 dB is the driver's 0x16a0). Rounded to the nearest step:
/// TotalMix rounds down (a stereo source at -6 dB is 0x1009, 4105.7), one
/// step (0.0015 dB) apart, but rounding down makes a quiet panned mono
/// level read back one step off each time, and a re-read would churn.
fn xp_raw(volume: f32) -> i64 {
    if volume <= 0.0 {
        0
    } else {
        (volume * XP_UNITY).round().clamp(0.0, XP_TOP) as i64
    }
}

/// A raw cell as a linear level on TotalMix's scale.
fn xp_volume(raw: i64) -> f32 {
    raw.max(0) as f32 / XP_UNITY
}

fn encode_volume_pan(volume: f32, pan: i8, max: f32) -> (i64, i64) {
    let (l, r) = pan_split(volume.clamp(0.0, 1.0), pan);
    ((l * max) as i64, (r * max) as i64)
}

// ── Following the driver ───────────────────────────────────────
//
// The kernel driver changes controls on its own: the front-panel wheel
// moves an output master, a preamp gain or a monitoring crosspoint, SET
// toggles 48V, and `alsactl restore` or another mixer can write any of
// them.  `poll_events` re-reads the model from the card whenever ALSA
// reports a change, so the GUI shows what the hardware really holds.

/// How long after the last ALSA event to read the card once more.  The
/// driver notifies the front-panel controls on every wheel click, but not
/// every control a click moves (a gain or a crosspoint), and the notify
/// can come before the write it reports - one more read after the burst
/// catches what the events themselves did not carry.
const RESYNC_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// How often the backend re-reads the whole card, and how long after
/// `open()` the first read runs.
///
/// The driver does not notify host-side writes: `bf_xpoint_put()` and the
/// master put write the register, update their cache and return without
/// `snd_ctl_notify()`. A restore or a second mixer therefore generates no
/// ALSA event at all. The startup case is the one that bites: probe
/// applies the factory routing (hardware inputs off) and udev's
/// `alsactl restore` then loads the user's state *after* the card is
/// registered - possibly after `open()` has already read the defaults. The
/// model then shows the factory values until the user touches a control,
/// which is issue #23 (a monitor fader read as off while the unit still
/// passed signal). The first read is late enough to let that restore land;
/// the slow poll also keeps the GUI on the card if another client writes
/// later.
const RESYNC_POLL: std::time::Duration = std::time::Duration::from_millis(2000);
const FIRST_RESYNC: std::time::Duration = std::time::Duration::from_millis(1000);

/// The panel changes a preamp through a different register than the host,
/// and its ALSA notification has proven unreliable, so the four gains are
/// also re-read on this cadence. Four integer reads, cheap next to the
/// meters.
const GAIN_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// Largest difference between the model and the card that still counts as
/// "the same value": the model keeps the float the UI asked for, the card
/// the integer it was rounded to, so a re-read must not nudge a fader the
/// user is holding.  Below one step of the finest control (a crosspoint,
/// full scale 0x2d41).
const RESYNC_EPSILON: f32 = 1.0 / 8192.0;

/// Whether a value read back from the card differs from the model by more
/// than the rounding a write introduces.
fn resync_differs(model: f32, card: f32) -> bool {
    (model - card).abs() > RESYNC_EPSILON
}

/// The driver's "Front Panel In"/"Front Panel Out" enum item (0 = not
/// known yet, 1..=3 = the three positions) as the 0-based selection the
/// USB backend reports: IN 0 = Ch 1/2, 1 = Ch 3/4, 2 = Opt; OUT 0 = Ch
/// 1/2, 1 = Phones, 2 = Opt.
fn panel_enum_to_sel(item: u32) -> Option<usize> {
    match item {
        1..=3 => Some(item as usize - 1),
        _ => None,
    }
}

/// Whether the DIM presses counted between two reads of the driver's
/// "DIM Button Press Count" leave DIM toggled: an odd number of presses
/// does, an even one cancels out.  The count wraps at 2^31.
fn dim_presses_toggle(prev: i64, now: i64) -> bool {
    let delta = (now - prev).rem_euclid(1 << 31);
    delta % 2 == 1
}

// ── Main struct ────────────────────────────────────────────────

/// Babyface Pro (FS) device controller.
pub struct BabyfacePro {
    mixer: AlsaMixer,
    /// While on, a level or pan set on a crosspoint goes to the model only
    /// (`RmeDevice::hold_card`): the caller writes the card itself, composed.
    card_held: bool,
    /// Which control grammar this card speaks — decided once at
    /// [`RmeDevice::open`] and never re-checked, since a card cannot
    /// change personality without re-enumerating (the PC/CC toggle is
    /// a power-on button combo, see `PROTOCOL.md`).
    grammar: ControlGrammar,
    /// The kernel's enumeration number of the USB device this mixer was
    /// opened on (see `AlsaMixer::card_devnum`): a re-created card comes
    /// back on the same ALSA index with the same names, so this is what
    /// `is_connected` has to tell "still ours" apart with.
    card_devnum: Option<String>,
    profile: &'static DeviceProfile,
    inputs: Vec<InputChannel>,
    playbacks: Vec<PlaybackChannel>,
    outputs: Vec<OutputChannel>,
    settings: DeviceSettings,
    /// AN1/2 input-strip stereo link state (`"AN1/2 Link"`).
    /// Not exposed via [`DeviceSettings`] — matches the USB backend,
    /// which also keeps this outside `DeviceSettings` (no getter exists
    /// in [`RmeDevice`] for it, only `set_input_link`).
    linked: bool,
    /// Input metering through PipeWire's own source, so the app never
    /// holds the card's capture PCM: holding it removes the input node
    /// from the graph and pins the card to the meter's own period. See
    /// `pw_meter`'s module doc comment.
    input_tap: Option<crate::pw_meter::Tap>,
    /// The FX send's peaks (capture ch12/13) from the last `meters` drain,
    /// for `fx_meters`; `None` while the tap carries no FX pair.
    fx_in: std::cell::Cell<Option<[f32; 2]>>,
    /// The offset on the FX send the unit gets (`set_fx_send_trim`).
    fx_send_trim_db: f32,
    /// Direct ALSA capture, used only when PipeWire is not running.
    /// `None` if the capture PCM couldn't be opened; metering is a
    /// nice-to-have, not something that should keep the rest of the
    /// device from working.
    capture_meter: Option<crate::capture_meter::CaptureMeter>,
    /// The driver's "DIM Button Press Count" as last seen, `None` when the
    /// control does not exist (an older driver that applied DIM itself,
    /// or the class-compliant mode).  Since `babyface-pro-linux` 1597d28
    /// the driver only counts presses; acting on them is the mixer
    /// application's job, as it is TotalMix's on Windows.
    dim_press_count: Option<i64>,
    /// The application acts on DIM presses itself
    /// (`RmeDevice::set_app_handles_dim_button`): presses are reported
    /// through `dim_toggle_pending` instead of toggling the driver's Dim.
    app_handles_dim: bool,
    /// An unreported DIM toggle, for `RmeDevice::take_dim_button_toggle`.
    dim_toggle_pending: bool,
    /// Front-panel state from the driver's "Front Panel *" controls:
    /// (MIX engaged, IN selection, OUT selection), see
    /// [`BabyfacePro::panel_selection`].
    panel: Option<(bool, usize, usize)>,
    /// When to re-read the card once more after a burst of ALSA events,
    /// see [`RESYNC_SETTLE`].
    resync_at: Option<std::time::Instant>,
    /// Next periodic full re-read, see [`RESYNC_POLL`].
    resync_poll_at: Option<std::time::Instant>,
    /// Next due re-read of the preamp gains, see [`GAIN_POLL`].
    gain_poll_at: Option<std::time::Instant>,
    /// The output pair Mono folds to one channel (the Control Room's Main;
    /// `RmeDevice::set_mono_out`), applied on every crosspoint write into
    /// it so a fader moved while Mono is on stays mono.
    mono_out: Option<usize>,
    /// The driver has "Raw Crosspoint" (2026-11-09): output phase and M/S.
    raw_crosspoint: bool,
    /// The driver has "DSP Block" (2026-11-09): Audra computes every EQ
    /// (inputs and outputs) and sends it; the driver's own input EQ is
    /// kept off (its high shelf and its channels are wrong, issue #15).
    dsp_block: bool,
    /// The stream rate the EQ was last computed for (the DSP runs at it).
    eq_rate: std::cell::Cell<u32>,
}

impl BabyfacePro {
    /// The last samples of inputs `inputs` (one, or a pair averaged) and
    /// the rate, for Audra's EQ panel spectrum; an empty list stops the tap
    /// keeping them. `None` without the PipeWire tap.
    pub fn input_scope(&self, inputs: &[usize]) -> Option<(Vec<f32>, u32)> {
        let tap = self.input_tap.as_ref()?;
        let caps: Vec<usize> = inputs
            .iter()
            .filter_map(|&i| crate::capture_meter::INPUT_FOR_CAPTURE.iter().position(|m| *m == Some(i)))
            .collect();
        tap.scope_channels(&caps);
        if caps.is_empty() {
            return None;
        }
        Some(tap.scope_recent())
    }

    /// Writes the FX send the unit should hold, in dB.
    fn write_fx_send(&mut self, db: f32) -> Result<(), Error> {
        if let Some(selem) = self.mixer.find_selem("FX Send", 0) {
            // The send's own law, 0x1000 = 0 dB (Windows session 3); the
            // fader curve put the knob 3 dB too loud below its top.
            let max = selem.get_playback_volume_range().1 as u16;
            selem.set_playback_volume(SelemChannelId::mono(), send_db_to_raw(db).min(max) as i64)?;
        }
        Ok(())
    }

    /// The driver's "<pair> FX Send" control for `channel`, if it has one.
    fn source_send_selem(&self, channel: ChannelId) -> Option<Selem<'_>> {
        if self.grammar == ControlGrammar::ClassCompliant {
            return None;
        }
        let pair = crate::device::source_send_pair(channel)?;
        self.mixer.find_selem(&format!("{pair} FX Send"), 0)
    }

    /// Whether the capture stream behind [`RmeDevice::meters`] opened at
    /// all. Per-channel availability is still exposed separately (see
    /// `capture_meter`'s module doc comment); this only answers "could
    /// we meter anything", so a failed PCM open does not read as a bank
    /// of silent-but-live inputs.
    /// The inputs' RMS levels since the last call (the RMS meters), from
    /// the same tap as `meters`, mapped to inputs the same way. `None`
    /// where there is no tap.
    pub fn input_rms(&self) -> Option<Vec<f32>> {
        let rms: Vec<f32> = if let Some(tap) = &self.input_tap {
            tap.drain_rms().to_vec()
        } else {
            let cm = self.capture_meter.as_ref()?;
            if !cm.is_connected() {
                return None;
            }
            cm.drain_rms().to_vec()
        };
        let mut levels = vec![0.0; self.inputs.len()];
        for (cap, input) in rms.iter().zip(crate::capture_meter::INPUT_FOR_CAPTURE) {
            if let Some(i) = input {
                if let Some(slot) = levels.get_mut(i) {
                    *slot = *cap;
                }
            }
        }
        Some(levels)
    }

    /// The inputs' longest runs of full-scale samples since the last call
    /// (the meters' OVR), from the same tap as `meters`. `None` where
    /// there is no tap.
    pub fn input_overs(&self) -> Option<Vec<u32>> {
        let runs: Vec<u32> = if let Some(tap) = &self.input_tap {
            tap.drain_overs().to_vec()
        } else {
            let cm = self.capture_meter.as_ref()?;
            if !cm.is_connected() {
                return None;
            }
            cm.drain_overs().to_vec()
        };
        let mut out = vec![0; self.inputs.len()];
        for (cap, input) in runs.iter().zip(crate::capture_meter::INPUT_FOR_CAPTURE) {
            if let Some(i) = input {
                if let Some(slot) = out.get_mut(i) {
                    *slot = *cap;
                }
            }
        }
        Some(out)
    }

    pub fn meters_available(&self) -> bool {
        self.input_tap.is_some() || self.capture_meter.is_some()
    }

    /// How many times the input capture meter has reopened its PCM
    /// after a stream error (a PipeWire profile change, a re-probe).
    /// Diagnostic only; the meter recovers on its own.
    pub fn meter_reconnects(&self) -> usize {
        self.capture_meter.as_ref().map_or(0, |cm| cm.reconnects())
    }

    /// Re-read the four preamp gains. Kept separate from
    /// `attach_mixer_elements` because `poll_events` also calls it on its
    /// own cadence, not only when the mixer reports an event.
    fn refresh_preamp_gains(&mut self) {
        let mono = SelemChannelId::mono();
        for i in 0..4 {
            let read = self.preamp_gain_selem(i).and_then(|s| {
                let v = s.get_capture_volume(mono).ok()?;
                Some((v as u32, s.get_capture_volume_range().1 as u32))
            });
            let Some(inp) = self.inputs.get_mut(i) else {
                continue;
            };
            if let Some((v, max)) = read {
                if inp.gain != Some(v) {
                    log::debug!("gain[{i}]: {:?} -> {v}", inp.gain);
                }
                inp.gain = Some(v);
                inp.gain_max = Some(max);
            }
        }
    }

    /// Look up a crosspoint's ALSA element: `BF_SOURCES[src]` at
    /// `.index = output*14 + src`, exactly `mixer.c`'s
    /// `babyface_create_xpoints` layout.
    fn crosspoint_selem(&self, src: usize, output: usize) -> Option<Selem<'_>> {
        self.mixer
            .find_selem(BF_SOURCES[src], (output * 14 + src) as u32)
    }

    /// A stereo source's crossed cells into output pair `output` (the
    /// driver's "<src> Cross Playback Volume", index the pair): channel 0
    /// its left channel into the R side, channel 1 its right channel into
    /// the L side. None for AN1-4 (single channels) and on a driver that
    /// predates them.
    /// Source `src`'s phase switch ("<src> Phase", index src): one
    /// channel for AN1-4, two (ØL, ØR) for a stereo source.
    fn phase_selem(&self, src: usize) -> Option<Selem<'_>> {
        self.mixer.find_selem(&format!("{} Phase", BF_SOURCES[src]), src as u32)
    }

    /// Sets channel `ch` of source `src`'s phase switch; None when the
    /// driver has no such switch.
    fn write_phase(&self, src: usize, ch: SelemChannelId, invert: bool) -> Option<Result<(), Error>> {
        let selem = self.phase_selem(src)?;
        Some(selem.set_playback_switch(ch, invert as i32).map_err(Error::from))
    }

    fn cross_selem(&self, src: usize, output: usize) -> Option<Selem<'_>> {
        if src < 4 {
            return None;
        }
        self.mixer.find_selem(&format!("{} Cross", BF_SOURCES[src]), output as u32)
    }

    /// The width source `src` (a `BF_SOURCES` index) plays with (#7): its
    /// pair's, while the pair plays as a linked stereo pair; 1 otherwise.
    fn source_width(&self, src: usize) -> f32 {
        let channel = match src {
            0..=3 => ChannelId::Input(src),
            4..=7 => ChannelId::Input(4 + (src - 4) * 2),
            8..=13 => ChannelId::Playback((src - 8) * 2),
            _ => return 1.0,
        };
        if !crate::device::plays_as_stereo_pair(self, channel) {
            return 1.0;
        }
        self.settings.source_width_of(channel)
    }

    /// Channel `cid` rewritten into output pair `o` at its level as heard
    /// now: muted, or out while another strip is soloed, stays at 0.
    fn rewrite_channel(&self, cid: ChannelId, o: usize) -> Result<(), Error> {
        let any_solo = self.inputs.iter().any(|c| c.solo) || self.playbacks.iter().any(|c| c.solo);
        match cid {
            ChannelId::Input(j) => {
                let Some((src, ch, is_mono)) = input_crosspoint_slot(j) else {
                    return Ok(());
                };
                let c = &self.inputs[j];
                let level = crate::channel::heard_level(c.volumes.get(o).copied().unwrap_or(0.0), c.mute, c.solo, any_solo);
                let pan = c.pans.get(o).copied().unwrap_or(0);
                self.write_crosspoint(src, ch, is_mono, o, level, pan)
            }
            ChannelId::Playback(j) => {
                let Some((src, ch)) = playback_crosspoint_slot(j) else {
                    return Ok(());
                };
                let c = &self.playbacks[j];
                let level = crate::channel::heard_level(self.playback_level(j, o, c.volumes.get(o).copied().unwrap_or(0.0)), c.mute, c.solo, any_solo);
                self.write_crosspoint(src, ch, false, o, level, 0)
            }
            ChannelId::Output(_) => Ok(()),
        }
    }

    /// Source `src`'s cells into output pair `pair` as the driver holds
    /// them (its caches, never negative), as [`crate::device::SourceCells`];
    /// `src` 0..14 is `BF_SOURCES`, 14 the FX return. None where the
    /// driver has no such control.
    fn plain_cells(&self, src: usize, pair: usize) -> Option<crate::device::SourceCells> {
        let get = |s: &Selem, ch| s.get_playback_volume(ch).map(|v| v as i32).unwrap_or(0);
        let (fl, fr) = (SelemChannelId::FrontLeft, SelemChannelId::FrontRight);
        let (own, cross) = if src == 14 {
            (self.mixer.find_selem("FX Return", pair as u32)?, self.mixer.find_selem("FX Return Cross", pair as u32))
        } else {
            (self.crosspoint_selem(src, pair)?, self.cross_selem(src, pair))
        };
        if src < 4 {
            // A mono source: its one channel into each side.
            return Some([get(&own, fl), get(&own, fr), 0, 0]);
        }
        let (b, c) = cross.map_or((0, 0), |x| (get(&x, fl), get(&x, fr)));
        Some([get(&own, fl), b, c, get(&own, fr)])
    }

    /// The driver's phase on source `src`'s left and right channel, as the
    /// sign it writes with (`bf_phased`): AN1-4 have one switch.
    fn source_signs(&self, src: usize) -> (i32, i32) {
        let Some(selem) = (src < 14).then(|| self.phase_selem(src)).flatten() else {
            return (1, 1);
        };
        let on = |ch| selem.get_playback_switch(ch).is_ok_and(|v| v != 0);
        let l = if on(SelemChannelId::FrontLeft) { -1 } else { 1 };
        let r = if src < 4 { l } else if on(SelemChannelId::FrontRight) { -1 } else { 1 };
        (l, r)
    }

    /// Output processing (phase, M/S; issue #15) for source `src` into
    /// output pair `pair`, or for every source if `None`: each cell read
    /// from the driver's controls, signed as the driver signs it, then
    /// M/S and the side's phase, and written with "Raw Crosspoint".
    ///
    /// The driver rewrites a cell positive on every write it makes, so
    /// this runs after each of Audra's writes into a processed output, and
    /// after a re-read of the card. `force` writes even with nothing on,
    /// which puts the driver's own values back (switching it off). The
    /// unit ramps every change over ~10 ms, so the moment between the
    /// driver's write and this one is not heard (measured 2026-11-09:
    /// phase steady through both writes).
    fn reassert_output(&self, pair: usize, src: Option<usize>, force: bool) -> Result<(), Error> {
        if !self.raw_crosspoint || self.grammar != ControlGrammar::Proprietary {
            return Ok(());
        }
        let (pl, pr, ms) = (
            self.settings.output_phase_of(pair * 2),
            self.settings.output_phase_of(pair * 2 + 1),
            self.settings.output_ms_of(pair),
        );
        let out_proc = pl || pr || ms || force;
        // A source's M/S (issue #15) by its BF_SOURCES index (AN1/2 and
        // AN3/4 are one stereo pair each, as TotalMix's), only while the
        // pair plays as a linked stereo pair: two separate mics never.
        let src_ms = |s: usize| -> bool {
            let channel = match s {
                0..=3 => ChannelId::Input(s),
                4..=7 => ChannelId::Input(4 + (s - 4) * 2),
                8..=13 => ChannelId::Playback((s - 8) * 2),
                _ => return false,
            };
            crate::device::plays_as_stereo_pair(self, channel) && self.settings.source_ms_of(channel)
        };
        if !out_proc && !src.map_or_else(|| (0..15).any(src_ms), src_ms) {
            return Ok(());
        }
        let base = 0x34 * pair as i32;
        // The groups written together: a source on its own, AN1/2 or AN3/4.
        let groups: Vec<usize> = match src {
            Some(1) => vec![0],
            Some(3) => vec![2],
            Some(s) => vec![s],
            None => (0..15).filter(|&s| s != 1 && s != 3).collect(),
        };
        let write = |cell: i32, v: i32| self.mixer.write_ctl_integers("Raw Crosspoint", &[cell, v.clamp(-0x8000, 0x7fff)]);
        for s in groups {
            if !out_proc && !src_ms(s) {
                continue;
            }
            if s == 0 || s == 2 {
                // AN1/2 and AN3/4: two mono sources each, the odd one the
                // pair's left channel, the even one its right.
                let (Some(ml), Some(mr)) = (self.plain_cells(s, pair), self.plain_cells(s + 1, pair)) else {
                    continue;
                };
                let (sl, sr) = (self.source_signs(s).0, self.source_signs(s + 1).0);
                let mut cells = [ml[0] * sl, ml[1] * sl, mr[0] * sr, mr[1] * sr];
                if src_ms(s) {
                    cells = crate::device::source_ms_cells(cells);
                }
                let [a, b, c, d] = crate::device::output_proc_cells(cells, pl, pr, ms);
                let (l, r) = (s as i32, s as i32 + 1);
                write(base + l, a)?;
                write(base + 0x1a + l, b)?;
                write(base + r, c)?;
                write(base + 0x1a + r, d)?;
                continue;
            }
            let Some(m) = self.plain_cells(s, pair) else {
                continue;
            };
            let (sl, sr) = self.source_signs(s);
            let mut cells = [m[0] * sl, m[1] * sl, m[2] * sr, m[3] * sr];
            if src_ms(s) {
                cells = crate::device::source_ms_cells(cells);
            }
            let [a, b, c, d] = crate::device::output_proc_cells(cells, pl, pr, ms);
            let (il, ir) = match s {
                0..=3 => (s as i32, None),
                14 => (24, Some(25)),
                _ => (2 * s as i32 - 4, Some(2 * s as i32 - 3)),
            };
            write(base + il, a)?;
            write(base + 0x1a + il, b)?;
            if let Some(ir) = ir {
                write(base + ir, c)?;
                write(base + 0x1a + ir, d)?;
            }
        }
        Ok(())
    }

    /// The rate the unit runs at: the open stream's, else the last one seen.
    fn stream_rate(&self) -> u32 {
        let card = self.mixer.card_name().trim_start_matches("hw:").to_string();
        for dir in ["pcm0p", "pcm0c"] {
            let Ok(text) = std::fs::read_to_string(format!("/proc/asound/card{card}/{dir}/sub0/hw_params")) else {
                continue;
            };
            if let Some(rate) = text.lines().find_map(|l| l.strip_prefix("rate: ")).and_then(|r| r.split_whitespace().next()?.parse::<u32>().ok()) {
                self.eq_rate.set(rate);
                return rate;
            }
        }
        self.eq_rate.get()
    }

    /// Sends one channel's EQ as Audra holds it (inputs AN1-IN4, outputs
    /// Main/Phones/AS); nothing for a channel with none.
    fn upload_eq(&self, ch: crate::eq_block::EqChannel) -> Result<(), Error> {
        use crate::eq_block::EqChannel;
        if !self.dsp_block {
            return Ok(());
        }
        let eq = match ch {
            EqChannel::Input(i) => self.inputs.get(i).and_then(|c| c.eq),
            EqChannel::Output(o) => self.settings.output_eq_of(o),
        };
        let Some(eq) = eq else {
            return Ok(());
        };
        let block = crate::eq_block::eq_block(ch, &eq, self.stream_rate());
        self.mixer.write_ctl_bytes("DSP Block", &block)
    }

    /// Every EQ Audra holds, again: after a scene, a resume (the unit loses
    /// its DSP, and the driver re-sends only its own, flat, input blocks)
    /// or a rate change.
    fn resend_eq(&self) {
        use crate::eq_block::EqChannel;
        for i in 0..4 {
            let _ = self.upload_eq(EqChannel::Input(i));
        }
        for o in 0..self.settings.output_eq.len() {
            let _ = self.upload_eq(EqChannel::Output(o));
        }
    }

    /// Keeps the FX return out of the FX send. The unit powers up with the
    /// return's own cells in the send block (0x0150/0x0151 into the send's
    /// left, 0x016A/0x016B into its right) near unity, and the driver never
    /// writes them: Reverb and Echo then fed back into their own input
    /// (measured 2026-11-09: a -30 dBFS tone into the return came back on
    /// the send at -33 dBFS; zeroed, at the noise floor). TotalMix zeroes
    /// the whole block at start; this zeroes the four that loop.
    fn clear_fx_feedback(&self) {
        if !self.raw_crosspoint {
            return;
        }
        for cell in [0x0150, 0x0151, 0x016A, 0x016B] {
            let _ = self.mixer.write_ctl_integers("Raw Crosspoint", &[cell, 0]);
        }
    }

    /// [`Self::reassert_output`] on every output pair with processing on.
    fn reassert_outputs(&self, src: Option<usize>) {
        for pair in 0..self.profile.output_pair_count() {
            let _ = self.reassert_output(pair, src, false);
        }
    }

    /// Writes `volume` (0.0..=1.0) straight to a crosspoint's ALSA
    /// element, preserving `pan` for the 4 true-mono sources — the
    /// same encoding `set_volume`/`set_pan` use, but without touching
    /// the stored `self.inputs`/`self.playbacks` volume model.
    /// `set_mute`/`set_solo` use this to force a crosspoint to 0 (or
    /// back) while keeping the channel's own "real" volume intact to
    /// restore later, exactly like `usb.rs`'s already
    /// hardware-validated mute/solo. A missing element (e.g. an
    /// unsupported output count) is a silent no-op, matching
    /// `set_volume`'s own `if let Some(selem) = ...` behavior.
    fn write_crosspoint(
        &self,
        src: usize,
        ch: SelemChannelId,
        is_mono: bool,
        output: usize,
        volume: f32,
        pan: i8,
    ) -> Result<(), Error> {
        // Held: the model alone takes it (`hold_card`); the caller's raw
        // write follows, and it never runs while held.
        if self.card_held {
            return Ok(());
        }
        if self.grammar == ControlGrammar::ClassCompliant {
            // Same arithmetic, different addressing: each destination
            // channel is its own mono control, so a mono source's pan
            // is two writes rather than two channels of one selem.
            let right = ch == SelemChannelId::FrontRight;
            let Some(src_name) = cc_source_name(src, right) else {
                return Ok(());
            };
            let mono = SelemChannelId::mono();
            if is_mono {
                let (Some(dl), Some(dr)) =
                    (cc_dest_name(output, false), cc_dest_name(output, true))
                else {
                    return Ok(());
                };
                let (Some(sl), Some(sr)) = (
                    self.mixer.find_selem(&format!("{src_name}-{dl}"), 0),
                    self.mixer.find_selem(&format!("{src_name}-{dr}"), 0),
                ) else {
                    return Ok(());
                };
                let max = sl.get_playback_volume_range().1 as f32;
                let (l, r) = encode_volume_pan(volume.clamp(0.0, 1.0), pan, max);
                sl.set_playback_volume(mono, l)?;
                sr.set_playback_volume(mono, r)?;
            } else {
                // A paired source feeds only its own side of the pair,
                // exactly as the proprietary path writes only `ch`.
                let Some(dest) = cc_dest_name(output, right) else {
                    return Ok(());
                };
                let Some(selem) = self.mixer.find_selem(&format!("{src_name}-{dest}"), 0) else {
                    return Ok(());
                };
                let max = selem.get_playback_volume_range().1 as f32;
                selem.set_playback_volume(mono, (volume.clamp(0.0, 1.0) * max) as i64)?;
            }
            return Ok(());
        }

        // The driver's crosspoints are in dB (TLV), with +6 dB at the top:
        // convert through that, as for the output masters.
        let Some(selem) = self.crosspoint_selem(src, output) else {
            return Ok(());
        };
        let volume = volume.clamp(0.0, 2.0);
        let mono = self.mono_out == Some(output);
        // TotalMix's scale (issue #13): raw cells, 0x2000 a stereo source's
        // 0 dB, a mono source panned by constant power.
        // The pair's width (#7) first, then Mono, as TotalMix stacks them.
        let w = self.source_width(src);
        if is_mono {
            let (l, r) = mono_sides(width_blend(pan_split_power(volume, pan), w), mono);
            selem.set_playback_volume(SelemChannelId::FrontLeft, xp_raw(l))?;
            selem.set_playback_volume(SelemChannelId::FrontRight, xp_raw(r))?;
        } else {
            let (own, cross) = stereo_cells(volume, w, mono);
            selem.set_playback_volume(ch, xp_raw(own))?;
            // Its crossed cell, off unless a width or Mono folds this
            // output (the driver keeps them; an older one has none).
            if let Some(cross_selem) = self.cross_selem(src, output) {
                cross_selem.set_playback_volume(ch, xp_raw(cross))?;
            }
        }
        // The output's phase and M/S over what the driver just wrote.
        self.reassert_output(output, Some(src), false)
    }

    /// Which control grammar this card speaks. Lets a UI gate the
    /// DSP-only features (EQ, phase, trim, split, loopback, the front
    /// panel) instead of offering them and letting
    /// [`Self::require_proprietary`] reject the click.
    pub fn grammar(&self) -> ControlGrammar {
        self.grammar
    }

    /// Refuses a feature that exists only in RME's own protocol.
    ///
    /// The class-compliant surface is routing + preamps + clock and
    /// nothing else — no EQ, no phase/trim/split, no loopback, no
    /// front panel. Those setters all look them up with a tolerant
    /// `if let Some(selem)`, which would return `Ok(())` having done
    /// nothing: the exact "silently lying to the user" failure this
    /// backend spent 2026-09-12 removing. Fail loudly instead.
    fn require_proprietary(&self, feature: &str) -> Result<(), Error> {
        if self.grammar == ControlGrammar::ClassCompliant {
            return Err(Error::InvalidChannel(format!(
                "{feature} needs the snd-usb-babyface-pro driver — this card is \
                 in Class Compliant mode, which does not expose it"
            )));
        }
        Ok(())
    }

    /// The preamp base name for input `idx` in the class-compliant
    /// grammar: `Mic-AN1`/`Mic-AN2` for the two mics, `Line-IN3`/
    /// `Line-IN4` for the two instrument inputs.
    fn cc_preamp_base(idx: usize) -> Option<&'static str> {
        Some(match idx {
            0 => "Mic-AN1",
            1 => "Mic-AN2",
            2 => "Line-IN3",
            3 => "Line-IN4",
            _ => return None,
        })
    }

    /// The preamp gain element for input `idx`, in either grammar.
    ///
    /// The two differ in more than spelling: the proprietary driver
    /// exposes one `Mic 1` element per `.index`, as a *capture* volume;
    /// the class-compliant surface has a distinctly-named element per
    /// input carrying both playback and capture volume, so the callers
    /// also have to pick the right accessor.
    fn preamp_gain_selem(&self, idx: usize) -> Option<Selem<'_>> {
        if self.grammar == ControlGrammar::ClassCompliant {
            self.mixer
                .find_selem(&format!("{} Gain", Self::cc_preamp_base(idx)?), 0)
        } else {
            self.mixer.find_selem("Mic 1", idx as u32)
        }
    }

    /// The 48V element for mic `idx` (0/1), in either grammar.
    fn phantom_selem(&self, idx: usize) -> Option<Selem<'_>> {
        if self.grammar == ControlGrammar::ClassCompliant {
            self.mixer
                .find_selem(&format!("{} 48V", Self::cc_preamp_base(idx)?), 0)
        } else {
            self.mixer.find_selem("Phantom Power Mic 1", idx as u32)
        }
    }

    /// The PAD element for mic `idx` (0/1), in either grammar.
    fn pad_selem(&self, idx: usize) -> Option<Selem<'_>> {
        if self.grammar == ControlGrammar::ClassCompliant {
            self.mixer
                .find_selem(&format!("{} PAD", Self::cc_preamp_base(idx)?), 0)
        } else {
            self.mixer.find_selem("Pad Mic 1", idx as u32)
        }
    }

    /// Whether the model's crosspoint value is what the card holds - the
    /// re-read test.  The driver's crosspoints are converted through dB, so
    /// compare raw values: a write rounds to the control's grid, and a
    /// re-read must not move a fader the user is holding.  The
    /// class-compliant card keeps the share-of-range comparison.
    #[allow(clippy::too_many_arguments)]
    fn crosspoint_matches(
        &self,
        src: usize,
        ch: SelemChannelId,
        is_mono: bool,
        output: usize,
        model_volume: f32,
        model_pan: i8,
        card_volume: f32,
    ) -> bool {
        if self.grammar == ControlGrammar::ClassCompliant {
            return !resync_differs(model_volume, card_volume);
        }
        let Some(selem) = self.crosspoint_selem(src, output) else {
            return false;
        };
        let volume = model_volume.clamp(0.0, 2.0);
        // Mono writes the model's level folded; the card matches that.
        let mono = self.mono_out == Some(output);
        let holds = |c: SelemChannelId, v: f32| selem.get_playback_volume(c).is_ok_and(|raw| raw == xp_raw(v));
        let w = self.source_width(src);
        if is_mono {
            let (l, r) = mono_sides(width_blend(pan_split_power(volume, model_pan), w), mono);
            holds(SelemChannelId::FrontLeft, l) && holds(SelemChannelId::FrontRight, r)
        } else {
            holds(ch, stereo_cells(volume, w, mono).0)
        }
    }

    /// Reads a crosspoint back as `(volume, pan)`, in whichever
    /// grammar this card speaks. `pan` is meaningful only for the four
    /// true-mono sources; paired sources report 0.
    ///
    /// The mirror of [`Self::write_crosspoint`] — kept next to it so
    /// the two addressing models stay visibly in step.
    fn read_crosspoint(
        &self,
        src: usize,
        ch: SelemChannelId,
        is_mono: bool,
        output: usize,
    ) -> Option<(f32, i8)> {
        if self.grammar == ControlGrammar::ClassCompliant {
            let right = ch == SelemChannelId::FrontRight;
            let src_name = cc_source_name(src, right)?;
            let mono = SelemChannelId::mono();
            if is_mono {
                let sl = self
                    .mixer
                    .find_selem(&format!("{src_name}-{}", cc_dest_name(output, false)?), 0)?;
                let sr = self
                    .mixer
                    .find_selem(&format!("{src_name}-{}", cc_dest_name(output, true)?), 0)?;
                let max = sl.get_playback_volume_range().1 as f32;
                let l = sl.get_playback_volume(mono).ok()?;
                let r = sr.get_playback_volume(mono).ok()?;
                Some(decode_volume_pan(l, r, max))
            } else {
                let selem = self
                    .mixer
                    .find_selem(&format!("{src_name}-{}", cc_dest_name(output, right)?), 0)?;
                let max = selem.get_playback_volume_range().1 as f32;
                Some((selem.get_playback_volume(mono).ok()? as f32 / max, 0))
            }
        } else {
            let selem = self.crosspoint_selem(src, output)?;
            let read = |c: SelemChannelId| selem.get_playback_volume(c).ok().map(xp_volume);
            if is_mono {
                // A width (#7) undone first; near mono the sides no longer
                // tell the pan, and they are read as they are.
                let sides = (read(SelemChannelId::FrontLeft)?, read(SelemChannelId::FrontRight)?);
                let (l, r) = width_unblend(sides, self.source_width(src)).unwrap_or(sides);
                Some(pan_join_power(l, r))
            } else {
                // Own plus crossed: the fader, whatever the width or Mono.
                let cross = self
                    .cross_selem(src, output)
                    .and_then(|e| e.get_playback_volume(ch).ok())
                    .map_or(0.0, xp_volume);
                Some((read(ch)? + cross, 0))
            }
        }
    }

    /// What playback channel `idx` sends the card into `output` at fader
    /// `volume`: its own side under the pair's balance; a split channel
    /// has no balance, whatever pan it was left with.
    /// The pair's balance is read from its left channel, the one its strip
    /// shows (a scene saved on the USB backend holds it there only).
    fn playback_level(&self, idx: usize, output: usize, volume: f32) -> f32 {
        if self.playbacks[idx].split {
            return volume;
        }
        let pan = self.playbacks[idx & !1].pans.get(output).copied().unwrap_or(0);
        playback_side(idx, volume, pan)
    }

    /// Whether playback channel `c` is silent on the card right now: muted,
    /// or left out while another strip is soloed (`set_mute`, `set_solo`
    /// zero its crosspoints and restore them from the model).
    fn playback_silenced(&self, c: usize) -> bool {
        let any_solo = self.inputs.iter().any(|x| x.solo) || self.playbacks.iter().any(|x| x.solo);
        self.playbacks[c].mute || (any_solo && !self.playbacks[c].solo)
    }

    /// The balance of the playback pair holding channel `idx`, into submix
    /// `output` (the owner, 2026-11-06: turning a playback strip's knob
    /// was refused). The pair's left channel reaches only the output's
    /// left and its right only the right, so a balance is the far side
    /// lowered to (1 − |balance|) of the fader and the near side left at
    /// it: the law `usb.rs` writes, measured on the unit there
    /// (`protocol::set_crosspoint_balance`). Both channels keep the pan,
    /// so either one reads it back. A split channel has no other side to
    /// lean to: centre is accepted, anything else refused.
    fn set_playback_balance(&mut self, idx: usize, output: usize, pan: i8) -> Result<(), Error> {
        let pair = idx / 2;
        if pair * 2 + 1 >= self.playbacks.len() {
            return Err(Error::InvalidChannel(format!("Playback {idx}")));
        }
        if output >= self.profile.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output {output}")));
        }
        if self.playbacks[idx].split {
            if pan == 0 {
                self.playbacks[idx].pans[output] = 0;
                return Ok(());
            }
            return Err(Error::InvalidChannel(
                "A split playback channel reaches only its own side, so it has no pan".into(),
            ));
        }
        for c in [pair * 2, pair * 2 + 1] {
            self.playbacks[c].pans[output] = pan;
        }
        for c in [pair * 2, pair * 2 + 1] {
            if self.playback_silenced(c) {
                continue;
            }
            let (src, ch) = playback_crosspoint_slot(c)
                .ok_or_else(|| Error::InvalidChannel(format!("Playback {c}")))?;
            let level = self.playback_level(c, output, self.playbacks[c].volumes[output]);
            self.write_crosspoint(src, ch, false, output, level, 0)?;
        }
        Ok(())
    }

    /// Match ALSA mixer elements to our channel model.
    ///
    /// Every lookup here is a deliberate `if let Some(selem)` no-op when
    /// the control is absent, because the driver grew its control set
    /// over time and an older module should still give a working mixer
    /// for whatever it *does* expose. (The 2026-08-28 version of this
    /// comment listed Sample Clock Source and per-input Sensitivity as
    /// permanently missing; both were added to `snd-usb-babyface-pro`
    /// on 2026-09-06 — as "Sample Clock Source" and "Instrument Ref
    /// Level" — and are matched below. The proprietary driver also
    /// exposes "Optical Out Format" (ADAT / S/PDIF) since 2026-10-18;
    /// the IEC958 controls remain Class Compliant only.)
    ///
    /// That tolerance is why [`RmeDevice::open`] has to check for a few
    /// sentinel controls *before* getting here: absent the guard, a card
    /// with an entirely foreign control grammar (a CC-mode Babyface on
    /// `snd-usb-audio`) matches nothing at all and still looks healthy.
    ///
    /// `resync` is set when re-reading a card already in use (see
    /// `poll_events`).  Mute and solo on the input/playback strips exist
    /// only here - they zero crosspoints on the card while the model keeps
    /// the real levels to restore - so a re-read leaves the crosspoints of
    /// a muted strip, and every strip's while one is soloed, alone.  So
    /// does an output's master in the class-compliant grammar while it is
    /// muted, which is model-level there too.
    #[allow(clippy::needless_range_loop)]
    fn attach_mixer_elements(&mut self, resync: bool) {
        let mono = SelemChannelId::mono();

        for (name, selem) in self.mixer.iter_selems() {
            if name == "Sample Clock Source" {
                if let Ok(current) = selem.get_enum_item(mono) {
                    if let Ok(item_name) = selem.get_enum_item_name(current) {
                        self.settings.clock_source = item_name;
                    }
                }
                if let Ok(count) = selem.get_enum_items() {
                    self.settings.clock_sources = (0..count)
                        .filter_map(|i| selem.get_enum_item_name(i).ok())
                        .collect();
                }
                continue;
            }
            if name == "Optical Out Format" {
                if let Ok(item) = selem.get_enum_item(mono) {
                    if let Ok(item_name) = selem.get_enum_item_name(item) {
                        self.settings.optical_out_spdif = item_name == "S/PDIF";
                    }
                }
                continue;
            }
            if name == "Instrument Ref Level" {
                // A single shared switch (see `set_sensitivity`'s own
                // doc comment) — read once, mirrored to every
                // Instrument channel below. The real control has 3
                // states (+4dBu/-10dBV/Boost) but this trait's
                // `Sensitivity` enum only has 2 — Boost (item 2) maps
                // to the closer of the two rather than being lost
                // entirely: it shares -10dBV's state bits, only the
                // one-shot commit differs (see `babyface-pro-linux`'s
                // own "Instrument Ref Level" control).
                if let Ok(item) = selem.get_enum_item(mono) {
                    let sens = match item {
                        0 => Some(Sensitivity::Plus4dBu),
                        _ => Some(Sensitivity::Minus10dBV), // 1 = -10dBV, 2 = Boost
                    };
                    // The full three-state reading, as the USB backend's
                    // `REF_*` codes (1 +4dBu, 2 -10dBV, 3 Boost; written out, as
                    // `usb` is only built with the "usb" feature).
                    let raw = match item {
                        0 => 1,
                        1 => 2,
                        _ => 3,
                    };
                    for c in self.inputs.iter_mut() {
                        if c.channel_type == ChannelType::Instrument {
                            c.ref_level = raw;
                            c.sensitivity = sens;
                        }
                    }
                }
                continue;
            }
            if name == "IEC958 Emphasis" {
                if let Ok(v) = selem.get_playback_switch(mono) {
                    self.settings.spdif_emphasis = v != 0;
                }
                continue;
            }
            if name == "IEC958 Pro Mask" {
                if let Ok(v) = selem.get_playback_switch(mono) {
                    self.settings.spdif_professional = v != 0;
                }
                continue;
            }
            if name == "IEC958" {
                if let Ok(v) = selem.get_playback_switch(mono) {
                    self.settings.spdif_enabled = v != 0;
                }
                continue;
            }
        }

        // ── Phantom 48V & PAD — the two mics. Read both first, then
        // apply: the grammar-aware lookups borrow `self`, so they can't
        // run while `self.inputs` is mutably borrowed.
        for i in 0..2 {
            let phantom = self
                .phantom_selem(i)
                .and_then(|s| s.get_playback_switch(mono).ok())
                .map(|v| v != 0);
            let pad = self
                .pad_selem(i)
                .and_then(|s| s.get_playback_switch(mono).ok())
                .map(|v| v != 0);
            let Some(inp) = self.inputs.get_mut(i) else {
                continue;
            };
            if let Some(v) = phantom {
                inp.phantom = v;
            }
            if let Some(v) = pad {
                inp.pad = v;
            }
        }

        // ── Sensitivity, class-compliant grammar: a separate
        // `<base> Sens.` enum per instrument input (the proprietary
        // "Instrument Ref Level" is read in the loop above instead, and
        // is a single shared switch). Resolved by item NAME, because the
        // two controls enumerate their items in opposite orders —
        // ["-10dBV","+4dBu"] here vs ["+4dBu","-10dBV","Boost"] there.
        // Without this, sensitivity silently reported the model default
        // rather than the card: observed 2026-09-12 showing "+4dBu"
        // while the card was really on -10dBV.
        if self.grammar == ControlGrammar::ClassCompliant {
            for i in 0..4.min(self.inputs.len()) {
                let read = Self::cc_preamp_base(i)
                    .and_then(|base| self.mixer.find_selem(&format!("{base} Sens."), 0))
                    .and_then(|selem| {
                        let item = selem.get_enum_item(mono).ok()?;
                        match selem.get_enum_item_name(item).ok()?.as_str() {
                            "+4dBu" => Some(Sensitivity::Plus4dBu),
                            "-10dBV" => Some(Sensitivity::Minus10dBV),
                            _ => None,
                        }
                    });
                if let Some(sens) = read {
                    self.inputs[i].sensitivity = Some(sens);
                }
            }
        }

        // ── Phase Ø invert: the driver's "<source> Phase" switches, one
        // channel for AN1-4, ØL/ØR for a stereo source (2026-11-07 driver).
        for i in 0..self.inputs.len() {
            if let Some((src, ch, is_mono)) = input_crosspoint_slot(i) {
                let ch = if is_mono { mono } else { ch };
                if let Some(v) = self.phase_selem(src).and_then(|e| e.get_playback_switch(ch).ok()) {
                    self.inputs[i].phase = v != 0;
                }
            }
        }
        for i in 0..self.playbacks.len() {
            if let Some((src, ch)) = playback_crosspoint_slot(i) {
                if let Some(v) = self.phase_selem(src).and_then(|e| e.get_playback_switch(ch).ok()) {
                    self.playbacks[i].phase = v != 0;
                }
            }
        }

        // No "<name> Trim Volume" read: that driver control wrote the trim
        // as the pair's Main cells, not a level before the mixer, and Audra
        // no longer sets it (2026-11-09).

        // ── Stereo split — playback pairs PB1-PB6, one control per
        // pair (`babyface-pro-linux`'s own "PBx Stereo Split" Switch,
        // added 2026-09-06); mirrored to both channels of the pair.
        for pair in 0..(self.playbacks.len() / 2) {
            if let Some(selem) = self
                .mixer
                .find_selem(&format!("PB{} Stereo Split", pair + 1), pair as u32)
            {
                if let Ok(v) = selem.get_playback_switch(mono) {
                    let split = v != 0;
                    if let Some(c) = self.playbacks.get_mut(pair * 2) {
                        c.split = split;
                    }
                    if let Some(c) = self.playbacks.get_mut(pair * 2 + 1) {
                        c.split = split;
                    }
                }
            }
        }

        // ── Preamp gain — inputs[0..4] (Mic1, Mic2, Instr3, Instr4) map
        // 1:1 to ALSA index 0..4 of "Mic 1" (a *capture* volume, not
        // playback — `mixer.c:1313-1326`). Re-read on its own cadence too,
        // see `GAIN_POLL`.
        self.refresh_preamp_gains();

        // ── Crosspoint volume (+ pan for the 4 true-mono sources) ──
        let any_solo = self.inputs.iter().any(|c| c.solo) || self.playbacks.iter().any(|c| c.solo);
        for i in 0..self.inputs.len() {
            let Some((src, ch, is_mono)) = input_crosspoint_slot(i) else {
                continue;
            };
            if resync && (self.inputs[i].mute || any_solo) {
                continue;
            }
            for out in 0..self.profile.output_pair_count() {
                if let Some((volume, pan)) = self.read_crosspoint(src, ch, is_mono, out) {
                    if resync
                        && self.crosspoint_matches(
                            src,
                            ch,
                            is_mono,
                            out,
                            self.inputs[i].volumes[out],
                            self.inputs[i].pans[out],
                            volume,
                        )
                    {
                        continue;
                    }
                    self.inputs[i].volumes[out] = volume;
                    // A silent crosspoint carries no pan: keep the
                    // model's, so a fader pulled down and back up does
                    // not come back centred.
                    if is_mono && (!resync || volume > 0.0) {
                        self.inputs[i].pans[out] = pan;
                    }
                }
            }
        }

        // A linked playback pair's two sides read back as its fader and its
        // balance (`set_playback_balance`): the louder side is the fader,
        // the quieter one how far the balance leans away from it. Read
        // per channel as the card holds it, then joined per pair.
        let n_out = self.profile.output_pair_count();
        let mut card: Vec<Vec<Option<f32>>> = vec![vec![None; n_out]; self.playbacks.len()];
        for i in 0..self.playbacks.len() {
            let Some((src, ch)) = playback_crosspoint_slot(i) else {
                continue;
            };
            if resync && (self.playbacks[i].mute || any_solo) {
                continue;
            }
            for out in 0..n_out {
                if let Some((volume, _)) = self.read_crosspoint(src, ch, false, out) {
                    let side = self.playback_level(i, out, self.playbacks[i].volumes[out]);
                    if resync && self.crosspoint_matches(src, ch, false, out, side, 0, volume) {
                        continue;
                    }
                    card[i][out] = Some(volume);
                }
            }
        }
        for pair in 0..self.playbacks.len() / 2 {
            let (lc, rc) = (pair * 2, pair * 2 + 1);
            for out in 0..n_out {
                if card[lc][out].is_none() && card[rc][out].is_none() {
                    continue;
                }
                if self.playbacks[lc].split {
                    for c in [lc, rc] {
                        if let Some(volume) = card[c][out] {
                            self.playbacks[c].volumes[out] = volume;
                            self.playbacks[c].pans[out] = 0;
                        }
                    }
                    continue;
                }
                // A side not just read is unchanged on a re-read: as the
                // model last wrote it.
                let side = |c: usize| card[c][out].unwrap_or_else(|| self.playback_level(c, out, self.playbacks[c].volumes[out]));
                let (fader, balance) = pan_join(side(lc), side(rc));
                for c in [lc, rc] {
                    self.playbacks[c].volumes[out] = fader;
                    // A silent pair carries no balance: keep the model's,
                    // as the mono inputs above do.
                    if !resync || fader > 0.0 {
                        self.playbacks[c].pans[out] = balance;
                    }
                }
            }
        }

        // ── Output pair master volume + mute — one 2-channel ALSA
        // element per pair (`PAIR_LABELS[pair_idx]` @ index `pair_idx`),
        // front-left/front-right are the pair's two physical channels;
        // the mute switch is shared by both (`mixer.c`'s `bf_mute_get`
        // mirrors the same value into both ALSA channels), so both
        // `self.outputs` entries for the pair get the same mute state.
        // In the class-compliant grammar each physical output is its
        // own `Main-Out <ch>` control and there is NO mute switch
        // anywhere on the card (the dump has exactly seven switches:
        // 2x48V, 2xPAD, 3xIEC958), so mute stays model-level there —
        // the same conclusion the 2026-08-13 CC probe reached.
        for pair_idx in 0..PAIR_LABELS.len() {
            for (side, right) in [(0usize, false), (1usize, true)] {
                let (volume, muted) = if self.grammar == ControlGrammar::ClassCompliant {
                    let Some(dest) = cc_dest_name(pair_idx, right) else {
                        continue;
                    };
                    let Some(selem) = self.mixer.find_selem(&format!("Main-Out {dest}"), 0) else {
                        continue;
                    };
                    let max = selem.get_playback_volume_range().1 as f32;
                    let Ok(v) = selem.get_playback_volume(mono) else {
                        continue;
                    };
                    (v as f32 / max, None)
                } else {
                    let Some(selem) = self
                        .mixer
                        .find_selem(PAIR_LABELS[pair_idx], pair_idx as u32)
                    else {
                        continue;
                    };
                    let ch = if right {
                        SelemChannelId::FrontRight
                    } else {
                        SelemChannelId::FrontLeft
                    };
                    let Some(v) = tlv_volume(&selem, ch) else {
                        continue;
                    };
                    // When the model's value is what the card holds after
                    // rounding, keep the model's: the check below then
                    // leaves the fader alone, and the mute still syncs.
                    let model = self
                        .outputs
                        .get(pair_idx * 2 + side)
                        .map_or(0.0, |o| o.volume);
                    let v = if resync && tlv_matches(&selem, ch, model) {
                        model
                    } else {
                        v
                    };
                    // The mute switch is shared by both channels
                    // (`mixer.c`'s `bf_mute_get` mirrors one value).
                    let muted = selem
                        .get_playback_switch(SelemChannelId::FrontLeft)
                        .map(|v| v == 0)
                        .unwrap_or(false);
                    (v, Some(muted))
                };
                if let Some(out) = self.outputs.get_mut(pair_idx * 2 + side) {
                    // Class-compliant mute is the master driven to 0.
                    let model_level_mute = muted.is_none() && out.mute;
                    if !(resync && (model_level_mute || !resync_differs(out.volume, volume))) {
                        out.volume = volume;
                    }
                    if let Some(m) = muted {
                        out.mute = m;
                    }
                }
            }
        }

        // ── Loopback — one bool per output pair.
        for pair_idx in 0..PAIR_LABELS.len() {
            if let Some(selem) = self.mixer.find_selem("Loopback", pair_idx as u32) {
                if let Ok(v) = selem.get_playback_switch(mono) {
                    let on = v != 0;
                    if let Some(o) = self.outputs.get_mut(pair_idx * 2) {
                        o.loopback = on;
                    }
                    if let Some(o) = self.outputs.get_mut(pair_idx * 2 + 1) {
                        o.loopback = on;
                    }
                }
            }
        }

        // ── Global switches / values (AN 1>2, AN1/2 Link, MS Processor,
        // Dim, Width, FX Send, Varispeed Pitch) — all real ALSA controls
        // (mixer.c:1016-1114), previously never wired up on this backend.
        if let Some(selem) = self.mixer.find_selem("AN 1>2", 0) {
            if let Ok(v) = selem.get_playback_switch(mono) {
                self.settings.an12 = v != 0;
            }
        }
        if let Some(selem) = self.mixer.find_selem("AN1/2 Link", 0) {
            if let Ok(v) = selem.get_playback_switch(mono) {
                self.linked = v != 0;
            }
        }
        if let Some(selem) = self.mixer.find_selem("Dim", 0) {
            if let Ok(v) = selem.get_playback_switch(mono) {
                self.settings.dim = v != 0;
            }
        }
        if let Some(selem) = self.mixer.find_selem("FX Send", 0) {
            if let Ok(v) = selem.get_playback_volume(mono) {
                let read = send_raw_to_db(v.clamp(0, 0x1000) as u16);
                // The unit holds the knob plus the follow offset: what we
                // wrote reads back as the knob; anything else (another mixer,
                // `alsactl restore`) is a new knob level under the offset.
                let wrote = self.settings.fx_send_db.map(|k| crate::device::fx_send_on_unit(k, self.fx_send_trim_db));
                if wrote.is_none_or(|w| (w - read).abs() > 0.6) {
                    self.settings.fx_send_db = Some(if read <= -65.0 {
                        -65.0
                    } else {
                        (read - self.fx_send_trim_db.max(-65.0)).clamp(-65.0, 0.0)
                    });
                }
            }
        }
        // The other sources' sends (issue #14), where the driver has them.
        for ch in [2usize, 4, 6, 8, 10].map(ChannelId::Input).into_iter().chain((0..12).step_by(2).map(ChannelId::Playback)) {
            let read = self.source_send_selem(ch).and_then(|s| s.get_playback_volume(mono).ok());
            if let (Some(v), Some(pair)) = (read, crate::device::source_send_pair(ch)) {
                self.settings.source_fx_send.insert(pair.to_string(), send_raw_to_db(v.clamp(0, 0x1000) as u16));
            }
        }
        // The FX return per output pair, where the driver opens it (the
        // left side stands for the pair: Audra writes both alike).
        for pair in 0..self.profile.output_pair_count() {
            let Some(selem) = self.mixer.find_selem("FX Return", pair as u32) else {
                break;
            };
            if let Ok(raw) = selem.get_playback_volume(SelemChannelId::FrontLeft) {
                self.settings.keep_fx_return(pair, xp_volume(raw));
            }
        }
        if let Some(selem) = self.mixer.find_selem("Varispeed Pitch", 0) {
            if let Ok(v) = selem.get_playback_volume(mono) {
                self.settings.pitch_percent = v as f32 / 10.0;
            }
        }

        // ── Hardware DSP EQ — analog inputs only (`eq_strip_name`). Read
        // once at open; with "DSP Block" the EQ is Audra's after that (the
        // driver's own strips are kept off), so a re-read leaves it.
        for i in 0..4 {
            if resync && self.dsp_block {
                break;
            }
            let Some(strip) = eq_strip_name(i) else {
                continue;
            };
            let mut eq = InputEq::default();
            let mut found_any = false;
            if let Some(selem) = self.mixer.find_selem(&format!("{strip} EQ Enable"), 0) {
                if let Ok(v) = selem.get_playback_switch(mono) {
                    eq.enabled = v != 0;
                    found_any = true;
                }
            }
            for band in 0..3 {
                if let Some(selem) = self
                    .mixer
                    .find_selem(&format!("{strip} EQ Band {} Type", band + 1), 0)
                {
                    if let Ok(v) = selem.get_enum_item(mono) {
                        eq.bands[band].band_type = eq_band_type_from_enum(v);
                        found_any = true;
                    }
                }
                if let Some(selem) = self
                    .mixer
                    .find_selem(&format!("{strip} EQ Band {} Freq", band + 1), 0)
                {
                    if let Ok(v) = selem.get_playback_volume(mono) {
                        eq.bands[band].freq_hz = v.clamp(0, 20_000) as u16;
                    }
                }
                if let Some(selem) = self
                    .mixer
                    .find_selem(&format!("{strip} EQ Band {} Q", band + 1), 0)
                {
                    if let Ok(v) = selem.get_playback_volume(mono) {
                        eq.bands[band].q = v as f32 / 100.0;
                    }
                }
                if let Some(selem) = self
                    .mixer
                    .find_selem(&format!("{strip} EQ Band {} Gain", band + 1), 0)
                {
                    if let Ok(v) = selem.get_playback_volume(mono) {
                        eq.bands[band].gain_db = v as f32 / 10.0;
                    }
                }
            }
            if let Some(selem) = self
                .mixer
                .find_selem(&format!("{strip} EQ Low Cut Freq"), 0)
            {
                if let Ok(v) = selem.get_playback_volume(mono) {
                    eq.low_cut_freq_hz = v.clamp(0, 20_000) as u16;
                }
            }
            if let Some(selem) = self
                .mixer
                .find_selem(&format!("{strip} EQ Low Cut Slope"), 0)
            {
                if let Ok(v) = selem.get_enum_item(mono) {
                    eq.low_cut_slope_db_oct = eq_slope_from_enum(v);
                }
            }
            if found_any {
                self.inputs[i].eq = Some(eq);
            }
        }

        self.read_panel();

        if !resync {
            info!(
                "Attached {} inputs, {} playbacks, clock: {}",
                self.inputs.len(),
                self.playbacks.len(),
                self.settings.clock_source
            );
        }
    }

    /// Read the driver's front-panel controls into `self.panel`, and act
    /// on DIM presses counted since the last read.
    fn read_panel(&mut self) {
        let mono = SelemChannelId::mono();
        if self.grammar != ControlGrammar::Proprietary {
            return;
        }
        let enum_item = |name: &str| {
            self.mixer
                .find_selem(name, 0)
                .and_then(|s| s.get_enum_item(mono).ok())
        };
        let (in_item, out_item) = (enum_item("Front Panel In"), enum_item("Front Panel Out"));
        let mix = self
            .mixer
            .find_selem("Front Panel Mix", 0)
            .and_then(|s| s.get_playback_switch(mono).ok())
            .map(|v| v != 0);
        self.panel = match (mix, in_item, out_item) {
            (Some(mix), Some(i), Some(o)) => Some((
                mix,
                panel_enum_to_sel(i).unwrap_or(0),
                panel_enum_to_sel(o).unwrap_or(0),
            )),
            _ => None,
        };

        let count = self
            .mixer
            .find_selem("DIM Button Press Count", 0)
            .and_then(|s| s.get_playback_volume(mono).ok());
        if let (Some(prev), Some(now)) = (self.dim_press_count, count) {
            if dim_presses_toggle(prev, now) {
                if self.app_handles_dim {
                    self.dim_toggle_pending = !self.dim_toggle_pending;
                } else {
                    let on = !self.settings.dim;
                    if let Err(e) = self.set_dim(on) {
                        log::warn!("front-panel DIM press: setting Dim failed: {e}");
                    }
                }
            }
        }
        self.dim_press_count = count;
    }

    /// The front-panel state the driver reports, for the UI to follow:
    /// (MIX engaged, IN selection, OUT selection), in the USB backend's
    /// convention (IN 0 = Ch 1/2, 1 = Ch 3/4, 2 = Opt; OUT 0 = Ch 1/2,
    /// 1 = Phones, 2 = Opt).  `None` if the driver has no front-panel
    /// controls, or the card is in class-compliant mode.
    pub fn panel_selection(&self) -> Option<(bool, usize, usize)> {
        self.panel
    }

    fn channel(&self, ch: ChannelId) -> Result<(&bool, &bool), Error> {
        match ch {
            ChannelId::Input(idx) => self
                .inputs
                .get(idx)
                .map(|c| (&c.mute, &c.solo))
                .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx))),
            ChannelId::Playback(idx) => self
                .playbacks
                .get(idx)
                .map(|c| (&c.mute, &c.solo))
                .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx))),
            ChannelId::Output(idx) => self
                .outputs
                .get(idx)
                .map(|c| (&c.mute, &c.solo))
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx))),
        }
    }

    fn channel_mut(&mut self, ch: ChannelId) -> Result<(&mut bool, &mut bool), Error> {
        match ch {
            ChannelId::Input(idx) => self
                .inputs
                .get_mut(idx)
                .map(|c| (&mut c.mute, &mut c.solo))
                .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx))),
            ChannelId::Playback(idx) => self
                .playbacks
                .get_mut(idx)
                .map(|c| (&mut c.mute, &mut c.solo))
                .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx))),
            ChannelId::Output(idx) => self
                .outputs
                .get_mut(idx)
                .map(|c| (&mut c.mute, &mut c.solo))
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx))),
        }
    }
}

impl RmeDevice for BabyfacePro {
    fn can_mono(&self) -> bool {
        self.grammar == ControlGrammar::Proprietary && self.cross_selem(8, 0).is_some()
    }

    fn mono_out(&self) -> Option<usize> {
        self.mono_out
    }

    fn set_mono_out(&mut self, out: Option<usize>) -> Result<(), Error> {
        if out.is_some() && !self.can_mono() {
            return Err(Error::MixerElementNotFound("Cross Playback Volume (update the driver for Mono)".into()));
        }
        let old = std::mem::replace(&mut self.mono_out, out);
        // Rewrite every source into the output that folds and the one that
        // unfolds, at its level as heard now.
        for o in [old, out].into_iter().flatten() {
            for j in 0..self.inputs.len() {
                self.rewrite_channel(ChannelId::Input(j), o)?;
            }
            for j in 0..self.playbacks.len() {
                self.rewrite_channel(ChannelId::Playback(j), o)?;
            }
        }
        Ok(())
    }

    /// Every linked stereo input and playback pair, on a driver with the
    /// crossed cells (AN1/2 and AN3/4 need none: they are two mono
    /// sources, each with both sides).
    fn can_source_width(&self, channel: ChannelId) -> bool {
        self.grammar == ControlGrammar::Proprietary
            && crate::device::source_width_pair(channel).is_some()
            && crate::device::plays_as_stereo_pair(self, channel)
            && (matches!(channel, ChannelId::Input(0..=3)) || self.cross_selem(8, 0).is_some())
    }

    /// Kept with the pair's name, then both channels rewritten into every
    /// output: own (1+w)/2 and crossed (1-w)/2 of the fader (TotalMix's
    /// writes, PROTOCOL.md "Width strip mapping").
    fn set_source_width(&mut self, channel: ChannelId, width: f32) -> Result<(), Error> {
        let pair = crate::device::source_width_pair(channel)
            .filter(|_| self.can_source_width(channel))
            .ok_or_else(|| Error::InvalidChannel("Width is for a linked stereo pair".into()))?;
        let w = width.clamp(-1.0, 1.0);
        if w >= 0.995 {
            self.settings.source_width.remove(pair);
        } else {
            self.settings.source_width.insert(pair.to_string(), w);
        }
        let (a, b) = match channel {
            ChannelId::Input(i) => (ChannelId::Input(i & !1), ChannelId::Input(i | 1)),
            ChannelId::Playback(i) => (ChannelId::Playback(i & !1), ChannelId::Playback(i | 1)),
            ChannelId::Output(_) => return Ok(()),
        };
        for o in 0..self.profile.output_pair_count() {
            self.rewrite_channel(a, o)?;
            self.rewrite_channel(b, o)?;
        }
        Ok(())
    }

    fn model_name(&self) -> &str {
        self.profile.model_name
    }

    fn output_pair_count(&self) -> usize {
        self.profile.output_pair_count()
    }

    /// The class-compliant surface is routing, preamps and clock; every
    /// feature in RME's own protocol is missing there (see
    /// `ControlGrammar`). The UIs gate on this so they stop offering
    /// controls that would fail.
    fn protocol_features(&self) -> bool {
        self.grammar != ControlGrammar::ClassCompliant
    }

    /// Card still resolvable (the unplug case ends here, silently), and
    /// still the same *enumeration*: a re-created card (replug,
    /// personality switch) comes back on the same ALSA index with the
    /// same names while our mixer keeps answering from a fd the old card
    /// no longer backs, and the USB `devnum` is the one thing that
    /// moves. A mixer iteration cannot tell: snd_mixer caches its
    /// element list, so it yields elements long after the card is gone
    /// (checked against a live personality switch, 2026-10-16: the chip
    /// kept saying "Proprietary" for a class-compliant card).
    fn is_connected(&self) -> bool {
        AlsaMixer::resolve_card(PROFILE.card_substring).is_some()
            && AlsaMixer::card_devnum(self.mixer.card_name()) == self.card_devnum
    }

    fn open() -> Result<Self, Error> {
        info!("Searching for RME Babyface Pro...");
        let profile = &PROFILE;
        let mixer = AlsaMixer::open_by_card_name(profile.card_substring)?;

        // Matching the card *name* isn't enough to know which mixer
        // this is. A Babyface Pro in Class Compliant mode on stock
        // `snd-usb-audio` announces itself as "RME Babyface Pro
        // (<serial>)", which contains `card_substring` just as ours
        // does, yet speaks a completely different control grammar.
        // Every lookup below is a tolerant `if let Some(selem)`, so
        // guessing wrong doesn't fail loudly — it comes up "working"
        // while driving nothing (observed 2026-09-12: 48V read as off
        // while the card had it on, and setters returning `Ok(())`
        // without reaching the hardware). So pick the grammar from a
        // sentinel control unique to each, and refuse only if neither
        // is there.
        let grammar = if mixer.find_selem("Mic 1", 0).is_some() {
            ControlGrammar::Proprietary
        } else if mixer.find_selem("Mic-AN1 Gain", 0).is_some() {
            ControlGrammar::ClassCompliant
        } else {
            return Err(Error::UnsupportedDeviceMode {
                card: mixer.card_name().to_string(),
                missing: "Mic 1 (proprietary), Mic-AN1 Gain (class-compliant)".into(),
            });
        };
        info!("ALSA control grammar: {grammar:?}");

        // Meter the inputs through PipeWire's own source when it is
        // running. Opening the capture PCM directly removes the input
        // node from the graph and pins the card to this meter's period,
        // which is what made a fresh session lose its capture device and
        // report 1024 frames. The direct capture stays as the fallback
        // for a system without PipeWire.
        let pipewire = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|dir| std::path::Path::new(&dir).join("pipewire-0").exists())
            .unwrap_or(false);
        let (input_tap, capture_meter) = if pipewire {
            (Some(crate::pw_meter::Tap::start_input()), None)
        } else {
            (
                None,
                crate::capture_meter::CaptureMeter::start(mixer.card_name(), 48_000),
            )
        };
        if input_tap.is_none() && capture_meter.is_none() {
            log::warn!(
                "Could not open input metering (PipeWire source or capture PCM); meters will read N/A"
            );
        }
        let mut device = Self {
            card_held: false,
            card_devnum: AlsaMixer::card_devnum(mixer.card_name()),
            mixer,
            grammar,
            profile,
            inputs: profile.build_inputs(),
            playbacks: profile.build_playbacks(),
            outputs: profile.build_outputs(),
            settings: DeviceSettings {
                clock_source: "Internal".into(),
                clock_sources: Vec::new(),
                spdif_optical: false,
                spdif_emphasis: false,
                spdif_professional: false,
                spdif_enabled: false,
                pitch_percent: 0.0,
                ms_proc: false,
                an12: false,
                eq_for_record: false,
                optical_out_spdif: false,
                dim: false,
                fx_send_db: None,
                source_fx_send: Default::default(),
                output_phase: Vec::new(),
                output_ms: Vec::new(),
                output_eq: Vec::new(),
                source_ms: Default::default(),
                source_width: Default::default(),
                fx_return: Vec::new(),
                width: 0.0,
                sample_rate: 48_000,
                input_link: true,
                output_link: Vec::new(),
                input_pair_link: Vec::new(),
            },
            input_tap,
            fx_in: std::cell::Cell::new(None),
            fx_send_trim_db: 0.0,
            capture_meter,
            linked: true,
            dim_press_count: None,
            app_handles_dim: false,
            dim_toggle_pending: false,
            panel: None,
            resync_at: None,
            resync_poll_at: Some(std::time::Instant::now() + FIRST_RESYNC),
            gain_poll_at: None,
            mono_out: None,
            raw_crosspoint: false,
            dsp_block: false,
            eq_rate: std::cell::Cell::new(48_000),
        };
        device.raw_crosspoint = device.mixer.has_ctl_elem("Raw Crosspoint");
        device.dsp_block = device.grammar == ControlGrammar::Proprietary && device.mixer.has_ctl_elem("DSP Block");
        device.clear_fx_feedback();
        device.attach_mixer_elements(false);
        if device.dsp_block {
            // The input EQ as the driver had it, now Audra's: the driver's
            // strips off (it sends flat blocks), then Audra's own.
            for strip in &BF_SOURCES[..4] {
                if let Some(selem) = device.mixer.find_selem(&format!("{strip} EQ Enable"), 0) {
                    let _ = selem.set_playback_switch(SelemChannelId::mono(), 0);
                }
            }
            device.resend_eq();
        }
        Ok(device)
    }

    fn inputs(&self) -> &[InputChannel] {
        &self.inputs
    }

    fn inputs_mut(&mut self) -> &mut [InputChannel] {
        &mut self.inputs
    }

    fn playbacks(&self) -> &[PlaybackChannel] {
        &self.playbacks
    }

    fn playbacks_mut(&mut self) -> &mut [PlaybackChannel] {
        &mut self.playbacks
    }

    fn outputs(&self) -> &[OutputChannel] {
        &self.outputs
    }

    fn outputs_mut(&mut self) -> &mut [OutputChannel] {
        &mut self.outputs
    }

    fn settings(&self) -> &DeviceSettings {
        &self.settings
    }

    fn settings_mut(&mut self) -> &mut DeviceSettings {
        &mut self.settings
    }

    fn set_volume(&mut self, channel: ChannelId, output: usize, volume: f32) -> Result<(), Error> {
        // A level that isn't a number is silence, never the top.
        let volume = crate::channel::safe_volume(volume);
        let vol_clamped = volume.clamp(0.0, 1.0);

        // Output channels: one 2-channel "<pair label> Playback Volume"
        // ALSA element per pair (`PAIR_LABELS`), front-left/front-right
        // are the pair's two physical channels — `idx / 2` finds the
        // pair, `idx % 2` picks which channel of it.
        if let ChannelId::Output(idx) = channel {
            let out = self
                .outputs
                .get_mut(idx)
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx)))?;
            let label = PAIR_LABELS
                .get(idx / 2)
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx)))?;
            let ch = if idx % 2 == 0 {
                SelemChannelId::FrontLeft
            } else {
                SelemChannelId::FrontRight
            };
            if self.grammar == ControlGrammar::ClassCompliant {
                if let Some(dest) = cc_dest_name(idx / 2, idx % 2 == 1) {
                    if let Some(selem) = self.mixer.find_selem(&format!("Main-Out {dest}"), 0) {
                        let max = selem.get_playback_volume_range().1 as f32;
                        selem.set_playback_volume(
                            SelemChannelId::mono(),
                            (vol_clamped * max) as i64,
                        )?;
                    }
                }
                out.volume = vol_clamped;
            } else {
                // The model's +6 dB headroom is real on this driver.
                let vol = volume.clamp(0.0, 2.0);
                if let Some(selem) = self.mixer.find_selem(label, (idx / 2) as u32) {
                    set_tlv_volume(&selem, ch, vol)?;
                }
                out.volume = vol;
            }
            return Ok(());
        }

        if output >= self.profile.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output {}", output)));
        }

        match channel {
            ChannelId::Input(idx) => {
                input_crosspoint_slot(idx).ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
            }
            ChannelId::Playback(idx) => {
                playback_crosspoint_slot(idx).ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx)))?;
            }
            ChannelId::Output(_) => unreachable!("Output handled above"),
        }
        // The driver's crosspoints reach +6 dB, the model's 2.0.
        let vol = if self.grammar == ControlGrammar::ClassCompliant {
            vol_clamped
        } else {
            volume.clamp(0.0, 2.0)
        };
        // The model takes the fader's level; the card gets it through the
        // strip's mute and solo (`rewrite_channel`, which also keeps its
        // pan and a playback pair's balance). A fader moved on a muted
        // strip wrote its level to the card and brought it back into the
        // monitors (OpenCode's job 8). On a failed write the model keeps
        // the level it had.
        let slot = match channel {
            ChannelId::Input(idx) => &mut self.inputs[idx].volumes[output],
            ChannelId::Playback(idx) => &mut self.playbacks[idx].volumes[output],
            ChannelId::Output(_) => unreachable!("Output handled above"),
        };
        let before = std::mem::replace(slot, vol);
        if let Err(e) = self.rewrite_channel(channel, output) {
            match channel {
                ChannelId::Input(idx) => self.inputs[idx].volumes[output] = before,
                ChannelId::Playback(idx) => self.playbacks[idx].volumes[output] = before,
                ChannelId::Output(_) => {}
            }
            return Err(e);
        }
        Ok(())
    }

    fn volume(&self, channel: ChannelId, output: usize) -> Result<f32, Error> {
        match channel {
            ChannelId::Input(idx) => {
                let ch = self
                    .inputs
                    .get(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
                ch.volumes
                    .get(output)
                    .copied()
                    .ok_or_else(|| Error::InvalidChannel(format!("Output {}", output)))
            }
            ChannelId::Playback(idx) => {
                let ch = self
                    .playbacks
                    .get(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx)))?;
                ch.volumes
                    .get(output)
                    .copied()
                    .ok_or_else(|| Error::InvalidChannel(format!("Output {}", output)))
            }
            ChannelId::Output(idx) => self
                .outputs
                .get(idx)
                .map(|c| c.volume)
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx))),
        }
    }

    /// The four mono inputs pan; a linked playback pair has its balance
    /// (`set_playback_balance`). The optical pairs have no balance on
    /// this driver yet, and a split playback channel nowhere to pan to.
    fn can_pan(&self, channel: ChannelId) -> bool {
        match channel {
            ChannelId::Input(idx) => input_crosspoint_slot(idx).is_some_and(|(_, _, mono)| mono),
            ChannelId::Playback(idx) => self.playbacks.get(idx).is_some_and(|p| !p.split),
            ChannelId::Output(_) => false,
        }
    }

    fn set_pan(&mut self, channel: ChannelId, output: usize, pan: i8) -> Result<(), Error> {
        let pan = pan.clamp(-100, 100);

        // Only the 4 true-mono sources (AN1-AN4) have an independent
        // left/right crosspoint pair to pan across — AS1/2, the ADAT
        // pairs and every PCM playback channel are hardware-linked
        // stereo sources with one ALSA channel each, no cross-routing
        // possible (see `BF_SOURCES`/`input_crosspoint_slot`). A linked
        // playback pair still has a balance, one side lowered
        // (`set_playback_balance`); the optical inputs don't have one here
        // yet.
        let (idx, volume) = match channel {
            ChannelId::Input(idx) => {
                let ch = self
                    .inputs
                    .get(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
                if output >= ch.pans.len() {
                    return Err(Error::InvalidChannel(format!("Output {}", output)));
                }
                (idx, ch.volumes[output])
            }
            ChannelId::Playback(idx) => return self.set_playback_balance(idx, output, pan),
            ChannelId::Output(_) => return Err(Error::InvalidChannel("Output has no pan".into())),
        };
        let (src, _ch, is_mono) = input_crosspoint_slot(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if !is_mono {
            return Err(Error::InvalidChannel(format!(
                "Input {} is a linked stereo source and has no independent pan",
                idx
            )));
        }

        self.write_crosspoint(src, SelemChannelId::FrontLeft, true, output, volume, pan)?;

        self.inputs[idx].pans[output] = pan;
        Ok(())
    }

    fn pan(&self, channel: ChannelId, output: usize) -> Result<i8, Error> {
        match channel {
            ChannelId::Input(idx) => self
                .inputs
                .get(idx)
                .and_then(|c| c.pans.get(output).copied())
                .ok_or_else(|| Error::InvalidChannel(format!("Channel {}", idx))),
            ChannelId::Playback(idx) => self
                .playbacks
                .get(idx)
                .and_then(|c| c.pans.get(output).copied())
                .ok_or_else(|| Error::InvalidChannel(format!("Channel {}", idx))),
            ChannelId::Output(_) => Err(Error::InvalidChannel("Output has no pan".into())),
        }
    }

    fn set_mute(&mut self, channel: ChannelId, mute: bool) -> Result<(), Error> {
        // Output mute is a real hardware switch, but it's shared by the
        // whole pair (`mixer.c`'s `bf_mute_get` mirrors one flag onto
        // both ALSA channels) — muting one physical output channel
        // mutes its sibling too, so mirror that into both `self.outputs`
        // entries rather than leaving them inconsistent with hardware.
        if let ChannelId::Output(idx) = channel {
            let pair_idx = idx / 2;
            let label = PAIR_LABELS
                .get(pair_idx)
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx)))?;
            if self.grammar == ControlGrammar::ClassCompliant {
                // No mute switch exists anywhere on the CC surface, so
                // mute is "drive the master to zero and put the stored
                // volume back on unmute" — the same trick `usb.rs` uses
                // for strip mute. The real level stays in the model, so
                // nothing is lost; the cost is that mute state doesn't
                // survive a re-open (there's no hardware bit to read).
                for side in 0..2 {
                    let Some(dest) = cc_dest_name(pair_idx, side == 1) else {
                        continue;
                    };
                    let Some(selem) = self.mixer.find_selem(&format!("Main-Out {dest}"), 0) else {
                        continue;
                    };
                    let max = selem.get_playback_volume_range().1 as f32;
                    let level = if mute {
                        0.0
                    } else {
                        self.outputs
                            .get(pair_idx * 2 + side)
                            .map(|o| o.volume)
                            .unwrap_or(0.0)
                    };
                    selem.set_playback_volume(SelemChannelId::mono(), (level * max) as i64)?;
                }
            } else if let Some(selem) = self.mixer.find_selem(label, pair_idx as u32) {
                let unmuted = (!mute) as i32;
                selem.set_playback_switch(SelemChannelId::FrontLeft, unmuted)?;
                selem.set_playback_switch(SelemChannelId::FrontRight, unmuted)?;
            }
            if let Some(out) = self.outputs.get_mut(pair_idx * 2) {
                out.mute = mute;
            }
            if let Some(out) = self.outputs.get_mut(pair_idx * 2 + 1) {
                out.mute = mute;
            }
            return Ok(());
        }

        // Input/Playback have no dedicated ALSA mute switch — same as
        // TotalMix on Windows, which does this by driving the routing
        // matrix directly: `cap_mute2.pcap` (2026-08-24) shows muting a
        // strip zeroing its crosspoints into EVERY output, restored
        // from the model on unmute. Ported from `usb.rs`'s own
        // already-hardware-validated implementation of the same
        // capture, just through ALSA crosspoint controls instead of
        // raw USB writes. No solo-awareness here, matching `usb.rs`
        // exactly: unmuting a strip while some other strip is soloed
        // restores this strip's own levels regardless — a real
        // TotalMix quirk this ports faithfully rather than "fixing".
        let n_outputs = self.profile.output_pair_count();
        match channel {
            ChannelId::Input(idx) => {
                let (src, ch, is_mono) = input_crosspoint_slot(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
                for out in 0..n_outputs {
                    let level = if mute {
                        0.0
                    } else {
                        self.inputs[idx].volumes[out]
                    };
                    let pan = self.inputs[idx].pans[out];
                    self.write_crosspoint(src, ch, is_mono, out, level, pan)?;
                }
            }
            ChannelId::Playback(idx) => {
                let (src, ch) = playback_crosspoint_slot(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx)))?;
                for out in 0..n_outputs {
                    let level = if mute { 0.0 } else { self.playback_level(idx, out, self.playbacks[idx].volumes[out]) };
                    self.write_crosspoint(src, ch, false, out, level, 0)?;
                }
            }
            ChannelId::Output(_) => unreachable!("handled above"),
        }

        let ch = self.channel_mut(channel)?;
        *ch.0 = mute;
        Ok(())
    }

    fn mute(&self, channel: ChannelId) -> Result<bool, Error> {
        let ch = self.channel(channel)?;
        Ok(*ch.0)
    }

    fn set_solo(&mut self, channel: ChannelId, solo: bool) -> Result<(), Error> {
        // Ported from `usb.rs`'s own already-hardware-validated
        // implementation of `cap_solo2.pcap` (2026-08-24): soloing a
        // strip does NOT touch its own crosspoints — it zeroes every
        // OTHER input/playback strip's send into every output instead,
        // so only the soloed strip is heard in the monitor mix.
        // Un-solo restores every other strip from the model,
        // respecting its own independent mute. Outputs have no solo in
        // TotalMix. Exclusive solo (TotalMix's default): soloing a
        // strip clears every other strip's solo flag first, so a later
        // un-solo of any of them is a no-op instead of fighting this
        // one.
        let (solo_i, solo_c) = match channel {
            ChannelId::Input(i) => (Some(i), None),
            ChannelId::Playback(c) => (None, Some(c)),
            ChannelId::Output(_) => return Ok(()),
        };
        // A linked pair is soloed one channel at a time
        // (`set_input_solo`/`set_playback_solo`), so the exclusive solo
        // below must spare the strip's stereo partner: it used to clear
        // the partner's flag and zero its sends, so a soloed linked pair
        // played one side only and its strip (which shows the left
        // channel) never lit.
        let partner_i = solo_i
            .filter(|i| self.input_pair_linked(i / 2))
            .map(|i| i ^ 1);
        let partner_c = solo_c
            .filter(|c| self.playback_linked(c / 2))
            .map(|c| c ^ 1);
        match channel {
            ChannelId::Input(i) => self.inputs[i].solo = solo,
            ChannelId::Playback(c) => self.playbacks[c].solo = solo,
            ChannelId::Output(_) => unreachable!(),
        }

        let n_outputs = self.profile.output_pair_count();
        for j in 0..self.inputs.len() {
            if solo_i == Some(j) || partner_i == Some(j) {
                continue;
            }
            if solo {
                self.inputs[j].solo = false;
            }
            let Some((src, ch, is_mono)) = input_crosspoint_slot(j) else {
                continue;
            };
            let muted = self.inputs[j].mute;
            for out in 0..n_outputs {
                let level = if solo || muted {
                    0.0
                } else {
                    self.inputs[j].volumes[out]
                };
                let pan = self.inputs[j].pans[out];
                self.write_crosspoint(src, ch, is_mono, out, level, pan)?;
            }
        }
        for j in 0..self.playbacks.len() {
            if solo_c == Some(j) || partner_c == Some(j) {
                continue;
            }
            if solo {
                self.playbacks[j].solo = false;
            }
            let Some((src, ch)) = playback_crosspoint_slot(j) else {
                continue;
            };
            let muted = self.playbacks[j].mute;
            for out in 0..n_outputs {
                let level = if solo || muted { 0.0 } else { self.playback_level(j, out, self.playbacks[j].volumes[out]) };
                self.write_crosspoint(src, ch, false, out, level, 0)?;
            }
        }
        Ok(())
    }

    fn solo(&self, channel: ChannelId) -> Result<bool, Error> {
        let ch = self.channel(channel)?;
        Ok(*ch.1)
    }

    fn set_phantom(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if inp.channel_type != ChannelType::Mic {
            return Err(Error::InvalidChannel(format!(
                "Input {} has no 48V phantom power",
                idx
            )));
        }
        // Mic 1/Mic 2 = inputs[0]/inputs[1], mapping 1:1 to ALSA `.index`
        // (both mics share the literal name "Phantom Power Mic 1").
        if let Some(selem) = self.phantom_selem(idx) {
            selem.set_playback_switch(SelemChannelId::mono(), on as i32)?;
        }
        self.inputs[idx].phantom = on;
        Ok(())
    }

    fn set_pad(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if inp.channel_type != ChannelType::Mic {
            return Err(Error::InvalidChannel(format!(
                "Input {} has no pad switch",
                idx
            )));
        }
        if let Some(selem) = self.pad_selem(idx) {
            selem.set_playback_switch(SelemChannelId::mono(), on as i32)?;
        }
        self.inputs[idx].pad = on;
        Ok(())
    }

    fn set_gain(&mut self, idx: usize, gain: u32) -> Result<(), Error> {
        let inp = self
            .inputs
            .get(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if !matches!(inp.channel_type, ChannelType::Mic | ChannelType::Instrument) {
            return Err(Error::InvalidChannel(format!(
                "Input {} has no gain control",
                idx
            )));
        }
        let clamped = inp.gain_max.map_or(gain, |max| gain.min(max));
        // A *capture* volume in both grammars (the class-compliant
        // `<base> Gain` carries playback and capture volume both, and
        // the capture side is the one that matches the proprietary
        // driver's law and range).
        if let Some(selem) = self.preamp_gain_selem(idx) {
            selem.set_capture_volume(SelemChannelId::mono(), clamped as i64)?;
        }
        self.inputs[idx].gain = Some(clamped);
        Ok(())
    }

    fn set_eq_enabled(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Enable"), 0) {
            selem.set_playback_switch(SelemChannelId::mono(), on as i32)?;
        }
        inp.eq.get_or_insert_with(InputEq::default).enabled = on;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_eq_band_type(
        &mut self,
        idx: usize,
        band: usize,
        band_type: EqBandType,
    ) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {}", band)));
        }
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Band {} Type", band + 1), 0)
        {
            selem.set_enum_item(SelemChannelId::mono(), eq_band_type_to_enum(band_type))?;
        }
        inp.eq.get_or_insert_with(InputEq::default).bands[band].band_type = band_type;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_eq_band_freq(&mut self, idx: usize, band: usize, freq_hz: u16) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {}", band)));
        }
        let clamped = freq_hz.min(20_000);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Band {} Freq", band + 1), 0)
        {
            selem.set_playback_volume(SelemChannelId::mono(), clamped as i64)?;
        }
        inp.eq.get_or_insert_with(InputEq::default).bands[band].freq_hz = clamped;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_eq_band_q(&mut self, idx: usize, band: usize, q: f32) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {}", band)));
        }
        let clamped = q.clamp(0.05, 10.0);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Band {} Q", band + 1), 0)
        {
            selem.set_playback_volume(SelemChannelId::mono(), (clamped * 100.0).round() as i64)?;
        }
        inp.eq.get_or_insert_with(InputEq::default).bands[band].q = clamped;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_eq_band_gain(&mut self, idx: usize, band: usize, gain_db: f32) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {}", band)));
        }
        let clamped = gain_db.clamp(-24.0, 24.0);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Band {} Gain", band + 1), 0)
        {
            selem.set_playback_volume(SelemChannelId::mono(), (clamped * 10.0).round() as i64)?;
        }
        inp.eq.get_or_insert_with(InputEq::default).bands[band].gain_db = clamped;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_eq_low_cut_freq(&mut self, idx: usize, freq_hz: u16) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        let clamped = freq_hz.min(20_000);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Low Cut Freq"), 0)
        {
            selem.set_playback_volume(SelemChannelId::mono(), clamped as i64)?;
        }
        inp.eq.get_or_insert_with(InputEq::default).low_cut_freq_hz = clamped;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_eq_low_cut_slope(&mut self, idx: usize, slope_db_oct: u8) -> Result<(), Error> {
        self.require_proprietary("EQ")?;
        let strip = eq_strip_name(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {} has no EQ", idx)))?;
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if let Some(selem) = eq_selem(&self.mixer, self.dsp_block, &format!("{strip} EQ Low Cut Slope"), 0)
        {
            selem.set_enum_item(SelemChannelId::mono(), eq_slope_to_enum(slope_db_oct))?;
        }
        inp.eq
            .get_or_insert_with(InputEq::default)
            .low_cut_slope_db_oct = slope_db_oct;
        self.upload_eq(crate::eq_block::EqChannel::Input(idx))
    }

    fn set_sensitivity(&mut self, idx: usize, sensitivity: Sensitivity) -> Result<(), Error> {
        if self
            .inputs
            .get(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?
            .channel_type
            != ChannelType::Instrument
        {
            return Err(Error::InvalidChannel(format!(
                "Input {} has no sensitivity switch",
                idx
            )));
        }
        // A single shared switch for the Instrument pair, not
        // per-channel — the protocol has no independent bits for IN3
        // vs IN4 (see `babyface-pro-linux`'s own "Instrument Ref
        // Level" control, added 2026-09-06 specifically to close this
        // gap the comment below used to describe as possibly a
        // genuinely-missing driver feature — it wasn't, just not built
        // yet at the time). Only 2 of that control's 3 states map to
        // this trait's 2-state `Sensitivity` enum; "Boost" isn't
        // reachable here (see `RmeDevice::set_ref_level`'s own 3-state
        // `REF_*` codes for that, used by the USB backend).
        if self.grammar == ControlGrammar::ClassCompliant {
            // Two differences from the proprietary control, both of
            // which would silently invert or over-apply the setting if
            // assumed away: it is PER-CHANNEL here (`Line-IN3 Sens.` /
            // `Line-IN4 Sens.`, so no mirroring), and its item order is
            // REVERSED — the card enumerates ["-10dBV", "+4dBu"], while
            // "Instrument Ref Level" enumerates ["+4dBu", "-10dBV",
            // "Boost"]. Resolve the item by *name* rather than by index
            // so the order can never bite us again.
            let base = Self::cc_preamp_base(idx).ok_or_else(|| {
                Error::InvalidChannel(format!("Input {idx} has no Sens. control"))
            })?;
            let selem = self
                .mixer
                .find_selem(&format!("{base} Sens."), 0)
                .ok_or_else(|| Error::InvalidChannel(format!("No {base} Sens. control")))?;
            let wanted = match sensitivity {
                Sensitivity::Plus4dBu => "+4dBu",
                Sensitivity::Minus10dBV => "-10dBV",
            };
            let count = selem.get_enum_items()?;
            let item = (0..count)
                .find(|i| selem.get_enum_item_name(*i).is_ok_and(|n| n == wanted))
                .ok_or_else(|| {
                    Error::InvalidChannel(format!("{base} Sens. has no {wanted} setting"))
                })?;
            selem.set_enum_item(SelemChannelId::mono(), item)?;
            self.inputs[idx].sensitivity = Some(sensitivity);
            return Ok(());
        }

        let item = match sensitivity {
            Sensitivity::Plus4dBu => 0,
            Sensitivity::Minus10dBV => 1,
        };
        let selem = self
            .mixer
            .find_selem("Instrument Ref Level", 0)
            .ok_or_else(|| Error::InvalidChannel("No Ref Level control".into()))?;
        selem.set_enum_item(SelemChannelId::mono(), item)?;
        // Mirror across every Instrument channel — there's only one
        // real switch, so both must agree (matches `usb.rs`'s own
        // `set_ref_level` fix, same day).
        for c in self.inputs.iter_mut() {
            if c.channel_type == ChannelType::Instrument {
                c.sensitivity = Some(sensitivity);
            }
        }
        Ok(())
    }

    /// The Instrument inputs' reference level with all three of the
    /// driver's states: `raw` is the USB backend's `REF_*` code (1 +4dBu,
    /// 2 -10dBV, 3 Boost). Boost exists only on the driver's own
    /// "Instrument Ref Level" control (regular mode); the class-compliant
    /// `Sens.` controls have the other two, set through `set_sensitivity`.
    /// One shared switch, mirrored to every Instrument channel.
    fn set_ref_level(&mut self, idx: usize, raw: u16) -> Result<(), Error> {
        if self.inputs.get(idx).map(|c| c.channel_type) != Some(ChannelType::Instrument) {
            return Err(Error::InvalidChannel(format!(
                "Input {idx} has no ref-level switch"
            )));
        }
        let sensitivity = match raw {
            1 => Sensitivity::Plus4dBu,
            2 | 3 => Sensitivity::Minus10dBV,
            other => {
                return Err(Error::InvalidChannel(format!("Unknown ref level {other}")));
            }
        };
        if self.grammar == ControlGrammar::ClassCompliant {
            if raw == 3 {
                return Err(Error::InvalidChannel(
                    "Boost needs the interface in its regular (PC) mode".into(),
                ));
            }
            return self.set_sensitivity(idx, sensitivity);
        }
        let wanted = match raw {
            1 => "+4dBu",
            2 => "-10dBV",
            _ => "Boost",
        };
        let selem = self
            .mixer
            .find_selem("Instrument Ref Level", 0)
            .ok_or_else(|| Error::InvalidChannel("No Ref Level control".into()))?;
        let count = selem.get_enum_items()?;
        let item = (0..count)
            .find(|i| selem.get_enum_item_name(*i).is_ok_and(|n| n == wanted))
            .ok_or_else(|| Error::InvalidChannel(format!("Ref Level has no {wanted} setting")))?;
        selem.set_enum_item(SelemChannelId::mono(), item)?;
        for c in self.inputs.iter_mut() {
            if c.channel_type == ChannelType::Instrument {
                c.ref_level = raw;
                c.sensitivity = Some(sensitivity);
            }
        }
        Ok(())
    }

    fn set_pitch(&mut self, pitch_percent: f32) -> Result<(), Error> {
        self.require_proprietary("Varispeed pitch")?;
        // "Varispeed Pitch" exists over ALSA too (mixer.c:1021-1030),
        // range -50..50 in 0.1%-steps (raw = percent * 10) — despite
        // this method's old comment claiming otherwise, this is not
        // USB-only.
        let clamped = pitch_percent.clamp(-5.0, 5.0);
        if let Some(selem) = self.mixer.find_selem("Varispeed Pitch", 0) {
            let raw = (clamped * 10.0).round() as i64;
            selem.set_playback_volume(SelemChannelId::mono(), raw)?;
        }
        self.settings.pitch_percent = clamped;
        Ok(())
    }

    fn set_loopback(&mut self, out: usize, on: bool) -> Result<(), Error> {
        self.require_proprietary("Loopback")?;
        if out >= PAIR_LABELS.len() {
            return Err(Error::InvalidChannel(format!("Output pair {}", out)));
        }
        if let Some(selem) = self.mixer.find_selem("Loopback", out as u32) {
            selem.set_playback_switch(SelemChannelId::mono(), on as i32)?;
        }
        if let Some(o) = self.outputs.get_mut(out * 2) {
            o.loopback = on;
        }
        if let Some(o) = self.outputs.get_mut(out * 2 + 1) {
            o.loopback = on;
        }
        Ok(())
    }

    /// M/S on AN1/2, written through the cells as on every other stereo
    /// source (#15; `reassert_output`). The driver's own "MS Processor"
    /// muted AN2 behind its cache, and its release put back AN2's Phones
    /// level into Main: it is only ever switched off here, so an older
    /// driver doesn't replay it at the next start.
    fn set_ms_proc(&mut self, on: bool) -> Result<(), Error> {
        self.require_proprietary("MS processing")?;
        if !self.can_output_proc() {
            return Err(Error::InvalidChannel("M/S needs the Babyface driver from 2026-11-09".into()));
        }
        let mono = SelemChannelId::mono();
        if let Some(selem) = self.mixer.find_selem("MS Processor", 0) {
            if selem.get_playback_switch(mono).is_ok_and(|v| v != 0) {
                selem.set_playback_switch(mono, 0)?;
            }
        }
        self.settings.ms_proc = on;
        for out in 0..self.profile.output_pair_count() {
            self.reassert_output(out, Some(0), true)?;
        }
        Ok(())
    }

    fn set_an12(&mut self, on: bool) -> Result<(), Error> {
        self.require_proprietary("AN 1>2")?;
        if let Some(selem) = self.mixer.find_selem("AN 1>2", 0) {
            selem.set_playback_switch(SelemChannelId::mono(), on as i32)?;
        }
        self.settings.an12 = on;
        Ok(())
    }

    fn set_input_link(&mut self, linked: bool) -> Result<(), Error> {
        self.require_proprietary("Input link")?;
        if let Some(selem) = self.mixer.find_selem("AN1/2 Link", 0) {
            selem.set_playback_switch(SelemChannelId::mono(), linked as i32)?;
        }
        self.linked = linked;
        self.settings.input_link = linked;
        Ok(())
    }

    /// Phase Ø invert for an input: the driver's "<source> Phase" switch
    /// (AN1-4 one channel; a stereo source's two, ØL and ØR, from the
    /// 2026-11-07 driver). Errors rather than silently no-opping if
    /// missing (an older module), since a user would notice it not taking.
    fn set_phase(&mut self, idx: usize, invert: bool) -> Result<(), Error> {
        self.require_proprietary("Phase invert")?;
        let (src, ch, is_mono) = input_crosspoint_slot(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        self.write_phase(src, if is_mono { SelemChannelId::mono() } else { ch }, invert)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx} has no phase switch (update the driver)")))??;
        self.inputs[idx].phase = invert;
        self.reassert_outputs(Some(src));
        Ok(())
    }

    fn can_phase(&self, channel: ChannelId) -> bool {
        if self.grammar != ControlGrammar::Proprietary {
            return false;
        }
        // Every input and playback with the driver's switch: the stereo
        // sources' leak (2026-11-07) came from the driver rewriting the
        // crossed cells on Ø; TotalMix writes only the own cells (Windows
        // session 3), and so does the driver since 2026-11-09 (measured:
        // ØL inverts the left at full level, the right stays at noise).
        let src = match channel {
            ChannelId::Input(i) => input_crosspoint_slot(i).map(|(s, _, _)| s),
            ChannelId::Playback(i) => playback_crosspoint_slot(i).map(|(s, _)| s),
            ChannelId::Output(_) => None,
        };
        src.is_some_and(|src| self.phase_selem(src).is_some())
    }

    fn set_playback_phase(&mut self, idx: usize, invert: bool) -> Result<(), Error> {
        self.require_proprietary("Phase invert")?;
        let (src, ch) = playback_crosspoint_slot(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Playback {idx}")))?;
        self.write_phase(src, ch, invert)
            .ok_or_else(|| Error::InvalidChannel(format!("Playback {idx} has no phase switch (update the driver)")))??;
        self.playbacks[idx].phase = invert;
        self.reassert_outputs(Some(src));
        Ok(())
    }
    /// Stereo split (a playback pair routed hard-split into the AN1/2
    /// monitor bus instead of the normal stereo pair) — `babyface-pro-
    /// linux`'s own per-pair `"PBx Stereo Split" Switch` controls
    /// (added 2026-09-06). `pb` is a raw playback CHANNEL index (0-11,
    /// matching `RmeDevice::set_stereo_split`'s own convention); the
    /// pair index (0-5) is `pb / 2`.
    fn set_stereo_split(&mut self, pb: usize, split: bool) -> Result<(), Error> {
        self.require_proprietary("Stereo split")?;
        if pb >= self.playbacks.len() {
            return Err(Error::InvalidChannel(format!("Playback {pb}")));
        }
        let pair = pb / 2;
        let selem = self
            .mixer
            .find_selem(&format!("PB{} Stereo Split", pair + 1), pair as u32)
            .ok_or_else(|| {
                Error::InvalidChannel(format!("Playback {pb} has no stereo split switch"))
            })?;
        selem.set_playback_switch(SelemChannelId::mono(), split as i32)?;
        self.playbacks[pair * 2].split = split;
        self.playbacks[pair * 2 + 1].split = split;
        // A driver before 2026-11-09 also wrote fixed levels into Main for
        // the pair (0 dB left and nothing right, split), behind its cache:
        // Main gets the pair's own cells back.
        if let Some((src, _)) = playback_crosspoint_slot(pair * 2) {
            self.reassert_output(0, Some(src), true)?;
        }
        // Split, each channel has only its own side: a balance the pair
        // had is let go, and the lowered side comes back to its fader.
        if split {
            for c in [pair * 2, pair * 2 + 1] {
                let had_balance = self.playbacks[c].pans.iter().any(|&p| p != 0);
                self.playbacks[c].pans.fill(0);
                if !had_balance || self.playback_silenced(c) {
                    continue;
                }
                let Some((src, ch)) = playback_crosspoint_slot(c) else {
                    continue;
                };
                for out in 0..self.profile.output_pair_count() {
                    let level = self.playbacks[c].volumes[out];
                    self.write_crosspoint(src, ch, false, out, level, 0)?;
                }
            }
        }
        Ok(())
    }

    /// Refused: the driver's "<name> Trim Volume" wrote the trim as fixed
    /// levels into the pair's Main cells (both sides, crossed ones too),
    /// not a level before the mixer, and behind its own cache: moving it
    /// back to 0 put both mics in Main at 0 dB (2026-11-09). TotalMix's
    /// Trim is Trim Gains, which Audra does with the faders.
    fn set_trim(&mut self, idx: usize, _db: f32) -> Result<(), Error> {
        Err(Error::InvalidChannel(format!(
            "Input {idx} has no trim before the mixer on the Babyface: use Trim Gains"
        )))
    }

    fn set_dim(&mut self, on: bool) -> Result<(), Error> {
        self.require_proprietary("Dim")?;
        if let Some(selem) = self.mixer.find_selem("Dim", 0) {
            selem.set_playback_switch(SelemChannelId::mono(), on as i32)?;
        }
        self.settings.dim = on;
        Ok(())
    }

    // No `set_width`: the driver's "Width" wrote AN1/2's Main and Phones
    // cells at fixed levels, whatever their faders said, and behind its
    // own cache (2026-11-09). The trait default refuses it.

    fn has_fx(&self) -> bool {
        self.grammar != ControlGrammar::ClassCompliant && self.mixer.find_selem("FX Return", 0).is_some()
    }

    fn can_output_proc(&self) -> bool {
        self.raw_crosspoint && self.grammar == ControlGrammar::Proprietary
    }

    fn can_output_eq(&self, ch: usize) -> bool {
        self.dsp_block && ch < 6
    }

    fn can_source_ms(&self, channel: ChannelId) -> bool {
        self.can_output_proc() && (matches!(channel, ChannelId::Input(0 | 1)) || crate::device::source_send_pair(channel).is_some())
    }

    fn set_source_ms(&mut self, channel: ChannelId, on: bool) -> Result<(), Error> {
        if let ChannelId::Input(0 | 1) = channel {
            return self.set_ms_proc(on);
        }
        let pair = crate::device::source_send_pair(channel)
            .filter(|_| self.can_output_proc())
            .ok_or_else(|| Error::InvalidChannel("M/S needs the Babyface driver from 2026-11-09".into()))?;
        if on {
            self.settings.source_ms.insert(pair.to_string(), true);
        } else {
            self.settings.source_ms.remove(pair);
        }
        // Every output, its cells written even with M/S now off (the
        // driver's own values back).
        let src = match channel {
            ChannelId::Input(i) => input_crosspoint_slot(i).map(|s| s.0),
            ChannelId::Playback(i) => playback_crosspoint_slot(i).map(|s| s.0),
            ChannelId::Output(_) => None,
        };
        for out in 0..self.profile.output_pair_count() {
            self.reassert_output(out, src, true)?;
        }
        Ok(())
    }

    fn set_output_eq(&mut self, ch: usize, eq: InputEq) -> Result<(), Error> {
        if !self.can_output_eq(ch) {
            return Err(Error::InvalidChannel(format!("Output {ch} has no EQ here (the Babyface driver from 2026-11-09 has Main, Phones and AS 1/2)")));
        }
        self.settings.keep_output_eq(ch, eq);
        self.upload_eq(crate::eq_block::EqChannel::Output(ch))
    }

    fn set_output_phase(&mut self, ch: usize, invert: bool) -> Result<(), Error> {
        if !self.can_output_proc() {
            return Err(Error::InvalidChannel("Output phase needs the Babyface driver from 2026-11-09".into()));
        }
        if ch >= self.profile.output_pair_count() * 2 {
            return Err(Error::InvalidChannel(format!("Output {ch}")));
        }
        self.settings.keep_output_phase(ch, invert);
        self.reassert_output(ch / 2, None, true)
    }

    fn set_output_ms(&mut self, pair: usize, on: bool) -> Result<(), Error> {
        if !self.can_output_proc() {
            return Err(Error::InvalidChannel("Output M/S needs the Babyface driver from 2026-11-09".into()));
        }
        if pair >= self.profile.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output pair {pair}")));
        }
        self.settings.keep_output_ms(pair, on);
        self.reassert_output(pair, None, true)
    }

    fn has_source_fx_send(&self, channel: ChannelId) -> bool {
        self.source_send_selem(channel).is_some()
    }

    fn set_source_fx_send(&mut self, channel: ChannelId, db: f32) -> Result<(), Error> {
        let db = db.clamp(-65.0, 0.0);
        let Some(selem) = self.source_send_selem(channel) else {
            return Err(Error::InvalidChannel("This FX send needs the driver from 2026-11-09".into()));
        };
        selem.set_playback_volume(SelemChannelId::mono(), send_db_to_raw(db) as i64)?;
        if let Some(pair) = crate::device::source_send_pair(channel) {
            self.settings.source_fx_send.insert(pair.to_string(), db);
        }
        Ok(())
    }

    fn has_fx_engine(&self) -> bool {
        self.mixer.find_selem("FX Engine", 0).is_some()
    }

    fn set_fx_engine(&mut self, on: bool) -> Result<(), Error> {
        // The driver's "FX Engine Switch" (2026-11-09); an older driver
        // has none, and nothing changes.
        let Some(selem) = self.mixer.find_selem("FX Engine", 0) else {
            return Ok(());
        };
        let mono = SelemChannelId::mono();
        if selem.get_playback_switch(mono).is_ok_and(|v| (v != 0) == on) {
            return Ok(());
        }
        if on {
            self.clear_fx_feedback();
        }
        selem.set_playback_switch(mono, on as i32)?;
        Ok(())
    }

    fn set_fx_return(&mut self, pair: usize, volume: f32) -> Result<(), Error> {
        self.require_proprietary("FX return")?;
        let Some(selem) = self.mixer.find_selem("FX Return", pair as u32) else {
            return Err(Error::InvalidChannel(
                "FX return needs the driver that opens the FX channels".into(),
            ));
        };
        // On TotalMix's scale, as every crosspoint (issue #13).
        let v = volume.clamp(0.0, 2.0);
        selem.set_playback_volume(SelemChannelId::FrontLeft, xp_raw(v))?;
        selem.set_playback_volume(SelemChannelId::FrontRight, xp_raw(v))?;
        self.settings.keep_fx_return(pair, v);
        self.reassert_output(pair, Some(14), false)
    }

    fn set_fx_send(&mut self, db: f32) -> Result<(), Error> {
        self.require_proprietary("FX send")?;
        let db = db.clamp(-65.0, 0.0);
        self.write_fx_send(crate::device::fx_send_on_unit(db, self.fx_send_trim_db))?;
        self.settings.fx_send_db = Some(db);
        Ok(())
    }

    fn set_fx_send_trim(&mut self, trim_db: f32) -> Result<(), Error> {
        self.fx_send_trim_db = trim_db;
        match self.settings.fx_send_db {
            Some(db) if self.grammar != ControlGrammar::ClassCompliant => self.write_fx_send(crate::device::fx_send_on_unit(db, trim_db)),
            _ => Ok(()),
        }
    }

    fn set_optical_out_format(&mut self, spdif: bool) -> Result<(), Error> {
        self.require_proprietary("optical out format")?;
        let wanted = if spdif { "S/PDIF" } else { "ADAT" };
        let selem = self
            .mixer
            .find_selem("Optical Out Format", 0)
            .ok_or_else(|| Error::InvalidChannel("No Optical Out Format control".into()))?;
        let count = selem.get_enum_items()?;
        let item = (0..count)
            .find(|i| selem.get_enum_item_name(*i).is_ok_and(|n| n == wanted))
            .ok_or_else(|| {
                Error::InvalidChannel(format!("Optical Out Format has no {wanted} setting"))
            })?;
        selem.set_enum_item(SelemChannelId::mono(), item)?;
        self.settings.optical_out_spdif = spdif;
        Ok(())
    }

    fn set_clock_source(&mut self, source: &str) -> Result<(), Error> {
        let selem = self
            .mixer
            .find_selem("Sample Clock Source", 0)
            .ok_or_else(|| Error::InvalidChannel("No clock source control".into()))?;
        let count = selem.get_enum_items().unwrap_or(0);
        for idx in 0..count {
            if selem
                .get_enum_item_name(idx)
                .map(|n| n == source)
                .unwrap_or(false)
            {
                selem.set_enum_item(SelemChannelId::mono(), idx)?;
                self.settings.clock_source = source.to_string();
                return Ok(());
            }
        }
        Err(Error::InvalidChannel(format!(
            "Unknown clock source: {}",
            source
        )))
    }

    fn capture_scene(&self) -> Scene {
        Scene {
            name: "Untitled".into(),
            model: self.canonical_model().to_string(),
            inputs: self.inputs.clone(),
            playbacks: self.playbacks.clone(),
            outputs: self.outputs.clone(),
            settings: self.settings.clone(),
            level_scale: Scene::LEVEL_SCALE,
        }
    }

    fn apply_scene(&mut self, scene: &Scene) -> Result<(), Error> {
        scene.check_compatible(self.canonical_model())?;
        // Another number of channels would replace this unit's lists
        // (OpenCode's job 50: the quit's sync and USB's start came this way).
        scene.check_shape(self.inputs.len(), self.playbacks.len(), self.outputs.len())?;
        scene.check_lists(&self.inputs, &self.playbacks)?;
        let proc_before = (self.settings.output_phase.clone(), self.settings.output_ms.clone());
        let src_ms_before = !self.settings.source_ms.is_empty();
        let width_before = !self.settings.source_width.is_empty();
        // The scene's mutes and solos first, so its levels go to the card
        // through them: written before, every muted strip played at its
        // level until the mutes landed (OpenCode's job 8).
        for (c, saved) in self.inputs.iter_mut().zip(&scene.inputs) {
            c.mute = saved.mute;
            c.solo = saved.solo;
        }
        for (c, saved) in self.playbacks.iter_mut().zip(&scene.playbacks) {
            c.mute = saved.mute;
            c.solo = saved.solo;
        }
        for (i, saved) in scene.inputs.iter().enumerate() {
            for (out, &v) in saved.volumes.iter().enumerate() {
                self.set_volume(ChannelId::Input(i), out, v)?;
            }
        }
        for (i, saved) in scene.playbacks.iter().enumerate() {
            for (out, &v) in saved.volumes.iter().enumerate() {
                self.set_volume(ChannelId::Playback(i), out, v)?;
            }
        }
        for (i, saved) in scene.outputs.iter().enumerate() {
            self.set_volume(ChannelId::Output(i), 0, saved.volume)?;
        }
        let split_before: Vec<bool> = self.playbacks.iter().map(|p| p.split).collect();
        // The preamps: the scene's gains are written (they only reached the
        // model, so an Undo of a gain turn, or a snapshot's gains, never
        // reached the unit: OpenCode's job 40), and 48V and PAD only the
        // safe way (`Scene::safe_preamps`).
        let mut inputs = scene.inputs.clone();
        Scene::safe_preamps(&mut inputs, &self.inputs);
        let now = self.inputs.clone();
        for (i, (s, n)) in inputs.iter().zip(&now).enumerate() {
            // The safe changes first (48V off, PAD on: quieter), then the
            // gain, on the inputs that have one (the local Qwen's review: a
            // line input's stale gain could stop the scene part way).
            if n.channel_type == ChannelType::Mic {
                if s.phantom != n.phantom {
                    self.set_phantom(i, s.phantom)?;
                }
                if s.pad != n.pad {
                    self.set_pad(i, s.pad)?;
                }
            }
            if matches!(n.channel_type, ChannelType::Mic | ChannelType::Instrument) {
                if let (Some(g), Some(c)) = (s.gain, n.gain) {
                    if g != c {
                        self.set_gain(i, g)?;
                    }
                }
            }
        }
        self.inputs = inputs;
        self.playbacks = scene.playbacks.clone();
        let mut outputs = scene.outputs.clone();
        Scene::safe_outputs(&mut outputs, &self.outputs);
        self.outputs = outputs;
        // A pair's balance on both its channels, as this backend keeps it
        // (a scene from the USB backend has it on the left one only).
        for pair in 0..self.playbacks.len() / 2 {
            if !self.playbacks[pair * 2].split {
                let pans = self.playbacks[pair * 2].pans.clone();
                self.playbacks[pair * 2 + 1].pans = pans;
            }
        }
        // The split switch is the card's, and a re-read takes the flag
        // from it: a scene that splits or joins a pair (an Undo of Stereo
        // pair off) writes it, or the next re-read would split the pair
        // back and take its lowered side for its fader (the reviewer).
        // Best effort: the class-compliant card has no such switch.
        for pair in 0..self.playbacks.len() / 2 {
            let split = self.playbacks[pair * 2].split;
            if split_before.get(pair * 2).copied() != Some(split) {
                let _ = self.set_stereo_split(pair * 2, split);
            }
        }
        let fx_return_now = std::mem::take(&mut self.settings.fx_return);
        let mut settings = scene.settings.clone();
        Scene::safe_settings(&mut settings, &self.settings);
        self.settings = settings;
        // Re-issue the actual ALSA writes for mute/solo/global toggles —
        // previously this only ever updated the in-memory model above
        // (`self.inputs`/etc.), so loading a scene looked right in the
        // GUI but never touched real hardware for anything except
        // volume. `set_mute`/`set_solo` read `self.inputs`/`self.
        // playbacks`/`self.outputs` to decide what to restore, so this
        // runs after the state's been adopted, not before.
        for i in 0..self.inputs.len() {
            self.set_mute(ChannelId::Input(i), scene.inputs[i].mute)?;
            self.set_solo(ChannelId::Input(i), scene.inputs[i].solo)?;
        }
        for i in 0..self.playbacks.len() {
            self.set_mute(ChannelId::Playback(i), scene.playbacks[i].mute)?;
            self.set_solo(ChannelId::Playback(i), scene.playbacks[i].solo)?;
        }
        for pair in 0..self.profile.output_pair_count() {
            if let Some(out) = scene.outputs.get(pair * 2) {
                self.set_mute(ChannelId::Output(pair * 2), out.mute)?;
                // LOOP as adopted (`Scene::safe_outputs`: only ever off),
                // not as the file had it.
                let lb = self.outputs.get(pair * 2).is_some_and(|o| o.loopback);
                let _ = self.set_loopback(pair, lb);
            }
        }
        // Best-effort: `ms_proc`/`an12`/`width`/`fx_send`/`clock_source`
        // are all real ALSA controls on this backend, but some (clock
        // source especially — see `PROTOCOL.md`/this session's own
        // finding that it's a CC-mode-only control surface under the
        // proprietary driver) may legitimately not exist depending on
        // which kernel driver mode is loaded. A missing optional control
        // shouldn't abort the rest of the scene load.
        let _ = self.set_ms_proc(self.settings.ms_proc);
        let _ = self.set_an12(self.settings.an12);
        if let Some(db) = self.settings.fx_send_db {
            let _ = self.set_fx_send(db);
        }
        for ch in [2usize, 4, 6, 8, 10].map(ChannelId::Input).into_iter().chain((0..12).step_by(2).map(ChannelId::Playback)) {
            if let Some(db) = crate::device::source_send_pair(ch).and_then(|p| self.settings.source_fx_send.get(p).copied()) {
                let _ = self.set_source_fx_send(ch, db);
            }
        }
        // A scene from before the FX return (none saved) leaves the
        // returns as they are, as an old scene leaves the send.
        if scene.settings.fx_return.is_empty() {
            self.settings.fx_return = fx_return_now;
        } else {
            for pair in 0..self.profile.output_pair_count() {
                let _ = self.set_fx_return(pair, self.settings.fx_return_of(pair));
            }
        }
        if !self.settings.clock_source.is_empty() {
            let _ = self.set_clock_source(&self.settings.clock_source.clone());
        }
        // The levels went to the unit under the widths from before the
        // scene: every channel again under the scene's (#7).
        if width_before || !self.settings.source_width.is_empty() {
            for o in 0..self.profile.output_pair_count() {
                for j in 0..self.inputs.len() {
                    self.rewrite_channel(ChannelId::Input(j), o)?;
                }
                for j in 0..self.playbacks.len() {
                    self.rewrite_channel(ChannelId::Playback(j), o)?;
                }
            }
        }
        // Output phase and M/S as the scene has them: every pair that had
        // or has some, so one switched off gets the driver's values back.
        for pair in 0..self.profile.output_pair_count() {
            let had = proc_before.0.get(pair * 2).copied().unwrap_or(false)
                || proc_before.0.get(pair * 2 + 1).copied().unwrap_or(false)
                || proc_before.1.get(pair).copied().unwrap_or(false)
                // A source's M/S, then or now: every cell again.
                || src_ms_before
                || !self.settings.source_ms.is_empty();
            let _ = self.reassert_output(pair, None, had);
        }
        self.resend_eq();
        Ok(())
    }

    fn set_app_handles_dim_button(&mut self, on: bool) {
        self.app_handles_dim = on;
        self.dim_toggle_pending = false;
    }

    fn take_dim_button_toggle(&mut self) -> bool {
        std::mem::take(&mut self.dim_toggle_pending)
    }

    fn hold_card(&mut self, on: bool) {
        self.card_held = on;
    }

    fn write_crosspoint_raw(
        &mut self,
        channel: ChannelId,
        output: usize,
        volume: f32,
        pan: i8,
    ) -> Result<(), Error> {
        if output >= self.profile.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output {}", output)));
        }
        let (src, ch, is_mono) = match channel {
            ChannelId::Input(idx) => input_crosspoint_slot(idx)
                .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?,
            ChannelId::Playback(idx) => {
                let (src, ch) = playback_crosspoint_slot(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx)))?;
                (src, ch, false)
            }
            ChannelId::Output(idx) => {
                return Err(Error::InvalidChannel(format!("Output {idx} has no crosspoint")))
            }
        };
        // Same ranges `set_volume` writes: the driver's crosspoints reach
        // +6 dB (2.0), the class-compliant ones 0 dB.
        let vol = if self.grammar == ControlGrammar::ClassCompliant {
            volume.clamp(0.0, 1.0)
        } else {
            volume.clamp(0.0, 2.0)
        };
        // A playback channel's `pan` is its pair's balance: its own side.
        let vol = match channel {
            ChannelId::Playback(idx) if !self.playbacks[idx].split => playback_side(idx, vol, pan),
            _ => vol,
        };
        self.write_crosspoint(src, ch, is_mono, output, vol, pan)
    }

    fn restore_auto_scene(&mut self, scene: &Scene) -> Result<(), Error> {
        scene.check_compatible(self.canonical_model())?;
        // The hardware already told us every level when it opened, so only
        // the names, which live in the app, are restored from the saved
        // snapshot. Re-applying the levels would move the unit back to a
        // stale state.
        for (mine, saved) in self.inputs.iter_mut().zip(&scene.inputs) {
            mine.name = saved.name.clone();
        }
        for (mine, saved) in self.playbacks.iter_mut().zip(&scene.playbacks) {
            mine.name = saved.name.clone();
        }
        for (mine, saved) in self.outputs.iter_mut().zip(&scene.outputs) {
            mine.name = saved.name.clone();
        }
        // What only Audra holds comes back with it, and goes to the unit
        // again: output phase, M/S and EQ, and the input EQ once Audra
        // computes it.
        let s = &scene.settings;
        self.settings.output_phase = s.output_phase.clone();
        self.settings.output_ms = s.output_ms.clone();
        self.settings.output_eq = s.output_eq.clone();
        self.settings.source_ms = s.source_ms.clone();
        // Widths (#7). A stereo source read right at open (own + crossed
        // is its fader whatever the width), but a linked AN pair's pans
        // came from the blended sides: where the unit holds what the saved
        // scene would write, the scene's level and pan are the real ones
        // (near mono the sides no longer tell the pan); otherwise the
        // sides, unblended.
        self.settings.source_width = s.source_width.clone();
        for i in 0..4.min(self.inputs.len()) {
            let Some((src, ch, true)) = input_crosspoint_slot(i) else {
                continue;
            };
            if self.source_width(src) >= 1.0 {
                continue;
            }
            let Some(saved) = scene.inputs.get(i) else {
                continue;
            };
            for out in 0..self.profile.output_pair_count() {
                let (sv, sp) = (saved.volumes.get(out).copied().unwrap_or(0.0), saved.pans.get(out).copied().unwrap_or(0));
                if self.crosspoint_matches(src, ch, true, out, sv, sp, sv) {
                    self.inputs[i].volumes[out] = sv;
                    self.inputs[i].pans[out] = sp;
                } else if let Some((v, p)) = self.read_crosspoint(src, ch, true, out) {
                    if v > 0.0 {
                        self.inputs[i].volumes[out] = v;
                        self.inputs[i].pans[out] = p;
                    }
                }
            }
        }
        if self.dsp_block {
            for (mine, saved) in self.inputs.iter_mut().zip(&scene.inputs).take(4) {
                if saved.eq.is_some() {
                    mine.eq = saved.eq;
                }
            }
        }
        // Every output's cells once, processing on or off: after a crash
        // the unit can still hold what is now off (the M/S key's check,
        // a phase switched off unsaved).
        for pair in 0..self.profile.output_pair_count() {
            let _ = self.reassert_output(pair, None, true);
        }
        self.resend_eq();
        Ok(())
    }

    fn poll_events(&mut self) -> Result<(), Error> {
        let now = std::time::Instant::now();
        // See RESYNC_POLL: host-side writes do not notify, and the first
        // read catches udev's `alsactl restore` after the card appears.
        let poll_due = self.resync_poll_at.is_some_and(|t| now >= t);
        if poll_due {
            self.resync_poll_at = Some(now + RESYNC_POLL);
        }
        if self.mixer.handle_events()? > 0 {
            self.attach_mixer_elements(true);
            self.resync_at = Some(now + RESYNC_SETTLE);
        } else if poll_due || self.resync_at.is_some_and(|t| now >= t) {
            self.resync_at = None;
            self.attach_mixer_elements(true);
            // The driver's own writes (the front-panel wheel, a restore,
            // a resume) put cells back positive: the output processing
            // again, once each burst has settled; and the EQ, which a
            // resume or a rate change undoes.
            self.reassert_outputs(None);
            self.resend_eq();
            // A resume can bring the unit's power-on send block back.
            self.clear_fx_feedback();
        }
        if self.gain_poll_at.is_none_or(|t| now >= t) {
            self.gain_poll_at = Some(now + GAIN_POLL);
            self.refresh_preamp_gains();
        }
        Ok(())
    }

    /// Real for every hardware input (AN1-AN4, AS1/2, ADAT3-8) via
    /// `capture_meter`; the GUI gates a failed capture open back to
    /// "N/A" through `DeviceHandle::has_input_meter`. `None` when the
    /// capture PCM could not be opened at all, or while the reader thread
    /// is reopening it after a PipeWire profile change, matching the
    /// trait's own contract. See `capture_meter`'s module doc comment for
    /// the live mapping behind `INPUT_FOR_CAPTURE`.
    fn meters(&self) -> Option<Vec<f32>> {
        // The tap reads up to 14 channels (the FX send past the 12
        // inputs), the direct capture 12; the inputs are the first 12.
        let peaks: Vec<f32> = if let Some(tap) = &self.input_tap {
            let (peaks, channels) = tap.drain();
            if channels == 0 {
                self.fx_in.set(None);
                return None;
            }
            self.fx_in.set((channels >= 14).then(|| [peaks[12], peaks[13]]));
            peaks.to_vec()
        } else {
            let cm = self.capture_meter.as_ref()?;
            if !cm.is_connected() {
                return None;
            }
            let peaks = cm.drain();
            crate::capture_meter::CaptureMeter::debug_log(&peaks);
            peaks.to_vec()
        };
        let mut levels = vec![0.0; self.inputs.len()];
        for (cap, input) in peaks.iter().zip(crate::capture_meter::INPUT_FOR_CAPTURE) {
            if let Some(i) = input {
                if let Some(slot) = levels.get_mut(i) {
                    *slot = *cap;
                }
            }
        }
        Some(levels)
    }

    fn fx_meters(&self) -> Option<[f32; 2]> {
        self.fx_in.get()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn mono_folds_both_sides_to_their_mean_as_totalmix_writes_it() {
        // The capture: PB1 at 0 dB into Main L only became -6 dB on both
        // sides (own and crossed).
        assert_eq!(stereo_cells(1.0, 1.0, true), (0.5, 0.5));
        assert_eq!(stereo_cells(1.0, 1.0, false), (1.0, 0.0));
        // A single channel panned hard left: both sides at half.
        assert_eq!(mono_sides((1.0, 0.0), true), (0.5, 0.5));
        assert_eq!(mono_sides((0.7, 0.3), false), (0.7, 0.3));
    }

    #[test]
    fn width_writes_totalmixs_ladder_and_keeps_the_fader_in_the_sum() {
        // PROTOCOL.md "Width strip mapping": at 0 dB the own cell steps
        // 0x1000 -> 0x1029 -> 0x1052 and the crossed one 0x1000 -> 0x0fd7 ->
        // 0x0fae as the width goes 0.00 -> 0.01 -> 0.02.
        for (w, own, cross) in [(0.0, 0x1000, 0x1000), (0.01, 0x1029, 0x0fd7), (0.02, 0x1052, 0x0fae)] {
            let (o, c) = stereo_cells(1.0, w, false);
            assert!((xp_raw(o) - own).abs() <= 1 && (xp_raw(c) - cross).abs() <= 1, "w {w}: {o} {c}");
        }
        // 1 as recorded, -1 swapped; own + crossed is the fader whatever
        // the width, Mono or not (what a re-read takes for the level).
        assert_eq!(stereo_cells(0.8, 1.0, false), (0.8, 0.0));
        assert_eq!(stereo_cells(0.8, -1.0, false), (0.0, 0.8));
        for w in [-1.0, -0.4, 0.0, 0.3, 1.0] {
            for mono in [false, true] {
                let (o, c) = stereo_cells(0.6, w, mono);
                assert!((o + c - 0.6).abs() < 1e-6);
            }
        }
        // Mono over any width is the same (L+R)/2.
        assert_eq!(stereo_cells(1.0, 0.3, true), stereo_cells(1.0, 1.0, true));
    }

    #[test]
    fn a_linked_an_pair_unblends_back_to_its_pans_except_near_mono() {
        // AN1 hard left at 0 dB, width 0.5: three quarters left, a quarter
        // right; undone, hard left again.
        let blended = width_blend(pan_split_power(1.0, -100), 0.5);
        assert!((blended.0 - 0.75).abs() < 1e-6 && (blended.1 - 0.25).abs() < 1e-6);
        let (l, r) = width_unblend(blended, 0.5).unwrap();
        assert!((l - 1.0).abs() < 1e-5 && r.abs() < 1e-5);
        assert_eq!(pan_join_power(l, r).1, -100);
        // Swapped (-1) undoes too; near mono it can't.
        let (l, r) = width_unblend(width_blend((0.9, 0.1), -1.0), -1.0).unwrap();
        assert!((l - 0.9).abs() < 1e-5 && (r - 0.1).abs() < 1e-5);
        assert!(width_unblend((0.5, 0.5), 0.05).is_none());
        assert_eq!(width_unblend((0.3, 0.2), 1.0), Some((0.3, 0.2)));
    }

    #[test]
    fn totalmix_scale_writes_what_the_capture_shows_and_reads_back_still() {
        // Windows session 3 (#13): a mono source at 0 dB centred writes
        // 0x16A0 on both sides; hard left 0x2000 and nothing; -6 dB centred
        // 0x0B57; a stereo source at 0 dB 0x2000.
        let mono = |v: f32, p: i8| {
            let (l, r) = pan_split_power(v, p);
            (xp_raw(l), xp_raw(r))
        };
        // Within a step of the capture (TotalMix rounds down).
        let (l, r) = mono(1.0, 0);
        assert!((l - 0x16A0).abs() <= 1 && l == r);
        assert!((xp_raw(10f32.powf(-6.0 / 20.0)) - 0x1009).abs() <= 1, "a stereo source at -6 dB");
        assert_eq!(mono(1.0, -100), (0x2000, 0));
        assert_eq!(mono(1.0, 100), (0, 0x2000));
        assert!((mono(10f32.powf(-6.0 / 20.0), 0).0 - 0x0B57).abs() <= 1);
        assert_eq!(xp_raw(1.0), 0x2000);
        assert_eq!(xp_raw(2.0), 0x4000, "+6 dB, the driver's top");
        // Read back and written again, every cell stays where it was (the
        // 2-second re-read must never walk a fader).
        for &v in &[0.01f32, 0.1, 0.25, 0.5, 0.708, 1.0, 1.41, 2.0] {
            for &p in &[-100i8, -73, -37, -1, 0, 1, 50, 99, 100] {
                let (l, r) = mono(v, p);
                let (v2, p2) = pan_join_power(xp_volume(l), xp_volume(r));
                let (l2, r2) = mono(v2, p2);
                assert!((l2 - l).abs() <= 1 && (r2 - r).abs() <= 1, "mono {v} {p}: {l},{r} -> {l2},{r2}");
                let (v3, p3) = pan_join_power(xp_volume(l2), xp_volume(r2));
                assert_eq!(mono(v3, p3), (l2, r2), "a fixed point after one round: {v} {p}");
            }
            assert_eq!(xp_raw(xp_volume(xp_raw(v))), xp_raw(v), "stereo {v}");
        }
    }

    #[test]
    fn pan_split_and_join_round_trip_in_linear_levels() {
        // The dB-converted crosspoints pan on linear levels, louder side
        // at the volume: centre, both extremes and between, up to +6 dB.
        for &volume in &[0.25f32, 1.0, 1.5, 2.0] {
            for &pan in &[-100i8, -37, 0, 1, 64, 100] {
                let (l, r) = pan_split(volume, pan);
                assert!((l.max(r) - volume).abs() < 1e-6);
                let (v, p) = pan_join(l, r);
                assert!((v - volume).abs() < 1e-6);
                assert_eq!(p, pan, "volume {volume} pan {pan}");
            }
        }
        assert_eq!(pan_join(0.0, 0.0), (0.0, 0));
    }

    #[test]
    fn a_playback_pairs_balance_lowers_the_far_side_and_reads_back() {
        // Centre: both sides at the fader.
        assert_eq!(playback_side(0, 1.0, 0), 1.0);
        assert_eq!(playback_side(1, 1.0, 0), 1.0);
        // To the right the left side drops, the right stays (usb.rs's law).
        assert!((playback_side(0, 1.0, 50) - 0.5).abs() < 1e-6);
        assert_eq!(playback_side(1, 1.0, 50), 1.0);
        assert_eq!(playback_side(2, 0.8, -100), 0.8);
        assert_eq!(playback_side(3, 0.8, -100), 0.0);
        // The two sides read back as the fader and the balance.
        for &v in &[0.3f32, 1.0, 2.0] {
            for &p in &[-100i8, -40, 0, 25, 100] {
                let (fader, balance) = pan_join(playback_side(4, v, p), playback_side(5, v, p));
                assert!((fader - v).abs() < 1e-6);
                assert_eq!(balance, p, "volume {v} balance {p}");
            }
        }
    }
    use super::*;

    const TEST_MAX: f32 = 65536.0;

    #[test]
    fn dim_toggles_on_an_odd_number_of_presses_only() {
        assert!(!dim_presses_toggle(5, 5));
        assert!(dim_presses_toggle(5, 6));
        assert!(!dim_presses_toggle(5, 7));
        assert!(dim_presses_toggle(5, 8));
    }

    #[test]
    fn dim_press_count_wraps_at_two_to_the_31() {
        let top = (1i64 << 31) - 1;
        // top -> 0 is one press, top -> 1 two.
        assert!(dim_presses_toggle(top, 0));
        assert!(!dim_presses_toggle(top, 1));
    }

    #[test]
    fn panel_enum_maps_to_the_usb_backend_selection() {
        assert_eq!(panel_enum_to_sel(0), None); // not known yet
        assert_eq!(panel_enum_to_sel(1), Some(0)); // Ch 1/2
        assert_eq!(panel_enum_to_sel(2), Some(1)); // Ch 3/4 / Phones
        assert_eq!(panel_enum_to_sel(3), Some(2)); // Opt
        assert_eq!(panel_enum_to_sel(4), None);
    }

    #[test]
    fn resync_ignores_rounding_but_not_a_real_change() {
        // A crosspoint step (full scale 0x2d41) is below the threshold...
        assert!(!resync_differs(0.5, 0.5 + 1.0 / 11585.0));
        // ...a wheel click (0.5 dB, about 6 %) is well above it.
        assert!(resync_differs(0.5, 0.53));
        assert!(resync_differs(0.0, 0.01));
    }

    #[test]
    fn decode_center_pan_from_equal_raw_volumes() {
        let (volume, pan) = decode_volume_pan(32768, 32768, TEST_MAX);
        assert!((volume - 0.5).abs() < 1e-4);
        assert_eq!(pan, 0);
    }

    #[test]
    fn decode_hard_left_from_silent_right() {
        let (volume, pan) = decode_volume_pan(65536, 0, TEST_MAX);
        assert!((volume - 1.0).abs() < 1e-4);
        assert_eq!(pan, -100);
    }

    #[test]
    fn decode_hard_right_from_silent_left() {
        let (volume, pan) = decode_volume_pan(0, 65536, TEST_MAX);
        assert!((volume - 1.0).abs() < 1e-4);
        assert_eq!(pan, 100);
    }

    #[test]
    fn decode_silence_defaults_to_center_pan() {
        let (volume, pan) = decode_volume_pan(0, 0, TEST_MAX);
        assert_eq!(volume, 0.0);
        assert_eq!(pan, 0);
    }

    #[test]
    fn encode_then_decode_round_trips_for_a_range_of_volume_pan_pairs() {
        for vol_pct in [0, 10, 25, 50, 75, 100] {
            for pan in [-100, -50, -25, 0, 25, 50, 100] {
                let volume = vol_pct as f32 / 100.0;
                let (l, r) = encode_volume_pan(volume, pan, TEST_MAX);
                let (decoded_vol, decoded_pan) = decode_volume_pan(l, r, TEST_MAX);
                assert!(
                    (decoded_vol - volume).abs() < 0.01,
                    "volume {} pan {}: decoded volume {}",
                    volume,
                    pan,
                    decoded_vol
                );
                // At volume 0 every raw pair is (0, 0), so pan is
                // undecidable — decode_volume_pan defaults it to 0
                // regardless of what was encoded.
                if vol_pct > 0 {
                    assert!(
                        (decoded_pan - pan).abs() <= 1,
                        "volume {} pan {}: decoded pan {}",
                        volume,
                        pan,
                        decoded_pan
                    );
                }
            }
        }
    }

    /// The identity probe's failure modes: a name that cannot be a card
    /// and an index no card occupies both resolve to no `devnum`, which
    /// `is_connected` reads as "not ours".
    #[test]
    fn card_devnum_is_none_for_a_missing_card() {
        assert!(AlsaMixer::card_devnum("nonsense").is_none());
        assert!(AlsaMixer::card_devnum("hw:99").is_none());
    }

    /// The "card is gone" half of the ALSA liveness probe: a name nothing
    /// matches must resolve to nothing, which is what makes the chip show
    /// Disconnected. (The "card is there" half needs the real card and is
    /// covered live.)
    #[test]
    fn resolve_card_finds_nothing_for_a_missing_name() {
        assert!(AlsaMixer::resolve_card("TuxMix No Such Card").is_none());
    }

    #[test]
    fn encode_center_pan_gives_equal_left_and_right() {
        let (l, r) = encode_volume_pan(0.75, 0, TEST_MAX);
        assert_eq!(l, r);
    }

    /// A host-side write must reach the model without an ALSA event. The
    /// driver's `bf_xpoint_put()` does not notify, which is also what kept
    /// udev's `alsactl restore` invisible at startup (#23); the periodic
    /// `RESYNC_POLL` read is what picks both up.
    ///
    /// Writes a PB1 crosspoint into an output pair whose master is fully
    /// down, so the change cannot be heard, and restores it before
    /// asserting. Run it with one pair muted; otherwise it skips.
    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_poll_picks_up_a_host_write() {
        let mut dev = BabyfacePro::open().expect("real device attached");

        if dev.playbacks[0].mute
            || dev.inputs.iter().any(|c| c.solo)
            || dev.playbacks.iter().any(|c| c.solo)
        {
            eprintln!("PB1 is muted or a solo is active; skipping");
            return;
        }

        // An output pair whose master is at the mute end of its range.
        let silent_pair = (0..dev.profile.output_pair_count()).find(|&pair| {
            dev.mixer
                .find_selem(PAIR_LABELS[pair], pair as u32)
                .is_some_and(|s| {
                    s.get_playback_volume(SelemChannelId::FrontLeft).ok() == Some(0)
                        && s.get_playback_volume(SelemChannelId::FrontRight).ok() == Some(0)
                })
        });
        let Some(pair) = silent_pair else {
            eprintln!("no output pair is muted; mute one pair and rerun");
            return;
        };

        // PB1 is playback channel 0: source 8, left ALSA channel.
        const SRC: usize = 8;
        let ch = SelemChannelId::FrontLeft;
        let original = dev.playbacks[0].volumes[pair];

        // The `Selem` borrows the mixer, so it cannot stay alive across
        // `poll_events()`. Do the write and the read-back in one block.
        let (orig_raw, expected) = {
            let selem = dev.crosspoint_selem(SRC, pair).expect("PB1 crosspoint");
            let orig_raw = selem.get_playback_volume(ch).expect("read crosspoint");
            let target_raw = if orig_raw == 0x16a0 { 0x1000 } else { 0x16a0 };
            selem
                .set_playback_volume(ch, target_raw)
                .expect("write crosspoint");
            (
                orig_raw,
                tlv_volume(&selem, ch).expect("read back crosspoint"),
            )
        };
        assert!(
            resync_differs(original, expected),
            "the target reads the same as the model, pick another"
        );

        // No event is generated by the write, so only the poll can see it.
        std::thread::sleep(RESYNC_POLL + std::time::Duration::from_millis(500));
        dev.poll_events().expect("poll events");
        let observed = dev.playbacks[0].volumes[pair];

        dev.crosspoint_selem(SRC, pair)
            .expect("PB1 crosspoint")
            .set_playback_volume(ch, orig_raw)
            .expect("restore crosspoint");

        assert!(
            (observed - expected).abs() <= RESYNC_EPSILON,
            "model {observed} did not follow the card {expected}"
        );
    }

    /// One step of output processing on the Phones (pair 1), for a
    /// loopback measurement between steps: `AUDRA_PROC_STEP` is
    /// phase-on (ØL), phase-off, ms-on or ms-off.
    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_output_proc_step() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        assert!(dev.can_output_proc(), "the driver has no Raw Crosspoint");
        match std::env::var("AUDRA_PROC_STEP").as_deref() {
            Ok("phase-on") => dev.set_output_phase(2, true).unwrap(),
            Ok("phase-off") => dev.set_output_phase(2, false).unwrap(),
            Ok("ms-on") => dev.set_output_ms(1, true).unwrap(),
            Ok("ms-off") => dev.set_output_ms(1, false).unwrap(),
            // TotalMix's scale (#13): PB1 into the Phones at 0 dB; AN1 at
            // 0 dB centred, then hard left; "scale-read" prints the model.
            Ok("scale-pb1") => dev.set_volume(ChannelId::Playback(0), 1, 1.0).unwrap(),
            Ok("scale-an1-c") => {
                dev.set_pan(ChannelId::Input(0), 1, 0).unwrap();
                dev.set_volume(ChannelId::Input(0), 1, 1.0).unwrap();
            }
            Ok("scale-an1-l") => dev.set_pan(ChannelId::Input(0), 1, -100).unwrap(),
            Ok("scale-read") => eprintln!(
                "model: PB1->Phones {:.4}, AN1->Phones {:.4} pan {}",
                dev.volume(ChannelId::Playback(0), 1).unwrap(),
                dev.volume(ChannelId::Input(0), 1).unwrap(),
                dev.pan(ChannelId::Input(0), 1).unwrap()
            ),
            // M/S on the PB1 pair (#15), or off.
            Ok("src-ms-on") => dev.set_source_ms(ChannelId::Playback(0), true).unwrap(),
            Ok("src-ms-off") => dev.set_source_ms(ChannelId::Playback(0), false).unwrap(),
            // PB1/2's width (#7), AUDRA_WIDTH -1..1, into every output.
            Ok("width") => {
                let w: f32 = std::env::var("AUDRA_WIDTH").expect("AUDRA_WIDTH").parse().expect("a number");
                dev.set_source_width(ChannelId::Playback(0), w).unwrap();
            }
            // M/S on AN1/2 off: AN1/2's cells in every output, from the cache.
            Ok("an12-ms-off") => dev.set_ms_proc(false).unwrap(),
            // Mono on the Phones (#11) for six seconds, then off: one step,
            // because a fresh open doesn't know Mono was on.
            Ok("mono-hold") => {
                dev.set_mono_out(Some(1)).unwrap();
                std::thread::sleep(std::time::Duration::from_secs(6));
                dev.set_mono_out(None).unwrap();
            }
            // A +12 dB low shelf at 1 kHz on both Phones channels, or off.
            Ok(step @ ("eq-on" | "eq-off")) => {
                let mut eq = InputEq { enabled: step == "eq-on", low_cut_freq_hz: 0, ..Default::default() };
                eq.bands[0] = crate::channel::EqBand { band_type: EqBandType::LowShelf, freq_hz: 1000, q: 0.7, gain_db: 12.0 };
                dev.set_output_eq(2, eq).unwrap();
                dev.set_output_eq(3, eq).unwrap();
            }
            other => panic!("AUDRA_PROC_STEP: {other:?}"),
        }
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_pan_round_trip() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        let cid = ChannelId::Input(1); // AN2, verified silent (0%) before running this
        let orig_vol = dev.volume(cid, 0).unwrap();
        let orig_pan = dev.pan(cid, 0).unwrap();

        dev.set_volume(cid, 0, 0.5).unwrap();
        dev.set_pan(cid, 0, 50).unwrap();
        assert!((dev.volume(cid, 0).unwrap() - 0.5).abs() < 0.02);
        assert!((dev.pan(cid, 0).unwrap() - 50).abs() <= 1);

        // Re-open to force a fresh attach_mixer_elements() read from
        // hardware, proving the round trip survives a real re-read,
        // not just the in-memory field set above.
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!((dev2.volume(cid, 0).unwrap() - 0.5).abs() < 0.02);
        assert!((dev2.pan(cid, 0).unwrap() - 50).abs() <= 1);
        drop(dev2);

        dev.set_volume(cid, 0, orig_vol).unwrap();
        dev.set_pan(cid, 0, orig_pan).unwrap();
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_sensitivity_round_trip() {
        // Was honestly-unmapped (this test used to assert it errored) —
        // now real, since babyface-pro-linux gained an "Instrument Ref
        // Level" control (2026-09-06). Verified via a fresh second
        // handle, forcing a real re-read, not the model this call just
        // updated itself.
        let mut dev = BabyfacePro::open().expect("real device attached");
        let idx = 2; // IN3 — Instrument-type, the only kind sensitivity applies to
        let orig = dev.inputs()[idx].sensitivity;

        dev.set_sensitivity(idx, Sensitivity::Minus10dBV).unwrap();
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert_eq!(
            dev2.inputs()[idx].sensitivity,
            Some(Sensitivity::Minus10dBV)
        );
        // Mirrored across both Instrument channels — there's only one
        // real switch (IN4 = idx 3).
        assert_eq!(dev2.inputs()[3].sensitivity, Some(Sensitivity::Minus10dBV));
        drop(dev2);

        dev.set_sensitivity(idx, Sensitivity::Plus4dBu).unwrap();
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert_eq!(dev3.inputs()[idx].sensitivity, Some(Sensitivity::Plus4dBu));
        drop(dev3);

        if let Some(orig) = orig {
            dev.set_sensitivity(idx, orig).unwrap();
        }
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_phase_round_trip() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        let idx = 1; // AN2, verified silent (0%) before running this

        dev.set_phase(idx, true).unwrap();
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!(dev2.inputs()[idx].phase);
        drop(dev2);

        dev.set_phase(idx, false).unwrap();
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert!(!dev3.inputs()[idx].phase);
        drop(dev3);
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_input_phantom_mirrors_across_a_linked_pair() {
        // User-caught: toggling 48V on a linked AN1/2 strip only ever
        // touched its own channel (Message::Phantom called set_phantom
        // directly, no pair-awareness at all) — verified here against
        // the real card, not just mock, since this is orchestration
        // over already-hardware-verified set_phantom/input_pair_linked.
        let mut dev = BabyfacePro::open().expect("real device attached");
        let was_linked = dev.input_pair_linked(0);
        if !was_linked {
            dev.set_input_pair_linked(0, true).unwrap();
        }
        let orig0 = dev.inputs()[0].phantom;
        let orig1 = dev.inputs()[1].phantom;

        dev.set_input_phantom(0, false).unwrap();
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!(!dev2.inputs()[0].phantom);
        assert!(
            !dev2.inputs()[1].phantom,
            "48V off must mirror to AN2 when linked"
        );
        drop(dev2);

        dev.set_input_phantom(0, true).unwrap();
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert!(dev3.inputs()[0].phantom);
        assert!(
            dev3.inputs()[1].phantom,
            "48V on must mirror to AN2 when linked"
        );
        drop(dev3);

        dev.set_phantom(0, orig0).unwrap();
        dev.set_phantom(1, orig1).unwrap();
        if !was_linked {
            dev.set_input_pair_linked(0, false).unwrap();
        }
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_stereo_split_round_trip() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        let pb = 0; // PB1 (playback channels 0/1)

        dev.set_stereo_split(pb, true).unwrap();
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!(dev2.playbacks()[0].split);
        assert!(
            dev2.playbacks()[1].split,
            "split mirrors across both channels of the pair"
        );
        drop(dev2);

        dev.set_stereo_split(pb, false).unwrap();
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert!(!dev3.playbacks()[0].split);
        assert!(!dev3.playbacks()[1].split);
        drop(dev3);
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_mute_zeroes_the_crosspoint_and_unmute_restores_it() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        let cid = ChannelId::Input(1); // AN2, verified silent (0%) before running this
        let orig_vol = dev.volume(cid, 0).unwrap();
        dev.set_volume(cid, 0, 0.5).unwrap();

        dev.set_mute(cid, true).unwrap();
        // The model's own "real" volume is untouched by mute — needed
        // so unmute can restore it.
        assert!((dev.volume(cid, 0).unwrap() - 0.5).abs() < 0.02);
        // But the actual ALSA crosspoint register really is zeroed —
        // proven by re-reading it fresh (a new `attach_mixer_elements`
        // pass), not from the model that deliberately wasn't touched.
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!(
            dev2.volume(cid, 0).unwrap() < 0.02,
            "crosspoint should read back near 0 while muted"
        );
        drop(dev2);

        dev.set_mute(cid, false).unwrap();
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert!(
            (dev3.volume(cid, 0).unwrap() - 0.5).abs() < 0.02,
            "crosspoint should be restored after unmute"
        );
        drop(dev3);

        dev.set_volume(cid, 0, orig_vol).unwrap();
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_a_muted_strips_fader_stays_silent_on_the_card() {
        // OpenCode's job 8: a fader moved on a muted strip wrote its level
        // to the card. ADAT 7 (nothing connected: silent whatever
        // happens), into Main; everything put back as it was.
        let mut dev = BabyfacePro::open().expect("real device attached");
        let names: Vec<String> = dev.inputs().iter().map(|c| c.name.clone()).collect();
        let idx = names.iter().position(|n| n.contains("ADAT") && n.contains('7')).unwrap_or_else(|| panic!("no ADAT 7 among {names:?}"));
        eprintln!("ADAT 7 is input {idx} ({})", names[idx]);
        let cid = ChannelId::Input(idx);
        let (orig_vol, orig_mute) = (dev.volume(cid, 0).unwrap(), dev.mute(cid).unwrap());
        dev.set_mute(cid, true).unwrap();
        dev.set_volume(cid, 0, 0.5).unwrap();
        assert!((dev.volume(cid, 0).unwrap() - 0.5).abs() < 0.02, "the model keeps the fader's level");
        let fresh = BabyfacePro::open().expect("real device attached");
        let on_card = fresh.volume(cid, 0).unwrap();
        drop(fresh);
        eprintln!("muted, fader at 0.5: the card holds {on_card:.3}");
        assert!(on_card < 0.02, "a muted strip's fader must not reach the card ({on_card})");
        dev.set_mute(cid, false).unwrap();
        let fresh = BabyfacePro::open().expect("real device attached");
        let restored = fresh.volume(cid, 0).unwrap();
        drop(fresh);
        eprintln!("unmuted: the card holds {restored:.3}");
        assert!((restored - 0.5).abs() < 0.02, "unmute brings the fader's level back ({restored})");
        dev.set_volume(cid, 0, orig_vol).unwrap();
        dev.set_mute(cid, orig_mute).unwrap();
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_solo_zeroes_other_channels_and_unsolo_restores_them() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        let soloed = ChannelId::Input(0); // AN1
        let other = ChannelId::Input(1); // AN2, verified silent (0%) before running this
        let orig_soloed_vol = dev.volume(soloed, 0).unwrap();
        let orig_other_vol = dev.volume(other, 0).unwrap();
        dev.set_volume(soloed, 0, 0.5).unwrap();
        dev.set_volume(other, 0, 0.5).unwrap();

        dev.set_solo(soloed, true).unwrap();
        // The soloed channel's own level is untouched by soloing it —
        // matching `usb.rs`'s own hardware-validated behavior.
        assert!((dev.volume(soloed, 0).unwrap() - 0.5).abs() < 0.02);
        // The other channel's model volume is preserved for restore...
        assert!((dev.volume(other, 0).unwrap() - 0.5).abs() < 0.02);
        // ...but its actual crosspoint register reads back near 0,
        // proven the same way as the mute test above.
        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!(
            (dev2.volume(soloed, 0).unwrap() - 0.5).abs() < 0.02,
            "soloed channel's own level must be untouched"
        );
        assert!(
            dev2.volume(other, 0).unwrap() < 0.02,
            "non-soloed channel should read back near 0 while something else is soloed"
        );
        drop(dev2);

        dev.set_solo(soloed, false).unwrap();
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert!(
            (dev3.volume(other, 0).unwrap() - 0.5).abs() < 0.02,
            "un-solo should restore the other channel's crosspoint"
        );
        drop(dev3);

        dev.set_volume(soloed, 0, orig_soloed_vol).unwrap();
        dev.set_volume(other, 0, orig_other_vol).unwrap();
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_apply_scene_reissues_mute_and_loopback_not_just_the_model() {
        // Previously `apply_scene` only ever updated `self.inputs`/
        // `self.outputs`/etc — a loaded scene looked right in the GUI
        // but never touched real hardware for anything besides volume.
        // Proves the fix the same way every other live_hardware test in
        // this file does: re-open a second handle to force a fresh ALSA
        // read, rather than trusting the in-memory model.
        let mut dev = BabyfacePro::open().expect("real device attached");
        let baseline = dev.capture_scene();

        let mut modified = baseline.clone();
        modified.inputs[1].mute = true; // AN2, verified silent (0%) before running this
        modified.outputs[0].loopback = true; // AN1/2 pair

        dev.apply_scene(&modified)
            .expect("apply_scene should succeed");

        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!(
            dev2.outputs[0].loopback,
            "loopback should read back engaged after apply_scene, not just in the model"
        );
        assert!(
            dev2.volume(ChannelId::Input(1), 0).unwrap() < 0.02,
            "AN2 crosspoint should read back near 0 while apply_scene's mute is engaged"
        );
        drop(dev2);

        dev.apply_scene(&baseline).expect("restore baseline");
        let dev3 = BabyfacePro::open().expect("real device attached");
        assert!(
            !dev3.outputs[0].loopback,
            "loopback should be restored off after re-applying the baseline scene"
        );
        drop(dev3);
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached AND switched to \
                Class Compliant mode (snd-usb-audio, USB 2a39:3fb0); run \
                manually with --ignored"]
    fn live_hardware_cc_grammar_is_detected_and_read_faithfully() {
        // This replaces an earlier test that asserted `open()` *refused*
        // a CC card. That was right for the few hours between adding the
        // guard and teaching the backend the CC grammar; now the card is
        // supported, and what matters is that the state it reports is the
        // card's own rather than model defaults — the failure mode that
        // started all of this (48V read off while the card had it on).
        let dev = BabyfacePro::open().expect("CC-mode card should now open");
        assert_eq!(dev.grammar, ControlGrammar::ClassCompliant);
        assert!(
            !dev.protocol_features(),
            "the CC personality drives routing, preamps and clock only; every \
             RME-protocol feature is missing, and this is what the UIs gate on"
        );

        // Ranges come from the hardware and differ per preamp law, so
        // they prove these are real reads: mics 0..65, instruments 0..18.
        assert_eq!(dev.inputs()[0].gain_max, Some(65), "mic gain law");
        assert_eq!(dev.inputs()[2].gain_max, Some(18), "instrument gain law");
        for i in 2..4 {
            assert!(
                dev.inputs()[i].sensitivity.is_some(),
                "input {i} sensitivity must be read from the card, not defaulted"
            );
        }
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached AND switched to \
                Class Compliant mode (snd-usb-audio, USB 2a39:3fb0); run \
                manually with --ignored"]
    fn live_hardware_cc_grammar_round_trips_through_a_fresh_handle() {
        // Every check re-opens the device, so a value that only ever
        // reached the in-memory model would fail here.
        let mut dev = BabyfacePro::open().expect("CC-mode card");

        // A paired source writes only its own side of the output pair.
        let orig = dev.volume(ChannelId::Input(4), 1).unwrap();
        dev.set_volume(ChannelId::Input(4), 1, 0.42).unwrap();
        assert!(
            (BabyfacePro::open()
                .unwrap()
                .volume(ChannelId::Input(4), 1)
                .unwrap()
                - 0.42)
                .abs()
                < 0.01
        );
        dev.set_volume(ChannelId::Input(4), 1, orig).unwrap();

        // A mono source's pan is two separate controls in this grammar.
        let (ov, op) = (
            dev.volume(ChannelId::Input(0), 0).unwrap(),
            dev.pan(ChannelId::Input(0), 0).unwrap(),
        );
        dev.set_volume(ChannelId::Input(0), 0, 0.8).unwrap();
        dev.set_pan(ChannelId::Input(0), 0, -50).unwrap();
        assert!(
            (BabyfacePro::open()
                .unwrap()
                .pan(ChannelId::Input(0), 0)
                .unwrap()
                + 50)
                .abs()
                <= 2
        );
        dev.set_volume(ChannelId::Input(0), 0, ov).unwrap();
        dev.set_pan(ChannelId::Input(0), 0, op).unwrap();

        // Sensitivity is per-channel here (the proprietary control is a
        // single shared switch) and its enum items are in the opposite
        // order, so this also guards the resolve-by-name fix.
        let orig_s = dev.inputs()[2].sensitivity.unwrap();
        let other = match orig_s {
            Sensitivity::Plus4dBu => Sensitivity::Minus10dBV,
            Sensitivity::Minus10dBV => Sensitivity::Plus4dBu,
        };
        dev.set_sensitivity(2, other).unwrap();
        let f = BabyfacePro::open().unwrap();
        assert_eq!(f.inputs()[2].sensitivity, Some(other), "IN3 must change");
        assert_eq!(
            f.inputs()[3].sensitivity,
            Some(orig_s),
            "IN4 must NOT mirror"
        );
        drop(f);
        dev.set_sensitivity(2, orig_s).unwrap();
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached AND switched to \
                Class Compliant mode (snd-usb-audio, USB 2a39:3fb0); run \
                manually with --ignored"]
    fn live_hardware_cc_grammar_refuses_proprietary_only_features() {
        // These all look their controls up with a tolerant `if let
        // Some(selem)`, so without the guard they would return Ok(())
        // having done nothing — the failure this whole day was about.
        let mut dev = BabyfacePro::open().expect("CC-mode card");
        assert!(dev.set_phase(0, true).is_err(), "phase");
        assert!(dev.set_trim(0, 3.0).is_err(), "trim");
        assert!(dev.set_loopback(0, true).is_err(), "loopback");
        assert!(dev.set_width(0.5).is_err(), "width");
        assert!(dev.set_dim(true).is_err(), "dim");
        assert!(dev.set_pitch(1.0).is_err(), "pitch");
        assert!(dev.set_eq_enabled(0, true).is_err(), "eq");
        assert!(dev.set_ms_proc(true).is_err(), "ms proc");
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached AND running in \
                Class Compliant mode (snd-usb-audio) — the from-scratch \
                snd-usb-babyface-pro driver has no Sample Clock Source \
                control yet (its README lists clock source as a future \
                follow-up, not yet implemented); run manually with \
                --ignored"]
    fn live_hardware_clock_source_round_trip() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        let orig = dev.settings().clock_source.clone();
        let sources = dev.settings().clock_sources.clone();
        let other = sources
            .iter()
            .find(|s| **s != orig)
            .expect("hardware exposes more than one clock source")
            .clone();

        dev.set_clock_source(&other).unwrap();
        assert_eq!(dev.settings().clock_source, other);

        let dev2 = BabyfacePro::open().expect("real device attached");
        assert_eq!(dev2.settings().clock_source, other);
        drop(dev2);

        dev.set_clock_source(&orig).unwrap();
    }

    #[test]
    #[ignore = "requires the real Babyface Pro FS attached; run manually with --ignored"]
    fn live_hardware_output_volume_beyond_sixth_pair_does_not_panic_or_clobber_a_sibling() {
        let mut dev = BabyfacePro::open().expect("real device attached");
        // idx=8 (ADAT5 OUT, the 9th individual output channel) is the
        // exact index reported crashing in the wild: `self.outputs` has
        // 12 individual-channel entries, but the old code indexed
        // `self.profile.outputs` (6 *pairs*) with the same raw `idx`,
        // panicking for any idx >= 6. idx=9 (ADAT6 OUT) is idx=8's pair
        // sibling — the old code, for idx in 2..=5, also silently wrote
        // *both* channels of the wrong pair, so round-tripping idx=8
        // alone and confirming idx=9 didn't move covers both failure
        // modes in one test.
        let target = ChannelId::Output(8);
        let sibling = ChannelId::Output(9);
        let orig_target = dev.volume(target, 0).unwrap();
        let orig_sibling = dev.volume(sibling, 0).unwrap();

        dev.set_volume(target, 0, 0.5).unwrap();
        assert!((dev.volume(target, 0).unwrap() - 0.5).abs() < 0.02);
        assert!(
            (dev.volume(sibling, 0).unwrap() - orig_sibling).abs() < 0.02,
            "writing Output(8) should not move Output(9)'s volume"
        );

        let dev2 = BabyfacePro::open().expect("real device attached");
        assert!((dev2.volume(target, 0).unwrap() - 0.5).abs() < 0.02);
        drop(dev2);

        dev.set_volume(target, 0, orig_target).unwrap();
    }
}
