//! A single channel strip: label + type tag, mute/solo, 48V/PAD, fader+VU,
//! dB readout (double-click to edit), pan readout.

use iced::advanced::text as advanced_text;
use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Canvas, Frame, Geometry};
use iced::widget::{button, column, container, mouse_area, row, text, text_input, tooltip};
use iced::{alignment, mouse, Color, Element, Length, Point, Rectangle, Renderer, Theme, Vector};
use std::time::Instant;
use tuxmix_core::ChannelId;

use crate::app::{short_label, Message};
use crate::theme;
use crate::widgets::fader::{fader, vu_meter, Fader, MeterFrame};
use crate::widgets::knob::{knob, Knob};

/// Base sizes at `scale == 1.0` (`theme::SCALE_DEFAULT`) — every dimension
/// in a strip is one of these times `StripParams::scale`, so the window's
/// adaptive scale (see `app::recompute_ui_scale`) resizes strips the same
/// way it resizes text.
pub(crate) const FADER_H: f32 = 170.0;
/// The longest a strip fader grows to on a tall window (see
/// `app::fader_height`): past this a drag covers too much screen for
/// too little precision.
pub(crate) const FADER_MAX: f32 = 340.0;
/// Widened from the original 80 — with `Fader::layout_x` centering a
/// `[track][ruler][meter]` group inside the fader canvas *and* a side
/// icon column (gear/EQ/collapse, see `full_strip`) claiming space in
/// the same row, 80 wasn't wide enough for both: the group overflowed
/// into the icon column's space, clipping the ruler's two-digit ticks
/// under the buttons (confirmed via a live screenshot). 96 gives ~5px
/// of headroom at `scale == 1.0` after both claims.
/// 104 from 96 (2026-11-03): the owner's TotalMix screenshots give the
/// meter a frame of its own and room between it and the fader's lane
/// ("too close ... It doesn't have the right padding"); a row's first
/// strip, with its numbered scale, needed the 8px to hold all of it.
pub(crate) const STRIP_W: f32 = 104.0;
/// What a full strip stacks above its fader: the name line, the pan knob
/// and the M/S row (`full_strip`, at scale 1, hairline spacings aside),
/// plus the mic line on a hardware input (`mic_band`). A collapsed strip
/// gives its name and M/S the same band, so its meter starts where its
/// neighbours' faders do, whatever their length (`StripParams::fader_h`).
const TOP_BAND_H: f32 = theme::TEXT_MD * 1.3 + 2.0 * TAPE_PAD_Y + crate::widgets::knob::BOX_SIZE + MS_HIT_H + PEAK_BAND + MS_GAP;
/// The M|S keys' well, as drawn.
const MS_WELL_H: f32 = 18.0;
/// The band M|S take in the strip: as tall as a desktop click target
/// (24px), the well drawn in its middle; a press in the band beside a key
/// is that key's (`ms_hits`; the audit, 2026-11-05: the keys pressed most
/// were 14px tall).
const MS_HIT_H: f32 = 24.0;
/// The peak readout's capsule under M|S, and the band it takes with the
/// space above it and the rows' hairlines. The space is what the hit band
/// leaves of the 4px there was between the well and the readout.
const PEAK_H: f32 = 16.0;
const PEAK_GAP: f32 = 4.0 - (MS_HIT_H - MS_WELL_H) / 2.0;
const PEAK_BAND: f32 = PEAK_H + PEAK_GAP + 2.0 * theme::SPACE_HAIRLINE;
/// The room under the M|S keys before the meter and the fader (the owner,
/// 2026-11-03: they sat "a bit too close to the sliders"; the reference
/// leaves a clear band there).
const MS_GAP: f32 = 8.0;
/// How much higher a folded strip's M and S stand than its band's foot
/// would put them, so M is level with an open strip's M|S (measured at
/// scale 1, critique #57).
const FOLD_MS_LIFT: f32 = 4.0;
/// The room between the fader and its value readout (the owner,
/// 2026-11-03: the readouts were "way too close" to the faders).
const READOUT_GAP: f32 = 8.0;

/// The extra height of an input's mic line (48V and gain), at scale 1,
/// with the hairline spacing `full_strip`'s column puts after it.
fn mic_band(cid: ChannelId, reserved: bool) -> f32 {
    if reserved || matches!(cid, ChannelId::Input(_)) {
        MIC_LINE_H + theme::SPACE_SM + theme::SPACE_HAIRLINE
    } else {
        0.0
    }
}
/// Collapsed strips are a glance-only readout: name, Mute/Solo (kept
/// live — see this function's own doc comment), and a VU meter; no
/// fader, no pan, no route. Width is set by the header (name + expand
/// button), not the meter, which is narrower than that on its own.
/// Deliberately the same `COLLAPSED_W` for every channel kind, output
/// strips included — collapsing is opting *out* of emphasis, so there's
/// nothing to differentiate here.
///
/// Narrowed from `60` after comparing against a live TotalMix screenshot
/// at pixel level (`Screenshot 2026-08-30 124715.png`): real collapsed
/// strips there measure ~29px against ~75px full strips — a ~0.39 ratio,
/// well under half, not the ~0.6 ours had. `44` keeps that same ratio
/// against the current (wider) `STRIP_W` while still leaving room for
/// the existing `vu_meter` widget's own fixed `METER_RULER_W` (30) plus
/// padding without clipping it — matching TotalMix's exact ~0.39 would
/// need a narrower meter widget too, not attempted this pass.
// 52 (was 44): folded, the name, keys and meter were crammed (the owner,
// 2026-11-14: "the collapse looks way to crammed").
pub(crate) const COLLAPSED_W: f32 = 52.0;


/// The narrow/wide key's height beside the dB readout at the strip's foot
/// (see `full_strip`), which hasn't much room to spare.
const ICON_BTN_H: f32 = 14.0;
/// The side column's keys (T, Ø, the wrench, EQ), and its width: a step
/// up from the narrow key's height, so they read as keys, not glyphs.
const SIDE_KEY: f32 = 18.0;
/// The room between the fader and its side keys (the owner, 2026-11-03:
/// "The buttons are way to close to the slider"; it was 2px). Not scaled,
/// like the row spacings around it. With 18px keys, a row's first strip
/// (its numbered scale) has exactly this much to spare.
const SIDE_KEY_GAP: f32 = 5.0;
/// The air between a fader's lane and its side keys, and between the keys
/// and the strip's padding (the reviewer: 15 and 6 px, so the keys looked
/// pushed out; now 10, and 3 plus the card's 6px padding to its edge), at
/// scale 1.
const LANE_TO_KEYS: f32 = 10.0;
const KEYS_TO_EDGE: f32 = 3.0;
/// A side key's press band, wide and tall, at every zoom (issue #21, the
/// owner's yes: "its hard to click on"; the band was as wide as the key,
/// 18px, and every band shrank with the zoom). Around its key, which keeps
/// its drawn size and place: the band reaches `SIDE_OVERHANG` toward the
/// lane, the rest toward the card's edge.
const SIDE_BAND: f32 = 24.0;
/// How far a side key's band reaches past it toward the lane, at scale 1:
/// half the band's extra at 100 %, so the lane keeps its air.
const SIDE_OVERHANG: f32 = (SIDE_BAND - SIDE_KEY) / 2.0;

/// A side key's band in pixels: `SIDE_BAND` at 100 % and up, never less
/// zoomed out.
fn side_band(scale: f32) -> f32 {
    (SIDE_BAND * scale).max(SIDE_BAND)
}

/// The width a strip with side keys takes beyond `STRIP_W` at its zoom:
/// none at 100 % and up; zoomed out, what its bands need past the scaled
/// strip (issue #21), on its right, so nothing else moves.
pub(crate) fn side_band_extra(scale: f32) -> f32 {
    side_band(scale) - SIDE_BAND * scale
}

/// A side key and the gap to the next, in pixels: as drawn, or a band's
/// height where a band is taller (zoomed out).
fn side_pitch(scale: f32) -> f32 {
    ((SIDE_KEY + SIDE_KEY_SPACING) * scale).max(side_band(scale))
}
/// What a row's first strip adds for its scale's numbers, so its meter
/// keeps the same air to its lane as every other strip's (the reviewer:
/// 5px there, under the 8px floor).
pub(crate) const RULER_EXTRA: f32 = 16.0;
/// How far apart the side keys stand, and how far the last stands up
/// from the fader's foot (in its lower half, as the reference's do).
const SIDE_KEY_SPACING: f32 = 8.0;
const SIDE_KEY_FOOT: f32 = 12.0;
/// The narrow switch's cap, at scale 1 (the owner, 2026-11-07: at 14px
/// "its too small"). Its slot is whatever the readout row leaves after the
/// value box, on to the card's edge (`edge_switch`).
const EDGE_KNOB_W: f32 = 18.0;
/// The card's frame at its widest (a selected card's), at scale 1.
const FRAME_W: f32 = 1.5;

/// The readout row's height: the value box.
fn readout_h(scale: f32) -> f32 {
    ICON_BTN_H.max(theme::TEXT_NUM + theme::SPACE_SM) * scale
}

/// The foot row's height: the routing field as it draws (a 12px line at
/// 1.3, its 2px padding above and below), and the narrow switch beside it
/// at the same height (the owner, 2026-11-13: "make their height the
/// same"), on an open card and a folded one alike.
fn foot_h(scale: f32) -> f32 {
    (theme::TEXT_SM * 1.3 + 2.0 * theme::SPACE_TIGHT) * scale
}

/// The narrow key as a switch on the card's right edge (the owner,
/// 2026-11-07: "It's like a switch", on "the side of the container";
/// critique #15: a switch shows both its positions). A slot cut into the
/// card's edge, rounded on its inner end and running off the border, a
/// dark knob in it with the triangle: at the edge end, pointing in, while
/// the strip is open; at the inner end, pointing out, once it is folded,
/// on the folded card's foot too. `open` is how open the strip is, 0..1,
/// so the knob slides as the card narrows or widens.
fn edge_switch<'a>(cid: ChannelId, open: f32, slot_w: f32, band: f32, h: f32, scale: f32, tip: &'static str) -> Element<'a, Message> {
    let pad = theme::WELL_PAD * scale;
    // The slot stops inside the card's frame (its widest, a selected
    // card's 1.5px), so the frame and its highlight run unbroken past it.
    let slot_w = slot_w - FRAME_W * scale;
    // The lip under the slot takes a pixel of the row's height.
    let lip = scale.max(1.0);
    let slot_h = h - lip;
    let slot_r = slot_h / 2.0;
    // Never wider than its slot leaves room for (critique #17).
    let knob_w = (EDGE_KNOB_W * scale).min((slot_w - pad).max(0.0));
    let knob_r = (slot_h - 2.0 * pad) / 2.0;
    // Clear of the slot's walls at its inner end; flat against its end at
    // the edge, as TotalMix's cap sits against the strip's edge.
    let travel = (slot_w - knob_w - pad).max(0.0);
    let open = open.clamp(0.0, 1.0);
    // Past halfway folded the arrow turns; only fully open does the cap
    // sit flat against the slot's end.
    let folded = open < 0.5;
    let slot = container(row![
        // The dark floor either side is the rail (TotalMix's: no line).
        iced::widget::Space::new().width(Length::Fixed(travel * open)),
        // The arrow lit: full once folded, softly while open (the owner's
        // pick from mockups, a cream cap with its arrow lit).
        button(crate::widgets::strip_icons::edge_arrow(
            !folded,
            // The lamp's hue deepened until it reads on the cap itself, at
            // full strength open or folded (the owner, 2026-11-10: "hard to
            // see the button": light amber, softened while open, all but
            // vanished on the cream cap).
            theme::readable_on(theme::lamp(), iced::Color::BLACK, &[theme::fader_metal()]),
            scale,
        ))
            .padding(0)
            .width(Length::Fixed(knob_w))
            .height(Length::Fill)
            .style(theme::edge_knob(knob_r, open >= 1.0 - 1e-3))
            .on_press(Message::ToggleCollapse(cid)),
    ])
    .padding(iced::Padding { top: pad, bottom: pad, left: pad, right: 0.0 })
    .width(Length::Fixed(slot_w))
    .height(Length::Fixed(slot_h))
    .style(theme::edge_slot(slot_r));
    // The whole slot presses it, and `band` of air above it too (the
    // owner, 2026-11-07: "its hard to click on"; the cap alone was about
    // 14x13px, under the 24px floor for a target), the cap's own press
    // taking it first so a click never toggles twice.
    hint(
        mouse_area(
            container(
                container(slot)
                    .padding(iced::Padding { bottom: lip, ..iced::Padding::ZERO })
                    .style(theme::edge_lip(slot_r)),
            )
            .padding(iced::Padding { top: band, ..iced::Padding::ZERO }),
        )
        .on_press(Message::ToggleCollapse(cid))
        .interaction(mouse::Interaction::Pointer),
        tip,
        scale,
    )
}

/// `el` with a discovery shimmer over it while `shimmer` is `which`: a band
/// of the lamp's light passing across (`theme::glint`), drawn over the
/// control and taking no clicks.
fn shimmered<'a>(
    el: Element<'a, Message>,
    shimmer: Option<(crate::motion::Shimmer, f32)>,
    which: crate::motion::Shimmer,
    radius: f32,
) -> Element<'a, Message> {
    match shimmer {
        Some((s, t)) if s == which => iced::widget::stack![
            el,
            container(iced::widget::Space::new()).width(Length::Fill).height(Length::Fill).style(theme::glint(t, radius)),
        ]
        .into(),
        _ => el,
    }
}

/// Between the FX mini fader and the first key under it, and the least
/// height the mini fader takes (under it, an FX key stands in its place,
/// where the stack has room for one).
const MINI_GAP: f32 = 6.0;
const MINI_MIN_H: f32 = 36.0;
/// The mic line under an input's name (48V switch and gain readout).
const MIC_LINE_H: f32 = 22.0;
const MIC_BADGE_W: f32 = 40.0;


/// Width (at `scale == 1.0`) of the EQ flyout — wide enough for the
/// three-column band matrix (type/gain/freq/Q) plus its row labels, and
/// the response plot above it.
/// 280 from 248 (2026-11-03, the owner: the EQ was "still to crammed
/// together").
pub(crate) const EQ_FLYOUT_W: f32 = 280.0;

/// The least width (at `scale == 1.0`) an input strip's Settings panel
/// needs for the mic presets' names and menu (2026-11-04). It was the
/// strip's own width, after the reference; it floats over the strips
/// beside it, so where a strip is narrower it takes two strips' width and
/// the gap between, its edge falling between strips (`app.rs`).
pub(crate) const INPUT_SETTINGS_MIN_W: f32 = 168.0;

/// One entry of a strip's routing field (manual 21.3): an output pair,
/// its name, and whether this channel already reaches it. In the open list
/// active routings carry an arrow, as the manual's do ("all activated
/// routings ... by arrows in front of the listed entries", 21.3); the
/// closed field shows the name alone (the owner, 2026-11-06:
/// "I dont like the little period on the left"). Entries are the same
/// entry when their pair is.
#[derive(Clone, Debug)]
pub struct RouteChoice {
    pub pair: usize,
    pub label: String,
    pub active: bool,
    /// Shown in the closed field: the name alone, led by `lead` figure
    /// spaces so it stands in the middle of the field (the list can't
    /// centre it, and padding would indent the open list too).
    pub in_field: bool,
    pub lead: usize,
}

impl PartialEq for RouteChoice {
    fn eq(&self, other: &Self) -> bool {
        self.pair == other.pair
    }
}

impl Eq for RouteChoice {}

impl std::fmt::Display for RouteChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.in_field {
            write!(f, "{}{}", "\u{2007}".repeat(self.lead), self.label)
        } else if self.active {
            // A slim mark and no gap: the list is as wide as its key, and
            // "\u{25b8} Phones 3/4" lost its last figure there (critiques
            // #97 and #98 B).
            // And a hair space between the word and its number: with the
            // field's padding gone the "4" still met the rim (critique #99
            // B), and with a thin space it had 1 px (critique #100 B).
            write!(f, "\u{203a}{}", self.label.replace(' ', "\u{200a}"))
        } else {
            write!(f, "\u{2009}{}", self.label.replace(' ', "\u{200a}"))
        }
    }
}

/// Which flyout (at most one, and only one per strip) is open — the gear
/// icon opens `Settings` (48V/PAD/Sensitivity, moved out of the strip's
/// own vertical flow so it doesn't grow/shrink the whole card; Gain stays
/// inline instead — see `app.rs::settings_popover`'s doc comment for why),
/// the EQ trigger (analog inputs only) opens `Eq` (3-band + low-cut, see
/// `app.rs::eq_popover`). (`Trim` is the old Trim card's, kept for stored
/// layout presets; nothing opens it.) Each opens beside its strip and pushes the rest of the row over (see
/// `mixer_view`'s row loops). Which mix is being edited is picked in the
/// counter's placard or by clicking an output, not from a strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FlyoutKind {
    Settings,
    Eq,
    /// The D panel (issue #27): compressor, expander, Auto Level.
    Dynamics,
    /// The old Trim card (until 2026-11-06, when T became Trim Gains and the
    /// input's trim a Settings row). Nothing opens it; kept so a layout
    /// preset stored with it open still loads.
    Trim,
}

/// The full (uncollapsed) width for a given channel — every channel kind
/// renders at the same width. Takes `cid` (unused) rather than being a
/// bare constant so `app.rs::set_collapsed`/`rendered_strip_width` don't
/// need to change if a future kind ever needs to differ again.
pub(crate) fn full_width(_cid: ChannelId) -> f32 {
    STRIP_W
}

/// How long a strip's collapse/expand width transition takes — longer than
/// the meter's 50ms interp window since this is a much bigger, structural
/// change (the whole card growing or shrinking), not a small value nudge.
/// Same linear ease-style interpolation as `fader::MeterFrame`, just a
/// dedicated type since the duration and the thing being interpolated
/// (a pixel width, not a volume/pan value) are both different.
const COLLAPSE_INTERP_MS: f32 = 160.0;

#[derive(Clone, Copy, Debug)]
pub struct CollapseAnim {
    pub prev: f32,
    pub value: f32,
    pub since: Instant,
    /// The step before the card changes width, in ms (0: none): the
    /// contents it is leaving fade out under a veil in the card's own face,
    /// the card keeping its width meanwhile (the owner, 2026-11-13: "There
    /// should be a closing animation before the collapse"; 2026-11-14:
    /// "make the dropdown fade out first ... similar to all other
    /// controls", then "still dont look nice").
    pub close_ms: f32,
    /// The step after: the contents it arrives at fade in, at their own
    /// width (0: none). Between the two the card resizes empty, so neither
    /// layout is ever squeezed (the folded one had snapped in at full width
    /// and been squeezed as the card narrowed).
    pub open_ms: f32,
}

/// A fold's fading-out step (`CollapseAnim::close_ms`).
pub const CLOSE_MS: f32 = 150.0;
/// An unfold's: the folded rail's few keys go quicker.
pub const UNFOLD_CLOSE_MS: f32 = 90.0;
/// Fading in after either (`CollapseAnim::open_ms`).
pub const OPEN_MS: f32 = 140.0;

impl CollapseAnim {
    fn ms(&self, now: Instant) -> f32 {
        now.duration_since(self.since).as_secs_f32() * 1000.0
    }

    pub fn at(&self, now: Instant) -> f32 {
        let t = ((self.ms(now) - self.close_ms) / COLLAPSE_INTERP_MS).clamp(0.0, 1.0);
        self.prev + (self.value - self.prev) * t
    }

    pub fn is_settling(&self, now: Instant) -> bool {
        (self.value - self.prev).abs() > f32::EPSILON && self.ms(now) < self.close_ms + COLLAPSE_INTERP_MS + self.open_ms
    }

    /// How opaque the veil over the contents is: rising through the
    /// fading-out step, whole while the card resizes, falling through the
    /// fading-in step (0 with neither step).
    pub fn veil(&self, now: Instant) -> f32 {
        if (self.value - self.prev).abs() <= f32::EPSILON {
            return 0.0;
        }
        let ms = self.ms(now);
        let ease = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let resized = self.close_ms + COLLAPSE_INTERP_MS;
        if ms < self.close_ms {
            ease(ms / self.close_ms)
        } else if ms < resized {
            if self.close_ms > 0.0 || self.open_ms > 0.0 { 1.0 } else { 0.0 }
        } else if self.open_ms > 0.0 {
            1.0 - ease((ms - resized) / self.open_ms)
        } else {
            0.0
        }
    }

    /// Whether the card still shows the contents it is leaving (through
    /// the fading-out step), not the ones it arrives at.
    pub fn leaving(&self, now: Instant) -> bool {
        self.close_ms > 0.0 && self.ms(now) < self.close_ms
    }
}

/// A strip's FX mini fader (TotalMix's, manual 21.3; the owner,
/// 2026-11-06: "the little bars sliders are send / returns from wet and
/// dry signal"): an input's or a playback's send to Reverb & Echo, an
/// output's return from them. Set by `app::finish_params`.
#[derive(Clone, Debug)]
pub struct FxMini {
    /// The level as the mini fader's travel, 0 (off) to 1.
    pub t: f32,
    /// The send (to the effects); else the return into output pair `pair`.
    pub send: bool,
    pub pair: usize,
    /// The driver has this level on this strip; else the slot rests greyed.
    pub live: bool,
    pub tip: String,
    /// An output's return held silent by Mute FX (its key takes the mute
    /// hue, `theme::side_key_held`).
    pub held: bool,
    /// The pointer on the Mute FX key that holds it (or would): it wears
    /// the link ring (`theme::link_ring`).
    pub linked: bool,
}

pub struct StripParams<'a> {
    pub cid: ChannelId,
    pub output_idx: usize,
    /// Whether the connection this strip is drawn for can drive RME's own
    /// protocol (see `RmeDevice::protocol_features`). False on the
    /// class-compliant personality, where EQ, Trim, Phase, Split and
    /// Loopback would only be able to fail: they are left out entirely,
    /// the same thing the `has_*` flags above already do for features a
    /// channel does not have.
    pub protocol_features: bool,
    pub name: String,
    pub type_tag: Option<(&'static str, Color)>,
    /// How many lines the header gives the name (`name_lines`): the most
    /// any strip in this row needs, so the row's faders stay level.
    pub name_lines: u8,
    /// Plain words for what the channel is, under its name (`app::plain_role`).
    pub plain: Option<&'static str>,
    /// The words for the name's hover, at any window size (with where a
    /// playback plays).
    pub plain_tip: Option<String>,
    /// Reserve the inputs' mic line (empty) on a strip that has none: a
    /// playback strip standing beside the inputs in two-row mode, so the
    /// two blocks' pans, faders and readouts line up.
    pub mic_line: bool,
    /// The meter's "speak here" bracket (`fader::Fader::speak_band`).
    pub speak_band: bool,
    /// The meter rung while the past-0 dB card names it (`fader::Fader::meter_named`).
    pub meter_named: bool,
    /// A small win's sparks flying from the strip's head, 0..1
    /// (`motion::win_spark`).
    pub win_spark: Option<f32>,
    /// How bright the M, S and 48V toggles' bloom is (`motion::bloom`):
    /// 1 the moment one lights, easing to 0.
    pub bloom_mute: f32,
    pub bloom_solo: f32,
    pub bloom_48v: f32,
    /// How much light is still going out of M, S and 48V, just switched
    /// off (`motion::cool`).
    pub cool_mute: f32,
    pub cool_solo: f32,
    pub cool_48v: f32,
    /// The same for an output's CUE and LOOP.
    pub bloom_cue: f32,
    pub cool_cue: f32,
    pub bloom_loop: f32,
    pub cool_loop: f32,
    /// How lit the gain readout is: the unit just changed the gain
    /// (`motion::gain_flash`).
    pub gain_flash: f32,
    /// 48V clicked once and waiting for the second (`app::phantom_armed`).
    pub armed_48v: bool,
    /// In the block a Mackie Control surface drives (`mackie::bank`).
    pub remote_bank: bool,
    /// An output's LOOP clicked once and waiting, and what LOOP does here
    /// (which input's recording it replaces) for its tooltip.
    pub armed_loop: bool,
    pub loop_note: String,
    /// Trim Gains: the channel's other active routings, for the fader's
    /// marks (`Fader::marks`).
    pub trim_marks: Vec<f32>,
    /// The outputs this strip can edit, for its routing field (manual
    /// 21.3): every input and playback strip has one. Empty on outputs.
    pub routes: Vec<RouteChoice>,
    /// The Free Routing Mode: a pick in the routing field moves this strip
    /// alone; else it picks the mix being edited.
    pub free_route: bool,
    /// The FX mini fader beside the fader; `None` where the strip has no
    /// room for it (a short fader) or no FX at all.
    pub fx_mini: Option<FxMini>,
    /// The row shows FX as a key on every strip: one strip in it has no
    /// room for the mini fader (critique #47, the owner's pick: a row with
    /// a key on one strip and a fader on the next read as three things).
    pub fx_key: bool,
    /// Live Mode: the S button does PFL (its tooltip says so).
    pub pfl_mode: bool,
    /// Trim Gains is on for every strip (the View page): T stands lit and
    /// is not the strip's to change.
    pub trim_all: bool,
    /// Trim Gains is on for this strip: its T stands lit, as the EQ key
    /// does while the EQ is on.
    pub trim_gains: bool,
    /// The faders show FX sends (`TuxMix::fader_fx`): this fader sets its
    /// send (an output's, its return), amber, and its moves are
    /// `Message::FxFaderDrag`.
    pub fx_lit: bool,
    pub vol: f32,
    pub pan: i8,
    /// The pan (a balance, on a stereo pair) is one the unit's driver can
    /// set (`RmeDevice::can_pan`); otherwise the knob rests inert and says
    /// why, instead of failing when turned.
    pub can_pan: bool,
    pub meter: MeterFrame,
    /// Whether `meter` is a real reading on the active backend — see
    /// `fader::draw_meter`'s doc comment.
    pub meter_available: bool,
    /// Set only when this strip represents a LINKED pair — the right
    /// channel's own meter, splitting the meter column into two thin
    /// bars (see `fader::Fader::meter2`'s own doc comment). `None` for
    /// every unlinked/mono strip.
    pub meter2: Option<MeterFrame>,
    pub meter2_available: bool,
    /// The clip LED of `meter` / `meter2` is held lit (see `Fader::clip`).
    pub clip: bool,
    pub clip2: bool,
    pub has_48v: bool,
    pub has_pad: bool,
    pub phantom: bool,
    pub pad: bool,
    /// Whether this input has a preamp gain control at all (Mic and
    /// Instrument only — Line/SPDIF/ADAT inputs have no gain knob).
    pub has_gain: bool,
    /// Raw hardware gain units, not dB — see `Message::Gain`.
    pub gain: u32,
    pub gain_max: u32,
    /// Whether this input has a sensitivity switch (Instrument only).
    pub has_sensitivity: bool,
    /// `true` = +4dBu, `false` = -10dBV.
    pub sensitivity_plus4: bool,
    /// Whether this input has an EQ strip at all (the 4 analog inputs
    /// only — see `InputChannel::eq`).
    pub has_eq: bool,
    /// Whether the EQ is currently engaged — lights up the trigger button
    /// even when its flyout isn't open, like the 48V/PAD toggles.
    pub eq_enabled: bool,
    /// A D key (issue #27): every hardware input and output.
    pub has_dyn: bool,
    /// Its dynamics on and shaping: the D key lit.
    pub dyn_on: bool,
    /// The one-time hint points at this strip's D key (critique #31's
    /// "B"): a ring in the lamp's light until it's answered.
    pub dyn_hint: bool,
    /// How far its dynamics turn it down now (0 or less), for the marks
    /// on its meter.
    pub dyn_reduction_db: f32,
    /// Whether this input has a Phase Ø invert switch — the 4 analog
    /// inputs only (`InputChannel::eq.is_some()` already means exactly
    /// that set, reused rather than a parallel gate that could drift).
    pub has_phase: bool,
    pub phase: bool,
    /// Output-only: `OutputChannel::loopback`.
    pub loopback: bool,
    /// Whether this channel's pair is currently a linked stereo bus
    /// (`RmeDevice::{input_pair,playback,output}_linked`, whichever
    /// matches `cid`'s kind) — TotalMix's default, shown/toggled from
    /// the Settings flyout for every channel kind.
    pub stereo_linked: bool,
    /// Which flyout (if any) is currently open *for this strip* — lights
    /// up the matching trigger (gear icon for `Settings`, the route
    /// button for `Route`); `app.rs` owns the actual popover content.
    pub open_flyout: Option<FlyoutKind>,
    /// Its Settings panel was opened by its FX key (`TuxMix::fx_opened`).
    pub fx_opened: bool,
    pub mute: bool,
    pub solo: bool,
    /// Off, but held for the sidebar's Mute / Solo master to bring back
    /// (`theme::armed_toggle_button`). Set by `app::finish_params`.
    pub mute_armed: bool,
    pub solo_armed: bool,
    /// `true` only for `ChannelId::Output` — every other channel kind
    /// has no CUE bus of its own (see `RmeDevice::set_cue`'s doc
    /// comment: CUE always monitors *some output's* dedicated
    /// playback pair through AN1/2, not an input/playback concept).
    pub has_cue: bool,
    pub cue: bool,
    pub default_vol: f32,
    pub editing: bool,
    pub edit_buf: &'a str,
    /// The name field is open for typing (`Message::RenameStart`), with
    /// what has been typed so far. Set by `app::finish_params`.
    pub renaming: bool,
    pub rename_buf: &'a str,
    pub drag_range: Option<(f32, f32)>,
    pub modifiers: Modifiers,
    pub collapsed: bool,
    pub collapse_anim: Option<CollapseAnim>,
    /// Offers collapsing (the minus button, a double-click on the card).
    /// Off in the Quick view, whose two strips are meant to be large.
    pub can_collapse: bool,
    pub scale: f32,
    pub selected: bool,
    /// The strip the keyboard is on (`TuxMix::focused_strip`): a focus rim.
    pub focused: bool,
    pub hovered: bool,
    /// An output strip whose submix the input and playback faders are
    /// currently editing; drawn brighter so the target is always visible.
    pub submix_active: bool,
    /// How far this strip stands in the lamp pool, 0 to 1 (see
    /// `app::lamp_target`): lit when it feeds the mix being edited, or is
    /// that mix's output. Fractional only while fading between submixes.
    pub lamp: f32,
    /// The active path's flare (`motion::path_pulse`), 0..1, over the
    /// lamp's glow.
    pub path_pulse: f32,
    /// A discovery shimmer passing over one of this strip's controls
    /// (`motion::shimmer`): which, and how far across, 0..1.
    pub shimmer: Option<(crate::motion::Shimmer, f32)>,
    /// This strip carries its row's dB scale (see `Fader::ruler_labels`):
    /// set by the row builders on the row's first full strip.
    pub ruler_labels: bool,
    /// The fader's length at scale 1: `FADER_H` on a short window, up to
    /// `FADER_MAX` on a tall one (`app::fader_height`). A collapsed
    /// strip's meter takes the same length, so its scale lines up.
    pub fader_h: f32,
}

/// A button's own padding-based centering isn't reliable across glyphs of
/// different intrinsic width (e.g. "S" sat visibly left of center while "M"
/// looked fine) — force it explicitly instead of trusting the default. The
/// default 1.2x line-height also reserves descender space these glyphs
/// (M, S, no descenders) never use, which reads as "sitting too high" once
/// centered — tightening it to 1:1 removes that residual vertical bias.
fn centered_label<'a>(s: &'a str, size: f32) -> Element<'a, Message> {
    container(
        // Block lettering on a block key: the semibold cut, so M, S and
        // 48V read at a glance like a console's printed keys.
        text(s)
            .font(theme::SANS_SEMI)
            .size(size)
            .line_height(iced::widget::text::LineHeight::Absolute(iced::Pixels(
                size,
            ))),
    )
    .center(Length::Fill)
    .into()
}

/// Wraps a control in a hover tooltip — for the abbreviations (M, S, 48V,
/// PAD) that read as pro-audio jargon to anyone not already fluent in it.
/// A short delay so it doesn't flash on every incidental mouse-over while
/// moving across the strip toward something else.
pub(crate) fn hint<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    scale: f32,
) -> Element<'a, Message> {
    hint_text(content, label.to_string(), scale)
}

/// `hint` with text made for this strip (it owns it).
pub(crate) fn hint_text<'a>(
    content: impl Into<Element<'a, Message>>,
    label: String,
    scale: f32,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label).size(theme::TEXT_SM * scale).color(theme::text_primary()))
            .padding(theme::SPACE_SM * scale)
            .style(crate::widgets::hint_panel),
        tooltip::Position::Top,
    )
    .gap(4.0 * scale)
    .delay(crate::widgets::hint_delay())
    .into()
}


/// A read-only text canvas rotated -90° (reads bottom-to-top) — the
/// collapsed strip's own name label. Confirmed against a live TotalMix
/// screenshot: collapsed buses are narrow enough (see `COLLAPSED_W`'s own
/// doc comment) that a normal horizontal name doesn't fit at all, so
/// TotalMix rotates it instead of truncating or wrapping it. iced's plain
/// `text()` widget has no rotation of its own, hence a small dedicated
/// canvas rather than reusing `centered_label`.
struct RotatedLabel {
    text: String,
    size: f32,
}

impl<Message> canvas::Program<Message> for RotatedLabel {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        frame.with_save(|frame| {
            frame.translate(Vector::new(center.x, center.y));
            // -90°: in this canvas's clockwise-from-+x, y-down convention
            // (matching `iced_graphics::geometry::path::Arc` and
            // `widgets::knob`'s own `angle_of`), this maps local "reading
            // direction" (+x) to global "up" — i.e. the text climbs the
            // strip bottom-to-top, matching the reference exactly rather
            // than guessed-and-checked.
            frame.rotate(-std::f32::consts::FRAC_PI_2);
            frame.fill_text(canvas::Text {
                content: self.text.clone(),
                // Rotated, the reading direction runs along the band's
                // height: a name longer than that wraps onto a second line
                // beside the first instead of being cut off.
                max_width: bounds.height,
                position: Point::ORIGIN,
                color: theme::text_primary(),
                size: self.size.into(),
                align_x: advanced_text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        });
        vec![frame.into_geometry()]
    }
}

fn rotated_label<'a, Msg: 'a + Clone>(text: String, size: f32) -> Element<'a, Msg> {
    // Fills what its band leaves it (see `collapsed_strip`).
    Canvas::new(RotatedLabel { text, size })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Full-strip header — just the name (+ type tag), centered, TotalMix-style
/// (see the reference design: no icons in the header at rest). See
/// `collapsed_header_row` for the collapsed strip's own header, which
/// still needs its chevron.
///
/// Double-clicking the name renames the strip (`Message::RenameStart`):
/// while `renaming`, the name is a text field, Enter keeps the typed name
/// and Escape drops it. The name's own `mouse_area` captures its clicks
/// (iced's `mouse_area` captures every press once it has a double-click
/// handler), so it forwards single clicks as `StripClicked` itself; that
/// keeps Ctrl/Shift-click selection working on the name, and stops a
/// double-click there from also collapsing the strip.
/// The most lines a strip header may take: up to three for the name, one
/// for its type tag.
const MAX_NAME_LINES: u8 = 4;

/// An estimate of `chars` characters' width at `size` px in the strip's
/// faces (Atkinson Hyperlegible Next runs about 0.53 em a character; the
/// margin keeps a name from touching the card's edge).
fn text_w(chars: usize, size: f32) -> f32 {
    chars as f32 * size * 0.56
}

/// The width a strip's header has for its name, at scale 1: the strip's
/// own padding, then the tape's (`TAPE_PAD_X` each side and its 1 px edge).
fn header_room() -> f32 {
    STRIP_W - 2.0 * theme::SPACE_MD - 2.0 * (TAPE_PAD_X + 1.0)
}

/// The name's size on its tape: a step under `TEXT_MD`, so "Main 1/2" and a
/// DIM beside it stay on one line (Fable's review: at 14 px the serif ran
/// DIM past the tape, and wrapped it would move the row on a Dim press).
pub(crate) const TAPE_NAME: f32 = 13.0;
/// A state word's size on its chip on the tape, in the semibold (Fable's
/// re-review: at 10 px regular, DIM on a light theme read quieter than it
/// did before the tape).
pub(crate) const TAPE_STATE: f32 = 11.0;
/// The tape's padding across, and up and down: none up and down, so the
/// tape fills the name's line (13 px type in the 18 px line) and no row
/// grows (a 4 px taller tape lost the second open row at 1600x900).
pub(crate) const TAPE_PAD_X: f32 = theme::SPACE_TIGHT;
/// The most room between a name and a state word's chip (PAD, DIM, LOOP)
/// on the tape: at `SPACE_TIGHT` the chip touched the name's last figure
/// (the owner on #39: "AN1/2 PAD" looked crammed).
pub(crate) const TAPE_CHIP_GAP: f32 = theme::SPACE_SM + theme::SPACE_TIGHT;

/// The gap before `tag` on a one-line tape, at scale 1: a state chip takes
/// what the tape has spare, up to `TAPE_CHIP_GAP`, so the room the fit
/// counts (`fits_one_line`, at `SPACE_TIGHT`) never changes and no row
/// re-lays ("Main 1/2" and DIM fit with half a pixel spare). A quiet type
/// word keeps the tight gap.
fn tag_gap(name: &str, tag: &str) -> f32 {
    if !is_state_word(tag) {
        return theme::SPACE_TIGHT;
    }
    let spare = header_room() - name_w(short_label(name), TAPE_NAME) - state_chip_w(tag);
    spare.clamp(theme::SPACE_TIGHT, TAPE_CHIP_GAP)
}
pub(crate) const TAPE_PAD_Y: f32 = 0.0;

/// The width of `name` in the tape's serif at `size`, a little over the
/// measured: Shippori Mincho's capitals and figures run wide (about 0.86 em;
/// "ADAT7/8" measured 80 px at 14), its lower case about 0.5 (i, l, t, f, r
/// and j 0.3, m and w 0.78; "Main 1/2" measured 61 px at 14), spaces 0.3
/// and strokes 0.36.
fn name_w(name: &str, size: f32) -> f32 {
    name.chars()
        .map(|c| match c {
            c if c.is_ascii_uppercase() || c.is_ascii_digit() => 0.86,
            'i' | 'l' | 'j' | 't' | 'f' | 'r' => 0.30,
            'm' | 'w' => 0.78,
            c if c.is_alphabetic() => 0.50,
            ' ' => 0.30,
            _ => 0.36,
        })
        .sum::<f32>()
        * size
}

/// Whether a header tag is a state word (on its dark chip) rather than a
/// quiet type word (ink on the tape).
pub(crate) fn is_state_word(tag: &str) -> bool {
    tag == "DIM" || tag == "PAD" || tag == "LOOP" || state_tag_hint(tag).is_some() || sides_mark_word(tag).is_some()
}

/// A state word's chip on the tape: the word at `TAPE_STATE` and its padding.
fn state_chip_w(word: &str) -> f32 {
    word.chars().count() as f32 * TAPE_STATE * 0.62 + 2.0 * theme::SPACE_TIGHT
}

/// The type tag a strip's header shows after its name: none for "OUT"
/// (the outputs' own floor says it) or for a tag the name already spells
/// ("ADAT3/4" + "ADAT").
pub(crate) fn shown_tag(name: &str, type_tag: Option<(&'static str, Color)>) -> Option<&'static str> {
    let upper = short_label(name).to_ascii_uppercase();
    type_tag
        .map(|(tag, _)| tag)
        // Not OUT or PB either: the row's own heading says it (on the tape
        // "ADAT3/4 PB" ran past its end in the serif).
        .filter(|tag| *tag != "OUT" && *tag != "PB" && (state_tag_hint(tag).is_some() || !upper.contains(tag)))
}

/// A tag that says a state the unit is in, not the channel's kind (M/S
/// running, a width off +1.00), with what it means on hover; always shown,
/// in LOOP's live coral (critique #22: in a type word's grey, and dropped
/// when the strip's name spelled it, it read as a channel type).
pub(crate) fn state_tag_hint(tag: &str) -> Option<&'static str> {
    let tag = sides_mark_word(tag).map_or(tag, |(_, mark)| mark);
    Some(match tag {
        "M/S" => "M/S processing is on: left plays the middle, right the sides (its Settings)",
        "DIM" => "Dim is on: this output plays quieter than its fader. Press Dim to bring it back",
        "PAD" => "PAD is on: this mic comes in 11 dB quieter, for a loud source. Its Settings (the wrench) turn it off",
        // Phase on, with its key given to FX (critique #48: drawn grey, as
        // "MIC", and with no hint).
        "\u{d8}" => "Phase invert is on: this channel's signal is flipped. Its Settings (the wrench) turn it off",
        "W" => "Width narrowed from +1.00 (its Settings)",
        "MONO" => "Width at 0: both sides the same (its Settings)",
        "L\u{21c4}R" => "Width below 0: left and right swapped (its Settings)",
        LEFT_LOUDER => "Its left side is louder than its right in this mix (set one alone in the Matrix, with edit pairs: mono). The fader shows the left; moving it moves both and keeps the difference",
        RIGHT_LOUDER => "Its right side is louder than its left in this mix (set one alone in the Matrix, with edit pairs: mono). The fader shows the left; moving it moves both and keeps the difference",
        _ => return None,
    })
}

/// The marks of a linked pair whose two sides stand at different levels in
/// the mix shown (OpenCode's job 39: the Matrix's Mono Mode sets one side
/// alone, and the strip, which shows the left, couldn't say so): which side
/// is louder. "L≠R" said only that they differ, and its ≠ blurred at 12 px
/// (critique #38); < and > draw crisp and say more.
pub(crate) const LEFT_LOUDER: &str = "L>R";
pub(crate) const RIGHT_LOUDER: &str = "L<R";

/// The type word with the side mark after it ("PB L>R"), one string so the
/// header measures both (`fits_one_line`, `name_lines`) and the type word
/// stays (critique #38: in its place, the playback row's "AN1/2" read as
/// the mic pair).
pub(crate) fn with_sides_mark(word: &'static str, left_louder: bool) -> &'static str {
    match (word, left_louder) {
        ("PB", true) => "PB L>R",
        ("PB", false) => "PB L<R",
        ("PAD", true) => "PAD L>R",
        ("PAD", false) => "PAD L<R",
        ("MIC", true) => "MIC L>R",
        ("MIC", false) => "MIC L<R",
        ("INST", true) => "INST L>R",
        ("INST", false) => "INST L<R",
        ("LINE", true) => "LINE L>R",
        ("LINE", false) => "LINE L<R",
        ("ADAT", true) => "ADAT L>R",
        ("ADAT", false) => "ADAT L<R",
        ("SPDIF", true) => "SPDIF L>R",
        ("SPDIF", false) => "SPDIF L<R",
        (_, true) => LEFT_LOUDER,
        (_, false) => RIGHT_LOUDER,
    }
}

/// A tag that carries a side mark: its type word and the mark.
fn sides_mark_word(tag: &str) -> Option<(&str, &'static str)> {
    tag.strip_suffix(" L>R")
        .map(|w| (w, LEFT_LOUDER))
        .or_else(|| tag.strip_suffix(" L<R").map(|w| (w, RIGHT_LOUDER)))
}

/// How many lines `name` takes wrapped at word breaks in a strip's header.
fn wrapped_lines(name: &str) -> u32 {
    let room = header_room();
    let space = name_w(" ", TAPE_NAME);
    let mut lines: u32 = 1;
    let mut line_w = 0.0f32;
    for word in short_label(name).split_whitespace() {
        let w = name_w(word, TAPE_NAME);
        if line_w > 0.0 && line_w + space + w <= room {
            line_w += space + w;
            continue;
        }
        if line_w > 0.0 {
            lines += 1;
        }
        // A word wider than the room breaks inside itself.
        let extra = ((w / room).ceil() as u32).saturating_sub(1);
        lines += extra;
        line_w = w - extra as f32 * room;
    }
    lines
}

/// Whether `name` and `tag` fit on one header line.
fn fits_one_line(name: &str, tag: Option<&str>) -> bool {
    let name_w = name_w(short_label(name), TAPE_NAME);
    let tag_w = tag.map_or(0.0, |t| theme::SPACE_TIGHT + if is_state_word(t) { state_chip_w(t) } else { text_w(t.chars().count(), theme::TEXT_SM) });
    name_w + tag_w <= header_room()
}

/// How many lines a strip header needs for `name` and its `tag` (1 to
/// `MAX_NAME_LINES`): one when both fit side by side, else the name
/// wrapped at word breaks with the tag on a line of its own. A row gives
/// every strip the most any of its strips needs (`StripParams::name_lines`),
/// so a long custom name and its tag are shown whole, not cut off.
pub(crate) fn name_lines(name: &str, tag: Option<&str>) -> u8 {
    let lines = if fits_one_line(name, tag) {
        1
    } else {
        wrapped_lines(name) + u32::from(tag.is_some())
    };
    lines.clamp(1, u32::from(MAX_NAME_LINES)) as u8
}

/// The S button's tooltip: Solo, or PFL in Live Mode, and whether the S
/// master holds it off.
fn solo_tip(pfl: bool, armed: bool) -> &'static str {
    match (pfl, armed) {
        (true, true) => "PFL (switched off by the S master, which brings it back)",
        (true, false) => "PFL: hear this strip before its fader where Cue plays",
        (false, true) => "Solo (switched off by the S master, which brings it back)",
        (false, false) => "Solo",
    }
}

/// The extra height, at scale 1, a header with `lines` name lines takes
/// over the one-line header `TOP_BAND_H` counts.
pub(crate) fn name_band_extra(lines: u8) -> f32 {
    f32::from(lines.saturating_sub(1)) * theme::TEXT_MD * 1.3
}

/// Whether Ø gives its side key's place away on this strip (`fader_h`
/// already scaled). Where the fader is too short for both the FX mini
/// fader and an FX key under the stack, Ø gives its place to FX, so the
/// working send stays on the strip (critique #13: AN1's vanished at
/// 1600x900); D counts too (issue #27), and with a D key Ø also gives way
/// where that brings the FX mini fader back (D pushed AN1's send into an
/// FX key at 1600x1000). TotalMix shows no Ø on the strip either; it stays
/// a row of the Settings panel, and the strip's tag says "Ø" while it's on
/// (critique #30).
fn phase_gives_way(p: &StripParams<'_>, fader_h: f32, scale: f32) -> bool {
    let t_key = usize::from(!matches!(p.cid, ChannelId::Output(_)));
    let planned = t_key + usize::from(p.has_phase && p.protocol_features) + 1 + usize::from(p.has_eq && p.protocol_features) + usize::from(p.has_dyn);
    // Not only where the stack still fits: a mic's fader at 1440x900 is
    // too short for all five keys already, and the send vanished with no
    // FX key at all (critique #47; the owner's pick, "go with
    // recommended"). Ø's place goes to an FX key; the count stays.
    let short = p.has_phase
        && p.protocol_features
        && p.fx_mini.is_some()
        && side_top(fader_h, planned, scale) - MINI_GAP * scale < MINI_MIN_H * scale
        && !stack_fits(fader_h, planned + 1, scale);
    let room_for_mini = |keys: usize| side_top(fader_h, keys, scale) - MINI_GAP * scale >= MINI_MIN_H * scale;
    short || (p.has_dyn && p.has_phase && p.protocol_features && p.fx_mini.is_some() && !room_for_mini(planned) && room_for_mini(planned - 1))
}

/// Whether this strip has no room for the FX mini fader and shows an FX
/// key instead (its row then shows keys throughout: `StripParams::fx_key`).
/// The side keys counted as `phase_gives_way` counts them.
pub fn fx_wants_key(p: &StripParams<'_>, scale: f32) -> bool {
    if p.fx_mini.is_none() {
        return false;
    }
    let fader_h = p.fader_h * scale;
    let t_key = usize::from(!matches!(p.cid, ChannelId::Output(_)));
    let planned = t_key + usize::from(p.has_phase && p.protocol_features) + 1 + usize::from(p.has_eq && p.protocol_features) + usize::from(p.has_dyn);
    let keys = planned - usize::from(phase_gives_way(p, fader_h, scale));
    side_top(fader_h, keys, scale) - MINI_GAP * scale < MINI_MIN_H * scale
}

fn header_row<'a>(
    cid: ChannelId,
    name: &str,
    type_tag: Option<(&'static str, Color)>,
    lines: u8,
    plain: Option<&'static str>,
    plain_tip: Option<String>,
    renaming: bool,
    rename_buf: &'a str,
    scale: f32,
) -> Element<'a, Message> {
    if renaming {
        // In the tape's room, so the header keeps its height (Fable's review).
        let field = text_input(short_label(name), rename_buf)
            .id(crate::app::RENAME_INPUT_ID)
            .on_input(Message::RenameChanged)
            .on_submit(Message::RenameCommit)
            .style(theme::text_input)
            .size(theme::TEXT_SM * scale)
            .padding([0.0, theme::SPACE_SM * scale])
            .width(Length::Fill);
        let h = Length::Fixed(theme::TEXT_MD * 1.3 * f32::from(lines.max(1)) * scale);
        return container(field).height(h).center_y(h).into();
    }
    // `.wrapping(Wrapping::None)` — without it, a long name ("Instr. 3/4",
    // "ADAT7/8") wraps to two lines partway through the collapse/expand
    // width animation, once the shrinking card gets narrower than the
    // name's natural width, growing the whole strip's *height* for the
    // ~160ms the animation is mid-flight before it settles back down —
    // user report: "pendant que ça se minimise le bus augmente de hauteur
    // un peu." No text in this card is meant to ever wrap (every label
    // here is a single short line by design), so this is a correctness
    // fix for the whole card, not just the header specifically — see the
    // matching `.wrapping(None)` added to every other `text()` in
    // `full_strip` below for the same reason.
    let label = short_label(name).to_string();
    // The type tag reads as a quiet word after the name. Its hue used to
    // carry the channel kind (blue MIC, violet ADAT, ...); the row a strip
    // stands in already says that, and hue is kept for state.
    let tag = shown_tag(name, type_tag);
    // The name on a strip of label tape, in Audra's serif (the owner's
    // pick, 2026-11-23).
    let name_text = |wrap: advanced_text::Wrapping| {
        text(label.clone())
            .font(theme::SERIF)
            .size(TAPE_NAME * scale)
            .wrapping(wrap)
            .color(theme::TAPE_INK)
    };
    // LOOP (the input's recording is an output's mix now) is live, so coral;
    // the type tags stay quiet.
    let tag_text = |tag: &'static str| -> Element<'a, Message> {
        // The type word, quiet, then the side mark in a colour of its own
        // (critique #38: it took the text tone in three themes).
        if let Some((word, side)) = sides_mark_word(tag) {
            // A state word (PAD) keeps its state's colour beside the mark.
            let word_ink = if word == "PAD" { theme::pad_ink() } else if state_tag_hint(word).is_some() { theme::state_mark_ink(theme::accent()) } else { theme::text_sec() };
            let quiet = text(word).font(theme::SANS_SEMI).color(word_ink).size(TAPE_STATE * scale).wrapping(advanced_text::Wrapping::None);
            let mark = text(side)
                .font(theme::SANS_SEMI)
                .color(theme::state_mark_ink(theme::accent()))
                .size(TAPE_STATE * scale)
                .wrapping(advanced_text::Wrapping::None);
            let tip = state_tag_hint(side).unwrap_or_default().to_string();
            let pair = row![quiet, hint_text(mark, tip, scale)].spacing(theme::SPACE_TIGHT * scale).align_y(iced::Alignment::Center);
            return container(pair).style(theme::tape_state_chip).padding([0.0, theme::SPACE_TIGHT * scale]).into();
        }
        // A state the unit is changing (LOOP, M/S, a width) in the live
        // coral, not solo's amber: "M/S" in amber over the M | S keys read
        // as mute and solo (critique #23).
        // Dim's tag in the lamp, as Dim's note in the counter is, its
        // lightness alone changed until it reads on every ground a strip
        // header has, lit or hovered (critique #40: on the floors' check it
        // came to 4.77:1 in Slate and 4.63:1 in Solarized).
        let color = if tag == "DIM" {
            theme::state_mark_ink(theme::lamp())
        } else if tag == "PAD" {
            // A state colour, its lightness alone moved, so it never reads as
            // the type word it replaces (`theme::pad_ink`).
            theme::pad_ink()
        } else if tag == "LOOP" || state_tag_hint(tag).is_some() {
            // Made readable on the strip's panel, lit or not (critique #28:
            // 3.3:1 on a hovered panel in Solarized).
            theme::readable_on_keys(theme::accent())
        } else {
            theme::TAPE_QUIET
        };
        let is_state = is_state_word(tag);
        let size = if is_state { TAPE_STATE } else { theme::TEXT_SM };
        let t = text(tag).font(if is_state { theme::SANS_SEMI } else { theme::SANS }).color(color).size(size * scale).wrapping(advanced_text::Wrapping::None);
        // A type word is ink on the tape; a state word keeps its colour on
        // the dark chip it always had (`theme::tape_state_chip`).
        let t: Element<'a, Message> = match state_tag_hint(tag) {
            Some(tip) => hint_text(t, tip.to_string(), scale),
            None => t.into(),
        };
        if is_state {
            container(t).style(theme::tape_state_chip).padding([0.0, theme::SPACE_TIGHT * scale]).into()
        } else {
            t
        }
    };
    // Name and tag side by side when they fit. Otherwise (`name_lines`) the
    // name wraps at word breaks and the tag takes the line below it, so
    // both are shown whole. The row gives every header the same fixed
    // height (`lines`), so the faders stay level and the collapse
    // animation cannot grow the card.
    let side_by_side = fits_one_line(name, tag);
    // Name and tag centred together. An output's DIM or M/S (its resting
    // OUT isn't drawn) moves the name half the tag's width when it comes;
    // centring the name alone was measured (critique #40) and puts DIM 2 px
    // past a 104 px strip's edge beside "Main 1/2", so the pair stays
    // centred.
    let content: Element<'a, Message> = if side_by_side {
        let mut header = row![name_text(advanced_text::Wrapping::None)]
            .spacing(tag.map_or(theme::SPACE_TIGHT, |t| tag_gap(name, t)) * scale)
            .align_y(iced::Alignment::Center);
        if let Some(tag) = tag {
            header = header.push(tag_text(tag));
        }
        header.into()
    } else {
        let mut stack = column![name_text(advanced_text::Wrapping::WordOrGlyph)
            .align_x(iced::alignment::Horizontal::Center)
            .width(Length::Fill)]
        .align_x(iced::Alignment::Center)
        .width(Length::Fill);
        if let Some(tag) = tag {
            stack = stack.push(tag_text(tag));
        }
        stack.into()
    };
    // The plain words, quiet, on a line of their own under the name and tag
    // (the row counts that line in `lines`).
    let content: Element<'a, Message> = match plain {
        Some(words) => column![
            content,
            text(words).size(theme::TEXT_SM * scale).color(theme::TAPE_QUIET).wrapping(advanced_text::Wrapping::None),
        ]
        .align_x(iced::Alignment::Center)
        .width(Length::Fill)
        .into(),
        None => content,
    };
    let header: Element<'a, Message> = if lines > 1 {
        let line_h = theme::TEXT_MD * 1.3 * scale;
        let h = Length::Fixed(line_h * f32::from(lines));
        container(content)
            .height(h)
            .center_y(h)
            .center_x(Length::Fill)
            .into()
    } else {
        content
    };
    let header = mouse_area(header)
        .on_press(Message::StripClicked(cid))
        .on_double_click(Message::RenameStart(cid))
        .interaction(mouse::Interaction::Text);
    // What it is in plain words, then how to rename it.
    let tip = match plain_tip {
        Some(words) => format!("{}: {words}. Double-click to rename", short_label(name)),
        None => "Double-click to rename".to_string(),
    };
    let plate = container(hint_text(header, tip, scale))
        .width(Length::Fill)
        .center_x(Length::Fill)
        .align_y(iced::Alignment::Center);
    // One line keeps the line height the strip's sums count (`TOP_BAND_H`:
    // `TEXT_MD` x 1.3), though the tape's type is a step smaller.
    let plate = plate.style(theme::tape_plate).padding([TAPE_PAD_Y * scale, TAPE_PAD_X * scale]);
    if lines <= 1 { plate.height(Length::Fixed(theme::TEXT_MD * 1.3 * scale)).center_y(Length::Fixed(theme::TEXT_MD * 1.3 * scale)).into() } else { plate.into() }
}

/// A collapsed strip is a glance-only readout, but per the reference
/// design keeps Mute/Solo live (unlike the fader/pan/route, still traded
/// away for space) — collapsing is for strips you're not actively
/// adjusting the level of, not ones you'd never want to silence in a
/// hurry. Only ever rendered fully settled (see `strip()`'s dispatch), so
/// `w` is always `COLLAPSED_W`, but it's threaded through rather than
/// hardcoded to keep this in lockstep with `full_strip`'s signature.
/// A card's contents under its fold's veil (`CollapseAnim::veil`): a face
/// like the card's, laid over them in a layer of its own (so over their
/// words and pictures too), as opaque as the fold has got.
fn veiled<'a>(contents: Element<'a, Message>, p: &StripParams<'a>) -> Element<'a, Message> {
    // Always there, clear when no fold runs: wrapping the contents only
    // while folding changed the widget tree as the fade began and ended,
    // so every meter and fader in the card started over for a frame (the
    // owner, 2026-11-14: "flickers when it fades").
    let a = p.collapse_anim.map_or(0.0, |anim| anim.veil(Instant::now()));
    let veil = container(iced::widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::strip_veil(p.selected, p.hovered, p.lamp, p.focused, match p.cid {
            ChannelId::Input(_) => 0,
            ChannelId::Playback(_) => 1,
            ChannelId::Output(_) => 2,
        }, a));
    // A small win's sparks over it, as always there as the veil (an
    // empty canvas when none fly).
    let sparks = iced::widget::canvas(crate::celebrate::StripSparks { t: p.win_spark, scale: p.scale })
        .width(Length::Fill)
        .height(Length::Fill);
    iced::widget::stack![contents, veil, sparks].into()
}

fn collapsed_strip<'a>(p: StripParams<'a>, w: f32) -> Element<'a, Message> {
    let cid = p.cid;
    let scale = p.scale;
    let btn_h = ICON_BTN_H * scale;
    // M and S as two keys with room between (the owner, 2026-11-03: on a
    // folded strip, joined, they were "way too close to each other").
    let mute_joint = theme::Joint::Alone;

    let mute_btn = hint(
        button(centered_label("M", theme::TEXT_SM * scale))
            .padding(0)
            .width(Length::Fill)
            .height(btn_h)
            .style(theme::firm_rim_key(theme::lamp_button(theme::mute_color(), theme::Lamp { active: p.mute, armed: p.mute_armed, bloom: p.bloom_mute, cool: p.cool_mute, joint: mute_joint, hue_label: true, inset: true, ..theme::Lamp::default() }), p.mute))
            .on_press(Message::Mute(cid, !p.mute)),
        if p.mute_armed {
            "Mute (switched off by the M master, which brings it back)"
        } else {
            "Mute"
        },
        scale,
    );
    let solo_btn = hint(
        button(centered_label("S", theme::TEXT_SM * scale))
            .padding(0)
            .width(Length::Fill)
            .height(btn_h)
            .style(theme::firm_rim_key(theme::lamp_button(theme::solo_color(), theme::Lamp { active: p.solo, armed: p.solo_armed, bloom: p.bloom_solo, cool: p.cool_solo, hue_label: true, inset: true, ..theme::Lamp::default() }), p.solo))
            .on_press(Message::Solo(cid, !p.solo)),
        solo_tip(p.pfl_mode, p.solo_armed),
        scale,
    );
    // No solo on outputs (see `full_strip`); an empty slot keeps the
    // collapsed cards' meters aligned across the row.
    let solo_slot: Element<'a, Message> = if matches!(cid, ChannelId::Output(_)) {
        iced::widget::Space::new().height(btn_h).into()
    } else {
        solo_btn
    };
    // Three bands on a full strip's grid: name and M/S where a full strip
    // has its name, pan and M/S; the meter exactly where the neighbouring
    // faders run, so their rulers line up; the expand button at the
    // bottom. The meter used to be taller and start lower, so its 0, 6,
    // 10... sat a few pixels off the strips beside it.
    // A folded mic strip still says 48V is on: an ember lamp under its
    // keys, in the band a full strip gives its mic line, its gain in the
    // tooltip (the critique, 2026-11-05: folded, phantom power went out
    // of sight).
    // A step more air between them (the owner, 2026-11-14: crammed). M and
    // S alone, at the band's foot, so they stand level on every folded
    // strip: 48V and PAD went under them and pushed them up on the strips
    // that had them (critique #56).
    let keys = column![mute_btn, solo_slot].spacing(theme::SPACE_MD * scale);
    // 48V and PAD, still in sight folded (the critique, 2026-11-05;
    // critique #42), as small marks at the top of the name's room.
    let mark = |words: &'static str, style: fn(&iced::Theme) -> container::Style, tip: String| -> Element<'a, Message> {
        hint_text(
            container(centered_label(words, theme::TEXT_SM * scale))
                .width(Length::Fill)
                .height(Length::Fixed(16.0 * scale))
                .style(style),
            tip,
            scale,
        )
    };
    let mut marks = column![].spacing(theme::SPACE_TIGHT * scale);
    if p.phantom && matches!(cid, ChannelId::Input(_)) {
        marks = marks.push(mark("48V", theme::phantom_mark, format!("48V phantom power is on; gain +{} dB", p.gain)));
    }
    if p.has_pad && p.pad && matches!(cid, ChannelId::Input(_)) {
        marks = marks.push(mark("PAD", theme::pad_mark, "PAD is on: this mic comes in 11 dB quieter, for a loud source".to_string()));
    }
    let top = column![
        marks,
        rotated_label(short_label(&p.name).to_string(), theme::TEXT_SM * scale),
        keys,
        // The same band of air a full strip leaves under its keys (the
        // owner: the S sat "way too close to the volume monitor"), and a
        // little more, so M stands level with an open strip's M|S
        // (critique #57 B: 4 px lower).
        iced::widget::Space::new().height(Length::Fixed((MS_GAP + FOLD_MS_LIFT) * scale)),
    ]
    .spacing(theme::SPACE_HAIRLINE)
    // As tall as a full strip's top in this row, its name lines included.
    // Whole pixels, rounded up (to nearest, the meter sat 1 px above the
    // open strips', critique #57): the 1.3x name line left M and S 0.3 px off the pixel
    // grid, and their shaded faces don't snap, so the top and bottom rims
    // smeared over two rows (critique #56: 2.25-2.63, the sides 3.5).
    .height(Length::Fixed(
        ((TOP_BAND_H + mic_band(cid, p.mic_line) + name_band_extra(p.name_lines)) * scale
            + 2.0 * theme::SPACE_HAIRLINE)
            .ceil(),
    ));
    // Narrowed, the meter is all there is: its hint says what the red
    // lights at its top are (the owner, 2026-11-10).
    let meter = container(hint_text(
        vu_meter(
            p.meter,
            p.fader_h * scale,
            scale,
            p.meter_available,
            p.clip,
            p.meter2.map(|m| (m, p.meter2_available, p.clip2)),
            (p.clip || p.clip2).then_some(Message::ClipReset(cid, p.meter2.is_some())),
        ),
        if p.clip || p.clip2 {
            "Level meter. Lit red at the top: this channel hit the top (too loud). Click to clear"
        } else {
            "Level meter. The lights at its top turn red when it hits the top (too loud)"
        }
        .to_string(),
        scale,
    ))
        .width(Length::Fill)
        .center_x(Length::Fill);
    // The + at the card's foot, the card as tall as its row: what sits
    // under a full strip's fader differs by kind (an output's LOOP, free
    // routing's picker), so a fixed band left folded cards taller or
    // shorter than their neighbours.
    // The narrow switch at the foot, its knob at the inner end (critique
    // #15: the same switch as an open strip's, in its other position), its
    // slot running off the card's right edge as there.
    let side = theme::SPACE_MD * scale;
    // The same height as an open strip's, which stands in its foot row
    // beside the routing field, and as far from the card's foot, so it
    // neither grows nor jumps as the card folds (the owner, 2026-11-13: it
    // "jumps while the other two disappear").
    // Its band of air above it presses it too (the owner, 2026-11-07: "its
    // hard to click on"; critique #41: the move into the foot row had left
    // it 19px tall). The room above it stretches, so the key stays put.
    let switch = edge_switch(cid, 0.0, w * scale - side, READOUT_GAP * scale, foot_h(scale), scale, "Expand (Ctrl: this and every strip to its right)");
    let mark = if p.submix_active || matches!(cid, ChannelId::Output(_)) { (theme::SPACE_SM + 3.0) * scale + theme::SPACE_HAIRLINE } else { 0.0 };
    let foot_pad = theme::SPACE_XL * scale + theme::SPACE_HAIRLINE + mark;
    let rows = column![top, meter, iced::widget::Space::new().height(Length::Fill)]
        .spacing(theme::SPACE_HAIRLINE)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::Alignment::Center);

    let panel = container(veiled(
        column![
            container(rows).padding(iced::Padding { top: theme::SPACE_SM * scale, right: side, bottom: 0.0, left: side }).height(Length::Fill),
            container(switch).padding(iced::Padding { left: side, bottom: foot_pad, ..iced::Padding::ZERO }),
        ]
        .spacing(theme::SPACE_HAIRLINE)
        .into(),
        &p,
    ))
        .style(theme::strip_panel(
            p.selected,
            p.hovered,
            p.lamp,
            p.focused,
        ))
        .width(Length::Fixed(w * p.scale))
        .height(Length::Fill)
        .clip(true);
    mouse_area(container(focus_tabbed(panel.into(), p.focused, p.scale)).style(theme::strip_glow(p.focused, p.lamp + p.path_pulse)))
    .on_press(Message::StripClicked(cid))
    .on_double_click(Message::ToggleCollapse(cid))
    // A folded strip opens on a double-click: a pointing hand says it can.
    .interaction(mouse::Interaction::Pointer)
    .on_enter(Message::StripHovered(Some(cid)))
    .on_exit(Message::StripHovered(None))
    .into()
}

/// The keyboard's strip carries a lit tab at the middle of its top edge,
/// as a console's channel-select lamp: a mark of its own shape, so focus
/// is never read as selection's coral rim (the critique, 2026-11-05). The
/// halo stays round it (`theme::strip_glow`).
fn focus_tabbed<'a>(card: Element<'a, Message>, focused: bool, scale: f32) -> Element<'a, Message> {
    if !focused {
        return card;
    }
    let tab = container(
        container(iced::widget::Space::new())
            .width(Length::Fixed(FOCUS_TAB_W * scale))
            .height(Length::Fixed(FOCUS_TAB_H * scale))
            .style(theme::focus_tab),
    )
    .width(Length::Fill)
    .align_x(alignment::Horizontal::Center);
    iced::widget::stack![card, tab].into()
}

/// The focus tab's size (`focus_tabbed`).
const FOCUS_TAB_W: f32 = 28.0;
const FOCUS_TAB_H: f32 = 4.0;

/// A side key (T, Ø, settings, EQ) in a press band `MS_HIT_H` tall: the
/// key keeps its own hover and press, and a press just above or below it
/// lands on the band, which says the same (the audit, 2026-11-05: 18px
/// keys under the desktop's 24px; see `ms_hits` for M|S).
fn side_hit<'a>(on_press: Message, scale: f32, key: Element<'a, Message>) -> Element<'a, Message> {
    let band = side_band(scale);
    iced::widget::stack![
        mouse_area(iced::widget::Space::new().width(Length::Fixed(band)).height(Length::Fixed(band)))
            .on_press(on_press)
            .interaction(mouse::Interaction::Pointer),
        side_slot(scale, key),
    ]
    .into()
}

/// `side_hit` for a key that can wear a ring round it, in the band's room
/// outside the key (the Mute FX link, critique #65 A 1: a shadow on the
/// key was clipped and never drew). The holder is always there, its ring
/// clear when `ring` is `None`, so nothing moves when it lights.
fn side_hit_ringed<'a>(on_press: Message, scale: f32, key: Element<'a, Message>, ring: Option<iced::Color>) -> Element<'a, Message> {
    let band = side_band(scale);
    let gap = RING_GAP * scale;
    let holder = container(key).padding(gap).style(move |_t: &iced::Theme| container::Style {
        border: iced::Border { color: ring.unwrap_or(iced::Color::TRANSPARENT), width: 1.5 * scale, radius: (theme::RADIUS_XS * scale + gap).into() },
        ..container::Style::default()
    });
    let slot = container(holder)
        .padding(iced::Padding { left: (SIDE_OVERHANG * scale - gap).max(0.0), ..iced::Padding::ZERO })
        .width(Length::Fixed(band))
        .height(Length::Fixed(band))
        .align_x(iced::Alignment::Start)
        .center_y(Length::Fixed(band));
    iced::widget::stack![
        mouse_area(iced::widget::Space::new().width(Length::Fixed(band)).height(Length::Fixed(band)))
            .on_press(on_press)
            .interaction(mouse::Interaction::Pointer),
        slot,
    ]
    .into()
}

/// The ring's distance out from the key, the band's whole room: the ring
/// 1.5 px with 1.5 px of ground inside it, a halo, not a heavier rim
/// (critique #66 A 4).
const RING_GAP: f32 = 3.0;

/// A side key in its band: `SIDE_OVERHANG` in from the band's lane side,
/// level with the band's middle.
fn side_slot<'a>(scale: f32, key: Element<'a, Message>) -> Element<'a, Message> {
    let band = side_band(scale);
    container(key)
        .padding(iced::Padding { left: SIDE_OVERHANG * scale, ..iced::Padding::ZERO })
        .width(Length::Fixed(band))
        .height(Length::Fixed(band))
        .align_x(iced::Alignment::Start)
        .center_y(Length::Fixed(band))
        .into()
}

/// `name` as it fits `room` characters: whole; else without its bracketed
/// part ("Phones 2 (AS1/2)" as "Phones 2"); else cut with an ellipsis.
fn fit_name(name: &str, room: usize) -> String {
    if name.chars().count() <= room {
        return name.to_string();
    }
    if let Some((head, _)) = name.split_once(" (") {
        if head.chars().count() <= room {
            return head.to_string();
        }
    }
    let mut cut: String = name.chars().take(room.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// The side key stack's height for `keys` keys.
fn stack_h(keys: usize, scale: f32) -> f32 {
    keys as f32 * SIDE_KEY * scale + keys.saturating_sub(1) as f32 * (side_pitch(scale) - SIDE_KEY * scale)
}

/// Where a stack of `keys` side keys starts beside a fader `fader_h` tall:
/// its first key level with the fader's middle, raised as far as the stack
/// needs to end `SIDE_KEY_FOOT` above the foot; less half the band's extra,
/// so the first key is drawn where it was.
fn side_top(fader_h: f32, keys: usize, scale: f32) -> f32 {
    ((fader_h / 2.0 - SIDE_KEY * scale / 2.0).min(fader_h - stack_h(keys, scale) - SIDE_KEY_FOOT * scale)
        - (side_band(scale) - SIDE_KEY * scale) / 2.0)
        .max(0.0)
}

/// Whether `keys` side keys end inside a fader `fader_h` tall.
fn stack_fits(fader_h: f32, keys: usize, scale: f32) -> bool {
    fader_h >= stack_h(keys, scale) + SIDE_KEY_FOOT * scale
}

/// A greyed side key in the same 24px band as a live one (`side_hit`), so
/// the stack keeps its spacing; nothing to press.
fn inert_band<'a>(scale: f32, key: Element<'a, Message>) -> Element<'a, Message> {
    side_slot(scale, key)
}

/// Picks between the two strip layouts and, while a collapse/expand
/// animation is in flight, the width the outer card should be drawn at
/// this frame. Growing, the full content is what's being revealed.
/// Shrinking, the narrow card's content shows at once and the card closes
/// round it: clipping the full strip down cut its keys and knobs in half
/// on the way and then jumped to the narrow card at the end (the owner,
/// 2026-11-10: "the animation isn't that good when it closes").
pub fn strip<'a>(p: StripParams<'a>) -> Element<'a, Message> {
    let now = Instant::now();
    let (w, show_full) = match &p.collapse_anim {
        // The layout it is leaving through the fading-out step, then the
        // one it arrives at (under the veil while the card resizes).
        Some(a) => (a.at(now), if a.leaving(now) { p.collapsed } else { !p.collapsed }),
        None => {
            if p.collapsed {
                (COLLAPSED_W, false)
            } else {
                (full_width(p.cid) + if p.ruler_labels { RULER_EXTRA } else { 0.0 }, true)
            }
        }
    };

    if show_full {
        full_strip(p, w)
    } else {
        collapsed_strip(p, w)
    }
}

/// The output index behind `cid`, when this strip should show the loopback
/// toggle at all: outputs only, and only on a connection that can drive
/// RME's own protocol (`OutputChannel::loopback` is a register the
/// class-compliant personality does not have).
fn loopback_target(cid: ChannelId, protocol_features: bool) -> Option<usize> {
    match cid {
        ChannelId::Output(i) if protocol_features => Some(i),
        _ => None,
    }
}

/// The strip's peak readout: each channel's held peak in dB (the level
/// while nothing is held), in an inset capsule under M|S, as TotalMix
/// prints its numbers over the meters. −∞ in silence, the clip colour at
/// 0 dBFS or over, a dash where the backend doesn't measure the channel.
fn peak_readout<'a>(p: &StripParams<'a>, scale: f32) -> Element<'a, Message> {
    let number = |m: MeterFrame, available: bool| -> Element<'a, Message> {
        let level = if m.peak > 0.0 { m.peak } else { m.value };
        let (label, color) = if !available {
            ("\u{2013}".to_string(), theme::muted())
        } else if level <= 1e-5 {
            // The secondary ink, as the numbers: in `muted` it read 3.59 on
            // the light capsule (carried by reviewer B since #70).
            ("\u{2212}\u{221e}".to_string(), theme::text_sec())
        } else if level >= 0.999 {
            (crate::app::db_number(level), theme::readable_on_floors(theme::clip()))
        } else {
            (crate::app::db_number(level).replace('-', "\u{2212}"), theme::text_sec())
        };
        text(label)
            .font(theme::MONO)
            .size(theme::TEXT_SM * scale)
            .color(color)
            .wrapping(advanced_text::Wrapping::None)
            .into()
    };
    let mut line = row![number(p.meter, p.meter_available)]
        .spacing(theme::SPACE_MD * scale)
        .align_y(iced::Alignment::Center);
    if let Some(m2) = p.meter2 {
        line = line.push(number(m2, p.meter2_available));
    }
    container(line)
        .width(Length::Fill)
        .height(Length::Fixed(PEAK_H * scale))
        .center_x(Length::Fill)
        .align_y(iced::Alignment::Center)
        .style(theme::inset_box(PEAK_H * scale / 2.0))
        .into()
}

fn full_strip<'a>(p: StripParams<'a>, w: f32) -> Element<'a, Message> {
    let cid = p.cid;
    let out = p.output_idx;
    let scale = p.scale;
    // The M/S row is 18px: the keys inside their well (`theme::key_well`).
    let btn_h = (MS_WELL_H - 2.0 * theme::WELL_PAD) * scale;

    // Every channel kind has at least the STEREO link/split toggle now
    // (see `app.rs::settings_popover`) — 48V/PAD/Sensitivity/Gain are
    // just the input-only additions to the same panel.
    let has_settings = true;
    // Phase on with its key given away (`phase_gives_way`): the tag says
    // so, lit, where "MIC" or "INST" stood (critique #30: phase could be on
    // with nothing on the strip to show it).
    let tag = if p.phase && phase_gives_way(&p, p.fader_h * scale, scale) { Some(("\u{d8}", theme::readable_on_keys(theme::accent()))) } else { p.type_tag };
    // Beside the inputs (`mic_line`), the words go in the band kept for
    // their gain row, not on a line of their own.
    let header = header_row(cid, &p.name, tag, p.name_lines, if p.mic_line || matches!(cid, ChannelId::Input(_)) { None } else { p.plain }, p.plain_tip.clone(), p.renaming, p.rename_buf, scale);
    let header = shimmered(header, p.shimmer, crate::motion::Shimmer::Name, theme::RADIUS_XS * scale);
    // M|S (or an output's M|CUE) as one block, split by a seam, as on a
    // console; an output with no CUE has its M alone.
    let is_output = matches!(cid, ChannelId::Output(_));
    let mute_joint = if is_output && !p.has_cue { theme::Joint::Alone } else { theme::Joint::Left };

    let mute_btn = hint(
        button(centered_label("M", theme::TEXT_SM * scale))
            .padding(0)
            .width(Length::Fill)
            .height(btn_h)
            .style(theme::lamp_button(theme::mute_color(), theme::Lamp { active: p.mute, armed: p.mute_armed, bloom: p.bloom_mute, cool: p.cool_mute, joint: mute_joint, hue_label: true, inset: true, round: btn_h / 2.0, ..theme::Lamp::default() }))
            .on_press(Message::Mute(cid, !p.mute)),
        if p.mute_armed {
            "Mute (switched off by the M master, which brings it back)"
        } else {
            "Mute"
        },
        scale,
    );
    let solo_btn = hint(
        button(centered_label("S", theme::TEXT_SM * scale))
            .padding(0)
            .width(Length::Fill)
            .height(btn_h)
            .style(theme::lamp_button(theme::solo_color(), theme::Lamp { active: p.solo, armed: p.solo_armed, bloom: p.bloom_solo, cool: p.cool_solo, joint: theme::Joint::Right, hue_label: true, inset: true, round: btn_h / 2.0, ..theme::Lamp::default() }))
            .on_press(Message::Solo(cid, !p.solo)),
        solo_tip(p.pfl_mode, p.solo_armed),
        scale,
    );
    // Fixed-width buttons left dead space flanking them whenever the card
    // was sized for a wider sibling row (48V/PAD, or just a long channel
    // name) — filling the row makes every row use the card's full width
    // instead of only the widest one.
    // Outputs have no solo: TotalMix has none, and neither backend that
    // drives the hardware does anything with one (`set_solo` returns early
    // for outputs), so an output's S lit only in the simulation.
    let mut ms_row = if is_output {
        row![mute_btn]
    } else {
        row![mute_btn, solo_btn]
    }
    .width(Length::Fill);
    // CUE joins the block on an output; the block then sits in its well.
    if p.has_cue {
        ms_row = ms_row.push(hint(
            // "CUE", not TotalMix's "C": right under the balance knob's
            // "C" (centre) the letter alone read as the same thing.
            button(centered_label("CUE", theme::TEXT_SM * scale))
                .padding(0)
                .width(Length::Fill)
                .height(btn_h)
                .style(theme::lamp_button(theme::accent(), theme::Lamp { active: p.cue, bloom: p.bloom_cue, cool: p.cool_cue, joint: theme::Joint::Right, inset: true, round: btn_h / 2.0, ..theme::Lamp::default() }))
                .on_press(Message::CueChanged(cid, !p.cue)),
            "Cue: hear this output's mix where Cue plays (Audra settings → Monitor)",
            scale,
        ));
    }

    let mut rows = column![header].spacing(theme::SPACE_HAIRLINE);

    // Mic setup on the strip itself: the 48V switch and the preamp gain,
    // which used to hide in the gear flyout. Every hardware input gets the
    // line, empty where the channel has neither, so the faders across the
    // input row stay level with each other (and with the row's dB scale).
    if let ChannelId::Input(idx) = cid {
        let mut mic = row![].spacing(theme::SPACE_SM * scale).align_y(iced::Alignment::Center);
        // An input with neither (the optical ones): its plain words here.
        if !p.has_48v && !p.has_gain {
            if let Some(words) = p.plain {
                mic = mic.push(text(words).size(theme::TEXT_SM * scale).color(theme::text_sec()).wrapping(advanced_text::Wrapping::None));
            }
        }
        if p.has_48v {
            mic = mic.push(hint(
                button(centered_label(if p.armed_48v { "48V?" } else { "48V" }, theme::TEXT_SM * scale))
                    .padding(0)
                    .width(MIC_BADGE_W * scale)
                    .height(MIC_LINE_H * scale)
                    // A steady rim while off (critique #104 B: its rim read
                    // 2.0-2.6, High contrast 2.3-3.0).
                    .style(theme::split_safe_key(
                        theme::lamp_button(theme::phantom(), theme::Lamp { active: p.phantom, armed: p.armed_48v, waiting: p.armed_48v, bloom: p.bloom_48v, cool: p.cool_48v, hue_label: true, ..theme::Lamp::default() }),
                        p.phantom || p.armed_48v,
                    ))
                    .on_press(Message::PhantomPress(idx)),
                if p.phantom {
                    "Phantom power on: click to switch off"
                } else if p.armed_48v {
                    "Click again to switch phantom power on (lower your monitors first)"
                } else {
                    "Phantom power (48V) for condenser mics: click, then click again"
                },
                scale,
            ));
        }
        if p.has_gain {
            // The number in the mono face, the unit smaller beside it: a
            // full "+40 dB" in mono (gain reaches +60 on AN1/2) ran past
            // the strip's edge beside the 48V badge.
            // When the unit changes the gain (its wheel), the readout lights
            // in coral and fades, so the change is seen where it lands.
            let flash = p.gain_flash.clamp(0.0, 1.0);
            let rest = if p.gain > 0 { theme::text_primary() } else { theme::muted() };
            let number = if flash > 0.0 { theme::blend(rest, theme::accent(), flash) } else { rest };
            mic = mic.push(hint(
                mouse_area(
                    container(
                        row![
                            text(format!("+{}", p.gain))
                                .font(theme::MONO)
                                .size(theme::TEXT_NUM * scale)
                                .color(number)
                                .wrapping(advanced_text::Wrapping::None),
                            text("dB")
                                .size(theme::TEXT_SM * scale)
                                .color(theme::text_sec())
                                .wrapping(advanced_text::Wrapping::None),
                        ]
                        .spacing(theme::SPACE_TIGHT * scale)
                        .align_y(iced::Alignment::Center),
                    )
                    .padding([0.0, theme::SPACE_TIGHT * scale])
                    .style(move |_t: &iced::Theme| container::Style {
                        background: (flash > 0.0)
                            .then(|| iced::Background::Color(Color { a: 0.22 * flash, ..theme::accent() })),
                        border: iced::Border {
                            color: Color { a: 0.7 * flash, ..theme::accent() },
                            width: if flash > 0.0 { 1.0 } else { 0.0 },
                            radius: theme::RADIUS_XS.into(),
                        },
                        ..container::Style::default()
                    }),
                )
                .on_press(Message::ToggleFlyout(cid, FlyoutKind::Settings))
                // A pointing hand: it opens something.
                .interaction(mouse::Interaction::Pointer),
                "Preamp gain: click for the gain knob",
                scale,
            ));
        }
        rows = rows.push(
            container(mic)
                .height(Length::Fixed(MIC_LINE_H * scale + theme::SPACE_SM * scale))
                .center_x(Length::Fill)
                .align_y(iced::Alignment::Center),
        );
    } else if p.mic_line {
        // A playback strip beside the inputs (two-row mode): the same
        // line, so the two blocks line up, holding its plain words.
        let band = container(match p.plain {
            Some(words) => Element::from(text(words).size(theme::TEXT_SM * scale).color(theme::text_sec()).wrapping(advanced_text::Wrapping::None)),
            None => Element::from(iced::widget::Space::new()),
        })
        .height(Length::Fixed(MIC_LINE_H * scale + theme::SPACE_SM * scale))
        .center_x(Length::Fill)
        .align_y(iced::Alignment::Center);
        rows = rows.push(band);
    }

    // Pan sits directly under the header, TotalMix-style, rather than
    // trailing at the very bottom — with pan above the fader and the
    // route picker (below) after it, the fader ends up vertically
    // centered in the strip's own flow instead of front-loaded.
    //
    // Outputs get this row too now, even though there's nothing real to
    // back it with yet — the user caught that skipping it made Output
    // strips visibly *shorter* than Input/Playback ones (confirmed via a
    // live screenshot: ~257px vs ~307px), when the real TotalMix
    // reference shows every strip, outputs included, at the exact same
    // height *and* with a knob in this exact spot (labeled "C", same as
    // an input's pan). That knob is very likely a real output-bus
    // **balance** control, not pan-into-a-bus (which wouldn't make sense
    // for an output) — but `RmeDevice::set_pan`'s signature is
    // `(channel, output)`, built for routing a channel's signal *into* a
    // submix, and neither `tuxmix-core` nor `tuxmix-usb`'s protocol layer
    // has anything resembling an output-master balance register (checked
    // both before writing this).
    //
    // First attempt reserved a blank `Space` here instead of a knob — the
    // user pointed out that read as the control being entirely missing,
    // not "present but not applicable yet." A dimmed, non-interactive
    // knob (`Knob::interactive: false`, see its own doc comment) reads
    // correctly as the latter — same footprint, same "C" TotalMix shows,
    // just visibly inert — without shipping a knob that would silently
    // do nothing if someone tried to drag it. `on_change`/`on_reset` are
    // genuinely unreachable: `interactive: false` makes `Knob::update`
    // return early before either could ever be invoked.
    let pan_str = match p.pan.cmp(&0) {
        std::cmp::Ordering::Less => format!("L{}", -p.pan),
        std::cmp::Ordering::Greater => format!("R{}", p.pan),
        std::cmp::Ordering::Equal => "C".to_string(),
    };
    let pan_knob = if is_output || !p.can_pan {
        Knob {
            readout: crate::widgets::knob::Readout::Beside,
            tint: None,
            value: if is_output { 0.0 } else { p.pan as f32 },
            range: (-100.0, 100.0),
            label: if is_output { "C".to_string() } else { pan_str },
            arc_from_center: true,
            home: None,
            interactive: false,
            log_scale: false,
            modifiers: p.modifiers,
            scale,
            on_change: Box::new(|_| unreachable!("interactive: false suppresses all input")),
            on_reset: Box::new(|| unreachable!("interactive: false suppresses all input")),
        }
    } else {
        Knob {
            readout: crate::widgets::knob::Readout::Beside,
            tint: None,
            value: p.pan as f32,
            range: (-100.0, 100.0),
            label: pan_str,
            arc_from_center: true,
            home: None,
            interactive: true,
            log_scale: false,
            modifiers: p.modifiers,
            scale,
            on_change: Box::new(move |v| Message::PanChanged(cid, out, v.round() as i8)),
            on_reset: Box::new(move || Message::PanReset(cid, out)),
        }
    };
    let pan_hint = if is_output {
        // In plain words, and with a way to do it now (the critique,
        // 2026-11-05: "no known hardware register" was the developer's).
        "Balance isn't available: the unit gives Audra no balance control for its outputs. To set left and right apart, turn off Stereo pair in this strip's Settings and use the two faders"
    } else if !p.can_pan {
        // In words, with what to do instead (the owner, 2026-11-06, met a
        // refusal turning a playback strip's knob).
        if p.stereo_linked {
            "No balance on this optical pair yet: the Linux driver gives Audra none. To set left and right apart, turn off Stereo pair in Settings and use the two faders"
        } else {
            match cid {
                ChannelId::Playback(_) => "No pan here: split, each channel only reaches its own side. Turn Stereo pair back on in Settings for a balance",
                _ => "No pan here: split, each channel only reaches its own side",
            }
        }
    } else {
        "Pan — drag, scroll, or double-click to reset"
    };
    rows = rows.push(
        container(hint(knob(pan_knob), pan_hint, scale))
            .width(Length::Fill),
    );

    let well = container(ms_row)
        .padding(theme::WELL_PAD * scale)
        .width(Length::Fill)
        .style(theme::key_well_round(btn_h / 2.0 + theme::WELL_PAD * scale));
    // The keys' press areas, a full 24px band under the well: the keys
    // still take their own hover and presses, and a press just above or
    // below one falls through to its half of the band.
    let hit = |message: Message| -> Element<'a, Message> {
        mouse_area(iced::widget::Space::new().width(Length::Fill).height(Length::Fixed(MS_HIT_H * scale)))
            .on_press(message)
            .interaction(mouse::Interaction::Pointer)
            .into()
    };
    let mut ms_hits = row![hit(Message::Mute(cid, !p.mute))].width(Length::Fill);
    if !is_output {
        ms_hits = ms_hits.push(hit(Message::Solo(cid, !p.solo)));
    } else if p.has_cue {
        ms_hits = ms_hits.push(hit(Message::CueChanged(cid, !p.cue)));
    }
    rows = rows.push(iced::widget::stack![
        ms_hits,
        container(well).height(Length::Fixed(MS_HIT_H * scale)).center_y(Length::Fixed(MS_HIT_H * scale)),
    ]);
    // The peak readout (TotalMix's numbers over its meters): each
    // channel's peak in dB in an inset capsule.
    rows = rows.push(iced::widget::Space::new().height(Length::Fixed(PEAK_GAP * scale)));
    rows = rows.push(peak_readout(&p, scale));
    rows = rows.push(iced::widget::Space::new().height(Length::Fixed(MS_GAP * scale)));
    // 48V/PAD/Sensitivity/Gain all live in the gear flyout now (see
    // `app.rs::settings_popover`), not inline here — keeps every strip's
    // own height fixed regardless of channel type, and matches the route
    // panel's own "opens beside the strip" shape instead of an inline
    // accordion that used to grow/shrink the card itself. Gain used to be
    // the one holdout (a `Knob` placed inside the old Stack-based flyout
    // broke click handling for the whole row), but the flyout isn't a
    // `Stack` anymore — it pushes the row instead of overlaying it — so
    // that failure mode doesn't apply and Gain moved in with the rest.

    let default_vol = p.default_vol;
    let fader_h = p.fader_h * scale;
    let fader_widget = fader(Fader {
        speak_band: p.speak_band,
        meter_named: p.meter_named,
        value: p.vol,
        range: p.drag_range.unwrap_or((0.0, 2.0)),
        default_value: default_vol,
        marks: p.trim_marks.clone(),
        trim_lit: p.trim_gains && !p.fx_lit,
        fx_lit: p.fx_lit,
        meter: p.meter,
        meter_available: p.meter_available,
        meter2: p.meter2,
        meter2_available: p.meter2_available,
        clip: p.clip,
        clip2: p.clip2,
        on_clip_reset: (p.clip || p.clip2)
            .then_some(Message::ClipReset(cid, p.meter2.is_some())),
        reduction_db: p.dyn_reduction_db,
        height: fader_h,
        show_meter: true,
        // The side column (the FX mini fader over `icon_col`, built
        // below) is this canvas's `row!` sibling, `SIDE_KEY` wide however
        // many keys it holds, plus the row's spacing (`SIDE_KEY_GAP`,
        // unscaled, as several spacings in this file are). See
        // `Fader::reserved_right`'s own doc comment for why the canvas
        // needs to know it: it feeds a pixel-level centering fix.
        reserved_right: side_band(scale) + SIDE_KEY_GAP,
        track_from_right: Some(LANE_TO_KEYS - SIDE_OVERHANG - SIDE_KEY_GAP / scale),
        modifiers: p.modifiers,
        scale,
        show_track: true,
        ruler_labels: p.ruler_labels,
        label: None,
        label_color: theme::text_sec(),
        // Unused: only matters when `show_meter: false` (see its own
        // doc comment) — this strip fader is never that.
        compact_width: 0.0,
        on_press: if p.fx_lit {
            Box::new(move |v, _| Message::FxFaderPressed(cid, v))
        } else {
            Box::new(move |v, range| Message::FaderPressed(cid, out, v, range))
        },
        on_drag: if p.fx_lit {
            Box::new(move |v| Message::FxFaderDrag(cid, v))
        } else {
            Box::new(move |v| Message::VolumeChanged(cid, out, v))
        },
        on_release: Box::new(move || Message::RangeCleared(cid)),
        // Showing a send, a double-click turns it off, never up.
        on_reset: if p.fx_lit {
            Box::new(move || Message::FxFaderDrag(cid, 0.0))
        } else {
            Box::new(move || Message::Reset(cid, out, default_vol))
        },
    });

    // Settings gear and EQ stacked on the strip's right flank, beside the
    // fader — confirmed against a live TotalMix screenshot at high zoom
    // (an earlier "put them in a row below the fader instead" call in
    // this file was a misreading of a narrower crop; a full-height zoom
    // on a real strip shows the gear/EQ column sitting to the right of
    // the meter, roughly spanning the fader's lower half, not underneath
    // it — the user caught this directly). `STRIP_W` was widened (see
    // its own doc comment) specifically so this column has room without
    // pushing the meter/ruler group past the fader canvas's own bounds.
    // The keys sit in 24px press bands (`side_hit`), the bands closer than
    // the keys were, so each key stays where it was drawn.
    let mut icon_col = column![]
        .spacing(side_pitch(scale) - side_band(scale))
        .align_x(iced::Alignment::Start);
    let mut side_keys = 0usize;
    // T is TotalMix's Trim Gains (manual 21.3; RME's Trim Gains video:
    // "with trim activated you can think of this channel fader now as a
    // master gain or master trim fader"): lit, this strip's fader moves the
    // channel in every mix at once. On every input and playback strip (the
    // owner, 2026-11-06, missed it on playback); outputs have none. Until
    // then it opened the input's hardware trim, which is now a row of the
    // strip's Settings panel. With Trim Gains on for every strip (View
    // page) it stands lit and is not the strip's to change.
    if !matches!(cid, ChannelId::Output(_)) {
        side_keys += 1;
        let key = button(centered_label("T", theme::TEXT_SM * scale))
            .padding(0)
            .width(SIDE_KEY * scale)
            .height(SIDE_KEY * scale)
            .style(theme::side_key(p.trim_gains));
        icon_col = icon_col.push(if p.trim_all {
            inert_band(scale, hint(key, "Trim Gains is on for every strip (View page)", scale))
        } else {
            side_hit(Message::TrimStripToggle(cid), scale, hint(
                key.on_press(Message::TrimStripToggle(cid)),
                if p.trim_gains {
                    "Trim Gains is on: this fader moves the channel in every mix at once. Click: off"
                } else {
                    "Trim Gains: make this fader move the channel in every mix at once"
                },
                scale,
            ))
        });
    }
    // Where the fader is too short for both the FX mini fader and an FX key
    // under the stack, Ø gives its place to FX, so the working send stays
    // on the strip (critique #13: AN1's vanished at 1600x900); Ø is a row
    // of the Settings panel too.
    let phase_gives_way = phase_gives_way(&p, fader_h, scale);
    if p.has_phase && p.protocol_features && !phase_gives_way {
        side_keys += 1;
        icon_col = icon_col.push(side_hit(Message::PhaseChanged(cid, !p.phase), scale, hint(
            button(centered_label("Ø", theme::TEXT_SM * scale))
                .padding(0)
                .width(SIDE_KEY * scale)
                .height(SIDE_KEY * scale)
                .style(theme::side_key(p.phase))
                .on_press(Message::PhaseChanged(cid, !p.phase)),
            "Phase Ø invert",
            scale,
        )));
    }
    // No "SP" button here any more: a playback pair's stereo split IS its
    // link state (`RmeDevice::playback_linked` is `!split`), so it was the
    // STEREO switch of the settings flyout a second time, lit under
    // another name.
    if has_settings {
        // Opened by the FX key, that key rings instead: one panel, one
        // ring (critique #60 A 2).
        let settings_open = p.open_flyout == Some(FlyoutKind::Settings);
        let wrench_ring = settings_open && !p.fx_opened;
        side_keys += 1;
        icon_col = icon_col.push(side_hit(Message::ToggleFlyout(cid, FlyoutKind::Settings), scale, hint(
            // Ringed while its panel is open, as EQ's and D's are with
            // their effect off: the wrench switches nothing on, so it is
            // never solid (critique #60 A 2, one rule: solid on, ring open).
            button(crate::widgets::strip_icons::wrench_icon(
                if wrench_ring { theme::side_key_ring_ink() } else { theme::muted() },
                scale,
            ))
                .padding(0)
                .width(SIDE_KEY * scale)
                .height(SIDE_KEY * scale)
                .style(move |t: &iced::Theme, st| if wrench_ring { theme::side_key_open(t, st) } else { (theme::side_key(false))(t, st) })
                .on_press(Message::ToggleFlyout(cid, FlyoutKind::Settings)),
            if settings_open {
                "Hide settings"
            } else if matches!(cid, ChannelId::Output(_)) {
                "Settings: the output's own (stereo, Talkback, phase, M/S, EQ, FX return)"
            } else {
                "Settings: the channel's own (stereo, gain, 48V, phase, EQ, sends)"
            },
            scale,
        )));
    }
    // No EQ key where the driver opens no EQ (critique #12, the owner's
    // choice: "Hide what can't work"); the strip's Settings panel says why.
    if p.has_eq && p.protocol_features {
        let eq_open = p.open_flyout == Some(FlyoutKind::Eq);
        side_keys += 1;
        icon_col = icon_col.push(side_hit(Message::ToggleFlyout(cid, FlyoutKind::Eq), scale, hint(
            button(centered_label("EQ", theme::TEXT_SM * scale))
                .padding(0)
                .width(SIDE_KEY * scale)
                .height(SIDE_KEY * scale)
                .style({
                    // Solid only while the EQ is on; its panel open with
                    // the EQ off, a ring (critique #59, the owner's pick:
                    // solid for both read "on" beside a panel saying off).
                    let ring = eq_open && !p.eq_enabled;
                    let lit = p.eq_enabled;
                    move |t: &iced::Theme, st| if ring { theme::side_key_open(t, st) } else { (theme::side_key(lit))(t, st) }
                })
                .on_press(Message::ToggleFlyout(cid, FlyoutKind::Eq)),
            if eq_open { "Hide EQ" } else { "Show EQ (3-band + low cut)" },
            scale,
        )));
    }
    // D under EQ, as TotalMix sets them (issue #27; the owner: "dynamics
    // should be a menu called D on the side").
    if p.has_dyn {
        let dyn_open = p.open_flyout == Some(FlyoutKind::Dynamics);
        side_keys += 1;
        // Never over "on": the lit key wins (OpenCode's job 61).
        let hinted = p.dyn_hint && !dyn_open && !p.dyn_on;
        let d_key: Element<'a, Message> = button(centered_label("D", theme::TEXT_SM * scale))
                .padding(0)
                .width(SIDE_KEY * scale)
                .height(SIDE_KEY * scale)
                .style({
                    // As EQ's: solid only while Voice/Dynamics is on.
                    let lit = p.dyn_on;
                    let ring = dyn_open && !p.dyn_on;
                    move |t: &iced::Theme, st| if hinted { theme::side_key_hinted(t, st) } else if ring { theme::side_key_open(t, st) } else { (theme::side_key(lit))(t, st) }
                })
                .on_press(Message::ToggleFlyout(cid, FlyoutKind::Dynamics))
                .into();
        // The one-time hint: a lamp dot at the key's corner, not a ring
        // (critique #60 A 1: its ring read as "panel open", the side
        // keys' ring since the owner's pick).
        let d_key = if hinted {
            let dot = 7.0 * scale;
            iced::widget::stack![
                d_key,
                container(container(iced::widget::Space::new().width(dot).height(dot)).style(theme::hint_dot(dot / 2.0)))
                    .width(SIDE_KEY * scale)
                    .height(SIDE_KEY * scale)
                    .align_x(iced::Alignment::End)
                    .align_y(iced::Alignment::Start),
            ]
            .into()
        } else {
            d_key
        };
        icon_col = icon_col.push(side_hit(Message::ToggleFlyout(cid, FlyoutKind::Dynamics), scale, hint(
            d_key,
            if dyn_open {
                if matches!(cid, ChannelId::Input(_)) { "Hide Voice" } else { "Hide Dynamics" }
            } else if matches!(cid, ChannelId::Input(_)) {
                // Critique #31: what's inside, named (it hid noise removal,
                // the presets, Cough and Auto-Tune behind a "D").
                "Voice, how others hear you: ready-made sounds, noise removal, harsh S sounds, Auto-Tune, and TotalMix's compressor"
            } else {
                "Dynamics for what the computer plays here: Volume protection, Even out, Night, and TotalMix's compressor"
            },
            scale,
        )));
    }
    // The side keys' top: level with the fader's middle, or as high as
    // the stack needs to end `SIDE_KEY_FOOT` above the fader's foot.
    let top_for = |keys: usize| side_top(fader_h, keys, scale);
    let mut side_keys_top = top_for(side_keys);
    // A fader too short for the FX mini fader keeps an FX key in its place,
    // opening the Settings panel where its knob is (critique #12: it stood
    // aside without a word), lit while the level is on.
    let mut fx_mini = p.fx_mini.clone();
    // The row's keys (`fx_key`) only where the key fits: a strip whose
    // stack is full keeps its mini fader rather than losing FX (the
    // advisor's catch).
    let too_short = side_keys_top - MINI_GAP * scale < MINI_MIN_H * scale || (p.fx_key && stack_fits(fader_h, side_keys + 1, scale));
    // The FX key only where the stack still ends inside the fader (the
    // reviewer); else the Settings panel (the wrench) is where it is.
    // Where Ø gave its place to it, the FX key stands in that place.
    if too_short && !phase_gives_way && !stack_fits(fader_h, side_keys + 1, scale) {
        fx_mini = None;
    }
    if let Some(m) = fx_mini.clone().filter(|_| too_short) {
        side_keys += 1;
        icon_col = icon_col.push(side_hit_ringed(Message::OpenFx(cid), scale, hint_text(
            button(centered_label("FX", theme::TEXT_SM * scale))
                .padding(0)
                .width(SIDE_KEY * scale)
                .height(SIDE_KEY * scale)
                .style({
                    // Solid while the level is on; with it off and the
                    // Settings panel open (where this key leads), a ring,
                    // as EQ's and D's (critique #59 A 10: it opened the
                    // panel and stayed dark beside the lit wrench).
                    let (lit, ring) = (m.t > 0.0, m.t <= 0.0 && p.fx_opened && p.open_flyout == Some(FlyoutKind::Settings));
                    let held = lit && m.held;
                    move |t: &iced::Theme, st| {
                        if held {
                            theme::side_key_held(t, st)
                        } else if ring {
                            theme::side_key_open(t, st)
                        } else {
                            (theme::side_key(lit))(t, st)
                        }
                    }
                })
                .on_press(Message::OpenFx(cid)),
            format!("{} Press to set it in Settings", m.tip.split(". Double-click").next().unwrap_or(&m.tip)),
            scale,
        ), m.linked.then(|| theme::link_ring(m.held))));
        side_keys_top = top_for(side_keys);
        fx_mini = None;
    }
    // Over the keys, TotalMix's FX mini fader (manual 21.3): the send on
    // an input or a playback, the return on an output, filled in the
    // send's coral or the return's green while on. It takes the room above
    // the keys; on a fader too short to leave it any, an FX key stands in
    // its place (above).
    let mini_h = side_keys_top - MINI_GAP * scale;
    let mini: Option<Element<'a, Message>> = fx_mini.filter(|_| mini_h >= MINI_MIN_H * scale).map(|m| {
        let (send, pair) = (m.send, m.pair);
        let widget = crate::widgets::mini_fader::mini_fader(crate::widgets::mini_fader::MiniFader {
            t: m.t,
            // Held silent by Mute FX: the mute hue, as the FX key takes it
            // on a short strip (critique #65 A 2: green at its kept level
            // on a tall window, as if heard).
            tint: if send {
                theme::accent()
            } else if m.held {
                theme::mute_color()
            } else {
                theme::signal()
            },
            on_change: m.live.then(|| {
                Box::new(move |t: f32| {
                    if send && matches!(cid, ChannelId::Input(0 | 1)) {
                        Message::FxSend(crate::fx::t_send_db(t))
                    } else if send {
                        // Another source's own send.
                        Message::SourceFxSend(cid, crate::fx::t_send_db(t))
                    } else {
                        Message::FxReturn(pair, crate::fx::t_return_vol(t))
                    }
                }) as Box<dyn Fn(f32) -> Message>
            }),
            on_reset: m.live.then(|| match (send, cid) {
                (true, ChannelId::Input(0 | 1)) => Message::FxSend(crate::fx::OFF_DB),
                (true, _) => Message::SourceFxSend(cid, crate::fx::OFF_DB),
                (false, _) => Message::FxReturn(pair, 0.0),
            }),
            rise_step: Some(if send { crate::fx::send_t_up } else { crate::fx::return_t_up }),
            height: mini_h / scale,
            scale,
            modifiers: p.modifiers,
        });
        // The Mute FX link on a tall strip too (critique #66 A 1): the ring
        // drawn over the mini fader, so its own drawing can't cover it.
        // In the band's room beside the fader (RING_GAP, as round a key)
        // and below its "FX" label, so it rings the lane, not the cap's
        // rims and the label with it (critique #67 A 5, B).
        let widget: Element<'a, Message> = match m.linked.then(|| theme::link_ring(m.held)) {
            Some(ring) => {
                use crate::widgets::mini_fader::{LABEL_FROM_H, LABEL_H};
                let label = if mini_h >= LABEL_FROM_H * scale { LABEL_H * scale } else { 0.0 };
                iced::widget::stack![
                    container(widget).padding([0.0, RING_GAP * scale]),
                    container(
                        container(iced::widget::Space::new().width(Length::Fill).height(Length::Fill))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(move |_t: &iced::Theme| container::Style {
                                border: iced::Border { color: ring, width: 1.5 * scale, radius: (theme::RADIUS_XS * scale + RING_GAP * scale).into() },
                                ..container::Style::default()
                            }),
                    )
                    .padding(iced::Padding { top: label, ..iced::Padding::ZERO })
                    .width(Length::Fill)
                    .height(Length::Fill),
                ]
                .into()
            }
            None => widget.into(),
        };
        hint_text(widget, m.tip, scale)
    });
    let side: Element<'a, Message> = match mini {
        Some(mini) => column![mini, iced::widget::Space::new().height(Length::Fixed(MINI_GAP * scale)), icon_col]
            .align_x(iced::Alignment::Center)
            .into(),
        None => column![iced::widget::Space::new().height(Length::Fixed(side_keys_top)), icon_col]
            .align_x(iced::Alignment::Center)
            .into(),
    };
    // The clip lights at the meter's top say what they are and that a
    // click clears them (the owner, 2026-11-10: "I can click on the red
    // two dots but I dont know whats it for"): a hint over just them,
    // letting the press through to the meter.
    let fader_widget: Element<'a, Message> = {
        let canvas_w = w * scale - 2.0 * theme::SPACE_MD * scale - SIDE_KEY_GAP - SIDE_BAND * scale - (KEYS_TO_EDGE - SIDE_OVERHANG).max(0.0) * scale;
        let zone = crate::widgets::fader::clip_lights_zone(canvas_w, LANE_TO_KEYS - SIDE_OVERHANG - SIDE_KEY_GAP / scale, p.ruler_labels, scale);
        let tip = if p.clip || p.clip2 {
            "Clip lights: lit red, this channel hit the top (too loud). Click to clear them"
        } else {
            "Clip lights: they turn red when this channel hits the top (too loud); a click clears them"
        };
        iced::widget::stack![
            fader_widget,
            container(hint_text(iced::widget::Space::new().width(Length::Fixed(zone.width)).height(Length::Fixed(zone.height)), tip.to_string(), scale))
                .padding(iced::Padding { left: zone.x, top: zone.y, ..iced::Padding::ZERO }),
        ]
        .into()
    };
    rows = rows.push(
        row![
            fader_widget,
            // Beside the lane from its middle down, as on the original
            // reference (the owner: "I remember them being in the
            // middle"): the top key level with the fader's middle, the
            // stack under it, raised as far as a short fader needs.
            // The bands reach `SIDE_OVERHANG` past the keys on the card's
            // side too (more, zoomed out, into `side_band_extra`), so the
            // keys stand `KEYS_TO_EDGE` from it as before.
            container(side)
                .height(Length::Fixed(fader_h))
                .align_y(iced::Alignment::Start)
                .padding(iced::Padding { right: (KEYS_TO_EDGE - SIDE_OVERHANG).max(0.0) * scale, ..iced::Padding::ZERO }),
        ]
        .spacing(SIDE_KEY_GAP)
        .width(Length::Fill),
    );

    let db_display: Element<'a, Message> = if p.editing {
        text_input("", p.edit_buf)
            .id(crate::app::DB_INPUT_ID)
            .on_input(Message::EditChanged)
            .on_submit(Message::EditCommit)
            .style(theme::text_input)
            .size(theme::TEXT_SM * scale)
            .width(Length::Fill)
            .into()
    } else {
        let initial = if p.vol > 0.0 {
            crate::app::db_number(p.vol)
        } else {
            "-inf".into()
        };
        // The number in the mono face, the unit smaller beside it (as the
        // gain readout): a full "-57.2 dB" in mono ran under the collapse
        // button beside it, or pushed the button out of the strip.
        mouse_area(
            row![
                // The number alone, as TotalMix's value box (the box got
                // shorter for the narrow switch beside it; "dB" is in its
                // tooltip and the double-click's field).
                text(if p.vol > 0.0 { crate::app::db_number(p.vol) } else { "-\u{221e}".into() })
                    .font(theme::MONO)
                    // Off: quieter than a level, but read (`muted` gave 3.59 on
                    // the light well; reviewer B, carried since #70).
                    .color(if p.vol > 0.0 { theme::text_primary() } else { theme::text_sec() })
                    .size(theme::TEXT_NUM * scale)
                    .wrapping(advanced_text::Wrapping::None),
            ]
            .spacing(theme::SPACE_TIGHT * scale)
            .align_y(iced::Alignment::Center),
        )
        .on_double_click(Message::EditStart(cid, initial))
        // A text cursor: a double-click types a level in.
        .interaction(mouse::Interaction::Text)
        .into()
    };
    // The narrow key beside the dB readout, at the bottom of the strip as
    // the reference has it (moved out of the side column at the user's
    // request). Its button has a fixed footprint (`SIDE_KEY` x
    // `ICON_BTN_H`): an unbounded `Length::Fill` inside a button once left
    // every strip a blank card. 2026-11-06: TotalMix's key, a solid
    // triangle pointing left on a side-key tile a little wider than tall
    // (the owner: "It looks more like button"); a minus, then two
    // chevrons closing on a bar, until then.
    // 2026-11-06, again (the owner: "that little expander is no good ...
    // a carve out button like in totalmix"): carved into the strip as the
    // value box beside it is, and as tall, the M|S treatment (a cut with
    // the key sunk in it).
    let open = p
        .collapse_anim
        .map(|a| ((a.at(Instant::now()) - COLLAPSED_W) / (w - COLLAPSED_W).max(1.0)).clamp(0.0, 1.0))
        .unwrap_or(1.0);
    // A fold's closing step: the value box and the foot row's field fold
    // into the switch (from the left, toward it), and its knob slides
    // over, before the card narrows (`CollapseAnim::close_ms`).
    // The fold no longer narrows the foot into the switch: the whole card's
    // contents fade under a veil instead (`CollapseAnim::veil`).
    let shut = 0.0;
    let keep = 1.0 - shut;
    let open = open.min(keep);
    // The value box under the meter and the lane exactly (critique #15:
    // it started left of the meter and stopped short of the lane): the
    // fader canvas is the row less the side column.
    let side = theme::SPACE_MD * scale;
    // The strip's pixels: its zoomed width, and zoomed out what its side
    // keys' bands need beyond it (`side_band_extra`), growing as it opens.
    let wpx = w * scale + side_band_extra(scale) * open;
    let canvas_w = w * scale - 2.0 * side - SIDE_KEY_GAP - SIDE_BAND * scale - (KEYS_TO_EDGE - SIDE_OVERHANG).max(0.0) * scale;
    let (box_left, box_right) =
        crate::widgets::fader::meter_to_lane(canvas_w, LANE_TO_KEYS - SIDE_OVERHANG - SIDE_KEY_GAP / scale, p.ruler_labels, scale);
    // The switch's slot runs from the side keys' column to the card's
    // edge, but no wider than its knob and a short slide need: the rest
    // goes to the routing field beside it, which shares its row now.
    let slot_full = wpx - side - box_right - SIDE_KEY_GAP;
    let slot_w = slot_full.min((EDGE_KNOB_W + theme::WELL_PAD + 6.0 + FRAME_W) * scale);
    let slot_spare = (slot_full - slot_w).max(0.0);
    // In the foot row now, beside the routing field (or LOOP) and as tall
    // (the owner, 2026-11-13: in the readout row it jumped as the value box
    // and the field went on folding; "share the space with the dropdown").
    let collapse_btn = edge_switch(cid, open, slot_w, 0.0, foot_h(scale), scale, "Collapse (Ctrl: this and every strip to its right)");
    let collapse_btn = shimmered(collapse_btn, p.shimmer, crate::motion::Shimmer::Fold, theme::RADIUS_BOX * scale);
    // Centered on the fader, not the meter — the fader's own row above
    // is centered on the *full* row width (`Fader::reserved_right`
    // accounts for `icon_col` so the track lands at exactly
    // `content_width / 2`), so centering `db_display` in a container
    // spanning that same full width lines the two up automatically.
    // A plain `row![db_display, Space::Fill, collapse_btn]` (the
    // previous shape) instead centered it in the *meter's* half of the
    // row — TotalMix-accurate positionally (matches the reference), but
    // not what the user asked for this time, which was explicitly
    // "under the fader." The left spacer matches `collapse_btn`'s own
    // fixed width exactly so the centered container's *available* width
    // stays symmetric around the row's true center, rather than losing
    // width asymmetrically to a right-hand sibling the way the fader
    // row itself used to (see `Fader::layout_x`'s doc comment).
    // Without collapsing, a spacer of the button's width keeps the
    // readout centered the same way.
    //
    // 2026-11-03: two layers instead of three slots. The readout is
    // centred across the whole row (so still under the fader) and the
    // collapse button sits over the row's right end. Three slots left the
    // readout about 50px between two 15px ones, and a "-57.2 dB" ran
    // under the button or pushed it out of the strip.
    // The readout's box gives up `READOUT_NUDGE` on the right where the
    // button sits, so the widest value ("-10.0 dB") keeps a gap before it
    // instead of touching it; the centre moves half that, which still
    // reads as under the fader.
    // The value in an inset box (the reference's value box under its
    // fader), the collapse key beside it.
    let box_w = (box_right - box_left).max(0.0);
    let value_box = container(db_display)
        .width(Length::Fixed(box_w * keep))
        .height(Length::Fixed(readout_h(scale)))
        .center_x(Length::Fixed(box_w * keep))
        .align_y(iced::Alignment::Center)
        .clip(true)
        .style(theme::inset_box(theme::RADIUS_BOX * scale));
    let value_box = shimmered(value_box.into(), p.shimmer, crate::motion::Shimmer::Level, theme::RADIUS_BOX * scale);
    // The narrow key apart from the value, under the side keys' column
    // (the owner, 2026-11-07: "It shouldn't be merging with the DB button.
    // It should be on the side of the bar that contains the controller",
    // as TotalMix's sits under its T and wrench): the same gap from the
    // value as the column from the fader, the same edge as the column.
    // The value box alone; the narrow switch is in the foot row.
    // Over the switch, the value row's empty end presses it too: a band of
    // `READOUT_GAP` just above the slot, so the key is a 24px target again
    // without its row growing (the owner, 2026-11-07: "its hard to click
    // on"; critique #41: in the foot row it had come to 19px).
    // The foot row's 2px of air above it moves into this row's foot, so
    // the band runs on down to the row's hairline seam (the advisor: 3px
    // between band and slot pressed the strip, not the key).
    let press_band: Element<'a, Message> = if p.can_collapse {
        container(
            mouse_area(
                iced::widget::Space::new().width(Length::Fixed(slot_w)).height(Length::Fixed((READOUT_GAP + theme::SPACE_TIGHT) * scale)),
            )
                .on_press(Message::ToggleCollapse(cid))
                .interaction(mouse::Interaction::Pointer),
        )
        .height(Length::Fixed((readout_h(scale) + theme::SPACE_TIGHT * scale).max(0.0)))
        .align_y(iced::Alignment::End)
        .into()
    } else {
        iced::widget::Space::new().width(Length::Fixed(0.0)).into()
    };
    let readout_row: Element<'a, Message> = row![
        container(value_box).padding(iced::Padding { left: box_left + box_w * shut, ..iced::Padding::ZERO }),
        iced::widget::Space::new().width(Length::Fill),
        press_band,
    ]
    .into();
    rows = rows.push(iced::widget::Space::new().height(Length::Fixed(READOUT_GAP * scale)));
    let upper = rows;
    let mut rows = column![].spacing(theme::SPACE_HAIRLINE);
    // The foot row's own control (the routing field, FX send, or an
    // output's LOOP), drawn beside the narrow switch where the strip folds.
    let mut foot: Option<Element<'a, Message>> = None;
    // As wide as the value box's right edge from the card's padding: the
    // switch beside it then stands under the side keys, as before.
    let foot_w = if p.can_collapse { box_right + slot_spare } else { (w - 2.0 * theme::SPACE_MD) * scale };

    // The routing field (manual 21.3; the owner, 2026-11-06, missed it):
    // at the foot of every input and playback strip, the output its fader
    // and pan edit, by name. A pick from its list edits another: in the
    // Submix mode that output becomes the mix being edited (every strip
    // follows), in the Free mode this strip alone goes there. The list
    // marks the outputs the channel already reaches (an arrow, as the
    // manual's list).
    if p.fx_lit && !matches!(cid, ChannelId::Output(_)) {
        // Showing FX sends, the field says so in amber, and a click is the
        // way back to the mix.
        foot = Some(hint(
            // Outlined in the lamp, never filled with it (DESIGN.md: the
            // lamp is light, not a button's fill; critique #19).
            button(text("FX send").size(theme::TEXT_SM * scale).center().width(Length::Fill).color(theme::readable_on_floors(theme::lamp())))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                .width(Length::Fill)
                // Its lamp rim held at 3:1 wherever it lands (critique #62
                // B 1: Daylight's amber rim read 1.76 on the lit strip, the
                // key's only edge).
                .style(theme::split_safe_key(theme::lamp_outline, false))
                .on_press(Message::FaderFxToggle),
            "Every fader sets its FX send now (an output's, its return). Click here for the mix again, on every strip",
            scale,
        ));
    } else if !p.routes.is_empty() {
        // TotalMix's field is the name alone on a band, no arrow (the owner,
        // 2026-11-06: "non of these have dropdowns"); a click opens the
        // list. The name stands in the middle, led by figure spaces (about a
        // character's width each), and is cut short with an
        // ellipsis where the field hasn't room for it all (a role with a
        // number, "Phones 2 (AS1/2)", wrapped and overflowed; the reviewer).
        // Its 2px sides and a closer guess of a 12px letter (Atkinson runs
        // about 0.48 em; 0.56 cut "Main 1/2" short beside the switch).
        let field_w = foot_w - 2.0 * theme::SPACE_TIGHT * scale;
        let char_w = 0.5 * theme::TEXT_SM * scale;
        let room = (field_w / char_w).floor().max(4.0) as usize;
        let current = p.routes.iter().find(|r| r.pair == out).cloned().map(|r| {
            let label = fit_name(&r.label, room);
            let lead = (room.saturating_sub(label.chars().count())) / 2;
            RouteChoice { in_field: true, lead, label, ..r }
        });
        let field = iced::widget::pick_list(p.routes.clone(), current, move |r: RouteChoice| {
            Message::SetStripRoute(cid, r.pair)
        })
        .handle(iced::widget::pick_list::Handle::None)
        // A panel hanging past its row takes no press while the list
        // is open (`widgets::overhang`): the list may lie over it.
        .on_open(Message::RouteMenu(true))
        .on_close(Message::RouteMenu(false))
        .style(theme::pick_list)
        .menu_style(theme::menu)
        // No side padding: the list's rows take the field's padding, and
        // with it "›Phones 3/4" touched the list's rim (0 px clear in all
        // five themes, critique #99 B, third round). The field's own 2px
        // sides keep its name off its edge.
        .padding([theme::SPACE_TIGHT * scale, 0.0])
        .text_size(theme::TEXT_SM * scale)
        .width(Length::Fill);
        // Always inside its hint: taking the hint away while the list was
        // open changed the field's place in the tree, which reset the open
        // list, so a pick took two clicks (the owner, 2026-11-10: "I have
        // to click twice"; the hint over the list was "a non-issue").
        foot = Some(hint(
            field,
            if p.free_route {
                "Where this strip's fader goes (free routing: this strip only). An arrow in the list marks the outputs it already plays in"
            } else {
                "The mix this fader sets. Pick another to edit that mix; \u{203a} marks where this strip already plays"
            },
            scale,
        ));
    }

    // The per-strip route pill ("AN1/2 ▸") that used to sit here is gone:
    // it only ever chose the submix being edited, which the Now Mixing
    // placard does now. Outputs keep this slot for their loopback toggle.
    if let Some(i) = loopback_target(cid, p.protocol_features) {
        // Outputs have no route trigger (a single master, not a
        // per-submix crosspoint) — the same row slot shows the loopback
        // toggle instead (`OutputChannel::loopback`, bReq 0x15: feeds the
        // input signal back into this output's own playback path). The
        // STEREO link/split toggle lives in the Settings flyout now
        // (`app.rs::settings_popover`), same as every other channel kind.
        foot = Some(
            container(hint_text(
                button(
                    text(if p.armed_loop { "LOOP?" } else { "LOOP" })
                        .size(theme::TEXT_SM * scale)
                        .wrapping(advanced_text::Wrapping::None),
                )
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                    // A steady rim while off (critique #104 B: its top read 2.36).
                    .style(theme::split_safe_key(
                        theme::lamp_button(theme::accent(), theme::Lamp { active: p.loopback, armed: p.armed_loop, waiting: p.armed_loop, bloom: p.bloom_loop, cool: p.cool_loop, ..theme::Lamp::default() }),
                        p.loopback || p.armed_loop,
                    ))
                    .on_press(Message::LoopPress(i)),
                if p.armed_loop {
                    "Click again to record this output (it replaces an input's recording)".to_string()
                } else {
                    p.loop_note.clone()
                },
                scale,
            ))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),
        );
    }
    // The foot row: its control and the narrow switch side by side, the
    // same height, running to the card's edge as the switch's slot does.
    let foot_row: Option<Element<'a, Message>> = match (foot, p.can_collapse) {
        (Some(f), true) => Some(
            row![
                iced::widget::Space::new().width(Length::Fixed(foot_w.max(0.0) * shut)),
                container(f)
                    .width(Length::Fixed(foot_w.max(0.0) * keep))
                    .height(Length::Fixed(foot_h(scale)))
                    .align_y(iced::Alignment::Center)
                    .clip(true),
                collapse_btn
            ]
            .spacing(SIDE_KEY_GAP)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        (None, true) => Some(
            row![iced::widget::Space::new().width(Length::Fixed(foot_w.max(0.0))), collapse_btn]
                .spacing(SIDE_KEY_GAP)
                .align_y(iced::Alignment::Center)
                .into(),
        ),
        (Some(f), false) => Some(container(f).width(Length::Fixed(foot_w.max(0.0))).into()),
        (None, false) => None,
    };

    // The output being mixed for carries a short coral mark at its foot
    // (a live state, so it may use coral), the strip-side twin of the
    // Now Mixing placard. Every output keeps the mark's slot, lit or not,
    // so the row's strips end on one line (the reviewer, 2026-11-05: Main
    // ended 8px below Phones and the outputs beside it).
    if p.submix_active || matches!(cid, ChannelId::Output(_)) {
        let lit = p.submix_active;
        rows = rows.push(
            container(
                container(iced::widget::Space::new().width(Length::Fill).height(Length::Fixed(3.0 * scale)))
                    .width(Length::FillPortion(2))
                    .style(move |_t: &iced::Theme| container::Style {
                        background: lit.then(|| iced::Background::Color(theme::accent())),
                        border: iced::Border {
                            radius: theme::RADIUS_XS.into(),
                            ..iced::Border::default()
                        },
                        ..container::Style::default()
                    }),
            )
            .padding(iced::Padding {
                top: theme::SPACE_SM * scale,
                right: w * scale * 0.2,
                bottom: 0.0,
                left: w * scale * 0.2,
            })
            .width(Length::Fill),
        );
    }

    // Double-click anywhere on the card that isn't already claimed by a
    // specific control (the fader/pan canvases capture their own
    // double-click for reset-to-default, the dB readout for its edit
    // field, buttons for their own press) collapses the strip — a bigger,
    // more discoverable target than the tiny "-" button alone. A plain
    // click there is a no-op; Ctrl/Shift+click toggles multi-selection
    // (see `Message::StripClicked`) — mute/solo/collapse on any selected
    // strip then apply to the whole selection at once.
    // Bottom padding wider than the top three sides — the route
    // trigger (the card's last row) otherwise sat almost flush against
    // the card's own bottom edge, reading as cramped next to the dB
    // readout right above it. The readout row alone runs to the right
    // edge, where its narrow key's tab sits.
    let side = theme::SPACE_MD * scale;
    let panel = container(veiled(
        column![
            container(upper.width(Length::Fill).align_x(iced::Alignment::Center))
                .padding(iced::Padding { top: theme::SPACE_SM * scale, right: side, bottom: 0.0, left: side }),
            container(readout_row).padding(iced::Padding { left: side, ..iced::Padding::ZERO }),
            container(match foot_row {
                // The air above the foot row is the value row's foot where
                // the strip folds (`press_band`).
                Some(f) => column![iced::widget::Space::new().height(Length::Fixed(if p.can_collapse { 0.0 } else { theme::SPACE_TIGHT * scale })), f],
                None => column![],
            })
            .padding(iced::Padding { left: side, ..iced::Padding::ZERO }),
            container(rows.width(Length::Fill).align_x(iced::Alignment::Center))
                .padding(iced::Padding { top: 0.0, right: side, bottom: theme::SPACE_XL * scale, left: side }),
        ]
        .spacing(theme::SPACE_HAIRLINE)
        .into(),
        &p,
    ))
    .style(theme::strip_panel(
        p.selected,
        p.hovered,
        p.lamp,
        p.focused,
    ))
    .width(Length::Fixed(wpx))
    .clip(true);
    // The keyboard's glow sits on a plain wrapper: see `theme::strip_glow`.
    let card = mouse_area(container(focus_tabbed(panel.into(), p.focused, scale)).style(theme::strip_glow(p.focused, p.lamp + p.path_pulse)))
    .on_press(Message::StripClicked(cid))
    // The manual's right-click menu (copy, paste, clear, mirror) lives in
    // the Settings panel here, so a right-click opens that.
    .on_right_press(Message::ToggleFlyout(cid, FlyoutKind::Settings))
    .on_enter(Message::StripHovered(Some(cid)))
    .on_exit(Message::StripHovered(None));
    let card: Element<'a, Message> = if p.can_collapse {
        card.on_double_click(Message::ToggleCollapse(cid)).into()
    } else {
        card.into()
    };
    if !p.remote_bank {
        return card;
    }
    // A strip a Mackie Control surface drives: a 2px coral line along its
    // top (live: moving the surface moves it), so the block is in view.
    iced::widget::Stack::new()
        .push(card)
        .push(
            container(
                container(iced::widget::Space::new().width(Length::Fill).height(Length::Fixed(2.0 * scale)))
                    .style(|_t: &iced::Theme| container::Style {
                        background: Some(iced::Background::Color(theme::accent())),
                        border: iced::border::rounded(1.0),
                        ..container::Style::default()
                    }),
            )
            .padding([0.0, theme::SPACE_MD * scale]),
        )
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_state_chip_has_room_beside_the_name() {
        // The owner on #39: "AN1/2 PAD" looked crammed at 2 px.
        assert_eq!(tag_gap("AN1/2", "PAD"), TAPE_CHIP_GAP);
        // "Main 1/2" and DIM keep fitting on one line: only spare room is used.
        assert!(fits_one_line("Main 1/2", Some("DIM")));
        assert!(tag_gap("Main 1/2", "DIM") >= theme::SPACE_TIGHT);
        assert_eq!(tag_gap("AN1/2", "MIC"), theme::SPACE_TIGHT, "a quiet type word");
    }

    use super::*;

    #[test]
    fn a_route_name_too_long_for_the_field_drops_its_brackets_then_is_cut() {
        assert_eq!(fit_name("Main 1/2", 12), "Main 1/2");
        assert_eq!(fit_name("Phones 2 (AS1/2)", 12), "Phones 2");
        assert_eq!(fit_name("Phones 3/4", 6), "Phone\u{2026}");
    }

    #[test]
    fn a_long_name_gets_more_header_lines_instead_of_being_cut_off() {
        assert_eq!(name_lines("AN1", None), 1);
        assert_eq!(name_lines("AN2", Some("MIC")), 1, "name and tag side by side");
        assert_eq!(name_lines("ADAT7/8", None), 1);
        assert_eq!(name_lines("Neumann vocal", None), 2, "seen cut off as 'Neumann voc'");
        assert_eq!(name_lines("Neumann vocal", Some("MIC")), 3, "the tag on its own line");
        assert_eq!(name_lines("Supercalifragilistic", None), 2, "one long word breaks inside");
        assert_eq!(name_lines("a very long name for one channel strip", Some("PB")), MAX_NAME_LINES);
        assert_eq!(shown_tag("ADAT3/4", Some(("ADAT", Color::WHITE))), None, "already in the name");
        assert_eq!(shown_tag("Main", Some(("OUT", Color::WHITE))), None);
        assert_eq!(name_band_extra(1), 0.0);
        assert!(name_band_extra(2) > 0.0);
    }

    #[test]
    fn loopback_is_output_only_and_needs_the_protocol() {
        assert_eq!(loopback_target(ChannelId::Output(2), true), Some(2));
        assert_eq!(
            loopback_target(ChannelId::Output(2), false),
            None,
            "the class-compliant personality has no loopback register"
        );
        assert_eq!(loopback_target(ChannelId::Input(2), true), None);
    }
    use std::time::Duration;

    #[test]
    fn settled_anim_is_never_settling() {
        let a = CollapseAnim { prev: STRIP_W, value: STRIP_W, since: Instant::now(), close_ms: 0.0, open_ms: 0.0 };
        assert!(!a.is_settling(a.since));
        assert!(!a.is_settling(a.since + Duration::from_millis(10)));
    }

    #[test]
    fn transitioning_anim_settles_after_the_interp_window() {
        let a = CollapseAnim { prev: STRIP_W, value: COLLAPSED_W, since: Instant::now(), close_ms: 0.0, open_ms: 0.0 };
        assert!(a.is_settling(a.since), "just started — should still be settling");
        assert!(
            a.is_settling(a.since + Duration::from_millis(80)),
            "mid-transition — should still be settling"
        );
        assert!(
            !a.is_settling(a.since + Duration::from_millis(200)),
            "past the interp window — should have stopped requesting redraws"
        );
    }

    #[test]
    fn a_fold_fades_out_resizes_empty_then_fades_in() {
        // The owner, 2026-11-13: "There should be a closing animation
        // before the collapse"; 2026-11-14: "fade out first", then "still
        // dont look nice": the open card's contents fade, the card narrows
        // under the veil, the folded contents fade in at their width.
        let a = CollapseAnim { prev: STRIP_W, value: COLLAPSED_W, since: Instant::now(), close_ms: CLOSE_MS, open_ms: OPEN_MS };
        let at = |ms: u64| a.since + Duration::from_millis(ms);
        assert_eq!(a.at(at(100)), STRIP_W, "the card keeps its width while it fades");
        assert!(a.leaving(at(100)) && !a.leaving(at(160)), "the open layout until the fade is done");
        assert!((a.veil(at(75)) - 0.5).abs() < 0.02, "half veiled at half the step");
        assert_eq!(a.veil(at(200)), 1.0, "hidden while it narrows");
        assert_eq!(a.at(at(150 + 160)), COLLAPSED_W, "narrowed over the usual time");
        assert!(a.veil(at(150 + 160 + 70)) < 0.6, "the folded contents coming in");
        assert!(a.is_settling(at(150 + 170)), "still moving while they fade in");
        assert!(!a.is_settling(at(150 + 160 + 150)) && a.veil(at(150 + 160 + 150)) == 0.0, "and done");
        let none = CollapseAnim { close_ms: 0.0, open_ms: 0.0, ..a };
        assert!(none.veil(at(50)) == 0.0 && !none.leaving(at(0)), "Animations off: no veil");
    }

    #[test]
    fn at_interpolates_linearly_from_prev_to_value() {
        let a = CollapseAnim { prev: STRIP_W, value: COLLAPSED_W, since: Instant::now(), close_ms: 0.0, open_ms: 0.0 };
        assert_eq!(a.at(a.since), STRIP_W);
        assert_eq!(a.at(a.since + Duration::from_millis(160)), COLLAPSED_W);
        let mid = a.at(a.since + Duration::from_millis(80));
        let expected_mid = (STRIP_W + COLLAPSED_W) / 2.0;
        assert!(
            (mid - expected_mid).abs() < 0.5,
            "expected ~{expected_mid} at the midpoint, got {mid}"
        );
    }

    #[test]
    fn a_side_keys_band_is_24px_at_every_zoom() {
        // Issue #21: 18px wide, and smaller zoomed out.
        use super::{side_band, side_band_extra, side_pitch};
        for scale in [0.5, 0.75, 1.0, 1.25, 1.5] {
            assert!(side_band(scale) >= 24.0, "at {scale}");
            assert!(side_pitch(scale) >= side_band(scale), "bands never overlap at {scale}");
        }
        assert_eq!(side_band_extra(1.0), 0.0, "nothing moves at 100 %");
        assert_eq!(side_band_extra(1.5), 0.0);
        assert_eq!(side_band_extra(0.75), 6.0, "zoomed out, the strip takes what the band needs");
    }
}
