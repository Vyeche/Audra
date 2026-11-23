//! Mock implementation of [`RmeDevice`] for development and testing.
//!
//! [`MockBabyfacePro`] simulates a Babyface Pro FS entirely in memory,
//! without any ALSA or hardware interaction.
//!
//! Use `--mock` to run the TUI/GUI without a physical device:
//! ```bash
//! cargo run -p tuxmix-gui -- --mock
//! ```

use rand::Rng;

use crate::channel::*;
use crate::device::{DeviceSettings, RmeDevice};
use crate::error::Error;
use crate::profiles::babyface_pro::PROFILE;
use crate::scene::Scene;

// ── Main struct ─────────────────────────────────────────────────

/// One level write the simulation device was asked for, in the order it
/// happened: what a listener would have heard, had there been one. The
/// value is the one that was asked for (before the mock's own range
/// limits), so a test can see a level that was written loud.
#[derive(Clone, Debug, PartialEq)]
pub enum MockWrite {
    /// A channel's send into an output pair: (channel, output pair, level).
    Volume(ChannelId, usize, f32),
    /// An output's own master: (output channel, level).
    OutputVolume(usize, f32),
    /// A preamp gain: (input, dB).
    Gain(usize, u32),
    /// A mic input's PAD (−11 dB, the manual's figure): (input, on). Logged so a test can replay
    /// a gain and PAD change in order (a preset turning PAD off).
    Pad(usize, bool),
    /// A mic input's 48V: (input, on). The real unit writes it first of a
    /// scene's preamps (OpenCode's job 55: missing, a replayed log proved
    /// less than it said).
    Phantom(usize, bool),
    /// A mute: (channel, muted).
    Mute(ChannelId, bool),
    /// A raw crosspoint write — what the Control Room's overrides (Dim,
    /// Talkback, Cue, External Input) put on the card: (channel, output
    /// pair, level, pan).
    RawCrosspoint(ChannelId, usize, f32, i8),
}

/// The level the mock holds: the unit's scale runs from silence to +6 dB
/// (0.0 to 2.0), as the app's and the card's do (job 15). A number that is
/// not one is silence.
fn safe_level(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 2.0) } else { 0.0 }
}

/// A simulated Babyface Pro FS that works without any hardware.
///
/// All state is kept in memory. Every operation succeeds immediately.
/// Use this to develop or test UI code without a physical RME device.
/// Topology (channel counts/names/types) comes from the same
/// [`PROFILE`] as [`crate::BabyfacePro`], so the two can't drift apart
/// the way the old hand-duplicated const tables did.
pub struct MockBabyfacePro {
    model_name: String,
    inputs: Vec<InputChannel>,
    playbacks: Vec<PlaybackChannel>,
    outputs: Vec<OutputChannel>,
    settings: DeviceSettings,
    input_meters: Vec<f32>,
    playback_meters: Vec<f32>,
    tick: u64,
    /// Whether this mock answers as a device that speaks RME's own protocol
    /// (see `RmeDevice::protocol_features`). Seeded from `TUXMIX_MOCK_CC`
    /// (any value, even empty) so the gated UI can be looked at without a
    /// class-compliant card, and flippable at runtime for tests.
    protocol_features: bool,
    /// Whether this mock answers as a device that is still plugged in
    /// (see `RmeDevice::is_connected`). Seeded from
    /// `TUXMIX_MOCK_DISCONNECT` so the disconnected chip can be looked at
    /// without unplugging anything, and flippable at runtime for tests.
    connected: bool,
    /// The PAD switch refuses every write (`refuse_pad`): a unit that says
    /// no, for the app's failure paths.
    refusing_pad: bool,
    /// Crosspoints written with `write_crosspoint_raw`: the hardware side,
    /// keyed by (source channel, output pair). A model write (`set_volume`,
    /// `set_pan`) rewrites the real crosspoint, so it drops the cell here;
    /// `set_mute`/`set_solo` do too, because the ALSA backend rewrites the
    /// affected crosspoints from the model. Lets tests check an override
    /// separately from the model.
    hardware: std::collections::HashMap<(ChannelId, usize), (f32, i8)>,
    /// See `RmeDevice::set_app_handles_dim_button`.
    app_handles_dim: bool,
    /// An unreported DIM toggle, for `RmeDevice::take_dim_button_toggle`.
    dim_toggle_pending: bool,
    /// The offset on the FX send (`RmeDevice::set_fx_send_trim`).
    fx_send_trim_db: f32,
    /// The output pair Mono folds (`RmeDevice::set_mono_out`).
    mono_out: Option<usize>,
    /// Every level write so far, when the log is on (off by default: the
    /// mock does nothing extra until a test asks for it).
    writes: Option<Vec<MockWrite>>,
    /// While on, a level or pan set on a crosspoint goes to the model only
    /// (`RmeDevice::hold_card`): the caller writes the card itself, composed.
    card_held: bool,
}

impl MockBabyfacePro {
    /// Starts the log of every level write (off by default). A test that
    /// wants to know what a listener would have heard turns it on, does
    /// its thing, and reads [`Self::writes`].
    pub fn log_writes(&mut self, on: bool) {
        self.writes = if on { Some(Vec::new()) } else { None };
    }

    /// The level writes so far, in order (empty when the log is off).
    pub fn writes(&self) -> &[MockWrite] {
        self.writes.as_deref().unwrap_or(&[])
    }

    /// Throws the log away (the recording goes on).
    pub fn clear_writes(&mut self) {
        if let Some(writes) = &mut self.writes {
            writes.clear();
        }
    }

    /// One write, into the log when it is on.
    fn log(&mut self, w: MockWrite) {
        if let Some(writes) = &mut self.writes {
            writes.push(w);
        }
    }

    /// The FX send the unit would get: the knob plus the follow offset.
    pub fn fx_send_on_unit(&self) -> f32 {
        crate::device::fx_send_on_unit(self.settings.fx_send_db.unwrap_or(-65.0), self.fx_send_trim_db)
    }

    /// The FX return into output pair `pair`, as last set.
    pub fn fx_return(&self, pair: usize) -> f32 {
        self.settings.fx_return_of(pair)
    }

    /// A press of the front panel's DIM button: reported to the application
    /// when it handles DIM, else toggles this device's own Dim, as the ALSA
    /// backend does with the driver's press count.
    pub fn press_dim_button(&mut self) {
        if self.app_handles_dim {
            self.dim_toggle_pending = !self.dim_toggle_pending;
        } else {
            self.settings.dim = !self.settings.dim;
        }
    }

    /// What the "hardware" carries at a crosspoint: the last raw write
    /// (`write_crosspoint_raw`), else the model's level and pan. Mute and
    /// solo zero the crosspoints on the real backend, so they do here too:
    /// a muted strip, or any strip other than the soloed one (and its
    /// linked partner), carries no level.
    pub fn hardware_crosspoint(&self, channel: ChannelId, output: usize) -> (f32, i8) {
        if let Some(&cell) = self.hardware.get(&(channel, output)) {
            return cell;
        }
        let pan = self.pan(channel, output).unwrap_or(0);
        if matches!(channel, ChannelId::Input(_) | ChannelId::Playback(_))
            && (self.mute(channel).unwrap_or(false) || self.soloed_elsewhere(channel))
        {
            return (0.0, pan);
        }
        (self.volume(channel, output).unwrap_or(0.0), pan)
    }

    /// Whether some strip other than `channel` (or its linked partner) is
    /// soloed. The ALSA backend zeroes every other strip's sends when a
    /// solo goes on, and spares the soloed strip and its partner.
    fn soloed_elsewhere(&self, channel: ChannelId) -> bool {
        let partner = match channel {
            ChannelId::Input(i) if self.input_pair_linked(i / 2) => {
                Some(ChannelId::Input(i ^ 1))
            }
            ChannelId::Playback(c) if self.playback_linked(c / 2) => {
                Some(ChannelId::Playback(c ^ 1))
            }
            _ => None,
        };
        self.inputs.iter().enumerate().any(|(i, c)| {
            c.solo && ChannelId::Input(i) != channel && Some(ChannelId::Input(i)) != partner
        }) || self.playbacks.iter().enumerate().any(|(c, ch)| {
            ch.solo && ChannelId::Playback(c) != channel && Some(ChannelId::Playback(c)) != partner
        })
    }

    /// Point this mock at the class-compliant personality's feature set, or
    /// back at the full one. See the field's own comment.
    pub fn set_protocol_features(&mut self, on: bool) {
        self.protocol_features = on;
    }

    /// Point this mock at "unplugged", or back. See the field's own
    /// comment.
    pub fn set_connected(&mut self, on: bool) {
        self.connected = on;
    }

    /// Make the PAD switch refuse every write, or take them again.
    pub fn refuse_pad(&mut self, on: bool) {
        self.refusing_pad = on;
    }

    fn update_meters(&mut self) {
        let mut rng = rand::rng();
        self.tick += 1;

        for v in &mut self.input_meters {
            let target = rng.random_range(0.0..0.95);
            *v += (target - *v) * 0.05;
            *v = v.clamp(0.0, 1.0);
        }
        for v in &mut self.playback_meters {
            let target = rng.random_range(0.0..0.85);
            *v += (target - *v) * 0.03;
            *v = v.clamp(0.0, 1.0);
        }
        // `TUXMIX_MOCK_CLIP=1` (or any value) has AN1 touch full scale now
        // and then, which is how the clip LED is shown and screenshotted
        // without a real overload.
        if self.tick.is_multiple_of(40) && std::env::var_os("TUXMIX_MOCK_CLIP").is_some() {
            if let Some(v) = self.input_meters.first_mut() {
                *v = 1.0;
            }
        }
    }

    pub fn input_meter(&self, idx: usize) -> f32 {
        self.input_meters.get(idx).copied().unwrap_or(0.0)
    }

    pub fn playback_meter(&self, idx: usize) -> f32 {
        self.playback_meters.get(idx).copied().unwrap_or(0.0)
    }

    pub fn input_meters(&self) -> &[f32] {
        &self.input_meters
    }

    pub fn playback_meters(&self) -> &[f32] {
        &self.playback_meters
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

impl RmeDevice for MockBabyfacePro {
    fn can_mono(&self) -> bool {
        self.protocol_features
    }

    fn mono_out(&self) -> Option<usize> {
        self.mono_out
    }

    fn set_mono_out(&mut self, out: Option<usize>) -> Result<(), Error> {
        if out.is_some() && !self.can_mono() {
            return Err(Error::MixerElementNotFound("Cross Playback Volume".into()));
        }
        self.mono_out = out;
        Ok(())
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn output_pair_count(&self) -> usize {
        PROFILE.output_pair_count()
    }

    fn protocol_features(&self) -> bool {
        self.protocol_features
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn open() -> Result<Self, Error> {
        // Same topology as the real device, phantom power off as a unit
        // comes up (critique #15: the simulator's first run started with
        // 48V on, a lived-in look; `AUDRA_MOCK_DEMO` still turns it on
        // for the pictures).
        let mut inputs = PROFILE.build_inputs();
        for (i, ch) in inputs.iter_mut().enumerate() {
            ch.phantom = false;
            // Ranges matching the kernel driver's controls (2026-08-26:
            // AN1/2 mic gain 0-65 dB, AN3/4 instrument 0-9 dB — the
            // instrument raw is dB×2 = 0-18, cap_gain34.pcap).
            ch.gain_max = match ch.channel_type {
                ChannelType::Mic => Some(65),
                ChannelType::Instrument => Some(9),
                _ => None,
            };
            ch.gain = ch.gain_max.map(|_| 0);
            // The 4 analog inputs have a hardware EQ strip (see
            // `InputChannel::eq`'s doc comment and `babyface.rs`'s
            // `eq_strip_name`, same `idx < 4` gate) — every other input
            // type stays `None`.
            ch.eq = (i < 4).then(InputEq::default);
            // Inputs start routed nowhere, as after TotalMix's "Reset
            // mix: straight playback" (see `playbacks` below).
            ch.volumes.fill(0.0);
        }
        // Each playback pair plays on its own output pair at 0 dB and
        // nowhere else, instead of every crosspoint at 0 dB: a mock that
        // looks like a real card's default mix, in the Matrix and in
        // screenshots.
        let mut playbacks = PROFILE.build_playbacks();
        for (i, ch) in playbacks.iter_mut().enumerate() {
            for (pair, v) in ch.volumes.iter_mut().enumerate() {
                *v = if pair == i / 2 { 1.0 } else { 0.0 };
            }
        }

        Ok(Self {
            model_name: format!("{} (mock)", PROFILE.model_name),
            hardware: Default::default(),
            app_handles_dim: false,
            dim_toggle_pending: false,
            fx_send_trim_db: 0.0,
            inputs,
            playbacks,
            outputs: PROFILE.build_outputs(),
            settings: DeviceSettings {
                clock_source: "Internal".into(),
                // Mocked from the real Babyface Pro FS's own enum items
                // (see `babyface.rs::attach_mixer_elements`), so the
                // Quick Control clock picker has something real to show
                // in --mock too.
                clock_sources: vec!["Internal".into(), "AutoSync".into()],
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
            input_meters: vec![0.0; PROFILE.input_count()],
            playback_meters: vec![0.0; PROFILE.output_pair_count() * 2],
            tick: 0,
            mono_out: None,
            writes: None,
            card_held: false,
            refusing_pad: false,
            // `TUXMIX_MOCK_CC=1` (or any value) turns the mock into the
            // class-compliant personality's feature set, which is how the
            // gated UI is screenshotted without unplugging anything.
            protocol_features: std::env::var_os("TUXMIX_MOCK_CC").is_none(),
            // `TUXMIX_MOCK_DISCONNECT=1` (or any value) is how the
            // disconnected chip is screenshotted without unplugging
            // anything.
            connected: std::env::var_os("TUXMIX_MOCK_DISCONNECT").is_none(),
        })
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
        match channel {
            ChannelId::Output(idx) => self.log(MockWrite::OutputVolume(idx, volume)),
            // Held: the model alone takes it, the card keeps the raw write.
            _ if self.card_held => {}
            _ => {
                self.log(MockWrite::Volume(channel, output, volume));
                self.hardware.remove(&(channel, output));
            }
        }
        let vol = safe_level(volume);
        match channel {
            ChannelId::Input(idx) => {
                let ch = self
                    .inputs
                    .get_mut(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
                ch.volumes
                    .get_mut(output)
                    .map(|v| *v = vol)
                    .ok_or_else(|| Error::InvalidChannel(format!("Output {}", output)))?;
            }
            ChannelId::Playback(idx) => {
                let ch = self
                    .playbacks
                    .get_mut(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx)))?;
                ch.volumes
                    .get_mut(output)
                    .map(|v| *v = vol)
                    .ok_or_else(|| Error::InvalidChannel(format!("Output {}", output)))?;
            }
            ChannelId::Output(idx) => {
                let ch = self
                    .outputs
                    .get_mut(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx)))?;
                ch.volume = vol;
            }
        }
        Ok(())
    }

    fn volume(&self, channel: ChannelId, output: usize) -> Result<f32, Error> {
        match channel {
            ChannelId::Input(idx) => self
                .inputs
                .get(idx)
                .and_then(|c| c.volumes.get(output).copied())
                .ok_or_else(|| Error::InvalidChannel(format!("Channel {}", idx))),
            ChannelId::Playback(idx) => self
                .playbacks
                .get(idx)
                .and_then(|c| c.volumes.get(output).copied())
                .ok_or_else(|| Error::InvalidChannel(format!("Channel {}", idx))),
            ChannelId::Output(idx) => self
                .outputs
                .get(idx)
                .map(|c| c.volume)
                .ok_or_else(|| Error::InvalidChannel(format!("Output {}", idx))),
        }
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
        if matches!(channel, ChannelId::Output(_)) || output >= self.output_pair_count() {
            return Err(Error::InvalidChannel(format!("{channel:?} into {output}")));
        }
        self.log(MockWrite::RawCrosspoint(
            channel,
            output,
            volume,
            pan,
        ));
        self.hardware
            .insert((channel, output), (safe_level(volume), pan.clamp(-100, 100)));
        Ok(())
    }

    fn set_pan(&mut self, channel: ChannelId, output: usize, pan: i8) -> Result<(), Error> {
        if !self.card_held {
            self.hardware.remove(&(channel, output));
        }
        let pan = pan.clamp(-100, 100);
        match channel {
            ChannelId::Input(idx) => {
                let ch = self
                    .inputs
                    .get_mut(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
                if output >= ch.pans.len() {
                    return Err(Error::InvalidChannel(format!("Output {}", output)));
                }
                ch.pans[output] = pan;
            }
            ChannelId::Playback(idx) => {
                let ch = self
                    .playbacks
                    .get_mut(idx)
                    .ok_or_else(|| Error::InvalidChannel(format!("Playback {}", idx)))?;
                if output >= ch.pans.len() {
                    return Err(Error::InvalidChannel(format!("Output {}", output)));
                }
                ch.pans[output] = pan;
            }
            ChannelId::Output(_) => return Err(Error::InvalidChannel("Output has no pan".into())),
        }
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
        self.log(MockWrite::Mute(channel, mute));
        let ch = self.channel_mut(channel)?;
        *ch.0 = mute;
        // Mute/unmute rewrites this channel's crosspoints from the model on
        // the ALSA backend, so an active raw override is gone.
        self.hardware
            .retain(|(raw_channel, _), _| *raw_channel != channel);
        Ok(())
    }

    fn mute(&self, channel: ChannelId) -> Result<bool, Error> {
        let ch = self.channel(channel)?;
        Ok(*ch.0)
    }

    fn set_solo(&mut self, channel: ChannelId, solo: bool) -> Result<(), Error> {
        let ch = self.channel_mut(channel)?;
        *ch.1 = solo;
        // Soloing zeroes every other strip's sends and un-solo restores
        // them from the model, so their raw overrides are gone. The ALSA
        // backend skips the soloed strip and its linked partner, so their
        // cells stay.
        let partner = match channel {
            ChannelId::Input(i) if self.input_pair_linked(i / 2) => {
                Some(ChannelId::Input(i ^ 1))
            }
            ChannelId::Playback(c) if self.playback_linked(c / 2) => {
                Some(ChannelId::Playback(c ^ 1))
            }
            _ => None,
        };
        if !matches!(channel, ChannelId::Output(_)) {
            self.hardware.retain(|(raw_channel, _), _| {
                *raw_channel == channel || Some(*raw_channel) == partner
            });
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
        inp.phantom = on;
        self.log(MockWrite::Phantom(idx, on));
        Ok(())
    }

    fn set_pad(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        if self.refusing_pad {
            return Err(Error::InvalidChannel(format!("Input {idx}: the unit refused the PAD switch")));
        }
        self.log(MockWrite::Pad(idx, on));
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
        inp.pad = on;
        Ok(())
    }

    fn set_gain(&mut self, idx: usize, gain: u32) -> Result<(), Error> {
        self.log(MockWrite::Gain(idx, gain));
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if !matches!(inp.channel_type, ChannelType::Mic | ChannelType::Instrument) {
            return Err(Error::InvalidChannel(format!(
                "Input {} has no gain control",
                idx
            )));
        }
        let clamped = inp.gain_max.map_or(gain, |max| gain.min(max));
        inp.gain = Some(clamped);
        Ok(())
    }

    /// Refused, as on the hardware (2026-11-09): the Babyface has no trim
    /// before the mixer; TotalMix's Trim is Trim Gains.
    fn set_trim(&mut self, idx: usize, _db: f32) -> Result<(), Error> {
        Err(Error::InvalidChannel(format!(
            "Input {idx} has no trim before the mixer on the Babyface: use Trim Gains"
        )))
    }

    fn set_sensitivity(&mut self, idx: usize, sensitivity: Sensitivity) -> Result<(), Error> {
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {}", idx)))?;
        if inp.channel_type != ChannelType::Instrument {
            return Err(Error::InvalidChannel(format!(
                "Input {} has no sensitivity switch",
                idx
            )));
        }
        inp.sensitivity = Some(sensitivity);
        Ok(())
    }


    fn set_pitch(&mut self, pitch_percent: f32) -> Result<(), Error> {
        self.settings.pitch_percent = pitch_percent.clamp(-5.0, 5.0);
        Ok(())
    }

    fn set_sample_rate(&mut self, rate: u32) -> Result<(), Error> {
        self.settings.sample_rate = rate;
        Ok(())
    }

    /// `out` is the output pair, as the unit's switch is: both its
    /// channels where outputs are two per pair, as the ALSA backend sets
    /// them (the mock set channel `out`, the wrong one past the first pair).
    fn set_loopback(&mut self, out: usize, on: bool) -> Result<(), Error> {
        let pairs = self.output_pair_count();
        let channels: Vec<usize> = if self.outputs.len() == 2 * pairs { vec![out * 2, out * 2 + 1] } else { vec![out] };
        if out >= pairs {
            return Err(Error::InvalidChannel(format!("Output pair {out}")));
        }
        for ch in channels {
            if let Some(o) = self.outputs.get_mut(ch) {
                o.loopback = on;
            }
        }
        Ok(())
    }

    fn set_an12(&mut self, on: bool) -> Result<(), Error> {
        self.settings.an12 = on;
        Ok(())
    }

    fn set_ms_proc(&mut self, on: bool) -> Result<(), Error> {
        self.settings.ms_proc = on;
        Ok(())
    }

    fn set_dim(&mut self, on: bool) -> Result<(), Error> {
        self.settings.dim = on;
        Ok(())
    }

    fn set_eq_for_record(&mut self, on: bool) -> Result<(), Error> {
        self.settings.eq_for_record = on;
        Ok(())
    }

    fn set_optical_out_format(&mut self, spdif: bool) -> Result<(), Error> {
        self.settings.optical_out_spdif = spdif;
        Ok(())
    }

    fn set_cue(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        if idx >= self.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output {idx}")));
        }
        for out in self.outputs.iter_mut() {
            out.cue = false;
        }
        if on {
            if let Some(l) = self.outputs.get_mut(idx * 2) {
                l.cue = true;
            }
            if let Some(r) = self.outputs.get_mut(idx * 2 + 1) {
                r.cue = true;
            }
        }
        Ok(())
    }

    fn set_input_link(&mut self, linked: bool) -> Result<(), Error> {
        self.settings.input_link = linked;
        Ok(())
    }

    fn set_phase(&mut self, idx: usize, invert: bool) -> Result<(), Error> {
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.phase = invert;
        Ok(())
    }

    fn can_phase(&self, channel: ChannelId) -> bool {
        self.protocol_features && !matches!(channel, ChannelId::Output(_))
    }

    fn set_playback_phase(&mut self, idx: usize, invert: bool) -> Result<(), Error> {
        let pb = self
            .playbacks
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Playback {idx}")))?;
        pb.phase = invert;
        Ok(())
    }

    fn can_output_proc(&self) -> bool {
        self.protocol_features
    }

    fn can_output_eq(&self, ch: usize) -> bool {
        self.protocol_features && ch < 6
    }

    fn can_source_ms(&self, channel: ChannelId) -> bool {
        self.protocol_features && (matches!(channel, ChannelId::Input(0 | 1)) || crate::device::source_send_pair(channel).is_some())
    }

    fn set_source_ms(&mut self, channel: ChannelId, on: bool) -> Result<(), Error> {
        if let ChannelId::Input(0 | 1) = channel {
            return self.set_ms_proc(on);
        }
        let pair = crate::device::source_send_pair(channel)
            .filter(|_| self.protocol_features)
            .ok_or_else(|| Error::InvalidChannel(format!("No M/S for {channel:?}")))?;
        if on {
            self.settings.source_ms.insert(pair.to_string(), true);
        } else {
            self.settings.source_ms.remove(pair);
        }
        Ok(())
    }

    fn can_source_width(&self, channel: ChannelId) -> bool {
        self.protocol_features
            && crate::device::source_width_pair(channel).is_some()
            && crate::device::plays_as_stereo_pair(self, channel)
    }

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
        Ok(())
    }

    fn set_output_eq(&mut self, ch: usize, eq: crate::channel::InputEq) -> Result<(), Error> {
        if !self.can_output_eq(ch) {
            return Err(Error::InvalidChannel(format!("Output {ch} has no EQ")));
        }
        self.settings.keep_output_eq(ch, eq);
        Ok(())
    }

    fn set_output_phase(&mut self, ch: usize, invert: bool) -> Result<(), Error> {
        if !self.protocol_features || ch >= PROFILE.output_pair_count() * 2 {
            return Err(Error::InvalidChannel(format!("Output {ch}")));
        }
        self.settings.keep_output_phase(ch, invert);
        Ok(())
    }

    fn set_output_ms(&mut self, pair: usize, on: bool) -> Result<(), Error> {
        if !self.protocol_features || pair >= PROFILE.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output pair {pair}")));
        }
        self.settings.keep_output_ms(pair, on);
        Ok(())
    }

    fn has_source_fx_send(&self, channel: ChannelId) -> bool {
        self.protocol_features && crate::device::source_send_pair(channel).is_some()
    }

    fn set_source_fx_send(&mut self, channel: ChannelId, db: f32) -> Result<(), Error> {
        let pair = crate::device::source_send_pair(channel)
            .filter(|_| self.protocol_features)
            .ok_or_else(|| Error::InvalidChannel(format!("No FX send for {channel:?}")))?;
        self.settings.source_fx_send.insert(pair.to_string(), db.clamp(-65.0, 0.0));
        Ok(())
    }

    fn set_fx_send(&mut self, db: f32) -> Result<(), Error> {
        self.settings.fx_send_db = Some(db.clamp(-65.0, 0.0));
        Ok(())
    }

    fn has_fx(&self) -> bool {
        self.protocol_features
    }

    fn set_fx_send_trim(&mut self, trim_db: f32) -> Result<(), Error> {
        self.fx_send_trim_db = trim_db;
        Ok(())
    }

    fn set_fx_return(&mut self, pair: usize, volume: f32) -> Result<(), Error> {
        if pair >= PROFILE.output_pair_count() {
            return Err(Error::InvalidChannel(format!("Output pair {pair}")));
        }
        self.settings.keep_fx_return(pair, safe_level(volume));
        Ok(())
    }

    fn set_stereo_split(&mut self, pb: usize, split: bool) -> Result<(), Error> {
        if pb >= self.playbacks.len() {
            return Err(Error::InvalidChannel(format!("Playback {pb}")));
        }
        self.playbacks[pb].split = split;
        self.playbacks[pb ^ 1].split = split;
        Ok(())
    }

    fn set_ref_level(&mut self, idx: usize, raw: u16) -> Result<(), Error> {
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        if inp.channel_type != ChannelType::Instrument {
            return Err(Error::InvalidChannel(format!(
                "Input {idx} has no ref-level switch"
            )));
        }
        // One shared switch on the unit, as the real backends mirror it.
        let sensitivity = match raw {
            1 => Some(Sensitivity::Plus4dBu),
            _ => Some(Sensitivity::Minus10dBV),
        };
        for c in self.inputs.iter_mut() {
            if c.channel_type == ChannelType::Instrument {
                c.ref_level = raw;
                c.sensitivity = sensitivity;
            }
        }
        Ok(())
    }

    // ── Hardware DSP EQ (mocked: only the 4 analog inputs, same gate
    // as `babyface.rs`'s `eq_strip_name`) ────────────────────────────

    fn set_eq_enabled(&mut self, idx: usize, on: bool) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq.get_or_insert_with(InputEq::default).enabled = on;
        Ok(())
    }

    fn set_eq_band_type(
        &mut self,
        idx: usize,
        band: usize,
        band_type: EqBandType,
    ) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {band}")));
        }
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq.get_or_insert_with(InputEq::default).bands[band].band_type = band_type;
        Ok(())
    }

    fn set_eq_band_freq(&mut self, idx: usize, band: usize, freq_hz: u16) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {band}")));
        }
        let clamped = freq_hz.min(20_000);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq.get_or_insert_with(InputEq::default).bands[band].freq_hz = clamped;
        Ok(())
    }

    fn set_eq_band_q(&mut self, idx: usize, band: usize, q: f32) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {band}")));
        }
        let clamped = q.clamp(0.05, 10.0);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq.get_or_insert_with(InputEq::default).bands[band].q = clamped;
        Ok(())
    }

    fn set_eq_band_gain(&mut self, idx: usize, band: usize, gain_db: f32) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        if band >= 3 {
            return Err(Error::InvalidChannel(format!("EQ band {band}")));
        }
        let clamped = gain_db.clamp(-24.0, 24.0);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq.get_or_insert_with(InputEq::default).bands[band].gain_db = clamped;
        Ok(())
    }

    fn set_eq_low_cut_freq(&mut self, idx: usize, freq_hz: u16) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        let clamped = freq_hz.min(20_000);
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq.get_or_insert_with(InputEq::default).low_cut_freq_hz = clamped;
        Ok(())
    }

    fn set_eq_low_cut_slope(&mut self, idx: usize, slope_db_oct: u8) -> Result<(), Error> {
        if idx >= 4 {
            return Err(Error::InvalidChannel(format!("Input {idx} has no EQ")));
        }
        let inp = self
            .inputs
            .get_mut(idx)
            .ok_or_else(|| Error::InvalidChannel(format!("Input {idx}")))?;
        inp.eq
            .get_or_insert_with(InputEq::default)
            .low_cut_slope_db_oct = slope_db_oct;
        Ok(())
    }



    fn set_clock_source(&mut self, source: &str) -> Result<(), Error> {
        if !self.settings.clock_sources.iter().any(|s| s == source) {
            return Err(Error::InvalidChannel(format!(
                "Unknown clock source: {}",
                source
            )));
        }
        self.settings.clock_source = source.to_string();
        Ok(())
    }

    fn capture_scene(&self) -> Scene {
        Scene {
            name: "Untitled".into(),
            model: self.model_name.clone(),
            inputs: self.inputs.clone(),
            playbacks: self.playbacks.clone(),
            outputs: self.outputs.clone(),
            settings: self.settings.clone(),
            level_scale: Scene::LEVEL_SCALE,
        }
    }

    fn apply_scene(&mut self, scene: &Scene) -> Result<(), Error> {
        scene.check_compatible(&self.model_name)?;
        // Another number of channels would replace this unit's lists
        // (OpenCode's job 50: the quit's sync and USB's start came this way).
        scene.check_shape(self.inputs.len(), self.playbacks.len(), self.outputs.len())?;
        scene.check_lists(&self.inputs, &self.playbacks)?;
        // What the card is given, in the order the real backend gives it:
        // the mutes and solos first, so a strip that the scene mutes is
        // never heard with its level first.
        for (i, ch) in scene.inputs.iter().enumerate() {
            self.log(MockWrite::Mute(ChannelId::Input(i), ch.mute));
        }
        for (i, ch) in scene.playbacks.iter().enumerate() {
            self.log(MockWrite::Mute(ChannelId::Playback(i), ch.mute));
        }
        for (i, ch) in scene.outputs.iter().enumerate() {
            self.log(MockWrite::Mute(ChannelId::Output(i), ch.mute));
        }
        for (i, ch) in scene.inputs.iter().enumerate() {
            for (out, &v) in ch.volumes.iter().enumerate() {
                self.log(MockWrite::Volume(ChannelId::Input(i), out, v));
            }
        }
        for (i, ch) in scene.playbacks.iter().enumerate() {
            for (out, &v) in ch.volumes.iter().enumerate() {
                self.log(MockWrite::Volume(ChannelId::Playback(i), out, v));
            }
        }
        for (i, ch) in scene.outputs.iter().enumerate() {
            self.log(MockWrite::OutputVolume(i, ch.volume));
        }
        // The levels the mock holds, in the unit's range (job 15).
        let mut inputs = scene.inputs.clone();
        for ch in &mut inputs {
            for v in ch.volumes.iter_mut() {
                *v = safe_level(*v);
            }
        }
        // 48V only off, PAD only on, as the real backends (job 40).
        Scene::safe_preamps(&mut inputs, &self.inputs);
        // The preamps last, as the real backends write them: PAD on (the
        // quieter change) before the gain, each only where it changes (the
        // advisor, after a13b329: unlogged, a test of an Undo mid-PAD-glide
        // saw the sends come up with PAD still off).
        let mut preamps = Vec::new();
        for (i, (s, n)) in inputs.iter().zip(&self.inputs).enumerate() {
            if s.phantom != n.phantom {
                preamps.push(MockWrite::Phantom(i, s.phantom));
            }
            if s.pad != n.pad {
                preamps.push(MockWrite::Pad(i, s.pad));
            }
            if let (Some(g), Some(c)) = (s.gain, n.gain) {
                if g != c {
                    preamps.push(MockWrite::Gain(i, g));
                }
            }
        }
        for w in preamps {
            self.log(w);
        }
        let mut playbacks = scene.playbacks.clone();
        for ch in &mut playbacks {
            for v in ch.volumes.iter_mut() {
                *v = safe_level(*v);
            }
        }
        let mut outputs = scene.outputs.clone();
        for ch in &mut outputs {
            ch.volume = safe_level(ch.volume);
        }
        Scene::safe_outputs(&mut outputs, &self.outputs);
        self.inputs = inputs;
        self.playbacks = playbacks;
        self.outputs = outputs;
        let fx_return_now = std::mem::take(&mut self.settings.fx_return);
        let mut settings = scene.settings.clone();
        Scene::safe_settings(&mut settings, &self.settings);
        self.settings = settings;
        // A scene from before the FX return leaves the returns as they are.
        if scene.settings.fx_return.is_empty() {
            self.settings.fx_return = fx_return_now;
        }
        Ok(())
    }

    fn poll_events(&mut self) -> Result<(), Error> {
        self.update_meters();
        Ok(())
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fx_return_starts_off_and_is_kept_per_output_pair() {
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(dev.has_fx());
        assert!((0..6).all(|p| dev.fx_return(p) == 0.0), "off until raised");
        dev.set_fx_return(2, 0.25).unwrap();
        dev.set_fx_return(0, 9.0).unwrap();
        assert_eq!((dev.fx_return(0), dev.fx_return(2), dev.fx_return(1)), (2.0, 0.25, 0.0));
        assert!(dev.set_fx_return(6, 1.0).is_err(), "six output pairs");
    }

    #[test]
    fn the_dim_button_goes_to_the_app_when_it_asks_for_it() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.press_dim_button();
        assert!(dev.settings().dim, "by default the device dims itself");
        assert!(!dev.take_dim_button_toggle());
        dev.set_app_handles_dim_button(true);
        dev.press_dim_button();
        assert!(dev.settings().dim, "its own Dim is left alone");
        assert!(dev.take_dim_button_toggle());
        assert!(!dev.take_dim_button_toggle(), "reported once");
        dev.press_dim_button();
        dev.press_dim_button();
        assert!(!dev.take_dim_button_toggle(), "two presses cancel out");
    }

    #[test]
    fn a_raw_crosspoint_write_leaves_the_model_alone() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let mic = ChannelId::Input(0);
        dev.set_volume(mic, 1, 0.2).unwrap();
        dev.write_crosspoint_raw(mic, 1, 0.7, -50).unwrap();
        assert_eq!(dev.hardware_crosspoint(mic, 1), (0.7, -50));
        assert_eq!(dev.volume(mic, 1).unwrap(), 0.2, "the model keeps the user's level");
        // A model write rewrites the real crosspoint, replacing the override.
        dev.set_volume(mic, 1, 0.3).unwrap();
        assert_eq!(dev.hardware_crosspoint(mic, 1).0, 0.3);
        // Outputs have no crosspoint of their own.
        assert!(dev.write_crosspoint_raw(ChannelId::Output(0), 1, 0.5, 0).is_err());
    }

    #[test]
    fn mute_and_solo_discard_raw_crosspoint_writes() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let mic = ChannelId::Input(0);
        let pb = ChannelId::Playback(0);
        let pb_partner = ChannelId::Playback(1);
        dev.set_volume(mic, 1, 0.2).unwrap();

        // Mute rewrites this strip's crosspoints from the model.
        dev.write_crosspoint_raw(mic, 1, 0.7, -50).unwrap();
        dev.set_mute(mic, true).unwrap();
        assert_eq!(
            dev.hardware_crosspoint(mic, 1).0,
            0.0,
            "mute zeroes the crosspoint"
        );
        dev.set_mute(mic, false).unwrap();
        assert_eq!(
            dev.hardware_crosspoint(mic, 1).0,
            0.2,
            "unmute restores the model, not the override"
        );

        // Soloing zeroes the other strips, their overrides gone; the soloed
        // strip and its linked partner keep theirs.
        dev.write_crosspoint_raw(mic, 1, 0.7, -50).unwrap();
        dev.write_crosspoint_raw(pb, 0, 0.8, 0).unwrap();
        dev.write_crosspoint_raw(pb_partner, 0, 0.9, 0).unwrap();
        dev.set_solo(pb, true).unwrap();
        assert_eq!(
            dev.hardware_crosspoint(mic, 1).0,
            0.0,
            "other strips are zeroed"
        );
        assert_eq!(dev.hardware_crosspoint(pb, 0), (0.8, 0));
        assert_eq!(dev.hardware_crosspoint(pb_partner, 0), (0.9, 0));
    }

    #[test]
    fn test_open_creates_correct_number_of_channels() {
        let dev = MockBabyfacePro::open().unwrap();
        assert_eq!(dev.inputs().len(), 12);
        assert_eq!(dev.playbacks().len(), 12);
    }

    #[test]
    fn protocol_features_follows_the_flag() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_protocol_features(true);
        assert!(dev.protocol_features());
        dev.set_protocol_features(false);
        assert!(
            !dev.protocol_features(),
            "the class-compliant personality has none of RME's own protocol features"
        );
    }

    #[test]
    fn test_output_pair_count() {
        let dev = MockBabyfacePro::open().unwrap();
        assert_eq!(dev.output_pair_count(), 6);
    }

    #[test]
    fn test_each_channel_has_per_output_volumes() {
        let dev = MockBabyfacePro::open().unwrap();
        for ch in dev.inputs() {
            assert_eq!(ch.volumes.len(), 6);
            assert_eq!(ch.pans.len(), 6);
        }
        for ch in dev.playbacks() {
            assert_eq!(ch.volumes.len(), 6);
            assert_eq!(ch.pans.len(), 6);
        }
    }

    #[test]
    fn test_per_output_volume() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let before_0 = dev.volume(ChannelId::Input(0), 0).unwrap();
        let before_5 = dev.volume(ChannelId::Input(0), 5).unwrap();
        // Set volume for input 0 towards output 2 only
        dev.set_volume(ChannelId::Input(0), 2, 0.3).unwrap();
        assert!((dev.volume(ChannelId::Input(0), 2).unwrap() - 0.3).abs() < 1e-6);
        // Other outputs should be unchanged
        assert!((dev.volume(ChannelId::Input(0), 0).unwrap() - before_0).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Input(0), 5).unwrap() - before_5).abs() < 1e-6);
    }

    #[test]
    fn test_per_output_pan() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_pan(ChannelId::Playback(0), 3, 50).unwrap();
        assert_eq!(dev.pan(ChannelId::Playback(0), 3).unwrap(), 50);
        assert_eq!(dev.pan(ChannelId::Playback(0), 0).unwrap(), 0);
    }

    #[test]
    fn test_volume_clamps_to_range() {
        // The unit's scale: silence to +6 dB (job 15).
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_volume(ChannelId::Input(0), 0, 1.5).unwrap();
        assert!((dev.volume(ChannelId::Input(0), 0).unwrap() - 1.5).abs() < 1e-6, "1.5 is held");
        dev.set_volume(ChannelId::Input(0), 0, -0.5).unwrap();
        assert!((dev.volume(ChannelId::Input(0), 0).unwrap() - 0.0).abs() < 1e-6, "below silence is silence");
    }

    #[test]
    fn the_mock_holds_levels_up_to_six_decibels_above_full_scale() {
        // Job 15: the app's and the card's scale runs to +6 dB (2.0), so
        // the simulation holds it too: a fader above 0 dB reads back at
        // +6 dB, not at 0 dB.
        let mut dev = MockBabyfacePro::open().unwrap();
        let at = |dev: &MockBabyfacePro, ch: ChannelId, out: usize| dev.volume(ch, out).unwrap();
        dev.set_volume(ChannelId::Input(0), 0, 2.0).unwrap();
        assert!((at(&dev, ChannelId::Input(0), 0) - 2.0).abs() < 1e-6, "+6 dB is held");
        dev.set_volume(ChannelId::Input(0), 0, 3.0).unwrap();
        assert!((at(&dev, ChannelId::Input(0), 0) - 2.0).abs() < 1e-6, "above the top is the top");
        dev.set_volume(ChannelId::Input(0), 0, -1.0).unwrap();
        assert!((at(&dev, ChannelId::Input(0), 0) - 0.0).abs() < 1e-6, "below silence is silence");
        dev.set_volume(ChannelId::Input(0), 0, f32::NAN).unwrap();
        assert!((at(&dev, ChannelId::Input(0), 0) - 0.0).abs() < 1e-6, "a number that is not one is silence");
        // The playbacks and the outputs too.
        dev.set_volume(ChannelId::Playback(0), 0, 2.0).unwrap();
        assert!((at(&dev, ChannelId::Playback(0), 0) - 2.0).abs() < 1e-6, "+6 dB on a playback");
        dev.set_volume(ChannelId::Output(0), 0, 2.0).unwrap();
        assert!((at(&dev, ChannelId::Output(0), 0) - 2.0).abs() < 1e-6, "+6 dB on an output");
        // And the output the app reads back through `outputs()`.
        assert!((dev.outputs()[0].volume - 2.0).abs() < 1e-6, "the output holds it too");
    }

    #[test]
    fn a_scene_holds_levels_in_the_same_range() {
        // Job 15: a scene's levels are held in the unit's range, so the
        // mock never shows a level the card could not hold.
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_volume(ChannelId::Input(0), 0, 2.0).unwrap();
        let mut scene = dev.capture_scene();
        scene.inputs[0].volumes[0] = 3.0;
        scene.outputs[0].volume = f32::NAN;
        dev.apply_scene(&scene).unwrap();
        assert!((dev.volume(ChannelId::Input(0), 0).unwrap() - 2.0).abs() < 1e-6, "above the top is the top");
        assert!((dev.volume(ChannelId::Output(0), 0).unwrap() - 0.0).abs() < 1e-6, "a number that is not one is silence");
    }

    #[test]
    fn test_invalid_channel_returns_error() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let result = dev.set_volume(ChannelId::Input(99), 0, 0.5);
        assert!(result.is_err());
        let result = dev.set_volume(ChannelId::Input(0), 99, 0.5);
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_and_apply_scene() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_volume(ChannelId::Input(0), 0, 0.25).unwrap();
        dev.set_pan(ChannelId::Playback(0), 1, 50).unwrap();

        let scene = dev.capture_scene();
        assert!((scene.inputs[0].volumes[0] - 0.25).abs() < 1e-6);
        assert_eq!(scene.playbacks[0].pans[1], 50);

        let mut dev2 = MockBabyfacePro::open().unwrap();
        dev2.apply_scene(&scene).unwrap();
        assert!((dev2.volume(ChannelId::Input(0), 0).unwrap() - 0.25).abs() < 1e-6);
        assert_eq!(dev2.pan(ChannelId::Playback(0), 1).unwrap(), 50);
    }

    #[test]
    fn test_poll_events_updates_meters() {
        let mut dev = MockBabyfacePro::open().unwrap();
        for v in dev.input_meters() {
            assert_eq!(*v, 0.0);
        }
        dev.poll_events().unwrap();
        let has_movement = dev.input_meters().iter().any(|v| *v > 0.0);
        assert!(has_movement);
    }

    #[test]
    fn test_meter_ranges() {
        let mut dev = MockBabyfacePro::open().unwrap();
        for _ in 0..100 {
            dev.poll_events().unwrap();
        }
        for v in dev.input_meters() {
            assert!(*v >= 0.0 && *v <= 1.0, "Meter {} out of range", v);
        }
        for v in dev.playback_meters() {
            assert!(*v >= 0.0 && *v <= 1.0, "Meter {} out of range", v);
        }
    }

    #[test]
    fn test_output_pair_linked_by_default() {
        let dev = MockBabyfacePro::open().unwrap();
        // No explicit entry yet — every pair reads as linked (TotalMix's
        // own default), not "false"/unset.
        for pair in 0..dev.output_pair_count() {
            assert!(dev.output_linked(pair));
        }
    }

    #[test]
    fn test_linked_output_volume_moves_both_channels() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_output_volume(0, 0, 0.5).unwrap();
        assert!((dev.volume(ChannelId::Output(0), 0).unwrap() - 0.5).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Output(1), 0).unwrap() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_split_output_volume_moves_only_the_selected_channel() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_output_linked(0, false).unwrap();
        assert!(!dev.output_linked(0));
        dev.set_output_volume(0, 0, 0.5).unwrap();
        dev.set_output_volume(0, 1, 0.25).unwrap();
        assert!((dev.volume(ChannelId::Output(0), 0).unwrap() - 0.5).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Output(1), 0).unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn test_linked_output_mute_and_solo_move_both_channels() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_output_mute(2, 0, true).unwrap();
        assert!(dev.mute(ChannelId::Output(4)).unwrap());
        assert!(dev.mute(ChannelId::Output(5)).unwrap());
        dev.set_output_solo(2, 1, true).unwrap();
        assert!(dev.solo(ChannelId::Output(4)).unwrap());
        assert!(dev.solo(ChannelId::Output(5)).unwrap());
    }

    #[test]
    fn test_input_pair_linked_by_default_and_moves_both_channels() {
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(dev.input_pair_linked(1));
        dev.set_input_volume(1, 0, 0, 0.5).unwrap();
        assert!((dev.volume(ChannelId::Input(2), 0).unwrap() - 0.5).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Input(3), 0).unwrap() - 0.5).abs() < 1e-6);
        dev.set_input_mute(1, 0, true).unwrap();
        assert!(dev.mute(ChannelId::Input(2)).unwrap());
        assert!(dev.mute(ChannelId::Input(3)).unwrap());
    }

    #[test]
    fn test_linking_a_pair_gives_its_right_side_the_left_level_and_mute() {
        // The owner's AN1/2 (two mics, unlinked by default) linked by a mic
        // preset at start: AN2's send to the Phones had stayed down, so one
        // ear was silent under a fader that showed AN1's level (2026-11-06).
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(!dev.input_pair_linked(0));
        let phones = 1;
        dev.set_volume(ChannelId::Input(0), phones, 0.7).unwrap();
        dev.set_volume(ChannelId::Input(1), phones, 0.0).unwrap();
        dev.set_mute(ChannelId::Input(1), true).unwrap();
        dev.set_input_pair_linked(0, true).unwrap();
        assert!((dev.volume(ChannelId::Input(1), phones).unwrap() - 0.7).abs() < 1e-6, "both sides at the fader's level");
        assert!(!dev.mute(ChannelId::Input(1)).unwrap(), "and the left's mute");
        // Unlinking leaves them as they are.
        dev.set_input_pair_linked(0, false).unwrap();
        assert!((dev.volume(ChannelId::Input(1), phones).unwrap() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn test_linked_pairs_with_their_sides_apart_are_aligned_at_start() {
        let mut dev = MockBabyfacePro::open().unwrap();
        // IN3/4 linked (by default) but stored with its sides apart, and a
        // playback pair the same.
        assert!(dev.input_pair_linked(1) && dev.playback_linked(0));
        dev.set_volume(ChannelId::Input(2), 0, 0.5).unwrap();
        dev.set_volume(ChannelId::Input(3), 0, 0.1).unwrap();
        dev.set_volume(ChannelId::Playback(0), 1, 0.9).unwrap();
        dev.set_volume(ChannelId::Playback(1), 1, 0.0).unwrap();
        // An unlinked pair is two sources: left alone.
        dev.set_volume(ChannelId::Input(0), 0, 0.3).unwrap();
        dev.set_volume(ChannelId::Input(1), 0, 0.0).unwrap();
        let two_per_pair = dev.outputs().len() == 2 * dev.output_pair_count();
        if two_per_pair {
            dev.set_volume(ChannelId::Output(2), 0, 0.8).unwrap();
            dev.set_volume(ChannelId::Output(3), 0, 0.0).unwrap();
        }
        dev.align_linked_pairs().unwrap();
        assert!((dev.volume(ChannelId::Input(3), 0).unwrap() - 0.5).abs() < 1e-6);
        if two_per_pair {
            assert!((dev.volume(ChannelId::Output(3), 0).unwrap() - 0.8).abs() < 1e-6, "a linked output pair too");
        }
        assert!((dev.volume(ChannelId::Playback(1), 1).unwrap() - 0.9).abs() < 1e-6);
        assert_eq!(dev.volume(ChannelId::Input(1), 0).unwrap(), 0.0, "unlinked: untouched");
    }

    #[test]
    fn test_microphone_pair_is_not_linked_by_default() {
        // Linked, a pair goes left and right: two microphones start as
        // two mono channels, in the centre.
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(!dev.input_pair_linked(0));
        // Setting a later pair fills the earlier ones with their own
        // defaults, not with "linked".
        dev.set_input_pair_linked(2, false).unwrap();
        assert!(!dev.input_pair_linked(0));
        assert!(dev.input_pair_linked(1));
    }

    #[test]
    fn test_linked_input_pair_goes_left_and_right() {
        // TuxMix#23: a linked IN3/4 wrote its fader with each channel's
        // own pan, both centred, so IN3 reached both sides of the bus.
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(dev.input_pair_linked(1));
        dev.set_input_volume(1, 0, 0, 1.0).unwrap();
        assert_eq!(dev.pan(ChannelId::Input(2), 0).unwrap(), -100);
        assert_eq!(dev.pan(ChannelId::Input(3), 0).unwrap(), 100);
        assert_eq!(dev.input_pair_pan(1, 0), 0);

        dev.set_input_pair_pan(1, 0, 40).unwrap();
        assert_eq!(dev.pan(ChannelId::Input(2), 0).unwrap(), -60);
        assert_eq!(dev.pan(ChannelId::Input(3), 0).unwrap(), 100);
        assert_eq!(dev.input_pair_pan(1, 0), 40);
        // A fader move keeps the pair's pan.
        dev.set_input_volume(1, 0, 0, 0.5).unwrap();
        assert_eq!(dev.input_pair_pan(1, 0), 40);
    }

    #[test]
    fn test_linked_pair_pan_round_trips() {
        use crate::device::{linked_pair_pan, linked_pair_pans};
        for p in -100..=100i8 {
            let (l, r) = linked_pair_pans(p);
            assert_eq!(linked_pair_pan(l, r), p, "pan {p}");
        }
        // Two centred channels, as before the fix, read as 0.
        assert_eq!(linked_pair_pan(0, 0), 0);
    }

    #[test]
    fn test_relinking_an_input_pair_puts_it_left_and_right() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_input_pair_linked(1, false).unwrap();
        dev.set_pan(ChannelId::Input(2), 0, 0).unwrap();
        dev.set_pan(ChannelId::Input(3), 0, 0).unwrap();
        dev.set_input_pair_linked(1, true).unwrap();
        assert_eq!(dev.pan(ChannelId::Input(2), 0).unwrap(), -100);
        assert_eq!(dev.pan(ChannelId::Input(3), 0).unwrap(), 100);
    }

    #[test]
    fn test_linked_input_pair_phantom_pad_and_gain_move_both_channels() {
        // Previously missing: set_phantom/set_pad/set_gain had no
        // pair-aware wrapper at all (unlike volume/mute/solo above),
        // so toggling 48V on a linked strip only ever touched its own
        // channel — found 2026-09-07.
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_input_pair_linked(0, true).unwrap();
        // Normalize to off first (the mock starts off; kept so the test
        // says what it starts from).
        dev.set_phantom(0, false).unwrap();
        dev.set_phantom(1, false).unwrap();

        dev.set_input_phantom(0, true).unwrap();
        assert!(dev.inputs()[0].phantom);
        assert!(dev.inputs()[1].phantom, "phantom must mirror across a linked pair");

        dev.set_input_pad(0, true).unwrap();
        assert!(dev.inputs()[0].pad);
        assert!(dev.inputs()[1].pad, "pad must mirror across a linked pair");

        dev.set_input_gain(0, 20).unwrap();
        assert_eq!(dev.inputs()[0].gain, Some(20));
        assert_eq!(dev.inputs()[1].gain, Some(20), "gain must mirror across a linked pair");
    }

    #[test]
    fn test_split_input_pair_phantom_moves_only_the_selected_channel() {
        let mut dev = MockBabyfacePro::open().unwrap();
        // Mock's own demo default starts both mics' phantom on (see
        // `open()`'s own comment) — normalize to a known baseline first
        // so this test isn't just re-asserting that default.
        dev.set_input_pair_linked(0, false).unwrap();
        dev.set_phantom(0, false).unwrap();
        dev.set_phantom(1, false).unwrap();

        dev.set_input_phantom(0, true).unwrap();
        assert!(dev.inputs()[0].phantom);
        assert!(!dev.inputs()[1].phantom);
    }

    #[test]
    fn test_split_input_pair_moves_only_the_selected_channel() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_input_pair_linked(0, false).unwrap();
        dev.set_input_volume(0, 0, 0, 0.5).unwrap();
        dev.set_input_volume(0, 1, 0, 0.25).unwrap();
        assert!((dev.volume(ChannelId::Input(0), 0).unwrap() - 0.5).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Input(1), 0).unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn test_playback_linked_reads_split_flag_inverted() {
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(dev.playback_linked(0));
        dev.set_playback_volume(0, 0, 0, 0.5).unwrap();
        assert!((dev.volume(ChannelId::Playback(0), 0).unwrap() - 0.5).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Playback(1), 0).unwrap() - 0.5).abs() < 1e-6);

        dev.set_playback_linked(0, false).unwrap();
        assert!(!dev.playback_linked(0));
        assert!(dev.playbacks()[0].split);
        dev.set_playback_volume(0, 0, 0, 0.75).unwrap();
        dev.set_playback_volume(0, 1, 0, 0.1).unwrap();
        assert!((dev.volume(ChannelId::Playback(0), 0).unwrap() - 0.75).abs() < 1e-6);
        assert!((dev.volume(ChannelId::Playback(1), 0).unwrap() - 0.1).abs() < 1e-6);
    }

    #[test]
    fn test_mute_solo_toggle() {
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(!dev.mute(ChannelId::Input(0)).unwrap());
        assert!(!dev.solo(ChannelId::Playback(0)).unwrap());

        dev.set_mute(ChannelId::Input(0), true).unwrap();
        assert!(dev.mute(ChannelId::Input(0)).unwrap());
        assert!(!dev.mute(ChannelId::Input(1)).unwrap()); // other channel unchanged

        dev.set_solo(ChannelId::Playback(0), true).unwrap();
        assert!(dev.solo(ChannelId::Playback(0)).unwrap());

        dev.set_mute(ChannelId::Input(99), true).unwrap_err();
        dev.set_solo(ChannelId::Playback(99), true).unwrap_err();
    }

    #[test]
    fn test_mute_solo_in_scene() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_mute(ChannelId::Input(0), true).unwrap();
        dev.set_solo(ChannelId::Playback(0), true).unwrap();

        let scene = dev.capture_scene();
        assert!(scene.inputs[0].mute);
        assert!(scene.playbacks[0].solo);

        let mut dev2 = MockBabyfacePro::open().unwrap();
        dev2.apply_scene(&scene).unwrap();
        assert!(dev2.mute(ChannelId::Input(0)).unwrap());
        assert!(dev2.solo(ChannelId::Playback(0)).unwrap());
    }

    #[test]
    fn test_serialize_scene_to_json() {
        let dev = MockBabyfacePro::open().unwrap();
        let scene = dev.capture_scene();
        let json = scene.to_json().unwrap();
        let restored = Scene::from_json(&json).unwrap();
        assert_eq!(restored.inputs.len(), scene.inputs.len());
    }

    #[test]
    fn a_linked_mic_pair_takes_its_left_48v_and_pad_on_both_sides() {
        // The owner's unit: AN1's 48V on from before, AN2's off, the pair
        // linked: aligning lights both P48 lanes, as on Windows.
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_input_pair_linked(0, false).unwrap();
        dev.set_phantom(0, true).unwrap();
        dev.set_phantom(1, false).unwrap();
        dev.set_pad(0, true).unwrap();
        dev.set_input_pair_linked(0, true).unwrap();
        assert!(dev.inputs()[1].phantom, "linking gives AN2 AN1's 48V");
        assert!(dev.inputs()[1].pad, "and its PAD");
        dev.set_phantom(1, false).unwrap();
        dev.align_linked_pairs().unwrap();
        assert!(dev.inputs()[1].phantom, "the start's alignment puts it back");
    }

    #[test]
    fn test_phantom_starts_off_as_a_unit_comes_up() {
        let dev = MockBabyfacePro::open().unwrap();
        assert!(dev.inputs().iter().all(|c| !c.phantom));
    }

    #[test]
    fn test_captured_scene_is_tagged_with_model() {
        let dev = MockBabyfacePro::open().unwrap();
        let scene = dev.capture_scene();
        assert_eq!(scene.model, dev.model_name());
    }

    #[test]
    fn test_apply_scene_rejects_model_mismatch() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let mut scene = dev.capture_scene();
        scene.model = "Some Other RME Device".into();

        let err = dev.apply_scene(&scene).unwrap_err();
        assert!(matches!(err, Error::SceneModelMismatch { .. }));
    }

    #[test]
    fn test_dim_toggle() {
        let mut dev = MockBabyfacePro::open().unwrap();
        assert!(!dev.settings().dim);
        dev.set_dim(true).unwrap();
        assert!(dev.settings().dim);
    }

    #[test]
    fn test_eq_only_on_first_four_inputs() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_eq_enabled(3, true).unwrap();
        assert!(dev.inputs()[3].eq.unwrap().enabled);

        let err = dev.set_eq_enabled(4, true).unwrap_err();
        assert!(matches!(err, Error::InvalidChannel(_)));
    }

    #[test]
    fn test_eq_band_params_clamp_and_persist() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_eq_band_type(0, 1, EqBandType::Bell).unwrap();
        dev.set_eq_band_freq(0, 1, 30_000).unwrap(); // clamps to 20_000
        dev.set_eq_band_q(0, 1, 50.0).unwrap(); // clamps to 10.0
        dev.set_eq_band_gain(0, 1, 100.0).unwrap(); // clamps to 24.0

        let band = dev.inputs()[0].eq.unwrap().bands[1];
        assert_eq!(band.band_type, EqBandType::Bell);
        assert_eq!(band.freq_hz, 20_000);
        assert!((band.q - 10.0).abs() < 1e-6);
        assert!((band.gain_db - 24.0).abs() < 1e-6);

        assert!(dev.set_eq_band_type(0, 3, EqBandType::Bell).is_err());
    }

    #[test]
    fn test_apply_scene_accepts_legacy_scene_with_no_model() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let mut scene = dev.capture_scene();
        scene.model = String::new(); // simulates a scene saved before `model` existed
        scene.inputs[0].volumes[0] = 0.42;

        dev.apply_scene(&scene).unwrap();
        assert!((dev.volume(ChannelId::Input(0), 0).unwrap() - 0.42).abs() < 1e-6);
    }
}
