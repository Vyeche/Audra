//! EQ presets (manual 21.3, docs/ui-spec.md §4.4): the settings of the
//! 3-band EQ and the Low Cut, stored and loaded on any channel with an EQ.
//!
//! - 16 user slots, shared by every channel, kept in the prefs (the manual
//!   keeps them in the workspace, not in snapshots; Audra has no
//!   workspaces, so they live with the window's own settings).
//! - A factory set of Audra's own, the manual's "Factory" list.
//! - Reset: every gain to 0 dB and the Low Cut to its lowest frequency,
//!   for no influence.
//!
//! A preset carries band types, frequencies, Q, gains and the Low Cut's
//! frequency and slope, never the EQ's or the Low Cut's on/off state: the
//! manual has a preset load "anytime" without switching anything on.

use serde::{Deserialize, Serialize};
use tuxmix_core::channel::{EqBandType, InputEq};

/// How many user slots there are (the manual's "EQ Preset 1 to 16").
pub const USER_SLOTS: usize = 16;

/// What the EQ panel's preset list loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// The manual's "Reset": no influence.
    Reset,
    /// A factory preset, by index into `factory()`.
    Factory(usize),
    /// A user slot (0-based).
    User(usize),
}

/// One band of a preset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PresetBand {
    pub band_type: EqBandType,
    pub freq_hz: u16,
    pub q: f32,
    pub gain_db: f32,
}

/// An EQ and Low Cut setting with a name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    pub name: String,
    pub bands: [PresetBand; 3],
    /// The Low Cut's frequency, used when the Low Cut is (or is later
    /// switched) on.
    pub low_cut_hz: u16,
    pub low_cut_slope: u8,
}

impl EqPreset {
    /// The channel's current EQ as a preset; `low_cut_hz` is the frequency
    /// the Low Cut has, or would have when switched on.
    pub fn capture(name: String, eq: &InputEq, low_cut_hz: u16) -> Self {
        let band = |i: usize| {
            let b = eq.bands[i];
            PresetBand {
                band_type: b.band_type,
                freq_hz: b.freq_hz,
                q: b.q,
                gain_db: b.gain_db,
            }
        };
        Self {
            name,
            bands: [band(0), band(1), band(2)],
            low_cut_hz,
            low_cut_slope: eq.low_cut_slope_db_oct,
        }
    }
}

const fn off() -> PresetBand {
    PresetBand {
        band_type: EqBandType::Off,
        freq_hz: 1000,
        q: 0.7,
        gain_db: 0.0,
    }
}

const fn bell(freq_hz: u16, gain_db: f32, q: f32) -> PresetBand {
    PresetBand {
        band_type: EqBandType::Bell,
        freq_hz,
        q,
        gain_db,
    }
}

const fn low_shelf(freq_hz: u16, gain_db: f32) -> PresetBand {
    PresetBand {
        band_type: EqBandType::LowShelf,
        freq_hz,
        q: 0.7,
        gain_db,
    }
}

const fn high_shelf(freq_hz: u16, gain_db: f32) -> PresetBand {
    PresetBand {
        band_type: EqBandType::HighShelf,
        freq_hz,
        q: 0.7,
        gain_db,
    }
}

/// Audra's factory presets: starting points, inside the manual's ranges
/// (±20 dB, Q 0.7–5, Low Cut 20–500 Hz). Names that need the Low Cut on
/// to sound as meant say so, as the manual's "Telephone (LC+EQ)" does.
pub fn factory() -> Vec<EqPreset> {
    let preset = |name: &str, bands: [PresetBand; 3], low_cut_hz: u16, low_cut_slope: u8| EqPreset {
        name: name.to_string(),
        bands,
        low_cut_hz,
        low_cut_slope,
    };
    vec![
        preset(
            "Vocal presence (LC+EQ)",
            [low_shelf(120, -2.0), bell(3000, 3.0, 1.2), high_shelf(10000, 2.0)],
            80,
            12,
        ),
        preset(
            "Speech clarity (LC+EQ)",
            [bell(250, -3.0, 1.0), bell(2500, 3.0, 1.0), high_shelf(8000, 1.5)],
            100,
            12,
        ),
        preset(
            "Podcast warmth",
            [low_shelf(150, 2.0), bell(400, -2.0, 1.4), high_shelf(9000, 2.0)],
            70,
            12,
        ),
        preset("Remove mud", [off(), bell(350, -4.0, 1.5), off()], 40, 12),
        preset("Air", [off(), off(), high_shelf(12000, 4.0)], 20, 12),
        preset("Soften sibilance", [off(), off(), bell(6500, -4.0, 3.0)], 20, 12),
        preset(
            "Acoustic guitar (LC+EQ)",
            [bell(200, -2.0, 1.0), bell(2500, 2.0, 1.0), high_shelf(10000, 2.0)],
            80,
            12,
        ),
        preset(
            "Electric guitar (LC+EQ)",
            [bell(300, -2.0, 1.0), bell(1800, 3.0, 1.5), high_shelf(6000, -2.0)],
            90,
            12,
        ),
        preset(
            "Bass",
            [low_shelf(80, 3.0), bell(700, -2.0, 1.5), bell(2500, 2.0, 2.0)],
            30,
            12,
        ),
        preset(
            "Kick",
            [bell(60, 4.0, 1.4), bell(350, -5.0, 1.5), bell(4000, 3.0, 2.0)],
            30,
            12,
        ),
        preset(
            "Telephone (LC+EQ)",
            [low_shelf(300, -12.0), bell(1500, 6.0, 1.0), high_shelf(4000, -15.0)],
            500,
            24,
        ),
        preset("Rumble cut (LC)", [off(), off(), off()], 100, 18),
        preset(
            "Loudness",
            [low_shelf(100, 4.0), off(), high_shelf(10000, 4.0)],
            20,
            12,
        ),
        preset("Mid scoop", [off(), bell(800, -4.0, 0.8), off()], 20, 12),
    ]
}

/// The extension of an exported preset file: Audra's own JSON. The
/// manual's `.tmeq` is RME's format, which is not documented.
pub const FILE_EXT: &str = "audraeq";

/// A preset file: the preset, marked with its format version so a file
/// from elsewhere is refused rather than misread.
#[derive(Serialize, Deserialize)]
struct PresetFile {
    audra_eq_preset: u32,
    #[serde(flatten)]
    preset: EqPreset,
}

const FILE_VERSION: u32 = 1;

/// Writes `preset` to `path` as an Audra EQ preset file.
pub fn write_file(path: &std::path::Path, preset: &EqPreset) -> Result<(), String> {
    let file = PresetFile {
        audra_eq_preset: FILE_VERSION,
        preset: preset.clone(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Reads an Audra EQ preset file. A file of another kind, or a later
/// format version, is refused with the reason.
pub fn read_file(path: &std::path::Path) -> Result<EqPreset, String> {
    let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let file: PresetFile =
        serde_json::from_str(&json).map_err(|_| "not an Audra EQ preset file".to_string())?;
    if file.audra_eq_preset > FILE_VERSION {
        return Err(format!(
            "made by a newer Audra (format {}), this one reads format {FILE_VERSION}",
            file.audra_eq_preset
        ));
    }
    Ok(file.preset)
}

/// The user slots as kept in prefs: exactly `USER_SLOTS`, empty ones `None`.
pub fn normalized_slots(mut slots: Vec<Option<EqPreset>>) -> Vec<Option<EqPreset>> {
    slots.resize(USER_SLOTS, None);
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::eq_plot::{clamp_gain, clamp_low_cut, clamp_q};

    #[test]
    fn the_factory_set_keeps_to_the_manuals_ranges() {
        let set = factory();
        assert_eq!(set.len(), 14, "the manual's Factory list has 14");
        for p in &set {
            assert_eq!(clamp_low_cut(p.low_cut_hz), p.low_cut_hz, "{}", p.name);
            assert!([6, 12, 18, 24].contains(&p.low_cut_slope), "{}", p.name);
            for b in &p.bands {
                assert_eq!(clamp_gain(b.gain_db), b.gain_db, "{}", p.name);
                assert_eq!(clamp_q(b.q), b.q, "{}", p.name);
            }
        }
    }

    #[test]
    fn a_preset_file_round_trips_and_other_files_are_refused() {
        let dir = std::env::temp_dir().join(format!("audra-eq-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("vocal.{FILE_EXT}"));
        let preset = factory().remove(0);
        write_file(&path, &preset).unwrap();
        assert_eq!(read_file(&path).unwrap(), preset);
        let other = dir.join("other.json");
        std::fs::write(&other, r#"{"hello": 1}"#).unwrap();
        assert!(read_file(&other).is_err());
        let newer = dir.join("newer.audraeq");
        let text = std::fs::read_to_string(&path).unwrap().replace(
            "\"audra_eq_preset\": 1",
            "\"audra_eq_preset\": 99",
        );
        std::fs::write(&newer, text).unwrap();
        assert!(read_file(&newer).unwrap_err().contains("newer Audra"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn slots_are_always_sixteen() {
        assert_eq!(normalized_slots(Vec::new()).len(), USER_SLOTS);
        assert_eq!(normalized_slots(vec![None; 20]).len(), USER_SLOTS);
    }
}
