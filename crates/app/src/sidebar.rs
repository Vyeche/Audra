//! The counter: the right-hand column, TotalMix's Control Strip arranged
//! around the jobs done every day.
//!
//! Top to bottom:
//! 1. The Now Mixing placard: the submix every input and playback fader
//!    is sending to, named on an ivory card. Its name is the submix
//!    selector, so changing the mix is changing the card. Under it, who
//!    the mix is for and what feeds it.
//! 2. The Main and Phones levels, read large in the Mono face, each with
//!    a slider on the faders' own dB taper.
//! 3. The Control Room keys (`control_room::keys`).
//! 4. Undo/redo and the global M/S/F masters.
//! 5. One tabbed area, a page at a time: Snapshots, Groups, Layouts and
//!    View (TotalMix's View Options).
//!
//! The device card that used to open this column lives in the top bar
//! (`device_button`). Coral marks only what can be pressed or is live:
//! a lit key or M/S/F master, the open tab's underline.

use iced::widget::{button, column, container, mouse_area, pick_list, row, text};
use iced::{Color, Element, Length};

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix, OUT_LABELS};
use crate::control_room;
use crate::theme;
use crate::widgets::fader;

/// Default column width (at `scale == 1.0`), used until the user drags
/// the resize strip: room for the placard's serif name, the large level
/// readouts and the Control Room keys two to a row.
pub const DEFAULT_WIDTH: f32 = 300.0;
/// Bounds on `TuxMix::sidebar_width`. At 280 the Control Room keys' labels
/// fit two to a row at the full key size (`control_room::keys`). The ceiling is where the
/// mixer's own strips stop having room to spare on a 1920px window.
pub const MIN_WIDTH: f32 = 280.0;
pub const MAX_WIDTH: f32 = 420.0;

/// A page's name in the prefs, and back ("none": every page closed).
pub fn page_name(tab: Option<Tab>) -> &'static str {
    match tab {
        None => "none",
        Some(Tab::Snapshots) => "snapshots",
        Some(Tab::Fx) => "fx",
        Some(Tab::Groups) => "groups",
        Some(Tab::Layouts) => "layouts",
        Some(Tab::View) => "view",
    }
}

pub fn page_of(name: &str) -> Option<Option<Tab>> {
    Some(match name {
        "none" => None,
        "snapshots" => Some(Tab::Snapshots),
        "fx" => Some(Tab::Fx),
        "groups" => Some(Tab::Groups),
        "layouts" => Some(Tab::Layouts),
        "view" => Some(Tab::View),
        _ => return None,
    })
}

/// Which page the counter's tabbed area shows. One at a time: TuxMix's
/// column stacked all four open, which pushed the column into a scroll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Snapshots,
    /// Reverb and Echo (`fx::page`).
    Fx,
    Groups,
    Layouts,
    View,
}
/// The counter's scroll (`Message::ScrollCounterToEnd`).
pub const COUNTER_SCROLL_ID: &str = "counter-scroll";
/// The counter's head (placard, levels, Control Room, tools row), which
/// scrolls in its own room only on a window too short for it.
pub const COUNTER_HEAD_SCROLL_ID: &str = "counter-head-scroll";

/// Width of the collapsed rail: the panel toggle plus its padding, so
/// collapsing reclaims most of the width for the mixer while the button
/// that brings the column back stays exactly where it was.
// 44 (was 34): folded, its keys and meters were crammed (the owner,
// 2026-11-14).
pub const COLLAPSED_WIDTH: f32 = 44.0;
/// The panel toggle's hit box, at scale 1 — the same in both states, so
/// the button never moves when it is pressed.
const TOGGLE_W: f32 = 26.0;
const TOGGLE_H: f32 = 22.0;

/// Width of the Options panel's caption column. Sized for the longest
/// caption the reference uses there ("solo/pfl mode") at `TEXT_SM`.
const OPTIONS_LABEL_W: f32 = 76.0;


/// Which of a group's three independent links (`GroupLinkToggle`
/// flips) a click on one of its `M1`/`S1`/`1`-style cells targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLink {
    Mute,
    Solo,
    Fader,
}

/// One of the 4 mute/solo/fader groups — a pure GUI-side convenience
/// over the existing per-channel `set_mute`/`set_solo`/volume calls
/// (not a device concept, so this lives here rather than in
/// `tuxmix-core`). All three link types share one membership list,
/// matching the reference's layout (one `M1`/`S1`/`1` row per group,
/// not three independent membership lists) — assigned via the
/// *existing* multi-select (`TuxMix::selected`) rather than a bespoke
/// picker UI: enter edit mode, select strips the normal Ctrl/Shift-click
/// way, click the group cell to assign.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub members: Vec<ChannelId>,
    pub mute_linked: bool,
    pub solo_linked: bool,
    pub fader_linked: bool,
}

/// Width of the drag strip that resizes this column, on its inner edge.
/// Sized to the column's own left padding (`SPACE_MD`), and the column's
/// left padding is in turn dropped in its favour (`sidebar`'s own
/// `scrollable`): the strip occupies exactly the space nothing was drawn
/// in, so resizing costs the layout no width and every row still starts
/// `SPACE_MD` from the column's left edge.
const RESIZE_W: f32 = theme::SPACE_MD;

/// The fold control, part of the panel itself: the "toggle sidebar"
/// icon (`widgets::panel_icon`) at the right end of the panel's title row
/// when open, and at the top of the rail when collapsed — the same spot
/// and size either way, the way editors and design tools do it. It
/// replaced a capsule tab that hung over the mixer/sidebar boundary as an
/// overlay: findable, but it belonged to neither side and read as a
/// scrollbar fragment.
fn panel_toggle(open: bool, scale: f32) -> Element<'static, Message> {
    let toggle = button(
        iced::widget::canvas(crate::widgets::panel_icon::PanelIcon {
            color: theme::text_sec(),
            open,
            scale,
        })
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .padding(0)
    .width(Length::Fixed(TOGGLE_W * scale))
    .height(Length::Fixed(TOGGLE_H * scale))
    .style(theme::ghost_button)
    .on_press(Message::ToggleSidebar);
    hint_at(
        toggle,
        if open { "Hide panel" } else { "Show panel" },
        scale,
        iced::widget::tooltip::Position::Left,
    )
}

/// The 1px line on the panel's inner edge while collapsed. Open, the
/// resize strip draws the same line at rest (see `widgets::splitter`).
fn edge_line(scale: f32) -> Element<'static, Message> {
    container(
        iced::widget::Space::new()
            .width(Length::Fixed(scale.max(1.0)))
            .height(Length::Fill),
    )
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(theme::border())),
        ..container::Style::default()
    })
    .into()
}

/// What the folded column keeps of the monitoring (the critique,
/// 2026-11-03: below about 1075px the column folds, and the Phones level
/// and its meter went with it): the Main and the first Phones pair's
/// meters standing upright, each with its level as a tooltip, and DIM.
fn rail_monitors(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut pairs = vec![control_room::monitor_pair(state)];
    pairs.extend(state.control_room.assign.phones_pairs().into_iter().take(1));
    let mut meters = row![].spacing(theme::SPACE_SM * scale);
    for pair in pairs {
        let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
        let Some(out) = state.device.outputs().get(ch) else { continue };
        if !crate::app::output_meter_available(state, ch) {
            continue;
        }
        let frame_of = |i: usize| state.output_meters.get(i).map(|m| m.frame());
        let Some(left) = frame_of(ch) else { continue };
        let right = (!state.device.outputs_one_per_pair()).then(|| frame_of(ch + 1)).flatten();
        let level = control_room::displayed_level(state, ch, out.volume);
        let tip = format!("{}: {} dB", mix_name(state, pair), db_number(level));
        meters = meters.push(crate::widgets::strip::hint_text(
            crate::widgets::fader::rail_meter(left, right, scale, 9.0 * scale, 150.0 * scale),
            tip,
            scale,
        ));
    }
    // The Main level itself, in whole dB under the meters: folded (a
    // narrow window, a big zoom), the level you listen at stays in sight,
    // not only in a tooltip (the audit, 2026-11-05).
    let main_pair = control_room::monitor_pair(state);
    let main_ch = if state.device.outputs_one_per_pair() { main_pair } else { main_pair * 2 };
    let main_level = state
        .device
        .outputs()
        .get(main_ch)
        .map(|o| control_room::displayed_level(state, main_ch, o.volume));
    // In the strips' value well, the number over "dB", in the serif as the
    // column's levels are: the Mono's slashed zero read as the phase key
    // (the reviewer, 2026-11-05), and so did the sans's (critique #28).
    let main_db: Option<Element<'_, Message>> = main_level.map(|v| {
        let db = 20.0 * v.max(1e-6).log10();
        let words = if v <= 0.0 || db < -99.0 { "\u{2212}\u{221e}".to_string() } else if db.round() == 0.0 { "0".to_string() } else { format!("{:+.0}", db).replace('-', "\u{2212}") };
        crate::widgets::strip::hint_text(
            container(
                column![
                    text(words).font(theme::SERIF).size(theme::TEXT_MD * scale).color(theme::text_primary()),
                    text("dB").size(theme::TEXT_SM * scale).color(theme::text_sec()),
                ]
                .align_x(iced::Alignment::Center),
            )
            .padding([theme::SPACE_TIGHT * scale, 0.0])
            .width(Length::Fixed((COLLAPSED_WIDTH - 4.0) * scale))
            .center_x(Length::Fixed((COLLAPSED_WIDTH - 4.0) * scale))
            .style(theme::inset_box(6.0 * scale)),
            format!("{}: {} dB", mix_name(state, main_pair), db_number(v)),
            scale,
        )
    });
    let dimmed = state.control_room.is_dimmed();
    let dim = hint(
        button(text("Dim").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
            .padding(0)
            .width(Length::Fixed(30.0 * scale))
            .height(Length::Fixed(22.0 * scale))
            // Steady rim (critique #58 B: its sides 1.8 on the narrow rail).
            .style(theme::split_safe_key(theme::toggle_button(dimmed, theme::accent()), dimmed))
            .on_press(Message::DimToggle),
        if dimmed { "Dim is on: click to bring Main back" } else { "Dim Main" },
        scale,
    );
    let mut col = column![meters].spacing(theme::SPACE_LG * scale).align_x(iced::Alignment::Center);
    if let Some(level) = main_db {
        col = col.push(level);
    }
    col = col.push(dim);
    // A mic open in the speakers, said on the rail too (critique #28's P1:
    // folded, the column took the warning with it, and nothing in the
    // Mixer said a mic could howl). A lit key in the warning's hue; a press
    // opens the column at the warning's answers.
    if let Some((cid, _)) = open_mic_shown(state) {
        let name = crate::app::strip_display_name(state, cid);
        // A warning's tag, outlined in the warning's hue, never a lit key:
        // as a key it read "the mic is on, click to turn it off" (critique
        // #55).
        let ink = theme::readable_on_floors(theme::attention());
        col = col.push(crate::widgets::strip::hint_text(
            // "Open mic", not "Mic!": at a glance that read as an error,
            // not "a mic can howl in the speakers" (critique #57).
            button(text("Open\nmic").font(theme::SANS_SEMI).size(10.0 * scale).line_height(1.1).color(ink).center())
                .padding(0)
                .width(Length::Fixed(34.0 * scale))
                .height(Length::Fixed(30.0 * scale))
                .style(move |_t: &iced::Theme, status: button::Status| {
                    let over = matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: over.then(|| iced::Background::Color(Color { a: 0.14, ..theme::attention() })),
                        text_color: ink,
                        border: iced::Border { color: theme::attention(), width: 1.5 * scale, radius: (theme::RADIUS_SM * scale).into() },
                        ..button::Style::default()
                    }
                })
                .on_press(Message::OpenMicFromRail),
            format!("Open mic in your speakers: {name}. Click to see what to do"),
            scale,
        ));
    }
    // An open mic's reverb in the speakers, said on the rail too (critique
    // #112 Fable A P2-2: folded, the column took the hazard row with it).
    // A press opens the column, where the row and its press are.
    if crate::fx::mic_reverb_hazard(state).is_some() {
        let ink = theme::readable_on_floors(theme::attention());
        let what = crate::fx::on_words(state);
        col = col.push(crate::widgets::strip::hint_text(
            button(text("Mic\nFX").font(theme::SANS_SEMI).size(10.0 * scale).line_height(1.1).color(ink).center())
                .padding(0)
                .width(Length::Fixed(34.0 * scale))
                .height(Length::Fixed(30.0 * scale))
                .style(move |_t: &iced::Theme, status: button::Status| {
                    let over = matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: over.then(|| iced::Background::Color(Color { a: 0.14, ..theme::attention() })),
                        text_color: ink,
                        border: iced::Border { color: theme::attention(), width: 1.5 * scale, radius: (theme::RADIUS_SM * scale).into() },
                        ..button::Style::default()
                    }
                })
                .on_press(Message::ToggleSidebar),
            format!("Your open mic's {what} plays in your speakers: it can howl. Click to open the column and move it"),
            scale,
        ));
    }
    col.into()
}

/// Top-level entry point — one scrollable column, filling the window's
/// full height alongside the main content (see `app.rs::view`). Its width
/// is the user's (`TuxMix::sidebar_width`, dragged via `resize_strip`, so
/// a narrow one trades the Options labels' one-line fit for mixer space
/// and a wide one gives the Groups/Layout lists room). Collapsible:
/// `state.sidebar_open == false` renders just `COLLAPSED_WIDTH` of rail
/// with an expand button, reclaiming the rest of the width for the mixer —
/// the whole point being asked for ("rabattable"), not just a visual
/// nicety.
pub fn sidebar(state: &TuxMix) -> Element<'_, Message> {
    // Capped (`app::sidebar_scale`): a column of small controls gains
    // nothing from the strips' full zoom.
    let scale = crate::app::sidebar_scale(state);

    // Collapsed, the rail is exactly the tab plus its own padding — the
    // one control that brings the column back *is* the whole rail, and
    // the tab is centered so it doesn't move height between states.
    if !state.sidebar_open {
        // The rail keeps the device's status under the toggle, beside the
        // mixer, so a card that drops out shows on this side too: as the
        // light on the unit's own icon, which opens the device settings.
        let (status_color, status_words) = device_state(state);
        let rail = column![
            panel_toggle(false, scale),
            crate::widgets::strip::hint_text(
                button(crate::widgets::strip_icons::device_icon(theme::text_sec(), status_color, scale))
                    .padding(0)
                    .width(Length::Fixed(TOGGLE_W * scale))
                    .height(Length::Fixed(TOGGLE_H * scale))
                    .style(theme::ghost_button)
                    .on_press(Message::ToggleDevicePanel),
                format!("Device settings ({status_words})"),
                scale,
            ),
            rail_monitors(state, scale),
        ]
        .spacing(theme::SPACE_LG * scale)
        // The toggle centered on the top bar's line, where it sits when
        // the panel is open.
        .padding(iced::Padding {
            top: (theme::TOP_BAR_H - TOGGLE_H) / 2.0 * scale,
            ..iced::Padding::ZERO
        })
        .align_x(iced::Alignment::Center)
        .width(Length::Fill);
        // The whole rail is the way back: one click anywhere on it opens
        // the panel, not just on the 26px button at its top. The button
        // captures its own press, so a click on it is not counted twice.
        return mouse_area(
            container(row![edge_line(scale), rail])
                .width(Length::Fixed(COLLAPSED_WIDTH * scale))
                .height(Length::Fill)
                .style(theme::counter),
        )
        // Double-click, like the open panel: one gesture both ways. The
        // button at the top still opens it with a single click.
        .on_double_click(Message::ToggleSidebar)
        .into();
    }

    // On a short window (the owner's 1280x700) the column draws tighter,
    // so the open page shows more before the column has to scroll (the
    // critique: the pages were cut at 700px).
    // Drawn tight on a tall window too where the foot slot's card needs
    // the room (`head_tight_needed`), for the head's own parts only.
    let tight = head_tight_needed(state);
    crate::app::set_head_tight(tight);
    let short = crate::app::head_tight(state);
    // The head stands still: the placard, the levels, the Control Room and
    // the tools row, which must stay in reach; only the pages scroll under
    // it (critique #7's P1: in one scroll with them, a notice or a learning
    // card in the foot slot shrank the view and pushed the Control Room's
    // keys out of sight on the owner's 1280x700).
    // Snugger still while a learning card has a short window's slot, so
    // the page keys stay in view over it (the owner's pick after critique
    // #27).
    let gap = if snug(state) {
        theme::SPACE_TIGHT
    } else if short {
        theme::SPACE_MD
    } else {
        theme::SPACE_XL
    } * scale;
    // The pages float (at every height, `pages_float`): the snapshots stand
    // in the head as TotalMix's row of keys with Store, and the pages' keys
    // under them open each page as a card beside the column (critique #8's
    // P1; the owner's "chips everywhere", 2026-11-07).
    let float = pages_float(state);
    // The sections' padding: tighter on a short window, so the snapshot
    // and page keys stay in view under a card or a notice.
    let pad = if snug(state) {
        theme::SPACE_SM
    } else if short {
        theme::SPACE_MD
    } else {
        theme::SPACE_LG
    };
    // The Control Room keys stand here unless they're beside Main and the
    // Phones in the Mixer (`control_room::keys_in_mixer`).
    let mut head_col = column![placard(state, scale), section_padded(None, levels(state, scale), scale, pad)];
    if !control_room::keys_in_mixer(state) {
        head_col = head_col.push(section_padded(Some("Control Room"), control_room::keys(state, scale), scale, pad));
    }
    head_col = head_col.push(tools_row(state, scale));
    // A notice that leaves the head too little room puts the page keys away
    // whole until it goes, rather than leave a sliver of them under it
    // (critique #41's whole-row rule; at 1280x700 with tips off a notice cut
    // the row to its top 4 px). Judged by the head as last measured with
    // the keys, so leaving them out never flips it back.
    let keys_away = float && !tight && page_keys_crowded(state, scale);
    if float {
        head_col = head_col.push(snapshot_section(state, scale));
        if !keys_away {
            head_col = head_col.push(page_keys(state, scale));
        }
    }
    let head_col = head_col
    .spacing(gap)
    // The resize strip (`RESIZE_W`) sits in the left padding.
    .padding(iced::Padding {
        top: if snug(state) {
            theme::SPACE_TIGHT
        } else if short {
            theme::SPACE_MD
        } else {
            theme::SPACE_XXL
        } * scale,
        right: theme::SPACE_XXL * scale,
        bottom: 0.0,
        left: theme::SPACE_XL * scale,
    })
    .width(Length::Fill);
    crate::app::set_head_tight(false);
    let tail_col = column![pages(state, scale)]
        .padding(iced::Padding {
            top: gap,
            right: theme::SPACE_XXL * scale,
            // Room at the foot, so the last key never sits sliced under the
            // zoom bar when the column scrolls (the reviewer, at 1280x700).
            bottom: 2.0 * theme::SPACE_XL * scale,
            left: theme::SPACE_XL * scale,
        })
        .width(Length::Fill);
    // The head gets the room first; on a window too short even for it, it
    // scrolls within its own room, so the slot and the zoom bar stay seen.
    let slot_h = foot_slot_h(state);
    // While the slot holds something the pages may close up entirely (they
    // come back as it goes); otherwise they keep a bar's room. A guide
    // teaching on the open page gets it first, and the head scrolls.
    let pages_min = if crate::guides::teaching_page(state).is_some() {
        220.0
    } else if slot_h > 0.0 || float {
        0.0
    } else {
        48.0
    };
    // The column runs the window's full height (the top bar stops short of
    // it); under the head: the zoom bar (30) and the gap over it (4), with
    // a little to spare.
    let head_max = state.window_size.map_or(f32::INFINITY, |(_, h)| {
        (h - (36.0 + pages_min + slot_h) * scale).max(140.0 * scale)
    });
    let head = container(
        // Measured only roomy, so drawing it tight never changes where the
        // card goes (it would flip back and forth).
        iced::widget::scrollable(crate::widgets::overhang::measure(head_col, if tight { &HEAD_H_TIGHT } else if keys_away { &HEAD_H_KEYS_AWAY } else { &HEAD_H }))
            .id(COUNTER_HEAD_SCROLL_ID)
            .direction(iced::widget::scrollable::Direction::Vertical(theme::thin_scrollbar()))
            .style(theme::scrollable_marked)
            .width(Length::Fill),
    )
    .max_height(head_max);

    // Thin scrollbar, like `app.rs::page`, for a window too short for the
    // pages.
    // Where the pages float there's nothing under the head but a page that
    // fits there (`page_inline`): plain room otherwise, not an empty scroll
    // whose padding showed a stray bar.
    let tail_col = match (float, page_inline(state)) {
        (true, Some(tab)) => column![tab_body(state, tab, scale)].padding(iced::Padding {
            top: gap,
            right: theme::SPACE_XXL * scale,
            bottom: 2.0 * theme::SPACE_XL * scale,
            left: theme::SPACE_XL * scale,
        }),
        _ => tail_col,
    };
    // On a tall window the learning card sits right under the page keys
    // (critique #15: at the window's foot it was stranded under a column
    // of empty room); a guide's card and a notice keep the foot slot.
    // Where the room under the head is too low for the full card (a
    // milestone, a trick), its short form (critique #26's P1: squeezed to a
    // sliver there at 1280x800, and under the listed snapshots in Quick
    // setup at 1600x1000).
    let compact = card_room(state).is_some_and(|room| room < (FULL_CARD_H + theme::SPACE_XL) * scale);
    let inline_card = card_inline(state).then(|| crate::app::foot_card_sized(state, compact)).flatten().map(|card| {
        container(card).padding(iced::Padding { top: gap, ..iced::Padding::ZERO }).width(Length::Fill)
    });
    let content: Element<'_, Message> = if float && page_inline(state).is_none() {
        match inline_card {
            Some(card) => column![
                container(card).padding(iced::Padding { top: 0.0, right: theme::SPACE_XXL * scale, bottom: 0.0, left: theme::SPACE_XL * scale }),
                iced::widget::Space::new().height(Length::Fill),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            None => iced::widget::Space::new().width(Length::Fill).height(Length::Fill).into(),
        }
    } else {
        let tail_col = match inline_card {
            Some(card) => tail_col.push(card),
            None => tail_col,
        };
        iced::widget::scrollable(tail_col)
        .id(COUNTER_SCROLL_ID)
        .on_scroll(|v| {
            let at_end = v.content_bounds().height <= v.bounds().height + 1.0 || v.relative_offset().y >= 0.99;
            Message::CounterScrolled(at_end)
        })
        .direction(iced::widget::scrollable::Direction::Vertical(
            theme::thin_scrollbar(),
        ))
        .style(theme::scrollable_marked)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    };

    // Double-click on any empty part of the column folds it. Controls
    // capture their own presses, so only the gaps, captions and the
    // title row reach this.
    mouse_area(
        container(
            row![
                resize_strip(scale),
                // A gap above the zoom bar, so a page that runs on under it
                // is cut clear of it (the reviewer, at 1280x700).
                {
                    let mut foot = column![head, content, iced::widget::Space::new().height(theme::SPACE_SM * scale)].width(Length::Fill);
                    // The notice's own slot, over the zoom bar: the column's
                    // pages make room for it, so it never covers a control.
                    if let Some((message, at)) = &state.notice {
                        NOTICE_MEASURED.with(|c| c.set(Some(*at)));
                        // Measured, so the head gives up what it takes
                        // (`foot_slot_h`; critique #28: estimated, a
                        // wrapped card cut the page keys).
                        foot = foot.push(crate::widgets::overhang::measure(
                            container(crate::app::notice_card(message, crate::app::notice_key(state), scale))
                                // In line with the head's edges (critique #27:
                                // 8px wider at the left, 12px at the right).
                                .padding(iced::Padding {
                                    top: theme::SPACE_SM * scale,
                                    right: theme::SPACE_XXL * scale,
                                    bottom: theme::SPACE_SM * scale,
                                    left: theme::SPACE_XL * scale,
                                })
                                .width(Length::Fill),
                            &NOTICE_SLOT_H,
                        ));
                    } else if let Some(card) = crate::app::foot_card_sized(state, true).filter(|_| !card_inline(state)) {
                        // The learning cards share the slot: a guide's, a
                        // milestone's, a trick's, never on the faders. Only
                        // a short window or a full head sends them here, so
                        // in their short forms.
                        foot = foot.push(crate::widgets::overhang::measure(
                            container(card)
                                // In line with the head's edges (critique #27:
                                // 8px wider at the left, 12px at the right).
                                // Snug, no air above it: those 4px left the page
                                // keys cut at their foot at 1280x700 (critiques
                                // #40 to #44).
                                .padding(iced::Padding {
                                    top: if snug(state) { 0.0 } else { theme::SPACE_SM * scale },
                                    right: theme::SPACE_XXL * scale,
                                    bottom: theme::SPACE_SM * scale,
                                    left: theme::SPACE_XL * scale,
                                })
                                .width(Length::Fill),
                            &SLOT_H,
                        ));
                    } else {
                        SLOT_H.with(|h| h.set(0.0));
                    }
                    // The card's height stays known under a notice (the
                    // head draws from it); a notice's own is new each time.
                    if state.notice.is_none() {
                        NOTICE_SLOT_H.with(|h| h.set(0.0));
                    }
                    foot.push(zoom_bar(state, scale))
                }
            ]
            .height(Length::Fill),
        )
        .width(Length::Fixed(state.sidebar_width * scale))
        .height(Length::Fill)
        .style(theme::counter),
    )
    .on_double_click(Message::ToggleSidebar)
    .into()
}

/// The room the counter's slot takes, at scale 1, padding included: a
/// little over what it draws, so the head gives up its last pixels rather
/// than push the zoom bar off the foot. On a short window a trick takes one
/// line, a milestone its two rows, and a guide under way its compact card.
/// Whether the learning card sits under the page keys instead of in the
/// foot slot: on a tall window, no notice showing, no guide under way (a
/// guide's card stays at the foot, always in sight while it teaches).
/// How tall the full milestone and trick cards stand, at scale 1.
const FULL_CARD_H: f32 = 150.0;
/// How tall the tallest short form stands (the milestone's two rows), at
/// scale 1.
const COMPACT_CARD_H: f32 = 80.0;

thread_local! {
    /// The column's head as it last laid out, unsqueezed: 0 before the
    /// first layout.
    static HEAD_H: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// The foot slot's learning card as it last laid out, in window
    /// pixels: 0 while there's none (`card_slot_h`). Kept while a notice
    /// covers the card.
    static SLOT_H: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// The notice in the foot slot as it last laid out, in window pixels:
    /// 0 while none shows (`foot_slot_h`).
    static NOTICE_SLOT_H: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// The notice `NOTICE_SLOT_H` measured (its time): another notice uses
    /// its own estimate until it's measured (OpenCode's job 55: a taller
    /// one took the last one's room for a frame).
    static NOTICE_MEASURED: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
    /// The card's height under the notice showing, as it was when the
    /// notice came (its time, the height): a card starting or ending
    /// under it doesn't re-lay the head until the notice goes (OpenCode's
    /// job 55).
    static CARD_UNDER_NOTICE: std::cell::Cell<Option<(std::time::Instant, f32)>> = const { std::cell::Cell::new(None) };
    /// The head with its page keys put away under a notice: kept apart, so
    /// `HEAD_H` stays the full head's.
    static HEAD_H_KEYS_AWAY: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
    /// The head drawn tight (`head_tight_needed`): kept apart, unread.
    static HEAD_H_TIGHT: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// Whether a short window's foot slot holds a learning card, shown or
/// under a notice: the head draws snugger and Quick's listening line
/// stands aside, so the page keys stay in view over the card. Never from
/// the notice itself: it goes in seconds, and the head would jump twice;
/// and a notice landing over a card keeps the head as it was (critique
/// #40: un-snugged, the Dim key moved 40 px at 125%).
pub(crate) fn snug(state: &TuxMix) -> bool {
    crate::app::short_window(state) && card_slot_h(state) > 0.0
}

/// Whether a tall window's head draws tight: its foot slot holds a
/// learning card (one the head left no room for, or a guide's), shown or
/// under a notice, and the roomy head and the card together are taller
/// than the window. Never from the notice (`snug`'s reasons).
fn head_tight_needed(state: &TuxMix) -> bool {
    if crate::app::short_window(state) {
        return false;
    }
    let slot = card_slot_h(state) * crate::app::sidebar_scale(state);
    slot > 0.0 && card_room(state).is_some_and(|room| room < slot)
}

/// Whether a notice leaves the head too little room for its page keys
/// (judged by the head as last measured with them): they step away whole
/// (`view`), and a notice kept for its Undo key goes at its time instead of
/// holding them away (`app`; critique #45).
pub(crate) fn page_keys_crowded(state: &TuxMix, scale: f32) -> bool {
    state.notice.is_some() && {
        let full = HEAD_H.with(std::cell::Cell::get);
        let room = state.window_size.map_or(f32::INFINITY, |(_, h)| (h - (36.0 + foot_slot_h(state)) * scale).max(140.0 * scale));
        full > 0.0 && full > room + 0.5
    }
}

/// The room under the column's head for a card, in window pixels, from the
/// head's height at the last layout (`None` before it): the window's less
/// the head and the zoom bar. The head's height doesn't change with the
/// card, so where the card goes never flickers.
fn card_room(state: &TuxMix) -> Option<f32> {
    let head = HEAD_H.with(std::cell::Cell::get);
    let (_, h) = state.window_size?;
    (head > 0.0).then(|| h - head - 36.0 * crate::app::sidebar_scale(state))
}

/// Whether the learning card stands under the page keys rather than in the
/// foot slot: on a tall window with room there for at least its short form
/// (a full head sends it to the slot, where the head makes room).
fn card_inline(state: &TuxMix) -> bool {
    state.notice.is_none() && card_inline_beneath(state)
}

/// Where the learning card goes once no notice covers it: under the page
/// keys (`card_inline`) or in the foot slot.
fn card_inline_beneath(state: &TuxMix) -> bool {
    let scale = crate::app::sidebar_scale(state);
    !crate::app::short_window(state)
        && state.guide.is_none()
        && card_room(state).is_none_or(|room| room >= (COMPACT_CARD_H + theme::SPACE_XL) * scale)
}

/// What the foot slot takes now, at scale 1: the notice's height while
/// one shows, else the card's (`card_slot_h`).
fn foot_slot_h(state: &TuxMix) -> f32 {
    if let Some((message, at)) = &state.notice {
        let estimate = notice_slot_estimate(state, message);
        let measured = NOTICE_SLOT_H.with(std::cell::Cell::get);
        let this_one = NOTICE_MEASURED.with(std::cell::Cell::get) == Some(*at);
        return if measured > 0.0 && this_one { measured / crate::app::sidebar_scale(state) + 2.0 } else { estimate };
    }
    card_slot_h(state)
}

/// The learning card's height in the foot slot, at scale 1, shown or
/// under a notice: 0 where there's none, or it stands under the page keys.
fn card_slot_h(state: &TuxMix) -> f32 {
    // While a notice shows, the card's height as it was when it came.
    let Some((_, at)) = &state.notice else {
        CARD_UNDER_NOTICE.with(|c| c.set(None));
        return card_slot_h_now(state);
    };
    if let Some((_, h)) = CARD_UNDER_NOTICE.with(std::cell::Cell::get).filter(|(held_at, _)| held_at == at) {
        return h;
    }
    let h = card_slot_h_now(state);
    CARD_UNDER_NOTICE.with(|c| c.set(Some((*at, h))));
    h
}

/// `card_slot_h` as the card stands now.
fn card_slot_h_now(state: &TuxMix) -> f32 {
    let estimate = card_slot_estimate(state);
    // What the slot measured at the last layout, where it holds something
    // (critique #28: "Next for you" and the clipping card wrap to two
    // lines at the column's width, so the estimate's one line cut the page
    // keys); the estimate before the first layout of a card.
    let measured = SLOT_H.with(std::cell::Cell::get);
    if estimate > 0.0 && measured > 0.0 {
        measured / crate::app::sidebar_scale(state) + 2.0
    } else {
        estimate
    }
}

/// A notice's height in the foot slot before it's measured, at scale 1.
fn notice_slot_estimate(state: &TuxMix, message: &str) -> f32 {
    // The words' width: the column's, less the slot's and the card's
    // padding, the × and its gap (and the Undo key's); about 6.5px a
    // character, and a tenth more for the words wrapping whole.
    let undo_w = crate::app::notice_key_w(state);
    // Open folder stands under the words: a key's row more, no width less.
    let (undo_w, key_row) = if undo_w > 60.0 { (0.0, 32.0) } else { (undo_w, 0.0) };
    let per_line = ((state.sidebar_width - 96.0 - undo_w) / 6.5).max(20.0);
    let lines = (message.chars().count() as f32 * 1.1 / per_line).ceil().max(1.0);
    28.0 + 17.0 * lines + key_row
}

fn card_slot_estimate(state: &TuxMix) -> f32 {
    if crate::app::card_beneath(state).is_none() || card_inline_beneath(state) {
        return 0.0;
    }
    if state.guide.is_some() {
        crate::guides::card_h(state)
    } else if matches!(crate::assist::current(state), Some(crate::assist::Assist::Clipping(_))) {
        // A clipping input's one line and its Fix it, ahead of the rest
        // (`app::foot_card`).
        48.0
    } else if state.celebration.is_none() && crate::keyfocus::focus_words(state).is_some() {
        // What the keyboard is on: a line, two where it wraps.
        56.0
    } else if state.celebration.is_some() {
        // In their short forms here (`card_inline`): on a short window one
        // line (the owner's pick after critique #27); else the milestone's
        // two rows, the burst and its words, the ladder.
        if crate::app::short_window(state) {
            // The 26px lens, the card's padding and the slot's.
            40.0
        } else {
            COMPACT_CARD_H
        }
    } else if state.trick_card.is_none() {
        // "Next for you": one line and its Start key.
        48.0
    } else {
        // The trick's one line.
        46.0
    }
}

/// A framed group of the column (`theme::section`), its title centred
/// over it when it has one (the owner's reference: each group of the
/// right-hand column is a framed panel with its name at the top).
pub(crate) fn section<'a>(title: Option<&'static str>, body: Element<'a, Message>, scale: f32) -> Element<'a, Message> {
    section_padded(title, body, scale, theme::SPACE_LG)
}

/// `section` with its padding at `pad` (scale 1).
fn section_padded<'a>(title: Option<&'static str>, body: Element<'a, Message>, scale: f32, pad: f32) -> Element<'a, Message> {
    let mut col = column![].spacing(theme::SPACE_MD * scale).width(Length::Fill);
    if let Some(title) = title {
        col = col.push(
            text(title)
                .font(theme::SANS_SEMI)
                .size(theme::TEXT_MD * scale)
                .color(theme::text_sec())
                .width(Length::Fill)
                .center(),
        );
    }
    container(col.push(body))
        .padding(pad * scale)
        .width(Length::Fill)
        .style(theme::section)
        .into()
}

/// One output pair as the placard names it (`mix_name`). A `Display`
/// type, because the placard's name is a `pick_list` over these.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MixChoice {
    pair: usize,
    name: String,
}

impl std::fmt::Display for MixChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// An output pair's name on the placard: its Control Room role with the
/// pair's numbers ("Main 1/2", "Phones 3/4"), the label in brackets where
/// the role is numbered itself ("Phones 2 (AS1/2)"), else the pair's own
/// label ("AS1/2").
/// A mix as a sentence names it: with the plain words on, the listener's
/// own outputs by what they are ("your speakers", "your headphones"), any
/// other by its code; with them off, `mix_name` (OpenCode's job 77: the
/// Reverb & Echo page, the FX sends' head, the open-mic card and a few
/// notices said "Main 1/2" beside a placard's "your speakers").
pub(crate) fn mix_said(state: &TuxMix, pair: usize) -> String {
    let a = &state.control_room.assign;
    if state.plain_names && (pair == a.main || a.is_phones(pair)) {
        mix_role_short(state, pair).to_string()
    } else {
        mix_name(state, pair)
    }
}

/// `mix_said` owning something: "your speakers' meter", "Main 1/2's meter".
pub(crate) fn mix_said_s(state: &TuxMix, pair: usize) -> String {
    let said = mix_said(state, pair);
    if said.ends_with('s') { format!("{said}\u{2019}") } else { format!("{said}'s") }
}

pub(crate) fn mix_name(state: &TuxMix, pair: usize) -> String {
    pair_name(&state.control_room.assign, pair)
}

fn pair_name(assign: &control_room::Assign, pair: usize) -> String {
    let label = OUT_LABELS.get(pair).copied().unwrap_or("?");
    match assign.role_name(pair) {
        Some(role) if role.ends_with(|c: char| c.is_ascii_digit()) => format!("{role} ({label})"),
        Some(role) => format!(
            "{role} {}",
            label.trim_start_matches(|c: char| c.is_ascii_alphabetic())
        ),
        None => label.to_string(),
    }
}

/// Who a mix is for, on the placard's second line: by its Control Room
/// role, else by the unit's own outputs (AN1/2 the XLR outs, PH3/4 the
/// phones jacks, the rest optical).
/// Who mix `pair` is for, in the placard's words ("your headphones").
pub(crate) fn mix_role_of(state: &TuxMix, pair: usize) -> &'static str {
    let assign = &state.control_room.assign;
    // A spare output looped back is the mix your apps record or stream
    // (critique #104 A P3-1: "optical 1/2" after Stream).
    if pair != assign.main && !assign.is_phones(pair) && crate::app::input_looped(state, pair * 2) {
        return "your apps (LOOP)";
    }
    mix_role(assign, pair)
}

/// `mix_role_of`, short for the line under an output strip's name.
pub(crate) fn mix_role_short(state: &TuxMix, pair: usize) -> &'static str {
    let assign = &state.control_room.assign;
    if pair == assign.main {
        "your speakers"
    } else if assign.is_phones(pair) {
        "your headphones"
    } else if crate::app::input_looped(state, pair * 2) {
        // Looped back: what your apps record or stream (critique #104 A P3-1).
        "your apps"
    } else {
        // The optical pairs told apart (critique #79 A 5: four strips all
        // said "optical out").
        match pair {
            0 => "XLR outs",
            1 => "phones jacks",
            2 => "optical 1/2",
            3 => "optical 3/4",
            4 => "optical 5/6",
            5 => "optical 7/8",
            _ => "optical out",
        }
    }
}

fn mix_role(assign: &control_room::Assign, pair: usize) -> &'static str {
    if pair == assign.main {
        "your speakers"
    } else if assign.is_phones(pair) {
        "your headphones"
    } else {
        // The optical pairs by number, as their strips say (critique #83 A 3).
        match pair {
            0 => "the XLR outputs",
            1 => "the phones jacks",
            2 => "optical 1/2",
            3 => "optical 3/4",
            4 => "optical 5/6",
            5 => "optical 7/8",
            _ => "the optical out",
        }
    }
}

/// The placard's last line: what feeds the mix, from the same model the
/// lamp pool reads (`app::feeds_mix`). Three names at most.
fn fed_line(state: &TuxMix) -> String {
    let names = crate::app::mix_feeders(state, state.sel_out);
    let fed = match names.len() {
        0 => "Nothing feeds it yet".to_string(),
        1 => format!("Fed by {}", names[0]),
        2 => format!("Fed by {} and {}", names[0], names[1]),
        3 => format!("Fed by {}, {} and {}", names[0], names[1], names[2]),
        n => format!("Fed by {}, {}, {} and\u{a0}{}\u{a0}more", names[0], names[1], names[2], n - 3),
    };
    // "audio" never alone on the second line (critique #84 A P3-4).
    let fed = fed.replace("computer audio", "computer\u{a0}audio");
    match fx_heard_words(state, state.sel_out) {
        Some(fx) => format!("{fed} \u{b7} {fx}"),
        None => fed,
    }
}

/// The effects' name on the placard: "FX" for both, as the Rec key's
/// "+ FX", so the fed line stays two lines with three feeders named.
fn fx_name(state: &TuxMix) -> Option<&'static str> {
    match (state.fx.reverb_on, state.fx.echo_on) {
        (true, false) => Some("Reverb"),
        (false, true) => Some("Echo"),
        (true, true) => Some("FX"),
        (false, false) => None,
    }
}

/// Whether the user listens on mix `pair`: Main, Main B, a Phones pair,
/// or the mix Quick listens on (critique #69 A 3: the not-heard row on a
/// mix kept dry on purpose turned a choice into a fault).
fn listened_on(state: &TuxMix, pair: usize) -> bool {
    let a = &state.control_room.assign;
    pair == a.main || a.main_b == Some(pair) || a.is_phones(pair) || pair == crate::quick::listen_pair(state)
}

/// What the fed line adds about Reverb & Echo in mix `pair`, from the one
/// "what you hear" model (`fx::block_in`; critique #67's bigger step):
/// "with Reverb" when heard on the unit. Nothing in the simulation, which
/// runs none (critique #68 A 1: it said "with Reverb" while the page and
/// Rec said nothing runs), and nothing when something's in the way: that
/// has its own row with the reason and the fix (`fx_unheard_row`).
pub(crate) fn fx_heard_words(state: &TuxMix, pair: usize) -> Option<String> {
    let fx = fx_name(state)?;
    (crate::fx::block_in(state, pair).is_none() && !state.device.is_mock()).then(|| format!("with {fx}"))
}

/// Why the effects aren't heard, in a few words (the placard's row).
pub(crate) fn block_short_words(state: &TuxMix, block: crate::fx::Block, pair: usize) -> String {
    use crate::fx::Block;
    let mix = mix_name(state, pair);
    match block {
        Block::Off => String::new(),
        Block::NoDriver | Block::OldDriver | Block::NoEngine => "needs a newer driver".into(),
        Block::NeedsProAudio => "needs the Pro Audio profile".into(),
        Block::ToolsMissing => "PipeWire's tools are missing".into(),
        Block::RateTooHigh(_) => "the unit runs above 48 kHz".into(),
        Block::Searching => "starting\u{2026}".into(),
        // One phrase for the return everywhere: "{mix}'s FX is turned down"
        // (critique #69 A 5, A 6).
        Block::NoSendNoReturn => "nothing goes into the effects yet".into(),
        Block::NoSend => "nothing goes into the effects yet".into(),
        Block::NoReturn => format!("{mix}'s FX is turned down"),
        Block::SendFollowsDown => "AN1/2 is down, so nothing feeds them".into(),
        Block::HeldByMute => "Mute FX is on".into(),
        Block::KeptForMic => "kept out for an open mic".into(),
    }
}

/// The placard's row when the effects are on but not heard in this mix:
/// the reason in a few words and the one press that fixes it (critique
/// #68 A 2: "Reverb not heard here" gave neither, and wrapped "here" onto
/// a third line). In the simulation only for what the mix itself sets
/// (a send, a return, Mute FX), since nothing runs there anyway.
/// The not-heard row's press and words, when it shows (`fx_unheard_row`;
/// the keyboard's stop reads it too, `keyfocus::CounterItem::FxUnheard`).
/// What "Make it heard" / "turn up …" does for `block`, one sentence for
/// the row and the FX-sends placard's key (critique #71 A 4: the key said it
/// brought up the return, already up with only the send down).
/// Make it heard's tip for mix `pair`: with an open mic on the speakers the
/// press turns up the headphones' return, not this mix's (critique #111
/// Fable B: "this mix's FX return" said there).
pub(crate) fn make_heard_tip_in(state: &TuxMix, pair: usize, block: crate::fx::Block) -> &'static str {
    use crate::fx::Block;
    if crate::fx::open_mic_reverb_risk(state, pair) && matches!(block, Block::NoSend | Block::NoReturn | Block::NoSendNoReturn) {
        return if state.control_room.assign.phones_pairs().is_empty() {
            "An open mic\u{2019}s reverb can howl in the speakers: assign headphones to hear it there"
        } else if block == Block::NoSend {
            "Turns up the FX send, heard in your headphones (not the speakers: an open mic\u{2019}s reverb there can howl). Undo takes it back"
        } else {
            "Turns up the FX send and your headphones\u{2019} FX return (not the speakers\u{2019}: an open mic\u{2019}s reverb there can howl). Undo takes it back"
        };
    }
    make_heard_tip(block)
}

pub(crate) fn make_heard_tip(block: crate::fx::Block) -> &'static str {
    match block {
        crate::fx::Block::NoSend => "Turns up the FX send, in one press (Undo takes it back)",
        crate::fx::Block::NoReturn => "Turns up this mix's FX return, in one press (Undo takes it back)",
        _ => "Turns up the FX send and this mix's FX return, in one press (Undo takes it back)",
    }
}

pub(crate) fn fx_unheard(state: &TuxMix) -> Option<(Message, String, String)> {
    use crate::fx::Block;
    let fx = fx_name(state)?;
    let pair = state.sel_out;
    // An open mic's reverb in the speakers: said whatever mix is being
    // edited, in the simulation too (critique #110 A P2-1, P2-2: the row
    // read "Reverb not heard" while it played there).
    if let Some(sp) = crate::fx::mic_reverb_hazard(state) {
        let has_phones = state.control_room.assign.phones_pairs().first().is_some();
        let verb = if has_phones { "move it to the headphones" } else { "take it out of the speakers" };
        return Some((
            Message::FxMakeHeard(sp),
            crate::celebrate::no_orphan(&format!("Open mic\u{2019}s {} in your speakers \u{b7} {verb}", crate::fx::on_words(state))),
            if has_phones {
                "It can howl there: one press takes the speakers\u{2019} FX return down (and brings it up in your headphones). Undo takes it back".to_string()
            } else {
                "It can howl there: one press takes the speakers\u{2019} FX return down. Assign headphones to hear it there. Undo takes it back".to_string()
            },
        ));
    }
    // Only for a mix you listen on (critique #69 A 3).
    if !listened_on(state, pair) {
        return None;
    }
    // On a short window with a card up, the page keys keep their room
    // (critique #69 A 2: the row pushed them out of sight at 1280x700).
    if crate::app::short_window(state) && (state.trick_card.is_some() || state.celebration.is_some()) {
        return None;
    }
    let block = crate::fx::block_in(state, pair)?;
    // Kept out on purpose: no row nudging the speakers' return up (critique
    // #108 A P1-1: it offered by hand what Make it heard refuses). With an
    // open mic and the speakers, none of the "not heard" rows: "turn up Main
    // 1/2's FX" and "release Mute FX" would each bring its reverb into the
    // speakers (critique #110 A, Opus: Mute FX's release; and the row pushed
    // the page keys under the guide card at 1280x700). The page says why.
    if block == Block::KeptForMic
        || (crate::fx::open_mic_reverb_risk(state, pair) && matches!(block, Block::NoReturn | Block::NoSendNoReturn | Block::HeldByMute))
    {
        return None;
    }
    let mix_set = matches!(block, Block::NoSend | Block::NoReturn | Block::NoSendNoReturn | Block::SendFollowsDown | Block::HeldByMute);
    if block == Block::Off || (state.device.is_mock() && !mix_set) {
        return None;
    }
    // The press's verb in the words, so a click's result is read before it
    // (critique #69 A 1: the › acted where the open-mic row's only opens);
    // a profile switch stays on the page's own labelled key.
    let mix = mix_name(state, pair);
    // A long mix name keeps the row to one line: "its FX" (critique #70
    // A 6).
    let mix_fx = if mix.chars().count() > 10 { "its FX".to_string() } else { format!("{mix}'s FX") };
    let (press, verb, tip): (Message, String, String) = match block {
        // The send is what's down: said so (critique #70 A 2: "turn up Main
        // 1/2's FX" when that was already up).
        Block::NoSend => (Message::FxMakeHeard(pair), "turn up the FX send".into(), make_heard_tip_in(state, pair, block).into()),
        Block::NoReturn | Block::NoSendNoReturn => (Message::FxMakeHeard(pair), format!("turn up {mix_fx}"), make_heard_tip_in(state, pair, block).into()),
        // Release promises the effects back only where something feeds
        // them (critique #71 A 2).
        Block::HeldByMute => (
            Message::MuteFxToggle,
            "release Mute FX".into(),
            match crate::fx::block_past_mute(state, pair) {
                None => "Mute FX off: the effects come back where it held them".into(),
                Some(Block::NoSend | Block::NoSendNoReturn) => "Mute FX off. Nothing is sent to the effects yet, so turn up a send next".into(),
                Some(Block::NoReturn) => format!("Mute FX off. {mix}'s FX is turned down too, so turn that up next"),
                Some(_) => "Mute FX off. Reverb & Echo then says what else is in the way".into(),
            },
        ),
        _ => (Message::SidebarTab(Tab::Fx), "see why".into(), "Open Reverb & Echo, which says what to do".into()),
    };
    let what = if fx == "FX" { "Effects" } else { fx };
    let said = crate::celebrate::no_orphan(&format!("{what} not heard \u{b7} {verb}"));
    // The reason in a few words, on its hover with what the press does;
    // in the simulation, that nothing will sound anyway (critique #70 A 4).
    let why = block_short_words(state, block, pair);
    let stop = if why.ends_with('\u{2026}') { "" } else { "." };
    let sim = if state.device.is_mock() { " (The simulation plays no effects.)" } else { "" };
    let tip = format!("Why: {why}{stop} {tip}{sim}");
    Some((press, said, tip))
}

fn fx_unheard_row(state: &TuxMix, scale: f32) -> Option<Element<'_, Message>> {
    let (press, said, tip) = fx_unheard(state)?;
    // The keyboard reaches it after "Only these" (critique #69 A 7).
    let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::FxUnheard);
    Some(hint_at(
        crate::keyfocus::placard_ring(
        button(
            row![
                // A hollow ring at full strength: information, apart from
                // the open-mic warning's filled amber dot (critique #70 A 3;
                // #69 B: 2.37 in Solarized at 60%).
                container(crate::widgets::status_ring(theme::on_placard_sec(), scale)),
                text(said).size(theme::TEXT_SM * scale).color(theme::on_placard()).width(Length::Fill),
                text("\u{203a}").size(theme::TEXT_SM * scale).color(theme::on_placard_sec()),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        )
        .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
        .width(Length::Fill)
        .style(theme::placard_button(false))
        .on_press(press)
        .into(),
            focused,
            theme::RADIUS_SM * scale,
        ),
        tip,
        scale,
        iced::widget::tooltip::Position::Left,
    ))
}

/// The Now Mixing placard (see the module doc). The name is a
/// `pick_list`, printed like the card's title until the cursor is on it.
fn placard(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    if state.fader_fx {
        return fx_placard(state, scale);
    }
    // The open menu lists each mix with what it is, in the body's size
    // (critique #104 A P3-4, carried: bare codes in a 36 px serif). iced's
    // menu takes the closed list's font, so the title is drawn over a list
    // whose own words are clear: the card looks as it did.
    let choice = |pair| MixChoice {
        pair,
        name: format!("{} \u{00b7} {}", mix_name(state, pair), mix_role_short(state, pair)),
    };
    let choices: Vec<MixChoice> = (0..OUT_LABELS.len()).map(choice).collect();
    let left = theme::SPACE_SM * scale - crate::keyfocus::PLACARD_RING_SLOT;
    // As tall as the title's line, so the card keeps its height.
    let pad_v = ((theme::TEXT_BIG - theme::TEXT_MD) * 1.3 * scale / 2.0).max(0.0);
    let list = pick_list(choices, Some(choice(state.sel_out)), |c: MixChoice| Message::SelectOutput(c.pair))
        .font(theme::SANS)
        .text_size(theme::TEXT_MD * scale)
        // The ring's slot comes out of the left padding, so the name
        // keeps the card's left edge (critique #75 A 2); the right keeps
        // its own, the caret 5 px from the ring (critique #76 A 3).
        // Its own words (the open menu's) inset from the menu's edge; the
        // title over it keeps the card's left edge with its own padding.
        .padding(iced::Padding { top: pad_v, bottom: pad_v, left: theme::SPACE_MD * scale, right: theme::SPACE_SM * scale })
        .width(Length::Fill)
        .style(theme::placard_pick_list_titled)
        .menu_style(theme::menu);
    let title = container(
        text(mix_name(state, state.sel_out))
            .font(theme::SERIF)
            .size(theme::TEXT_BIG * scale)
            .color(theme::on_placard())
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .padding(iced::Padding { left, ..iced::Padding::ZERO })
    .height(Length::Fill)
    .align_y(iced::alignment::Vertical::Center);
    // The tooltip goes to the left, over the mixer: the mix menu opens
    // below the name, and a tooltip there covered its first row.
    let name = hint_at(
        crate::keyfocus::placard_ring_flat(
            iced::widget::stack![list, title].into(),
            crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::MixName),
            theme::RADIUS_SM * scale,
        ),
        "Choose the mix to edit (or click an output strip)",
        scale,
        iced::widget::tooltip::Position::Left,
    );
    let rule = container(iced::widget::Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
        .style(|_t: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(Color {
                a: 0.18,
                ..theme::on_placard()
            })),
            ..container::Style::default()
        });
    let warning = open_mic_warning(state, scale);
    let mut col = column![
            name,
            container({
                // Locked, said at rest where the mix is named (critique #8:
                // only a lit padlock and a refused drag said it).
                let role: Element<'_, Message> = if state.ui_locked {
                    hint_at(
                        Element::from(row![
                            container(crate::widgets::strip_icons::lock_icon(true, theme::on_placard(), scale * 0.75, 0.0))
                                .width(Length::Fixed(14.0 * scale))
                                .height(Length::Fixed(14.0 * scale)),
                            text(format!("Locked \u{00b7} {}", mix_role_of(state, state.sel_out)))
                                .size(theme::TEXT_SM * scale)
                                .font(theme::SANS_SEMI)
                                .color(theme::on_placard()),
                        ]
                        .spacing(theme::SPACE_SM * scale)
                        .align_y(iced::Alignment::Center)),
                        "The mix is locked: nothing changes it until the padlock at the top unlocks it",
                        scale,
                        iced::widget::tooltip::Position::Left,
                    )
                } else {
                    text(format!("Now mixing \u{00b7} {}", mix_role_of(state, state.sel_out)))
                        .size(theme::TEXT_SM * scale)
                        .color(theme::on_placard_sec())
                        .into()
                };
                // In Quick, where its own listening may be another mix: said,
                // so "where am I listening?" has one answer (critique #6: the
                // card said Main 1/2 while Quick said Phones 3/4).
                let quick_pair = crate::quick::listen_pair(state);
                // On a short window it stands aside while the open-mic
                // warning is opened, so the head keeps its tools row (the
                // reviewer: Quick, the warning and its "All set" card).
                // And while a learning card has the slot on a short window
                // (the owner's pick after critique #27: every key in view;
                // Quick's page says where you listen anyway).
                // Not the Trim Gains line, which says what every fader does
                // (critique #28: hidden under a card, critique #15's fix
                // was undone).
                let room = !(crate::app::head_tight(state) && state.open_mic_expanded);
                let quick_room = room && !snug(state);
                // Trim Gains on: said where the mix is named (critique #15:
                // only the coral T keys told that a fader now moves every
                // mix, the Phones' too).
                let line = if state.trim_all && room {
                    Some("Trim Gains: each fader moves every mix".to_string())
                } else if state.view == crate::app::View::Quick && quick_pair != state.sel_out && quick_room {
                    Some(format!("Quick setup listens on {}", if state.plain_names { mix_role_of(state, quick_pair).to_string() } else { mix_name(state, quick_pair) }))
                } else {
                    None
                };
                match line {
                    Some(line) => column![
                        role,
                        text(line).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI).color(theme::on_placard()),
                    ]
                    .spacing(theme::SPACE_TIGHT * scale)
                    .into(),
                    None => role,
                }
            })
            .padding([0.0, theme::SPACE_SM * scale]),
            rule,
            // "Only these": the mixer shows just what feeds this mix, so a
            // headphone mix's mic and DAW return sit side by side even on
            // a short window (the "in mix" channel view).
            {
                let focused = state.channel_view == crate::app::ChannelView::Submix;
                row![
                    // "Fed by …" opens the map of this mix beside its
                    // output's panel (issue #23, the owner's pick).
                    hint_at(
                        // The row's spare width, its words wrapping as the
                        // plain line's did, so "Only these" keeps its own;
                        // no "›" (it wrapped onto a line of its own): the
                        // small map before it says it opens the map, the
                        // tint under the pointer and its hint too.
                        crate::keyfocus::placard_ring_flat(button(
                            row![
                                crate::widgets::strip_icons::map_icon(theme::on_placard_sec(), scale),
                                text(fed_line(state)).size(theme::TEXT_SM * scale).width(Length::Fill),
                            ]
                            .spacing(theme::SPACE_TIGHT * scale)
                            .align_y(iced::Alignment::Center),
                        )
                            .padding(0)
                            .width(Length::Fill)
                            .style(theme::placard_link)
                            .on_press(Message::FlowOpen(crate::flow::FlowOf::Mix(state.sel_out)))
                            .into(),
                            crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::FedBy),
                            theme::RADIUS_SM * scale,
                        ),
                        "A map of everything you hear in this mix",
                        scale,
                        iced::widget::tooltip::Position::Bottom,
                    ),
                    hint_at(
                        crate::keyfocus::placard_ring(
                            button(text(if focused { "Show all" } else { "Only these" }).size(theme::TEXT_SM * scale))
                                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                                .style(theme::placard_button(focused))
                                .on_press(Message::SetChannelView(if focused {
                                    crate::app::ChannelView::Layout
                                } else {
                                    crate::app::ChannelView::Submix
                                }))
                                .into(),
                            crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::OnlyThese),
                            theme::RADIUS_SM * scale,
                        ),
                        if focused {
                            "Show every strip again"
                        } else {
                            "Show only what feeds this mix: on a short window its inputs and playback sit side by side"
                        },
                        scale,
                        iced::widget::tooltip::Position::Left,
                    ),
                ]
                .spacing(theme::SPACE_SM * scale)
                .align_y(iced::Alignment::Center)
                // The Fed-by line's ring slot comes out of this padding, so its words keep the card's one left edge (critique #75 A 2).
                .padding(iced::Padding { top: 0.0, bottom: 0.0, left: theme::SPACE_SM * scale - crate::keyfocus::PLACARD_RING_SLOT, right: theme::SPACE_SM * scale })
            },
        ]
        .spacing(theme::SPACE_MD * scale);
    // The safety warning first, the information under it (critique #70 A 3).
    // The two rows keep their focus-ring slots (`keyfocus::placard_ring`)
    // and no spacing between them, the slots making the gap, and the last
    // slot comes out of the card's foot: the slots cost the card no height
    // the page keys needed at 1280x700 (critique #72 fix, measured).
    let slot = crate::keyfocus::PLACARD_RING_SLOT;
    let mut rows = column![];
    let mut keyed_foot = false;
    if let Some(w) = warning {
        rows = rows.push(w);
        keyed_foot = true;
    }
    if let Some(row) = fx_unheard_row(state, scale) {
        rows = rows.push(row);
        keyed_foot = true;
    }
    if keyed_foot {
        col = col.push(rows);
    }
    if let Some(note) = output_proc_note(state, scale) {
        col = col.push(note);
        keyed_foot = false;
    }
    let foot = theme::SPACE_XL * scale - if keyed_foot { slot } else { 0.0 };
    container(col)
    .padding(iced::Padding { top: theme::SPACE_XL * scale, bottom: foot, left: theme::SPACE_LG * scale, right: theme::SPACE_LG * scale })
    .width(Length::Fill)
    .style(placard_style(state))
    .into()
}

/// Main or a Phones played inverted or as mid/side, said where the mix is
/// read (critique #18: once the output's panel closed nothing showed it,
/// and the listener blamed the mic): one line, which opens that output's
/// Settings, where it is switched off.
/// A warning dot on the placard: the attention hue made to read on the
/// card's face (`theme::attention_dot_on`), faded once noted. Every placard
/// row's dot comes from here (critique #77 A 1, #78 A 3: a row that took the
/// floors' raw amber read 1.53 on a dark theme's card).
pub(crate) fn placard_dot<'a>(quiet: bool, scale: f32) -> Element<'a, Message> {
    let amber = placard_dot_color();
    crate::widgets::status_dot(if quiet { iced::Color { a: 0.55, ..amber } } else { amber }, scale)
}

pub(crate) fn placard_dot_color() -> iced::Color {
    theme::attention_dot_on(theme::text_primary())
}

/// The output note's press, while the note shows (its keyboard stop,
/// `keyfocus::CounterItem::OutputNote`).
pub(crate) fn output_note_press(state: &TuxMix) -> Option<Message> {
    let (pair, _) = output_note_of(state)?;
    let cid = ChannelId::Output(if state.device.outputs_one_per_pair() { pair } else { pair * 2 });
    Some(Message::ToggleFlyout(cid, crate::widgets::strip::FlyoutKind::Settings))
}

/// The mix before or after the one being edited, for the placard name's
/// keyboard stop (left and right).
pub(crate) fn mix_step(state: &TuxMix, step: i32) -> Option<Message> {
    let n = OUT_LABELS.len() as i32;
    (n > 0).then(|| Message::SelectOutput(((state.sel_out as i32 + step).rem_euclid(n)) as usize))
}

/// The listening pair the placard's output note speaks of, and its words.
/// Not the Control Room's own Mid/Side check while its keys stand in this
/// column under the placard: the lit key says it, and the note's row
/// pushed the keys down a row as it came, Dim landing under the pointer
/// that had just pressed Mid/Side (the double-press check, 2026-11-18).
pub(crate) fn output_note_of(state: &TuxMix) -> Option<(usize, String)> {
    let (pair, words) = crate::app::listener_pairs(state)
        .into_iter()
        .find_map(|p| crate::app::output_proc_words(state, p).map(|w| (p, w)))?;
    let check_only = state.control_room.ms_check.is_some_and(|(p, _)| p == pair) && words.ends_with("plays mid and side");
    (!(check_only && !control_room::keys_in_mixer(state))).then_some((pair, words))
}

fn output_proc_note(state: &TuxMix, scale: f32) -> Option<Element<'_, Message>> {
    let (pair, words) = output_note_of(state)?;
    let cid = ChannelId::Output(if state.device.outputs_one_per_pair() { pair } else { pair * 2 });
    Some(hint_at(
        crate::keyfocus::placard_ring(button(
            row![
                // The placard's dot, as the open-mic row's: raw amber read 1.53 on
                // a dark theme's cream card (critique #77 A 1).
                container(placard_dot(false, scale)),
                text(words).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).color(theme::on_placard()).width(Length::Fill),
                text("\u{203a}").size(theme::TEXT_SM * scale).color(theme::on_placard_sec()),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        )
        .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
        .width(Length::Fill)
        .style(theme::placard_button(false))
        .on_press(Message::ToggleFlyout(cid, crate::widgets::strip::FlyoutKind::Settings))
        .into(),
        crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::OutputNote),
        theme::RADIUS_SM * scale,
        ),
        "Its Settings, where phase and M/S are switched off",
        scale,
        iced::widget::tooltip::Position::Left,
    ))
}

/// The placard while the faders set FX sends (the owner's choice from
/// mockups, 2026-11-09: FX mode as another mix): Reverb & Echo named where
/// the mix is, what feeds it, and the way back to the mix. The open-mic
/// warning and the speakers' note stay.
/// The FX-sends placard's second line, whether its "Make it heard" key
/// shows, and whether its "Turn on Reverb" key does: all by the one model
/// (`fx::block_in`), so the header and the Reverb & Echo page agree.
pub(crate) fn fx_sends_head(state: &TuxMix) -> (String, bool, bool) {
    use crate::fx::Block;
    let pair = state.sel_out;
    let mix = mix_said(state, pair);
    let block = crate::fx::block_in(state, pair);
    // Keys only where the effects are (OpenCode's job 63).
    let can = crate::fx::in_panels(state);
    // Every block it names (OpenCode's job 71: with only the send down, the
    // header said "Now setting" while the page said not heard); the key
    // only where its press is the fix. Both effects off: said, with the
    // press that fixes it (critique #60 A 4); a return off: said with the
    // page's own press (critique #61 A 1).
    let off = can && block == Some(Block::Off);
    let fixable = can && matches!(block, Some(Block::NoSend | Block::NoReturn | Block::NoSendNoReturn));
    // An open mic's reverb in the speakers, whatever mix is set: said here
    // too, the row under it holds the press (critique #111 A P2-1: with the
    // faders on FX sends and the page closed, nothing said it).
    if crate::fx::mic_reverb_hazard(state).is_some() {
        return ("FX sends \u{00b7} open mic\u{2019}s reverb in your speakers".to_string(), false, false);
    }
    let setting = match block {
        // Where the key is, in plain words (critique #62 A 5); "silences"
        // only where something would be heard without it (critique #68 A 4).
        Some(Block::HeldByMute) if crate::fx::silenced_by_mute(state, pair) => format!("FX sends \u{00b7} effects muted in {mix}"),
        Some(Block::HeldByMute) => "FX sends \u{00b7} Mute FX is on".to_string(),
        _ if off => "FX sends \u{00b7} Reverb and Echo off".to_string(),
        // Every other block, the system's own too (OpenCode's job 72).
        Some(_) => format!("FX sends \u{00b7} not heard in {mix} yet"),
        _ => "Now setting \u{00b7} FX sends".to_string(),
    };
    (setting, fixable, off)
}

/// The FX-sends placard's second key's press, when one shows beside Back:
/// Make it heard, Turn on Reverb or Release Mute FX (its keyboard stop
/// reads it, `keyfocus::CounterItem::FxSendsKey`).
pub(crate) fn fx_sends_second(state: &TuxMix) -> Option<Message> {
    let pair = state.sel_out;
    let (_, fixable, off) = fx_sends_head(state);
    if fixable {
        Some(Message::FxMakeHeard(pair))
    } else if off {
        Some(Message::FxOn(crate::fx::Unit::Reverb, true))
    } else if crate::fx::block_in(state, pair) == Some(crate::fx::Block::HeldByMute) {
        // Released, an open mic's reverb would play in the speakers: the
        // move instead (critique #111 Opus A P2-1).
        Some(crate::fx::mute_holds_mic_reverb(state).map_or(Message::MuteFxToggle, Message::FxMakeHeard))
    } else {
        None
    }
}

fn fx_placard(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    use crate::fx::Block;
    let feeders: Vec<String> = crate::app::mixer_strip_order(state)
        .into_iter()
        .filter(|&c| crate::app::feeds_fx(state, c))
        .map(|c| crate::app::strip_display_name(state, c))
        .collect();
    let fed = match feeders.len() {
        // A fader raised would put an open mic's reverb in the speakers
        // (critique #111 Opus A P2-1): the press that keeps it out.
        0 if fx_sends_head(state).1
            && std::iter::once(state.control_room.assign.main)
                .chain(state.control_room.assign.main_b)
                .any(|p| crate::fx::open_mic_reverb_risk(state, p) && crate::fx::user_return(state, p) > 0.0) =>
        {
            "No FX send is up: press Make it heard (it keeps an open mic\u{2019}s reverb out of your speakers)".to_string()
        }
        0 => "No FX send is up: raise a fader".to_string(),
        1..=3 => format!("Fed by {}", feeders.join(", ")),
        n => format!("Fed by {}, {} and\u{a0}{}\u{a0}more", feeders[0], feeders[1], n - 2),
    };
    // Each key a keyboard stop, ringed when it has the focus (they had
    // none, and Tab stopped on the hidden "Only these" instead).
    let ringed = |b: iced::widget::Button<'static, Message>, item| -> Element<'static, Message> {
        crate::keyfocus::placard_ring(b.into(), crate::keyfocus::counter_focused(state, item), theme::RADIUS_SM * scale)
    };
    let back = hint_at(
        ringed(
            button(text(format!("Back to {}", mix_said(state, state.sel_out))).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                .style(theme::placard_button(true))
                .on_press(Message::FaderFxToggle),
            crate::keyfocus::CounterItem::FxSendsBack,
        ),
        "The faders set the mix again",
        scale,
        iced::widget::tooltip::Position::Left,
    );
    // Both effects off, the sends feed nothing heard: said, with the press
    // that fixes it beside Back, in the same rows (critique #60 A 4: a
    // send raised gave silence with no reason shown).
    let pair = state.sel_out;
    let (setting, fixable, off) = fx_sends_head(state);
    let keys: Element<'_, Message> = if fixable {
        row![
            back,
            hint_at(
                ringed(button(text("Make it heard").size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::placard_button(true))
                    .on_press(Message::FxMakeHeard(pair)), crate::keyfocus::CounterItem::FxSendsKey),
                crate::fx::block_in(state, pair).map_or("", |b| make_heard_tip_in(state, pair, b)),
                scale,
                iced::widget::tooltip::Position::Left,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .into()
    } else if off {
        row![
            back,
            hint_at(
                ringed(button(text("Turn on Reverb").size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::placard_button(true))
                    .on_press(Message::FxOn(crate::fx::Unit::Reverb, true)), crate::keyfocus::CounterItem::FxSendsKey),
                "Switches Reverb on, so the sends are heard through each output's FX return",
                scale,
                iced::widget::tooltip::Position::Left,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .into()
    } else if let (Some(Block::HeldByMute), Some(sp)) = (crate::fx::block_in(state, pair), crate::fx::mute_holds_mic_reverb(state)) {
        // Released, an open mic's reverb would play in the speakers: the
        // move first (critique #111 Opus A P2-1).
        row![
            back,
            hint_at(
                ringed(button(text(if state.control_room.assign.phones_pairs().is_empty() { "Take it out of the speakers" } else { "Move it to the headphones" }).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::placard_button(true))
                    .on_press(Message::FxMakeHeard(sp)), crate::keyfocus::CounterItem::FxSendsKey),
                "Released now, an open mic\u{2019}s reverb would play in your speakers, where it can howl: this takes the speakers\u{2019} FX return down first (Undo takes it back)",
                scale,
                iced::widget::tooltip::Position::Left,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .into()
    } else if crate::fx::block_in(state, pair) == Some(crate::fx::Block::HeldByMute) {
        // Mute FX in the way: its release here, in one press (critique #71
        // A's idea: the header named it, the key was down in the Control
        // Room).
        row![
            back,
            hint_at(
                ringed(button(text("Release Mute FX").size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::placard_button(true))
                    .on_press(Message::MuteFxToggle), crate::keyfocus::CounterItem::FxSendsKey),
                match crate::fx::block_past_mute(state, pair) {
                    _ if crate::fx::silenced_by_mute(state, pair) => "Mute FX off: the effects come back where it held them",
                    Some(Block::NoSend | Block::NoSendNoReturn) => "Mute FX off. Nothing is sent to the effects yet, so raise a fader next",
                    Some(Block::NoReturn) => "Mute FX off. This mix\u{2019}s FX return is turned down too, so turn that up next",
                    _ => "Mute FX off. Reverb & Echo then says what else is in the way",
                },
                scale,
                iced::widget::tooltip::Position::Left,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .into()
    } else {
        back.into()
    };
    let mut col = column![
        // Inset as the mix placard's lines are (its name in the pick list's
        // padding, its role line in its own), so the text keeps one left edge
        // when the card switches (critique #76 A, #77: 1167 vs 1171).
        container(text("Reverb & Echo").font(theme::SERIF).size(theme::TEXT_BIG * scale).color(theme::on_placard())).padding([0.0, theme::SPACE_SM * scale]),
        container(text(setting).size(theme::TEXT_SM * scale).color(theme::on_placard_sec())).padding([0.0, theme::SPACE_SM * scale]),
        container(text(fed).size(theme::TEXT_SM * scale).color(theme::on_placard_sec())).padding([0.0, theme::SPACE_SM * scale]),
    ]
    .spacing(theme::SPACE_SM * scale);
    // The keys and the warning keep their focus-ring slots with no spacing
    // between them, and the last slot comes out of the card's foot, as on
    // the mix's placard (critique #73 A 4: the slots made this card 12 px
    // taller, not the 3 px the log said).
    let slot = crate::keyfocus::PLACARD_RING_SLOT;
    let mut rows = column![keys];
    let mut keyed_foot = true;
    if let Some(w) = open_mic_warning(state, scale) {
        rows = rows.push(w);
    }
    // The hazard's row and its press, as on the mix's placard (critique
    // #111 A P2-1).
    if crate::fx::mic_reverb_hazard(state).is_some() {
        if let Some(row) = fx_unheard_row(state, scale) {
            rows = rows.push(row);
        }
    }
    col = col.push(rows);
    if let Some(note) = output_proc_note(state, scale) {
        col = col.push(note);
        keyed_foot = false;
    }
    let foot = theme::SPACE_XL * scale - if keyed_foot { slot } else { 0.0 };
    container(col)
        .padding(iced::Padding { top: theme::SPACE_XL * scale, bottom: foot, left: theme::SPACE_LG * scale, right: theme::SPACE_LG * scale })
        .width(Length::Fill)
        .style(placard_style(state))
        .into()
}

/// The placard's look: close over the levels where the column draws snug
/// or tight (`theme::placard_close`), its full lift otherwise.
fn placard_style(state: &TuxMix) -> fn(&iced::Theme) -> iced::widget::container::Style {
    if snug(state) || crate::app::head_tight(state) {
        theme::placard_close
    } else {
        theme::placard
    }
}

/// How long the placard says a mic was taken out of the speakers.
const OPEN_MIC_SAID_FOR: std::time::Duration = std::time::Duration::from_secs(4);

/// The open mic the placard is warning about now, if any (the keyboard's
/// stops for its answers). In Quick, whose own step says it for its mix,
/// only another mix's.
pub(crate) fn open_mic_shown(state: &TuxMix) -> Option<(ChannelId, usize)> {
    if state.open_mic_taken.is_some_and(|(_, _, at)| at.elapsed() < OPEN_MIC_SAID_FOR) {
        return None;
    }
    let shown = open_mic_listed(state);
    // Its answers open, the mic they were opened for stays named while it
    // can still howl, so a press acts on the mic read (OpenCode's job 60:
    // a hotter one named a frame later took the press).
    if let Some(held) = state.open_mic_held.filter(|h| state.open_mic_expanded && shown.contains(h)) {
        return Some(held);
    }
    // A mic new to the warning, or hotter than when it was seen, is named
    // first: a quiet one named ahead of it would hide the news (job 56).
    shown.iter().copied().find(|(c, p)| !crate::app::open_mic_quiet(state, *c, *p)).or_else(|| shown.first().copied())
}

/// The open mics the placard can warn about: those that can howl, less
/// the one Quick's own step already says for its mix (`open_mic_shown`
/// picks one, `open_mic_names` names them all).
fn open_mic_listed(state: &TuxMix) -> Vec<(ChannelId, usize)> {
    let quick_says = state.view == crate::app::View::Quick && crate::quick::speaker_risk(state);
    // Only when it can howl (critique #14, the owner's choice): the
    // speakers up and not muted, and the mic picking up the room lately;
    // else it was a warning that never went away. Dim doesn't hide it: the
    // mic is still in the speakers, only quieter, and hiding it read as
    // "safe" and moved the whole column (critique #39, the owner's pick:
    // "keep it, reworded"; `open_mic_warning` says it's quieter).
    let shown = open_mic_candidates(state)
        .into_iter()
        .filter(|(c, p)| !(quick_says && *c == state.quick_channel && *p == crate::quick::listen_pair(state)))
        .collect::<Vec<_>>();
    shown
}

/// The open mics the placard names: the one it warns about first, then
/// any other that can howl ("AN1 + AN2"; a third and on as "+ 2 more"),
/// so a second hot mic isn't hidden until the first is dealt with
/// (critique #58). Each opens to its own answers.
pub(crate) fn open_mic_names(state: &TuxMix, first: ChannelId) -> String {
    let mut names = vec![crate::app::strip_display_name(state, first)];
    // From the list the line was chosen from: never the mic Quick's step
    // already warns about (OpenCode's job 60).
    for (c, _) in open_mic_listed(state) {
        let n = crate::app::strip_display_name(state, c);
        if !names.contains(&n) {
            names.push(n);
        }
    }
    match names.len() {
        1 | 2 => names.join(" + "),
        n => format!("{} + {} + {} more", names[0], names[1], n - 2),
    }
}

/// Every open mic that can howl now (`open_mic_shown` names one).
pub(crate) fn open_mic_candidates(state: &TuxMix) -> Vec<(ChannelId, usize)> {
    let heard = |c: &ChannelId| state.open_mic_heard.get(c).is_some_and(|at| at.elapsed() < crate::quick::MIC_HEARD_FOR);
    crate::quick::open_mics_in_speakers(state)
        .into_iter()
        .filter(|(c, p)| heard(c) && crate::quick::speakers_up(state, *p))
        .collect()
}

/// An open, hot mic in the speakers, said on the placard where "which mix,
/// fed by what" is read, whatever the view (critique #6: Quick warned of a
/// howl, the Mixer showed the same state without a word): one line that
/// opens to two answers: take it out of that mix (one undo step), or "It's
/// meant to be there", which quiets it for that mic in that mix, remembered
/// between starts until Audra settings → App warns again.
fn open_mic_warning(state: &TuxMix, scale: f32) -> Option<Element<'_, Message>> {
    // Just taken out: the slot answers the press a moment before it closes.
    if let Some((cid, pair, _)) = state.open_mic_taken.filter(|(_, _, at)| at.elapsed() < OPEN_MIC_SAID_FOR) {
        return Some(
            container(
                text(format!(
                    "{} is out of {}. Undo brings it back.",
                    crate::app::strip_display_name(state, cid),
                    mix_said(state, pair)
                ))
                .size(theme::TEXT_SM * scale)
                .color(theme::on_placard_sec()),
            )
            .padding([0.0, theme::SPACE_SM * scale])
            .into(),
        );
    }
    let (cid, pair) = open_mic_shown(state)?;
    let name = open_mic_names(state, cid);
    let mix = mix_said(state, pair);
    // Opened once and nothing hotter since: a calm line, no amber (the
    // owner's pick after critique #43).
    let quiet = crate::app::open_mic_quiet(state, cid, pair);
    // Dim lowers whichever Main plays (Main B with Speaker B on), and the
    // mix is Main's either way (OpenCode's job 54).
    let assign = state.control_room.assign;
    let dimmed = state.control_room.is_dimmed() && (pair == assign.main || Some(pair) == assign.main_b);
    let said = if dimmed { format!("Open mic: {name} \u{00b7} Dim quiets it") } else { format!("Open mic in your speakers: {name}") };
    // One line, the dot and the mic, which opens to its answers (critique
    // #7: three lines and two keys took the room the Control Room needed on
    // a short window, and read as the card's secondary lines).
    let open = state.open_mic_expanded;
    let line = hint_at(
        crate::keyfocus::placard_ring(
            button(
                row![
                    // Quiet, it stays a warning you've noted: the amber dot
                    // faded, the words in the card's ink at regular weight
                    // (critique #44: in grey it read as the "Fed by" line,
                    // and Solarized's grey came to 4.2:1).
                    // The amber pulled toward the card's ink until it reads on
                    // the placard's face (critique #70 B: 1.43-3.28 there, the
                    // raw hue made for the floors, not this inverted card).
                    container({
                        placard_dot(quiet, scale)
                    }),
                    text(said)
                        .font(if quiet { theme::SANS } else { theme::SANS_SEMI })
                        .size(theme::TEXT_SM * scale)
                        .color(theme::on_placard())
                        .width(Length::Fill),
                    text(if open { "\u{25be}" } else { "\u{203a}" }).size(theme::TEXT_SM * scale).color(theme::on_placard_sec()),
                ]
                .spacing(theme::SPACE_SM * scale)
                .align_y(iced::Alignment::Center),
            )
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
            .width(Length::Fill)
            .style(theme::placard_button(false))
            .on_press(Message::OpenMicExpand)
            .into(),
            crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::OpenMicExpand),
            theme::RADIUS_SM * scale,
        ),
        if open { "Close this" } else { "What it means, and what to do" },
        scale,
        iced::widget::tooltip::Position::Left,
    );
    if !open {
        return Some(line);
    }
    let keys = row![
        iced::widget::Space::new().width(Length::Fill),
        // The safe answer, filled: it carries the weight.
        hint_at(
            crate::keyfocus::placard_ring(
                button(text("Take it out").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::placard_button(true))
                    .on_press(Message::OpenMicTakeOut(cid, pair))
                    .into(),
                crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::OpenMicTakeOut),
                theme::RADIUS_SM * scale,
            ),
            "Send it nothing in this mix (Undo brings it back)",
            scale,
            iced::widget::tooltip::Position::Left,
        ),
        hint_at(
            crate::keyfocus::placard_ring(
                button(text("Hide: I listen on headphones").size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::placard_button(false))
                    .on_press(Message::OpenMicKeep(cid, pair))
                    .into(),
                crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::OpenMicKeep),
                theme::RADIUS_SM * scale,
            ),
            "No more warning for this mic in this mix, remembered (Audra settings \u{2192} App warns again)",
            scale,
            iced::widget::tooltip::Position::Left,
        ),
    ]
    .spacing(theme::SPACE_SM * scale);
    Some(
        column![
            line,
            container(
                column![
                    // One line: the mic is named on the line above.
                    text(format!("Near the mic, {mix} can howl."))
                        .size(theme::TEXT_SM * scale)
                        .color(theme::on_placard_sec()),
                    keys,
                ]
                .spacing(theme::SPACE_SM * scale),
            )
            .padding([0.0, theme::SPACE_SM * scale]),
        ]
        .spacing(theme::SPACE_SM * scale)
        .into(),
    )
}

/// A level as the counter prints it: one decimal, a true minus sign.
fn db_number(v: f32) -> String {
    if v > 0.0 {
        let db = 20.0 * v.log10();
        let s = format!("{:.1}", db.abs());
        // Signed both ways (critique #57), as the strips print it.
        if db < -0.05 { format!("\u{2212}{s}") } else if db >= 0.05 { format!("+{s}") } else { s }
    } else {
        "\u{2212}\u{221e}".to_string()
    }
}

/// A large level readout set in the placard's serif, whose zero is plain
/// (both Atkinson faces slash theirs, which at this size read as the
/// strips' phase "Ø"). The serif's figures are proportional, so each
/// character sits in a fixed cell, centred, as tabular figures would:
/// the number holds its width and place while a fader moves it. Cell
/// widths are the font's own advances in em (the widest digit, "8", is
/// 0.652). The serif has no "∞", so silence draws it from the Mono.
fn tabular_level<'a>(value: &str, size: f32, color: Color) -> Element<'a, Message> {
    let mut cells = row![].align_y(iced::Alignment::Center);
    for ch in value.chars() {
        let (em, font) = match ch {
            '0'..='9' => (0.66, theme::SERIF),
            '.' => (0.28, theme::SERIF),
            '\u{2212}' => (0.79, theme::SERIF),
            _ => (0.8, theme::MONO),
        };
        cells = cells.push(
            container(text(ch.to_string()).font(font).size(size).color(color))
                .center_x(Length::Fixed(em * size)),
        );
    }
    cells.into()
}

/// The Main and Phones levels.
fn levels(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let assign = state.control_room.assign;
    // The pair playing the Main mix: Main, or Main B with Speaker B on.
    let mut pairs = vec![control_room::monitor_pair(state)];
    pairs.extend(assign.phones_pairs());
    // A short window draws them tighter, so the head keeps the snapshot
    // and page keys in view under a card (critique #8's P1).
    let gap = if crate::app::head_tight(state) { theme::SPACE_MD } else { theme::SPACE_XL };
    iced::widget::Column::with_children(pairs.into_iter().map(|pair| level(state, pair, scale)))
        .spacing(gap * scale)
        .width(Length::Fill)
        .into()
}

/// One monitor level: the role and pair, the level heard (Main reads
/// amber while dimmed, with the level it returns to), and a slider on
/// the faders' dB taper that moves the user's own level.
fn level(state: &TuxMix, pair: usize, scale: f32) -> Element<'_, Message> {
    let ch = if state.device.outputs_one_per_pair() { pair } else { pair * 2 };
    let Some(out) = state.device.outputs().get(ch) else {
        return iced::widget::Space::new().into();
    };
    let user = control_room::displayed_level(state, ch, out.volume);
    let main = pair == control_room::monitor_pair(state);
    let dimmed = main && state.control_room.is_dimmed();
    // Letting go of Dim glides the sound back (the owner's pick): the
    // readout climbs with it and its amber fades, so the gentle return is
    // seen, not only heard (critique #35 asked that it be felt).
    let easing = state.control_room.dim_easing.filter(|_| main && state.control_room.dimmed_main.is_some());
    // The number is your level, Dim or not, as the fader and the Main
    // strip show it; Dim's effect is said in the note beside it, in the
    // lamp (critique #39, the owner's pick: one rule; the counter said
    // −20.0 while the strip beside the lit Dim key said 0.0).
    let heard = user;
    let amber = match easing {
        // With Animations off the amber holds until the sound is back,
        // then turns plain at once: the colour doesn't move, and it never
        // says "back" early (OpenCode job 30).
        Some(e) if state.animations => theme::blend(theme::readable_on_floors(theme::lamp()), theme::text_primary(), e.progress),
        Some(_) => theme::readable_on_floors(theme::lamp()),
        None if dimmed => theme::readable_on_floors(theme::lamp()),
        None => theme::text_primary(),
    };
    // An override on this level (Dim on Main, Talkback on a Phones), said
    // under its name in a few words, the sentence on hover. Up here beside
    // the number, which stands taller than the two lines, its line is kept
    // even when empty, so nothing below moves when one comes or goes
    // (critique #38: Talkback's line pushed the snapshots and tabs down
    // 20 px, and overflowed the column at 125%). Where the number draws
    // small (`snug`, `head_tight`) the line shows only when there's a note.
    let talking = state.control_room.talkback && control_room::talkback_pairs(state).contains(&pair);
    let note: Option<(String, String)> = if dimmed && state.control_room.dim_db > -0.5 {
        // Its amount at 0 dB: on, but nothing is quieter (OpenCode's job 54).
        Some(("Dim is on".to_string(), "Dim is on, at 0 dB: nothing is quieter. Its amount is in Audra settings \u{2192} Monitor.".to_string()))
    } else if dimmed {
        Some((
            format!("Dim: {:.0} dB quieter", -state.control_room.dim_db),
            format!(
                "Dim is on: Main plays {:.0} dB quieter than your level, {} dB. Press Dim to bring it back.",
                -state.control_room.dim_db,
                db_number(user)
            ),
        ))
    } else if easing.is_some() {
        Some(("Dim: coming back up".to_string(), "Dim is off: Main is gliding back up to your level.".to_string()))
    } else if talking {
        Some((control_room::talkback_short(state), control_room::talkback_words(state)))
    } else if state.control_room.ms_check.is_some_and(|(p, _)| p == pair) {
        // The Control Room's Mid/Side check leaves its mark here as Dim
        // does: once its notice had gone, a lit key in a grid of ten was
        // the only sign (critique #92 A P3-3).
        Some(("Mid/Side check".to_string(), "This output plays the middle on the left and the sides on the right, until you press Mid/Side again.".to_string()))
    } else {
        None
    };
    // Both at once: Dim's note with the Mid/Side check after it, the
    // stranger of the two states (critique #93 A P3-5: Dim alone showed).
    let ms_on = state.control_room.ms_check.is_some_and(|(p, _)| p == pair);
    let note = match note {
        Some((short, sentence)) if ms_on && short.starts_with("Dim") => Some((
            format!("{short} \u{00b7} Mid/Side"),
            format!("{sentence} Mid/Side is on too: the middle plays on the left and the sides on the right."),
        )),
        other => other,
    };
    let small_number = snug(state) || crate::app::head_tight(state);
    // The placard's name for it ("Main 1/2", "Phones 3/4"): one output,
    // one name everywhere.
    let name_text = text(mix_name(state, pair)).font(theme::SANS_SEMI).size(theme::TEXT_MD * scale);
    let mut name_col = column![];
    match note {
        // Where the number draws small, the note goes on the name's own
        // line, one word, so it never adds height and no key below moves
        // (critique #39: at 1280x700 and 125% the Dim key slid from under
        // the pointer).
        Some((short, sentence)) if small_number => {
            let word = if (dimmed || easing.is_some()) && ms_on {
                "Dim \u{00b7} Mid/Side"
            } else if dimmed || easing.is_some() {
                "Dim"
            } else if short.starts_with("Mid/Side") {
                "Mid/Side"
            } else {
                "Talkback"
            };
            let tag = text(format!("\u{00b7} {word}"))
                .size(theme::TEXT_SM * scale)
                .color(theme::readable_on_floors(theme::lamp()))
                .wrapping(iced::advanced::text::Wrapping::None);
            name_col = name_col.push(
                row![name_text, crate::widgets::strip::hint_text(tag, sentence, scale)]
                    .spacing(theme::SPACE_SM * scale)
                    .align_y(iced::Alignment::Center),
            );
        }
        Some((short, sentence)) => {
            name_col = name_col.push(name_text);
            // The lamp, fading out as Dim's glide brings Main back.
            let line = text(short)
                .size(theme::TEXT_SM * scale)
                .color(if easing.is_some() { amber } else { theme::readable_on_floors(theme::lamp()) })
                .wrapping(iced::advanced::text::Wrapping::None);
            name_col = name_col.push(crate::widgets::strip::hint_text(line, sentence, scale));
        }
        None if !small_number => {
            name_col = name_col.push(name_text).push(text(" ").size(theme::TEXT_SM * scale));
        }
        None => {
            name_col = name_col.push(name_text);
        }
    }
    let head = row![
        container(name_col).width(Length::Fill).clip(true),
        tabular_level(
            &db_number(heard),
            if snug(state) {
                theme::TEXT_ROW
            } else if crate::app::head_tight(state) {
                theme::TEXT_XL
            } else {
                theme::TEXT_BIG
            } * scale,
            // The lamp's hue made readable (critique #27: raw, it read at
            // 1.6:1 on Daylight; the lamp is light, never words).
            theme::text_primary(),
        ),
        text("dB").size(theme::TEXT_SM * scale).color(theme::text_sec()),
    ]
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Center);
    // A fader on its side, in the strip faders' materials and manners
    // (`widgets::level_fader`). Letting go ends the undo step, as a fader
    // release does.
    let bar = crate::widgets::level_fader::level_fader(crate::widgets::level_fader::LevelFader {
        t: fader::vol_to_t(user),
        default_t: fader::vol_to_t(1.0),
        dimmed,
        scale,
        modifiers: state.modifiers,
        on_change: Box::new(move |t| Message::VolumeChanged(ChannelId::Output(ch), ch, fader::t_to_vol(t))),
        on_press: Box::new(move |t| Message::FaderPressed(ChannelId::Output(ch), ch, fader::t_to_vol(t), None)),
        on_reset: Message::Reset(ChannelId::Output(ch), ch, 1.0),
        on_release: Message::RangeCleared(ChannelId::Output(ch)),
    });
    // The keyboard's level (`keyfocus`): the slider ←/→ and +/− move.
    let bar = crate::keyfocus::ring(
        bar,
        crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Level(pair)),
        14.0 * scale,
    );
    let mut col = column![head, bar].spacing(theme::SPACE_SM * scale);
    // What this output plays, under its slider and on its taper: the
    // headphones' or speakers' level in view while you work on a row that
    // doesn't show the outputs. Nothing where the backend can't measure
    // it (a flat meter would claim silence).
    if crate::app::output_meter_available(state, ch) {
        let frame_of = |i: usize| state.output_meters.get(i).map(|m| m.frame());
        let right = (!state.device.outputs_one_per_pair()).then(|| frame_of(ch + 1)).flatten();
        if let Some(left) = frame_of(ch) {
            let meter = crate::widgets::fader::level_meter(left, right, scale);
            // Rung in the lamp while the past-0 dB card says to watch it
            // (`tricks::names_mix_meter`); the ring always there, clear
            // otherwise, so nothing moves when it comes.
            let named = pair == state.sel_out && crate::tricks::names_mix_meter(state);
            // Breathing as the strip's ring does (critique #59).
            let ring = Color { a: 1.0 - 0.45 * crate::motion::breath(), ..theme::readable_on_floors(theme::lamp()) };
            col = col.push(
                container(meter)
                    .padding(2.0 * scale)
                    .style(move |_t: &iced::Theme| container::Style {
                        // 2.5px: at 1.5 it read as a hairline (critique #59).
                        border: iced::Border { color: if named { ring } else { Color::TRANSPARENT }, width: 2.5 * scale, radius: (3.0 * scale).into() },
                        ..container::Style::default()
                    }),
            );
        }
    }
    col.width(Length::Fill).into()
}

/// Undo/redo on the left, the global M/S/F masters and the fold toggle
/// on the right.
fn tools_row(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    row![
        undo_redo_row(state, scale),
        iced::widget::Space::new().width(Length::Fill),
        msf_row(state, scale),
        panel_toggle(true, scale),
    ]
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Center)
    .width(Length::Fill)
    .into()
}

/// The column's pages under header bars (2026-11-03, the owner's
/// reference: TotalMix stacks "Snapshots", "Groups", "Layout Presets"
/// and "View Options" as bars, each opening its page under it). One page
/// is open at a time, as the tabs were, so the column stays short.
fn pages(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut col = column![].spacing(theme::SPACE_SM * scale).width(Length::Fill);
    for (label, t) in [
        ("Snapshots", Tab::Snapshots),
        ("Reverb & Echo", Tab::Fx),
        ("Groups", Tab::Groups),
        ("Layouts", Tab::Layouts),
        ("View", Tab::View),
    ] {
        let open = state.sidebar_tab == Some(t);
        // Reverb & Echo's bar carries a lit dot while either runs, so a
        // closed page still says the effects are on.
        let title: Element<'_, Message> = if t == Tab::Fx && state.fx.any_on() {
            row![
                crate::widgets::status_dot(theme::accent(), scale),
                text(label).font(theme::SANS_SEMI).size(theme::TEXT_NUM * scale),
            ]
            .spacing(theme::SPACE_MD * scale)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            text(label).font(theme::SANS_SEMI).size(theme::TEXT_NUM * scale).into()
        };
        let bar = button(
            iced::widget::stack![
                container(title)
                    .width(Length::Fill)
                    .center_x(Length::Fill),
                container(crate::widgets::strip_icons::floor_chevron(
                    open,
                    if open { theme::text_primary() } else { theme::text_sec() },
                    scale,
                ))
                .width(Length::Fill)
                .align_x(iced::Alignment::End),
            ],
        )
        .padding([theme::SPACE_MD * scale, theme::SPACE_MD * scale])
        .width(Length::Fill)
        .height(Length::Fixed(28.0 * scale))
        .style(theme::header_bar(open))
        .on_press(Message::SidebarTab(t));
        // A stop for the keyboard (`keyfocus::CounterItem::Page`).
        let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Page(t));
        col = col.push(crate::keyfocus::ring(bar.into(), focused, theme::RADIUS_SM * scale));
        // Where the pages float, they open as cards (`page_card_body`) and
        // this list isn't drawn (`snapshot_keys`, `page_keys`).
        if open {
            col = col.push(
                container(tab_body(state, t, scale))
                    .padding([theme::SPACE_SM * scale, theme::SPACE_TIGHT * scale])
                    .width(Length::Fill),
            );
        }
    }
    col.into()
}

/// Whether the pages open as cards beside the column rather than under
/// their bars: always now (below). Kept as a function so the column's
/// two layouts stay in one place should a height rule come back.
pub(crate) fn pages_float(_state: &TuxMix) -> bool {
    // At every height (the owner, 2026-11-07, from mockups: "chips
    // everywhere"): the snapshots as the unit's row of keys and the pages
    // as keys that open a card, as on a short window; a tall window keeps
    // the room. Until then a window 950px or taller opened the pages in
    // the column, the snapshots a named list (critique #15: two forms).
    true
}

/// The page shown as a card beside the column now, if any: one too tall
/// for the room under the head (`page_inline` takes the others).
pub(crate) fn page_card_open(state: &TuxMix) -> Option<Tab> {
    // It steps aside while a dialog that opens in its place is up (the
    // device panel, Workspaces, Audra settings), and comes back after.
    let dialog = state.show_settings || state.show_device_panel || state.show_workspaces;
    if pages_float(state) && state.sidebar_open && !dialog && state.page_card && page_inline(state).is_none() {
        state.sidebar_tab.filter(|&t| !(t == Tab::Snapshots && snapshots_listed(state)))
    } else {
        None
    }
}

/// Whether the snapshots stand in the head as their named list (issue #18,
/// the owner's yes): on a tall window the list takes the place of the 1–8
/// keys, as TotalMix's snapshot panel is a named list with Store, and the
/// Snapshots page no longer opens under the page keys as well (critiques
/// #19–#24: the same eight snapshots twice). It does wherever their page
/// would have stood open under the keys (the head, the zoom bar and the
/// page in the column's own pixels; the list in the head is a keys' row
/// shorter than keys and page together). A shorter window keeps the keys,
/// and the page opens from the section's title as a card.
pub(crate) fn snapshots_listed(state: &TuxMix) -> bool {
    if !pages_float(state) || !state.sidebar_open || crate::app::short_window(state) {
        return false;
    }
    let Some((_, h)) = state.window_size else {
        return false;
    };
    h / crate::app::sidebar_scale(state) - 650.0 - 36.0 >= page_height(Tab::Snapshots)
}

/// Where the pages float, the page opened by its key that fits in the
/// column's room under the head: it opens there, under the keys, rather
/// than over the mixer (critique #9, the owner's choice: at 1600x900 the
/// column stood ~250px empty while a card covered strips). The room is the
/// window's less the head and the zoom bar, without the foot slot, so a
/// card arriving never sends the page off to float.
pub(crate) fn page_inline(state: &TuxMix) -> Option<Tab> {
    // On a tall window the page last opened stays open under the keys
    // where it fits, without a press (the owner, 2026-11-07, from mockups:
    // the room under the keys stood empty); closing it clears it. A short
    // window opens it only when asked.
    let tall = !crate::app::short_window(state);
    if !pages_float(state) || !state.sidebar_open || !(state.page_card || tall) {
        return None;
    }
    let tab = state.sidebar_tab?;
    let listed = snapshots_listed(state);
    if tab == Tab::Snapshots && listed {
        return None;
    }
    let (_, h) = state.window_size?;
    let h = h / crate::app::sidebar_scale(state);
    // The head at scale 1 where the pages float, measured: tighter on a
    // short window; with the snapshots listed in it, their page's height
    // in place of the keys' row.
    let head = match (tall, listed) {
        (true, true) => 650.0 - 34.0 + page_height(Tab::Snapshots),
        (true, false) => 650.0,
        (false, _) => 590.0,
    };
    // What stands in the room for long: a guide under way (the transient
    // cards are left out, so their arrival doesn't send the page off), and
    // the open-mic warning opened to its answers.
    let guide = if state.guide.is_some() { crate::guides::card_h(state) } else { 0.0 };
    let warning = if state.open_mic_expanded && open_mic_shown(state).is_some() { 50.0 } else { 0.0 };
    (h - head - 36.0 - guide - warning >= page_height(tab)).then_some(tab)
}

/// About how tall a page stands in the column, at scale 1, its padding
/// included (measured).
fn page_height(tab: Tab) -> f32 {
    match tab {
        Tab::Snapshots => 195.0,
        Tab::Groups => 240.0,
        Tab::Layouts => 425.0,
        Tab::View => 570.0,
        Tab::Fx => 640.0,
    }
}

/// The page open where the pages float, as a card or under the keys.
pub(crate) fn page_open(state: &TuxMix) -> Option<Tab> {
    page_inline(state).or_else(|| page_card_open(state))
}

/// A page's name as its key and its card say it.
pub(crate) fn page_title(tab: Tab) -> &'static str {
    match tab {
        Tab::Snapshots => "Snapshots",
        Tab::Fx => "Reverb & Echo",
        Tab::Groups => "Groups",
        Tab::Layouts => "Layouts",
        Tab::View => "View",
    }
}

/// The page as a card beside the column: its name in the serif and the
/// cards' ×, then the page.
pub(crate) fn page_card_body(state: &TuxMix, tab: Tab) -> Element<'_, Message> {
    if tab == Tab::Fx {
        return crate::fx::card_body(state);
    }
    let scale = state.ui_scale;
    let close = hint(
        button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
            .padding(0)
            .width(Length::Fixed(20.0 * scale))
            .height(Length::Fixed(20.0 * scale))
            .style(theme::icon_button(false))
            .on_press(Message::FxCardClose),
        "Close (Esc)",
        scale,
    );
    column![
        row![
            text(page_title(tab)).font(theme::SERIF).size(theme::TEXT_ROW * scale).color(theme::text_primary()).width(Length::Fill),
            close,
        ]
        .align_y(iced::Alignment::Center),
        tab_body(state, tab, scale),
    ]
    .spacing(theme::SPACE_LG * scale)
    .into()
}

/// The snapshots as TotalMix's row of eight keys, then Store, in the head
/// where the pages float: a key's LED lit for the snapshot recalled,
/// glowing for one stored, its number dim while empty; its name and Alt
/// and its number in its tooltip. Armed, the keys sweep as the list's
/// rows do, Store becomes Cancel and a line says what to do.
/// The mark of a snapshot that switches Reverb and Echo on: a dot in a
/// ring, so it is its own shape beside the amber "changed" dot, and in the
/// key's own ink on a lit key (critique #22: a plain coral dot vanished on
/// a recalled key and was told apart by hue alone).
fn fx_mark<'a>(lit: bool, scale: f32) -> Element<'a, Message> {
    let d = 7.0 * scale;
    let (fill, ring) = if lit { (theme::on_active(), theme::on_active()) } else { (theme::accent(), theme::text_primary()) };
    container(iced::widget::Space::new())
        .width(d)
        .height(d)
        .style(move |_t: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(fill)),
            border: iced::Border { color: ring, width: 1.5 * scale, radius: (d / 2.0).into() },
            ..container::Style::default()
        })
        .into()
}

fn snapshot_keys(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    use crate::keyfocus::{CounterItem, KeyFocus};
    let now = std::time::Instant::now();
    let storing = state.snapshot_storing;
    let sweep = state.store_sweep;
    let mut keys = row![].spacing(theme::SPACE_TIGHT * scale).align_y(iced::Alignment::Center);
    for n in 1..=SNAPSHOT_COUNT {
        let i = (n - 1) as usize;
        let is_active = state.active_snapshot == Some(n as usize);
        let is_stored = state.stored_snapshots.get(i).copied().unwrap_or(false);
        let name = state.snapshot_names.get(i).cloned().flatten().unwrap_or_else(|| format!("Snapshot {n}"));
        // Recalled and unchanged: lit. Changed since: held, its number in
        // amber (TotalMix flashes the button; the list shows a dot).
        let lit = is_active && !state.snapshot_dirty;
        let ink = if lit {
            theme::on_active()
        } else if is_active {
            // On the held slot's own floor (critique #28).
            theme::readable_on(theme::attention(), theme::text_primary(), &theme::held_led_grounds())
        } else if is_stored {
            theme::text_primary()
        } else {
            // `text_sec`, not `muted`: on the light theme's empty slot
            // `muted` measured 3.4:1, under the 4.5:1 floor (the audit).
            theme::text_sec()
        };
        let face = container(text(n.to_string()).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI).color(ink).center())
            .width(Length::Fixed(22.0 * scale))
            .height(Length::Fixed(22.0 * scale))
            .center_x(Length::Fixed(22.0 * scale))
            .center_y(Length::Fixed(22.0 * scale))
            .style(theme::list_led(lit, is_stored || is_active));
        // A slot that brings Reverb and Echo in: a coral dot in its corner
        // (the owner's pick from mockups, 2026-11-10).
        let brings_fx = is_stored && state.snapshot_fx.get(i).copied().flatten().is_some_and(|(r, e)| r || e);
        // Just stored into or recalled: a lamp ring flares and fades round
        // it (`motion::slot_flare`).
        let flare = crate::motion::slot_flare(state, n);
        let face: Element<'_, Message> = if flare > 0.0 {
            let lamp = theme::readable_on_floors(theme::lamp());
            container(face)
                .style(move |_t: &iced::Theme| container::Style {
                    border: iced::Border { color: Color { a: flare, ..lamp }, width: 2.0 * scale, radius: (theme::RADIUS_XS * scale).into() },
                    shadow: iced::Shadow { color: Color { a: 0.45 * flare, ..lamp }, offset: iced::Vector::ZERO, blur_radius: 8.0 * scale * flare },
                    ..container::Style::default()
                })
                .into()
        } else {
            face.into()
        };
        let face: Element<'_, Message> = if brings_fx {
            // A short coral bar along the key's foot, under its number: a
            // ringed dot in the corner crowded the number on a 22 px key
            // (the owner, 2026-11-14: "looks strange and crambed").
            let hue = if lit { theme::on_active() } else { theme::readable_on_floors(theme::accent()) };
            iced::widget::stack![
                face,
                container(
                    container(iced::widget::Space::new())
                        .width(Length::Fixed(10.0 * scale))
                        .height(Length::Fixed(2.0 * scale))
                        .style(move |_t: &iced::Theme| container::Style {
                            background: Some(iced::Background::Color(hue)),
                            border: iced::Border { radius: (1.0 * scale).into(), ..iced::Border::default() },
                            ..container::Style::default()
                        }),
                )
                .width(Length::Fixed(22.0 * scale))
                .height(Length::Fixed(22.0 * scale))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding { bottom: 2.5 * scale, ..iced::Padding::ZERO }),
            ]
            .into()
        } else {
            face
        };
        let key = button(face)
            .padding(0)
            .style(move |t: &iced::Theme, st| {
                let target = storing.then(|| sweep.map_or(1.0, |at| crate::motion::sweep_level(at, i, now)));
                theme::list_row(is_active, target)(t, st)
            })
            .on_press(Message::SnapshotSlotPressed(n));
        let tip = match (storing, is_stored) {
            (true, true) => format!("Store here, over {name}"),
            (true, false) => format!("Store in {name}"),
            (false, true) if is_active && state.snapshot_dirty => format!("{name}, changed since (Alt+{n} recalls it)"),
            (false, true) if brings_fx => format!("Recall {name} (Alt+{n}): it switches Reverb and Echo on"),
            (false, true) => format!("Recall {name} (Alt+{n})"),
            (false, false) => format!("{name}: empty. Press Store, then this slot"),
        };
        let focused = matches!(state.key_focus, Some(KeyFocus::Counter(CounterItem::Slot(k))) if k == n);
        keys = keys.push(crate::widgets::strip::hint_text(crate::keyfocus::ring(key.into(), focused, theme::RADIUS_XS * scale), tip, scale));
    }
    let store_focused = crate::keyfocus::counter_focused(state, CounterItem::Store);
    let store_key = button(text(if storing { "Cancel" } else { "Store" }).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale))
        .padding([theme::SPACE_TIGHT * scale, theme::SPACE_LG * scale])
        .on_press(Message::SnapshotStore);
    let store_key: Element<'_, Message> = if storing {
        store_key.style(theme::ghost_button).into()
    } else {
        store_key.style(theme::page_key(11.0 * scale)).into()
    };
    let store: Element<'_, Message> = crate::keyfocus::ring(
        store_key,
        store_focused,
        11.0 * scale,
    );
    let line = row![keys, iced::widget::Space::new().width(Length::Fill), store].align_y(iced::Alignment::Center).width(Length::Fill);
    if storing {
        column![
            line,
            text("Click a slot (or Alt and its number) to keep this mix there. A stored one is replaced; Undo brings it back.")
                .size(theme::TEXT_SM * scale)
                .color(theme::readable_on_floors(theme::accent())),
        ]
        .spacing(theme::SPACE_SM * scale)
        .into()
    } else {
        line.into()
    }
}

/// The snapshots where the pages float, framed as the Control Room is
/// (critique #9: three unframed rows under it read as a drawer): its title
/// opens the Snapshots page (the names, the files, Workspaces), beside it
/// the snapshot recalled, by name, and whether the mix has changed since
/// (names were hover-only); under them the eight keys and Store.
fn snapshot_section(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    use crate::keyfocus::CounterItem;
    let pad = if crate::app::head_tight(state) { theme::SPACE_MD } else { theme::SPACE_LG } * scale;
    if snapshots_listed(state) {
        // The named list in place of the keys (issue #18): the title, the
        // snapshot recalled, then the list, Store, the files, Workspaces.
        let title = text("Snapshots").font(theme::SANS_SEMI).size(theme::TEXT_MD * scale).color(theme::text_primary());
        return container(
            column![
                row![title, iced::widget::Space::new().width(Length::Fill), recalled_line(state, scale)].align_y(iced::Alignment::Center),
                snapshots_panel(state, scale),
            ]
            .spacing(theme::SPACE_SM * scale),
        )
        .padding(pad)
        .width(Length::Fill)
        .style(theme::section)
        .into();
    }
    let open = page_open(state) == Some(Tab::Snapshots);
    let title = button(
        row![
            text("Snapshots").font(theme::SANS_SEMI).size(theme::TEXT_MD * scale).color(if open { theme::text_primary() } else { theme::text_sec() }),
            text(if open { "\u{25be}" } else { "\u{203a}" }).size(theme::TEXT_SM * scale).color(theme::text_sec()),
        ]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center),
    )
    .padding([0.0, theme::SPACE_TIGHT * scale])
    .style(theme::plain_button)
    .on_press(Message::SidebarTab(Tab::Snapshots));
    let title = crate::keyfocus::ring(title.into(), crate::keyfocus::counter_focused(state, CounterItem::Page(Tab::Snapshots)), theme::RADIUS_SM * scale);
    container(
        column![
            row![title, iced::widget::Space::new().width(Length::Fill), recalled_line(state, scale)].align_y(iced::Alignment::Center),
            snapshot_keys(state, scale),
        ]
        .spacing(theme::SPACE_SM * scale),
    )
    .padding(pad)
    .width(Length::Fill)
    .style(theme::section)
    .into()
}

/// The snapshot recalled, by name, and "changed" in amber once the mix
/// has moved since; nothing while none is.
fn recalled_line(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    match state.active_snapshot {
        Some(n) => {
            let name = state.snapshot_names.get(n - 1).cloned().flatten().unwrap_or_else(|| format!("Snapshot {n}"));
            let mut line = row![text(name).size(theme::TEXT_SM * scale).color(theme::text_primary())].spacing(theme::SPACE_SM * scale);
            if state.snapshot_dirty {
                line = line.push(text("changed").size(theme::TEXT_SM * scale).color(theme::readable_on_floors(theme::attention())));
            }
            line.align_y(iced::Alignment::Center).into()
        }
        None => iced::widget::Space::new().into(),
    }
}

/// The pages' keys where they float, in the console's key material, lit
/// while their page is open (critique #9: a rimmed chip's open state read
/// weaker than the keyboard's ring); each opens its page under the keys
/// where it fits, else as a card beside the column, and its second press
/// closes it. Reverb & Echo's key carries the lit dot while either effect
/// runs. Snapshots' page opens from its section's title.
fn page_keys(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    // Across the column, each key as wide as its words want and the row as
    // wide as the panel above it, a step of air between (critique #56: the
    // four were packed 2 px apart and stopped short of the column's edge).
    // A narrow column wraps them as before.
    let wide = state.sidebar_width >= 260.0;
    let mut keys = row![].spacing(theme::SPACE_SM * scale);
    for t in [Tab::Fx, Tab::Groups, Tab::Layouts, Tab::View] {
        let open = page_open(state) == Some(t);
        let title = page_title(t);
        let label = text(title).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI).wrapping(iced::widget::text::Wrapping::None);
        let content: Element<'_, Message> = if t == Tab::Fx && state.fx.any_on() {
            row![crate::widgets::status_dot(theme::accent(), scale), label].spacing(theme::SPACE_SM * scale).align_y(iced::Alignment::Center).into()
        } else {
            label.into()
        };
        let key = button(container(content).center_x(Length::Fill))
            .padding([theme::SPACE_SM * scale, theme::SPACE_SM * scale])
            .width(if wide { Length::FillPortion(title.chars().count() as u16 + 4 + if t == Tab::Fx { 2 } else { 0 }) } else { Length::Shrink })
            // Spread across the column, their inner sides fall between
            // pixels: a rim that reads wherever it lands (critique #57:
            // 1.86-2.25 on Daylight).
            .style(theme::split_safe_key(theme::key_button(open, 0.0, 0.0, theme::RADIUS_SM * scale), open))
            .on_press(Message::SidebarTab(t));
        let focused = crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Page(t));
        keys = keys.push(crate::keyfocus::ring(key.into(), focused, theme::RADIUS_SM * scale));
    }
    if wide {
        keys.width(Length::Fill).into()
    } else {
        keys.wrap().vertical_spacing(theme::SPACE_SM * scale).into()
    }
}

/// The open page of the tabbed area.
fn tab_body(state: &TuxMix, tab: Tab, scale: f32) -> Element<'_, Message> {
    match tab {
        Tab::Snapshots => snapshots_panel(state, scale),
        Tab::Fx => crate::fx::page(state, scale),
        Tab::Groups => groups_panel(state, scale),
        Tab::Layouts => layout_panel(state, scale),
        Tab::View => options_panel(state, scale),
    }
}

/// The device, its state and its clock as one line in the top bar; a
/// click opens Device settings. It used to be the boxed card at the head
/// of this column.
/// The room the device chip needs for its name and state ("Babyface Pro
/// FS Simulated"): the top bar shows its version label only above it, so the
/// label always goes before the chip shortens (critique #75 A 1: between the
/// label's 160 px and this, the label stood beside "Simulated").
pub(crate) fn chip_named_room(state: &TuxMix, scale: f32) -> f32 {
    let (_, status_label) = device_state(state);
    let display_name = state.device.model_name().strip_suffix(" (mock)").unwrap_or(state.device.model_name());
    let chrome = 7.0 + 2.0 * theme::SPACE_SM + 2.0 * theme::SPACE_MD + 4.0;
    (display_name.chars().count() as f32 * 8.4 + theme::SPACE_SM + status_label.chars().count() as f32 * 6.8 + chrome) * scale
}

pub(crate) fn device_button(state: &TuxMix, scale: f32, room: f32) -> Element<'_, Message> {
    let (status_color, status_label) = device_state(state);
    // `model_name()` carries " (mock)" in the simulation, which the state
    // word already says.
    let display_name = state
        .device
        .model_name()
        .strip_suffix(" (mock)")
        .unwrap_or(state.device.model_name());
    let settings = state.device.settings();
    // The chip steps down to fit the top bar rather than being cut off
    // (at 150% zoom the clock line ran past the window): the full line,
    // then the name with the state word, then the state and clock without
    // the name, then the state word alone. The name goes first: the state
    // ("Simulated", "Disconnected") is what tells you whether anything
    // reaches the unit (critique #38: at 125% the chip kept the name, which
    // spilled past its rim, and dropped "Simulated"). The device panel it
    // opens has every detail.
    let full = format!("{status_label} \u{00b7} {}", clock_line(state, &settings.clock_source));
    // A pitch off zero says so on the chip, in the attention hue: it moves
    // everything the unit plays (critique #20: it left no trace). Only
    // where it runs (the internal clock), and counted in what fits.
    let pitch = settings.pitch_percent;
    let mut pitch_line = (pitch.abs() >= 0.05 && state.device.protocol_features() && crate::app::clock_is_internal(state))
        .then(|| format!("Pitch {pitch:+.1}%"));
    let pitch_w = pitch_line.as_ref().map_or(0.0, |p| (p.chars().count() as f32 * 6.6 + theme::SPACE_SM) * scale);
    // Semi-bold capitals run wider than 7.6 px a letter: the name spilled.
    let width = |with_name: bool, line: &str| {
        let chrome = 7.0 + 2.0 * theme::SPACE_SM + 2.0 * theme::SPACE_MD + 4.0;
        let name_w = if with_name { display_name.chars().count() as f32 * 8.4 + theme::SPACE_SM } else { 0.0 };
        (name_w + line.chars().count() as f32 * 6.8 + chrome) * scale + pitch_w
    };
    let (show_name, line) = if width(true, &full) <= room {
        (true, full)
    } else if width(true, status_label) <= room {
        (true, status_label.to_string())
    } else if width(false, &full) <= room {
        (false, full)
    } else if width(false, status_label) <= room {
        (false, status_label.to_string())
    } else if width(false, chip_short(status_label)) <= room {
        // A word, so the dot in its ring isn't read as a record key a
        // few keys from Rec (critique #63 A 2): the state, short (critique
        // #64 A 3: "Unit" named hardware the simulation hasn't got); the
        // whole state in the hover.
        (false, chip_short(status_label).to_string())
    } else {
        // No room even for its one word (critique #58 B: at 900 px wide
        // "Simulated" ran past the pill into the rail): the dot alone, the
        // words in its hover.
        (false, String::new())
    };
    // The state isn't on the chip (dot alone, or the short word): the
    // hover says it.
    let line_hidden = !show_name && (line.is_empty() || line == chip_short(status_label));
    // The dot alone: a status light, its ring only under the pointer
    // (critique #63 A 2: a dot in a firm ring read as a record key).
    let dot_only = !show_name && line.is_empty();
    let mut content = row![crate::widgets::status_dot(status_color, scale)]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center);
    if show_name {
        content = content.push(
            text(display_name)
                .font(theme::SANS_SEMI)
                .size(theme::TEXT_SM * scale)
                .wrapping(iced::advanced::text::Wrapping::None),
        );
    }
    if !line.is_empty() {
        content = content.push(
            text(line)
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec())
                .wrapping(iced::advanced::text::Wrapping::None),
        );
    }
    // The dot alone carries no pitch line beside it (critique #64 A 2):
    // the hover says it.
    let pitch_in_hover = if dot_only { pitch_line.take() } else { None };
    if let Some(p) = pitch_line {
        content = content.push(
            text(p)
                .size(theme::TEXT_SM * scale)
                // On the chip's `bg_deep` (critique #27: 3.9:1 on Daylight).
                .color(theme::readable_on(theme::attention(), theme::text_primary(), &[theme::bg_deep()]))
                .wrapping(iced::advanced::text::Wrapping::None),
        );
    }
    let chip = button(content)
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        // The dot alone: a faint ring in its own colour, so it reads as the
        // device's light, not the Help key's badge (critique #64 A 2).
        .style(theme::chip_button_lit(state.show_device_panel, if dot_only { Color { a: 0.4, ..status_color } } else { theme::chip_edge() }))
        .on_press(Message::ToggleDevicePanel);
    if line_hidden {
        // The dot alone: its words in the hover.
        return crate::widgets::strip::hint_text(
            chip,
            format!(
                "{}{}. Device settings: clock source, sample rate, pitch and the Global switches",
                // "Sim" could read as a SIM card (critique #65 A 5): said whole.
                if status_label == "Simulated" { "Simulated: no Babyface connected".to_string() } else { format!("{display_name}: {status_label}") },
                pitch_in_hover.map(|p| format!(", {p}")).unwrap_or_default()
            ),
            scale,
        );
    }
    hint(chip, "Device settings: clock source, sample rate, pitch and the Global switches", scale)
}

/// The device chip's last word before the dot alone: its state, short.
fn chip_short(status_label: &str) -> &'static str {
    match status_label {
        "Disconnected" => "Off",
        "Waiting" => "Wait",
        "Demo" => "Demo",
        "Simulated" => "Sim",
        "Class Compliant" => "CC",
        _ => "On",
    }
}

/// The zoom control, fixed at the foot of the column: `-  100%  +`.
/// The zoom was Ctrl+wheel / Ctrl+= only, which nothing on screen
/// mentioned. The percentage resets to 100 %. (A fit-to-window mode lived
/// next to it for a day and was removed: see GUI-NOTES.md.)
fn zoom_bar(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let pct = format!("{:.0}%", state.ui_scale * 100.0);
    let cell = |label: &'static str| {
        // In the ink: the thin "−" read 2.4 against "+" 4.9 (critique #107 A).
        // Semi-bold: the thin "−" drew half-covered rows (critique #108 B: 3.06).
        button(text(label).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI).color(theme::text_primary()).center())
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
            .width(Length::Fixed(26.0 * scale))
    };
    container(
        row![
            hint(
                cell("\u{2212}")
                    .style(theme::ghost_button)
                    .on_press(Message::ZoomOut),
                "Zoom out (Ctrl -)",
                scale,
            ),
            hint(
                // In the ink, not the quiet tone: under a notice's or a
                // card's shadow it came to 4.45:1 (critique #42).
                button(text(pct).size(theme::TEXT_SM * scale).color(theme::text_primary()).center())
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .width(Length::Fill)
                    .style(theme::ghost_button)
                    .on_press(Message::ZoomReset),
                "Reset to 100% (Ctrl 0)",
                scale,
            ),
            hint(
                cell("+")
                    .style(theme::ghost_button)
                    .on_press(Message::ZoomIn),
                "Zoom in (Ctrl +)",
                scale,
            ),
        ]
        .spacing(theme::SPACE_TIGHT * scale)
        .align_y(iced::Alignment::Center),
    )
    .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
    .width(Length::Fill)
    .into()
}

/// The resize grip on the column's inner edge — a `splitter` that reports
/// horizontal drag deltas as `Message::SidebarResize`. Only ever reached
/// with the column open (the collapsed rail has a fixed width), and it
/// needs no overlay of its own: it sits in the column's own left padding.
fn resize_strip(scale: f32) -> Element<'static, Message> {
    crate::widgets::splitter::splitter(Message::SidebarResize, scale, RESIZE_W * scale)
}

/// The device chip's own state word and color, in priority order: a
/// disconnected device outranks the personality it had (the green/amber
/// words describe a card that is actually there), and the mock's
/// "Simulated" would otherwise mask a mock-side disconnect. Pure, so the
/// mapping is pinned by a test (`app`'s own, where states are easy to
/// build).
pub(crate) fn device_state(state: &TuxMix) -> (Color, &'static str) {
    if !state.connected {
        (theme::clip(), "Disconnected")
    } else if state.no_device == Some(crate::app::NoDevice::Waiting) {
        (theme::attention(), "Waiting")
    } else if state.no_device.is_some() {
        (theme::attention(), "Demo")
    } else if state.device.is_mock() {
        (theme::attention(), "Simulated")
    } else if !state.device.protocol_features() {
        (theme::attention(), "Class Compliant")
    } else {
        (theme::signal(), "Proprietary")
    }
}

/// The device card's third line: what the card is running at. While the card
/// plays, its real rate and period ("48 kHz · 256 smp · Internal"); idle, the
/// sound server's rate; the USB backend sets the rate itself. With the kernel
/// driver and no PipeWire there is no rate to report truthfully (the model's
/// own field is a constant), so only the clock source shows.
pub(crate) fn clock_line(state: &TuxMix, clock_source: &str) -> String {
    use crate::app::rate_label;
    // The card's own playback stream wins while it plays: its period is the
    // card's real buffer. With nothing playing there is no real period to
    // show (the graph's quantum is not it: the card opens at what its
    // clients ask, 256 by default), so the line gives the rate only.
    if let Some(card) = state.card_stream {
        return format!(
            "{} \u{00b7} {} smp \u{00b7} {clock_source}",
            rate_label(card.rate),
            card.period
        );
    }
    match state.engine {
        Some(e) => format!("{} \u{00b7} {clock_source}", rate_label(e.effective_rate())),
        None if state.device.rate_follows_sound_server() => clock_source.to_string(),
        None => format!(
            "{} \u{00b7} {clock_source}",
            rate_label(state.device.settings().sample_rate)
        ),
    }
}

/// Real: pops/pushes `undo_stack`/`redo_stack` (see `Message::Undo`'s
/// own doc comment for the capture granularity). Disabled (no
/// `on_press`) when the relevant stack is empty, via `on_press_maybe`.
/// Flat, and the only two rows here with nothing else to say: an empty
/// stack now reads as "nothing to do" instead of as two disabled boxes.
/// Keys, as the notice's Undo is (critique #42: as bare words they read as
/// labels once the notice had gone). While a notice's Undo key shows, this
/// one's tooltip says what it puts back too.
fn undo_redo_row(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let undo_tip = crate::app::notice_undo_tip(state).or_else(|| crate::app::kept_undo_tip(state)).unwrap_or("Undo (Ctrl+Z)");
    row![
        hint(
            button(text("Undo").size(theme::TEXT_SM * scale).wrapping(iced::advanced::text::Wrapping::None))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_LG * scale])
                .width(Length::Shrink)
                .style(theme::column_key(11.0 * scale))
                .on_press_maybe((!state.undo_stack.is_empty()).then_some(Message::Undo)),
            undo_tip,
            scale,
        ),
        hint(
            button(text("Redo").size(theme::TEXT_SM * scale).wrapping(iced::advanced::text::Wrapping::None))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_LG * scale])
                .width(Length::Shrink)
                .style(theme::column_key(11.0 * scale))
                .on_press_maybe((!state.redo_stack.is_empty()).then_some(Message::Redo)),
            "Redo (Ctrl+Shift+Z)",
            scale,
        ),
    ]
    .spacing(theme::SPACE_SM * scale)
    // As wide as its keys: filling, it split the row's spare room with the
    // spacer beside it and squeezed Redo into a circle (critique #43).
    .width(Length::Shrink)
    .into()
}

/// The global row, TotalMix's Mute/Solo/Fader masters plus the hardware
/// Dim. All four report real state, so all keep `toggle_button`'s full
/// fill when lit.
///
/// * `M` lights while any channel is muted; a press clears those mutes and
///   the next one brings them back (`Message::GlobalMuteToggle`). In
///   between, it and the channels it holds show as armed (dimmed outline,
///   `theme::armed_toggle_button`), as TotalMix keeps them marked.
/// * `S` is the same for solos (`Message::GlobalSoloToggle`).
/// * `F` is TotalMix's temporary fader group, which here is the strip
///   multi-selection (Ctrl/Shift-click): its faders already move together
///   relatively. Lit while a selection exists, a press clears it; with
///   nothing selected it is disabled, since there is nothing to clear.
/// * `DIM` flips `RmeDevice::set_dim`, the switch the front-panel DIM
///   button toggles, which had no on-screen control at all. RME-protocol
///   only, so it is absent in Class Compliant mode.
fn msf_row(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let any_mute = crate::app::any_channel_muted(state);
    let any_solo = crate::app::any_channel_soloed(state);
    // Armed: the master switched its mutes (solos) off and a press brings
    // them back; the channels it holds show the same dimmed state.
    let mute_held = !any_mute && !state.last_cleared_mutes.is_empty();
    let solo_held = !any_solo && !state.last_cleared_solos.is_empty();
    // F, TotalMix's Fader master: lit while anything moves together (a
    // Fader group switched on under Groups, or two or more strips picked;
    // one picked strip moves nothing with it), armed while it holds groups
    // it let go.
    let fader_live = state.groups.iter().any(|g| g.fader_linked) || state.selected.len() > 1;
    let fader_held = !fader_live && !state.last_released_fader_groups.is_empty();
    // With nothing to act on, a master rests unlit, and a press still
    // answers in the notice with what it would do, as its tip does
    // (2026-11-06, the owner: they "dont appear to do anything or suggest
    // anything to me why they dont work").
    let cell = |label: &'static str| {
        button(text(label).size(theme::TEXT_SM * scale).center())
            .padding(theme::SPACE_SM * scale)
            .width(Length::Fixed(28.0 * scale))
    };
    let r = row![
        hint(
            cell("M")
                .style(theme::lamp_button(
                    theme::mute_color(),
                    theme::Lamp { active: any_mute, armed: mute_held, hue_label: true, ..theme::Lamp::default() },
                ))
                .on_press(Message::GlobalMuteToggle),
            if mute_held {
                "Mute master: M brings the mutes it switched off back"
            } else if any_mute {
                "Mute master: lit while strips are muted; M unmutes them all at once (and M again mutes them back)"
            } else {
                "Mute master: nothing is muted. Once strips are muted, M unmutes them all at once, and M again mutes them back"
            },
            scale,
        ),
        hint(
            cell("S")
                .style(theme::lamp_button(
                    theme::solo_color(),
                    theme::Lamp { active: any_solo, armed: solo_held, hue_label: true, ..theme::Lamp::default() },
                ))
                .on_press(Message::GlobalSoloToggle),
            if solo_held {
                "Solo master: S brings the solos it switched off back"
            } else if any_solo {
                "Solo master: lit while strips are soloed; S unsolos them all at once (and S again solos them back)"
            } else {
                "Solo master: nothing is soloed. Once strips are soloed, S unsolos them all at once, and S again solos them back"
            },
            scale,
        ),
        hint(
            cell("F")
                .style(theme::lamp_button(
                    theme::accent(),
                    theme::Lamp { active: fader_live, armed: fader_held, hue_label: true, ..theme::Lamp::default() },
                ))
                .on_press(Message::FaderMaster),
            if fader_held {
                "Fader master: F links the Fader groups it let go back"
            } else if fader_live {
                "Fader master: lit while strips move together (a Fader group, or strips picked with Ctrl or Shift); F lets them all go"
            } else {
                "Fader master: nothing moves together. Switch on a Fader group (Groups) or Ctrl- or Shift-click strips, and F lets them all go at once"
            },
            scale,
        ),
    ]
    .spacing(theme::SPACE_TIGHT * scale);
    r.into()
}

/// A short tooltip under a sidebar control, in the same chip as the panel
/// toggle's.
fn hint<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    scale: f32,
) -> Element<'a, Message> {
    hint_at(content, label, scale, iced::widget::tooltip::Position::Top)
}

/// A tooltip on the counter, the same panel, delay and gap as a strip's
/// (`strip::hint`), at `position`. It used to sit below its control,
/// at once and in a chip style of its own, so the placard's tooltip
/// covered the first row of the mix menu that opens below it.
fn hint_at<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl iced::widget::text::IntoFragment<'a>,
    scale: f32,
    position: iced::widget::tooltip::Position,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        content,
        container(text(label).size(theme::TEXT_SM * scale).color(theme::text_primary()))
            .padding(theme::SPACE_SM * scale)
            .style(crate::widgets::hint_panel),
        position,
    )
    .gap(4.0 * scale)
    .delay(crate::widgets::hint_delay())
    .into()
}

/// The caption cell of an Options row — dimmer than the control labels it
/// captions and fixed-width, so the four rows' controls line up in a
/// column of their own instead of every caption eating a full line of the
/// panel's width above its row.
fn sub_label(label: &str, scale: f32) -> Element<'_, Message> {
    text(label)
        .size(theme::TEXT_SM * scale)
        .color(theme::text_sec())
        .width(Length::Fixed(OPTIONS_LABEL_W * scale))
        .into()
}

/// One Options row: caption on the left, control filling the rest.
/// `None` for the row that continues the caption above it (the `2 row` /
/// `2 row in` half of the `show` pair): the empty cell keeps its control
/// aligned with the row above rather than sliding left under the caption.
/// `options_row` with what it does under it, always shown, in from the
/// captions (critique #27: the View page's rows said so only on hover).
fn options_row_note<'a>(caption: &'a str, control: Element<'a, Message>, note: &'a str, scale: f32) -> Element<'a, Message> {
    column![
        options_row(Some(caption), control, scale),
        container(text(note).size(theme::TEXT_SM * scale).color(theme::muted()).width(Length::Fill)).padding(iced::Padding {
            left: (OPTIONS_LABEL_W + theme::SPACE_SM) * scale,
            ..iced::Padding::ZERO
        }),
    ]
    .spacing(theme::SPACE_HAIRLINE * scale)
    .width(Length::Fill)
    .into()
}

fn options_row<'a>(
    caption: Option<&'a str>,
    control: Element<'a, Message>,
    scale: f32,
) -> Element<'a, Message> {
    let caption: Element<'a, Message> = match caption {
        Some(label) => sub_label(label, scale),
        None => iced::widget::Space::new()
            .width(Length::Fixed(OPTIONS_LABEL_W * scale))
            .into(),
    };
    row![caption, control]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .into()
}

/// A 2-way segmented switch: one side lit, a press on either side
/// sends that side's message (`None` leaves a side inert).
fn segmented_row<'a>(
    left_label: &'a str,
    right_label: &'a str,
    left_active: bool,
    on_left: Option<Message>,
    on_right: Option<Message>,
    scale: f32,
) -> Element<'a, Message> {
    segment_track(
        button(text(left_label).size(theme::TEXT_SM * scale).center())
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
            .width(Length::Fill)
            .style(theme::segment_cell(left_active))
            .on_press_maybe(on_left)
            .into(),
        button(text(right_label).size(theme::TEXT_SM * scale).center())
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
            .width(Length::Fill)
            .style(theme::segment_cell(!left_active))
            .on_press_maybe(on_right)
            .into(),
        scale,
    )
}

/// An N-way segmented switch in the shared track: `(label, lit, message)`
/// per cell, `None` leaving a cell inert.
fn segmented<'a>(cells: &[(&'a str, bool, Option<Message>)], scale: f32) -> Element<'a, Message> {
    let mut r = row![].spacing(theme::SPACE_HAIRLINE * scale);
    for (label, lit, msg) in cells {
        r = r.push(
            button(text(*label).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                .width(Length::Fill)
                .style(theme::segment_cell(*lit))
                .on_press_maybe(msg.clone()),
        );
    }
    container(r)
        .padding(theme::SPACE_TIGHT * scale)
        .width(Length::Fill)
        .style(theme::segment_track)
        .into()
}

/// Wraps two switch cells in the shared `theme::segment_track`.
fn segment_track<'a>(
    left: Element<'a, Message>,
    right: Element<'a, Message>,
    scale: f32,
) -> Element<'a, Message> {
    container(row![left, right].spacing(theme::SPACE_HAIRLINE * scale))
        .padding(theme::SPACE_TIGHT * scale)
        .width(Length::Fill)
        .style(theme::segment_track)
        .into()
}

/// The snapshot slots' names and renaming (manual 21.5.2; a right-click
/// on a slot names it here, as a double-click would recall it first).
/// Layouts have none.
pub(crate) struct SlotNames<'a> {
    pub names: &'a [Option<String>],
    /// Whether each slot switches Reverb and Echo on when recalled
    /// (`TuxMix::snapshot_fx`): a coral dot by its name.
    pub fx: &'a [Option<(bool, bool)>],
    /// The slot being renamed (1-based) and the name typed so far.
    pub renaming: Option<(usize, &'a str)>,
    pub on_rename: fn(u8) -> Message,
}

/// The text field a slot turns into while it is renamed.
pub(crate) const SLOT_RENAME_ID: &str = "snapshot-rename";

/// A slot's name as it fits its tile: long names end in "…" (the
/// tooltip and the Store button carry the whole name).
fn slot_label(title: &str) -> String {
    const MAX: usize = 14;
    if title.chars().count() <= MAX {
        title.to_string()
    } else {
        let cut: String = title.chars().take(MAX - 1).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Numbered slots (Snapshots, Mixer Layouts) as a two-column grid of
/// equal cells, like the slot buttons on a hardware console, instead of a
/// list of full-width rows: eight rows of "Mix n" were the tallest thing
/// in the column for the least information. The selected slot is a switch
/// position, not an "on" state: a filled tile with a coral edge
/// (`theme::slot_button`). After Store (`storing`) every slot is a target
/// with a coral edge (`theme::slot_target_button`), and a press stores.
#[allow(clippy::too_many_arguments)]
fn slot_grid<'a>(
    count: u8,
    prefix: &'a str,
    active: Option<usize>,
    dirty: bool,
    stored: &[bool],
    storing: bool,
    sweep: Option<std::time::Instant>,
    // The slot just stored into or recalled and how bright its flare is
    // (`motion::slot_flare`; Snapshots only).
    landed: Option<(u8, f32)>,
    on_press: fn(u8) -> Message,
    names: Option<SlotNames<'a>>,
    // The slot the keyboard is on (`keyfocus`), 1-based.
    focused: Option<u8>,
    scale: f32,
) -> Element<'a, Message> {
    let now = std::time::Instant::now();
    let mut grid = column![].spacing(theme::SPACE_TIGHT * scale);
    for first in (1..=count).step_by(2) {
        let mut line = row![].spacing(theme::SPACE_TIGHT * scale);
        for n in first..=(first + 1).min(count) {
            let is_active = active == Some(n as usize);
            // An empty slot is still clickable (selecting it is how you
            // pick where "store" writes), but it reads as empty. Dimmer
            // than a stored slot, yet clearly above the 0.4 of a disabled
            // control, which it used to be mistaken for.
            let is_stored = stored.get((n - 1) as usize).copied().unwrap_or(false);
            // `muted`, the quiet text role every theme holds to 4.5:1, not
            // a faded `text_sec`, which no check covered (the audit,
            // 2026-11-05: 3.6:1 on Daylight's well). The lamp says
            // empty or stored too.
            let name_color = if is_stored || is_active {
                None
            } else {
                Some(theme::muted())
            };
            // The slot's own name when it has one ("Tracking"), else
            // "Mix 3".
            let title = names
                .as_ref()
                .and_then(|s| s.names.get((n - 1) as usize).cloned().flatten())
                .unwrap_or_else(|| format!("{prefix} {n}"));
            // Being renamed: the slot is a text field until Enter or Esc.
            if let Some((_, typed)) = names
                .as_ref()
                .and_then(|s| s.renaming)
                .filter(|(slot, _)| *slot == n as usize)
            {
                line = line.push(
                    iced::widget::text_input(&format!("{prefix} {n}"), typed)
                        .id(SLOT_RENAME_ID)
                        .on_input(Message::SnapshotRenameChanged)
                        .on_submit(Message::SnapshotRenameCommit)
                        .style(theme::text_input)
                        .size(theme::TEXT_SM * scale)
                        .width(Length::Fill),
                );
                continue;
            }
            // A slot that brings Reverb and Echo in wears a coral dot (the
            // owner's pick from mockups, 2026-11-10); the selected slot,
            // modified since it was recalled or stored, an amber one:
            // TotalMix flashes the button instead.
            let brings_fx = is_stored
                && names
                    .as_ref()
                    .and_then(|s| s.fx.get((n - 1) as usize).copied().flatten())
                    .is_some_and(|(r, e)| r || e);
            let mut label = row![text(slot_label(&title)).size(theme::TEXT_SM * scale).color_maybe(name_color)]
                .spacing(theme::SPACE_SM * scale)
                .align_y(iced::Alignment::Center);
            if brings_fx {
                label = label.push(fx_mark(false, scale));
            }
            if is_active && dirty {
                label = label.push(crate::widgets::status_dot(theme::attention(), scale));
            }
            let label: Element<'a, Message> = label.into();
            let fx_words = if brings_fx && !storing { ". It switches Reverb and Echo on" } else { "" };
            let tip = match (storing, is_stored, names.is_some()) {
                (true, true, _) => format!("Store here, over {title}"),
                (true, false, _) => format!("Store in {title}"),
                (false, true, true) => format!("Recall {title}{fx_words}. Right-click to name it"),
                (false, true, false) => format!("Recall {title}{fx_words}"),
                (false, false, true) => "Empty: press Store, then this slot. Right-click to name it".to_string(),
                (false, false, false) => "Empty: press Store, then this slot".to_string(),
            };
            // A list row (the reference's): a round lamp, lit for the
            // selected slot, glowing dimly for a stored one, and the name
            // (round since 2026-11-14: the square read as a checkbox).
            // Just stored into or recalled, the lamp strikes: a halo in
            // the lamp's colour that blooms and fades, as the numbered
            // keys' ring does (`motion::slot_flare`; the list had none).
            let flare = landed.filter(|&(m, _)| m == n).map_or(0.0, |(_, f)| f);
            let led = container(iced::widget::Space::new())
                .width(Length::Fixed(12.0 * scale))
                .height(Length::Fixed(12.0 * scale))
                .style(theme::list_lamp(is_active, is_stored, 12.0 * scale, flare));
            let slot = button(row![led, label].spacing(theme::SPACE_MD * scale).align_y(iced::Alignment::Center))
                .padding([theme::SPACE_SM * scale, theme::SPACE_SM * scale])
                .width(Length::Fill)
                .style(move |t: &iced::Theme, st| {
                    let target = storing.then(|| sweep.map_or(1.0, |at| crate::motion::sweep_level(at, (n - 1) as usize, now)));
                    theme::list_row(is_active, target)(t, st)
                })
                .on_press(on_press(n));
            // A button takes the left press only, so the right one reaches
            // this area: it names the slot.
            let slot: Element<'a, Message> = match names.as_ref() {
                Some(s) => iced::widget::mouse_area(slot).on_right_press((s.on_rename)(n)).into(),
                None => slot.into(),
            };
            let tipped: Element<'a, Message> = iced::widget::tooltip(
                slot,
                container(text(tip).size(theme::TEXT_SM * scale).color(theme::text_primary()))
                    .padding(theme::SPACE_SM * scale)
                    .style(crate::widgets::hint_panel),
                iced::widget::tooltip::Position::Top,
            )
            .gap(4.0 * scale)
            .delay(crate::widgets::hint_delay())
            .into();
            line = line.push(crate::keyfocus::ring(tipped, focused == Some(n), theme::RADIUS_XS * scale));
        }
        grid = grid.push(line);
    }
    // The list sits in a well, as the reference's does.
    container(grid.width(Length::Fill))
        .padding(theme::SPACE_TIGHT * scale)
        .width(Length::Fill)
        .style(theme::well_box(theme::RADIUS_SM))
        .into()
}

/// The Store action of the Snapshot and Mixer Layout panels, in
/// TotalMix's order (manual 21.5.2): press Store, then the slot. At rest a
/// flat "Store…" at the panel's right edge; while the slots wait, the
/// instruction and a Cancel (Esc too). Storing never touches the mix, so
/// the slot you store into is never recalled over your work first.
fn store_row(storing: bool, what: &'static str, on_press: Message, focused: bool, scale: f32) -> Element<'static, Message> {
    let control: Element<'static, Message> = if storing {
        row![
            // The keyboard's way too (critique #6's keyboard round).
            // For a mix, what happens to a stored slot too (critique #96 A
            // P2-1: the words were changed in the compact form only).
            text(if what == "the mix" {
                "Click a slot (or Alt and its number) to keep this mix there. A stored one is replaced; Undo brings it back.".to_string()
            } else {
                format!("Click a slot (or Alt and its number) to store {what}")
            })
                .size(theme::TEXT_SM * scale)
                .color(theme::readable_on_floors(theme::accent()))
                .width(Length::Fill),
            // Armed, the keyboard's ring moves to Cancel, which Store became.
            crate::keyfocus::ring(
                button(text("Cancel").size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    .style(theme::ghost_button)
                    .on_press(on_press)
                    .into(),
                focused,
                theme::RADIUS_SM * scale,
            ),
        ]
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        // No tooltip here: one on this right-aligned button stopped iced
        // drawing much of the window's text (2026-11-03, bisected; the
        // label and the store-mode line say what it does).
        container(crate::keyfocus::ring(
            button(text("Store\u{2026}").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_XL * scale])
                .style(theme::page_key(11.0 * scale))
                .on_press(on_press)
                .into(),
            focused,
            11.0 * scale,
        ))
        .width(Length::Fill)
        .align_x(iced::Alignment::End)
        .into()
    };
    container(control).width(Length::Fill).into()
}

/// TotalMix's Options panel, cut to what this app can actually do: the
/// mixer layout ("2 Row" in TotalMix, `Message::SetRowMode`), solo,
/// Trim Gains for all, the S button (PFL), the Routing Mode ("free", a
/// routing field per strip, 2026-11-03) and "names" (TotalMix's "Show
/// Names", `Message::SetShowOriginalNames`). The rest was dropped rather
/// than shown as dead switches: "meters: post FX" is moot without an FX
/// engine and "RMS" needs sample-level meter data the app doesn't get.
fn options_panel(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut body = column![]
        .spacing(theme::SPACE_SM * scale)
        .width(Length::Fill);

    use crate::app::RowMode;
    body = body.push(options_row_note(
        // "rows" as the caption keeps the three cells one word each: in
        // a 210 px column "3 rows" / "2 rows" wrapped onto two lines.
        "mixer rows",
        segmented(
            &[
                ("auto", RowMode::Auto),
                ("3", RowMode::Three),
                ("2", RowMode::Two),
                ("1", RowMode::One),
            ]
            .map(|(label, mode)| {
                (
                    label,
                    state.row_mode == mode,
                    Some(Message::SetRowMode(mode)),
                )
            }),
            scale,
        ),
        "How many rows of strips show; auto fits them to the window.",
        scale,
    ));

    // TotalMix's "Exclusive Solo" (manual p. 59): one solo at a time, or
    // several. Solo itself acts on the submix being edited only.
    body = body.push(options_row_note(
        "solo",
        hint(
            segmented_row(
                "several",
                "exclusive",
                !state.solo_exclusive,
                Some(Message::SetExclusiveSolo(false)),
                Some(Message::SetExclusiveSolo(true)),
                scale,
            ),
            "Exclusive: soloing a strip releases every other solo",
            scale,
        ),
        "Exclusive: soloing one strip lets go of the others.",
        scale,
    ));

    // TotalMix's "Trim" in the View Options (manual 21.5.1): Trim Gains on
    // for every strip, "like a conventional, simple mixing desk". Per strip
    // it is a toggle in the strip's Settings panel.
    body = body.push(options_row_note(
        "trim gains",
        hint(
            segmented_row(
                "per strip",
                "all",
                !state.trim_all,
                Some(Message::SetTrimAll(false)),
                Some(Message::SetTrimAll(true)),
                scale,
            ),
            "All: every fader moves its channel in every mix at once",
            scale,
        ),
        "All: each fader moves its channel in every mix at once.",
        scale,
    ));

    // TotalMix's "Live Mode" (manual p. 59): "PFL replaces Solo", pre-fader
    // listening on the output Cue plays on (Settings, Control Room).
    body = body.push(options_row_note(
        "S button",
        hint(
            segmented_row(
                "solo",
                "PFL",
                !state.control_room.live_mode,
                Some(Message::SetLiveMode(false)),
                Some(Message::SetLiveMode(true)),
                scale,
            ),
            "PFL: S plays the strip before its fader where Cue plays",
            scale,
        ),
        "PFL: S plays a strip where Cue plays (usually the Phones), whatever its fader.",
        scale,
    ));

    // TotalMix's Routing Mode (manual 21.5.1): "submix", every strip edits
    // the mix on the placard; "free", each input and playback strip has
    // its own routing field, to edit several mixes side by side.
    body = body.push(options_row_note(
        "routing",
        hint(
            segmented_row(
                "submix",
                "free",
                !state.free_routing,
                Some(Message::SetFreeRouting(false)),
                Some(Message::SetFreeRouting(true)),
                scale,
            ),
            "Submix: every strip edits the mix you picked. Free: each input and playback strip picks its own",
            scale,
        ),
        "Submix: every strip edits the mix on the placard. Free: each strip picks its own.",
        scale,
    ));

    // TotalMix's "Show Names": the hardware names, even where a strip was
    // renamed. With nothing renamed there is nothing to switch, so the row
    // is inert and says how to rename instead of lighting up and doing
    // nothing.
    let has_labels = !state.labels.is_empty();
    let names = segmented_row(
        "custom",
        "original",
        !state.show_original_names || !has_labels,
        has_labels.then_some(Message::SetShowOriginalNames(false)),
        has_labels.then_some(Message::SetShowOriginalNames(true)),
        scale,
    );
    body = body.push(options_row_note(
        "names",
        names,
        if has_labels { "Your names for the strips, or the unit's own." } else { "Double-click a strip's name to rename it." },
        scale,
    ));

    body = body.push(theme_picker(state, true, scale));
    body.into()
}

/// The colour themes as a grid of tiles, each drawn in its own theme's
/// colours (its ground, its name in its ink, a bar of its accent), so a
/// theme can be judged before it is picked. One click switches; the
/// current one carries an accent edge. Here in the View page, and on the
/// Look page of Audra settings (the critique, 2026-11-04: the Look page
/// had no theme picker).
/// `caption`: the "THEME" line over the tiles; Audra settings leaves it
/// off, its "Appearance" head already saying so.
pub(crate) fn theme_picker(state: &TuxMix, caption: bool, scale: f32) -> Element<'_, Message> {
    // With the theme by time of day on, the heading says whose slot a
    // click fills ("day theme") and when the other comes.
    let heading = if state.theme_schedule.on {
        let sch = state.theme_schedule;
        if sch.is_day(crate::themetime::local_minutes()) {
            format!("Day theme \u{00b7} night from {}", crate::themetime::clock(sch.night_from))
        } else {
            format!("Night theme \u{00b7} day from {}", crate::themetime::clock(sch.day_from))
        }
    } else {
        // Sentence case, as every caption (critique #27: "THEME").
        "Theme".to_string()
    };
    let mut grid = column![].spacing(theme::SPACE_SM * scale).width(Length::Fill);
    if caption {
        grid = grid.push(text(heading).size(theme::TEXT_SM * scale).color(theme::text_sec()));
    }
    // The tiles, one stop for the keyboard: left and right try the theme
    // before or after (`keyfocus::reach`).
    let mut tiles = column![].spacing(theme::SPACE_SM * scale).width(Length::Fill);
    let mut line = row![].spacing(theme::SPACE_SM * scale);
    for (i, entry) in theme::themes().iter().enumerate() {
        let p = entry.palette;
        let current = i == state.theme_shown;
        let edge = theme::accent();
        // A theme of yours that reads worse than the built-in ones carries
        // an amber dot, and its tooltip says where.
        let mut name_row = row![text(entry.name)
            .size(theme::TEXT_SM * scale)
            .color(p.text_primary)
            .wrapping(iced::advanced::text::Wrapping::None)]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center);
        if !entry.issues.is_empty() {
            name_row = name_row.push(crate::widgets::status_dot(theme::attention(), scale));
        }
        let tile = button(
            column![
                name_row,
                container(iced::widget::Space::new().width(Length::Fill).height(Length::Fixed(4.0 * scale)))
                    .style(move |_t: &iced::Theme| container::Style {
                        background: Some(iced::Background::Color(p.accent)),
                        border: iced::border::rounded(2.0 * scale),
                        ..container::Style::default()
                    }),
            ]
            .spacing(theme::SPACE_SM * scale),
        )
        .padding([theme::SPACE_SM * scale, theme::SPACE_SM * scale])
        .width(Length::FillPortion(1))
        .style(move |_t: &iced::Theme, status: button::Status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(iced::Background::Color(p.floors[0])),
                text_color: p.text_primary,
                // The edge in the page's own theme, not the tile's: a tile's
                // own hairline lies on this page, and read 1.31-2.27 on Dusk
                // and 1.46-2.53 on High contrast (critiques #98 and #99 B).
                border: iced::Border {
                    color: if current {
                        edge
                    } else if hovered {
                        theme::text_primary()
                    } else {
                        theme::firm_edge()
                    },
                    width: if current { 2.0 } else { theme::SPLIT_EDGE_W },
                    radius: theme::RADIUS_SM.into(),
                },
                ..button::Style::default()
            }
        })
        .on_press(Message::SetTheme(i));
        let tip = entry.issues.first().copied().unwrap_or(entry.blurb);
        line = line.push(hint(tile, tip, scale));
        if i % 3 == 2 {
            tiles = tiles.push(line);
            line = row![].spacing(theme::SPACE_SM * scale);
        }
    }
    // A short last line keeps the grid's columns.
    let count = theme::themes().len();
    if !count.is_multiple_of(3) {
        for _ in 0..(3 - count % 3) {
            line = line.push(iced::widget::Space::new().width(Length::FillPortion(1)));
        }
        tiles = tiles.push(line);
    }
    let shown = state.theme_shown;
    let try_theme = |to: Option<usize>| to.filter(|&t| t < count).map(Message::SetTheme);
    grid = grid.push(crate::keyfocus::reach(
        "Theme",
        tiles.into(),
        None,
        try_theme(shown.checked_sub(1)),
        try_theme(Some(shown + 1)),
        theme::RADIUS_SM * scale,
    ));
    // Themes of your own (`usertheme`): files in the themes folder.
    let ghost = |label: &'static str, msg: Message| {
        crate::keyfocus::reach_press(
            label,
            // A quiet pill, as Store… is (critique #26: bare words read as
            // captions, not as things to press).
            button(text(label).size(theme::TEXT_SM * scale))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                .style(theme::page_key(11.0 * scale))
                .on_press(msg.clone())
                .into(),
            Some(msg),
            11.0 * scale,
        )
    };
    grid = grid.push(
        row![
            ghost("Save as your own", Message::ThemeSaveCopy),
            ghost("Open folder", Message::ThemesOpenFolder),
            ghost("Reload", Message::ThemesReload),
        ]
        .spacing(theme::SPACE_SM * scale)
        .wrap(),
    );
    // Fill, so the line wraps in the column: left to its own width it
    // asked for the whole line's length and threw the window's layout off
    // (the mixer's rows stopped folding and much of its text went missing).
    grid = grid.push(
        text("Your own themes are files in Audra's themes folder: save one, change its colours, reload.")
            .size(theme::TEXT_SM * scale)
            .color(theme::text_sec())
            .width(Length::Fill),
    );
    grid.into()
}

const SNAPSHOT_COUNT: u8 = 8;

/// Real — "Mix 1".."Mix 8" through the existing scene save/load
/// machinery (`scenes.rs`, `RmeDevice::capture_scene`/`apply_scene`),
/// mirroring `Message::SceneSave`/`SceneLoad` almost verbatim (see
/// `app.rs`'s own handlers).
///
/// Slots are a two-column grid (`slot_grid`), the selected one in the
/// same graphite thumb as every other switch position in this column.
fn snapshots_panel(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut body = column![]
        .spacing(theme::SPACE_SM * scale)
        .width(Length::Fill);

    body = body.push(slot_grid(
        SNAPSHOT_COUNT,
        // "Snapshot n", not "Mix n": beside "Now mixing Main 1/2" a "Mix 3"
        // read as an output mix (critique #5).
        "Snapshot",
        state.active_snapshot,
        state.snapshot_dirty,
        &state.stored_snapshots,
        state.snapshot_storing,
        state.store_sweep,
        state.slot_landed.map(|(n, _)| (n, crate::motion::slot_flare(state, n))),
        Message::SnapshotSlotPressed,
        Some(SlotNames {
            names: &state.snapshot_names,
            fx: &state.snapshot_fx,
            renaming: state
                .snapshot_renaming
                .map(|slot| (slot + 1, state.snapshot_rename_buf.as_str())),
            on_rename: Message::SnapshotRenameStart,
        }),
        // Where the pages float the head's keys carry the keyboard's ring
        // and Store; the card keeps the names, the files and Workspaces.
        // Listed in the head (issue #18), the list is the keys: its rows
        // carry the ring and Store stands under it.
        match state.key_focus {
            Some(crate::keyfocus::KeyFocus::Counter(crate::keyfocus::CounterItem::Slot(n))) if !pages_float(state) || snapshots_listed(state) => Some(n),
            _ => None,
        },
        scale,
    ));
    if !pages_float(state) || snapshots_listed(state) {
        body = body.push(store_row(state.snapshot_storing, "the mix", Message::SnapshotStore, crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::Store), scale));
    }
    // What the box is, where its rows are listed (critique #95 A P2-2: four
    // ways to keep a mix and none said what it keeps).
    // Two lines, and not while Store waits for its slot, when a click
    // stores (critique #96 A P2-1, P2-3: three lines pushed the file keys
    // off Quick setup's column and cut the pages' keys under a guide card).
    // (And in the Mixer only: where the Control Room or a learning card
    // shares the column, in Quick setup and the Matrix, it cut the file
    // keys and left the pages' keys a sliver; critique #97 A P2-4, P2-5.)
    if snapshots_listed(state) && !state.snapshot_storing && state.view == crate::app::View::Mixer {
        body = body.push(
            text("Saved mixes: click one to bring it back. A lit dot: something is stored.")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec()),
        );
    }
    // One snapshot on its own as a file (manual 21.5.2), to carry a mix
    // between machines or keep more than eight. Quiet pills, as Store… is
    // (critique #26: as bare words they read as captions).
    body = body.push(
        row![
            hint(
                button(text("Save to a file…").size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                    .style(theme::page_key(11.0 * scale))
                    .on_press(Message::SnapshotSaveFile),
                "Saves the mix as it is now as one snapshot file, to keep more than eight or carry it to another computer",
                scale,
            ),
            hint(
                button(text("Open a file…").size(theme::TEXT_SM * scale))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                    .style(theme::page_key(11.0 * scale))
                    .on_press(Message::SnapshotOpenFile),
                "Loads a snapshot file into the mix; Store keeps it in a slot",
                scale,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .wrap(),
    );
    // Workspaces (manual 21.9) hold the snapshots with everything else,
    // so their door sits here (also the W key).
    body = body.push(hint(
        button(text("Workspaces…").size(theme::TEXT_SM * scale))
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
            .style(theme::page_key(11.0 * scale))
            .on_press(Message::ToggleWorkspaces),
        "The whole setup: mix, snapshots, groups, layouts, names (W)",
        scale,
    ));

    body.into()
}

/// Real — 4 groups, membership from the existing multi-select
/// (`state.selected`), independent mute/solo/fader link toggles per
/// group. See `Group`'s own doc comment for the membership model and
/// `app.rs`'s propagation logic for how a linked group actually
/// affects Mute/Solo/Volume.
///
/// The three link cells keep the full-strength fill: a linked group is
/// the one thing in this panel that changes what the mixer does, so it
/// stays the loudest state the sidebar has.
fn groups_panel(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut body = column![]
        .spacing(theme::SPACE_SM * scale)
        .width(Length::Fill);

    // Column captions in secondary text over the `M1`/`S1`/`1`
    // labels, so the cells read as the panel's content and
    // these as their headers.
    body = body.push(
        row![
            text("Mute")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec())
                .width(Length::Fill)
                .center(),
            text("Solo")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec())
                .width(Length::Fill)
                .center(),
            text("Fader")
                .size(theme::TEXT_SM * scale)
                .color(theme::text_sec())
                .width(Length::Fill)
                .center(),
        ]
        .spacing(theme::SPACE_TIGHT * scale),
    );

    let mut grid = column![].spacing(theme::SPACE_TIGHT * scale).width(Length::Fill);
    for (i, g) in state.groups.iter().enumerate() {
        // Cells reflect the group's real link state regardless of edit
        // mode — only the "edit" button itself (below) shows whether a
        // click here will assign a fresh membership or just flip a link.
        // A grid cell (the reference's groups grid): the group's number
        // under its column's head, an inset key wearing its column's hue,
        // lit in it (mute periwinkle, solo amber, fader coral).
        let cell = |lit: bool, hue: iced::Color, link: GroupLink| -> Element<'_, Message> {
            button(text(format!("{}", i + 1)).font(theme::SANS_SEMI).size(theme::TEXT_NUM * scale).center())
                .padding(theme::SPACE_TIGHT * scale)
                .width(Length::Fill)
                .style(theme::grid_cell(hue, lit))
                .on_press(Message::GroupLinkToggle(i, link))
                .into()
        };
        grid = grid.push(
            row![
                cell(g.mute_linked, theme::mute_color(), GroupLink::Mute),
                cell(g.solo_linked, theme::solo_color(), GroupLink::Solo),
                cell(g.fader_linked, theme::accent(), GroupLink::Fader),
            ]
            .spacing(theme::SPACE_TIGHT * scale),
        );
    }
    body = body.push(
        container(grid)
            .padding(theme::SPACE_TIGHT * scale)
            .width(Length::Fill)
            .style(theme::well_box(theme::RADIUS_SM)),
    );
    body = body.push(iced::widget::Space::new().height(Length::Fixed(theme::SPACE_SM * scale)));

    let clear_armed = state.groups_clear_armed.is_some_and(|at| at.elapsed() < crate::resetmix::ARM_FOR);
    // Edit mode is a switch position, not a lit channel: it says where a
    // click on the cells above will land, so it gets the dim pill. "clear"
    // is the pair's destructive half and stays flat — as a `plain_button`
    // it was the lightest-colored box in the whole column, which read as
    // the most important thing in the panel.
    body = body.push(
        row![
            // A toggle like every other (it was a dark segment pill that
            // read as a text field).
            // Edit | Clear as one capsule, as the reference's.
            button(text("Edit").font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).center())
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                .width(Length::Fill)
                // The steady rim (critique #97 B: this pair's top read
                // 2.51-2.81, and 3.35 against High contrast's 4.5).
                .style(theme::split_safe_key(
                    theme::lamp_button(theme::accent(), theme::Lamp { active: state.group_editing, joint: theme::Joint::Left, round: 11.0 * scale, ..theme::Lamp::default() }),
                    state.group_editing,
                ))
                .on_press(Message::GroupEditToggle),
            button(
                text(if clear_armed { "Clear all?" } else { "Clear" })
                    .font(theme::SANS_SEMI)
                    .size(theme::TEXT_SM * scale)
                    .center(),
            )
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                .width(Length::Fill)
                // Armed, lit in the warning's hue until the second press.
                .style(theme::split_safe_key(
                    theme::lamp_button(
                        if clear_armed { theme::attention() } else { theme::accent() },
                        theme::Lamp { active: clear_armed, joint: theme::Joint::Right, round: 11.0 * scale, ..theme::Lamp::default() },
                    ),
                    clear_armed,
                ))
                .on_press(Message::GroupClear),
        ],
    );
    // How to make a group, said where it's done (it was in the wiki only).
    body = body.push(
        text(if state.group_editing {
            "Editing: the strips you've selected join the group whose cell you press. Press Edit again to finish."
        } else {
            "A group ties strips together: mute, solo or move one and the rest follow. To make one: Ctrl- or Shift-click strips to select them, press Edit, then the group's cell under Mute, Solo or Fader."
        })
        .size(theme::TEXT_SM * scale)
        .color(if state.group_editing { theme::accent() } else { theme::text_sec() })
        .width(Length::Fill),
    );

    body.into()
}

pub(crate) const LAYOUT_COUNT: u8 = 6;

/// Real — a layout is `state.collapsed` (which strips are collapsed;
/// there's no strip-reordering yet, so that's the whole story),
/// persisted to disk per slot (see `layouts.rs`, mirroring `scenes.rs`'s
/// own file-per-slot pattern). TotalMix's "all/submix" scope row is not
/// here: a layout is not scoped per submix in this app's model.
fn layout_panel(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    let mut body = column![]
        .spacing(theme::SPACE_SM * scale)
        .width(Length::Fill);

    let layouts_stored: Vec<bool> = state.layouts.iter().map(Option::is_some).collect();
    body = body.push(slot_grid(
        LAYOUT_COUNT,
        "Layout",
        state.active_layout,
        false,
        &layouts_stored,
        state.layout_storing,
        state.store_sweep,
        None,
        Message::LayoutClicked,
        None,
        match state.key_focus {
            Some(crate::keyfocus::KeyFocus::Counter(crate::keyfocus::CounterItem::LayoutSlot(n))) => Some(n),
            _ => None,
        },
        scale,
    ));
    body = body.push(store_row(
        state.layout_storing,
        "the layout",
        Message::LayoutStore,
        crate::keyfocus::counter_focused(state, crate::keyfocus::CounterItem::LayoutStore),
        scale,
    ));
    body = body.push(channel_layout(state, scale));

    body.into()
}

/// Channel Layout (manual 21.5.3), in the tab whose slots store it: a
/// chip per strip, lit while it shows, a click hiding or showing it; and
/// the manual's "All" and "Sub" as a switch for a quick look. Hidden
/// strips keep playing and keep their place in every mix.
fn channel_layout(state: &TuxMix, scale: f32) -> Element<'_, Message> {
    use crate::app::ChannelView;
    let mut col = column![]
        .spacing(theme::SPACE_SM * scale)
        .width(Length::Fill);
    col = col.push(
        text("Channels")
            .size(theme::TEXT_SM * scale)
            .color(theme::text_sec()),
    );
    col = col.push(options_row(
        Some("show"),
        hint(
            segmented(
                &[
                    ("layout", ChannelView::Layout),
                    ("all", ChannelView::All),
                    ("in mix", ChannelView::Submix),
                ]
                .map(|(label, view)| {
                    (label, state.channel_view == view, Some(Message::SetChannelView(view)))
                }),
                scale,
            ),
            "layout: all but the hidden strips. all: every strip for a look. in mix: only what feeds the selected mix",
            scale,
        ),
        scale,
    ));
    // What the chips edit: what the mixer shows, or what MIDI and OSC
    // reach (manual 21.5.3 hides a channel from either).
    col = col.push(options_row(
        Some("for"),
        hint(
            segmented_row(
                "mixer",
                "MIDI, OSC",
                !state.layout_remote,
                Some(Message::SetLayoutRemote(false)),
                Some(Message::SetLayoutRemote(true)),
                scale,
            ),
            "mixer: which strips show. MIDI, OSC: which ones a controller or a remote app can reach",
            scale,
        ),
        scale,
    ));
    let remote = state.layout_remote;
    let strips = crate::app::mixer_strip_ids(state);
    for (row, caption) in [(0usize, "inputs"), (1, "playback"), (2, "outputs")] {
        let mut chips = row![].spacing(theme::SPACE_TIGHT * scale);
        for &cid in strips.iter().filter(|c| {
            crate::app::row_of(**c) == row && crate::app::hideable(state, **c)
        }) {
            // In "remote", a strip hidden from the mixer is out of reach too,
            // and its chip can't give it back (show it first).
            let in_mixer = !state.hidden.contains(&cid);
            let shown = if remote { !crate::app::remote_hidden(state, cid) } else { in_mixer };
            let press = if remote {
                in_mixer.then_some(Message::SetStripRemote(cid, !shown))
            } else {
                Some(Message::SetStripShown(cid, !shown))
            };
            // The name as the strip's header has it ("AN1/2", not "PCM AN1/2").
            let name = crate::app::strip_display_name(state, cid);
            let label = text(crate::app::short_label(&name).to_string())
                .size(theme::TEXT_SM * scale)
                .color(if shown { theme::text_primary() } else { theme::muted() });
            chips = chips.push(
                button(label)
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    // Shown: a tile, like a slot. Hidden: flat and faint.
                    .style(move |t: &iced::Theme, st| {
                        if shown {
                            // A tile with a rim that reads (critique #97 B:
                            // 1.27-1.68, its face 1.26 from the card).
                            theme::split_safe_key(theme::slot_button(false), false)(t, st)
                        } else {
                            theme::ghost_button(t, st)
                        }
                    })
                    .on_press_maybe(press),
            );
        }
        col = col.push(sub_label(caption, scale));
        col = col.push(chips.wrap().vertical_spacing(theme::SPACE_TIGHT * scale));
    }
    col = col.push(
        text(if remote {
            "Click a channel to keep it from MIDI and OSC, or give it back. A channel hidden from the mixer is out of their reach too."
        } else {
            "Click a channel to hide or show it. Hidden ones keep playing; a layout slot stores them."
        })
        .size(theme::TEXT_SM * scale)
        .color(theme::text_sec())
        .width(Length::Fill),
    );
    col.into()
}

#[cfg(test)]
mod tests {
    use super::{db_number, mix_role, pair_name};
    use crate::control_room::Assign;

    #[test]
    fn the_not_heard_row_keeps_to_mixes_you_listen_on_and_leaves_room() {
        // Critique #69 A 3, A 2, A 1.
        use crate::app::{new, update, Message};
        let mut state = new(true, None, None);
        state.fx.reverb_on = true;
        let main = state.control_room.assign.main;
        state.sel_out = main;
        assert!(super::fx_unheard_row(&state, 1.0).is_some(), "Main, where you listen");
        // A mix kept dry on purpose (not Main, Main B, the Phones or Quick's).
        use tuxmix_core::RmeDevice;
        let dry = (0..state.device.output_pair_count()).find(|&p| !super::listened_on(&state, p)).expect("a mix nobody listens on");
        state.sel_out = dry;
        assert!(super::fx_unheard_row(&state, 1.0).is_none(), "no nagging about a dry mix");
        // A short window with a card up: the page keys keep their room.
        state.sel_out = main;
        state.window_size = Some((1280.0, 700.0));
        assert!(crate::app::short_window(&state));
        state.trick_card = Some((crate::tricks::Card::Tip("space"), std::time::Instant::now()));
        assert!(super::fx_unheard_row(&state, 1.0).is_none(), "a card up on a short window");
        let _ = update(&mut state, Message::Tick);
    }

    #[test]
    fn the_keyboard_reaches_the_not_heard_row_and_runs_its_press() {
        // Critique #69 A 7: Tab skipped it.
        use crate::keyfocus::{counter_items, CounterItem};
        let mut state = crate::app::new(true, None, None);
        state.sel_out = state.control_room.assign.main;
        assert!(!counter_items(&state).contains(&CounterItem::FxUnheard), "effects off: no row, no stop");
        state.fx.reverb_on = true;
        let items = counter_items(&state);
        let at = items.iter().position(|i| *i == CounterItem::FxUnheard).expect("a stop for the row");
        // After Only these and the open-mic warning, as drawn.
        assert!(matches!(items[at - 1], CounterItem::OnlyThese | CounterItem::OpenMicExpand | CounterItem::OpenMicKeep), "{:?}", items[at - 1]);
        let (press, said, _) = super::fx_unheard(&state).unwrap();
        let plain = |w: &str| w.replace('\u{a0}', " ");
        assert!(matches!(press, crate::app::Message::FxMakeHeard(_)) && plain(&said).contains("turn up Main 1/2's FX"), "{said}");
        // Only the send down: the send is named (critique #70 A 2).
        let main = state.control_room.assign.main;
        crate::fx::set_return(&mut state, main, crate::fx::db_vol(-12.0));
        let (_, said, _) = super::fx_unheard(&state).unwrap();
        assert!(plain(&said).contains("turn up the FX send"), "{said}");
    }

    #[test]
    fn a_reason_that_isnt_the_mix_or_the_mic_reads_as_it_stands() {
        // Critique #69 A 4: the driver's reason came out as "AN1 sends
        // nothing to the effects".
        let state = crate::app::new(true, None, None);
        assert_eq!(crate::app::heard_why_named(&state, "this Babyface driver can't play the effects back"), "this Babyface driver can't play the effects back");
    }

    #[test]
    fn every_reader_agrees_with_the_one_model() {
        // Critique #68's idea: a table of states, each reader against
        // `fx::block_in`, so no sentence works out its own conditions.
        use crate::app::{new, update, Message};
        use crate::fx::Block;
        let setups: [(&str, fn(&mut crate::app::TuxMix, usize)); 5] = [
            ("off", |_, _| {}),
            ("no send", |s, _| s.fx.reverb_on = true),
            ("return down", |s, m| {
                s.fx.reverb_on = true;
                let _ = update(s, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), m, 0.5));
                let _ = update(s, Message::FxSend(-10.0));
            }),
            ("heard", |s, m| {
                s.fx.reverb_on = true;
                let _ = update(s, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), m, 0.5));
                let _ = update(s, Message::FxSend(-10.0));
                crate::fx::set_return(s, m, crate::fx::db_vol(-12.0));
            }),
            ("held", |s, m| {
                s.fx.reverb_on = true;
                let _ = update(s, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), m, 0.5));
                let _ = update(s, Message::FxSend(-10.0));
                crate::fx::set_return(s, m, crate::fx::db_vol(-12.0));
                crate::fx::set_mute(s, true);
            }),
        ];
        for (name, setup) in setups {
            let mut state = new(true, None, None);
            let main = state.control_room.assign.main;
            state.sel_out = main;
            state.quick_listen = Some(main);
            setup(&mut state, main);
            crate::app::settle_glides(&mut state);
            let block = crate::fx::block_in(&state, main);
            // The placard's row: there exactly when something's in the way
            // that the mix sets (the simulation), and never when heard.
            let row_due = matches!(block, Some(Block::NoSend | Block::NoReturn | Block::NoSendNoReturn | Block::SendFollowsDown | Block::HeldByMute));
            assert_eq!(super::fx_unheard_row(&state, 1.0).is_some(), row_due, "{name}: the row, {block:?}");
            // Rec's reason for Mute FX and for a return down.
            let rec = crate::app::take_fx_heard(&state);
            let silenced = block == Some(Block::HeldByMute) && crate::fx::silenced_by_mute(&state, crate::quick::listen_pair(&state));
            assert_eq!(rec == Err("Mute FX silences the effects where you listen"), silenced, "{name}: Rec, {block:?}");
            if matches!(block, Some(Block::NoReturn | Block::NoSendNoReturn)) {
                assert!(rec.is_err(), "{name}: Rec promises a take the mix can't hear");
            }
            // The strip's FX hover says "silenced" only when it is.
            let out = if state.device.outputs_one_per_pair() { main } else { main * 2 };
            if let Some(mini) = crate::app::fx_mini_of(&state, tuxmix_core::ChannelId::Output(out)) {
                assert_eq!(mini.tip.contains("silenced"), block == Some(Block::HeldByMute), "{name}: the strip's hover");
            }
            // The page's line and `heard` agree with the model.
            assert_eq!(crate::fx::heard(&state), block.is_none(), "{name}: fx::heard");
        }
    }

    #[test]
    fn the_placard_says_whether_the_effects_are_heard_by_the_one_model() {
        // Critique #67's bigger step: the placard, Rec's reasons and the
        // Mute FX line read one model (`fx::block_in`).
        use crate::app::{new, update, Message};
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        state.sel_out = main;
        assert_eq!(super::fx_heard_words(&state, main), None, "both off: nothing said");
        state.fx.reverb_on = true;
        assert_eq!(crate::fx::block_in(&state, main), Some(crate::fx::Block::NoSendNoReturn));
        assert_eq!(super::block_short_words(&state, crate::fx::Block::NoSendNoReturn, main), "nothing goes into the effects yet");
        assert_eq!(super::block_short_words(&state, crate::fx::Block::NoReturn, main), "Main 1/2's FX is turned down");
        let _ = update(&mut state, Message::VolumeChanged(tuxmix_core::ChannelId::Input(0), main, 0.5));
        let _ = update(&mut state, Message::FxSend(-10.0));
        crate::fx::set_return(&mut state, main, crate::fx::db_vol(-12.0));
        crate::app::settle_glides(&mut state);
        assert!(crate::fx::heard_in(&state, main));
        // Critique #68 A 1: the simulation runs nothing, so the placard
        // claims nothing, as Rec doesn't.
        assert_eq!(super::fx_heard_words(&state, main), None);
        assert!(crate::app::take_fx(&state).is_err());
        // Mute FX: the model says held, and Rec's reason agrees.
        crate::fx::set_mute(&mut state, true);
        crate::app::settle_glides(&mut state);
        assert_eq!(crate::fx::block_in(&state, main), Some(crate::fx::Block::HeldByMute));
        assert_eq!(super::block_short_words(&state, crate::fx::Block::HeldByMute, main), "Mute FX is on");
        state.quick_listen = Some(main);
        assert_eq!(crate::app::take_fx_heard(&state), Err("Mute FX silences the effects where you listen"));
    }

    #[test]
    fn the_fx_sends_placard_keys_are_keyboard_stops_and_only_these_isnt() {
        // Its keys had no stops, and Tab stopped on the hidden "Only these"
        // and the not-heard row, neither drawn on this placard.
        use crate::app::new;
        use crate::keyfocus::{counter_items, CounterItem};
        let mut state = new(true, None, None);
        state.sidebar_open = true;
        let main = state.control_room.assign.main;
        state.sel_out = main;
        state.fx.reverb_on = true;
        state.fader_fx = true;
        let items = counter_items(&state);
        assert_eq!(&items[..2], &[CounterItem::FxSendsBack, CounterItem::FxSendsKey], "{items:?}");
        assert!(!items.contains(&CounterItem::OnlyThese) && !items.contains(&CounterItem::FxUnheard));
        assert!(matches!(super::fx_sends_second(&state), Some(crate::app::Message::FxMakeHeard(_))));
        crate::fx::set_mute(&mut state, true);
        crate::app::settle_glides(&mut state);
        assert!(matches!(super::fx_sends_second(&state), Some(crate::app::Message::MuteFxToggle)));
        state.fader_fx = false;
        assert_eq!(&counter_items(&state)[..3], &[CounterItem::MixName, CounterItem::FedBy, CounterItem::OnlyThese]);
    }

    #[test]
    fn every_placard_dot_takes_the_placard_amber() {
        // Critique #77 A 1, #78 A 3: one helper for every placard row's dot,
        // tested on the card in every theme (the old check read the source).
        for entry in crate::theme::THEMES {
            crate::theme::set_palette(entry.palette);
            let dot = super::placard_dot_color();
            let face = crate::theme::text_primary();
            assert!(crate::theme::contrast(dot, face) >= 3.0, "{}: the placard dot {:.2}", entry.name, crate::theme::contrast(dot, face));
        }
        crate::theme::set_palette(&crate::theme::AUDRA);
        let src = include_str!("sidebar.rs");
        let raw = ["status_dot(theme::", "attention()"].concat();
        for f in ["fn output_proc_note(", "fn open_mic_warning("] {
            let start = src.find(f).expect(f);
            let body = &src[start..start + src[start + 1..].find("\nfn ").map_or(src.len() - start, |i| i + 1)];
            assert!(!body.contains(&raw), "{f}: a dot not through placard_dot");
        }
    }

    #[test]
    fn mute_fx_comes_first_and_the_fx_sends_header_agrees_with_the_page() {
        // OpenCode's job 71: Mute FX holding with no send up named the send
        // and offered a press that only set the level kept for later; and
        // the FX-sends header read two of the model's blocks.
        use crate::app::new;
        use crate::fx::Block;
        let mut state = new(true, None, None);
        let main = state.control_room.assign.main;
        state.sel_out = main;
        state.fx.reverb_on = true;
        crate::fx::set_return(&mut state, main, crate::fx::db_vol(-12.0));
        crate::app::settle_glides(&mut state);
        // Only the send down: the header says so, with the page's press.
        assert_eq!(crate::fx::block_in(&state, main), Some(Block::NoSend));
        let (said, fixable, off) = super::fx_sends_head(&state);
        assert!(said.contains("not heard in your speakers yet") && fixable && !off, "{said}");
        // Mute FX on, still no send: Mute FX first, its release the press,
        // but nothing "silenced" (nothing would be heard without it).
        crate::fx::set_mute(&mut state, true);
        crate::app::settle_glides(&mut state);
        assert_eq!(crate::fx::block_in(&state, main), Some(Block::HeldByMute));
        assert!(!crate::fx::silenced_by_mute(&state, main));
        let (said, fixable, _) = super::fx_sends_head(&state);
        assert!(said.ends_with("Mute FX is on") && !fixable, "{said}");
        let (press, _, tip) = super::fx_unheard(&state).expect("the row on Main");
        assert!(matches!(press, crate::app::Message::MuteFxToggle));
        // Critique #71 A 2: Release isn't promised to bring back the room.
        assert!(tip.contains("Nothing is sent to the effects yet") && !tip.contains("come back"), "{tip}");
        assert_eq!(crate::fx::block_past_mute(&state, main), Some(Block::NoSend));
        // A 4: the key's hover by block.
        assert!(super::make_heard_tip(Block::NoSend).contains("FX send") && !super::make_heard_tip(Block::NoSend).contains("return"));
        // Job 72: Rec doesn't blame Mute FX where nothing is sent anyway.
        state.quick_listen = Some(main);
        assert_ne!(crate::app::take_fx_heard(&state), Err("Mute FX silences the effects where you listen"));
        // Released: the send is the next thing named.
        crate::fx::set_mute(&mut state, false);
        crate::app::settle_glides(&mut state);
        assert_eq!(crate::fx::block_in(&state, main), Some(Block::NoSend));
    }

    #[test]
    fn a_clipping_input_takes_the_slot_ahead_of_a_milestone() {
        // The owner's yes after critique #26: the warning is loud now, a
        // reward can wait.
        use crate::app::{new, update, Message};
        let mut state = new(true, None, None);
        state.tips = true;
        state.window_size = Some((1280.0, 700.0));
        crate::celebrate::celebrate(&mut state, crate::celebrate::Reward::Milestone("first-snapshot"));
        assert_eq!(super::foot_slot_h(&state), 40.0, "the milestone's one line on a short window");
        let _ = update(&mut state, Message::Gain(0, 30));
        state.input_meters[0].clipped = true;
        assert!(crate::assist::card(&state).is_some());
        assert_eq!(super::foot_slot_h(&state), 48.0, "the warning's line, ahead of the milestone");
    }

    #[test]
    fn a_notice_over_a_card_never_re_lays_the_head() {
        // Critique #40: at 125% a notice un-snugged the head and the Dim
        // key moved 40 px. The head draws from the card under the notice.
        use crate::app::new;
        use std::time::Instant;
        let mut state = new(true, None, None);
        state.tips = true;
        // A short window (the owner's 1280x700) with a milestone's card.
        state.window_size = Some((1280.0, 700.0));
        crate::celebrate::celebrate(&mut state, crate::celebrate::Reward::Milestone("first-snapshot"));
        assert!(super::snug(&state), "the card snugs the head");
        state.notice = Some(("PAD off: AN1 comes up 11 dB. If it clips, Audra stops.".into(), Instant::now()));
        assert!(super::snug(&state), "the notice over it keeps the head as it was");
        assert!(super::foot_slot_h(&state) > 40.0, "the slot takes the notice's room");
        // The card ending under the notice (OpenCode's job 55) waits for
        // the notice to go before the head changes.
        state.celebration = None;
        state.tips = false;
        assert!(crate::app::card_beneath(&state).is_none(), "no card left under it");
        assert!(super::snug(&state), "the card ended under the notice: still as it was");
        state.notice = None;
        assert!(!super::snug(&state), "the notice gone, the head follows");
        // A tall window whose head leaves too little room for the card:
        // tight, with or without a notice over it.
        let mut state = new(true, None, None);
        state.tips = true;
        state.window_size = Some((1600.0, 1000.0));
        crate::celebrate::celebrate(&mut state, crate::celebrate::Reward::Milestone("first-snapshot"));
        super::HEAD_H.with(|h| h.set(900.0));
        let tight = super::head_tight_needed(&state);
        assert!(tight, "a full head draws tight for the card");
        state.notice = Some(("Saved.".into(), Instant::now()));
        assert_eq!(super::head_tight_needed(&state), tight, "a notice over the card leaves it as it was");
        super::HEAD_H.with(|h| h.set(0.0));
    }

    #[test]
    fn a_kept_undo_notice_gives_the_page_keys_back_and_its_undo_to_the_tools_row() {
        // Critique #45: at 1280x700 with tips off, a kept Undo notice held the
        // page keys away until closed.
        use crate::app::{new, update, Message};
        use std::time::{Duration, Instant};
        let mut state = new(true, None, None);
        state.window_size = Some((1280.0, 700.0));
        let _ = update(&mut state, Message::Gain(0, 40));
        let _ = update(&mut state, Message::Pad(0, true));
        let _ = update(&mut state, Message::Pad(0, false));
        assert!(crate::app::notice_undoable(&state));
        // The head as measured with its keys: taller than the room left.
        super::HEAD_H.with(|h| h.set(690.0));
        assert!(super::page_keys_crowded(&state, 1.0));
        let aged = Instant::now() - Duration::from_secs(6);
        state.notice.as_mut().unwrap().1 = aged;
        state.notice_undo.as_mut().unwrap().0 = aged;
        let _ = update(&mut state, Message::Tick);
        super::HEAD_H.with(|h| h.set(0.0));
        assert!(state.notice.is_none(), "it goes at its time, giving the keys back");
        assert_eq!(crate::app::kept_undo_tip(&state), Some("Turn PAD back on (Ctrl+Z)"), "the tools row's Undo says what it puts back");
        let _ = update(&mut state, Message::VolumeChanged(tuxmix_core::ChannelId::Input(4), 0, 0.5));
        assert_eq!(crate::app::kept_undo_tip(&state), None, "a newer change: the tooltip goes back to plain");
    }

    #[test]
    fn the_placard_names_the_control_room_mixes_by_role() {
        let a = Assign::default();
        assert_eq!(pair_name(&a, 0), "Main 1/2");
        assert_eq!(pair_name(&a, 1), "Phones 3/4");
        assert_eq!(pair_name(&a, 2), "AS1/2");
        assert_eq!(mix_role(&a, 1), "your headphones");
        // A second Phones numbers both, so the label goes in brackets.
        let two = a.with_phones(1, Some(2));
        assert_eq!(pair_name(&two, 1), "Phones 1 (PH3/4)");
        assert_eq!(pair_name(&two, 2), "Phones 2 (AS1/2)");
        // Unassigned, the phones jacks are named for what they are.
        let none = a.with_phones(0, None);
        assert_eq!(pair_name(&none, 1), "PH3/4");
        assert_eq!(mix_role(&none, 1), "the phones jacks");
    }

    #[test]
    fn levels_print_with_a_true_minus_and_infinity_for_silence() {
        assert_eq!(db_number(1.0), "0.0");
        assert_eq!(db_number(0.0), "\u{2212}\u{221e}");
        assert_eq!(db_number(0.5), "\u{2212}6.0");
        assert_eq!(db_number(2.0), "+6.0");
    }

    #[test]
    fn a_tall_column_lists_the_snapshots_once() {
        // Issue #18: where the Snapshots page would stand open under the
        // keys, the list takes the keys' place and the page doesn't open
        // as well; the keyboard reaches its rows and Store.
        use super::{page_inline, page_open, snapshots_listed, Tab};
        use crate::keyfocus::{counter_items, CounterItem};
        let mut state = crate::app::new(true, None, None);
        state.sidebar_open = true;
        state.sidebar_tab = Some(Tab::Snapshots);
        state.window_size = Some((1600.0, 1000.0));
        assert!(snapshots_listed(&state));
        assert_eq!(page_inline(&state), None);
        assert_eq!(page_open(&state), None, "not again as a page");
        let items = counter_items(&state);
        assert!(!items.contains(&CounterItem::Page(Tab::Snapshots)), "its title opens nothing");
        assert!(items.contains(&CounterItem::Slot(1)) && items.contains(&CounterItem::Store));
        let _ = crate::app::view(&state, iced::window::Id::unique());
        // Shorter: the keys, and the page opens from the title.
        state.window_size = Some((1280.0, 880.0));
        assert!(!snapshots_listed(&state));
        assert!(counter_items(&state).contains(&CounterItem::Page(Tab::Snapshots)));
    }
}
