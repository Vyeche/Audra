//! The GUI's own preferences — how the window was laid out, not what the
//! mixer is doing (that is the shared auto-saved scene, see
//! `tuxmix_core::scene`). One JSON file in the XDG config directory
//! (`$XDG_CONFIG_HOME/audra/gui.json`, else `~/.config/audra/gui.json`):
//! the open view, the sidebar's state, the mixer row count, the zoom, and
//! the user's channel names, the PipeWire rate / buffer size forced here.
//!
//! Every field is optional on read (`#[serde(default)]`), so a file from
//! an older build, or a hand-edited one missing keys, still loads and
//! leaves the rest at the app's defaults. A file that does not parse at
//! all is ignored the same way: preferences are a convenience, never a
//! reason not to start.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tuxmix_core::ChannelId;

use crate::app::View;

/// One output's D (issue #27): on or off, and its settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OutDynPref {
    /// The output (its pair's first channel).
    pub output: usize,
    pub on: bool,
    pub params: audra_fx::DynamicsParams,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub view: Option<View>,
    pub sidebar_open: Option<bool>,
    pub sidebar_width: Option<f32>,
    /// Superseded by `row_mode`; still read, so an older file keeps its
    /// choice, and no longer written.
    #[serde(skip_serializing)]
    pub two_row: Option<bool>,
    pub row_mode: Option<crate::app::RowMode>,
    /// The floor open while one row shows at a time (0 inputs, 1 playback,
    /// 2 outputs).
    pub open_floor: Option<usize>,
    /// Outputs mirroring another's submix: (source pair, target pair).
    pub mirrors: Vec<(usize, usize)>,
    /// The 16 EQ preset slots (empty ones `null`), once any is used.
    pub eq_presets: Vec<Option<crate::eq_presets::EqPreset>>,
    /// The mic presets (`mic_presets`), in the order saved.
    pub mic_presets: Vec<crate::mic_presets::MicPreset>,
    /// The mic preset loaded when Audra starts, and onto which input.
    pub startup_mic_preset: Option<crate::mic_presets::StartupPreset>,
    pub ui_scale: Option<f32>,
    /// User channel names, keyed by the strip they were typed on. A list
    /// of pairs rather than a map: JSON object keys must be strings, and
    /// `ChannelId` serializes as a tagged value.
    pub labels: Vec<(ChannelId, String)>,
    /// TotalMix's "Show Names": display the original names even where the
    /// user renamed a channel.
    pub show_original_names: bool,
    /// The PipeWire rate / buffer size forced from the device settings
    /// (`0` = PipeWire's own), applied again at launch.
    pub engine_force_rate: Option<u32>,
    /// Logical size of the window at the last close.
    pub window_size: Option<(f32, f32)>,
    /// Closing the window hides it in the tray instead of quitting.
    pub keep_in_tray: Option<bool>,
    /// The first close asked whether to keep running in the tray
    /// (`app::tray_ask`); asked once.
    pub tray_asked: bool,
    /// The tricks found and the tips shown (`tricks`), and tips switched
    /// off.
    pub tricks_found: Vec<String>,
    pub tips_shown: Vec<String>,
    /// The discovery shimmers shown, each once ever (`motion::Shimmer`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shimmers_shown: Vec<String>,
    pub tips_off: bool,
    /// The helper cat switched off (Audra settings → App).
    #[serde(default)]
    pub helper_cat_off: bool,
    /// Milestones' cards and notes switched off on their own (the owner,
    /// 2026-11-14: "Silence milestones can be a seperate option"); before,
    /// Tips off took them too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub milestones_off: bool,
    /// The small wins' sparks and note switched off on their own (the
    /// owner's pick, 2026-11-16).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub small_wins_off: bool,
    /// The plain words under the channels' names switched off (the owner's
    /// pick, 2026-11-16).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plain_names_off: bool,
    /// Reverb and Echo: on or off and their settings (issue #9). The unit
    /// keeps none of it; Audra runs them (`fx_loop`).
    pub fx: crate::fx_loop::FxSettings,
    /// The user's own Reverb and Echo presets (`fx::UserPresets`).
    pub fx_presets: crate::fx::UserPresets,
    /// What AN1/2's FX send follows (`fx::SendFollow`).
    pub fx_send_follow: crate::fx::SendFollow,
    /// AN1/2's FX send knob, dB: kept here because the unit holds the knob
    /// plus what it follows, which reads back as a different level.
    pub fx_send_db: Option<f32>,
    /// The other source pairs' FX sends, dB, by the driver's pair name
    /// ("AN3/4", "PB1", ...): the driver starts them off at every boot.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub source_fx_send: std::collections::BTreeMap<String, f32>,
    /// The milestones reached (`celebrate`); absent in a file from before
    /// them, which seeds the list silently from what is already true.
    pub milestones: Option<Vec<String>>,
    /// The day every milestone was reached ("2026-11-14"), for Help's
    /// finale line (critique #50's "a lasting trace").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestones_all_on: Option<String>,
    /// The day every guide was done, for Help's guides finale line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guides_all_on: Option<String>,
    /// `true` once the celebration sounds were switched off.
    pub celebration_sounds_off: bool,
    /// The guides finished (`guides`), when any.
    pub guides_done: Option<Vec<String>>,
    /// Audra writes its launcher entry and icon for the running copy
    /// (`launcher`); `Some(false)` once switched off in Audra settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_entry: Option<bool>,
    /// Open mics kept in the speakers on purpose ("It's meant to be
    /// there"): each mic in its mix, warned about no more.
    pub open_mics_kept: Option<Vec<(tuxmix_core::ChannelId, usize)>>,
    /// The inputs with AutoSet on (issue #26), when any.
    pub autoset: Option<Vec<usize>>,
    /// The input whose dynamics run (issue #27), when one does, and the
    /// dynamics' settings, when changed.
    pub dynamics_input: Option<usize>,
    /// Auto-Tune on the processed mic, when on.
    pub auto_tune: Option<bool>,
    /// Noise removal on the processed mic (1 light, 2 strong), when on.
    pub noise_removal: Option<u8>,
    /// Lower delay on the processed mic, when on.
    pub low_delay: Option<bool>,
    /// The de-esser on the processed mic (1 light, 2 strong), when on.
    pub de_esser: Option<u8>,
    /// The D panel's "More" (TotalMix's knobs) open, when it is: kept
    /// between starts (critique #31: a power user reopened it every time).
    pub dyn_more: Option<bool>,
    /// A mic's Voice panel: the Fine-tune rows open.
    pub dyn_fine_open: Option<bool>,
    /// Quick setup's optional step 4 ("Make your voice sound good") put
    /// away with "Not now".
    pub quick_voice_skipped: Option<bool>,
    /// The one-time hint at the mic's D key answered.
    pub d_hint_done: Option<bool>,
    /// Where Rec saves, when not `~/Music/Audra`.
    pub record_folder: Option<String>,
    /// Rec's takes leave Reverb & Echo out. They include them by default,
    /// as you hear them (the owner, 2026-11-15: "let's add the reverb or
    /// echo please").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub record_plain: bool,
    /// Rec's takes leave the Reverb out, or the Echo (the owner,
    /// 2026-11-15: "a button to include in record on and off per Echo or
    /// Reverb"). `record_plain` from before is both.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub record_no_reverb: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub record_no_echo: bool,
    pub dynamics: Option<audra_fx::DynamicsParams>,
    /// The voice's own D settings a preset, Reset or Fit replaced ("Yours"
    /// in the preset menu).
    pub dynamics_yours: Option<audra_fx::DynamicsParams>,
    /// Its extras: noise removal, harsh S (0 to 2) and Auto-Tune.
    pub dynamics_yours_extras: Option<(u8, u8, bool)>,
    /// Each output's D (issue #27), when any was ever opened.
    pub dynamics_outputs: Option<Vec<OutDynPref>>,
    /// Room EQ for Main's speakers (issue #29), once anything was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_eq: Option<audra_fx::room_eq::RoomEqParams>,
    /// The counter's open page ("snapshots", "fx", "groups", "layouts",
    /// "view"), or "none" with every page closed; absent, Snapshots.
    pub open_page: Option<String>,
    /// `Some(false)` when Exclusive Solo was switched off (on by default).
    pub solo_exclusive: Option<bool>,
    /// `Some(true)` while Live Mode (PFL on the S buttons) is on.
    pub live_mode: Option<bool>,
    /// What loading a snapshot leaves as it is.
    pub snapshot_keep: Option<crate::control_room::SnapshotKeep>,
    /// Channel Layout: the channels off the mixer and the matrix.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<ChannelId>,
    /// The channels kept from MIDI and OSC.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hidden_remote: Vec<ChannelId>,
    /// The user's names for the 8 snapshot slots, when any is named.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub snapshot_names: Vec<Option<String>>,
    /// Whether Reverb and Echo were on in each snapshot slot when it was
    /// stored (`None`: stored before Audra kept it; a recall leaves them).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub snapshot_fx: Vec<Option<(bool, bool)>>,
    /// The "Talking + reverb" snapshot was put in once (never again, so a
    /// renamed or replaced one stays the user's).
    pub fun_snapshot_seeded: bool,
    /// `Some(true)` while Trim Gains is on for every strip.
    pub trim_all: Option<bool>,
    /// `Some(true)` while the mix is locked (Lock User Interface).
    pub ui_locked: Option<bool>,
    /// Mixer Views preferences (manual 21.7, `app::MixerPref`).
    pub center_pan_on_split: Option<bool>,
    pub no_fader_double_click: Option<bool>,
    pub no_mouse_wheel: Option<bool>,
    pub layout_panels: Option<bool>,
    /// `Some(false)` when 48V switches on with one click.
    pub confirm_48v: Option<bool>,
    /// `Some(false)` when the animations are switched off.
    pub animations: Option<bool>,
    /// `Some(false)` when UI sounds are switched off (on by default since
    /// 2026-11-03, the owner's choice; an older `Some(true)` still reads as on),
    /// and their
    /// volume in dB when not the default.
    pub ui_sounds: Option<bool>,
    pub ui_sound_db: Option<f32>,
    /// The meters' Peak Hold in seconds, when not the default.
    pub peak_hold_s: Option<f32>,
    /// `Some(false)` with the RMS bar off; `Some(true)` with RMS +3 dB.
    pub meter_rms: Option<bool>,
    pub rms_plus3: Option<bool>,
    /// Samples for OVR, when not 1.
    pub over_samples: Option<u8>,
    /// The scroll location markers, two per strip row, when any is stored.
    pub scroll_marks: Option<[[Option<f32>; 2]; 3]>,
    /// Key Commands (F4 to F8, the unit's DIM), when changed.
    pub key_commands: Option<[crate::keycommands::KeyAction; 6]>,
    /// `Some(true)` in the free Routing Mode.
    pub free_routing: Option<bool>,
    /// MIDI remote control on, and the learned bindings.
    pub midi_enabled: Option<bool>,
    /// Mackie Control: on, the surface's port name, and `Some(false)` with
    /// its meters off.
    pub mackie_on: Option<bool>,
    pub mackie_device: Option<String>,
    pub mackie_meters: Option<bool>,
    /// The Mackie extender's port name, and `Some(true)` when it stands
    /// left of the main surface.
    pub mackie_extender: Option<String>,
    pub mackie_extender_left: Option<bool>,
    /// The colour theme's id (`theme::THEMES`), when not Audra's.
    pub theme: Option<String>,
    /// The theme by time of day (`themetime`): on, and the day and night
    /// themes and start times (minutes after midnight) when not the defaults.
    pub theme_auto: Option<bool>,
    /// "Follow the desktop's light and dark" (`themetime`): on.
    #[serde(default)]
    pub theme_desktop: Option<bool>,
    pub theme_day: Option<String>,
    pub theme_night: Option<String>,
    pub theme_day_from: Option<u16>,
    pub theme_night_from: Option<u16>,
    #[serde(default)]
    pub midi_bindings: Vec<crate::midi::Binding>,
    /// Strips with their own Trim Gains on.
    pub trim_strips: Vec<ChannelId>,
    /// Output pairs flagged No Trim.
    pub no_trim_outputs: Vec<usize>,
    pub engine_force_quantum: Option<u32>,
    /// Control Room Dim amount in dB.
    pub cr_dim_db: Option<f32>,
    /// Control Room Recall (listening) level in dB.
    pub cr_recall_db: Option<f32>,
    /// The user's Main levels (L, R), present only while Dim is engaged,
    /// so the next start can undo a Dim left on by a quit or crash.
    pub cr_dimmed_main: Option<[f32; 2]>,
    /// The FX returns Mute FX holds (output pair, your level), while it is
    /// on: brought back at the next start.
    pub cr_fx_muted: Option<Vec<(usize, f32)>>,
    /// The user's own Phones submix (inputs then playbacks, level and pan),
    /// present only while Cue or Talkback plays on the Phones, so the next
    /// start can undo an override left on by a quit or crash.
    pub cr_phones_mix: Option<Vec<(f32, i8)>>,
    /// The user's own Main submix, present only while External Input plays.
    pub cr_main_mix: Option<Vec<(f32, i8)>>,
    /// The soloed submix (output pair) and the user's own mix there,
    /// present only while a solo is on.
    pub cr_solo_mix: Option<(usize, Vec<(f32, i8)>)>,
    /// The Control Room Assign menu: Main, the Phones, where Cue plays.
    pub cr_assign: Option<crate::control_room::Assign>,
    /// `Some(true)` while Speaker B was on.
    pub cr_speaker_b: Option<bool>,
    /// `Some(false)` when the Main/Main B fader Link was switched off.
    pub cr_speaker_b_link: Option<bool>,
    /// `Some(true)` while Mono was on.
    pub cr_mono: Option<bool>,
    /// The user's own submix in every output pair that carried an override
    /// (Cue, Talkback, External Input, solo), so the next start can undo
    /// one left on by a quit or crash. Replaces `cr_phones_mix`,
    /// `cr_main_mix` and `cr_solo_mix`, which are still read.
    pub cr_mixes: Vec<(usize, Vec<(f32, i8)>)>,
    /// Talkback mic (input index) and Phones dim in dB.
    pub cr_tb_input: Option<usize>,
    pub cr_tb_dim_db: Option<f32>,
    /// Output pairs flagged Talkback.
    pub cr_tb_outputs: Vec<usize>,
    /// The Listenback mic (input index), if one is chosen, and its Main
    /// dim in dB.
    pub cr_lb_input: Option<usize>,
    pub cr_lb_dim_db: Option<f32>,
    /// External Input pair (input pair index) and gain in dB.
    pub cr_ext_pair: Option<usize>,
    pub cr_ext_db: Option<f32>,
}

/// Where a session's preferences live: the simulation (`--mock`) keeps its
/// own file, so a test run never writes the real unit's settings (the
/// harden review: a simulated Dim level came back on the real Main at the
/// next start).
/// The most a file Audra opens may hold (a workspace, a snapshot, a
/// preset, a theme). Real ones are a few hundred KB at most; a picked file
/// that someone sent could be anything, and is read whole.
pub(crate) const FILE_CAP: u64 = 16 * 1024 * 1024;

/// `read_to_string`, refusing a file over `FILE_CAP` before reading it.
pub(crate) fn read_capped(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    if f.metadata()?.len() > FILE_CAP {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the file is too big to be one of Audra's"));
    }
    let mut text = String::new();
    // `take` too: a file that grows while it's read stops at the cap.
    f.take(FILE_CAP + 1).read_to_string(&mut text)?;
    if text.len() as u64 > FILE_CAP {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the file is too big to be one of Audra's"));
    }
    Ok(text)
}

pub fn path_for(mock: bool) -> Option<PathBuf> {
    let p = path()?;
    Some(if mock { p.with_file_name("gui-mock.json") } else { p })
}

/// Where the preferences live. `None` only if neither `XDG_CONFIG_HOME`
/// nor `HOME` is set, in which case nothing is persisted.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("audra").join("gui.json"))
}

pub fn load(path: &Path) -> Prefs {
    read(path).0
}

/// What reading the settings file couldn't keep (`read`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trouble {
    /// These fields were unreadable and are left at their defaults;
    /// everything else loaded.
    Fields(Vec<String>),
    /// The file as a whole couldn't be read (not JSON, or not readable):
    /// the defaults, with the reason.
    Whole(String),
}

/// The settings file, read field by field where it must be: one bad field
/// no longer drops the whole file, mic presets and names included (issue
/// #22; the next save then wrote the defaults over it). A missing file is
/// no trouble.
pub fn read(path: &Path) -> (Prefs, Option<Trouble>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Prefs::default(), None),
        Err(e) => return (Prefs::default(), Some(Trouble::Whole(e.to_string()))),
    };
    parse(&text)
}

/// `read`'s parsing, on the file's text.
pub fn parse(text: &str) -> (Prefs, Option<Trouble>) {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return (Prefs::default(), Some(Trouble::Whole(e.to_string()))),
    };
    if let Ok(prefs) = serde_json::from_value::<Prefs>(value.clone()) {
        return (prefs, None);
    }
    let Some(fields) = value.as_object() else {
        return (Prefs::default(), Some(Trouble::Whole("not a settings object".into())));
    };
    // Each field on its own: the ones that read keep their values.
    let mut good = serde_json::Map::new();
    let mut bad = Vec::new();
    for (key, val) in fields {
        let one = serde_json::Value::Object(std::iter::once((key.clone(), val.clone())).collect());
        if serde_json::from_value::<Prefs>(one).is_ok() {
            good.insert(key.clone(), val.clone());
        } else {
            bad.push(key.clone());
        }
    }
    match serde_json::from_value::<Prefs>(serde_json::Value::Object(good)) {
        Ok(prefs) => (prefs, Some(Trouble::Fields(bad))),
        Err(e) => (Prefs::default(), Some(Trouble::Whole(e.to_string()))),
    }
}

/// A copy of a settings file that didn't read whole, beside it as
/// `gui.json.bad`, before anything is saved over it; where it went.
pub fn keep_bad(path: &Path) -> Option<PathBuf> {
    let bad = path.with_extension("json.bad");
    std::fs::copy(path, &bad).ok().map(|_| bad)
}

/// The notice for `Trouble`, in plain words, naming the copy kept.
pub fn trouble_words(trouble: &Trouble, kept: Option<&Path>) -> String {
    let copy = kept.map_or_else(
        || " Audra couldn't keep a copy of the old file.".to_string(),
        |p| format!(" The old file is kept as {}.", p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())),
    );
    match trouble {
        Trouble::Fields(fields) => format!(
            "Some settings couldn't be read ({}) and are back to their defaults; the rest loaded.{copy}",
            fields.join(", ")
        ),
        Trouble::Whole(why) => format!("Your settings couldn't be read ({why}), so Audra started with its defaults.{copy}"),
    }
}

pub fn save(path: &Path, prefs: &Prefs) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    // Write-then-rename, so a crash mid-write never leaves a truncated
    // file behind (which `load` would then silently discard).
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_bad_field_keeps_the_rest_of_the_settings() {
        // Issue #22: one bad field used to drop the whole file.
        let (p, trouble) = parse(r#"{ "two_row": true, "sidebar_width": "wide", "view": "Matrix" }"#);
        assert_eq!(p.two_row, Some(true));
        assert_eq!(trouble, Some(Trouble::Fields(vec!["sidebar_width".into()])));
        let (_, whole) = parse("{ not json");
        assert!(matches!(whole, Some(Trouble::Whole(_))));
        let (_, none) = parse(r#"{ "two_row": true }"#);
        assert_eq!(none, None);
        let words = trouble_words(&Trouble::Fields(vec!["sidebar_width".into()]), Some(Path::new("/x/gui.json.bad")));
        assert!(words.contains("sidebar_width") && words.contains("gui.json.bad"));
    }

    #[test]
    fn a_partial_file_loads_with_defaults_for_the_rest() {
        let p: Prefs = serde_json::from_str(r#"{ "two_row": true }"#).unwrap();
        assert_eq!(p.two_row, Some(true));
        assert_eq!(p.view, None);
        assert!(p.labels.is_empty());
    }

    #[test]
    fn prefs_round_trip_through_a_file() {
        let dir = std::env::temp_dir().join(format!("tuxmix-prefs-test-{}", std::process::id()));
        let file = dir.join("gui.json");
        let prefs = Prefs {
            view: Some(View::Matrix),
            sidebar_open: Some(false),
            sidebar_width: Some(260.0),
            launcher_entry: Some(false),
            two_row: None,
            row_mode: Some(crate::app::RowMode::Two),
            open_floor: Some(2),
            mirrors: vec![(0, 3)],
            eq_presets: {
                let mut slots = crate::eq_presets::normalized_slots(Vec::new());
                slots[2] = crate::eq_presets::factory().into_iter().next();
                slots
            },
            mic_presets: vec![crate::mic_presets::MicPreset {
                name: "TLM 107 talking".into(),
                gain: 35,
                phantom: true,
                pad: false,
                phase: false,
                eq_on: true,
                low_cut_on: true,
                stereo: Some(true),
                an12: Some(true),
                eq: crate::eq_presets::factory().remove(1),
            }],
            startup_mic_preset: Some(crate::mic_presets::StartupPreset { input: 0, preset: "TLM 107 talking".into() }),
            ui_scale: Some(1.25),
            labels: vec![(ChannelId::Input(0), "Vocal".into())],
            show_original_names: true,
            engine_force_rate: Some(96_000),
            window_size: Some((1600.0, 900.0)),
            keep_in_tray: Some(false),
            tray_asked: true,
            tricks_found: vec!["space".into()],
            tips_shown: vec!["reset".into()],
            shimmers_shown: vec!["fold".into()],
            tips_off: true,
            helper_cat_off: true,
            milestones_off: true,
            small_wins_off: true,
            plain_names_off: true,
            fx: crate::fx_loop::FxSettings {
                reverb: audra_fx::ReverbParams { kind: audra_fx::ReverbKind::Space, time_s: 3.5, ..Default::default() },
                echo: audra_fx::EchoParams { kind: audra_fx::EchoKind::Pong, delay_ms: 250.0, ..Default::default() },
                reverb_on: true,
                echo_on: true,
            },
            fx_send_follow: crate::fx::SendFollow::Main,
            fx_send_db: Some(-12.0),
            source_fx_send: [("PB1".to_string(), -18.0)].into(),
            milestones: Some(vec!["first-snapshot".into()]),
            milestones_all_on: Some("2026-11-14".into()),
            guides_all_on: Some("2026-11-14".into()),
            celebration_sounds_off: true,
            guides_done: Some(vec!["snapshot".into()]),
            open_page: Some("none".into()),
            open_mics_kept: Some(vec![(tuxmix_core::ChannelId::Input(0), 0)]),
            autoset: Some(vec![0, 1]),
            dynamics_input: Some(0),
            auto_tune: Some(true),
            noise_removal: Some(1),
            low_delay: Some(true),
            de_esser: Some(2),
            dyn_more: Some(true),
            dyn_fine_open: Some(true),
            quick_voice_skipped: Some(true),
            d_hint_done: Some(true),
            record_folder: Some("/tmp/takes".into()),
            record_plain: false,
            record_no_reverb: true,
            record_no_echo: false,
            dynamics: Some(audra_fx::dynamics::presets()[1].1),
            dynamics_yours: Some(audra_fx::DynamicsParams { threshold_db: -31.0, ..audra_fx::dynamics::presets()[0].1 }),
            dynamics_yours_extras: Some((1, 2, true)),
            dynamics_outputs: Some(vec![OutDynPref { output: 2, on: true, params: audra_fx::dynamics::output_presets()[0].1 }]),
            room_eq: Some(audra_fx::room_eq::RoomEqParams { on: true, ..crate::room_eq::fresh() }),
            fx_presets: crate::fx::UserPresets {
                reverb: vec![crate::fx::Named { name: "Booth".into(), params: audra_fx::ReverbParams { room_scale: 0.7, ..Default::default() } }],
                echo: Vec::new(),
            },
            solo_exclusive: Some(false),
            live_mode: Some(true),
            snapshot_keep: Some(crate::control_room::SnapshotKeep::MainPhones),
            snapshot_names: vec![None, Some("Tracking".into()), None, None, None, None, None, None],
            snapshot_fx: vec![None, Some((true, false)), None, None, None, None, None, None],
            fun_snapshot_seeded: true,
            hidden: vec![ChannelId::Input(4), ChannelId::Input(5)],
            hidden_remote: vec![ChannelId::Playback(2)],
            trim_all: Some(true),
            ui_locked: Some(true),
            center_pan_on_split: Some(true),
            no_fader_double_click: Some(true),
            no_mouse_wheel: Some(true),
            layout_panels: Some(true),
            confirm_48v: Some(false),
            animations: Some(false),
            ui_sounds: Some(false),
            ui_sound_db: Some(-24.0),
            peak_hold_s: Some(2.5),
            meter_rms: Some(false),
            rms_plus3: Some(true),
            over_samples: Some(3),
            scroll_marks: Some([[Some(120.0), None], [None, None], [None, Some(40.0)]]),
            key_commands: Some([crate::keycommands::KeyAction::Talkback; 6]),
            free_routing: Some(true),
            midi_enabled: Some(true),
            mackie_on: Some(true),
            mackie_device: Some("X-Touch: X-Touch MIDI 1".into()),
            mackie_meters: Some(false),
            mackie_extender: Some("X-Touch Extender: MIDI 1".into()),
            mackie_extender_left: Some(true),
            theme: Some("monokai".into()),
            theme_auto: Some(true),
            theme_desktop: None,
            theme_day: Some("nord".into()),
            theme_night: Some("midnight".into()),
            theme_day_from: Some(6 * 60 + 30),
            theme_night_from: Some(20 * 60),
            midi_bindings: vec![crate::midi::Binding { source: crate::midi::MidiSource::Cc { channel: 0, number: 7 }, target: crate::midi::MidiTarget::MainLevel }],
            trim_strips: vec![ChannelId::Input(2)],
            no_trim_outputs: vec![4],
            engine_force_quantum: Some(128),
            cr_dim_db: Some(-15.0),
            cr_recall_db: Some(-12.0),
            cr_dimmed_main: Some([0.8, 0.7]),
            cr_fx_muted: Some(vec![(0, 0.5)]),
            cr_phones_mix: Some(vec![(0.5, -20), (0.0, 0)]),
            cr_main_mix: Some(vec![(1.0, 0)]),
            cr_solo_mix: Some((2, vec![(0.25, 10), (0.0, 0)])),
            cr_assign: Some(crate::control_room::Assign::default().with_phones(1, Some(2))),
            cr_mixes: vec![(1, vec![(0.5, -20)]), (4, vec![(0.1, 0)])],
            cr_speaker_b: Some(true),
            cr_speaker_b_link: Some(false),
            cr_mono: Some(true),
            cr_tb_input: Some(1),
            cr_tb_dim_db: Some(-12.0),
            cr_tb_outputs: vec![2],
            cr_lb_input: Some(1),
            cr_lb_dim_db: Some(-30.0),
            cr_ext_pair: Some(3),
            cr_ext_db: Some(-3.0),
        };
        save(&file, &prefs).unwrap();
        assert_eq!(load(&file), prefs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("tuxmix-prefs-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gui.json");
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(load(&file), Prefs::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
