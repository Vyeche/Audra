//! `tuxmix-gui` — Graphical RME interface controller.
//!
//! ```bash
//! cargo run -p tuxmix-gui              # with real hardware
//! cargo run -p tuxmix-gui -- --mock    # simulation
//! cargo run -p tuxmix-gui -- --mock --osc   # + OSC control surface
//! cargo run -p tuxmix-gui -- --backend usb  # force libusb over the kernel driver
//! cargo run -p tuxmix-gui -- --view mixer  # open straight on the Mixer page
//! cargo run -p tuxmix-gui -- --fullscreen  # start fullscreen (F11 toggles)
//! ```

mod about;
mod helper;
mod control;
mod assist;
mod autoset;
mod recorder;
mod priority;
mod recordings;
mod dynamics_loop;
mod dynamics_panel;
mod round_trip;
mod celebrate;
mod guides;
mod hearfix;
mod app;
mod control_room;
mod eq_presets;
mod flow;
mod fx;
mod fx_loop;
mod mic_presets;
mod icon;
mod layouts;
mod matrix;
mod mixops;
mod resetmix;
mod motion;
mod sounds;
mod spectrum;
mod workspaces;
mod keycommands;
mod keyfocus;
mod launcher;
mod live_spectrum;
mod midi;
mod osc;
mod prefs;
mod quick;
mod room_eq;
mod scenes;
mod setups;
mod sidebar;
mod theme;
mod unit_sketch;
mod themetime;
mod usertheme;
mod mackie;
#[cfg(test)]
mod bench;
mod tray;
mod tricks;
mod widgets;
#[cfg(test)]
mod listener_safety_tests;

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use iced::window;
use iced::Size;

/// Audra's three faces, all SIL OFL 1.1 (licences beside them in
/// assets/fonts/). Atkinson Hyperlegible Next sets every label and was
/// drawn for legibility at small sizes, which matters in a dim room. Its
/// Mono sibling sets every number, so readouts keep a fixed width while
/// they move. Shippori Mincho Bold sets only the row names and the Now
/// Mixing placard. The Atkinson files are fixed-weight instances of the
/// variable fonts, and the Mincho file is cut down to Latin and the
/// symbols the app uses (−, ∞, ·), 36 KB instead of 8.5 MB.
const FONTS: [&[u8]; 5] = [
    include_bytes!("../assets/fonts/AtkinsonHyperlegibleNext-400.ttf"),
    include_bytes!("../assets/fonts/AtkinsonHyperlegibleNext-600.ttf"),
    include_bytes!("../assets/fonts/AtkinsonHyperlegibleNext-700.ttf"),
    include_bytes!("../assets/fonts/AtkinsonHyperlegibleMono-400.ttf"),
    include_bytes!("../assets/fonts/ShipporiMincho-Bold-Latin.ttf"),
];

/// The application id, matching the basename of `tuxmix.desktop`, so the
/// desktop environment associates the window with the app entry (icon
/// included) in the taskbar and the window list.
const APP_ID: &str = "audra";

/// The window icon, from the same decoded PNG the About dialog draws (see
/// `icon`). `None` on a decode error: a missing icon is cosmetic, not a
/// reason to fail the launch.
fn window_icon() -> Option<window::Icon> {
    let (width, height, pixels) = icon::rgba()?;
    window::icon::from_rgba(pixels, width, height).ok()
}

/// Value following a `--flag <value>` pair, if present.
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// `--open`: a panel shown from the start, mostly so `--screenshot` can
/// capture screens that otherwise need a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenPanel {
    /// Audra settings (F2).
    Settings,
    /// A strip's settings panel (`input-in3`, `playback`).
    StripSettings(tuxmix_core::ChannelId),
    /// A strip's EQ panel (`output-eq`, `phones-eq`).
    StripEq(tuxmix_core::ChannelId),
    /// A strip's D panel (`dyn`, `phones-dyn`, `main-dyn`; issue #27).
    StripDyn(tuxmix_core::ChannelId),
    /// AN1's D panel asking to move the shaped voice from AN2 (`move-ask`;
    /// critique #32's pick, "A").
    MoveAsk,
    /// The Phones in M/S with their left side inverted and EQ on, their
    /// Settings open: the strip's marks and the placard's note.
    PhonesMs,
    /// The faders showing FX sends, the Reverb & Echo page open.
    FxFaders,
    /// FX sends on the faders, the Reverb on, Main's return off: the
    /// header's "not heard in Main 1/2 yet" and Make it heard (`fx-unheard`).
    FxUnheard,
    /// FX sends on the faders, the Reverb on, Mute FX holding Main's return:
    /// the header's "held off by Mute FX" (`fx-held`).
    FxHeld,
    /// FX sends on the faders, the Reverb on and Main's return up, but no
    /// send up: the header's "not heard in Main 1/2 yet" (`fx-nosend`;
    /// OpenCode's job 71).
    FxNoSend,
    /// `fx-nosend` with Mute FX on: Mute FX named first, "Mute FX is on"
    /// (`mute-nosend`; OpenCode's job 71).
    MuteNoSend,
    /// `mute-nosend` with the faders on the mix: the placard's row,
    /// "· release Mute FX" (`mute-nosend-row`; critique #71 A 1).
    MuteNoSendRow,
    /// `mute-nosend` with the keyboard on Release Mute FX
    /// (`fx-sends-focus`).
    FxSendsFocus,
    /// `fx-held` with the pointer on Mute FX: Main's FX key glowing
    /// (`mute-fx-hover`).
    MuteFxHover,
    /// Mute FX off, the pointer on it, Main's return up: the faint preview
    /// of what it would silence (`mute-fx-preview`).
    MuteFxPreview,
    /// `fx-held` with the pointer on Main's strip: Mute FX ringed
    /// (`mute-fx-strip`).
    MuteFxStrip,
    /// Help at its Milestones tab, four reached ("Room in your take" among
    /// them) (`help-milestones`).
    HelpMilestones,
    /// Help at its Keys or About tab (critique #87 A P3-7: never captured).
    HelpTabAt(about::HelpTab),
    /// "Where AN1 goes" after Podcast or call: Main's lane silent, "not in
    /// your speakers" (critique #87 A P3-7).
    FlowSilent,
    /// Mute FX pressed for the first time: its line (`first-mutefx`).
    FirstMuteFx,
    /// Reverb on, Main's FX return down: the placard's "not heard" row
    /// (`placard-noreturn`).
    PlacardNoReturn,
    /// Reverb and Echo on and heard: the placard's "· with FX"
    /// (`placard-both`).
    PlacardBoth,
    /// "Room in your take"'s card (`milestone-room`).
    MilestoneRoom,
    /// The first amber meter's line, as on the unit (`first-meter`).
    FirstMeter,
    /// In recordings switched back on, its line (`first-recfx`).
    FirstRecFx,
    /// Audra settings at one of its pages (`settings-look` and so on).
    SettingsAt(app::SettingsPage),
    /// The device panel (F3).
    Device,
    /// About (F1).
    About,
    /// The Workspaces panel (W).
    Workspaces,
    /// Main's Room EQ page (issue #29), with a few bands set.
    RoomEq,
    /// The helper cat beside a tip (`helper.rs`).
    Helper,
    /// The helper cat alone in its band: the friendly cards out of the way
    /// (Tips off), its place and clock from `AUDRA_CAT`.
    CatBand,
    WorkspacesBefore,
    /// The EQ panel of the first input strip.
    Eq,
    /// AN1's EQ panel open, its EQ bypassed (`eq-off`).
    EqOff,
    /// The first two input strips collapsed.
    Collapsed,
    /// The Settings panel of the first input strip.
    Input,
    /// AN1's panel with Stereo pair pressed once: the link's arm.
    LinkArm,
    /// The Settings panel of the AS1/2 output strip, with the outputs open.
    Output,
    /// The Phones' Settings panel, in the Control Room.
    Phones,
    /// The Matrix with a patch's pulse under way (Fun pass 2).
    PatchPulse,
    /// A mix just picked, its feeding strips' path flaring (Fun pass 2).
    PathPulse,
    /// A signal-flow card (`flow`), its dots held.
    Flow(flow::FlowOf),
    /// A flow map with the Reverb on and heard as far as the mix sets
    /// it (`flow-source-fx`, `flow-mix-fx`).
    FlowFx(flow::FlowOf),
    /// `flow-mix-fx` with Mute FX on (`flow-mix-held`).
    FlowMixHeld,
    /// AN1's map with the Reverb on and no send (`flow-source-nosend`).
    FlowSourceNoSend,
    /// `fx-held` with the keyboard on Back (`fx-sends-back-focus`).
    FxSendsBackFocus,
    /// `placard-noreturn` with the keyboard on the not-heard row
    /// (`placard-row-focus`).
    PlacardRowFocus,
    /// The keyboard on the placard's "Only these" (`only-these-focus`).
    OnlyTheseFocus,
    /// The keyboard on the placard's mix name (`mix-name-focus`) and its
    /// "Fed by" map line (`fed-by-focus`).
    MixNameFocus,
    FedByFocus,
    /// The Phones in M/S, the keyboard on the placard's output note
    /// (`output-note-focus`).
    OutputNoteFocus,
    /// "What's this?" mode on: its banner (`explain`).
    Explain,
    /// Quick setup with My setup saved and lit (critique #80 A 11).
    MySetupSaved,
    /// ... and its Replace asking (critique #80 A 2).
    MySetupReplace,
    /// My setup brought back for real: its notice, Undo lit (critique #81 A 5).
    MySetupRecalled,
    /// The plain words and the sockets sketch switched off (critique #81 A 5).
    PlainOff,
    /// An input's Settings with the switch off: no sketch (critique #82 A 6).
    InputPlainOff,
    /// Quick setup right after "Podcast or call": the lit tile and its card
    /// with Undo (critique #83 A P3-6).
    SetupFresh,
    /// ... then a change: "… was set; the mix has changed since", no Undo.
    SetupChanged,
    /// "Podcast or call" with the mic's 48V off: the card's "48V waits for
    /// your click" (critique #84 A P3-7), and (true) after that click.
    SetupArmed(bool),
    /// The Record and the Stream setups' cards.
    SetupRecord,
    SetupStream,
    SetupHeadphones,
    /// The keyboard on the open-mic line (`open-mic-focus`).
    OpenMicFocus,
    /// A discovery shimmer held mid-sweep on IN3/4 (`motion::Shimmer`).
    Shimmer(motion::Shimmer),
    /// The keyboard on IN3/4 (its focus rim), as after Tab or an arrow.
    Focus,
    /// IN1/2 selected, IN3/4 selected and focused; Mix 2 active.
    FocusSelect,
    /// The first close's question about the tray.
    TrayAsk,
    /// Quitting's question while apps play through a virtual speaker.
    QuitAsk,
    /// AN1's D on, turning down 9 dB: the meter's marks.
    DMarks,
    /// AN1's D panel with a mic check's report card.
    MicCheck,
    /// The keyboard on the counter's Dim key.
    KbCounter,
    /// AN1/2's Settings panel open, the keyboard on its gain.
    KbPanel,
    /// The Matrix, the keyboard on IN3's cell for Main.
    KbMatrix,
    /// Quick setup with steps to do: AN1's 48V off, nothing sent to the
    /// Phones.
    QuickTodo,
    /// Five tricks found, the card for the last one showing.
    Tricks,
    /// The trick card, a third find close together, its segment blooming.
    TricksRoll,
    /// Snapshot 1 stored with Reverb on: its key's coral bar.
    SnapshotFx,
    /// PAD switched on for the first time: its "What that did" card.
    FirstPad,
    /// A fader pushed past 0 dB for the first time: its caution card.
    FirstAbove0,
    /// Five tricks found, About open with them ticked.
    TricksAbout,
    MuteFx,
    FxSave,
    KbFx,
    KbSettings,
    KbQuick,
    KbEq,
    QuickSpeakers,
    Milestone,
    MilestoneLate,
    GuidePhones,
    GuideStep,
    GuideStore,
    OpenMic,
    Notice,
    ClipShock,
    /// An assist note in the counter's slot (AN1 clipping).
    Assist,
    ClipShockLate,
    GuideDone,
    /// The last guide's finish: the finale line on its card.
    GuideFinale,
    QuickLively,
    QuickUnlit,
    FxHeard,
    /// An open mic's reverb in the speakers (the speakers' FX return raised
    /// by hand after Make it heard), seen from the speakers' mix and from
    /// the headphones' (critique #110: no capture showed it).
    FxHazard,
    FxHazardPhones,
    Masters,
    /// A playback pair's right side lower than its left in Main (the
    /// Matrix's Mono Mode): its strip's L≠R tag.
    SidesApart,
    /// Talkback on: its line under the Phones level in the sidebar.
    TalkbackOn,
    /// Phones as the mix being edited: the strips' routing fields say
    /// "Phones 3/4", the longest common name, beside the narrow switch.
    MixPhones,
    /// Dim on: its note under Main's name.
    DimOn,
    /// PAD just switched off on AN1: the gain dipped, and the notice says why.
    PadOff,
    /// PAD on on AN1: its strip says PAD where its type word stands.
    PadOn,
    /// PAD on on AN1, folded: its mark under the 48V lamp.
    PadFolded,
    /// The open-mic warning opened once and closed: its line quiet.
    OpenMicSeen,
    /// A take just saved, with gaps: its note and Open folder key.
    TakeSaved,
    /// An output strip's FX key pressed: its Settings at the Reverb & Echo
    /// row, the ring on it.
    FxKey,
    KbPage,
    KbAbout,
    KbStore,
}

fn parse_open(args: &[String]) -> Option<OpenPanel> {
    match arg_value(args, "--open").as_deref() {
        Some("settings") => Some(OpenPanel::Settings),
        Some(s) if s.starts_with("settings-") => app::SettingsPage::ALL
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(&s["settings-".len()..]))
            .map(OpenPanel::SettingsAt),
        Some("device") => Some(OpenPanel::Device),
        Some("about") => Some(OpenPanel::About),
        Some("workspaces") => Some(OpenPanel::Workspaces),
        Some("room-eq") => Some(OpenPanel::RoomEq),
        Some("helper") => Some(OpenPanel::Helper),
        Some("cat-band") => Some(OpenPanel::CatBand),
        Some("workspaces-before") => Some(OpenPanel::WorkspacesBefore),
        Some("eq") => Some(OpenPanel::Eq),
        Some("eq-off") => Some(OpenPanel::EqOff),
        Some("input") => Some(OpenPanel::Input),
        Some("link-arm") => Some(OpenPanel::LinkArm),
        Some("input-in3") => Some(OpenPanel::StripSettings(tuxmix_core::ChannelId::Input(2))),
        Some("playback") => Some(OpenPanel::StripSettings(tuxmix_core::ChannelId::Playback(0))),
        Some("output-eq") => Some(OpenPanel::StripEq(tuxmix_core::ChannelId::Output(4))),
        Some("phones-eq") => Some(OpenPanel::StripEq(tuxmix_core::ChannelId::Output(2))),
        Some("dyn") => Some(OpenPanel::StripDyn(tuxmix_core::ChannelId::Input(0))),
        Some("phones-dyn") => Some(OpenPanel::StripDyn(tuxmix_core::ChannelId::Output(2))),
        Some("main-dyn") => Some(OpenPanel::StripDyn(tuxmix_core::ChannelId::Output(0))),
        Some("phones-ms") => Some(OpenPanel::PhonesMs),
        Some("fx-faders") => Some(OpenPanel::FxFaders),
        Some("fx-unheard") => Some(OpenPanel::FxUnheard),
        Some("fx-held") => Some(OpenPanel::FxHeld),
        Some("fx-nosend") => Some(OpenPanel::FxNoSend),
        Some("mute-nosend") => Some(OpenPanel::MuteNoSend),
        Some("mute-nosend-row") => Some(OpenPanel::MuteNoSendRow),
        Some("fx-sends-focus") => Some(OpenPanel::FxSendsFocus),
        Some("mute-fx-hover") => Some(OpenPanel::MuteFxHover),
        Some("mute-fx-preview") => Some(OpenPanel::MuteFxPreview),
        Some("mute-fx-strip") => Some(OpenPanel::MuteFxStrip),
        Some("help-milestones") => Some(OpenPanel::HelpMilestones),
        Some("first-mutefx") => Some(OpenPanel::FirstMuteFx),
        Some("placard-noreturn") => Some(OpenPanel::PlacardNoReturn),
        Some("placard-both") => Some(OpenPanel::PlacardBoth),
        Some("milestone-room") => Some(OpenPanel::MilestoneRoom),
        Some("first-meter") => Some(OpenPanel::FirstMeter),
        Some("first-recfx") => Some(OpenPanel::FirstRecFx),
        Some("collapsed") => Some(OpenPanel::Collapsed),
        Some("output") => Some(OpenPanel::Output),
        Some("phones") => Some(OpenPanel::Phones),
        Some("patch-pulse") => Some(OpenPanel::PatchPulse),
        Some("path-pulse") => Some(OpenPanel::PathPulse),
        Some("flow-source") => Some(OpenPanel::Flow(flow::FlowOf::Source(tuxmix_core::ChannelId::Input(0)))),
        Some("flow-mix") => Some(OpenPanel::Flow(flow::FlowOf::Mix(1))),
        Some("flow-source-fx") => Some(OpenPanel::FlowFx(flow::FlowOf::Source(tuxmix_core::ChannelId::Input(0)))),
        Some("flow-mix-fx") => Some(OpenPanel::FlowFx(flow::FlowOf::Mix(0))),
        Some("flow-mix-held") => Some(OpenPanel::FlowMixHeld),
        Some("flow-silent") => Some(OpenPanel::FlowSilent),
        Some("help-keys") => Some(OpenPanel::HelpTabAt(about::HelpTab::Keys)),
        Some("help-about") => Some(OpenPanel::HelpTabAt(about::HelpTab::About)),
        Some("flow-source-nosend") => Some(OpenPanel::FlowSourceNoSend),
        Some("fx-sends-back-focus") => Some(OpenPanel::FxSendsBackFocus),
        Some("placard-row-focus") => Some(OpenPanel::PlacardRowFocus),
        Some("only-these-focus") => Some(OpenPanel::OnlyTheseFocus),
        Some("mix-name-focus") => Some(OpenPanel::MixNameFocus),
        Some("output-note-focus") => Some(OpenPanel::OutputNoteFocus),
        Some("explain") => Some(OpenPanel::Explain),
        Some("mysetup-saved") => Some(OpenPanel::MySetupSaved),
        Some("mysetup-replace") => Some(OpenPanel::MySetupReplace),
        Some("mysetup-recalled") => Some(OpenPanel::MySetupRecalled),
        Some("plain-off") => Some(OpenPanel::PlainOff),
        Some("input-plain-off") => Some(OpenPanel::InputPlainOff),
        Some("setup-fresh") => Some(OpenPanel::SetupFresh),
        Some("setup-changed") => Some(OpenPanel::SetupChanged),
        Some("setup-armed") => Some(OpenPanel::SetupArmed(false)),
        Some("setup-armed-clicked") => Some(OpenPanel::SetupArmed(true)),
        Some("setup-record") => Some(OpenPanel::SetupRecord),
        Some("setup-stream") => Some(OpenPanel::SetupStream),
        Some("setup-headphones") => Some(OpenPanel::SetupHeadphones),
        Some("fed-by-focus") => Some(OpenPanel::FedByFocus),
        Some("open-mic-focus") => Some(OpenPanel::OpenMicFocus),
        Some("shimmer-fold") => Some(OpenPanel::Shimmer(motion::Shimmer::Fold)),
        Some("shimmer-level") => Some(OpenPanel::Shimmer(motion::Shimmer::Level)),
        Some("shimmer-name") => Some(OpenPanel::Shimmer(motion::Shimmer::Name)),
        Some("focus") => Some(OpenPanel::Focus),
        Some("focus-select") => Some(OpenPanel::FocusSelect),
        Some("tray-ask") => Some(OpenPanel::TrayAsk),
        Some("quit-ask") => Some(OpenPanel::QuitAsk),
        Some("d-marks") => Some(OpenPanel::DMarks),
        Some("mic-check") => Some(OpenPanel::MicCheck),
        Some("move-ask") => Some(OpenPanel::MoveAsk),
        Some("kb-counter") => Some(OpenPanel::KbCounter),
        Some("kb-panel") => Some(OpenPanel::KbPanel),
        Some("kb-matrix") => Some(OpenPanel::KbMatrix),
        Some("quick-todo") => Some(OpenPanel::QuickTodo),
        Some("tricks") => Some(OpenPanel::Tricks),
        Some("tricks-roll") => Some(OpenPanel::TricksRoll),
        Some("snapshot-fx") => Some(OpenPanel::SnapshotFx),
        Some("first-pad") => Some(OpenPanel::FirstPad),
        Some("first-above0") => Some(OpenPanel::FirstAbove0),
        Some("tricks-about") => Some(OpenPanel::TricksAbout),
        Some("mute-fx") => Some(OpenPanel::MuteFx),
        Some("fx-save") => Some(OpenPanel::FxSave),
        Some("kb-fx") => Some(OpenPanel::KbFx),
        Some("kb-settings") => Some(OpenPanel::KbSettings),
        Some("kb-quick") => Some(OpenPanel::KbQuick),
        Some("kb-eq") => Some(OpenPanel::KbEq),
        Some("quick-speakers") => Some(OpenPanel::QuickSpeakers),
        Some("milestone") => Some(OpenPanel::Milestone),
        Some("milestone-late") => Some(OpenPanel::MilestoneLate),
        Some("guide-step") => Some(OpenPanel::GuideStep),
        Some("guide-phones") => Some(OpenPanel::GuidePhones),
        Some("guide-store") => Some(OpenPanel::GuideStore),
        Some("open-mic") => Some(OpenPanel::OpenMic),
        Some("notice") => Some(OpenPanel::Notice),
        Some("clip-shock") => Some(OpenPanel::ClipShock),
        Some("assist") => Some(OpenPanel::Assist),
        Some("clip-shock-late") => Some(OpenPanel::ClipShockLate),
        Some("guide-done") => Some(OpenPanel::GuideDone),
        Some("guide-finale") => Some(OpenPanel::GuideFinale),
        Some("quick-lively") => Some(OpenPanel::QuickLively),
        Some("quick-unlit") => Some(OpenPanel::QuickUnlit),
        Some("fx-heard") => Some(OpenPanel::FxHeard),
        Some("fx-hazard") => Some(OpenPanel::FxHazard),
        Some("fx-hazard-phones") => Some(OpenPanel::FxHazardPhones),
        Some("masters") => Some(OpenPanel::Masters),
        Some("sides-apart") => Some(OpenPanel::SidesApart),
        Some("talkback-on") => Some(OpenPanel::TalkbackOn),
        Some("mix-phones") => Some(OpenPanel::MixPhones),
        Some("dim-on") => Some(OpenPanel::DimOn),
        Some("pad-off") => Some(OpenPanel::PadOff),
        Some("pad-on") => Some(OpenPanel::PadOn),
        Some("pad-folded") => Some(OpenPanel::PadFolded),
        Some("open-mic-seen") => Some(OpenPanel::OpenMicSeen),
        Some("take-saved") => Some(OpenPanel::TakeSaved),
        Some("fx-key") => Some(OpenPanel::FxKey),
        Some("kb-page") => Some(OpenPanel::KbPage),
        Some("kb-about") => Some(OpenPanel::KbAbout),
        Some("kb-store") => Some(OpenPanel::KbStore),
        Some(other) => {
            eprintln!("Unknown --open value {other:?} (use settings|settings-mixer|settings-monitor|settings-look|settings-remote|settings-app|device|about|workspaces|room-eq|eq|input|input-in3|playback|output-eq|phones-eq|phones-ms|fx-faders|output|phones|patch-pulse|path-pulse|collapsed|focus|focus-select|tray-ask|kb-counter|kb-panel|kb-matrix|quick-todo|tricks|tricks-about|mute-fx|fx-save|kb-fx|kb-settings|kb-quick|kb-eq|quick-speakers|milestone|milestone-late|guide-step|guide-done|quick-lively|quick-unlit|fx-heard|masters|kb-page|kb-about|kb-store|sides-apart|talkback-on|pad-off|pad-on|pad-folded|open-mic-seen|take-saved|fx-key|dim-on|mix-phones), opening none");
            None
        }
        None => None,
    }
}

/// `--tab snapshots|fx|groups|layouts|view`: the counter tab shown at start.
fn parse_tab(args: &[String]) -> Option<sidebar::Tab> {
    match arg_value(args, "--tab").as_deref() {
        Some("snapshots") => Some(sidebar::Tab::Snapshots),
        Some("fx") => Some(sidebar::Tab::Fx),
        Some("groups") => Some(sidebar::Tab::Groups),
        Some("layouts") => Some(sidebar::Tab::Layouts),
        Some("view") => Some(sidebar::Tab::View),
        Some(other) => {
            eprintln!("Unknown --tab value {other:?} (use snapshots|groups|layouts|view)");
            None
        }
        None => None,
    }
}

fn open_panel(state: &mut app::TuxMix, panel: OpenPanel) {
    match panel {
        OpenPanel::Settings => state.show_settings = true,
        OpenPanel::SettingsAt(page) => {
            // As the app opens it, the App page's recordings read.
            let _ = app::update(state, app::Message::OpenSettingsAt(page));
        }
        OpenPanel::Device => state.show_device_panel = true,
        OpenPanel::About => state.show_about = true,
        OpenPanel::Workspaces => state.show_workspaces = true,
        // The made-up bands only in the simulation, never into the unit's
        // settings (the review: `--open room-eq` on the unit saved them).
        OpenPanel::RoomEq if !state.device.is_mock() => state.show_room_eq = true,
        OpenPanel::CatBand => {
            state.tips = false;
        }
        OpenPanel::Helper => {
            state.trick_card = Some((tricks::Card::Tip("reset"), std::time::Instant::now()));
        }
        OpenPanel::RoomEq => {
            // A room's usual fixes: a bass boom cut, a dip lifted a little,
            // the nearer (left) speaker delayed and turned down.
            use crate::room_eq::{update, RoomMsg, Sides};
            update(state, RoomMsg::Power(true));
            update(state, RoomMsg::Pick(1));
            update(state, RoomMsg::Gain(-6.0));
            update(state, RoomMsg::Freq(110.0));
            update(state, RoomMsg::Pick(3));
            update(state, RoomMsg::Gain(2.5));
            update(state, RoomMsg::Sides(Sides::Left));
            update(state, RoomMsg::Delay(1.2));
            update(state, RoomMsg::Trim(-1.5));
            update(state, RoomMsg::Pick(1));
            state.show_room_eq = true;
        }
        // After a file was opened: the key that brings back the one before.
        OpenPanel::WorkspacesBefore => {
            state.workspace_before = Some(crate::workspaces::capture(state, "Before opening Theirs"));
            state.workspace_loaded = true;
            state.show_workspaces = true;
        }
        OpenPanel::Eq => {
            state.flyout_open.insert((
                tuxmix_core::ChannelId::Input(0),
                widgets::strip::FlyoutKind::Eq,
            ));
        }
        // AN1's EQ panel open with its EQ bypassed: the EQ key's ring
        // (critique #60 A 9: no capture showed it).
        OpenPanel::EqOff => {
            let _ = app::update(state, app::Message::EqEnabled(0, false));
            state.flyout_open.insert((tuxmix_core::ChannelId::Input(0), widgets::strip::FlyoutKind::Eq));
        }
        OpenPanel::StripSettings(cid) => app::open_flyout(state, cid, widgets::strip::FlyoutKind::Settings),
        OpenPanel::StripEq(cid) => app::open_flyout(state, cid, widgets::strip::FlyoutKind::Eq),
        OpenPanel::StripDyn(cid) => app::open_flyout(state, cid, widgets::strip::FlyoutKind::Dynamics),
        OpenPanel::FxFaders => {
            use tuxmix_core::RmeDevice;
            state.fader_fx = true;
            let _ = state.device.set_fx_send(-10.0);
            let _ = state.device.set_source_fx_send(tuxmix_core::ChannelId::Playback(0), -18.0);
            let _ = state.device.set_source_fx_send(tuxmix_core::ChannelId::Input(4), -24.0);
            let _ = state.device.set_fx_return(1, 0.5);
            state.sidebar_tab = Some(crate::sidebar::Tab::Fx);
        }
        OpenPanel::FxUnheard => {
            open_panel(state, OpenPanel::FxFaders);
            state.fx.reverb_on = true;
        }
        OpenPanel::FxHeld => {
            open_panel(state, OpenPanel::FxFaders);
            state.fx.reverb_on = true;
            let pair = state.sel_out;
            fx::set_return(state, pair, fx::db_vol(-12.0));
            fx::set_mute(state, true);
        }
        OpenPanel::FxSendsFocus => {
            open_panel(state, OpenPanel::MuteNoSend);
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::FxSendsKey));
        }
        OpenPanel::FxNoSend | OpenPanel::MuteNoSend | OpenPanel::MuteNoSendRow => {
            state.notice = None;
            state.d_hint_done = true;
            state.fader_fx = panel != OpenPanel::MuteNoSendRow;
            state.fx.reverb_on = true;
            let pair = state.sel_out;
            fx::set_return(state, pair, fx::db_vol(-12.0));
            state.sidebar_tab = Some(crate::sidebar::Tab::Fx);
            if panel != OpenPanel::FxNoSend {
                fx::set_mute(state, true);
            }
        }
        OpenPanel::MuteFxHover => {
            open_panel(state, OpenPanel::FxHeld);
            state.mute_fx_hover = true;
        }
        OpenPanel::FirstMuteFx => {
            state.notice = None;
            state.d_hint_done = true;
            // Something to silence, and the room in the Phones (critique
            // #67 A 1, #68 A 5: a send up too, so it's really heard).
            state.fx.reverb_on = true;
            let main = state.control_room.assign.main;
            let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), main, 0.5));
            let _ = app::update(state, app::Message::FxSend(-10.0));
            fx::set_return(state, main, fx::db_vol(-12.0));
            if let Some(&phones) = state.control_room.assign.phones_pairs().first() {
                fx::set_return(state, phones, fx::db_vol(-12.0));
            }
            state.tips_shown.retain(|t| t != "first:mute-fx");
            let _ = app::update(state, app::Message::MuteFxToggle);
            state.notice = None;
        }
        OpenPanel::PlacardNoReturn | OpenPanel::PlacardBoth => {
            state.notice = None;
            state.d_hint_done = true;
            state.fx.reverb_on = true;
            let main = state.control_room.assign.main;
            state.sel_out = main;
            let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), main, 0.5));
            let _ = app::update(state, app::Message::FxSend(-10.0));
            if panel == OpenPanel::PlacardBoth {
                state.fx.echo_on = true;
                fx::set_return(state, main, fx::db_vol(-12.0));
            }
        }
        OpenPanel::HelpMilestones => {
            state.notice = None;
            state.milestones_seeded = true;
            state.milestones = vec!["first-snapshot".into(), "mic-preset".into(), "headphone-mix".into(), "reverb-heard".into(), "first-take".into(), "first-take-room".into()];
            state.show_about = true;
            state.help_tab = about::HelpTab::Milestones;
        }
        OpenPanel::MilestoneRoom => {
            state.notice = None;
            state.milestones_seeded = true;
            state.milestones = vec!["first-snapshot".into(), "first-take".into(), "first-take-room".into()];
            state.celebration = Some(celebrate::Celebration {
                reward: celebrate::Reward::Milestone("first-take-room"),
                at: std::time::Instant::now(),
                chain: 2,
                run: 1,
                sounded: true,
            });
            state.capture_burst_t = Some(0.55);
        }
        OpenPanel::MuteFxStrip => {
            open_panel(state, OpenPanel::FxHeld);
            let main = state.control_room.assign.main;
            let out = if state.device.outputs_one_per_pair() { main } else { main * 2 };
            state.hovered_strip = Some(tuxmix_core::ChannelId::Output(out));
        }
        OpenPanel::MuteFxPreview => {
            let main = state.control_room.assign.main;
            fx::set_return(state, main, fx::db_vol(-12.0));
            state.mute_fx_hover = true;
        }
        // The simulation never brings it (`tricks::note_meter`): shown as
        // the unit would.
        OpenPanel::FirstMeter => {
            state.notice = None;
            state.d_hint_done = true;
            state.trick_card = Some((tricks::Card::First(tricks::FIRST_METER_HOT), std::time::Instant::now()));
        }
        OpenPanel::FirstRecFx => {
            state.notice = None;
            state.d_hint_done = true;
            state.sidebar_tab = Some(sidebar::Tab::Fx);
            state.tips_shown.retain(|t| t != "first:rec-fx");
            let _ = app::update(state, app::Message::FxOn(fx::Unit::Reverb, true));
            let _ = app::update(state, app::Message::SetRecordFx(fx::Unit::Reverb, false));
            let _ = app::update(state, app::Message::SetRecordFx(fx::Unit::Reverb, true));
            state.notice = None;
        }
        OpenPanel::PhonesMs => {
            use tuxmix_core::RmeDevice;
            let _ = state.device.set_output_ms(1, true);
            let _ = state.device.set_output_phase(2, true);
            let mut eq = app::flat_output_eq();
            eq.enabled = true;
            let _ = state.device.set_output_eq(2, eq);
            state.flyout_open.insert((tuxmix_core::ChannelId::Output(2), widgets::strip::FlyoutKind::Settings));
        }
        OpenPanel::Input => {
            state.flyout_open.insert((
                tuxmix_core::ChannelId::Input(0),
                widgets::strip::FlyoutKind::Settings,
            ));
        }
        OpenPanel::LinkArm => {
            let an1 = tuxmix_core::ChannelId::Input(0);
            state.flyout_open.insert((an1, widgets::strip::FlyoutKind::Settings));
            let _ = tuxmix_core::RmeDevice::set_input_phantom(&mut state.device, 0, true);
            let _ = tuxmix_core::RmeDevice::set_input_phantom(&mut state.device, 1, false);
            let _ = app::update(state, app::Message::StereoLinkChanged(an1, true));
        }
        OpenPanel::Collapsed => {
            state.collapsed.insert(tuxmix_core::ChannelId::Input(0));
            state.collapsed.insert(tuxmix_core::ChannelId::Input(1));
        }
        OpenPanel::Focus => state.focused_strip = Some(tuxmix_core::ChannelId::Input(2)),
        OpenPanel::FocusSelect => {
            // IN1/2 only selected, IN3/4 selected and focused: the glow has
            // to tell the two apart.
            for ch in 0..4 {
                state.selected.insert(tuxmix_core::ChannelId::Input(ch));
            }
            state.focused_strip = Some(tuxmix_core::ChannelId::Input(2));
            // And Mix 2 the active snapshot, so the slot lamps show all
            // three states: lit, stored, empty.
            state.active_snapshot = Some(2);
        }
        OpenPanel::TrayAsk => state.tray_asking = true,
        OpenPanel::QuitAsk => {
            state.quit_asking = Some(vec![
                "Apps playing to \u{201c}Audra Phones 3/4\u{201d} move to Main 1/2 (your speakers), without its shaping.".into(),
                "Apps recording from \u{201c}Audra processed mic\u{201d} lose it and switch to another mic.".into(),
            ])
        }
        OpenPanel::MoveAsk => {
            // The voice running on AN2; a pick on AN1 asked to move it.
            state.dyn_input = Some(1);
            state.dyn_move_ask = Some(0);
            app::open_flyout(state, tuxmix_core::ChannelId::Input(0), widgets::strip::FlyoutKind::Dynamics);
        }
        OpenPanel::MicCheck => {
            // A fit as if from ten seconds of talking in a slightly noisy
            // room: the report card and its fix.
            let cid = tuxmix_core::ChannelId::Input(0);
            app::open_flyout(state, cid, widgets::strip::FlyoutKind::Dynamics);
            let readings: Vec<(f32, f32)> = (0..300)
                .map(|i| if i % 3 == 0 { (-46.0 + (i % 5) as f32 * 0.5, -40.0) } else { let r = -32.0 + (i % 17) as f32; (r, r + 11.0) })
                .collect();
            if let Ok(fit) = dynamics_panel::fit_from(&readings) {
                state.dyn_fit_said = Some("Fitted to your voice: on the unit, your loud words get evened out and the room goes quiet between words. (Figures under More.)".to_string());
                state.dyn_fit_report = Some(fit);
            }
        }
        OpenPanel::DMarks => {
            state.dyn_input = Some(0);
            state.capture_reduction = Some(-9.0);
        }
        OpenPanel::KbCounter => {
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::Key("DIM")));
        }
        OpenPanel::KbPanel => {
            let an1 = tuxmix_core::ChannelId::Input(0);
            state.focused_strip = Some(an1);
            state.flyout_open.insert((an1, widgets::strip::FlyoutKind::Settings));
            state.key_focus = Some(keyfocus::KeyFocus::Panel(keyfocus::PanelItem::Gain));
        }
        OpenPanel::QuickTodo => {
            state.quick_channel = tuxmix_core::ChannelId::Input(0);
            let _ = app::update(state, app::Message::Phantom(0, false));
            let _ = app::update(state, app::Message::SelectOutput(1));
            let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), 1, 0.0));
        }
        // Mute FX held, Main's return kept at −12 dB: the lit key.
        OpenPanel::MuteFx => {
            let main = state.control_room.assign.main;
            fx::set_return(state, main, fx::db_vol(-12.0));
            fx::set_mute(state, true);
        }
        // Reverb & Echo: "Vocal room" loaded and being saved as your own.
        OpenPanel::FxSave => {
            state.sidebar_tab = Some(sidebar::Tab::Fx);
            state.page_card = true;
            fx::load_preset(state, fx::Unit::Reverb, fx::PresetPick::Factory(1));
            state.fx_naming = Some((fx::Unit::Reverb, "My close vocal".into()));
        }
        // The keyboard in Audra settings, on the Monitor page's Dim knob.
        OpenPanel::KbSettings => {
            state.show_settings = true;
            state.settings_page = app::SettingsPage::Monitor;
            state.key_focus = Some(keyfocus::KeyFocus::Item(keyfocus::first_id(keyfocus::Region::Settings, "Dim")));
        }
        // The keyboard in the Quick setup, on the gain.
        OpenPanel::KbQuick => {
            state.view = app::View::Quick;
            state.key_focus = Some(keyfocus::KeyFocus::Item(keyfocus::first_id(keyfocus::Region::Quick, "Gain")));
        }
        // A milestone's card, its burst a third of the way out (so the
        // capture shows it flying), second in the session's chain; late:
        // the sparks fading and the newest segment blooming.
        OpenPanel::Milestone | OpenPanel::MilestoneLate => {
            state.notice = None;
            state.milestones_seeded = true;
            state.milestones = vec!["first-snapshot".into(), "mic-preset".into(), "reverb-heard".into()];
            state.celebration = Some(celebrate::Celebration {
                reward: celebrate::Reward::Milestone("reverb-heard"),
                at: std::time::Instant::now(),
                chain: 1,
                run: 1,
                sounded: true,
            });
            state.capture_burst_t = Some(if matches!(panel, OpenPanel::MilestoneLate) { 0.55 } else { 0.35 });
        }
        // "Add reverb to my voice" under way: Reverb on, the send next,
        // with its "Show me".
        // The headphone-mix guide under way: its steps in the plain voice
        // (critique #86 A P3-2).
        OpenPanel::GuidePhones => {
            state.notice = None;
            let i = guides::GUIDES.iter().position(|g| g.id == "phones").unwrap_or(1);
            state.guide = Some(guides::Run::new(i, state));
        }
        OpenPanel::GuideStep => {
            state.notice = None;
            state.fx.reverb_on = true;
            state.guide = Some(guides::Run::new(0, state));
            // As its start does: its page open.
            state.sidebar_tab = Some(sidebar::Tab::Fx);
            state.page_card = true;
            if let Some(r) = state.guide.as_mut() {
                r.noted[0] = true;
            }
        }
        // AN1 clipping, its assist note in the counter's slot.
        OpenPanel::Assist => {
            state.notice = None;
            state.trick_card = None;
            state.celebration = None;
            state.tips = true;
            state.input_meters[0].light_clip_for_capture();
        }
        // AN1 just clipped: its LED's shockwave held a third of the way out.
        OpenPanel::ClipShock | OpenPanel::ClipShockLate => {
            state.notice = None;
            for m in state.input_meters.iter_mut().take(2) {
                m.light_clip_for_capture();
            }
            state.capture_shock_t = Some(if matches!(panel, OpenPanel::ClipShockLate) { 0.7 } else { 0.3 });
        }
        // A long notice in the counter's slot (a mic preset's load).
        OpenPanel::Notice => {
            state.notice = Some((
                "Loaded \u{201c}TLM 107 talking\u{201d} on AN1 and AN2: gain, PAD, phase, EQ and Low cut. Its 48V waits for you: click 48V? to switch phantom power on.".into(),
                std::time::Instant::now(),
            ));
        }
        // The open-mic warning opened to its why and its two answers.
        OpenPanel::OpenMic => {
            state.notice = None;
            state.open_mic_expanded = true;
        }
        // "Keep a mix as a snapshot" begun, on its page: the counter gives
        // the page room before its head.
        OpenPanel::GuideStore => {
            state.notice = None;
            state.guide = Some(guides::Run::new(2, state));
            state.sidebar_tab = Some(sidebar::Tab::Snapshots);
        }
        // "Keep a mix as a snapshot" just done, a third into its finish
        // (the steps relighting), with the milestone it reached.
        OpenPanel::FirstPad => {
            state.notice = None;
            state.d_hint_done = true;
            state.tips_shown.retain(|t| t != "first:pad");
            let _ = app::update(state, app::Message::Pad(0, true));
            state.notice = None;
            // Its sparks caught in flight, `AUDRA_SPARK_AT_MS` into them
            // when drawn (after the capture's 1.5 s of ticks).
            if let Some(ms) = std::env::var("AUDRA_SPARK_AT_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
                // On AN1 whatever sparked (PAD no longer does, critique #59).
                let cid = state.win_spark.map_or(tuxmix_core::ChannelId::Input(0), |(c, _)| c);
                let at = std::time::Instant::now() + std::time::Duration::from_millis(1500u64.saturating_sub(ms));
                state.win_spark = Some((cid, at));
            }
        }
        OpenPanel::FirstAbove0 => {
            state.notice = None;
            state.d_hint_done = true;
            state.tips_shown.retain(|t| t != "first:above0");
            let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(1), 0, 1.4));
            state.notice = None;
        }
        OpenPanel::SnapshotFx => {
            if let Some(slot) = state.snapshot_fx.get_mut(0) {
                *slot = Some((true, false));
            }
            if let Some(slot) = state.stored_snapshots.get_mut(0) {
                *slot = true;
            }
        }
        OpenPanel::GuideFinale => {
            state.notice = None;
            state.guides_done = guides::GUIDES.iter().map(|g| g.id.to_string()).collect();
            state.guides_all_on = Some("2026-11-14".into());
            let mut run = guides::Run::new(2, state);
            run.noted = vec![true; 3];
            run.finished = Some(std::time::Instant::now());
            run.finale = true;
            state.guide = Some(run);
            state.capture_burst_t = Some(0.35);
        }
        OpenPanel::GuideDone => {
            state.notice = None;
            state.guides_done = vec!["snapshot".into(), "keys".into()];
            let mut run = guides::Run::new(2, state);
            run.noted = vec![true; 3];
            run.finished = Some(std::time::Instant::now());
            run.milestone = Some("Your first snapshot");
            state.guide = Some(run);
            state.capture_burst_t = Some(0.35);
        }
        // Quick, its first step's tick drawing in and the level band lit,
        // held part way (`capture_burst_t`).
        OpenPanel::QuickLively | OpenPanel::QuickUnlit => {
            state.view = app::View::Quick;
            state.quick_channel = tuxmix_core::ChannelId::Input(0);
            let _ = app::update(state, app::Message::Phantom(0, true));
            let now = std::time::Instant::now();
            state.quick_done_at = [Some(now), None, None, None];
            state.quick_band_at = (panel == OpenPanel::QuickLively).then_some(now);
            state.capture_burst_t = Some(0.45);
        }
        // Reverb & Echo the moment the Reverb is first heard: its key
        // flaring, held part way.
        OpenPanel::FxHeard => {
            state.sidebar_tab = Some(sidebar::Tab::Fx);
            state.page_card = true;
            let pair = state.sel_out;
            let _ = app::update(state, app::Message::FxOn(fx::Unit::Reverb, true));
            let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), pair, 1.0));
            let _ = app::update(state, app::Message::FxMakeHeard(pair));
            state.fx_seen = Some((true, pair));
            state.fx_heard_at = Some(std::time::Instant::now());
            state.capture_burst_t = Some(0.08);
        }
        OpenPanel::FxHazard | OpenPanel::FxHazardPhones => {
            state.sidebar_tab = Some(sidebar::Tab::Fx);
            state.page_card = true;
            let main = state.control_room.assign.main;
            let _ = app::update(state, app::Message::FxOn(fx::Unit::Reverb, true));
            let _ = app::update(state, app::Message::FxMakeHeard(main));
            // Then the speakers' return turned up by hand.
            let _ = app::update(state, app::Message::FxReturn(main, 10f32.powf(-1.5 / 20.0)));
            // The setup's own "Made it heard" isn't part of the state shown
            // (critique #111 B: it read as contradicting the hazard line).
            state.notice = None;
            state.notice_undo = None;
            if panel == OpenPanel::FxHazardPhones {
                if let Some(&ph) = state.control_room.assign.phones_pairs().first() {
                    state.sel_out = ph;
                }
            }
        }
        // The keyboard on the counter's Groups bar.
        OpenPanel::KbPage => {
            state.sidebar_tab = None;
            state.milestones_seeded = true;
            state.milestones = celebrate::MILESTONES.iter().map(|m| m.id.to_string()).collect();
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::Page(sidebar::Tab::Groups)));
        }
        // Store armed by Alt+S, the keyboard's ring on Cancel.
        OpenPanel::KbStore => {
            state.milestones_seeded = true;
            state.milestones = celebrate::MILESTONES.iter().map(|m| m.id.to_string()).collect();
            let _ = app::update(state, app::Message::SnapshotStore);
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::Store));
        }
        // The keyboard in About, on the first guide's Start.
        OpenPanel::KbAbout => {
            state.show_about = true;
            state.key_focus = Some(keyfocus::KeyFocus::Item(keyfocus::first_id(keyfocus::Region::About, "reverb")));
        }
        // The counter with every page closed, and the Mute master just
        // pressed: its notice, and M holding the mutes it switched off.
        // F lit by a Fader group, and AS1/2 muted after the press, so a lit
        // mute stands beside the held ones.
        OpenPanel::DimOn => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::DimToggle);
        }
        OpenPanel::MixPhones => {
            state.sidebar_tab = None;
            let pair = state.control_room.assign.phones_pairs().first().copied().unwrap_or(1);
            let _ = app::update(state, app::Message::SelectOutput(pair));
        }
        OpenPanel::TalkbackOn => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::TalkbackToggle);
        }
        OpenPanel::PadOff => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::Pad(0, true));
            let _ = app::update(state, app::Message::Pad(0, false));
        }
        OpenPanel::PadOn => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::Pad(0, true));
            state.notice = None;
        }
        OpenPanel::OpenMicSeen => {
            state.sidebar_tab = None;
            state.open_mic_heard.insert(tuxmix_core::ChannelId::Input(0), std::time::Instant::now());
            let _ = app::update(state, app::Message::OpenMicExpand);
            let _ = app::update(state, app::Message::OpenMicExpand);
        }
        OpenPanel::FxKey => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::OpenFx(tuxmix_core::ChannelId::Output(0)));
        }
        OpenPanel::TakeSaved => {
            state.sidebar_tab = None;
            let dir = crate::recorder::default_folder();
            app::say_take(
                state,
                crate::recorder::RecState::Saved { path: dir.join("Audra 2026-11-13 13-44-47 AN1 processed.wav"), seconds: 9.2, why: None, parts: 1, gaps: 9, with_fx: (false, false) },
            );
        }
        OpenPanel::PadFolded => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::Pad(0, true));
            state.notice = None;
            state.collapsed.insert(tuxmix_core::ChannelId::Input(0));
        }
        OpenPanel::SidesApart => {
            state.sidebar_tab = None;
            state.open_floor = 1;
            state.source_floor = 1;
            let _ = tuxmix_core::RmeDevice::set_volume(&mut state.device, tuxmix_core::ChannelId::Playback(0), 0, 1.0);
            let _ = tuxmix_core::RmeDevice::set_volume(&mut state.device, tuxmix_core::ChannelId::Playback(1), 0, 0.35);
        }
        OpenPanel::Masters => {
            state.sidebar_tab = None;
            let _ = app::update(state, app::Message::Mute(tuxmix_core::ChannelId::Input(2), true));
            let _ = app::update(state, app::Message::GlobalMuteToggle);
            let _ = app::update(state, app::Message::Mute(tuxmix_core::ChannelId::Input(4), true));
            state.groups[0].fader_linked = true;
        }
        // Quick listening on the speakers with AN1's 48V on: the warning.
        OpenPanel::QuickSpeakers => {
            state.view = app::View::Quick;
            state.quick_channel = tuxmix_core::ChannelId::Input(0);
            state.quick_listen = Some(state.control_room.assign.main);
        }
        // The keyboard in AN1's EQ, on band 1's gain.
        OpenPanel::KbEq => {
            let cid = tuxmix_core::ChannelId::Input(0);
            state.flyout_open.insert((cid, widgets::strip::FlyoutKind::Eq));
            state.focused_strip = Some(cid);
            state.key_focus = Some(keyfocus::KeyFocus::Item(keyfocus::first_id(keyfocus::Region::Eq(cid), "Band 1 gain")));
        }
        // The keyboard on the Reverb's second knob.
        OpenPanel::KbFx => {
            state.sidebar_tab = Some(sidebar::Tab::Fx);
            state.page_card = true;
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::FxKnob(fx::Unit::Reverb, 1)));
        }
        OpenPanel::Tricks | OpenPanel::TricksAbout | OpenPanel::TricksRoll => {
            state.tricks_found = ["tab", "ms-keys", "nudge", "reset", "space"].iter().map(|s| s.to_string()).collect();
            if panel == OpenPanel::TricksRoll {
                state.notice = None;
                state.milestones_seeded = true;
                state.milestones = celebrate::MILESTONES.iter().map(|m| m.id.to_string()).collect();
                state.trick_streak = (3, Some(std::time::Instant::now()));
                // The D key's one-time hint outranks a trick: seen already.
                state.d_hint_done = true;
                // Its segment just lit, blooming (`tricks::newest_arrival`).
                let at = std::time::Instant::now().checked_sub(std::time::Duration::from_millis(190)).unwrap_or_else(std::time::Instant::now);
                state.trick_card = Some((tricks::Card::Found("space"), at));
            } else if panel == OpenPanel::Tricks {
                // The card waits behind a notice; the startup one is let go,
                // and every milestone is reached already, so none outranks it.
                state.notice = None;
                state.milestones_seeded = true;
                state.milestones = celebrate::MILESTONES.iter().map(|m| m.id.to_string()).collect();
                state.trick_card = Some((tricks::Card::Found("space"), std::time::Instant::now()));
            } else {
                state.show_about = true;
            }
        }
        OpenPanel::KbMatrix => {
            state.key_focus = Some(keyfocus::KeyFocus::Matrix(Some(tuxmix_core::ChannelId::Input(2)), 0));
        }
        OpenPanel::PatchPulse => {
            // AN1 patched into AS1/2's mix a moment ago.
            state.matrix_pulse = Some((
                tuxmix_core::ChannelId::Input(0),
                2,
                std::time::Instant::now(),
            ));
            motion::freeze_pulses_at(120);
        }
        OpenPanel::Shimmer(which) => {
            state.shimmer = Some((tuxmix_core::ChannelId::Input(2), which, std::time::Instant::now()));
            motion::freeze_pulses_at(380);
        }
        OpenPanel::HelpTabAt(tab) => {
            state.notice = None;
            state.show_about = true;
            state.help_tab = tab;
        }
        OpenPanel::FlowSilent => {
            let _ = app::update(state, app::Message::Setup(setups::Setup::Podcast));
            state.notice = None;
            open_panel(state, OpenPanel::Flow(flow::FlowOf::Source(tuxmix_core::ChannelId::Input(0))));
        }
        OpenPanel::FlowMixHeld => {
            open_panel(state, OpenPanel::FlowFx(flow::FlowOf::Mix(0)));
            fx::set_mute(state, true);
        }
        OpenPanel::FlowSourceNoSend => {
            state.notice = None;
            state.d_hint_done = true;
            state.fx.reverb_on = true;
            open_panel(state, OpenPanel::Flow(flow::FlowOf::Source(tuxmix_core::ChannelId::Input(0))));
        }
        OpenPanel::FxSendsBackFocus => {
            open_panel(state, OpenPanel::FxHeld);
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::FxSendsBack));
        }
        OpenPanel::Explain => {
            state.notice = None;
            state.explain = true;
        }
        OpenPanel::MySetupSaved | OpenPanel::MySetupReplace => {
            state.mock_my_setup = Some(control_room::capture_scene(state));
            state.my_setup_stored = true;
            state.my_setup_lit = true;
            state.my_setup_replacing = matches!(panel, OpenPanel::MySetupReplace);
            state.notice = None;
        }
        OpenPanel::MySetupRecalled => {
            let main = state.control_room.assign.main;
            let an1 = tuxmix_core::ChannelId::Input(0);
            let _ = app::update(state, app::Message::MySetupSave);
            let _ = app::update(state, app::Message::VolumeChanged(an1, main, 0.2));
            let _ = app::update(state, app::Message::MySetupRecall);
        }
        OpenPanel::SetupFresh | OpenPanel::SetupChanged => {
            let _ = app::update(state, app::Message::Setup(setups::Setup::Podcast));
            if matches!(panel, OpenPanel::SetupChanged) {
                let main = state.control_room.assign.main;
                let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(2), main, 0.4));
            }
            state.notice = None;
        }
        OpenPanel::SetupArmed(clicked) => {
            let _ = app::update(state, app::Message::Phantom(0, false));
            let _ = app::update(state, app::Message::Setup(setups::Setup::Podcast));
            if clicked {
                let _ = app::update(state, app::Message::Phantom(0, true));
            }
            state.notice = None;
        }
        OpenPanel::SetupRecord | OpenPanel::SetupStream | OpenPanel::SetupHeadphones => {
            let setup = match panel {
                OpenPanel::SetupRecord => setups::Setup::Record,
                OpenPanel::SetupStream => setups::Setup::Stream,
                _ => setups::Setup::Headphones,
            };
            let _ = app::update(state, app::Message::Setup(setup));
            state.notice = None;
        }
        OpenPanel::InputPlainOff => {
            open_panel(state, OpenPanel::PlainOff);
            open_panel(state, OpenPanel::Input);
        }
        OpenPanel::PlainOff => {
            let _ = app::update(state, app::Message::SetPlainNames(false));
            state.notice = None;
        }
        OpenPanel::OutputNoteFocus => {
            open_panel(state, OpenPanel::PhonesMs);
            state.flyout_open.clear();
            state.notice = None;
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::OutputNote));
        }
        OpenPanel::MixNameFocus | OpenPanel::FedByFocus => {
            state.notice = None;
            let item = if panel == OpenPanel::MixNameFocus { keyfocus::CounterItem::MixName } else { keyfocus::CounterItem::FedBy };
            state.key_focus = Some(keyfocus::KeyFocus::Counter(item));
        }
        OpenPanel::OnlyTheseFocus => {
            state.notice = None;
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::OnlyThese));
        }
        OpenPanel::OpenMicFocus => {
            state.notice = None;
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::OpenMicExpand));
        }
        OpenPanel::PlacardRowFocus => {
            open_panel(state, OpenPanel::PlacardNoReturn);
            state.key_focus = Some(keyfocus::KeyFocus::Counter(keyfocus::CounterItem::FxUnheard));
        }
        OpenPanel::FlowFx(of) => {
            // The Reverb on, AN1 sending and Main's return up: the maps'
            // Reverb lane (`flow-source-fx`, `flow-mix-fx`).
            state.notice = None;
            state.d_hint_done = true;
            state.fx.reverb_on = true;
            let main = state.control_room.assign.main;
            let _ = app::update(state, app::Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), main, 0.5));
            let _ = app::update(state, app::Message::FxSend(-10.0));
            fx::set_return(state, main, fx::db_vol(-12.0));
            open_panel(state, OpenPanel::Flow(of));
        }
        OpenPanel::Flow(of) => {
            // As its row in the panel opens it (issue #23).
            let _ = app::update(state, app::Message::FlowOpen(of));
            motion::freeze_pulses_at(500);
        }
        OpenPanel::PathPulse => {
            // Main just picked over the Phones: its feeding strips flare.
            // Dated ahead so the tick keeps it for the screenshot (held by
            // `freeze_pulses_at`).
            state.lamp_from = Some((1, std::time::Instant::now() + std::time::Duration::from_secs(60)));
            motion::freeze_pulses_at(150);
        }
        OpenPanel::Phones => {
            // The Phones' Settings panel, beside it in the Control Room.
            let pair = state.control_room.assign.phones_pairs().first().copied().unwrap_or(1);
            let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
            state.flyout_open.insert((
                tuxmix_core::ChannelId::Output(ch),
                widgets::strip::FlyoutKind::Settings,
            ));
            state.open_floor = 2;
        }
        OpenPanel::Output => {
            // AS1/2, the first output outside the Control Room by default.
            let ch = if state.device.outputs_one_per_pair() { 2 } else { 4 };
            state.flyout_open.insert((
                tuxmix_core::ChannelId::Output(ch),
                widgets::strip::FlyoutKind::Settings,
            ));
            state.open_floor = 2;
        }
    }
}

/// `--view quick|mixer|matrix`: the page the window opens on. `Quick` is
/// the default (the landing view), but the Mixer is where the full work
/// happens and where the device panel lives, so the caller can land there
/// directly. Unknown values fall back to the default, the same way
/// `--backend` falls back to auto-detect instead of opening nothing.
fn parse_view(args: &[String]) -> Option<app::View> {
    match arg_value(args, "--view").as_deref() {
        Some("quick") => Some(app::View::Quick),
        Some("mixer") => Some(app::View::Mixer),
        Some("matrix") => Some(app::View::Matrix),
        Some(other) => {
            eprintln!("Unknown --view value {other:?} (use quick|mixer|matrix), starting on Quick");
            None
        }
        None => None,
    }
}

const USAGE: &str = "\
Audra - hardware mixer for the RME Babyface Pro FS

Usage: audra [options]

Options:
  --mock                 run the simulation instead of the hardware
  --backend alsa|usb     force a backend (default: auto)
  --view quick|mixer|matrix
  --osc                  enable the OSC control surface (UDP, loopback)
  --osc-port PORT        OSC listen port (default 9000)
  --osc-send-port PORT   OSC send port (default 9001)
  --osc-host HOST        OSC send host (default 127.0.0.1)
  --osc-dialect NAME     OSC address space: native (default) or rme (alias: oscmix)
  --fullscreen           start fullscreen
  --minimized            start hidden in the tray
  --control              listen on $XDG_RUNTIME_DIR/audra-control.sock for automation
                         (press keys, read state, screenshot; see control.rs)
  --screenshot FILE.png  save the window once it has drawn, then quit
                         (preferences are not saved in this mode)
  --open settings|device|about|workspaces|eq|input|output|phones|collapsed
                         open that panel at start (eq: input 1's EQ;
                         input, output, phones: a strip's Settings panel;
                         collapsed: AN1 and AN2)
  --tab snapshots|fx|groups|layouts|view
                         open that tab in the right column
  --mix N                start editing output pair N's mix (0 = AN1/2, 1 = PH3/4, ...)
  -h, --help             show this help
";

/// The `RADV_DEBUG` value with DCC switched off, or `None` when it already
/// is. Mesa's AMD Vulkan driver (RADV) still flags RDNA4 (RX 9000 series)
/// as not conformant, and there delta color compression of the window's
/// render targets showed as glitter on every control while the pointer
/// moved (RX 9070 XT, Mesa 26.1.6, 2026-10-30); `nodcc` cured it, at a cost
/// that does not matter for a mixer window. A value the user set is kept.
fn radv_debug_with_nodcc(current: Option<&str>) -> Option<String> {
    match current.map(str::trim) {
        Some(v) if v.split(',').any(|flag| flag.trim() == "nodcc") => None,
        Some(v) if !v.is_empty() => Some(format!("{v},nodcc")),
        _ => Some("nodcc".to_owned()),
    }
}

fn main() -> iced::Result {
    // Before anything touches the GPU, and before any thread starts. Only
    // RADV reads the variable; AUDRA_KEEP_DCC=1 leaves the driver alone.
    if std::env::var_os("AUDRA_KEEP_DCC").is_none() {
        let current = std::env::var("RADV_DEBUG").ok();
        if let Some(value) = radv_debug_with_nodcc(current.as_deref()) {
            std::env::set_var("RADV_DEBUG", value);
        }
    }
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let mock = args.iter().any(|a| a == "--mock");

    // `None` = auto-detect (ALSA/kernel-driver first, USB fallback — see
    // `DeviceHandle::open_real`). An unrecognized value falls back to
    // auto rather than silently opening nothing.
    let backend = arg_value(&args, "--backend").and_then(|v| match v.as_str() {
        "alsa" | "usb" => Some(v),
        other => {
            eprintln!("Unknown --backend value {other:?} (use alsa|usb), auto-detecting");
            None
        }
    });

    // Opt-in, loopback-only OSC control surface — see osc.rs. Off unless
    // `--osc` is passed, so enabling it is always a deliberate choice.
    // `--control`: the local control socket for automation (`control.rs`).
    let control_socket = args.iter().any(|a| a == "--control");
    let osc_config = args.iter().any(|a| a == "--osc").then(|| osc::OscConfig {
        recv_port: arg_value(&args, "--osc-port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(9000),
        send_port: arg_value(&args, "--osc-send-port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(9001),
        send_host: arg_value(&args, "--osc-host")
            .and_then(|v| v.parse::<IpAddr>().ok())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        dialect: arg_value(&args, "--osc-dialect")
            .and_then(|v| {
                osc::Dialect::from_arg(&v).or_else(|| {
                    eprintln!("Unknown --osc-dialect {v:?} (use native|rme), using native");
                    None
                })
            })
            .unwrap_or_default(),
    });

    let start_view = parse_view(&args);
    let start_panel = parse_open(&args);
    let start_tab = parse_tab(&args);
    let start_mix = arg_value(&args, "--mix").and_then(|m| m.parse::<usize>().ok());
    let screenshot = arg_value(&args, "--screenshot").map(PathBuf::from);
    if screenshot.is_some() {
        app::CAPTURE_RUN.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    // Audra in the app launcher and the dock (`launcher`), for the copy
    // that's running, unless switched off in Audra settings → App; never
    // from a screenshot or the simulation, which mustn't touch the user's
    // folders.
    let launcher_on = prefs::path().is_none_or(|p| prefs::load(&p).launcher_entry != Some(false));
    if screenshot.is_none() && !mock && launcher_on {
        std::thread::spawn(|| {
            if let Err(e) = launcher::install() {
                log::warn!("Could not add Audra to the app launcher: {e}");
            }
        });
    }
    // `--size WxH` for `--screenshot` (logical pixels; 1280x700 by default).
    let shot_size = arg_value(&args, "--size")
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some(Size::new(w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or(Size::new(1280.0, 700.0));
    // `--cursor X,Y` for `--screenshot`: a pointer there, so hover states
    // can be checked (each theme's hover lift, the cursor's own look not
    // included).
    let shot_cursor = arg_value(&args, "--cursor").and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some(iced::Point::new(x.parse().ok()?, y.parse().ok()?))
    });
    let saved_size = prefs::path()
        .and_then(|p| prefs::load(&p).window_size)
        .map(|(w, h)| Size::new(w.max(app::MIN_WINDOW.0), h.max(app::MIN_WINDOW.1)))
        .unwrap_or(Size::new(1280.0, 800.0));
    let start_fullscreen = args.iter().any(|a| a == "--fullscreen");

    // One TuxMix per session: a second launch asks the first to show its
    // window (it may be hidden in the tray) and leaves.
    // Not a `--screenshot` run: it has no window to show, and it must never
    // raise a simulation window the owner has open, nor leave without its
    // picture and report success (two capture runs at once did, 2026-11-18).
    if screenshot.is_none() && matches!(tray::claim(mock), tray::Instance::Secondary) {
        eprintln!("Audra is already running: asked it to show its window.");
        return Ok(());
    }
    // The tray icon (not for the simulation, which would put a second icon
    // beside the real one). `--minimized` starts hidden in it, and only when
    // there is one: a hidden window with no icon has no way back.
    let tray_active = !mock
        && tray::start(tray::Status {
            window_visible: true,
            dim: false,
        });
    let start_hidden = tray_active && args.iter().any(|a| a == "--minimized");

    // A daemon, not a plain application: the window is opened and closed by
    // the app. Hiding it in the tray means closing it while the process
    // lives on (winit cannot hide a window on Wayland, and an application
    // exits with its last window).
    iced::daemon(
        move || {
            let mut state = app::new(mock, osc_config, backend.clone());
            state.control_socket = control_socket;
            state.settle_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
            // Restore the window's own layout (view, sidebar, rows, zoom,
            // channel names) before the command line overrides any of it.
            // Themes of the user's own first, so a choice of one in the
            // preferences finds it.
            if let Some(dir) = usertheme::dir() {
                let (custom, problems) = usertheme::load_dir(&dir);
                for p in problems {
                    log::warn!("Theme file: {p}");
                }
                theme::register(custom);
            }
            let mut prefs_trouble = None;
            if let Some(path) = prefs::path_for(state.device.is_mock()) {
                let (saved, trouble) = prefs::read(&path);
                // A file that didn't read whole is copied aside before the
                // first save writes over it, and the start says so.
                if let Some(t) = trouble {
                    let kept = prefs::keep_bad(&path);
                    log::warn!("Settings file {}: {t:?}", path.display());
                    prefs_trouble = Some(prefs::trouble_words(&t, kept.as_deref()));
                }
                // The first run opens Quick setup (critique #7, the owner's
                // choice): a newcomer starts with their mic, not the desk.
                state.view = app::view_at_start(&saved);
                app::apply_prefs(&mut state, saved);
                state.prefs_path = Some(path);
            }
            app::restore_engine(&mut state);
            // The mic preset chosen to load at start (Audra settings →
            // Mixer), after the prefs, the device and the saved mix.
            app::apply_startup_mic_preset(&mut state);
            // Every start is the plain talking setup: nothing to or from
            // Reverb and Echo (the owner, 2026-11-10); the "Talking +
            // reverb" snapshot, put in once, brings them in.
            fx::sends_and_returns_off(&mut state);
            app::seed_fun_snapshot(&mut state);
            // Then every linked pair whole: a side left down plays nothing
            // under its fader (2026-11-06, one ear silent at start).
            app::align_linked_pairs(&mut state);
            // Main came back dimmed (`control_room::recover`, issue #22):
            // said after the start's own notices (the mic preset loaded at
            // start says its own), so it isn't lost under them.
            if state.control_room.is_dimmed() {
                let at = std::time::Instant::now();
                state.notice = Some((control_room::DIMMED_AT_START.to_string(), at));
                state.notice_sticky_at = Some(at);
            }
            // Last, so no start notice takes its place; it stays until closed.
            if let Some(words) = prefs_trouble {
                let at = std::time::Instant::now();
                state.notice = Some((words, at));
                state.notice_sticky_at = Some(at);
            }
            // The demo studio (`AUDRA_MOCK_DEMO`) has a few snapshots stored,
            // so the wiki's pictures of the Snapshots tab aren't all empty.
            if mock && std::env::var_os("AUDRA_MOCK_DEMO").is_some() {
                for slot in 0..3 {
                    state.mock_snapshots[slot] = Some(app::capture_demo_scene(&state));
                    state.stored_snapshots[slot] = true;
                }
            }
            if let Some(view) = start_view {
                state.view = view;
            }
            if let Some(panel) = start_panel {
                open_panel(&mut state, panel);
            }
            if let Some(mix) = start_mix.filter(|&m| m < tuxmix_core::RmeDevice::output_pair_count(&state.device)) {
                state.sel_out = mix;
            }
            if let Some(tab) = start_tab {
                state.sidebar_tab = Some(tab);
                state.page_card = true;
                state.sidebar_open = true;
            }
            state.fullscreen = start_fullscreen;
            state.tray_active = tray_active;
            state.window_hidden = start_hidden;
            if start_hidden {
                return (state, iced::Task::none());
            }
            let (id, open) = window::open(window_settings(saved_size, start_fullscreen));
            state.window_id = Some(id);
            // `--screenshot`: let the window draw, save it, and quit. A
            // capture must not touch the user's preferences: the panels it
            // opens and the window size it gets are not choices they made.
            if let Some(path) = screenshot.clone() {
                state.prefs_path = None;
                let _ = (open, id);
                match capture_offscreen(&mut state, &path, shot_size, shot_cursor) {
                    Ok(()) => println!("Saved {}", path.display()),
                    Err(err) => eprintln!("Could not save {}: {err}", path.display()),
                }
                return (state, iced::exit());
            }
            // First launch only (no saved zoom): ask the monitor's size to
            // pick a starting zoom.
            let task = if state.initial_zoom_pending {
                open.then(move |_| window::monitor_size(id))
                    .map(app::Message::MonitorSize)
            } else {
                open.discard()
            };
            (state, task)
        },
        app::update,
        app::view,
    )
    .title(app::title)
    .subscription(app::subscription)
    .theme(|state: &app::TuxMix, _id: window::Id| {
        theme::set_palette(&themetime::shown_palette(state));
        theme::iced_theme()
    })
    .font(FONTS[0])
    .font(FONTS[1])
    .font(FONTS[2])
    .font(FONTS[3])
    .font(FONTS[4])
    .default_font(theme::SANS)
    // Smooth edges on every drawn shape (knobs, meters, icons): without it
    // each curve was stair-stepped ("choppy", the owner, 2026-11-03).
    .antialiasing(true)
    .run()
}

/// The main window: `size` floored at the minimum, the app's icon and id.
pub fn window_settings(size: Size, fullscreen: bool) -> window::Settings {
    window::Settings {
        // The close button is handled by the app (`Message::CloseRequested`):
        // hide in the tray, or save and quit.
        exit_on_close_request: false,
        size,
        // See `app::MIN_WINDOW`.
        min_size: Some(Size::new(app::MIN_WINDOW.0, app::MIN_WINDOW.1)),
        fullscreen,
        icon: window_icon(),
        platform_specific: window::settings::PlatformSpecific {
            application_id: APP_ID.to_owned(),
            ..Default::default()
        },
        // No enforced minimum — the adaptive scale (down to `SCALE_MIN`)
        // plus per-row horizontal scroll and the page's vertical scroll
        // handle small windows fine now, so a fixed 960×600 floor is just
        // an outdated restriction on shrinking.
        ..window::Settings::default()
    }
}

/// Writes a window screenshot as an RGBA PNG.
pub(crate) fn save_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

/// `--screenshot`: the window drawn off screen, at `size`.
///
/// It used to open the window and grab it after two seconds. A Wayland
/// compositor stops sending frames to a window it isn't showing (behind
/// another, on another workspace, the GPU busy), so that grab was often
/// the window's very first frame: no text yet, no meters, the rows not
/// folded (2026-11-03, the "flaky capture tool"). Now the app runs as a
/// window would, a resize to `size` and 1.5 s of real ticks (meters fill,
/// motion settles), then the UI is laid out and drawn by iced's headless
/// renderer, with no window and no compositor involved. Hover states show
/// only for a pointer placed with `cursor` (`--cursor X,Y`); tooltips
/// never do.
fn capture_offscreen(state: &mut app::TuxMix, path: &Path, size: Size, cursor: Option<iced::Point>) -> Result<(), String> {
    let cursor = cursor.map_or(iced::mouse::Cursor::Unavailable, iced::mouse::Cursor::Available);
    use iced::advanced::renderer::Headless;
    {
        let mut fonts = iced::advanced::graphics::text::font_system()
            .write()
            .map_err(|_| "the font system is poisoned".to_string())?;
        for bytes in FONTS {
            fonts.load_font(std::borrow::Cow::Borrowed(bytes));
        }
    }
    let _ = app::update(state, app::Message::WindowSize(size.width, size.height));
    for _ in 0..45 {
        std::thread::sleep(std::time::Duration::from_millis(33));
        let _ = app::update(state, app::Message::Tick);
        let _ = app::update(state, app::Message::CollapseTick);
    }
    // `AUDRA_CAPTURE_BACKEND=tiny-skia`: drawn on the CPU with exact edge
    // smoothing, a reference for the GPU's four samples a pixel (the
    // knobs' stepped rings, 2026-11-14).
    let backend = std::env::var("AUDRA_CAPTURE_BACKEND").ok();
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        theme::SANS,
        iced::Pixels(16.0),
        backend.as_deref(),
    ))
    .ok_or("no headless renderer (no GPU adapter or software fallback)")?;
    theme::set_palette(&themetime::shown_palette(state));
    let iced_theme = theme::iced_theme();
    let id = window::Id::unique();
    let first = iced_runtime::UserInterface::build(
        app::view(state, id),
        size,
        iced_runtime::user_interface::Cache::new(),
        &mut renderer,
    );
    // Built again, as a window is laid out on every tick: what a layout
    // measures shows on the next build (the column's head, `sidebar`; the
    // top bar's room, which steps down a frame at a time). Six builds, so
    // those settle as a live window's do over its first frames (critique
    // #59: two left the device chip and the column's keys unsettled).
    let mut cache = first.into_cache();
    for _ in 0..5 {
        let _ = app::update(state, app::Message::Tick);
        cache = iced_runtime::UserInterface::build(app::view(state, id), size, cache, &mut renderer).into_cache();
    }
    let mut ui = iced_runtime::UserInterface::build(app::view(state, id), size, cache, &mut renderer);
    // The redraw a window gets before it draws: widgets settle their state
    // on it (a button with no status yet draws as disabled).
    let mut messages = Vec::new();
    let mut events = Vec::new();
    if let iced::mouse::Cursor::Available(at) = cursor {
        events.push(iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at }));
    }
    events.push(iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now())));
    // A preset being named has the keyboard in its field, as "Save as new…"
    // puts it there (`fx-save`): the same focus operation, on this tree.
    if state.fx_naming.is_some() {
        let id = iced::advanced::widget::Id::new(fx::NAME_INPUT_ID);
        ui.operate(&renderer, &mut iced::advanced::widget::operation::focusable::focus(id.clone()));
        ui.operate(&renderer, &mut iced::advanced::widget::operation::text_input::select_all(id));
    }
    let _ = ui.update(
        &events,
        cursor,
        &mut renderer,
        &mut iced::advanced::clipboard::Null,
        &mut messages,
    );
    // `AUDRA_PROBE="wheel X Y; press X Y; hover X Y; snap F; row N F"`:
    // send each gesture to the UI as laid out here and print the messages
    // it makes; with `AUDRA_PROBE_APPLY=1` they are applied too and the
    // window rebuilt, as a running window does (an opened list says so,
    // and the next gesture sees it). A check that a pointer over a floating
    // panel never reaches a fader under it (issue #25); `snap F` scrolls the
    // panel the keyboard is in to F (0 top, 1 foot) as a keyboard step
    // does, and the capture shows where it landed. For testing only.
    let apply = std::env::var_os("AUDRA_PROBE_APPLY").is_some();
    if let Ok(probe) = std::env::var("AUDRA_PROBE") {
        for gesture in probe.split(';').map(str::trim).filter(|g| !g.is_empty()) {
            let words: Vec<&str> = gesture.split_whitespace().collect();
            if let (Some(&"snap"), Some(Ok(f))) = (words.first(), words.get(1).map(|w| w.parse::<f32>())) {
                ui.operate(
                    &renderer,
                    &mut iced::advanced::widget::operation::scrollable::snap_to(
                        iced::advanced::widget::Id::new(app::PANEL_SCROLL_ID),
                        iced::widget::scrollable::RelativeOffset { x: None, y: Some(f) },
                    ),
                );
                // The redraw a window gets next, which moves the overlays.
                let redraw = [iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))];
                let _ = ui.update(&redraw, cursor, &mut renderer, &mut iced::advanced::clipboard::Null, &mut Vec::new());
                eprintln!("probe {gesture}: done");
                continue;
            }
            // `page F`: scroll the mixer's page down to F (0 top, 1 foot),
            // as a window scrolled down shows it.
            if let (Some(&"page"), Some(Ok(f))) = (words.first(), words.get(1).map(|w| w.parse::<f32>())) {
                ui.operate(
                    &renderer,
                    &mut iced::advanced::widget::operation::scrollable::snap_to(
                        iced::advanced::widget::Id::new(app::FLOORS_SCROLL_ID),
                        iced::widget::scrollable::RelativeOffset { x: None, y: Some(f) },
                    ),
                );
                let redraw = [iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))];
                let _ = ui.update(&redraw, cursor, &mut renderer, &mut iced::advanced::clipboard::Null, &mut Vec::new());
                eprintln!("probe {gesture}: done");
                continue;
            }
            // `row N F`: scroll strip row N sideways to F (0 start, 1 end).
            if let (Some(&"row"), Some(Ok(n)), Some(Ok(f))) =
                (words.first(), words.get(1).map(|w| w.parse::<usize>()), words.get(2).map(|w| w.parse::<f32>()))
            {
                if let Some(&id) = app::ROW_SCROLL_IDS.get(n) {
                    ui.operate(
                        &renderer,
                        &mut iced::advanced::widget::operation::scrollable::snap_to(
                            iced::advanced::widget::Id::new(id),
                            iced::widget::scrollable::RelativeOffset { x: Some(f), y: None },
                        ),
                    );
                }
                // The redraw a window gets next, which moves the overlays;
                // what the scroll reports (where the row stands) is applied
                // with `AUDRA_PROBE_APPLY`, as a window's is.
                let redraw = [iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))];
                let mut made = Vec::new();
                let _ = ui.update(&redraw, cursor, &mut renderer, &mut iced::advanced::clipboard::Null, &mut made);
                eprintln!("probe {gesture}: done {made:?}");
                if apply && !made.is_empty() {
                    let cache = ui.into_cache();
                    for message in made {
                        let _ = app::update(state, message);
                    }
                    ui = iced_runtime::UserInterface::build(app::view(state, id), size, cache, &mut renderer);
                    let _ = ui.update(&redraw, cursor, &mut renderer, &mut iced::advanced::clipboard::Null, &mut Vec::new());
                }
                continue;
            }
            let (Some(kind), Some(Ok(x)), Some(Ok(y))) =
                (words.first(), words.get(1).map(|w| w.parse::<f32>()), words.get(2).map(|w| w.parse::<f32>()))
            else {
                eprintln!("probe {gesture:?}: want \"wheel X Y\" or \"press X Y\"");
                continue;
            };
            let at = iced::Point::new(x, y);
            let mut sent = vec![iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: at })];
            if *kind == "hover" {
                // Past a hint's delay (400 ms), then the redraw that opens
                // it; with `--cursor X,Y` at the same place, the capture
                // shows it.
                let mut made = Vec::new();
                let _ = ui.update(&sent, iced::mouse::Cursor::Available(at), &mut renderer, &mut iced::advanced::clipboard::Null, &mut made);
                eprintln!("probe {gesture} (moved): {made:?}");
                made.clear();
                std::thread::sleep(std::time::Duration::from_millis(450));
                let redraw = [iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))];
                let _ = ui.update(&redraw, iced::mouse::Cursor::Available(at), &mut renderer, &mut iced::advanced::clipboard::Null, &mut made);
                eprintln!("probe {gesture}: {made:?}");
                continue;
            }
            match *kind {
                "wheel" => sent.push(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                    delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
                })),
                // `down X Y`: the wheel turned towards the foot (issue #44:
                // a panel scrolled to its foot has to stay there).
                "down" => sent.push(iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                    delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
                })),
                "press" => {
                    sent.push(iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)));
                    sent.push(iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)));
                }
                other => {
                    eprintln!("probe {gesture:?}: unknown gesture {other:?}");
                    continue;
                }
            }
            let mut made = Vec::new();
            let _ = ui.update(
                &sent,
                iced::mouse::Cursor::Available(at),
                &mut renderer,
                &mut iced::advanced::clipboard::Null,
                &mut made,
            );
            // The redraw a window gets next: a captured press drops the
            // overlays' layout until then (an open menu, a panel).
            let redraw = [iced::Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))];
            let _ = ui.update(&redraw, iced::mouse::Cursor::Available(at), &mut renderer, &mut iced::advanced::clipboard::Null, &mut made);
            eprintln!("probe {gesture}: {made:?}");
            if apply && !made.is_empty() {
                let cache = ui.into_cache();
                let mut tasks = Vec::new();
                for message in made {
                    // Never from a probe: what would reach the desktop or
                    // the sound system (a file dialog, a recording, a sound,
                    // the Voice's virtual mic, the sound card's profile,
                    // the launcher entry), whichever press made it: the
                    // second press of a pair meets whatever the first one's
                    // change put under the pointer.
                    // The whole message as printed, not its first name alone:
                    // a wrapper carries the message it will run
                    // (`KeyShortcut(_, inner)`), and a function key runs
                    // whatever the user bound to it (OpenCode's job 80).
                    let whole = format!("{message:?}");
                    let name = whole.split(['(', ' ', '{']).next().unwrap_or_default().to_string();
                    // ("Hear" is not among them: it held back only Make it
                    // heard, which sets two levels; every sound's message
                    // has "Sound" or "Clap" in its name.)
                    // ("Audition" is the clap through the effects: the list
                    // had "Clap", which no message is named; the same job.)
                    const HELD: [&str; 22] = [
                        "File", "Folder", "Import", "Export", "Pick", "Url", "ProAudio", "Autostart", "Launcher", "Quit", "Record", "Take", "Sound",
                        "Audition", "FunctionKey", "Device", "Engine", "Shape", "Dyn", "Voice", "Midi", "Osc",
                    ];
                    if HELD.iter().any(|h| whole.contains(h)) {
                        eprintln!("probe {gesture}: {name} not applied (it would reach the desktop or the sound system)");
                        continue;
                    }
                    tasks.push(app::update(state, message));
                }
                // `AUDRA_PROBE_WAIT_MS`: drawn that long after the press, to
                // see a motion part way (a fold's closing step).
                if let Some(ms) = std::env::var("AUDRA_PROBE_WAIT_MS").ok().and_then(|v| v.parse::<u64>().ok()) {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
                ui = iced_runtime::UserInterface::build(app::view(state, id), size, cache, &mut renderer);
                let mut after = Vec::new();
                let _ = ui.update(&redraw, iced::mouse::Cursor::Available(at), &mut renderer, &mut iced::advanced::clipboard::Null, &mut after);
                // What the press asked of the widgets (a panel scrolled to a
                // row, issue #40), as the runtime would run it on the new
                // window; anything that isn't ready at once is left.
                for task in tasks {
                    use iced::futures::{FutureExt, StreamExt};
                    let Some(mut stream) = iced_runtime::task::into_stream(task) else { continue };
                    while let Some(Some(action)) = stream.next().now_or_never() {
                        if let iced_runtime::Action::Widget(mut operation) = action {
                            ui.operate(&renderer, operation.as_mut());
                            eprintln!("probe {gesture}: ran a widget operation");
                        }
                    }
                    let _ = ui.update(&redraw, iced::mouse::Cursor::Available(at), &mut renderer, &mut iced::advanced::clipboard::Null, &mut after);
                }
                // `AUDRA_PROBE_TICKS`: that many of the window's ticks after
                // the press and what it asked of the widgets, each with its
                // rebuild, so a panel it opened finishes opening as it does
                // in a window: after the scroll was asked for, not before.
                for _ in 0..std::env::var("AUDRA_PROBE_TICKS").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0) {
                    std::thread::sleep(std::time::Duration::from_millis(33));
                    let cache = ui.into_cache();
                    let _ = app::update(state, app::Message::Tick);
                    let _ = app::update(state, app::Message::CollapseTick);
                    ui = iced_runtime::UserInterface::build(app::view(state, id), size, cache, &mut renderer);
                    let _ = ui.update(&redraw, iced::mouse::Cursor::Available(at), &mut renderer, &mut iced::advanced::clipboard::Null, &mut after);
                }
            }
        }
    }
    ui.draw(
        &mut renderer,
        &iced_theme,
        &iced::advanced::renderer::Style { text_color: theme::text_primary() },
        cursor,
    );
    let (w, h) = (size.width as u32, size.height as u32);
    let rgba = renderer.screenshot(iced::Size::new(w, h), 1.0, theme::bg_deep());
    save_png(path, &rgba, w, h).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{parse_open, parse_tab, parse_view, radv_debug_with_nodcc, OpenPanel};
    use crate::app::View;

    #[test]
    fn dcc_is_switched_off_without_losing_the_users_radv_flags() {
        assert_eq!(radv_debug_with_nodcc(None).as_deref(), Some("nodcc"));
        assert_eq!(radv_debug_with_nodcc(Some("")).as_deref(), Some("nodcc"));
        assert_eq!(
            radv_debug_with_nodcc(Some("nohiz")).as_deref(),
            Some("nohiz,nodcc")
        );
        assert_eq!(radv_debug_with_nodcc(Some("nohiz, nodcc")), None);
    }

    fn args_with_view(value: &str) -> Vec<String> {
        vec!["tuxmix-gui".into(), "--view".into(), value.into()]
    }

    #[test]
    fn view_flag_picks_the_starting_page() {
        assert_eq!(parse_view(&args_with_view("quick")), Some(View::Quick));
        assert_eq!(parse_view(&args_with_view("mixer")), Some(View::Mixer));
        assert_eq!(parse_view(&args_with_view("matrix")), Some(View::Matrix));
        assert_eq!(parse_view(&args_with_view("wat")), None);
    }

    #[test]
    fn open_and_tab_flags_pick_what_shows_at_start() {
        let args = |flag: &str, v: &str| -> Vec<String> {
            vec!["audra".into(), flag.into(), v.into()]
        };
        assert_eq!(parse_open(&args("--open", "settings")), Some(OpenPanel::Settings));
        assert_eq!(parse_open(&args("--open", "eq")), Some(OpenPanel::Eq));
        assert_eq!(parse_open(&args("--open", "wat")), None);
        assert_eq!(parse_tab(&args("--tab", "view")), Some(crate::sidebar::Tab::View));
        assert_eq!(parse_tab(&["audra".into()]), None);
    }

    #[test]
    fn view_flag_absent_starts_on_the_default() {
        assert_eq!(parse_view(&["tuxmix-gui".into()]), None);
        assert_eq!(parse_view(&["tuxmix-gui".into(), "--mock".into()]), None);
    }
}
