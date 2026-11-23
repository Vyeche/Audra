//! Mixer Layout persistence — mirrors `scenes.rs`'s own file-per-slot
//! pattern, one JSON file per numbered slot (1-6), sibling directory to the
//! scenes one. A layout is which strips are collapsed (`TuxMix::collapsed`)
//! and, when Audra settings' "Layouts keep open panels" is on (manual
//! 21.7, "Store channel open/close in Layout Preset"), which strip panels
//! were open, and which channels are hidden (Channel Layout, manual
//! 21.5.3). A file from before panels existed is a bare list of collapsed
//! strips and still loads.

use std::collections::HashSet;
use std::path::PathBuf;

use tuxmix_core::ChannelId;

use crate::widgets::strip::FlyoutKind;

/// One stored layout.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    pub collapsed: HashSet<ChannelId>,
    /// The channels off the mixer and the matrix (`TuxMix::hidden`).
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub hidden: HashSet<ChannelId>,
    /// The strip panels open when it was stored, if it keeps them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panels: Option<Vec<(ChannelId, FlyoutKind)>>,
}

fn layouts_dir() -> PathBuf {
    let dir = crate::scenes::scenes_dir()
        .parent()
        .map(|p| p.join("layouts"))
        .unwrap_or_else(|| PathBuf::from("layouts"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Reads a layout file's text: the current shape, or the older bare list
/// of collapsed strips.
pub fn parse_layout(content: &str) -> Option<Layout> {
    serde_json::from_str::<Layout>(content).ok().or_else(|| {
        serde_json::from_str::<HashSet<ChannelId>>(content)
            .ok()
            .map(|collapsed| Layout { collapsed, ..Default::default() })
    })
}

pub fn load_layout_file(slot: u8) -> Option<Layout> {
    let content =
        std::fs::read_to_string(layouts_dir().join(format!("Layout {slot}.json"))).ok()?;
    parse_layout(&content)
}

/// Removes a stored layout's file (an empty slot), if there is one and it
/// reads (as `scenes::remove_scene_file`).
pub fn remove_layout_file(slot: u8) {
    if load_layout_file(slot).is_some() {
        let _ = std::fs::remove_file(layouts_dir().join(format!("Layout {slot}.json")));
    }
}

pub fn save_layout_file(slot: u8, layout: &Layout) -> Result<(), String> {
    let json = serde_json::to_string_pretty(layout).map_err(|e| e.to_string())?;
    std::fs::write(layouts_dir().join(format!("Layout {slot}.json")), json)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_layout_file_still_loads() {
        let old = serde_json::to_string(&HashSet::from([ChannelId::Input(2)])).unwrap();
        let layout = parse_layout(&old).expect("a bare list of collapsed strips");
        assert!(layout.collapsed.contains(&ChannelId::Input(2)));
        assert_eq!(layout.panels, None);
    }

    #[test]
    fn a_layout_keeps_its_panels() {
        let layout = Layout {
            collapsed: HashSet::new(),
            hidden: HashSet::from([ChannelId::Playback(4), ChannelId::Playback(5)]),
            panels: Some(vec![(ChannelId::Input(0), FlyoutKind::Eq)]),
        };
        let text = serde_json::to_string(&layout).unwrap();
        assert_eq!(parse_layout(&text), Some(layout));
    }
}
