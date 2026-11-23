//! The About dialog: what the app is, which version of it is running, and
//! which device it is driving. Opened from the brand lockup in the top bar
//! (the mark + wordmark, see `app::top_bar`), it draws as a centered modal
//! above the whole window (see `about_modal`) rather than the docked drawer
//! it used to be: that drawer was pushed after the page content, so on the
//! Quick view it landed below the fold and on the Mixer view it came after
//! every strip row.

use iced::widget::{button, column, container, mouse_area, opaque, row, scrollable, text, Space};
use iced::{Alignment, Background, Color, Element, Length};

use tuxmix_core::RmeDevice;

use crate::app::{Message, TuxMix};
use crate::theme;

/// Upstream TuxMix by Ismaïl Bahloul (GPL-3.0), which Audra's mixer is
/// built on; credited and linked from the About card.
pub const REPO_URL: &str = "https://github.com/ismail-bahloul/TuxMix";

/// The how-to wiki, one page per part of the app.
pub const WIKI_URL: &str = "https://github.com/Vyeche/Audra/wiki";

/// The card's width cap at `ui_scale == 1.0`: wide enough for the repo URL
/// on one line, narrow enough to read as a dialog, and capped rather than
/// fixed so a narrow window shrinks it instead of clipping it.
const CARD_W: f32 = 520.0;

/// Help's body, under its standing head (the keyboard scrolls it).
pub const HELP_SCROLL_ID: &str = "help-scroll";

fn header(scale: f32) -> Element<'static, Message> {
    row![
        // Help, the learning first (critique #8: eight guides filed under
        // "About", where a newcomer wouldn't look); Audra's own card at
        // the foot.
        // In the serif, as every panel's head (critique #56: 14 px sans
        // beside "Audra settings" in the serif).
        text("Help")
            .font(theme::SERIF)
            .color(theme::text_primary())
            .size(theme::TEXT_ROW * scale),
        Space::new().width(Length::Fill),
        // The cards' drawn × (critique #7: "Close" in words here, × on
        // every card).
        crate::widgets::strip::hint(
            button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                .padding(0)
                .width(Length::Fixed(20.0 * scale))
                .height(Length::Fixed(20.0 * scale))
                .style(theme::icon_button(false))
                .on_press(Message::ToggleAbout),
            "Close (Esc)",
            scale,
        ),
    ]
    .spacing(theme::SPACE_MD * scale)
    .align_y(Alignment::Center)
    .into()
}

/// Every gesture and key the app answers to, in one place: most of them
/// (double-clicks, Ctrl/Shift-clicks, the Matrix drags) are otherwise only
/// discoverable by accident or by a tooltip you have to hover first. In
/// groups (the critique, 2026-11-05: 27 rows in one list, the last of them
/// cut off by the window).
const SHORTCUTS: [(&str, &[(&str, &str)]); 5] = [
    (
        "Views and panels",
        &[
            ("Ctrl+Tab", "Next view (Quick setup, Mixer, Matrix)"),
            ("X", "Matrix view"),
            ("F1", "This list"),
            ("?", "What's this? Point at anything to read what it does (? or Esc ends it)"),
            ("F2 / Ctrl ,", "Audra settings (also the top bar's settings button)"),
            ("F3", "Device settings (clock, sample rate)"),
            ("W", "Workspaces"),
            ("F", "Faders show FX sends, or the mix again (Esc too)"),
            ("Ctrl + / Ctrl - / Ctrl 0", "Zoom in / out / reset (also Ctrl+wheel)"),
            ("Ctrl+1 / Ctrl+2 / Ctrl+3", "Fold or open the inputs, playback and outputs rows"),
            ("F11", "Full screen"),
            ("Ctrl+W / Ctrl+Q", "Close the window (to the tray, where there is one) / quit Audra"),
            ("Esc", "Close a dialog or a strip's panel, cancel an edit or a store"),
        ],
    ),
    (
        "The strip you're on",
        &[
            ("Tab / Shift+Tab", "On through the strips, a strip's open panel and the right-hand column; in Quick setup its steps, in Audra settings its options"),
            ("\u{2190} \u{2192}", "Walk the Mixer's strips (Esc lets go)"),
            ("M / S", "Mute / solo it (M with none: the Mixer)"),
            ("Space", "Open / close its Settings panel"),
            ("Shift+\u{2190} \u{2192}", "Move its pan, in the mix you're editing"),
            ("N", "Narrow or widen it"),
            ("Space / Enter", "Elsewhere: press the button the keyboard is on, switch the row, load the preset, set the Matrix cell to 0 dB or off"),
            ("+ / \u{2212}", "Change the level, gain or preset the keyboard is on (\u{2190} \u{2192} on a level too)"),
            ("\u{2191} / \u{2193}", "Nudge its fader, else the one under the pointer (Shift: finer)"),
        ],
    ),
    (
        "Settings, panels and Reverb",
        &[
            ("Tab in Audra settings", "Walk its options (held inside it); \u{2190} \u{2192} turn the one you're on"),
            ("Tab into a panel", "From a strip on into its open EQ or Voice"),
            ("Tab in Quick setup", "Walk the setup's controls"),
            ("\u{2190} \u{2192} on Reverb & Echo", "Turn a knob, change the type, or flip presets as you listen"),
        ],
    ),
    (
        "Mixes and snapshots",
        &[
            ("[ / ]", "Edit the previous / next mix (Main, Phones\u{2026})"),
            ("Alt+1 \u{2026} Alt+8", "Recall snapshot 1\u{2013}8 (after Store\u{2026}: store there)"),
            ("Alt+S", "Store\u{2026}: then Alt and a slot's number keeps the mix there"),
            ("Ctrl+Z / Ctrl+Shift+Z", "Undo / redo"),
            ("F4 \u{2026} F8", "Key Commands: F4 Dim, F5 Talkback, F6 Mute master; set in Audra settings \u{2192} Remote"),
            ("C (hold)", "Cough: silence what others hear through Voice (\u{201c}Audra processed mic\u{201d}) while it's held; Control Room has the key too"),
        ],
    ),
    (
        "Mouse",
        &[
            ("Ctrl / Shift + click", "Select strips; their faders move together"),
            ("Right-click a strip", "Its Settings panel (Copy, Paste, Hide)"),
            ("Right-click a snapshot", "Name it"),
            ("Double-click a name", "Rename the strip"),
            ("Double-click a dB value", "Type a level"),
            ("Double-click a fader", "Back to 0 dB, gliding up; never turns up Main or Phones"),
            ("Double-click a strip", "Collapse / expand it"),
            ("Ctrl + click collapse", "Collapse this strip and all to its right"),
            ("Shift + drag", "Fine adjustment"),
            // The Matrix's own, moved here from its one-line caption
            // (critique #51).
            ("Ctrl + click a Matrix box", "0 dB, or off"),
            ("Arrows, + / \u{2212} in the Matrix", "Move between boxes and turn one up or down"),
            ("Right-click a scroll mark", "Store the row's position there"),
        ],
    ),
];

/// The trick each row teaches (`tricks`), ticked in the list once found.
fn trick_for(keys: &str) -> Option<&'static str> {
    Some(match keys {
        "Ctrl+Tab" => "views",
        "F1" => "help-key",
        "F2 / Ctrl ," => "settings-key",
        "Ctrl + / Ctrl - / Ctrl 0" => "zoom",
        "Tab / Shift+Tab" => "tab",
        "M / S" => "ms-keys",
        "Space" => "space",
        "\u{2191} / \u{2193}" => "nudge",
        "+ / \u{2212}" => "plus-minus",
        "[ / ]" => "step-mix",
        "Alt+1 \u{2026} Alt+8" => "alt-snapshot",
        "Ctrl+Z / Ctrl+Shift+Z" => "undo",
        "Ctrl / Shift + click" => "select",
        "Double-click a name" => "rename",
        "Double-click a dB value" => "type-level",
        "Double-click a fader" => "reset",
        "Double-click a strip" => "fold",
        "Tab in Audra settings" => "settings-keys",
        "Tab into a panel" => "panel-keys",
        "Tab in Quick setup" => "quick-keys",
        "\u{2190} \u{2192} on Reverb & Echo" => "fx-keys",
        // The 22nd trick had no row to tick (critique #88 A P2-1: "21 of
        // 22" with nothing left to try).
        "Arrows, + / \u{2212} in the Matrix" => "matrix-keys",
        _ => return None,
    })
}

/// The list, its groups under quiet heads (Help's body scrolls, so the
/// card never runs off a short window). Rows of tricks already found carry
/// a tick, and the head counts them (`tricks`).
fn shortcuts(state: &TuxMix, scale: f32) -> Element<'static, Message> {
    let mut list = column![].spacing(theme::SPACE_LG * scale);
    for (group, rows) in SHORTCUTS {
        let mut block = column![text(group)
            .font(theme::SANS_SEMI)
            .color(theme::text_primary())
            .size(theme::TEXT_SM * scale)]
        .spacing(theme::SPACE_SM * scale);
        for (keys, action) in rows.iter() {
            // A trick's row: an empty ring beside its key until found, then
            // the tick in it; other rows keep the slot empty (the reviewer:
            // unfound tricks looked like rows that can never be ticked).
            let d = 14.0 * scale;
            let mark: Element<'static, Message> = match trick_for(keys) {
                Some(id) if crate::tricks::found(state, id) => {
                    container(crate::widgets::strip_icons::check_icon(theme::readable_on_floors(theme::signal()), scale * 0.8))
                        .width(Length::Fixed(d))
                        .height(Length::Fixed(d))
                        .style(move |_t: &iced::Theme| container::Style {
                            border: iced::Border { color: theme::signal(), width: 1.0, radius: (d / 2.0).into() },
                            ..container::Style::default()
                        })
                        .into()
                }
                Some(_) => container(Space::new())
                    .width(Length::Fixed(d))
                    .height(Length::Fixed(d))
                    .style(move |_t: &iced::Theme| container::Style {
                        border: iced::Border { color: theme::control_edge(), width: 1.0, radius: (d / 2.0).into() },
                        ..container::Style::default()
                    })
                    .into(),
                None => Space::new().width(Length::Fixed(d)).into(),
            };
            block = block.push(
                row![
                    mark,
                    text(*keys)
                        .color(theme::text_primary())
                        .size(theme::TEXT_SM * scale)
                        .width(Length::Fixed(140.0 * scale)),
                    text(*action)
                        .color(theme::text_sec())
                        .size(theme::TEXT_SM * scale)
                        .width(Length::Fill),
                ]
                .spacing(theme::SPACE_SM * scale),
            );
        }
        list = list.push(block);
    }
    let (n, of) = crate::tricks::found_count(state);
    column![
        row![
            text("Keys and gestures")
                .font(theme::SERIF)
                .color(theme::text_primary())
                .size(theme::TEXT_MD * scale),
            Space::new().width(Length::Fill),
            row![
                crate::tricks::ladder(state, None, scale),
                // What a ring is, said where the rings are (critique #88 A
                // P3-2: only Settings said so).
                text(format!("{n} of {of} found \u{b7} the ringed ones tick as you use them"))
                    .size(theme::TEXT_SM * scale)
                    .color(theme::text_sec()),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(Alignment::Center),
        ]
        .align_y(Alignment::Center),
        list,
    ]
    .spacing(theme::SPACE_SM * scale)
    .into()
}

/// The whole overlay: a dimming scrim that closes on click and blocks the
/// window behind it, with the card centered on it. The card is `opaque`,
/// so a click inside it (its padding included) is captured before the
/// scrim's `mouse_area` and never closes the dialog under the cursor.
/// Help's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpTab {
    /// "How do I…": the guides.
    #[default]
    Guides,
    Milestones,
    /// Keys and gestures, with the tricks found.
    Keys,
    /// The build, the device, the wiki and the credits.
    About,
}

const HELP_TABS: [(HelpTab, &str, &str); 4] = [
    (HelpTab::Guides, "How do I\u{2026}", "help-tab-guides"),
    (HelpTab::Milestones, "Milestones", "help-tab-milestones"),
    (HelpTab::Keys, "Keys", "help-tab-keys"),
    (HelpTab::About, "About", "help-tab-about"),
];

/// The tab keys: lit for the one open, reachable from the keyboard.
fn tabs(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let keys = HELP_TABS.iter().map(|&(tab, label, id)| {
        let on = state.help_tab == tab;
        crate::keyfocus::reach_press(
            id,
            button(text(label).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                // Unlit, a rim that reads wherever it lands (critique #66 B:
                // 2.43-2.92 on the dialog, carried from About).
                .style(theme::split_safe_key(theme::lamp_button(theme::accent(), theme::Lamp { active: on, ..theme::Lamp::default() }), on))
                .on_press(Message::HelpTab(tab))
                .into(),
            Some(Message::HelpTab(tab)),
            theme::RADIUS_SM * scale,
        )
    });
    // "What's this?" from the pointer (critique #80 A 1: only the ? key
    // turned it on): an action, not a tab, so a quiet capsule apart from
    // them. It closes Help and turns the mode on.
    let explain = crate::keyfocus::reach_press(
        "help-whats-this",
        crate::widgets::strip::hint(
            button(text("What's this?").size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                .padding([theme::SPACE_SM * scale, theme::SPACE_LG * scale])
                .style(theme::page_key(11.0 * scale))
                .on_press(Message::Explain(true)),
            "Point at anything for what it does, right away (or press ?)",
            scale,
        ),
        Some(Message::Explain(true)),
        11.0 * scale,
    );
    row(keys)
        .push(iced::widget::Space::new().width(Length::Fill))
        .push(explain)
        .spacing(theme::SPACE_SM * scale)
        .align_y(Alignment::Center)
        .into()
}

/// The About tab: the build, the device, the wiki and the credits.
fn identity(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    // Which build, and which device it is driving right now, in which
    // state (the same four words the sidebar chip uses, disconnect
    // included). Between them they are most of what a bug report needs
    // to start with, and the sidebar chip that says the same thing
    // scrolls away with the mixer. The mock suffix goes: the word right
    // after it already says it, and "(mock) (simulated)" reads like a
    // bug of its own.
    let model = state
        .device
        .model_name()
        .strip_suffix(" (mock)")
        .unwrap_or_else(|| state.device.model_name());
    let device = format!(
        "Driving {model} ({})",
        if !state.connected {
            "disconnected"
        } else if state.no_device.is_some() {
            "demo"
        } else if state.device.is_mock() {
            "simulated"
        } else if !state.device.protocol_features() {
            "class-compliant"
        } else {
            "proprietary"
        },
    );

    // The drawn wordmark as the title (its "A" is the mark), so the card
    // shows one A, not the mark beside a typeset name.
    let title: Element<'_, Message> = match crate::icon::wordmark_in(theme::text_primary(), theme::accent()) {
        Some((handle, ratio)) => {
            let wordmark_h = 32.0 * scale;
            iced::widget::image(handle)
                .width(wordmark_h * ratio)
                .height(wordmark_h)
                .filter_method(iced::widget::image::FilterMethod::Linear)
                .into()
        }
        None => text("Audra")
            .color(theme::text_primary())
            .size(theme::TEXT_XL * scale)
            .into(),
    };
    let identity = column![
        row![
            title,
            text(format!("{} ({})", version(), build()))
                .color(theme::text_sec())
                .size(theme::TEXT_MD * scale),
        ]
        .spacing(theme::SPACE_LG)
        .align_y(Alignment::Center),
        text("Hardware mixer for the RME Babyface Pro FS on Linux")
            .color(theme::text_sec())
            .size(theme::TEXT_SM * scale),
        text(device)
            .color(theme::text_sec())
            .size(theme::TEXT_SM * scale),
        crate::keyfocus::reach_press(
            "wiki",
            button(
                text("How to use Audra, and what to do when something is wrong: the wiki")
                    .color(theme::accent())
                    .size(theme::TEXT_SM * scale)
            )
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
            .style(theme::plain_button)
            .on_press(Message::OpenWikiUrl)
            .into(),
            Some(Message::OpenWikiUrl),
            theme::RADIUS_SM * scale,
        ),
        text("Audra is free software (GPL-3.0-only).").color(theme::text_sec()).size(theme::TEXT_SM * scale),
        text("Based on TuxMix by Isma\u{ef}l Bahloul (GPL-3.0)")
            .color(theme::text_sec())
            .size(theme::TEXT_SM * scale),
        // Under its credit and named as TuxMix's: the page's one web
        // address read as where an Audra problem goes (critique #88 A P2-2).
        crate::keyfocus::reach_press(
            "repo",
            button(
                text(format!("TuxMix\u{2019}s source code (not Audra\u{2019}s): {REPO_URL}"))
                    .color(theme::accent())
                    .size(theme::TEXT_SM * scale)
            )
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
            .style(theme::plain_button)
            .on_press(Message::OpenRepoUrl)
            .into(),
            Some(Message::OpenRepoUrl),
            theme::RADIUS_SM * scale,
        ),
        // Noise removal's library (its BSD licence asks for the notice).
        text("Noise removal: RNNoise in Rust (nnnoiseless), \u{a9} Joe Neeman, Mozilla, Jean-Marc Valin, Xiph.Org Foundation, Mark Borgerding (BSD-3-Clause)")
            .color(theme::text_sec())
            .size(theme::TEXT_SM * scale),
    ]
    .spacing(theme::SPACE_SM * scale);

    identity.into()
}

pub fn about_modal(state: &TuxMix) -> Element<'_, Message> {
    let scale = state.ui_scale;


    let card = opaque(
        container(
            column![
                header(scale),
                // The head stands; the rest scrolls as one, Audra's own
                // card at its foot (the reviewer: with the keys alone
                // scrolling, the card below them was never on screen).
                // One section at a time under tabs (the owner, 2026-11-07,
                // from mockups: in one column Help was "messy").
                tabs(state, scale),
                scrollable(
                    container(match state.help_tab {
                        HelpTab::Guides => crate::guides::about_list(state, scale),
                        HelpTab::Milestones => crate::celebrate::about_list(state, scale),
                        HelpTab::Keys => shortcuts(state, scale),
                        // Built only when shown: a built link is a keyboard
                        // stop, and a hidden one caught the Tab key.
                        HelpTab::About => identity(state, scale),
                    }),
                )
                .id(HELP_SCROLL_ID)
                // A lane of its own beside the body, not over its keys.
                .direction(iced::widget::scrollable::Direction::Vertical(theme::thin_scrollbar().spacing(theme::SPACE_SM * scale)))
                .style(theme::scrollable_marked)
                .height(Length::Shrink)
            ]
                .spacing(theme::SPACE_LG * scale)
                .width(Length::Fill),
        )
        .style(theme::dialog)
        .padding(theme::SPACE_XL * scale)
        .width(Length::Fill)
        .max_width(CARD_W * scale),
    );

    // The scrim is the invisible half of the modal: dim the window, catch
    // the click that closes the dialog, and keep every event from reaching
    // the window underneath. It wraps the card rather than sitting under
    // it in a `stack`: there, the full-window container on top made iced
    // levitate the cursor for the scrim below, so clicking outside the
    // card never closed anything. The card is `opaque`, so its own clicks
    // are captured before `mouse_area` sees them.
    opaque(
        mouse_area(
            container(card)
                .center_x(Length::Fill)
                // From the top, where its longest tab starts: centred, a
                // shorter tab's card stood 300 px lower and the tab bar
                // moved from under the pointer with every tab picked, a
                // second click landing on the scrim and closing Help (the
                // double-press check, 2026-11-18).
                .align_y(iced::alignment::Vertical::Top)
                .width(Length::Fill)
                .height(Length::Fill)
                // Room round the card, so it never meets the window's edge
                // and its list scrolls instead.
                .padding(theme::SPACE_XL * scale)
                .style(|_theme: &iced::Theme| container::Style {
                    background: Some(Background::Color(Color {
                        a: 0.55,
                        ..Color::BLACK
                    })),
                    ..container::Style::default()
                }),
        )
        .on_press(Message::ToggleAbout),
    )
}

/// How far along Audra is: alpha (it works, it changes often, expect
/// rough edges), then beta, then stable.
pub const STAGE: &str = "alpha";

/// The version as people see it, with the build number: "v0.1.0 alpha ·
/// build 412" ("+" when built with uncommitted changes).
pub fn version() -> String {
    format!("v{} {STAGE} \u{b7} build {}", env!("CARGO_PKG_VERSION"), env!("AUDRA_BUILD_NUMBER"))
}

/// The commit this build was made from ("+" when it had uncommitted
/// changes), stamped by `build.rs`.
pub fn build() -> &'static str {
    env!("AUDRA_BUILD")
}

#[cfg(test)]
mod trick_rows {
    use super::{trick_for, SHORTCUTS};

    #[test]
    fn every_trick_has_a_row_to_tick() {
        // Critique #88 A P2-1: 22 tricks counted, 21 rows with a ring.
        let rows: Vec<&str> = SHORTCUTS.iter().flat_map(|(_, rows)| rows.iter().map(|(k, _)| *k)).filter_map(trick_for).collect();
        for t in crate::tricks::TRICKS.iter() {
            assert!(rows.contains(&t.id), "no row in Help's Keys tab ticks the trick {:?}", t.id);
        }
    }
}
