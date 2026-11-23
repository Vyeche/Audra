use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::channel::{InputChannel, OutputChannel, PlaybackChannel};
use crate::device::DeviceSettings;
use crate::error::Error;

/// Directory for saved scenes/presets (~/.local/share/tuxmix/scenes).
pub fn scenes_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(home).join(".local/share/tuxmix/scenes");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Path of the SHARED auto-saved state. Both the GUI and the TUI load
/// this on open and save to it on change/exit — the device has no
/// gain/volume readback, so this single file is how the two UIs stay in
/// sync (the last writer wins; saving on exit avoids restoring a stale
/// state).
pub fn auto_scene_path() -> PathBuf {
    scenes_dir().join("auto.json")
}

/// Load the last auto-saved mixer state, if any.
pub fn load_auto_scene() -> Option<Scene> {
    let content = std::fs::read_to_string(auto_scene_path()).ok()?;
    Scene::from_json(&content).ok()
}

/// Save the mixer state to the shared auto file.
pub fn save_auto_scene(scene: &Scene) -> Result<(), String> {
    write_atomic(&auto_scene_path(), &scene.to_json().map_err(|e| e.to_string())?)
}

/// Writes `contents` to `path` through a temporary file beside it and a
/// rename, so a full disk or a crash mid-write leaves the old file whole
/// (the harden review: `fs::write` truncated it first).
pub fn write_atomic(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Read the shared auto file back WITHOUT re-applying it, returning the
/// raw JSON string (or `None` if absent/unreadable).
pub fn read_auto_scene_raw() -> Option<String> {
    std::fs::read_to_string(auto_scene_path()).ok()
}

/// Detect whether the OTHER UI (GUI ↔ TUI) has written the shared auto
/// file since our last write.
///
/// Both UIs auto-save their in-memory state every few seconds and on
/// exit, and neither has a hardware readback to re-derive the other's
/// changes — so without a check the last UI to save would clobber the
/// other's state with its own (possibly stale) copy. Before saving,
/// call this: if it returns `Some`, re-apply that scene (the other UI's
/// changes) to the device first, THEN save your own capture.
///
/// `our_last_json` = the JSON string of our most recent write (pass the
/// UI's `last_saved_json`). If the file on disk differs, another UI has
/// written since — return the parsed scene.
pub fn auto_scene_written_by_other(our_last_json: Option<&str>) -> Option<Scene> {
    let raw = read_auto_scene_raw()?;
    if our_last_json.is_some_and(|ours| ours == raw) {
        return None; // the file still holds OUR last write
    }
    // Either we never wrote (fresh start, file pre-exists) or another UI
    // wrote after us — reload their state.
    Scene::from_json(&raw).ok()
}

/// A snapshot of the full device state, serializable for
/// save/restore (scenes / presets).
///
/// Read through [`SceneOnDisk`], so a scene saved before the level scale
/// changed (issue #13) arrives converted, wherever it was kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "SceneOnDisk")]
pub struct Scene {
    /// Name of the scene (user-defined).
    pub name: String,

    /// The `RmeDevice::model_name()` this scene was captured from.
    /// Empty = legacy scene, captured before this field existed —
    /// treated as "unknown, skip the compatibility check" rather than
    /// a hard mismatch. Applying a scene by blind positional index to
    /// a different model's channel layout would silently write wrong
    /// values, so this is checked before `apply_scene` mutates anything.
    #[serde(default)]
    pub model: String,

    /// Hardware input channels.
    pub inputs: Vec<InputChannel>,

    /// Software playback channels.
    pub playbacks: Vec<PlaybackChannel>,

    /// Physical output channels.
    pub outputs: Vec<OutputChannel>,

    /// Global device-level settings.
    pub settings: DeviceSettings,

    /// The scale its levels are on: 0 (missing) is the driver's old one,
    /// 0x16a0 as 0 dB with a linear balance pan on mono sources;
    /// [`Scene::LEVEL_SCALE`] is TotalMix's (issue #13).
    #[serde(default)]
    pub level_scale: u8,
}

/// A [`Scene`] as stored, before its levels are brought to today's scale.
#[derive(Deserialize)]
pub struct SceneOnDisk {
    pub name: String,
    #[serde(default)]
    pub model: String,
    pub inputs: Vec<InputChannel>,
    pub playbacks: Vec<PlaybackChannel>,
    pub outputs: Vec<OutputChannel>,
    pub settings: DeviceSettings,
    #[serde(default)]
    pub level_scale: u8,
}

impl From<SceneOnDisk> for Scene {
    fn from(d: SceneOnDisk) -> Self {
        let mut scene = Scene {
            name: d.name,
            model: d.model,
            inputs: d.inputs,
            playbacks: d.playbacks,
            outputs: d.outputs,
            settings: d.settings,
            level_scale: d.level_scale,
        };
        scene.migrate_levels();
        scene
    }
}

impl Scene {
    /// TotalMix's level scale (Windows session 3, issue #13): a crosspoint's
    /// unity is 0x2000, and a mono source pans by constant power (-3 dB at
    /// the centre, 0 dB hard to a side).
    pub const LEVEL_SCALE: u8 = 1;

    /// Brings a scene saved on the old scale to [`Scene::LEVEL_SCALE`] so it
    /// sounds as it did: every crosspoint cell keeps its value, and only the
    /// numbers change. A stereo source's level is 3 dB lower in TotalMix's
    /// terms (0x16a0 of 0x2000); a mono source keeps its two cells, read
    /// back as level and pan by the new law (its centre unchanged, a hard pan
    /// 3 dB lower). The FX return, a stereo source, as the stereo ones.
    pub fn migrate_levels(&mut self) {
        if self.level_scale >= Self::LEVEL_SCALE {
            return;
        }
        const K: f32 = 5792.0 / 8192.0;
        for c in &mut self.inputs {
            if c.is_mono() {
                for (v, p) in c.volumes.iter_mut().zip(c.pans.iter_mut()) {
                    let (l, r) = crate::channel::pan_split(*v, *p);
                    let (nv, np) = crate::channel::pan_join_power(l * K, r * K);
                    *v = nv;
                    if nv > 0.0 {
                        *p = np;
                    }
                }
            } else {
                c.volumes.iter_mut().for_each(|v| *v *= K);
            }
        }
        for c in &mut self.playbacks {
            c.volumes.iter_mut().for_each(|v| *v *= K);
        }
        self.settings.fx_return.iter_mut().for_each(|v| *v *= K);
        self.level_scale = Self::LEVEL_SCALE;
    }

    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            model: String::new(),
            level_scale: Self::LEVEL_SCALE,
            inputs: Vec::new(),
            playbacks: Vec::new(),
            outputs: Vec::new(),
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
        }
    }

    /// Serialize the scene to a JSON string.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize a scene from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The preamps a scene may set on the unit (OpenCode's job 40): its
    /// gains, but 48V only off and PAD only on. A scene (a snapshot, a
    /// workspace, Undo) never powers a mic or makes an input 20 dB louder:
    /// those wait for the user's own press. An input with no gain on the
    /// unit keeps none, whatever the file says (OpenCode's job 43: a
    /// hand-edited file's gain on a line input showed on screen and never
    /// reached the unit). `now` is what the unit has.
    pub fn safe_preamps(inputs: &mut [InputChannel], now: &[InputChannel]) {
        for (s, n) in inputs.iter_mut().zip(now) {
            if n.gain.is_none() {
                s.gain = None;
            }
            if n.channel_type == crate::channel::ChannelType::Mic {
                s.phantom = s.phantom && n.phantom;
                s.pad = s.pad || n.pad;
            } else {
                // No 48V or PAD here: as the unit has them, whatever a scene
                // holds (the local Qwen's review).
                s.phantom = n.phantom;
                s.pad = n.pad;
            }
        }
    }

    /// Returns an error if this scene has another number of inputs,
    /// playbacks or outputs than the unit: applied, its lists would replace
    /// the unit's (OpenCode's job 40).
    pub fn check_shape(&self, inputs: usize, playbacks: usize, outputs: usize) -> Result<(), Error> {
        let scene = (self.inputs.len(), self.playbacks.len(), self.outputs.len());
        if scene != (inputs, playbacks, outputs) {
            let words = |(a, b, c): (usize, usize, usize)| format!("{a} inputs, {b} playbacks and {c} outputs");
            return Err(Error::SceneShapeMismatch { scene: words(scene), device: words((inputs, playbacks, outputs)) });
        }
        Ok(())
    }

    /// Returns an error if this scene was captured on a different
    /// model than `device_model`. A blank `self.model` (legacy scene,
    /// or one built via `Scene::new`) is always treated as compatible.
    pub fn check_compatible(&self, device_model: &str) -> Result<(), Error> {
        if !self.model.is_empty() && self.model != device_model {
            return Err(Error::SceneModelMismatch {
                scene_model: self.model.clone(),
                device_model: device_model.to_string(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_scene_never_powers_a_mic_or_takes_pad_off() {
        use crate::channel::{ChannelType, InputChannel};
        use crate::Scene;
        let mic = |phantom: bool, pad: bool| {
            let mut c = InputChannel::new(0, "AN1", ChannelType::Mic, 4);
            c.phantom = phantom;
            c.pad = pad;
            c.gain = Some(20);
            c
        };
        let now = vec![mic(false, true), mic(true, false), InputChannel::new(2, "ADAT1", ChannelType::Line, 4)];
        let mut scene = vec![mic(true, false), mic(false, true), {
            let mut c = InputChannel::new(2, "ADAT1", ChannelType::Line, 4);
            c.pad = true;
            c.gain = Some(30);
            c
        }];
        scene[0].gain = Some(40);
        Scene::safe_preamps(&mut scene, &now);
        assert!(!scene[0].phantom, "48V never switched on by a scene");
        assert!(scene[0].pad, "PAD never taken off by a scene");
        assert!(!scene[1].phantom, "48V off is fine");
        assert!(scene[1].pad, "PAD on is fine");
        assert!(!scene[2].pad, "a line input keeps what the unit has");
        assert_eq!(now[2].gain, None);
        assert_eq!(scene[2].gain, None, "and no gain, a hand-edited file's or not");
        assert_eq!(scene[0].gain, Some(40), "a mic keeps the scene's gain");
    }

    #[test]
    fn a_backend_refuses_a_scene_of_another_shape_and_keeps_its_channels() {
        // OpenCode's job 50: every way in (a snapshot, the quit's sync, a
        // start on the USB connection) goes through `apply_scene`.
        use crate::device::RmeDevice;
        let mut mock = <crate::mock::MockBabyfacePro as RmeDevice>::open().unwrap();
        let mut scene = mock.capture_scene();
        let inputs = mock.inputs().len();
        scene.inputs.pop();
        assert!(matches!(mock.apply_scene(&scene), Err(crate::Error::SceneShapeMismatch { .. })));
        assert_eq!(mock.inputs().len(), inputs, "its own channels stay");
    }

    #[test]
    fn a_scene_from_before_the_scale_change_sounds_as_it_did() {
        // Captured on the old scale (no level_scale): a playback at the old
        // 0 dB (0x16a0), AN1 centred at 0 dB and hard left at 0 dB.
        use crate::device::RmeDevice;
        let mut old = <crate::mock::MockBabyfacePro as RmeDevice>::open().unwrap().capture_scene();
        old.level_scale = 0;
        old.playbacks[0].volumes[0] = 1.0;
        old.inputs[0].volumes[0] = 1.0;
        old.inputs[0].pans[0] = 0;
        old.inputs[0].volumes[1] = 1.0;
        old.inputs[0].pans[1] = -100;
        old.settings.fx_return = vec![1.0];
        let json = serde_json::to_string(&old).unwrap().replace("\"level_scale\":0,", "").replace(",\"level_scale\":0", "");
        assert!(!json.contains("level_scale"), "as an old file holds it");
        let new: Scene = serde_json::from_str(&json).unwrap();
        assert_eq!(new.level_scale, Scene::LEVEL_SCALE);
        let k = 5792.0 / 8192.0;
        assert!((new.playbacks[0].volumes[0] - k).abs() < 1e-6, "a stereo source reads 3 dB lower");
        assert!((new.inputs[0].volumes[0] - 1.0).abs() < 1e-3 && new.inputs[0].pans[0] == 0, "centre unchanged");
        assert!((new.inputs[0].volumes[1] - k).abs() < 1e-3 && new.inputs[0].pans[1] == -100, "hard left 3 dB lower");
        assert!((new.settings.fx_return[0] - k).abs() < 1e-6);
        // Saved again, it's read as it is.
        let again: Scene = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
        assert_eq!(again.playbacks[0].volumes[0], new.playbacks[0].volumes[0]);
    }

    use super::*;

    /// Point the shared auto file at a temp path so tests never touch
    /// the real `~/.local/share/tuxmix`. `scenes_dir()` is fixed, so
    /// instead we override the HOME env var for the duration.
    struct TempHome;
    impl TempHome {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("tuxmix-test-{}", std::process::id()));
            std::env::set_var("HOME", &dir);
            let _ = std::fs::remove_dir_all(&dir);
            Self
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            // Restore a sane HOME for the rest of the test process.
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn auto_scene_other_write_detection() {
        let _home = TempHome::new();
        let ours = Scene::new("ours");
        let ours_json = ours.to_json().unwrap();
        let theirs = Scene::new("theirs");

        // No file yet: nothing written by "the other" (nothing to
        // clobber) — but a pre-existing file from a previous run IS
        // "other" until we've written once.
        assert!(auto_scene_written_by_other(None).is_none());

        // We write, then the other UI overwrites with a different scene:
        // our last_json no longer matches the disk → detected.
        save_auto_scene(&ours).unwrap();
        save_auto_scene(&theirs).unwrap();
        let detected = auto_scene_written_by_other(Some(&ours_json));
        assert!(detected.is_some());
        assert_eq!(detected.unwrap().name, "theirs");

        // Disk still holds OUR write → not "other".
        save_auto_scene(&ours).unwrap();
        assert!(auto_scene_written_by_other(Some(&ours_json)).is_none());

        // Our last_json unknown (UI restarted) but disk exists → other.
        save_auto_scene(&theirs).unwrap();
        assert!(auto_scene_written_by_other(None).is_some());
    }
}
