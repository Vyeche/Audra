//! Workspaces (manual 21.9, 21.10): the whole setup in one file, the mix
//! and everything around it, so a different session ("podcast",
//! "tracking", "mixdown") is one load away. The manual's Workspace Quick
//! Select keeps 30 of them on hand (hotkey W); a workspace can also be
//! opened from and saved to a file anywhere.
//!
//! A workspace holds: the current mix, the 8 snapshots, the 4 groups, the
//! 6 layouts, the EQ presets, the channel names, the collapsed strips, the
//! row, solo, Live Mode and Trim Gains settings, the mirrors and the
//! scroll markers. Window size, zoom and Audra's own settings (tray, Dim
//! amount, sounds...) stay with the computer, not the workspace.
//!
//! Audra's readings:
//! - Loading takes two clicks (the first arms it), as it replaces the
//!   snapshots, layouts and groups, which Undo does not cover. The mix
//!   itself is one Undo step, as the manual says ("Undo/Redo also
//!   operates across Workspaces").
//! - Loading leaves Main (and the Phones) alone when Audra settings'
//!   Snapshots option says so, as a snapshot load does, so opening a
//!   workspace never jumps the monitoring.
//! - Alt+1-8 stay on the snapshots (TotalMix puts workspaces there), as
//!   Audra had them before; W opens the workspaces.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tuxmix_core::{ChannelId, Scene};

use crate::app::{RowMode, TuxMix};

/// The file extension of a workspace.
pub const FILE_EXT: &str = "audraws";
/// The manual's Workspace Quick Select slots.
pub const QUICK_SLOTS: usize = 30;

/// One workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub mix: Scene,
    #[serde(default)]
    pub snapshots: Vec<Option<Scene>>,
    #[serde(default)]
    pub groups: Vec<crate::sidebar::Group>,
    #[serde(default)]
    pub layouts: Vec<Option<crate::layouts::Layout>>,
    #[serde(default)]
    pub eq_presets: Vec<Option<crate::eq_presets::EqPreset>>,
    /// The mic presets (`mic_presets`); a workspace from before they
    /// existed leaves the ones there alone.
    #[serde(default)]
    pub mic_presets: Vec<crate::mic_presets::MicPreset>,
    #[serde(default)]
    pub labels: Vec<(ChannelId, String)>,
    #[serde(default)]
    pub collapsed: Vec<ChannelId>,
    #[serde(default)]
    pub row_mode: Option<RowMode>,
    #[serde(default)]
    pub solo_exclusive: bool,
    #[serde(default)]
    pub live_mode: bool,
    #[serde(default)]
    pub trim_all: bool,
    #[serde(default)]
    pub trim_strips: Vec<ChannelId>,
    #[serde(default)]
    pub no_trim_outputs: Vec<usize>,
    #[serde(default)]
    pub mirrors: Vec<crate::mixops::Mirror>,
    #[serde(default)]
    pub scroll_marks: Option<[[Option<f32>; 2]; 3]>,
    /// The snapshot slots' names (manual 21.5.2).
    #[serde(default)]
    pub snapshot_names: Vec<Option<String>>,
    /// Whether each slot had Reverb and Echo on (`TuxMix::snapshot_fx`).
    #[serde(default)]
    pub snapshot_fx: Vec<Option<(bool, bool)>>,
    /// Channel Layout: the hidden channels (manual 21.5.3).
    #[serde(default)]
    pub hidden: Vec<ChannelId>,
    /// The channels kept from MIDI and OSC.
    #[serde(default)]
    pub hidden_remote: Vec<ChannelId>,
}

/// A stable order for channels, so a stored file doesn't reshuffle from
/// one save to the next.
fn channel_key(c: &ChannelId) -> (u8, usize) {
    match *c {
        ChannelId::Input(i) => (0, i),
        ChannelId::Playback(i) => (1, i),
        ChannelId::Output(i) => (2, i),
    }
}

fn sorted_channels(items: impl IntoIterator<Item = ChannelId>) -> Vec<ChannelId> {
    let mut v: Vec<ChannelId> = items.into_iter().collect();
    v.sort_by_key(channel_key);
    v
}

/// The setup as it is now, under `name`.
pub fn capture(state: &TuxMix, name: &str) -> Workspace {
    let snapshots = (1..=8usize)
        .map(|n| {
            if state.device.is_mock() {
                state.mock_snapshots[n - 1].clone()
            } else if state.stored_snapshots[n - 1] {
                crate::scenes::load_scene_file(&format!("Mix {n}"))
            } else {
                None
            }
        })
        .collect();
    let mut labels: Vec<(ChannelId, String)> =
        state.labels.iter().map(|(c, l)| (*c, l.clone())).collect();
    labels.sort_by_key(|(c, _)| channel_key(c));
    Workspace {
        name: name.to_string(),
        mix: crate::control_room::capture_scene(state),
        snapshots,
        groups: state.groups.to_vec(),
        layouts: state.layouts.to_vec(),
        eq_presets: state.eq_presets.clone(),
        mic_presets: state.mic_presets.clone(),
        labels,
        collapsed: sorted_channels(state.collapsed.iter().copied()),
        row_mode: Some(state.row_mode),
        solo_exclusive: state.solo_exclusive,
        live_mode: state.control_room.live_mode,
        trim_all: state.trim_all,
        trim_strips: sorted_channels(state.trim_strips.iter().copied()),
        no_trim_outputs: {
            let mut v: Vec<usize> = state.no_trim_outputs.to_vec();
            v.sort_unstable();
            v
        },
        mirrors: state.mirrors.clone(),
        scroll_marks: Some(state.scroll_marks),
        snapshot_names: state.snapshot_names.to_vec(),
        snapshot_fx: state.snapshot_fx.to_vec(),
        hidden: sorted_channels(state.hidden.iter().copied()),
        hidden_remote: sorted_channels(state.hidden_remote.iter().copied()),
    }
}

/// Loads `ws`: the mix first (through the Control Room, so overrides are
/// released and put back), then everything around it.
pub fn apply(state: &mut TuxMix, mut ws: Workspace) {
    // Measured before the overrides let go, so an undim glides too.
    let levels = crate::app::output_levels(state);
    let voices = crate::app::plain_voices_in_streams(state);
    crate::control_room::release_holding(state, std::time::Instant::now());
    crate::control_room::set_live_mode(state, ws.live_mode);
    crate::control_room::keep_levels(state, &mut ws.mix, state.snapshot_keep);
    if let Err(e) = crate::app::apply_mix_scene_from(state, &ws.mix, &levels) {
        state.notice = Some((format!("Could not load the workspace's mix: {e}"), std::time::Instant::now()));
        return;
    }
    crate::control_room::reapply(state);

    for n in 1..=8usize {
        let scene = ws.snapshots.get(n - 1).cloned().flatten();
        state.stored_snapshots[n - 1] = scene.is_some();
        if state.device.is_mock() {
            state.mock_snapshots[n - 1] = scene;
        } else if let Some(scene) = scene {
            if let Err(e) = crate::scenes::save_scene_file(&format!("Mix {n}"), &scene) {
                log::warn!("Workspace: could not store snapshot {n}: {e}");
            }
        }
    }
    // The slots are the workspace's now: no Undo puts an older store back
    // over them (`TuxMix::undo_slots`).
    state.undo_slots.clear();
    state.redo_slots.clear();
    state.snapshot_names = std::array::from_fn(|i| ws.snapshot_names.get(i).cloned().flatten());
    state.snapshot_fx = std::array::from_fn(|i| ws.snapshot_fx.get(i).copied().flatten());
    state.snapshot_renaming = None;
    state.active_snapshot = None;
    state.snapshot_dirty = false;

    for (slot, group) in state.groups.iter_mut().enumerate() {
        *group = ws.groups.get(slot).cloned().unwrap_or_default();
    }
    for slot in 0..state.layouts.len() {
        let layout = ws.layouts.get(slot).cloned().flatten();
        if !state.device.is_mock() {
            if let Some(l) = &layout {
                let _ = crate::layouts::save_layout_file((slot + 1) as u8, l);
            }
        }
        state.layouts[slot] = layout;
    }
    state.active_layout = None;
    if !ws.eq_presets.is_empty() {
        state.eq_presets = ws.eq_presets;
    }
    if !ws.mic_presets.is_empty() {
        state.mic_presets = ws.mic_presets;
    }
    state.labels = ws.labels.into_iter().collect();
    state.collapsed = ws.collapsed.into_iter().collect();
    state.hidden = ws.hidden.into_iter().collect();
    state.hidden_remote = ws.hidden_remote.into_iter().collect();
    state.collapse_anim.clear();
    if let Some(mode) = ws.row_mode {
        state.row_mode = mode;
    }
    state.solo_exclusive = ws.solo_exclusive;
    state.trim_all = ws.trim_all;
    state.trim_strips = ws.trim_strips.into_iter().collect();
    state.no_trim_outputs = ws.no_trim_outputs.into_iter().collect();
    state.mirrors = crate::mixops::sane_mirrors(state, &ws.mirrors);
    state.scroll_marks = ws.scroll_marks.unwrap_or_default();
    state.prefs_dirty = true;
    state.notice = Some((
        format!("Workspace \"{}\" loaded. Undo (Ctrl+Z) brings the mix back.", ws.name),
        std::time::Instant::now(),
    ));
    crate::app::add_loop_held_note(state);
    // A stream that lost the voice, said last (critique #112 A P3-1 / Fable
    // A P2-1).
    crate::app::say_stream_lost_voice(state, &voices, "That workspace", "Undo (Ctrl+Z) brings the mix back.");
}

fn workspaces_dir() -> PathBuf {
    let dir = crate::scenes::scenes_dir()
        .parent()
        .map(|p| p.join("workspaces"))
        .unwrap_or_else(|| PathBuf::from("workspaces"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn slot_path(slot: usize) -> PathBuf {
    workspaces_dir().join(format!("Workspace {:02}.{FILE_EXT}", slot + 1))
}

pub fn write_file(path: &Path, ws: &Workspace) -> Result<(), String> {
    let json = serde_json::to_string_pretty(ws).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn read_file(path: &Path) -> Result<Workspace, String> {
    // Plain words, the parser's text to the log (issue #22), as a
    // snapshot file's (`scenes::read_snapshot`).
    let text = crate::prefs::read_capped(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "the file isn't there any more".to_string(),
        std::io::ErrorKind::PermissionDenied => "Audra isn't allowed to read it".to_string(),
        std::io::ErrorKind::InvalidData => "it isn't a text file, so not an Audra workspace".to_string(),
        _ => e.to_string(),
    })?;
    serde_json::from_str(&text).map_err(|e| {
        log::warn!("workspace file {}: {e}", path.display());
        if serde_json::from_str::<serde_json::Value>(&text).is_err() {
            "it isn't an Audra workspace file".to_string()
        } else {
            "it isn't an Audra workspace, or part of it is missing".to_string()
        }
    })
}

/// A quick slot's workspace, if one is stored there.
pub fn load_slot(slot: usize) -> Option<Workspace> {
    read_file(&slot_path(slot)).ok()
}

pub fn store_slot(slot: usize, ws: &Workspace) -> Result<(), String> {
    write_file(&slot_path(slot), ws)
}

/// The names in the quick slots, read once at start.
pub fn slot_names() -> Vec<Option<String>> {
    (0..QUICK_SLOTS).map(|s| load_slot(s).map(|w| w.name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, update, Message};
    use tuxmix_core::RmeDevice;

    #[test]
    fn a_workspace_carries_the_whole_setup_and_brings_it_back() {
        let mut state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        let _ = update(&mut state, Message::VolumeChanged(an1, 0, 0.5));
        state.labels.insert(an1, "Vocal".into());
        state.collapsed.insert(ChannelId::Input(5));
        state.groups[1].members = vec![an1, ChannelId::Input(1)];
        state.mock_snapshots[2] = Some(crate::control_room::capture_scene(&state));
        state.stored_snapshots[2] = true;
        state.snapshot_names[2] = Some("Interview".into());
        state.hidden.insert(ChannelId::Playback(6));
        state.trim_all = true;
        let ws = capture(&state, "Podcast");
        // Round trip through the file format.
        let text = serde_json::to_string(&ws).unwrap();
        let ws: Workspace = serde_json::from_str(&text).unwrap();

        // Change everything, then load it back.
        let _ = update(&mut state, Message::VolumeChanged(an1, 0, 0.1));
        state.labels.clear();
        state.collapsed.clear();
        state.groups = Default::default();
        state.mock_snapshots = Default::default();
        state.stored_snapshots = [false; 8];
        state.snapshot_names = Default::default();
        state.hidden.clear();
        state.trim_all = false;
        apply(&mut state, ws);
        crate::app::settle_glides(&mut state);
        assert_eq!(state.device.volume(an1, 0).ok(), Some(0.5));
        assert_eq!(state.labels.get(&an1).map(String::as_str), Some("Vocal"));
        assert!(state.collapsed.contains(&ChannelId::Input(5)));
        assert_eq!(state.groups[1].members.len(), 2);
        assert!(state.stored_snapshots[2] && state.mock_snapshots[2].is_some());
        assert_eq!(state.snapshot_names[2].as_deref(), Some("Interview"));
        assert!(state.hidden.contains(&ChannelId::Playback(6)));
        assert!(state.trim_all);
        assert!(state.notice.as_ref().is_some_and(|(t, _)| t.contains("Podcast")));
    }

    #[test]
    fn an_older_or_smaller_workspace_file_still_loads() {
        let state = new(true, None, None);
        let mix = serde_json::to_value(crate::control_room::capture_scene(&state)).unwrap();
        let text = serde_json::json!({ "name": "Bare", "mix": mix }).to_string();
        let ws: Workspace = serde_json::from_str(&text).expect("only the name and mix are needed");
        assert!(ws.snapshots.is_empty() && ws.groups.is_empty());
    }
}
