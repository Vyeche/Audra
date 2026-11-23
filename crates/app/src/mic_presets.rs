//! Mic presets (2026-11-04, the owner, of their Neumann TLM 107: the
//! preset "should include the AN1/2 preset ... not just for EQ, but also
//! gain"): an input's whole setup outside the mix, kept under a name and
//! loaded onto any input strip with a preamp; on a linked pair, onto both
//! sides.
//!
//! A preset holds the preamp gain, PAD, phase, 48V, and the EQ and Low Cut
//! with their on/off (an EQ preset, `eq_presets`, carries the curve alone
//! and never switches anything). Loading never switches phantom power on
//! by itself: a preset with 48V on arms the strip's 48V key and one click
//! confirms (the owner's choice; phantom can harm some mics, ribbons among
//! them). A preset with 48V off switches it off, which is always safe.
//!
//! Kept in the prefs, as many as the user saves, in the order saved.

use serde::{Deserialize, Serialize};
use tuxmix_core::channel::InputChannel;

use crate::eq_presets::EqPreset;

/// An input's setup under a name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MicPreset {
    pub name: String,
    /// Preamp gain in dB (the hardware's own 1 dB steps; held to the
    /// input's own range when loaded).
    pub gain: u32,
    pub phantom: bool,
    #[serde(default)]
    pub pad: bool,
    #[serde(default)]
    pub phase: bool,
    pub eq_on: bool,
    pub low_cut_on: bool,
    /// Whether the input's pair is one stereo strip (its STEREO): a preset
    /// saved from a linked pair links it on load (the owner, 2026-11-04:
    /// the TLM 107's preset left AN1/2 as two strips). `None` in a preset
    /// from before this was kept: the link is left as it is.
    #[serde(default)]
    pub stereo: Option<bool>,
    /// AN1 on both sides of the AN1/2 pair (the unit's AN1→2), for a
    /// preset saved on AN1 or AN2: one mono mic heard in both ears with the
    /// pair linked (the owner, 2026-11-05, of their one TLM 107: linked,
    /// AN1 alone played hard left). `None` elsewhere, and in a preset from
    /// before this was kept: left as it is.
    #[serde(default)]
    pub an12: Option<bool>,
    /// The bands and the Low Cut's frequency and slope (its own `name` is
    /// the preset's).
    pub eq: EqPreset,
}

/// The mic preset Audra loads when it starts (Audra settings → Mixer):
/// the preset by name, onto input `input` (the owner, 2026-11-05: "load
/// my preset for next time"). A name no preset has any more loads nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupPreset {
    pub input: usize,
    pub preset: String,
}

impl MicPreset {
    /// Input `ch` as it is now, under `name`; `low_cut_hz` is the Low Cut's
    /// frequency, on or not (`app::low_cut_setting`); `stereo`, whether its
    /// pair is linked; `an12`, AN1→2 where the input is AN1 or AN2.
    pub fn capture(name: String, ch: &InputChannel, low_cut_hz: u16, stereo: bool, an12: Option<bool>) -> Self {
        let eq = ch.eq.unwrap_or_default();
        Self {
            gain: ch.gain.unwrap_or(0),
            phantom: ch.phantom,
            pad: ch.pad,
            phase: ch.phase,
            eq_on: eq.enabled,
            low_cut_on: eq.low_cut_on(),
            stereo: Some(stereo),
            an12,
            eq: EqPreset::capture(name.clone(), &eq, low_cut_hz),
            name,
        }
    }
}

/// The name a new preset gets: the one typed, trimmed, else "Mic preset N"
/// for the first N no preset has.
pub fn name_for(typed: &str, taken: &[MicPreset]) -> String {
    match typed.trim() {
        "" => (1..)
            .map(|n| format!("Mic preset {n}"))
            .find(|name| taken.iter().all(|p| &p.name != name))
            .unwrap_or_default(),
        name => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(name: &str) -> MicPreset {
        MicPreset {
            name: name.into(),
            gain: 35,
            phantom: true,
            pad: false,
            phase: false,
            eq_on: true,
            low_cut_on: true,
            stereo: Some(true),
            an12: Some(true),
            eq: crate::eq_presets::factory().remove(0),
        }
    }

    #[test]
    fn a_new_preset_takes_the_typed_name_or_the_first_free_number() {
        assert_eq!(name_for("  TLM 107 talking ", &[]), "TLM 107 talking");
        assert_eq!(name_for("", &[]), "Mic preset 1");
        assert_eq!(name_for(" ", &[preset("Mic preset 1"), preset("Mic preset 3")]), "Mic preset 2");
    }

    #[test]
    fn a_preset_round_trips_through_json_and_old_files_without_pad_or_phase_load() {
        let p = preset("TLM 107 talking");
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<MicPreset>(&json).unwrap(), p);
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let o = v.as_object_mut().unwrap();
        o.remove("pad");
        o.remove("phase");
        let back: MicPreset = serde_json::from_value(v).unwrap();
        assert!(!back.pad && !back.phase);
    }
}
