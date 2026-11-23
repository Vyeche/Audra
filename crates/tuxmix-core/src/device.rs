use crate::channel::{ChannelId, ChannelType, EqBandType, InputChannel, OutputChannel, PlaybackChannel, Sensitivity};
use crate::error::Error;
use crate::scene::Scene;

/// Global device-level settings.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceSettings {
    /// Current clock source (e.g. "Internal", "AutoSync").
    pub clock_source: String,
    /// Every valid value for `clock_source`, as reported by the
    /// hardware's own enum control (not hardcoded — a different model
    /// could expose a different set). Empty until a real device has
    /// been attached; `#[serde(default)]` for old Scene JSON.
    #[serde(default)]
    pub clock_sources: Vec<String>,
    /// SPDIF optical mode (true = optical, false = coaxial). No
    /// matching ALSA control was found on the Babyface Pro FS (its
    /// SPDIF I/O appears to be optical-only, with no toggle) — this
    /// field is currently unmapped and always `false`.
    pub spdif_optical: bool,
    /// SPDIF emphasis (`IEC958 Emphasis`).
    ///
    /// **Vestigial since 2026-09-12 — read-only, in practice always
    /// `false`.** This and the two fields below are only ever populated
    /// by `BabyfacePro::attach_mixer_elements`, from generic IEC958
    /// controls that exist solely in Class Compliant mode, which
    /// `BabyfacePro::open` now refuses. Their setters were removed at
    /// the same time (they were never Babyface Pro features — see the
    /// note next to `set_optical_out_format`). Kept as fields so scenes
    /// written before that date keep round-tripping unchanged; the
    /// card's real optical/SPDIF state is `optical_out_spdif`.
    pub spdif_emphasis: bool,
    /// SPDIF professional/consumer format flag (`IEC958 Pro Mask`).
    /// Vestigial — see [`DeviceSettings::spdif_emphasis`].
    pub spdif_professional: bool,
    /// Whether the SPDIF output is actively transmitting (`IEC958
    /// Switch`, shown as `"IEC958"` in the simple-mixer view — the
    /// standard generic ALSA S/PDIF enable switch, not RME-specific).
    /// Vestigial — see [`DeviceSettings::spdif_emphasis`].
    /// `#[serde(default)]` so scenes saved before this field existed
    /// still deserialize (same pattern as `Scene::model`).
    #[serde(default)]
    pub spdif_enabled: bool,
    /// Pitch/varispeed in percent (-5..+5). 0 = nominal. Not persisted
    /// (the pitch is a live clock setting; the auto-scene keeps 0).
    #[serde(default, skip_serializing)]
    pub pitch_percent: f32,
    /// MS-proc engaged (the AN2 "side" crosspoints are muted).
    #[serde(default)]
    pub ms_proc: bool,
    /// AN 1>2 engaged (0x17 wIdx=0x1000 flag).
    #[serde(default)]
    pub an12: bool,
    /// EQ for Record: whether the hardware DSP EQ is applied to the
    /// recorded signal too, not just the monitor mix. Settings
    /// keepalive bit 6 (`PROTOCOL.md`'s `cap_eqr.pcap`, hardware-
    /// verified) — an RME-specific setting, distinct from the generic
    /// `spdif_*` fields above.
    #[serde(default)]
    pub eq_for_record: bool,
    /// Optical Out format: `true` = SPDIF (2-channel), `false` = ADAT
    /// (8-channel, the device default). Settings keepalive bit 10
    /// (`PROTOCOL.md`'s `cap_opt.pcap`, hardware-verified) — an
    /// RME-specific setting for the single physical optical port,
    /// distinct from the generic `spdif_*` fields above (which target
    /// the standard ALSA S/PDIF control surface this device doesn't
    /// expose).
    #[serde(default)]
    pub optical_out_spdif: bool,
    /// Dim engaged on the Phones output (an absolute -20 dB cut,
    /// independent of the Phones master's own volume). See
    /// [`RmeDevice::set_dim`].
    #[serde(default)]
    pub dim: bool,
    /// FX send level in dB (-65..0, None = unset). 0 dB = max send.
    /// `Option` so old scenes (no send) stay untouched on apply.
    #[serde(default)]
    pub fx_send_db: Option<f32>,
    /// The other sources' FX sends in dB (-65..0), by the driver's pair
    /// name ("AN3/4", "AS1/2", "PB1", ...; see [`source_send_pair`]).
    /// Missing is unset: an old scene leaves them as they are.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub source_fx_send: std::collections::BTreeMap<String, f32>,
    /// Phase invert on each output channel (index = `ChannelId::Output`),
    /// done host-side as TotalMix does: every source's cell into that
    /// side negated (issue #15). Missing is off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_phase: Vec<bool>,
    /// M/S processing on each output pair: L' = (L+R)/2, R' = (L-R)/2,
    /// built into every source's cells (issue #15). Missing is off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_ms: Vec<bool>,
    /// Each output channel's EQ (index = `ChannelId::Output`), computed by
    /// Audra and sent with the driver's "DSP Block" (issue #15). None, or
    /// past the end, is never set: the unit's EQ is left as it is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_eq: Vec<Option<crate::channel::InputEq>>,
    /// M/S processing on a stereo source (TotalMix's M/S Proc on an input
    /// or playback pair, issue #15), by the driver's pair name ("AN3/4",
    /// "AS1/2", "PB1", ...; see [`source_send_pair`]). Missing is off.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub source_ms: std::collections::BTreeMap<String, bool>,
    /// Each linked stereo pair's width (issue #7), TotalMix's -1..+1: 1 as
    /// recorded, 0 mono, -1 left and right swapped. By pair name
    /// ([`source_width_pair`]); missing is 1.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub source_width: std::collections::BTreeMap<String, f32>,
    /// The FX return (the host's Reverb/Echo) into each output pair, 0..2
    /// on the crosspoints' scale (1 = 0 dB). Empty, or a pair past its end,
    /// is off: what the driver starts at, and what an old scene means.
    #[serde(default)]
    pub fx_return: Vec<f32>,
    /// The old global width (the driver's AN1/2 "Width", removed
    /// 2026-11-09). Kept so old scenes load; nothing reads it. Width is
    /// per pair now: [`Self::source_width`].
    #[serde(default)]
    pub width: f32,
    /// Active sample rate in Hz (proprietary USB mode; the ALSA backend
    /// leaves it to the kernel driver). `#[serde(default = …)]` so old
    /// Scene JSON (and scenes from the ALSA backend) still deserialize.
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Input-strip stereo link mirror, kept in sync by
    /// [`RmeDevice::set_input_link`] so it's readable (the trait itself
    /// only has a setter — this is where the UI reads the current state
    /// from). `true` = linked, TotalMix's default.
    #[serde(default = "default_input_link")]
    pub input_link: bool,
    /// Per-output-pair stereo link state — index = submix pair (same
    /// indexing as [`RmeDevice::output_pair_count`]/`OUT_LABELS`).
    /// Missing/short index = linked (TotalMix's default for every pair),
    /// not a plain `#[serde(default)]` empty-vec special case — see
    /// [`RmeDevice::output_linked`]. Mirrors a real TotalMix behavior
    /// confirmed against hardware captures: the stereo-link toggle itself
    /// writes nothing to the device (it's a pure UI/model state); only
    /// the *next* volume/mute/solo write on either channel of the pair is
    /// affected by whether it's linked.
    #[serde(default)]
    pub output_link: Vec<bool>,
    /// Per-input-pair stereo link state — generic display/control
    /// grouping (one fader/mute/solo for the pair vs two independent
    /// channels), same shape and default as [`Self::output_link`] — see
    /// [`RmeDevice::input_pair_linked`]. Deliberately separate from
    /// [`Self::input_link`]: that field mirrors the AN1/2-specific
    /// hardware preamp-link flag (48V/gain bits physically written
    /// together for that one pair); this one is the same pure-UI
    /// grouping concept `output_link` uses, and applies to every input
    /// pair, not just AN1/2 — TotalMix shows a stereo button on every
    /// strip, not only the mic pair.
    #[serde(default)]
    pub input_pair_link: Vec<bool>,
}

impl DeviceSettings {
    /// Whether output channel `ch` has its phase inverted.
    pub fn output_phase_of(&self, ch: usize) -> bool {
        self.output_phase.get(ch).copied().unwrap_or(false)
    }

    /// The width of the stereo pair `channel` belongs to (1 when none set).
    pub fn source_width_of(&self, channel: crate::channel::ChannelId) -> f32 {
        source_width_pair(channel).and_then(|p| self.source_width.get(p).copied()).unwrap_or(1.0)
    }

    /// Whether the stereo source `channel` belongs to has M/S on (AN1/2's
    /// is [`Self::ms_proc`], as it always was).
    pub fn source_ms_of(&self, channel: crate::channel::ChannelId) -> bool {
        if let crate::channel::ChannelId::Input(0 | 1) = channel {
            return self.ms_proc;
        }
        source_send_pair(channel).is_some_and(|p| self.source_ms.get(p).copied().unwrap_or(false))
    }

    /// Output channel `ch`'s EQ, if Audra ever set one.
    pub fn output_eq_of(&self, ch: usize) -> Option<crate::channel::InputEq> {
        self.output_eq.get(ch).copied().flatten()
    }

    /// Keeps output channel `ch`'s EQ.
    pub fn keep_output_eq(&mut self, ch: usize, eq: crate::channel::InputEq) {
        if self.output_eq.len() <= ch {
            self.output_eq.resize(ch + 1, None);
        }
        self.output_eq[ch] = Some(eq);
    }

    /// Whether output pair `pair` has M/S processing on.
    pub fn output_ms_of(&self, pair: usize) -> bool {
        self.output_ms.get(pair).copied().unwrap_or(false)
    }

    /// Keeps output channel `ch`'s phase; an all-off list is emptied so
    /// scenes stay as they were.
    pub fn keep_output_phase(&mut self, ch: usize, on: bool) {
        if self.output_phase.len() <= ch {
            self.output_phase.resize(ch + 1, false);
        }
        self.output_phase[ch] = on;
        if !self.output_phase.iter().any(|&b| b) {
            self.output_phase.clear();
        }
    }

    /// Keeps output pair `pair`'s M/S processing, as `keep_output_phase`.
    pub fn keep_output_ms(&mut self, pair: usize, on: bool) {
        if self.output_ms.len() <= pair {
            self.output_ms.resize(pair + 1, false);
        }
        self.output_ms[pair] = on;
        if !self.output_ms.iter().any(|&b| b) {
            self.output_ms.clear();
        }
    }

    /// The FX send of the pair `channel` belongs to, in dB (-65 = off,
    /// also for a pair never set). AN1/2's is `fx_send_db`.
    pub fn source_fx_send_of(&self, channel: crate::channel::ChannelId) -> f32 {
        source_send_pair(channel)
            .and_then(|p| self.source_fx_send.get(p).copied())
            .unwrap_or(-65.0)
    }

    /// The FX return into output pair `pair` (0 = off).
    pub fn fx_return_of(&self, pair: usize) -> f32 {
        self.fx_return.get(pair).copied().unwrap_or(0.0)
    }

    /// Keeps `volume` as the FX return into `pair`.
    pub fn keep_fx_return(&mut self, pair: usize, volume: f32) {
        if self.fx_return.len() <= pair {
            self.fx_return.resize(pair + 1, 0.0);
        }
        self.fx_return[pair] = volume;
    }
}

/// Serde default for [`DeviceSettings::sample_rate`].
fn default_sample_rate() -> u32 {
    48_000
}

/// Serde default for [`DeviceSettings::input_link`] (TotalMix's own
/// default is linked).
fn default_input_link() -> bool {
    true
}

/// A generic RME audio interface.
///
/// Each implementation maps to a specific hardware model and knows
/// how to discover and control its ALSA mixer elements.
///
/// The device exposes a matrix (submix) mixer: each input and playback
/// channel has its own volume and pan towards every hardware output pair.
pub trait RmeDevice {
    /// Human-readable model name (e.g. "Babyface Pro FS"), decorated per
    /// backend for display in the UI (e.g. "Babyface Pro FS (USB)",
    /// "Babyface Pro FS (mock)") — NOT what scene compatibility should be
    /// judged on, see [`Self::canonical_model`].
    fn model_name(&self) -> &str;

    /// Model identity used for [`Scene::check_compatible`] — deliberately
    /// separate from [`Self::model_name`]. Two backends that talk to the
    /// same physical device (e.g. the ALSA kernel-driver path and the
    /// direct-USB path both controlling a Babyface Pro FS) should be able
    /// to restore each other's saved scenes; only genuinely different
    /// hardware should be rejected. Defaults to [`Self::model_name`] for
    /// backends where the two don't need to differ (e.g. the mock, whose
    /// captures are never persisted to the shared auto-scene anyway).
    fn canonical_model(&self) -> &str {
        self.model_name()
    }

    /// Whether the DSP features that live in RME's own protocol can be
    /// driven on this connection: EQ, phase, trim, stereo split, MS
    /// processing, AN 1>2, input link, width, FX send, varispeed pitch,
    /// loopback and the front panel. Routing, preamps, gains, sample rate
    /// and clock are *not* on that list; they work on both personalities.
    ///
    /// False on the class-compliant personality, where stock
    /// `snd-usb-audio` exposes routing, preamps and clock and nothing else
    /// (see `ControlGrammar` in the `babyface` module): the ALSA backend
    /// answers from the grammar it detected at open, every other backend
    /// drives the full set, and the UIs use this to stop offering controls
    /// that cannot work.
    fn protocol_features(&self) -> bool {
        true
    }

    /// Whether the device this handle was opened on is still there, and
    /// still the same one. A card that was unplugged (or re-enumerated
    /// under a new card index, e.g. by a personality switch) keeps
    /// answering a mixer or USB handle it was opened from: only a fresh
    /// lookup sees the difference, so the UI polls this instead of
    /// trusting the handle. Defaults to `true`, since a backend that
    /// cannot tell must keep assuming the best rather than flashing a
    /// false "disconnected".
    fn is_connected(&self) -> bool {
        true
    }

    /// Number of physical stereo output pairs on this device.
    fn output_pair_count(&self) -> usize;

    /// Attempt to detect the device on the ALSA bus and open a mixer handle.
    fn open() -> Result<Self, Error>
    where
        Self: Sized;

    /// Returns a reference to all hardware input channels.
    fn inputs(&self) -> &[InputChannel];

    /// Returns a mutable reference to all hardware input channels.
    fn inputs_mut(&mut self) -> &mut [InputChannel];

    /// Returns a reference to all software playback channels.
    fn playbacks(&self) -> &[PlaybackChannel];

    /// Returns a mutable reference to all software playback channels.
    fn playbacks_mut(&mut self) -> &mut [PlaybackChannel];

    /// Returns a reference to all physical output channels.
    fn outputs(&self) -> &[OutputChannel];

    /// Returns a mutable reference to all physical output channels.
    fn outputs_mut(&mut self) -> &mut [OutputChannel];

    /// Returns the current global device settings.
    fn settings(&self) -> &DeviceSettings;

    /// Returns a mutable reference to the global device settings.
    fn settings_mut(&mut self) -> &mut DeviceSettings;

    // ── Control operations (submix / matrix) ──────────────────────

    /// Set the volume (0.0 = silence, 1.0 = 0 dB, 2.0 = +6 dB, the top) for a
    /// given channel into a specific output pair.
    fn set_volume(&mut self, channel: ChannelId, output: usize, volume: f32) -> Result<(), Error>;

    /// Get the volume (0.0 = silence, 1.0 = 0 dB, 2.0 = +6 dB) for a given
    /// channel into a specific output pair.
    fn volume(&self, channel: ChannelId, output: usize) -> Result<f32, Error>;

    /// Set the pan (-100 .. 100) for a given channel into a specific output pair.
    fn set_pan(&mut self, channel: ChannelId, output: usize, pan: i8) -> Result<(), Error>;

    /// Get the pan (-100 .. 100) for a given channel into a specific output pair.
    fn pan(&self, channel: ChannelId, output: usize) -> Result<i8, Error>;

    /// Whether `channel`'s pan (a balance, on a stereo pair) is one this
    /// backend can set. An output has none; a backend whose driver lacks
    /// it for some sources says which, so the knob says so at rest
    /// instead of failing when turned.
    fn can_pan(&self, channel: ChannelId) -> bool {
        !matches!(channel, ChannelId::Output(_))
    }

    // ── Mute / Solo ────────────────────────────────────────────────

    /// Set mute state for a channel.
    fn set_mute(&mut self, channel: ChannelId, mute: bool) -> Result<(), Error>;

    /// Get mute state for a channel.
    fn mute(&self, channel: ChannelId) -> Result<bool, Error>;

    /// Set solo state for a channel.
    fn set_solo(&mut self, channel: ChannelId, solo: bool) -> Result<(), Error>;

    /// Get solo state for a channel.
    fn solo(&self, channel: ChannelId) -> Result<bool, Error>;

    // ── Preamp controls (input-only: 48V, pad, gain) ────────────────

    /// Set 48V phantom power for an input. Errors if the input isn't a
    /// Mic channel (the only type with a phantom power switch).
    fn set_phantom(&mut self, idx: usize, on: bool) -> Result<(), Error>;

    /// Set the -20dB pad for an input. Errors if the input type has no
    /// pad switch.
    fn set_pad(&mut self, idx: usize, on: bool) -> Result<(), Error>;

    /// Set the preamp gain (dB, 1 dB steps, see
    /// [`InputChannel::gain_max`]) for an input. Errors if the input
    /// type has no gain control.
    fn set_gain(&mut self, idx: usize, gain: u32) -> Result<(), Error>;

    /// Sets input `idx`'s phantom power, respecting its pair's link
    /// state — same idea as [`Self::set_input_mute`]/[`Self::
    /// set_input_volume`], but `idx` alone is enough here (no separate
    /// `pair`/`which`): a linked pair only ever exposes ONE strip (the
    /// left/even channel), so the caller never needs to disambiguate
    /// which side was clicked the way Mute/Solo/Volume's own callers
    /// do when driven from a bare pair index.
    ///
    /// Previously missing entirely — every caller called [`Self::
    /// set_phantom`] directly, so toggling 48V on a linked strip only
    /// ever touched its own (left) channel, silently leaving the right
    /// channel's phantom power wherever it last was — found 2026-09-07
    /// (the user noticed AN1/2's 48V toggle wasn't actually affecting
    /// both physical inputs).
    fn set_input_phantom(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        self.set_phantom(idx, on)?;
        let pair = idx / 2;
        if self.input_pair_linked(pair) {
            self.set_phantom(idx ^ 1, on)?;
        }
        Ok(())
    }

    /// Same idea as [`Self::set_input_phantom`], for the -20dB pad.
    fn set_input_pad(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        self.set_pad(idx, on)?;
        let pair = idx / 2;
        if self.input_pair_linked(pair) {
            self.set_pad(idx ^ 1, on)?;
        }
        Ok(())
    }

    /// Same idea as [`Self::set_input_phantom`], for preamp gain.
    fn set_input_gain(&mut self, idx: usize, gain: u32) -> Result<(), Error> {
        self.set_gain(idx, gain)?;
        let pair = idx / 2;
        if self.input_pair_linked(pair) {
            self.set_gain(idx ^ 1, gain)?;
        }
        Ok(())
    }

    /// Set the sample clock pitch/varispeed in percent (-5..+5, 0 = nominal).
    /// The 0x1B DDS quad shifts the device clock dynamically (the
    /// actual sample rate moves with the pitch).
    fn set_pitch(&mut self, pitch_percent: f32) -> Result<(), Error>;

    /// Set the active sample rate in Hz. The proprietary USB backend
    /// does a mid-session `SET_INTERFACE` + stream restart; the ALSA
    /// backend and the mock leave it to the system (no-op). Errors on
    /// unsupported rates (e.g. 50 kHz).
    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Error> {
        let _ = rate;
        Ok(())
    }

    // ── §9 controls (decoded on Windows 2026-08-23; see PROTOCOL.md) ──
    // These default to an error so a backend that hasn't mapped them
    // fails loudly instead of silently doing nothing.

    /// Loopback on an output pair (bReq 0x15, one mono channel per side).
    fn set_loopback(&mut self, out: usize, on: bool) -> Result<(), Error> {
        let _ = (out, on);
        Err(Error::InvalidChannel(
            "Loopback is not supported on this backend".into(),
        ))
    }

    /// MS-proc engage: mutes the AN2 crosspoints, restores the saved
    /// fader on disengage.
    fn set_ms_proc(&mut self, on: bool) -> Result<(), Error> {
        let _ = on;
        Err(Error::InvalidChannel(
            "MS proc is not supported on this backend".into(),
        ))
    }

    /// AN 1>2 toggle (0x17 wIdx=0x1000 flag).
    fn set_an12(&mut self, on: bool) -> Result<(), Error> {
        let _ = on;
        Err(Error::InvalidChannel(
            "AN 1>2 is not supported on this backend".into(),
        ))
    }

    /// Dim the Phones output by a fixed -20 dB, independent of its
    /// current master volume (disengage restores the pre-dim level).
    fn set_dim(&mut self, on: bool) -> Result<(), Error> {
        let _ = on;
        Err(Error::InvalidChannel(
            "Dim is not supported on this backend".into(),
        ))
    }

    /// EQ for Record toggle (settings keepalive bit 6).
    fn set_eq_for_record(&mut self, on: bool) -> Result<(), Error> {
        let _ = on;
        Err(Error::InvalidChannel(
            "EQ for Record is not supported on this backend".into(),
        ))
    }

    /// Optical Out format: `true` = SPDIF, `false` = ADAT (settings
    /// keepalive bit 10).
    fn set_optical_out_format(&mut self, spdif: bool) -> Result<(), Error> {
        let _ = spdif;
        Err(Error::InvalidChannel(
            "Optical Out format is not supported on this backend".into(),
        ))
    }

    /// CUE: exclusively monitors output `idx`'s own dedicated playback
    /// pair through the AN1/2 bus, muting every other playback pair
    /// there (`on = true`); `false` restores the normal monitor mix.
    /// Exclusive across every output, same as solo — engaging CUE on
    /// one output silently disengages whichever other output had it.
    fn set_cue(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        let _ = (idx, on);
        Err(Error::InvalidChannel(
            "CUE is not supported on this backend".into(),
        ))
    }

    /// Input-strip stereo link (the AN1/2 pair): `true` = linked (the
    /// default TotalMix state — gains/48V move together), `false` =
    /// split into individual AN1/AN2 buses. USB backend writes the
    /// 0x17 wIdx=0x1000 flag; ALSA/mock are no-ops.
    fn set_input_link(&mut self, linked: bool) -> Result<(), Error> {
        let _ = linked;
        Ok(())
    }

    // ── Output pair stereo link (bus grouping) ──────────────────────
    // TotalMix shows each hardware output pair as ONE linked stereo bus
    // by default (one fader/mute/solo drives both channels); the
    // "stereo" button on that bus splits it into two independently
    // controllable mono channels. Confirmed against hardware captures
    // that the toggle itself never writes to the device — only
    // `set_output_volume`/`set_output_mute`/`set_output_solo` below,
    // called for a fader/mute/solo action on the strip, need to know the
    // current link state. Assumes `ChannelId::Output(pair * 2 + which)`
    // addressing (one physical channel each), so every backend's
    // `outputs()` must lay out pairs contiguously in that order.

    /// Whether output pair `pair` (submix index, 0-based) is currently a
    /// linked stereo bus. `true` (linked) if `pair` has no explicit entry
    /// in `DeviceSettings::output_link` yet — TotalMix's own default.
    fn output_linked(&self, pair: usize) -> bool {
        self.settings().output_link.get(pair).copied().unwrap_or(true)
    }

    /// Sets whether output pair `pair` is linked. Pure model state — see
    /// this section's doc comment; no backend needs to override this.
    fn set_output_linked(&mut self, pair: usize, linked: bool) -> Result<(), Error> {
        let v = &mut self.settings_mut().output_link;
        if v.len() <= pair {
            v.resize(pair + 1, true);
        }
        v[pair] = linked;
        Ok(())
    }

    /// Sets output pair `pair`'s volume — the entry point the UI should
    /// use for an output-strip fader drag instead of calling
    /// [`Self::set_volume`] directly. Linked: writes both channels of
    /// the pair to `volume`. Split: writes only `which` (0 = the pair's
    /// first/left channel, 1 = the second/right one).
    fn set_output_volume(&mut self, pair: usize, which: usize, volume: f32) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.output_linked(pair) {
            self.set_volume(ChannelId::Output(l), 0, volume)?;
            self.set_volume(ChannelId::Output(r), 0, volume)?;
        } else {
            self.set_volume(ChannelId::Output(if which == 0 { l } else { r }), 0, volume)?;
        }
        Ok(())
    }

    /// Writes a crosspoint's level and pan to the hardware only, leaving the
    /// stored mix (the `inputs`/`playbacks` volumes and pans) untouched. For
    /// host-side overrides, such as an app's Control Room Cue, Talkback or
    /// External Input, that compose temporary values on top of the user's
    /// mix and must be able to put the mix back exactly. Backends without
    /// such a path report `UnsupportedDeviceMode` rather than change the
    /// model.
    ///
    /// `volume` uses the same range as [`Self::set_volume`]. `pan` only
    /// applies to the four true-mono inputs (AN1-AN4): the linked stereo
    /// sources (AS1/2 and the ADAT pairs) and every playback pair have one
    /// hardware level per side with no cross-routing, so `pan` is ignored
    /// for them.
    ///
    /// Mute and solo win over a raw write. [`Self::set_mute`] rewrites this
    /// channel's crosspoints from the model, and [`Self::set_solo`] does the
    /// same for every strip except the soloed one (and its linked partner),
    /// so both discard any active override. In the other direction, a raw
    /// write on a muted or non-soloed channel puts sound back on the
    /// hardware while the model still reports the channel as muted/silent.
    /// Callers must respect mute/solo themselves and re-apply their override
    /// after toggling either.
    /// While `on`, a level or pan set on a crosspoint (`set_volume`,
    /// `set_pan` and what calls them) changes the model only and leaves the
    /// card as it is. For a caller that holds that crosspoint itself with
    /// [`Self::write_crosspoint_raw`] (the app's Control Room: a solo,
    /// Talkback, an ease): it writes the card composed straight after, and
    /// the plain level never plays, even for a moment. Output masters are
    /// not held. Off by default; the caller turns it off again. A backend
    /// without raw writes ignores it.
    fn hold_card(&mut self, on: bool) {
        let _ = on;
    }

    fn write_crosspoint_raw(
        &mut self,
        channel: ChannelId,
        output: usize,
        volume: f32,
        pan: i8,
    ) -> Result<(), Error> {
        let _ = (channel, output, volume, pan);
        Err(Error::UnsupportedDeviceMode {
            card: self.model_name().to_string(),
            missing: "raw crosspoint writes".into(),
        })
    }

    /// Lets the application act on the front panel's DIM button itself
    /// (`true`) instead of the backend applying its own Dim, the way
    /// TotalMix decides what DIM does on Windows. Backends without such a
    /// button ignore it.
    fn set_app_handles_dim_button(&mut self, on: bool) {
        let _ = on;
    }

    /// Whether the DIM button toggled since the last call (an odd number of
    /// presses), while the application handles it. Read after
    /// [`Self::poll_events`].
    fn take_dim_button_toggle(&mut self) -> bool {
        false
    }

    /// Sets output pair `pair`'s mute, respecting its link state — same
    /// shape as [`Self::set_output_volume`].
    fn set_output_mute(&mut self, pair: usize, which: usize, mute: bool) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.output_linked(pair) {
            self.set_mute(ChannelId::Output(l), mute)?;
            self.set_mute(ChannelId::Output(r), mute)?;
        } else {
            self.set_mute(ChannelId::Output(if which == 0 { l } else { r }), mute)?;
        }
        Ok(())
    }

    /// Sets output pair `pair`'s solo, respecting its link state — same
    /// shape as [`Self::set_output_volume`].
    fn set_output_solo(&mut self, pair: usize, which: usize, solo: bool) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.output_linked(pair) {
            self.set_solo(ChannelId::Output(l), solo)?;
            self.set_solo(ChannelId::Output(r), solo)?;
        } else {
            self.set_solo(ChannelId::Output(if which == 0 { l } else { r }), solo)?;
        }
        Ok(())
    }

    // ── Input pair stereo link (bus grouping) ───────────────────────
    // Same generic display/control grouping as the output section above
    // — TotalMix shows a stereo-link button on every strip, not only the
    // AN1/2 mic pair. Deliberately separate from `input_link`/
    // `set_input_link`, which is the AN1/2-specific hardware preamp-link
    // flag (48V/gain bits physically written together for that one
    // pair) — this is the same pure-UI grouping `output_link` uses,
    // generalized to every input pair.

    /// Whether input pair `pair` is currently a linked stereo bus. Unset,
    /// a pair is linked, except two microphone inputs: a linked pair goes
    /// left and right, and a single microphone belongs in the centre.
    fn input_pair_linked(&self, pair: usize) -> bool {
        self.settings()
            .input_pair_link
            .get(pair)
            .copied()
            .unwrap_or_else(|| self.input_pair_linked_by_default(pair))
    }

    /// The link state of input pair `pair` before it is set: see
    /// [`Self::input_pair_linked`].
    fn input_pair_linked_by_default(&self, pair: usize) -> bool {
        self.inputs()
            .get(pair * 2)
            .is_none_or(|c| c.channel_type != ChannelType::Mic)
    }

    /// Sets whether input pair `pair` is linked. Linking puts the pair's
    /// two channels left and right in every submix, as TotalMix does
    /// (see [`Self::set_input_pair_pan`]); unlinking leaves them there.
    fn set_input_pair_linked(&mut self, pair: usize, linked: bool) -> Result<(), Error> {
        let defaults: Vec<bool> = (0..=pair)
            .map(|p| self.input_pair_linked_by_default(p))
            .collect();
        let v = &mut self.settings_mut().input_pair_link;
        while v.len() <= pair {
            v.push(defaults[v.len()]);
        }
        v[pair] = linked;
        if linked {
            for out in 0..self.output_pair_count() {
                let pan = self.input_pair_pan(pair, out);
                self.set_input_pair_pan(pair, out, pan)?;
            }
            self.align_pair(ChannelId::Input(pair * 2), ChannelId::Input(pair * 2 + 1))?;
            self.align_pair_preamp(pair)?;
        }
        Ok(())
    }

    /// A linked mic pair's 48V and PAD as one, the left's (TotalMix has one
    /// +48V key for a stereo pair, and both P48 lanes light on the unit;
    /// the owner, 2026-11-07: "P48 should be lit on both sides like in
    /// windows"). The unit keeps 48V across restarts, so a pair whose
    /// right mic had been switched apart kept AN2 dark under a lit strip.
    fn align_pair_preamp(&mut self, pair: usize) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        let (Some(left), Some(right)) = (self.inputs().get(l).cloned(), self.inputs().get(r).cloned()) else {
            return Ok(());
        };
        if left.channel_type != ChannelType::Mic || right.channel_type != ChannelType::Mic {
            return Ok(());
        }
        if right.phantom != left.phantom {
            self.set_phantom(r, left.phantom)?;
        }
        if right.pad != left.pad {
            self.set_pad(r, left.pad)?;
        }
        Ok(())
    }

    /// Makes a linked pair one source: its right channel takes the left's
    /// level in every submix and the left's mute, where they differ, as
    /// TotalMix does when a pair goes stereo. The strip shows (and moves)
    /// the pair as its left channel, so a right side left as it was before
    /// the link, down, played nothing under the one fader: the owner's
    /// headphones lost a side at start until a send was moved (2026-11-06,
    /// "the headphones dont work on one side").
    fn align_pair(&mut self, l: ChannelId, r: ChannelId) -> Result<(), Error> {
        // A source has a level in every submix; an output, one of its own.
        let outs: Vec<usize> = match l {
            ChannelId::Output(_) => vec![0],
            _ => (0..self.output_pair_count()).collect(),
        };
        for out in outs {
            if let (Ok(vl), Ok(vr)) = (self.volume(l, out), self.volume(r, out)) {
                if (vl - vr).abs() > 1e-6 {
                    self.set_volume(r, out, vl)?;
                }
            }
        }
        let mute_of = |d: &Self, c: ChannelId| match c {
            ChannelId::Input(i) => d.inputs().get(i).map(|ch| ch.mute),
            ChannelId::Playback(i) => d.playbacks().get(i).map(|ch| ch.mute),
            ChannelId::Output(i) => d.outputs().get(i).map(|ch| ch.mute),
        };
        if let (Some(ml), Some(mr)) = (mute_of(self, l), mute_of(self, r)) {
            if ml != mr {
                self.set_mute(r, ml)?;
            }
        }
        Ok(())
    }

    /// Every linked input and playback pair made one source again
    /// ([`Self::align_pair`]): at start, after the saved mix and the start
    /// preset, so a pair stored or restored with its sides apart (from
    /// before the link aligned them, or an ALSA state from another driver)
    /// plays as the strip shows it.
    /// Whether this card can fold an output to mono (Mono in the Control
    /// Room): the driver must reach the stereo sources' crossed cells.
    fn can_mono(&self) -> bool {
        false
    }

    /// The output pair Mono folds, if any.
    fn mono_out(&self) -> Option<usize> {
        None
    }

    /// Folds output pair `out` to mono, or none: every source into it at
    /// (L+R)/2 on both sides, as TotalMix writes it (PROTOCOL.md "MONO
    /// and PHASE are crosspoint writes"). Held as a mode, so a level set
    /// while it is on stays mono.
    fn set_mono_out(&mut self, out: Option<usize>) -> Result<(), Error> {
        if out.is_some() {
            return Err(Error::MixerElementNotFound("crossed crosspoints (Mono)".into()));
        }
        Ok(())
    }

    fn align_linked_pairs(&mut self) -> Result<(), Error> {
        for pair in 0..self.inputs().len() / 2 {
            if self.input_pair_linked(pair) {
                self.align_pair(ChannelId::Input(pair * 2), ChannelId::Input(pair * 2 + 1))?;
                self.align_pair_preamp(pair)?;
            }
        }
        for pair in 0..self.playbacks().len() / 2 {
            if self.playback_linked(pair) {
                self.align_pair(ChannelId::Playback(pair * 2), ChannelId::Playback(pair * 2 + 1))?;
            }
        }
        // Outputs too, where each pair is two channels (the ALSA layout; the
        // USB one keeps one level per pair): a Phones pair restored with its
        // right side down is one silent ear as well.
        if self.outputs().len() == 2 * self.output_pair_count() {
            for pair in 0..self.output_pair_count() {
                if self.output_linked(pair) {
                    self.align_pair(ChannelId::Output(pair * 2), ChannelId::Output(pair * 2 + 1))?;
                }
            }
        }
        Ok(())
    }

    /// The pan of linked input pair `pair` into submix `output`: 0 when
    /// its left channel is hard left and its right channel hard right.
    /// Channels that are not in that shape (both centred, as they were
    /// before a linked pair was panned this way) read as 0.
    fn input_pair_pan(&self, pair: usize, output: usize) -> i8 {
        let l = self.pan(ChannelId::Input(pair * 2), output).unwrap_or(0);
        let r = self.pan(ChannelId::Input(pair * 2 + 1), output).unwrap_or(0);
        linked_pair_pan(l, r)
    }

    /// Pans linked input pair `pair` into submix `output`. At 0 the left
    /// channel goes hard left and the right one hard right; panning moves
    /// both together, so that at +100 both are hard right.  Writing the
    /// same pan to both channels instead made the pair mono (TuxMix#23).
    /// A source that has no pan of its own (a hardware stereo pair such
    /// as AS1/2 or ADAT) is already left and right, and is left alone.
    fn set_input_pair_pan(&mut self, pair: usize, output: usize, pan: i8) -> Result<(), Error> {
        let (pl, pr) = linked_pair_pans(pan);
        let (l, r) = (ChannelId::Input(pair * 2), ChannelId::Input(pair * 2 + 1));
        if self.pan(l, output).ok() == Some(pl) && self.pan(r, output).ok() == Some(pr) {
            return Ok(());
        }
        if self.set_pan(l, output, pl).is_err() {
            return Ok(());
        }
        self.set_pan(r, output, pr)
    }

    /// Sets input pair `pair`'s volume (into submix `output`), respecting
    /// its link state — same shape as [`Self::set_output_volume`], with
    /// an extra `output` (submix bus) parameter since inputs route into
    /// one of several buses, unlike an output's single master.
    fn set_input_volume(
        &mut self,
        pair: usize,
        which: usize,
        output: usize,
        volume: f32,
    ) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.input_pair_linked(pair) {
            let pan = self.input_pair_pan(pair, output);
            self.set_input_pair_pan(pair, output, pan)?;
            self.set_volume(ChannelId::Input(l), output, volume)?;
            self.set_volume(ChannelId::Input(r), output, volume)?;
        } else {
            self.set_volume(ChannelId::Input(if which == 0 { l } else { r }), output, volume)?;
        }
        Ok(())
    }

    /// Sets input pair `pair`'s mute, respecting its link state.
    fn set_input_mute(&mut self, pair: usize, which: usize, mute: bool) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.input_pair_linked(pair) {
            self.set_mute(ChannelId::Input(l), mute)?;
            self.set_mute(ChannelId::Input(r), mute)?;
        } else {
            self.set_mute(ChannelId::Input(if which == 0 { l } else { r }), mute)?;
        }
        Ok(())
    }

    /// Sets input pair `pair`'s solo, respecting its link state.
    fn set_input_solo(&mut self, pair: usize, which: usize, solo: bool) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.input_pair_linked(pair) {
            self.set_solo(ChannelId::Input(l), solo)?;
            self.set_solo(ChannelId::Input(r), solo)?;
        } else {
            self.set_solo(ChannelId::Input(if which == 0 { l } else { r }), solo)?;
        }
        Ok(())
    }

    // ── Playback pair stereo link (bus grouping) ────────────────────
    // Same concept again, but there's no new settings field: a playback
    // pair's link state IS `PlaybackChannel::split`, inverted (`split`
    // already exists, per-channel, and is the real state
    // `RmeDevice::set_stereo_split` writes — duplicating it in
    // `DeviceSettings` would just risk the two disagreeing).

    /// Whether playback pair `pair` is currently linked — reads
    /// `!playbacks[pair*2].split` (both channels of a pair always carry
    /// the same flag, see `set_stereo_split`'s own implementations).
    fn playback_linked(&self, pair: usize) -> bool {
        !self
            .playbacks()
            .get(pair * 2)
            .map(|p| p.split)
            .unwrap_or(false)
    }

    /// The balance playback channel `idx` plays into `output` at: its
    /// pair's, as the pair's strip (its left channel) holds it, while the
    /// pair is linked; a split channel has none. Either channel of a pair
    /// asks the same, whichever one a backend stored it on.
    fn playback_balance(&self, idx: usize, output: usize) -> i8 {
        let pair = idx / 2;
        if !self.playback_linked(pair) {
            return 0;
        }
        self.pan(ChannelId::Playback(pair * 2), output).unwrap_or(0)
    }

    /// Sets whether playback pair `pair` is linked — the inverse of
    /// [`Self::set_stereo_split`], which already writes both channels.
    fn set_playback_linked(&mut self, pair: usize, linked: bool) -> Result<(), Error> {
        self.set_stereo_split(pair * 2, !linked)?;
        if linked {
            self.align_pair(ChannelId::Playback(pair * 2), ChannelId::Playback(pair * 2 + 1))?;
        }
        Ok(())
    }

    /// Sets playback pair `pair`'s volume (into submix `output`),
    /// respecting its link state — same shape as
    /// [`Self::set_input_volume`].
    fn set_playback_volume(
        &mut self,
        pair: usize,
        which: usize,
        output: usize,
        volume: f32,
    ) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.playback_linked(pair) {
            self.set_volume(ChannelId::Playback(l), output, volume)?;
            self.set_volume(ChannelId::Playback(r), output, volume)?;
        } else {
            self.set_volume(ChannelId::Playback(if which == 0 { l } else { r }), output, volume)?;
        }
        Ok(())
    }

    /// Sets playback pair `pair`'s mute, respecting its link state.
    fn set_playback_mute(&mut self, pair: usize, which: usize, mute: bool) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.playback_linked(pair) {
            self.set_mute(ChannelId::Playback(l), mute)?;
            self.set_mute(ChannelId::Playback(r), mute)?;
        } else {
            self.set_mute(ChannelId::Playback(if which == 0 { l } else { r }), mute)?;
        }
        Ok(())
    }

    /// Sets playback pair `pair`'s solo, respecting its link state.
    fn set_playback_solo(&mut self, pair: usize, which: usize, solo: bool) -> Result<(), Error> {
        let (l, r) = (pair * 2, pair * 2 + 1);
        if self.playback_linked(pair) {
            self.set_solo(ChannelId::Playback(l), solo)?;
            self.set_solo(ChannelId::Playback(r), solo)?;
        } else {
            self.set_solo(ChannelId::Playback(if which == 0 { l } else { r }), solo)?;
        }
        Ok(())
    }

    /// A trim before the mixer. The Babyface has none: what the captures
    /// called "trim" were the AN pair's Main cells at a fixed level, which
    /// put the mics in Main whatever the faders said (2026-11-09). Every
    /// backend refuses it; TotalMix's Trim is Trim Gains, on the faders.
    fn set_trim(&mut self, idx: usize, db: f32) -> Result<(), Error> {
        let _ = db;
        Err(Error::InvalidChannel(format!("Input {idx} has no trim before the mixer")))
    }

    /// Whether `channel` has a phase switch here. AN1-4 by default; the
    /// ALSA backend with the 2026-11-07 driver, every input and playback.
    fn can_phase(&self, channel: ChannelId) -> bool {
        matches!(channel, ChannelId::Input(i) if i < 4)
    }

    /// Phase Ø invert for a playback channel (its crosspoints negated).
    fn set_playback_phase(&mut self, idx: usize, invert: bool) -> Result<(), Error> {
        let _ = (idx, invert);
        Err(Error::InvalidChannel("Playback phase is not supported on this backend".into()))
    }

    /// Phase Ø invert for an input (its crosspoints negated).
    fn set_phase(&mut self, idx: usize, invert: bool) -> Result<(), Error> {
        let _ = (idx, invert);
        Err(Error::InvalidChannel(
            "Phase is not supported on this backend".into(),
        ))
    }

    /// Whether the driver opens the FX send and return (Reverb and Echo
    /// run on the host, manual 21.6): its PCM carries the FX pair past the
    /// 12 audio channels and it has the "FX Return" controls.
    fn has_fx(&self) -> bool {
        false
    }

    /// Whether the driver can arm the FX pair (`set_fx_engine`).
    fn has_fx_engine(&self) -> bool {
        false
    }

    /// Arms the unit's FX pair while Reverb or Echo runs (issue #16: without
    /// it the unit ignores the FX return). A no-op where there is none.
    fn set_fx_engine(&mut self, on: bool) -> Result<(), Error> {
        let _ = on;
        Ok(())
    }

    /// The FX return (the host's Reverb/Echo, mixer source idx 24/25)
    /// into output pair `pair`, `volume` 0..2 on the crosspoints' scale
    /// (1 = 0 dB, as `set_volume`).
    fn set_fx_return(&mut self, pair: usize, volume: f32) -> Result<(), Error> {
        let _ = (pair, volume);
        Err(Error::InvalidChannel(
            "FX return is not supported on this backend".into(),
        ))
    }

    /// An offset on the FX send as the unit gets it, not kept in the
    /// settings: the send follows a fader (the channel's highest submix, or
    /// the Main mix, BF §21.7) while `fx_send_db` stays the user's knob.
    /// The unit gets `fx_send_db + trim_db`, held to the send's range;
    /// `f32::NEG_INFINITY` is off. Backends without a send ignore it.
    fn set_fx_send_trim(&mut self, trim_db: f32) -> Result<(), Error> {
        let _ = trim_db;
        Ok(())
    }

    /// Whether this backend can invert an output's phase and run M/S on an
    /// output pair (the Babyface driver's "Raw Crosspoint", 2026-11-09).
    fn can_output_proc(&self) -> bool {
        false
    }

    /// Inverts output channel `ch`'s phase (index = `ChannelId::Output`).
    fn set_output_phase(&mut self, ch: usize, invert: bool) -> Result<(), Error> {
        let _ = (ch, invert);
        Err(Error::InvalidChannel(
            "Output phase needs the Babyface driver from 2026-11-09".into(),
        ))
    }

    /// M/S processing on output pair `pair`.
    fn set_output_ms(&mut self, pair: usize, on: bool) -> Result<(), Error> {
        let _ = (pair, on);
        Err(Error::InvalidChannel(
            "Output M/S needs the Babyface driver from 2026-11-09".into(),
        ))
    }

    /// Whether `channel`'s stereo pair can run M/S (TotalMix's M/S Proc on
    /// a stereo input or playback; the driver's "Raw Crosspoint").
    fn can_source_ms(&self, channel: crate::channel::ChannelId) -> bool {
        let _ = channel;
        false
    }

    /// M/S on the stereo source `channel` belongs to: its left channel
    /// becomes the mid, its right the side, into every output.
    fn set_source_ms(&mut self, channel: crate::channel::ChannelId, on: bool) -> Result<(), Error> {
        let _ = (channel, on);
        Err(Error::InvalidChannel(
            "M/S needs the Babyface driver from 2026-11-09".into(),
        ))
    }

    /// Whether `channel`'s pair has a stereo width here: a linked stereo
    /// input or playback pair (TotalMix gives every one a width knob).
    fn can_source_width(&self, channel: crate::channel::ChannelId) -> bool {
        let _ = channel;
        false
    }

    /// The width of the stereo pair `channel` belongs to, -1..+1 (see
    /// [`DeviceSettings::source_width`]), into every output it plays in.
    fn set_source_width(&mut self, channel: crate::channel::ChannelId, width: f32) -> Result<(), Error> {
        let _ = (channel, width);
        Err(Error::InvalidChannel(
            "Width needs the Babyface driver's crossed cells (2026-11-07)".into(),
        ))
    }

    /// Whether output channel `ch` has an EQ (the driver's "DSP Block",
    /// 2026-11-09; Main, Phones and AS 1/2, the outputs TotalMix's
    /// captures show).
    fn can_output_eq(&self, ch: usize) -> bool {
        let _ = ch;
        false
    }

    /// Sets output channel `ch`'s whole EQ (3 bands and the low cut).
    fn set_output_eq(&mut self, ch: usize, eq: crate::channel::InputEq) -> Result<(), Error> {
        let _ = (ch, eq);
        Err(Error::InvalidChannel(
            "Output EQ needs the Babyface driver from 2026-11-09".into(),
        ))
    }

    /// Whether `channel` has an FX send of its own beside AN1/2's (the
    /// driver's "<pair> FX Send Volume", issue #14). A pair's two channels
    /// share it, as AN1 and AN2 share theirs.
    fn has_source_fx_send(&self, channel: crate::channel::ChannelId) -> bool {
        let _ = channel;
        false
    }

    /// Sets the FX send of the pair `channel` belongs to, in dB (-65..0,
    /// -65 = off), kept in `settings().source_fx_send`.
    fn set_source_fx_send(&mut self, channel: crate::channel::ChannelId, db: f32) -> Result<(), Error> {
        let _ = (channel, db);
        Err(Error::InvalidChannel(
            "This FX send needs the driver from 2026-11-09".into(),
        ))
    }

    /// FX send level in dB (-65..0, 0 = max send).
    fn set_fx_send(&mut self, db: f32) -> Result<(), Error> {
        let _ = db;
        Err(Error::InvalidChannel(
            "FX send is not supported on this backend".into(),
        ))
    }

    /// Stereo-split a playback strip (its AN1/2 crosspoints are
    /// rewritten to 0x2000/0x0000 split-mono instead of the -6 dB pair).
    fn set_stereo_split(&mut self, pb: usize, split: bool) -> Result<(), Error> {
        let _ = (pb, split);
        Err(Error::InvalidChannel(
            "Stereo split is not supported on this backend".into(),
        ))
    }

    /// Stereo width (-1..+1, 0 = normal).
    fn set_width(&mut self, width: f32) -> Result<(), Error> {
        let _ = width;
        Err(Error::InvalidChannel(
            "Width is not supported on this backend".into(),
        ))
    }

    /// Ref level code (Instr 3/4) as the raw state word (bits 2-3).
    fn set_ref_level(&mut self, idx: usize, raw: u16) -> Result<(), Error> {
        let _ = (idx, raw);
        Err(Error::InvalidChannel(
            "Ref level is not supported on this backend".into(),
        ))
    }

    // ── Hardware DSP EQ (3-band + low cut, analog inputs only) ──────
    // The device DSP does the biquad math — these just carry the plain
    // freq/Q/gain/type parameters, see `channel::InputEq`.

    /// Enable/bypass an input's EQ strip. Errors if the input has no
    /// EQ (only the 4 analog inputs do — see [`InputChannel::eq`]).
    fn set_eq_enabled(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        let _ = (idx, on);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set one band's filter type (`band` is 0-2 for bands 1-3).
    fn set_eq_band_type(
        &mut self,
        idx: usize,
        band: usize,
        band_type: EqBandType,
    ) -> Result<(), Error> {
        let _ = (idx, band, band_type);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set one band's center/corner frequency (20-20000 Hz).
    fn set_eq_band_freq(&mut self, idx: usize, band: usize, freq_hz: u16) -> Result<(), Error> {
        let _ = (idx, band, freq_hz);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set one band's Q factor (0.05-10.0).
    fn set_eq_band_q(&mut self, idx: usize, band: usize, q: f32) -> Result<(), Error> {
        let _ = (idx, band, q);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set one band's gain (-24.0..+24.0 dB).
    fn set_eq_band_gain(&mut self, idx: usize, band: usize, gain_db: f32) -> Result<(), Error> {
        let _ = (idx, band, gain_db);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set the low-cut filter's corner frequency (20-20000 Hz).
    fn set_eq_low_cut_freq(&mut self, idx: usize, freq_hz: u16) -> Result<(), Error> {
        let _ = (idx, freq_hz);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set the low-cut filter's slope (6, 12, 18, or 24 dB/octave).
    fn set_eq_low_cut_slope(&mut self, idx: usize, slope_db_oct: u8) -> Result<(), Error> {
        let _ = (idx, slope_db_oct);
        Err(Error::InvalidChannel(
            "EQ is not supported on this backend".into(),
        ))
    }

    /// Set input sensitivity (+4dBu / -10dBV). Errors if the input
    /// type has no sensitivity switch (only Instrument inputs do).
    fn set_sensitivity(&mut self, idx: usize, sensitivity: Sensitivity) -> Result<(), Error>;

    // NOTE: `set_spdif_enabled`/`_emphasis`/`_professional` lived here
    // until 2026-09-12. They mapped to `IEC958 Switch`/`Emphasis`/`Pro
    // Mask` — generic controls `snd-usb-audio` synthesizes for any
    // class-compliant device with an S/PDIF endpoint, *not* Babyface
    // Pro features: RME's own Fireface USB Settings dialog for this
    // card exposes no such options (see `PROTOCOL.md`'s captured UI
    // reference), and the strings never appear in any Windows capture.
    // The card's real optical/SPDIF control is `set_optical_out_format`
    // below. If a future device profile genuinely needs AES channel
    // status (the bigger Fireface units do), add it back deliberately
    // rather than resurrecting these.

    /// Set the sample clock source. `source` must be one of
    /// `DeviceSettings::clock_sources`; errors otherwise.
    fn set_clock_source(&mut self, source: &str) -> Result<(), Error>;

    // ── Scene / snapshot ────────────────────────────────────────

    /// Read the full hardware state into a [`Scene`].
    fn capture_scene(&self) -> Scene;

    /// Apply a previously captured [`Scene`] to the hardware.
    fn apply_scene(&mut self, scene: &Scene) -> Result<(), Error>;

    /// Restore the auto-saved scene when the device opens. A backend that
    /// reads its mixer state back from the hardware must not re-apply the
    /// saved volumes over it: the unit is the source of truth, and a stale
    /// snapshot would silently move every fader. Such a backend overrides
    /// this to restore only app-level state (channel names). The ones with
    /// no readback keep the default and re-apply everything.
    fn restore_auto_scene(&mut self, scene: &Scene) -> Result<(), Error> {
        self.apply_scene(scene)
    }

    // ── Polling ─────────────────────────────────────────────────

    /// Process pending ALSA events (e.g. hardware state changes).
    /// Should be called periodically from the UI event loop.
    fn poll_events(&mut self) -> Result<(), Error>;

    // ── Meters ───────────────────────────────────────────────────

    /// Current input meter levels (0.0-1.0 of full scale, one entry per
    /// hardware input channel), or `None` if the backend provides no
    /// meters (e.g. the ALSA class-compliant path).
    ///
    /// Draining: each call returns the levels accumulated since the
    /// previous call, so the UI should poll it **once per tick** (not
    /// once per channel) — a second call in the same tick returns
    /// zeros.
    fn meters(&self) -> Option<Vec<f32>> {
        None
    }

    /// The FX send's peaks (left, right) as of the last `meters` call, or
    /// `None` where the backend doesn't carry the FX pair.
    fn fx_meters(&self) -> Option<[f32; 2]> {
        None
    }
}

/// The pair a channel's stereo width is kept under (issue #7): its FX
/// send pair's name ([`source_send_pair`]), and "AN1/2" for AN1 and AN2.
pub fn source_width_pair(channel: crate::channel::ChannelId) -> Option<&'static str> {
    match channel {
        crate::channel::ChannelId::Input(0 | 1) => Some("AN1/2"),
        other => source_send_pair(other),
    }
}

/// Whether `channel` plays as part of a linked stereo pair here: an input
/// pair linked, or a playback pair not split. A split pair's channels are
/// two mono strips and have no width.
pub fn plays_as_stereo_pair<D: RmeDevice + ?Sized>(dev: &D, channel: crate::channel::ChannelId) -> bool {
    use crate::channel::ChannelId;
    match channel {
        ChannelId::Input(i) => dev.input_pair_linked(i / 2),
        ChannelId::Playback(i) => dev.playbacks().get(i).is_some_and(|p| !p.split),
        ChannelId::Output(_) => false,
    }
}

/// The Babyface driver's FX-send pair for a channel other than AN1/2
/// (issue #14: "<pair> FX Send Volume"), or `None` (AN1/2, whose send is
/// [`RmeDevice::set_fx_send`], or an output). Two channels share a pair:
/// IN3 and IN4, AS1 and AS2, each ADAT pair, each playback pair.
pub fn source_send_pair(channel: crate::channel::ChannelId) -> Option<&'static str> {
    use crate::channel::ChannelId;
    match channel {
        ChannelId::Input(2 | 3) => Some("AN3/4"),
        ChannelId::Input(4 | 5) => Some("AS1/2"),
        ChannelId::Input(6 | 7) => Some("ADAT3/4"),
        ChannelId::Input(8 | 9) => Some("ADAT5/6"),
        ChannelId::Input(10 | 11) => Some("ADAT7/8"),
        ChannelId::Playback(i) if i < 12 => Some(["PB1", "PB2", "PB3", "PB4", "PB5", "PB6"][i / 2]),
        _ => None,
    }
}

/// The FX send the unit gets: the knob plus the follow offset, held to the
/// send's range (−65 or under is off).
pub fn fx_send_on_unit(knob_db: f32, trim_db: f32) -> f32 {
    if knob_db <= -65.0 || trim_db == f32::NEG_INFINITY {
        return -65.0;
    }
    (knob_db + trim_db).clamp(-65.0, 0.0)
}

/// The two channel pans of a linked pair panned to `pan`: see
/// [`RmeDevice::set_input_pair_pan`].
pub fn linked_pair_pans(pan: i8) -> (i8, i8) {
    let p = i16::from(pan.clamp(-100, 100));
    ((p - 100).max(-100) as i8, (p + 100).min(100) as i8)
}

/// The inverse of [`linked_pair_pans`]; 0 for channel pans it cannot
/// have produced.
pub fn linked_pair_pan(left: i8, right: i8) -> i8 {
    let (l, r) = (i16::from(left), i16::from(right));
    if r - l < 100 {
        0
    } else if r < 100 {
        (r - 100) as i8
    } else {
        (l + 100) as i8
    }
}

/// The cells one source sends into an output pair, as a matrix: `a` its
/// left (or only) channel into the L side, `b` that channel into the R
/// side, `c` its right channel into the L side, `d` into the R side.
/// Values are the crosspoints' raw scale (signed: phase is a negative
/// value).
pub type SourceCells = [i32; 4];

/// TotalMix's M/S Proc on a stereo source (Windows session 3, Instr 3/4 at
/// -32 dB: own L, L->R and R->L at +v/2, own R at -v/2): the source's two
/// channels become M = (L+R)/2 and S = (L-R)/2, then go where its left and
/// right went. With [`SourceCells`]' a (L->outL), b (L->outR), c (R->outL),
/// d (R->outR): L->outL (a+c)/2, R->outL (a-c)/2, L->outR (b+d)/2,
/// R->outR (b-d)/2.
pub fn source_ms_cells(m: SourceCells) -> SourceCells {
    let [a, b, c, d] = m;
    [(a + c) / 2, (b + d) / 2, (a - c) / 2, (b - d) / 2]
}

/// TotalMix's output processing on one source's cells (Windows session 3):
/// M/S first, L' = (L+R)/2 and R' = (L-R)/2, then each side's phase,
/// every cell of that side negated.
pub fn output_proc_cells(m: SourceCells, phase_l: bool, phase_r: bool, ms: bool) -> SourceCells {
    let [mut a, mut b, mut c, mut d] = m;
    if ms {
        (a, b) = ((a + b) / 2, (a - b) / 2);
        (c, d) = ((c + d) / 2, (c - d) / 2);
    }
    if phase_l {
        a = -a;
        c = -c;
    }
    if phase_r {
        b = -b;
        d = -d;
    }
    [a, b, c, d]
}

#[cfg(test)]
mod output_proc_tests {
    use super::*;

    #[test]
    fn ms_and_phase_write_what_totalmix_writes() {
        // Instr 3/4 at -32 dB (0x00CC) into a pair, M/S on: Windows
        // session 3's cells (own L, L->R, R->L = +0x66; own R negative).
        let [a, b, c, d] = output_proc_cells([0xCC, 0, 0, 0xCC], false, false, true);
        assert_eq!((a, b, c), (0x66, 0x66, 0x66));
        assert_eq!(d, -0x66);
        // The same capture is M/S Proc on the source (TotalMix's switch was
        // on Instr 3/4): the source form gives the same four cells.
        assert_eq!(source_ms_cells([0xCC, 0, 0, 0xCC]), [0x66, 0x66, 0x66, -0x66]);
        // A source panned into one side only keeps its channels apart.
        assert_eq!(source_ms_cells([0x100, 0, 0, 0]), [0x80, 0, 0x80, 0]);
        // Phones ØL: every source's L cell negated, the R side untouched.
        assert_eq!(output_proc_cells([0x2000, 0, 0x100, 0x2000], true, false, false), [-0x2000, 0, -0x100, 0x2000]);
        // Nothing on: the driver's values as they are.
        assert_eq!(output_proc_cells([5, 6, 7, 8], false, false, false), [5, 6, 7, 8]);
    }

    #[test]
    fn output_proc_settings_stay_out_of_old_scenes() {
        let mut s = <crate::mock::MockBabyfacePro as RmeDevice>::open().unwrap().settings().clone();
        s.keep_output_phase(3, true);
        s.keep_output_ms(1, true);
        assert!(s.output_phase_of(3) && !s.output_phase_of(2) && s.output_ms_of(1));
        s.keep_output_phase(3, false);
        s.keep_output_ms(1, false);
        assert!(s.output_phase.is_empty() && s.output_ms.is_empty(), "all off saves as nothing");
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("output_phase") && !json.contains("output_ms"));
    }
}
