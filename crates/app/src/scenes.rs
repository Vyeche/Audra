//! Scene (preset) file I/O — ported verbatim from v1.

use std::path::PathBuf;
use tuxmix_core::Scene;

pub fn scenes_dir() -> PathBuf {
    tuxmix_core::scene::scenes_dir()
}

pub fn load_scene_file(name: &str) -> Option<Scene> {
    let content = std::fs::read_to_string(scenes_dir().join(format!("{}.json", name))).ok()?;
    Scene::from_json(&content).ok()
}

pub fn save_scene_file(name: &str, scene: &Scene) -> Result<(), String> {
    tuxmix_core::scene::write_atomic(
        &scenes_dir().join(format!("{}.json", name)),
        &scene.to_json().map_err(|e| e.to_string())?,
    )
}

/// The file extension of one snapshot saved on its own (manual 21.5.2:
/// a snapshot can be saved to and loaded from a file).
pub const SNAPSHOT_EXT: &str = "audrasnap";

/// One snapshot as a file: its name and the mix it holds.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotFile {
    pub name: String,
    pub mix: Scene,
}

pub fn write_snapshot(path: &std::path::Path, snap: &SnapshotFile) -> Result<(), String> {
    let json = serde_json::to_string_pretty(snap).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Reads a snapshot file. A bare scene (a `Mix n.json` from the snapshots
/// folder) opens too, named after the file.
pub fn read_snapshot(path: &std::path::Path) -> Result<SnapshotFile, String> {
    // Said in plain words, the parser's own text to the log (issue #22: a
    // notice read "missing field `inputs`").
    let text = crate::prefs::read_capped(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "the file isn't there any more".to_string(),
        std::io::ErrorKind::PermissionDenied => "Audra isn't allowed to read it".to_string(),
        std::io::ErrorKind::InvalidData => "it isn't a text file, so not an Audra snapshot".to_string(),
        _ => e.to_string(),
    })?;
    if let Ok(snap) = serde_json::from_str::<SnapshotFile>(&text) {
        return Ok(snap);
    }
    let mix = Scene::from_json(&text).map_err(|e| {
        log::warn!("snapshot file {}: {e}", path.display());
        if serde_json::from_str::<serde_json::Value>(&text).is_err() {
            "it isn't an Audra snapshot file".to_string()
        } else {
            "it isn't an Audra snapshot, or part of it is missing".to_string()
        }
    })?;
    let name = path
        .file_stem()
        .map_or_else(|| "Snapshot".to_string(), |s| s.to_string_lossy().into_owned());
    Ok(SnapshotFile { name, mix })
}
