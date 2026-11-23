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
use tuxmix_core::{ChannelId, RmeDevice, Scene};

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
/// Whether it was applied: a mix that doesn't fit the unit is refused,
/// with a notice, before the snapshots, layouts and presets are touched.
pub fn apply(state: &mut TuxMix, mut ws: Workspace) -> bool {
    // Another shape than the unit's is refused before anything lets go:
    // checked only inside the mix's load, a refused file had already
    // released Dim, Talkback and Mute FX and switched Live Mode (the review).
    let shape = ws
        .mix
        .check_shape(state.device.inputs().len(), state.device.playbacks().len(), state.device.outputs().len())
        .and_then(|()| ws.mix.check_lists(state.device.inputs(), state.device.playbacks()));
    if let Err(e) = shape {
        state.notice = Some((format!("Could not load the workspace's mix: {e}"), std::time::Instant::now()));
        return false;
    }
    // Measured before the overrides let go, so an undim glides too.
    let levels = crate::app::output_levels(state);
    let voices = crate::app::plain_voices_in_streams(state);
    crate::control_room::release_holding(state, std::time::Instant::now());
    crate::control_room::set_live_mode(state, ws.live_mode);
    crate::control_room::keep_levels(state, &mut ws.mix, state.snapshot_keep);
    if let Err(e) = crate::app::apply_mix_scene_from(state, &ws.mix, &levels) {
        state.notice = Some((format!("Could not load the workspace's mix: {e}"), std::time::Instant::now()));
        return false;
    }
    crate::control_room::reapply(state);

    // A workspace without snapshots (a hand-made or older file, its list
    // left out) leaves the user's alone: read as eight empty slots, it
    // removed every stored snapshot (the review).
    let has_snapshots = !ws.snapshots.is_empty();
    for n in (1..=8usize).filter(|_| has_snapshots) {
        let scene = ws.snapshots.get(n - 1).cloned().flatten();
        state.stored_snapshots[n - 1] = scene.is_some();
        if state.device.is_mock() {
            state.mock_snapshots[n - 1] = scene;
        } else if let Some(scene) = scene {
            if let Err(e) = crate::scenes::save_scene_file(&format!("Mix {n}"), &scene) {
                log::warn!("Workspace: could not store snapshot {n}: {e}");
            }
        } else {
            // An empty slot in the workspace empties the file too: left, the
            // old snapshot came back at the next start (the review).
            crate::scenes::remove_scene_file(&format!("Mix {n}"));
        }
    }
    // The slots are the workspace's now: no Undo puts an older store back
    // over them (`TuxMix::undo_slots`).
    if has_snapshots {
        state.undo_slots.clear();
        state.redo_slots.clear();
        state.snapshot_names = std::array::from_fn(|i| ws.snapshot_names.get(i).cloned().flatten());
        state.snapshot_fx = std::array::from_fn(|i| ws.snapshot_fx.get(i).copied().flatten());
    }
    state.snapshot_renaming = None;
    state.active_snapshot = None;
    state.snapshot_dirty = false;

    for (slot, group) in state.groups.iter_mut().enumerate() {
        *group = ws.groups.get(slot).cloned().unwrap_or_default();
    }
    // The same for layouts.
    let has_layouts = !ws.layouts.is_empty();
    for slot in (0..state.layouts.len()).filter(|_| has_layouts) {
        let layout = ws.layouts.get(slot).cloned().flatten();
        if !state.device.is_mock() {
            match &layout {
                Some(l) => {
                    let _ = crate::layouts::save_layout_file((slot + 1) as u8, l);
                }
                None => crate::layouts::remove_layout_file((slot + 1) as u8),
            }
        }
        state.layouts[slot] = layout;
    }
    state.active_layout = None;
    if !ws.eq_presets.is_empty() {
        state.eq_presets = crate::eq_presets::normalized_slots(ws.eq_presets);
    }
    // A workspace's mic presets replace the user's. The one that loads at
    // start is found by name, so a workspace (one someone sent, say) whose
    // preset of that name is different would load by itself at the next
    // start: then no preset loads at start until the user picks one again
    // (the security review).
    let mut startup_note = None;
    if !ws.mic_presets.is_empty() {
        let start = state.startup_mic_preset.clone();
        let before = start.as_ref().and_then(|st| state.mic_presets.iter().find(|p| p.name == st.preset).cloned());
        state.mic_presets = ws.mic_presets;
        if let Some(st) = start {
            let name = st.preset;
            let after = state.mic_presets.iter().find(|p| p.name == name).cloned();
            if after.is_none() || after != before {
                state.startup_mic_preset = None;
                startup_note = Some(format!(
                    "Its mic presets replaced yours, so \u{201c}{name}\u{201d} no longer loads at start: pick it again in the input\u{2019}s Settings if you want it."
                ));
            }
        }
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
    if let (Some(note), Some((text, _))) = (startup_note, state.notice.as_mut()) {
        text.push(' ');
        text.push_str(&note);
    }
    true
}

fn workspaces_dir() -> PathBuf {
    let dir = crate::scenes::scenes_dir()
        .parent()
        .map(|p| p.join("workspaces"))
        .unwrap_or_else(|| PathBuf::from("workspaces"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Keeps what opening a workspace file replaced as "Before last open"
/// beside the quick slots: the key goes with the session, the file stays,
/// and "Open a file…" can open it later.
pub fn save_before(ws: &Workspace) -> Result<(), String> {
    write_file(&workspaces_dir().join(format!("Before last open.{FILE_EXT}")), ws).inspect_err(|e| log::warn!("Workspace: could not keep the one from before: {e}"))
}

/// Loads `ws` (a quick slot or a file) and keeps what it replaces, to
/// bring back with Workspaces → "Bring back the workspace from before".
/// The backup is the user's own setup: taken when the setup now is theirs
/// (nothing loaded yet, or brought back since), kept through later loads.
/// It is written to disk before the load changes any file. Whether the
/// load went in.
pub fn load_keeping_before(state: &mut TuxMix, ws: Workspace, what: &str) -> bool {
    let take = state.workspace_before.is_none() || !state.workspace_loaded;
    let before = take.then(|| (capture(state, &format!("Before {what}")), state.startup_mic_preset.clone()));
    let saved = match &before {
        Some((b, _)) if !state.device.is_mock() => save_before(b),
        _ => Ok(()),
    };
    if !apply(state, ws) {
        return false;
    }
    if let Some((b, startup)) = before {
        state.workspace_before = Some(b);
        state.workspace_before_startup = startup;
    }
    state.workspace_loaded = true;
    if let Some((text, _)) = state.notice.as_mut() {
        text.push_str(" Your snapshots, layouts and presets from before are kept: Workspaces \u{2192} Bring back the workspace from before.");
        if let Err(e) = saved {
            text.push_str(&format!(" A copy couldn't be saved ({e}), so it's kept until Audra closes."));
        }
    }
    true
}

/// Brings back the backup as a swap: what it replaces becomes the backup,
/// so a second press undoes a mis-click. Refused, the backup stays.
pub fn bring_back(state: &mut TuxMix) {
    let Some(ws) = state.workspace_before.take() else { return };
    let startup = state.workspace_before_startup.take();
    let now = capture(state, "Before bringing back");
    let now_startup = state.startup_mic_preset.clone();
    if apply(state, ws.clone()) {
        state.workspace_before = Some(now);
        state.workspace_before_startup = now_startup;
        state.workspace_loaded = !state.workspace_loaded;
        // The start-up preset comes back with its presets.
        if let Some(st) = startup {
            if state.mic_presets.iter().any(|p| p.name == st.preset) {
                state.startup_mic_preset = Some(st);
                state.prefs_dirty = true;
            }
        }
    } else {
        state.workspace_before = Some(ws);
        state.workspace_before_startup = startup;
    }
}

/// A backup made in the demo is no setup of the unit's: dropped when a
/// unit is plugged in (the review: it would have been applied to the unit
/// and written over its files).
pub fn forget_demo_backup(state: &mut TuxMix) {
    state.workspace_before = None;
    state.workspace_before_startup = None;
    state.workspace_loaded = false;
}

fn slot_path(slot: usize) -> PathBuf {
    workspaces_dir().join(format!("Workspace {:02}.{FILE_EXT}", slot + 1))
}

pub fn write_file(path: &Path, ws: &Workspace) -> Result<(), String> {
    let json = serde_json::to_string_pretty(ws).map_err(|e| e.to_string())?;
    // Through a temporary file and a rename: a crash mid-write never leaves
    // a cut-off workspace behind (the review).
    tuxmix_core::scene::write_atomic(path, &json)
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

    #[test]
    fn a_workspaces_different_preset_of_the_same_name_doesnt_load_at_start() {
        // The security review: the start-up preset is found by name, so a
        // workspace (one someone sent) could make its own load by itself.
        use crate::mic_presets::{MicPreset, StartupPreset};
        let mut state = new(true, None, None);
        let ch = state.device.inputs()[0].clone();
        let mine = MicPreset::capture("My mic".into(), &ch, 80, false, None);
        state.mic_presets = vec![mine.clone()];
        state.startup_mic_preset = Some(StartupPreset { input: 0, preset: "My mic".into() });
        // The same preset in the workspace: it stays the start-up one.
        let mut ws = capture(&state, "Same");
        apply(&mut state, ws.clone());
        assert!(state.startup_mic_preset.is_some(), "an identical preset keeps it");
        // Another preset under the same name: no start-up preset any more.
        let mut other = mine.clone();
        other.gain = other.gain.saturating_add(30);
        other.phantom = !mine.phantom;
        ws.mic_presets = vec![other];
        apply(&mut state, ws);
        assert!(state.startup_mic_preset.is_none(), "a different one of that name doesn't load at start");
        let said = state.notice.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        assert!(said.contains("no longer loads at start"), "{said}");
    }

    #[test]
    fn opening_a_workspace_file_keeps_what_it_replaced_to_bring_back() {
        let mut state = new(true, None, None);
        let an1 = ChannelId::Input(0);
        let _ = update(&mut state, Message::VolumeChanged(an1, 0, 0.5));
        state.snapshot_names[0] = Some("Mine".into());
        let mut theirs = capture(&state, "Theirs");
        theirs.snapshot_names = vec![Some("Theirs".into())];
        let path = test_dir().join(format!("audra-ws-test-{}.{FILE_EXT}", std::process::id()));
        write_file(&path, &theirs).unwrap();
        let _ = update(&mut state, Message::WorkspaceOpenFrom(Some(path.clone())));
        let _ = std::fs::remove_file(&path);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Theirs"));
        assert!(state.workspace_before.is_some(), "what it replaced is kept");
        let said = state.notice.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        assert!(said.contains("Bring back the workspace from before"), "{said}");
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine"), "and it comes back");
        // A swap: a second press undoes a mis-click.
        assert!(state.workspace_before.is_some(), "what it replaced is kept in turn");
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Theirs"), "pressed again, the file's is back");
    }

    /// Test files stay in the project's target folder, not /tmp.
    fn test_dir() -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_file(state: &mut crate::app::TuxMix, ws: &Workspace, tag: &str) {
        let path = test_dir().join(format!("audra-ws-{tag}-{}.{FILE_EXT}", std::process::id()));
        write_file(&path, ws).unwrap();
        let _ = update(state, Message::WorkspaceOpenFrom(Some(path.clone())));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_setup_from_before_survives_several_opens() {
        // The review: a second open replaced the backup with the first
        // file's, and the owner's own setup was gone.
        let mut state = new(true, None, None);
        state.snapshot_names[0] = Some("Mine".into());
        let mut a = capture(&state, "A");
        a.snapshot_names = vec![Some("A".into())];
        let mut b = capture(&state, "B");
        b.snapshot_names = vec![Some("B".into())];
        open_file(&mut state, &a, "a");
        open_file(&mut state, &b, "b");
        assert_eq!(state.snapshot_names[0].as_deref(), Some("B"));
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine"), "the setup from before the first open");
    }

    #[test]
    fn a_refused_file_keeps_no_backup() {
        // The review: the backup was taken before the mix was checked, so a
        // file that didn't fit left a "before" of nothing changed.
        let mut state = new(true, None, None);
        state.snapshot_names[0] = Some("Mine".into());
        let mut bad = capture(&state, "Bad");
        bad.snapshot_names = vec![Some("Bad".into())];
        bad.mix.inputs.pop();
        open_file(&mut state, &bad, "bad");
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine"), "nothing of it came in");
        assert!(state.workspace_before.is_none(), "and no backup was made");
        let said = state.notice.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
        assert!(said.contains("Could not load") && !said.contains("Bring back"), "{said}");
        // Refused on bringing back, the backup stays.
        let good = capture(&state, "Good");
        open_file(&mut state, &good, "good");
        let mut kept = state.workspace_before.clone().expect("a backup");
        kept.mix.inputs.pop();
        state.workspace_before = Some(kept);
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert!(state.workspace_before.is_some(), "a refused bring-back keeps the backup");
    }

    #[test]
    fn bringing_back_restores_the_start_up_preset() {
        // The review: a file with another preset of the same name cleared
        // the start-up preset, and bringing back didn't restore it.
        use crate::mic_presets::{MicPreset, StartupPreset};
        let mut state = new(true, None, None);
        let ch = state.device.inputs()[0].clone();
        let mine = MicPreset::capture("My mic".into(), &ch, 80, false, None);
        state.mic_presets = vec![mine.clone()];
        state.startup_mic_preset = Some(StartupPreset { input: 0, preset: "My mic".into() });
        let mut theirs = capture(&state, "Theirs");
        let mut other = mine.clone();
        other.gain = other.gain.saturating_add(30);
        other.phantom = !mine.phantom;
        theirs.mic_presets = vec![other];
        open_file(&mut state, &theirs, "startup");
        assert!(state.startup_mic_preset.is_none());
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.startup_mic_preset.as_ref().map(|s| s.preset.as_str()), Some("My mic"), "it comes back with its preset");
    }

    #[test]
    fn a_unit_plugged_in_drops_a_demo_backup() {
        let mut state = new(true, None, None);
        let a = capture(&state, "A");
        open_file(&mut state, &a, "demo");
        assert!(state.workspace_before.is_some());
        forget_demo_backup(&mut state);
        assert!(state.workspace_before.is_none(), "the demo's setup is no setup of the unit's");
    }

    #[test]
    fn work_after_bringing_back_is_kept_by_the_next_open() {
        // The re-review's blocker: after a bring-back the backup held the
        // file's setup, so the next open kept nothing of the work since.
        let mut state = new(true, None, None);
        state.snapshot_names[0] = Some("Mine".into());
        let mut a = capture(&state, "A");
        a.snapshot_names = vec![Some("A".into())];
        open_file(&mut state, &a, "w-a");
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine"));
        state.snapshot_names[0] = Some("Mine, an hour later".into());
        let mut b = capture(&state, "B");
        b.snapshot_names = vec![Some("B".into())];
        open_file(&mut state, &b, "w-b");
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine, an hour later"), "the work since is what comes back");
    }

    #[test]
    fn a_workspace_without_snapshots_or_layouts_leaves_them_alone() {
        let mut state = new(true, None, None);
        state.snapshot_names[0] = Some("Mine".into());
        state.stored_snapshots[0] = true;
        state.mock_snapshots[0] = Some(crate::control_room::capture_scene(&state));
        let mut bare = capture(&state, "Bare");
        bare.snapshots.clear();
        bare.layouts.clear();
        bare.snapshot_names.clear();
        assert!(apply(&mut state, bare));
        assert!(state.stored_snapshots[0] && state.mock_snapshots[0].is_some(), "the stored snapshot stays");
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine"));
    }

    #[test]
    fn a_quick_slot_load_keeps_what_it_replaced_too() {
        let mut state = new(true, None, None);
        state.snapshot_names[0] = Some("Mine".into());
        let mut slot = capture(&state, "Slot");
        slot.snapshot_names = vec![Some("Slot".into())];
        state.mock_workspaces[0] = Some(slot);
        let _ = update(&mut state, Message::WorkspaceLoad(0));
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Slot"));
        let _ = update(&mut state, Message::WorkspaceRestoreBefore);
        assert_eq!(state.snapshot_names[0].as_deref(), Some("Mine"));
    }

    #[test]
    fn a_file_of_another_shape_changes_nothing_before_it_is_refused() {
        // The re-review: Live Mode switched and Dim let go before the
        // shape was checked.
        let mut state = new(true, None, None);
        let live = state.control_room.live_mode;
        let mut bad = capture(&state, "Bad");
        bad.live_mode = !live;
        bad.mix.outputs.pop();
        assert!(!apply(&mut state, bad.clone()));
        assert_eq!(state.control_room.live_mode, live, "Live Mode stays");
        // The right counts but a short list inside: refused before too.
        let mut short = capture(&state, "Short");
        short.live_mode = !live;
        short.mix.inputs[0].volumes.pop();
        assert!(!apply(&mut state, short));
        assert_eq!(state.control_room.live_mode, live, "Live Mode stays for a short list");
    }
}
