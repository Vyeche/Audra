//! Key Commands (manual 21.10, "ARC & Key Commands"): the keyboard's F4
//! to F8 and the unit's DIM button, each set to a function in Audra
//! settings. An assigned key sends the same message the matching button
//! does, so the lock, undo and the notices treat it the same way.
//!
//! Audra's readings:
//! - Every function toggles. The unit reports its DIM button as a press
//!   count, with no release, so press-and-hold (talkback held while the
//!   key is down) can't be offered for it; the F keys toggle the same way
//!   for one rule.
//! - The unit's SET, MIX and OUT buttons are acted on by the driver itself
//!   and can't be reassigned yet (the driver keeps it as the 48V switch).
//! - F4 starts as Dim, F5 as Talkback, F6 as the Mute master (`defaults`);
//!   F7 and F8 unassigned; DIM starts as Dim on Main, as it was.

use serde::{Deserialize, Serialize};

use crate::app::{Message, TuxMix};

/// The keys that can be set: F4 to F8, then the unit's DIM button.
pub const KEYS: [&str; 6] = ["F4", "F5", "F6", "F7", "F8", "Unit's DIM button"];
/// `KEYS` index of the unit's DIM button.
pub const DIM_BUTTON: usize = 5;

/// What a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum KeyAction {
    #[default]
    Nothing,
    Dim,
    Recall,
    Talkback,
    ExternalInput,
    SpeakerB,
    Listenback,
    MuteMaster,
    SoloMaster,
    LockMix,
}

impl KeyAction {
    pub const ALL: [KeyAction; 10] = [
        KeyAction::Nothing,
        KeyAction::Dim,
        KeyAction::Recall,
        KeyAction::Talkback,
        KeyAction::ExternalInput,
        KeyAction::SpeakerB,
        KeyAction::Listenback,
        KeyAction::MuteMaster,
        KeyAction::SoloMaster,
        KeyAction::LockMix,
    ];

    /// The message the matching button sends, or `None` for Nothing.
    pub fn message(self, state: &TuxMix) -> Option<Message> {
        Some(match self {
            KeyAction::Nothing => return None,
            KeyAction::Dim => Message::DimToggle,
            KeyAction::Recall => Message::RecallMain,
            KeyAction::Talkback => Message::TalkbackToggle,
            KeyAction::ExternalInput => Message::ExtInToggle,
            KeyAction::SpeakerB => Message::SpeakerBToggle,
            KeyAction::Listenback => Message::ListenbackToggle,
            KeyAction::MuteMaster => Message::GlobalMuteToggle,
            KeyAction::SoloMaster => Message::GlobalSoloToggle,
            KeyAction::LockMix => Message::SetUiLock(!state.ui_locked),
        })
    }
}

impl std::fmt::Display for KeyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KeyAction::Nothing => "nothing",
            KeyAction::Dim => "Dim",
            KeyAction::Recall => "Recall",
            KeyAction::Talkback => "Talkback",
            KeyAction::ExternalInput => "External Input",
            KeyAction::SpeakerB => "Speaker B",
            KeyAction::Listenback => "Listenback",
            KeyAction::MuteMaster => "Mute master (all mutes)",
            KeyAction::SoloMaster => "Solo master (all solos)",
            KeyAction::LockMix => "Lock the mix",
        })
    }
}

/// The starting assignment: F4 Dim, F5 Talkback, F6 the Mute master (the
/// critique, 2026-11-04: all five keys did nothing until set up, so Dim and
/// Talkback needed the mouse), F7 and F8 free, the unit's DIM on Dim.
pub fn defaults() -> [KeyAction; 6] {
    let mut keys = [KeyAction::Nothing; 6];
    keys[0] = KeyAction::Dim;
    keys[1] = KeyAction::Talkback;
    keys[2] = KeyAction::MuteMaster;
    keys[DIM_BUTTON] = KeyAction::Dim;
    keys
}
