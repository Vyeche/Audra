//! Matrix (submix) view — TotalMix's own crosspoint grid: every input/
//! playback *channel* (not pair) as a row, every hardware output
//! *channel* (not pair) as a column, one small numeric dB cell per
//! crosspoint. Rebuilt 2026-09-06 against a real TotalMix Matrix
//! screenshot (`Totalmix UI Screenshots/`, provided directly by the
//! user) — the previous version (rows = output *pairs*, columns =
//! input/playback channels, one small fader per cell) had the axes
//! backwards and used a fader-per-cell instead of TotalMix's bare
//! numeric cells.
//!
//! Interaction and colors follow RME's own Matrix documentation
//! (docs.rme-audio.com, "The Matrix"), see `widgets::crosspoint`: drag a
//! cell up/down for its gain, Ctrl+click toggles it between off and 0 dB,
//! double-click resets it, the wheel scrolls the grid. Pointing at a cell
//! lights its row and column labels ("the outer labels light up ...
//! according to the mouse position") and the view's caption reads it out.
//! The column header, the output masters and the bus labels stay put
//! while the source rows scroll under them (TotalMix: "the labels are
//! floating. They won't leave the visible area when scrolling").
//!
//! Only the side of an output pair a channel can actually reach is
//! interactive; the other side (for every source except the 4 true-mono
//! AN1-4 inputs) is an inert cell, since the hardware itself has no way
//! to route there (see `push_channel_rows`'s own doc comment).

use iced::keyboard::Modifiers;
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Color, Element, Length};

use tuxmix_core::{ChannelId, ChannelType, RmeDevice};

use crate::app::{pair_bus_label, short_label, Message, TuxMix, OUT_LABELS};
use crate::theme;
use crate::widgets::crosspoint::{cell_text, crosspoint, Crosspoint};

/// Column width / row height at `scale == 1.0` — sized for a signed,
/// one-decimal dB value.
const CELL_W: f32 = 50.0;
const CELL_H: f32 = 20.0;
/// Row-group label column ("AN1", "IN3/4", "AS1/2", ...).
// Wide enough that the label columns hold the floors' serif captions
// ("Software playback") clear of the first mix column (critique #52: the
// caption ran into the lit Main 1/2 band).
const GROUP_LABEL_W: f32 = 100.0;

/// The group label column's width at scale 1: `GROUP_LABEL_W`, or wider
/// for a longer name (a renamed channel), so no label is cut off. Estimated
/// from the face's average advance at `TEXT_SM`, with a little margin.
fn group_label_w(state: &TuxMix) -> f32 {
    input_row_groups(state)
        .iter()
        .chain(playback_row_groups(state).iter())
        .map(|g| g.label.chars().count())
        .chain(std::iter::once("Outputs".len()))
        // The rows' plain words share the column (`row_words`).
        .chain(state.plain_names.then_some("optical in 1/2".len()))
        .map(|chars| chars as f32 * theme::TEXT_SM * 0.6 + theme::SPACE_MD)
        .fold(GROUP_LABEL_W, f32::max)
}
/// Per-row sequential label column ("In 1", "Pb 12", ...).
const IDX_LABEL_W: f32 = 40.0;
/// A double-clicked crosspoint goes to 0 dB, as a double-clicked fader
/// does (BF §21.3: "jumps to 0 dB"); it was -2.5 dB, which a comment here
/// claimed the strips used (the critique, 2026-11-05). Gently up
/// (`Message::MatrixSet`).
const RESET_VOLUME: f32 = 1.0;

/// Extra space after each output pair's two columns (and between source
/// groups' rows), on top of the grid's hairline: without it twelve equal
/// columns read as one undifferentiated wall of numbers.
const PAIR_GAP: f32 = theme::SPACE_SM;

/// One grid cell. `cid`/`pair` say what a gesture writes
/// (`Message::VolumeChanged(cid, pair, _)`), `col` is the physical output
/// column, for the hover crosshair.
#[allow(clippy::too_many_arguments)]
fn cell(
    state: &TuxMix,
    cid: ChannelId,
    pair: usize,
    col: usize,
    value: f32,
    shown: f32,
    interactive: bool,
    muted: bool,
    phase: bool,
    scale: f32,
) -> Element<'static, Message> {
    let modifiers: Modifiers = state.modifiers;
    let xp = crosspoint(
        Crosspoint {
            value,
            shown,
            interactive,
            muted,
            phase,
            master: matches!(cid, ChannelId::Output(_)),
            hovered: state.matrix_hover == Some((cid, col)),
            pulse: crate::motion::matrix_pulse(state, cid, pair),
            modifiers,
            scale,
            reset_value: RESET_VOLUME,
            on_set: Box::new(move |v, reset| Message::MatrixSet(cid, pair, v, reset)),
            // Mono Mode acts on this row's channel alone; the output row
            // (masters) is never paired that way.
            on_change: if state.matrix_mono && !matches!(cid, ChannelId::Output(_)) {
                Box::new(move |v| Message::MatrixVolume(cid, pair, v))
            } else {
                Box::new(move |v| Message::VolumeChanged(cid, pair, v))
            },
            on_release: Message::RangeCleared(cid),
            on_hover: Box::new(move |over| Message::MatrixHover(cid, col, over)),
        },
        CELL_W * scale,
        CELL_H * scale,
    );
    // The keyboard's cell (`keyfocus`) carries the focus ring.
    let row = if matches!(cid, ChannelId::Output(_)) { None } else { Some(cid) };
    crate::keyfocus::ring_tinted(xp, crate::keyfocus::matrix_focused(state, row, col), 3.0 * scale)
}

/// The rows the keyboard's cell walks (`keyfocus`), as the grid stands:
/// every source channel shown, inputs then playback, then the Outputs
/// row (`None`).
pub(crate) fn cursor_rows(state: &TuxMix) -> Vec<Option<ChannelId>> {
    let mut rows = Vec::new();
    type Source = (Vec<RowGroup>, fn(usize) -> ChannelId);
    let sources: [Source; 2] =
        [(input_row_groups(state), ChannelId::Input), (playback_row_groups(state), ChannelId::Playback)];
    for (groups, cid_of) in sources {
        for g in groups {
            for local in 0..g.count {
                let cid = cid_of(g.first_idx + local);
                if row_source(state, cid).is_some() {
                    rows.push(Some(cid));
                }
            }
        }
    }
    rows.push(None);
    rows
}

/// The grid's columns, physical output channels in the order they stand.
pub(crate) fn cursor_cols(state: &TuxMix) -> Vec<usize> {
    shown_out_pairs(state).into_iter().flat_map(|p| [p * 2, p * 2 + 1]).collect()
}

/// A column head: a mix's name, a step up from the row labels (the
/// reviewer: at their size and colour the heads had no rank), coral for
/// the selected mix and the hovered column's.
/// `kb_side`: the keyboard's cell is in this pair's left (0) or right (1)
/// column, marked by a short tick under the head in the keyboard's colour,
/// so its cell reads as a crosshair with its lit row name (the reviewer,
/// 2026-11-05: in a grid of lit rims one cell's ring was weak).
fn header_cell(label: String, lit: bool, width: f32, kb_side: Option<usize>, scale: f32) -> Element<'static, Message> {
    let head = container(
        text(label)
            .font(theme::SANS_SEMI)
            .size(theme::TEXT_MD * scale)
            // Readable on the panel under the lamp's band (critique #28:
            // 3.9:1 on Solarized, raw).
            .color(if lit { theme::readable_on(theme::accent(), theme::text_primary(), &theme::matrix_head_grounds()) } else { theme::text_primary() })
            .wrapping(iced::advanced::text::Wrapping::None),
    )
    .width(Length::Fixed(width * scale))
    .height(Length::Fixed(CELL_H * scale))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center)
    .clip(true);
    let Some(side) = kb_side else {
        return head.into();
    };
    const TICK_W: f32 = 16.0;
    let centre = side as f32 * (CELL_W * scale + theme::SPACE_HAIRLINE) + CELL_W * scale / 2.0;
    let tick = container(Space::new().width(Length::Fixed(TICK_W * scale)).height(Length::Fixed(2.0 * scale)))
        .style(|_t: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(theme::focus_mark())),
            border: iced::border::rounded(1.0),
            ..iced::widget::container::Style::default()
        });
    iced::widget::stack![
        head,
        container(row![Space::new().width(Length::Fixed((centre - TICK_W * scale / 2.0).max(0.0))), tick])
            .height(Length::Fixed(CELL_H * scale))
            .align_y(iced::Alignment::End),
    ]
    .into()
}

/// The plain words for a pair's second row, the empty cell under its
/// code: "optical in 1/2" as the input's strip says, "computer audio"
/// for a playback (critique #84 A P3-3: three things
/// were "AS1/2" and only the column said which).
fn row_words(state: &TuxMix, cid: ChannelId) -> Option<&'static str> {
    if !state.plain_names {
        return None;
    }
    match cid {
        ChannelId::Input(i) => {
            let ins = state.device.inputs();
            let optical = |t: ChannelType| matches!(t, ChannelType::SPDIF | ChannelType::ADAT);
            if !ins.get(i).is_some_and(|c| optical(c.channel_type)) {
                return None;
            }
            let first = ins.iter().position(|c| optical(c.channel_type)).unwrap_or(i);
            ["optical in 1/2", "optical in 3/4", "optical in 5/6", "optical in 7/8"].get(i.saturating_sub(first) / 2).copied()
        }
        // Not the strip's "to speakers": in a grid the column is where it
        // goes (critique #85 A P3-3).
        ChannelId::Playback(_) => Some("computer audio"),
        ChannelId::Output(_) => None,
    }
}

fn label_cell(label: String, width: f32, color: Color, scale: f32) -> Element<'static, Message> {
    container(
        text(label)
            .size(theme::TEXT_SM * scale)
            .color(color)
            .wrapping(iced::advanced::text::Wrapping::None),
    )
    .width(Length::Fixed(width * scale))
    .height(Length::Fixed(CELL_H * scale))
    .align_x(iced::Alignment::Start)
    .align_y(iced::Alignment::Center)
    .clip(true)
    .into()
}

/// One row-group's worth of rows: a shared label ("AN1", "IN3/4", ...),
/// a starting sequential index (for "In N"/"Pb N"), and how many
/// physical channels it covers (1 for the two solo Mic inputs, 2 for
/// every hardware-paired source and every playback pair).
struct RowGroup {
    label: String,
    first_idx: usize,
    count: usize,
}

/// Input row-groups: the two Mic-type channels (AN1, AN2) are each
/// their own physical jack, shown solo — every other input type comes
/// in an inherent hardware pair (Instrument, Line, ADAT), shown as one
/// 2-row group. Mirrors `mixer_view`'s own pairing assumption
/// (`n_input_pairs = inputs().len() / 2`) but, unlike the Mixer's
/// strips, never collapses a pair into one combined row regardless of
/// `input_pair_linked` — TotalMix's Matrix always shows every physical
/// channel's own crosspoints, link state or not.
fn input_row_groups(state: &TuxMix) -> Vec<RowGroup> {
    let mut groups = all_input_row_groups(state);
    groups.retain(|g| group_shown(state, g, ChannelId::Input));
    groups
}

/// Channel Layout (manual 21.5.3): a row group shows while any of its
/// channels does (`app::channel_hidden`).
fn group_shown(state: &TuxMix, g: &RowGroup, cid_of: fn(usize) -> ChannelId) -> bool {
    (g.first_idx..g.first_idx + g.count).any(|i| !crate::app::channel_hidden(state, cid_of(i)))
}

/// The output pairs the grid shows: a pair goes when both its channels are
/// hidden; the Control Room's pairs always show.
fn shown_out_pairs(state: &TuxMix) -> Vec<usize> {
    (0..state.device.outputs().len() / 2)
        .filter(|&p| {
            crate::control_room::is_control_room_pair(state, p)
                || !(crate::app::channel_hidden(state, ChannelId::Output(p * 2))
                    && crate::app::channel_hidden(state, ChannelId::Output(p * 2 + 1)))
        })
        .collect()
}

fn all_input_row_groups(state: &TuxMix) -> Vec<RowGroup> {
    let inputs = state.device.inputs();
    let mut groups = Vec::new();
    let mut i = 0;
    while i < inputs.len() {
        let ty = inputs[i].channel_type;
        let (label, count) = if ty == ChannelType::Mic {
            (inputs[i].name.clone(), 1)
        } else if let Some(next) = inputs.get(i + 1) {
            (pair_bus_label(&inputs[i].name, &next.name), 2)
        } else {
            (inputs[i].name.clone(), 1)
        };
        let label = crate::app::user_label(state, ChannelId::Input(i))
            .map(str::to_string)
            .unwrap_or(label);
        groups.push(RowGroup {
            label,
            first_idx: i,
            count,
        });
        i += count;
    }
    groups
}

/// Playback row-groups: always one pair per hardware output bus,
/// labeled with the same verified `OUT_LABELS` the rest of the app
/// uses (not TotalMix's own instance-specific renaming in the
/// reference screenshot, e.g. "Main" for what this hardware calls
/// "PH3/4" — that's this session's own established naming, kept).
fn playback_row_groups(state: &TuxMix) -> Vec<RowGroup> {
    let n = state.device.playbacks().len();
    let mut groups: Vec<RowGroup> = (0..n / 2)
        .map(|pair| RowGroup {
            label: crate::app::user_label(state, ChannelId::Playback(pair * 2))
                .map(str::to_string)
                .or_else(|| OUT_LABELS.get(pair).map(|s| s.to_string()))
                .unwrap_or_default(),
            first_idx: pair * 2,
            count: 2,
        })
        .collect();
    groups.retain(|g| group_shown(state, g, ChannelId::Playback));
    groups
}

/// A mono source's (volume, pan) decoded into what reaches the left and
/// right outputs of a pair.
fn mono_sides(vol: f32, pan: i8) -> (f32, f32) {
    let pf = pan as f32 / 100.0;
    if pan == 0 {
        (vol, vol)
    } else if pf < 0.0 {
        (vol, vol * (1.0 + pf))
    } else {
        (vol * (1.0 - pf), vol)
    }
}

/// What one source row needs from the device model.
struct RowSource {
    volumes: Vec<f32>,
    /// `Some` for the true-mono sources (AN1-4), which reach both sides
    /// of every pair through their pan.
    pans: Option<Vec<i8>>,
    /// A linked playback pair's balance per output pair: the cell shows
    /// its own side as the balance leaves it (the stored level stays the
    /// fader's).
    balance: Option<Vec<i8>>,
    muted: bool,
    phase: bool,
}

fn row_source(state: &TuxMix, cid: ChannelId) -> Option<RowSource> {
    match cid {
        ChannelId::Input(i) => state.device.inputs().get(i).map(|ch| RowSource {
            volumes: ch.volumes.clone(),
            pans: (i < 4).then(|| ch.pans.clone()),
            balance: None,
            muted: ch.mute,
            phase: ch.phase,
        }),
        ChannelId::Playback(i) => state.device.playbacks().get(i).map(|ch| RowSource {
            volumes: ch.volumes.clone(),
            pans: None,
            balance: Some((0..ch.volumes.len()).map(|p| state.device.playback_balance(i, p)).collect()),
            muted: ch.mute,
            phase: false,
        }),
        ChannelId::Output(_) => None,
    }
}

/// The gain a source row sends to physical output column `col`, as the
/// cell shows it (`None` = the hardware has no path there).
fn shown_gain(src: &RowSource, idx: usize, col: usize) -> Option<f32> {
    let pair = col / 2;
    let vol = *src.volumes.get(pair)?;
    match &src.pans {
        Some(pans) => {
            let (l, r) = mono_sides(vol, pans.get(pair).copied().unwrap_or(0));
            Some(if col.is_multiple_of(2) { l } else { r })
        }
        None => (col % 2 == idx % 2).then(|| match &src.balance {
            Some(b) => tuxmix_core::channel::balance_side(idx, vol, b.get(pair).copied().unwrap_or(0)),
            None => vol,
        }),
    }
}

/// Builds every data row for one section (inputs or playbacks) and
/// appends them to `rows`.
///
/// **Which columns are interactive**: our own per-channel model stores
/// one volume *per output pair*, not per individual physical output
/// channel — matching the hardware itself, which (per `babyface.rs`'s
/// own crosspoint comment) has no way to route a non-mono source
/// differently into an output pair's left vs right side at all. That
/// one value is editable through whichever single column (left/right)
/// this channel naturally feeds (`idx % 2`, the same convention
/// `set_channel_volume` already uses everywhere); the other column of
/// that pair is inert — there's nothing real on this hardware to drag.
/// The 4 true-mono sources (AN1-4) are the one genuine exception: both
/// columns show their pan-decoded values and both are interactive,
/// moving the *same* underlying `volumes[pair]` (this view edits level,
/// not pan).
#[allow(clippy::too_many_arguments)]
fn push_channel_rows<'a>(
    state: &'a TuxMix,
    mut rows: iced::widget::Column<'a, Message>,
    groups: &[RowGroup],
    out_pairs: &[usize],
    row_prefix: &'static str,
    group_w: f32,
    cid_of: impl Fn(usize) -> ChannelId,
    scale: f32,
) -> iced::widget::Column<'a, Message> {
    let hovered_row = state.matrix_hover.map(|(c, _)| c);
    for (gi, g) in groups.iter().enumerate() {
        if gi > 0 {
            rows = rows.push(iced::widget::Space::new().height(PAIR_GAP * scale));
        }
        for local in 0..g.count {
            let idx = g.first_idx + local;
            let cid = cid_of(idx);
            let Some(src) = row_source(state, cid) else {
                continue;
            };
            let lit = hovered_row == Some(cid);
            // The keyboard's row: its number in the keyboard's colour.
            let kb_row = matches!(state.key_focus, Some(crate::keyfocus::KeyFocus::Matrix(Some(c), _)) if c == cid);
            let mut r = row![
                label_cell(
                    if local == 0 {
                        g.label.clone()
                    } else {
                        row_words(state, cid).unwrap_or_default().to_string()
                    },
                    group_w,
                    // The name in the ink, as a strip's is: hue is kept
                    // for state (the critique, 2026-11-04: the rows wore
                    // the type colours the Mixer had given up). Its words
                    // quieter, as under the column heads.
                    if local > 0 { theme::text_sec() } else if lit { theme::accent() } else { theme::text_primary() },
                    scale
                ),
                label_cell(
                    format!("{row_prefix} {}", idx + 1),
                    IDX_LABEL_W,
                    if kb_row { theme::focus_mark() } else if lit { theme::accent() } else { theme::text_sec() },
                    scale
                ),
            ]
            .spacing(theme::SPACE_HAIRLINE);

            for col in out_pairs.iter().flat_map(|&p| [p * 2, p * 2 + 1]) {
                let pair = col / 2;
                let Some(&stored) = src.volumes.get(pair) else { continue };
                r = r.push(match shown_gain(&src, idx, col) {
                    Some(shown) => cell(
                        state, cid, pair, col, stored, shown, true, src.muted, src.phase, scale,
                    ),
                    None => cell(state, cid, pair, col, 0.0, 0.0, false, false, false, scale),
                });
                if col % 2 == 1 {
                    r = r.push(pair_gap(scale));
                }
            }
            rows = rows.push(r);
        }
    }
    rows
}

/// A row naming the source section below it, in the Mixer's own section
/// vocabulary, so the playback rows don't run straight on from the inputs.
fn section_caption(label: &str, scale: f32) -> Element<'static, Message> {
    // In the serif the Mixer names its rows in, sentence case.
    container(
        text(label.to_string())
            .font(theme::SERIF)
            .size(theme::TEXT_MD * scale)
            .color(theme::text_primary()),
    )
    .height(Length::Fixed(CELL_H * scale))
    .align_y(iced::Alignment::End)
    .padding(iced::Padding {
        bottom: theme::SPACE_TIGHT * scale,
        ..iced::Padding::ZERO
    })
    .into()
}

fn pair_gap(scale: f32) -> Element<'static, Message> {
    iced::widget::Space::new().width(PAIR_GAP * scale).into()
}

fn hairline(width: f32) -> Element<'static, Message> {
    container(
        iced::widget::Space::new()
            .width(Length::Fixed(width))
            .height(1),
    )
    .style(|_theme: &iced::Theme| container::Style {
        background: Some(iced::Background::Color(theme::border())),
        ..container::Style::default()
    })
    .into()
}

/// The hovered cell, said in a sentence for the view's caption, with
/// what a gesture there would do: "AN1 plays in Phones 3/4 (left) at
/// -6.0 dB. Drag to change it; …", the mix by the name the column head
/// gives it. `None` when nothing is hovered.
pub fn hover_readout(state: &TuxMix) -> Option<String> {
    let (cid, col) = state.matrix_hover?;
    let bus = column_head(state, col / 2);
    let side = if col % 2 == 0 { "L" } else { "R" };
    let (source, gain) = match cid {
        ChannelId::Output(i) => {
            let out = state.device.outputs().get(i)?;
            ("Output master".to_string(), Some(out.volume))
        }
        ChannelId::Input(i) | ChannelId::Playback(i) => {
            let name = crate::app::user_label(state, cid)
                .map(str::to_string)
                .unwrap_or_else(|| {
                    match cid {
                        ChannelId::Input(_) => state.device.inputs().get(i).map(|c| c.name.clone()),
                        _ => state
                            .device
                            .playbacks()
                            .get(i)
                            .map(|c| short_label(&c.name).to_string()),
                    }
                    .unwrap_or_default()
                });
            let src = row_source(state, cid)?;
            (name, shown_gain(&src, i, col))
        }
    };
    // In a sentence, with what to do next (Learning pass 1, "Explain this
    // grid": the owner was learning the Matrix by trial).
    let side_word = if side == "L" { "left" } else { "right" };
    Some(match (cid, gain) {
        (ChannelId::Output(_), Some(v)) => format!(
            "{bus}'s own level, {}: everything this output plays, after its mix. Drag to set it",
            cell_text(v).map_or("off".to_string(), |db| format!("{db} dB"))
        ),
        (_, None) => format!(
            "{source} can't reach {bus}'s {side_word} side: a stereo pair's channel plays only on its own side"
        ),
        (_, Some(v)) => match cell_text(v) {
            Some(db) => format!(
                "{source} plays in {bus} ({side_word}) at {db} dB{}. Drag to change it; Ctrl+click takes it out",
                // The metal box: full level, said (critique #56: silver
                // read as switched off on a light theme).
                if v > 0.0 && (20.0 * v.log10()).abs() < 0.05 { ", full level" } else { "" }
            ),
            None => format!(
                "{source} isn't in {bus} ({side_word}). Ctrl+click sends it at 0 dB, or drag up to fade it in"
            ),
        },
    })
}

/// An output pair's name over its two columns: the user's name for it,
/// or the placard's ("Main 1/2", "Phones 3/4").
pub(crate) fn column_head(state: &TuxMix, pair: usize) -> String {
    crate::app::user_label(state, ChannelId::Output(pair * 2))
        .map(str::to_string)
        .unwrap_or_else(|| crate::sidebar::mix_name(state, pair))
}

pub fn view(state: &TuxMix) -> Element<'_, Message> {
    iced::widget::responsive(move |size| grid_view(state, size)).into()
}

/// The whole grid for a panel of `size` (see the frozen-panes note below
/// for why the size matters).
fn grid_view(state: &TuxMix, size: iced::Size) -> Element<'_, Message> {
    // Channel Layout (manual 21.5.3): hidden sources and outputs leave the
    // grid as they leave the mixer.
    let out_pairs = shown_out_pairs(state);
    let n_outputs = out_pairs.len() * 2;
    let (in_groups, pb_groups) = (input_row_groups(state), playback_row_groups(state));
    let rows_in = |gs: &[RowGroup]| gs.iter().map(|g| g.count).sum::<usize>();
    let group_gaps = in_groups.len().saturating_sub(1) + pb_groups.len().saturating_sub(1);
    let dims = GridDims {
        n_outputs,
        n_rows: rows_in(&in_groups) + rows_in(&pb_groups),
        group_gaps,
        group_w: group_label_w(state),
        plain_heads: state.plain_names,
    };
    let pad = theme::SPACE_LG;
    let scale = fit_scale(state.ui_scale, &dims, size, pad);
    let hovered_col = state.matrix_hover.map(|(_, c)| c);

    // ── Column heads: each output pair by its mix's name ("Main 1/2",
    // "Phones 3/4"), one name over both its columns, lit for the selected
    // submix and the hovered column's pair. (They were "Out 1".."Out 12"
    // here and the names sat under the grid: the reviewer, 2026-11-03,
    // "Main is still Out 1/2 in the Matrix heads".)
    let mut header = row![
        label_cell(String::new(), dims.group_w, theme::text_sec(), scale),
        label_cell(String::new(), IDX_LABEL_W, theme::text_sec(), scale),
    ]
    .spacing(theme::SPACE_HAIRLINE);
    for &pair in &out_pairs {
        let kb_side = match state.key_focus {
            Some(crate::keyfocus::KeyFocus::Matrix(_, col)) if col / 2 == pair => Some(col % 2),
            _ => None,
        };
        header = header.push(header_cell(
            column_head(state, pair),
            pair == state.sel_out || hovered_col.is_some_and(|c| c / 2 == pair),
            2.0 * CELL_W + theme::SPACE_HAIRLINE / scale,
            kb_side,
            scale,
        ));
        header = header.push(pair_gap(scale));
    }
    // The plain words under the heads ("your speakers", "optical 1/2"), as
    // under the strips' names (critique #83 A P3-2).
    let header: Element<'_, Message> = if state.plain_names {
        let mut words = row![
            label_cell(String::new(), dims.group_w, theme::text_sec(), scale),
            label_cell(String::new(), IDX_LABEL_W, theme::text_sec(), scale),
        ]
        .spacing(theme::SPACE_HAIRLINE);
        for &pair in &out_pairs {
            words = words.push(
                container(
                    text(crate::sidebar::mix_role_short(state, pair))
                        .size(theme::TEXT_SM * scale)
                        .color(theme::text_sec())
                        .wrapping(iced::advanced::text::Wrapping::None),
                )
                .width(Length::Fixed((2.0 * CELL_W + theme::SPACE_HAIRLINE / scale) * scale))
                .height(Length::Fixed(CELL_H * scale))
                .align_x(iced::Alignment::Center)
                .align_y(iced::Alignment::Center)
                .clip(true),
            );
            words = words.push(pair_gap(scale));
        }
        column![header, words].spacing(theme::SPACE_HAIRLINE).into()
    } else {
        header.into()
    };

    let mut sources =
        column![section_caption("Hardware inputs", scale)].spacing(theme::SPACE_HAIRLINE);
    sources = push_channel_rows(
        state,
        sources,
        &in_groups,
        &out_pairs,
        "In",
        dims.group_w,
        ChannelId::Input,
        scale,
    );
    sources = sources.push(iced::widget::Space::new().height(PAIR_GAP * scale));
    sources = sources.push(section_caption("Software playback", scale));
    sources = push_channel_rows(
        state,
        sources,
        &pb_groups,
        &out_pairs,
        "PB",
        dims.group_w,
        ChannelId::Playback,
        scale,
    );

    // ── Master "Outputs" row: each output channel's own master volume,
    // distinct from (and separated by a divider from) the crosspoints.
    let out_lit = matches!(state.matrix_hover, Some((ChannelId::Output(_), _)));
    let mut out_row = row![
        label_cell(
            "Outputs".into(),
            dims.group_w,
            if out_lit {
                theme::accent()
            } else {
                theme::text_primary()
            },
            scale
        ),
        label_cell(String::new(), IDX_LABEL_W, theme::text_sec(), scale),
    ]
    .spacing(theme::SPACE_HAIRLINE);
    let outputs = state.device.outputs();
    for idx in out_pairs.iter().flat_map(|&p| [p * 2, p * 2 + 1]) {
        let Some(out) = outputs.get(idx) else { continue };
        out_row = out_row.push(cell(
            state,
            ChannelId::Output(idx),
            0,
            idx,
            out.volume,
            out.volume,
            true,
            out.mute,
            false,
            scale,
        ));
        if idx % 2 == 1 {
            out_row = out_row.push(pair_gap(scale));
        }
    }

    // Frozen panes: only the source rows scroll vertically, under a fixed
    // header and above the fixed output row; the whole grid
    // scrolls sideways as one, so the columns never drift from their
    // labels. The source area gets a fixed height only when its rows do
    // not fit: a column lays its children out in order, so a scrollable
    // left to size itself takes all the room before the output row is
    // placed, and pushes it off the panel.
    let sources_h = dims.sources_h(scale);
    let fixed_h = dims.fixed_h(scale);
    let grid_w = dims.width(scale);

    let sideways_bar = if grid_w > size.width - 2.0 * pad {
        10.0
    } else {
        0.0
    };
    let room = size.height - 2.0 * pad - fixed_h - sideways_bar;
    let (body_h, body_px) = if sources_h <= room {
        (Length::Shrink, sources_h)
    } else {
        let h = room.max(3.0 * CELL_H * scale);
        (Length::Fixed(h), h)
    };
    let body = scrollable(sources)
        .direction(scrollable::Direction::Vertical(theme::thin_scrollbar()))
        .height(body_h)
        .style(theme::scrollable);
    let grid = column![
        header,
        body,
        iced::widget::Space::new().height(PAIR_GAP * scale),
        hairline(grid_w),
        out_row,
    ]
    .spacing(theme::SPACE_HAIRLINE);
    // The selected mix's lamp: one band behind its two columns, from the
    // head to the Outputs row, as the wash lies over the strips (the
    // reviewer: painted into each cell it lifted the cuts into brown
    // patches and skipped the cells with no path, a checkerboard). The
    // band is the stack's first layer, so it sizes the stack: the grid's
    // own size, with a pixel or two to spare.
    let grid_h = fixed_h + body_px;
    let slack = 2.0;
    let band: Element<'_, Message> = match out_pairs.iter().position(|&p| p == state.sel_out) {
        Some(k) => {
            let h = theme::SPACE_HAIRLINE;
            let pair_w = 2.0 * CELL_W * scale + h;
            let step = 2.0 * CELL_W * scale + PAIR_GAP * scale + 3.0 * h;
            let grow = (PAIR_GAP * scale / 2.0).min(3.0 * scale);
            let x0 = (dims.group_w + IDX_LABEL_W) * scale + 2.0 * h + k as f32 * step - grow;
            row![
                iced::widget::Space::new().width(x0.max(0.0)),
                container(iced::widget::Space::new())
                    .width(pair_w + 2.0 * grow)
                    .height(Length::Fill)
                    .style(theme::lamp_band),
            ]
            .width(grid_w + slack)
            .height(grid_h + slack)
            .into()
        }
        None => iced::widget::Space::new().width(grid_w + slack).height(grid_h + slack).into(),
    };
    let grid = iced::widget::stack![band, grid];
    // Sized to the grid, so the panel hugs it instead of running to the
    // window's edge as an empty frame; it still scrolls sideways when the
    // window is narrower than the grid.
    let sideways = scrollable(grid)
        .direction(scrollable::Direction::Horizontal(theme::thin_scrollbar()))
        .width(Length::Shrink)
        .style(theme::scrollable);
    container(container(sideways).style(theme::panel).padding(pad))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// What the grid's size depends on, besides the scale.
struct GridDims {
    n_outputs: usize,
    /// Input plus playback rows.
    n_rows: usize,
    /// Gaps between source row-groups, both sections together.
    group_gaps: usize,
    /// The group label column's width at scale 1 (`group_label_w`).
    group_w: f32,
    /// The plain words' line under the column heads (the switch on).
    plain_heads: bool,
}

impl GridDims {
    /// The scrolling source rows, with their two section captions.
    fn sources_h(&self, scale: f32) -> f32 {
        let elements = self.n_rows + 2 + self.group_gaps + 1;
        (self.n_rows + 2) as f32 * CELL_H * scale
            + (self.group_gaps + 1) as f32 * PAIR_GAP * scale
            + elements.saturating_sub(1) as f32 * theme::SPACE_HAIRLINE
    }

    /// The frozen rows: the column heads and the output row.
    fn fixed_h(&self, scale: f32) -> f32 {
        let plain = if self.plain_heads { CELL_H * scale + theme::SPACE_HAIRLINE } else { 0.0 };
        2.0 * CELL_H * scale + PAIR_GAP * scale + 1.0 + 4.0 * theme::SPACE_HAIRLINE + plain
    }

    fn width(&self, scale: f32) -> f32 {
        // A row is two label columns, the cells, and a gap after each
        // pair, with a hairline between every two of those items.
        let items = 2 + self.n_outputs + self.n_outputs / 2;
        (self.group_w + IDX_LABEL_W) * scale
            + self.n_outputs as f32 * CELL_W * scale
            + (self.n_outputs / 2) as f32 * PAIR_GAP * scale
            + (items - 1) as f32 * theme::SPACE_HAIRLINE
    }
}

/// The scale the grid is drawn at: the largest that fits the panel whole,
/// with no scrolling, capped at `SCALE_MAX`, and never below the manual
/// zoom. At 1.0 the grid filled a third of a 1080p window; the Mixer keeps
/// its fixed geometry, but a grid has no reason to stay small. Zooming in
/// past the fit still works, and scrolls as before.
fn fit_scale(zoom: f32, dims: &GridDims, size: iced::Size, pad: f32) -> f32 {
    // Every size above is `a * scale + b`: solve for the scale that fills
    // the room. The 4 px are the panel's border and a margin for layout
    // rounding, without which an exact fit could still scroll by a pixel.
    let room_w = size.width - 2.0 * pad - 4.0;
    let room_h = size.height - 2.0 * pad - 4.0;
    let (bw, aw) = (dims.width(0.0), dims.width(1.0) - dims.width(0.0));
    let h = |s: f32| dims.sources_h(s) + dims.fixed_h(s);
    let (bh, ah) = (h(0.0), h(1.0) - h(0.0));
    let fit = ((room_w - bw) / aw).min((room_h - bh) / ah);
    if fit.is_finite() {
        zoom.max(fit.min(theme::SCALE_MAX))
    } else {
        zoom
    }
}

#[cfg(test)]
mod tests {
    use super::{mono_sides, shown_gain, RowSource};

    #[test]
    fn a_stereo_source_reaches_only_its_own_side() {
        let src = RowSource {
            volumes: vec![0.5, 1.0],
            pans: None,
            balance: None,
            muted: false,
            phase: false,
        };
        // An odd channel (right of its pair) reaches the odd columns.
        assert_eq!(shown_gain(&src, 1, 0), None);
        assert_eq!(shown_gain(&src, 1, 1), Some(0.5));
        assert_eq!(shown_gain(&src, 1, 3), Some(1.0));
    }

    #[test]
    fn a_playback_pairs_cell_shows_its_side_under_the_balance() {
        // Balance R50 into the first pair: the left channel's side is
        // lowered to half, the right one stays at the fader.
        let src = RowSource {
            volumes: vec![1.0, 1.0],
            pans: None,
            balance: Some(vec![50, 0]),
            muted: false,
            phase: false,
        };
        assert_eq!(shown_gain(&src, 0, 0), Some(0.5));
        assert_eq!(shown_gain(&src, 1, 1), Some(1.0));
        assert_eq!(shown_gain(&src, 0, 2), Some(1.0), "centred in the other pair");
    }

    #[test]
    fn a_mono_source_is_split_by_its_pan() {
        assert_eq!(mono_sides(1.0, 0), (1.0, 1.0));
        let (l, r) = mono_sides(1.0, -50);
        assert_eq!(l, 1.0);
        assert!((r - 0.5).abs() < 1e-6);
        let src = RowSource {
            volumes: vec![1.0],
            pans: Some(vec![100]),
            balance: None,
            muted: false,
            phase: false,
        };
        assert_eq!(shown_gain(&src, 0, 0), Some(0.0));
        assert_eq!(shown_gain(&src, 0, 1), Some(1.0));
    }
}

#[cfg(test)]
mod channel_layout_tests {
    use super::*;
    use crate::app::{new, update, Message};

    #[test]
    fn hidden_sources_and_outputs_leave_the_grid() {
        let mut state = new(true, None, None);
        let in_rows = input_row_groups(&state).len();
        let pairs = shown_out_pairs(&state).len();
        // IN3/4 as one strip: hiding it takes its two-row group away.
        let _ = update(&mut state, Message::HideStrip(ChannelId::Input(2)));
        assert_eq!(input_row_groups(&state).len(), in_rows - 1);
        assert!(input_row_groups(&state).iter().all(|g| g.first_idx != 2));
        // A plain output pair goes as a column pair.
        let plain = (0..state.device.outputs().len() / 2)
            .find(|&p| !crate::control_room::is_control_room_pair(&state, p))
            .unwrap();
        let _ = update(&mut state, Message::HideStrip(ChannelId::Output(plain * 2)));
        assert_eq!(shown_out_pairs(&state).len(), pairs - 1);
        assert!(!shown_out_pairs(&state).contains(&plain));
        // The grid still builds.
        let _ = grid_view(&state, iced::Size::new(1200.0, 700.0));
    }
}
