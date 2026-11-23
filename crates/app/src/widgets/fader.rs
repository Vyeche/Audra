//! Custom vertical fader + VU meter, drawn and driven entirely through a
//! [`canvas::Program`] — the one widget egui's built-in slider couldn't give
//! us (shift-drag fine range, scroll-wheel nudge, double-click reset).

use iced::advanced::text as advanced_text;
use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{mouse, window, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use std::hash::Hash;
use std::time::{Duration, Instant};

use crate::theme;

fn hash_of(f: impl FnOnce(&mut std::collections::hash_map::DefaultHasher)) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f(&mut h);
    h.finish()
}

/// Whether meters are interpolated and redrawn *between* ticks, at the
/// display's refresh rate. Measured 2026-10-22 (release build, mixer view,
/// signal on every meter): 19 % of a core with it, 5.5 % without. Every
/// one of those in-between redraws walks the whole window, and with a
/// signal playing they ran continuously. Off: a meter moves once per tick,
/// and the tick is fast enough (30 Hz, `app::TICK_ACTIVE`) that it still
/// reads as motion; the cap's easing after an outside change keeps its own
/// short animation.
const INTERPOLATE_METERS: bool = false;

/// How long a meter keyframe transition takes to visually settle — matches
/// `app.rs`'s `Tick` interval, since that's how often a new keyframe
/// (`MeterFrame`) arrives.
const METER_INTERP_MS: f32 = 50.0;

/// A meter's ease-out keyframe pair — the ballistics in `app.rs` only
/// compute a new value once per `Tick` (50ms), which reads as a stair-step
/// rather than motion if drawn as-is. `Fader`/`VuMeter` interpolate between
/// `prev` and `value` using their own draw-time `Instant::now()` instead of
/// the single stale snapshot `view()` was built with, and self-drive extra
/// redraws while still catching up (`canvas::Action::request_redraw()`),
/// settling back to the normal `Tick`-driven redraw rate once caught up —
/// full-refresh-rate motion without polling the device or rebuilding the
/// view any more often than before.
#[derive(Clone, Copy, Debug)]
pub struct MeterFrame {
    pub prev: f32,
    pub value: f32,
    pub since: Instant,
    /// The held peak (manual 21.7, Peak Hold), drawn as a line; 0 for none.
    pub peak: f32,
    /// The RMS level (the loudness), drawn as the solid part of the bar
    /// with the peaks lighter above it; negative when not measured.
    pub rms: f32,
    /// When the clip LED last came on: the shockwave it sends out
    /// (`CLIP_SHOCK`).
    pub clip_at: Option<Instant>,
    /// When `peak` was taken (Quick's words trust it only briefly:
    /// `quick::held_peak`).
    pub peak_at: Option<Instant>,
}

/// The detent's hold round `home` in travel: `SNAP_PX` of the hand's travel,
/// never wider than half a dB either side, so a short fader keeps every
/// level near home within reach (critique #11: at 120px the 5px zone
/// swallowed -3 to +2.6 dB).
fn snap_width(home: f32, run_h: f32, scale: f32) -> f32 {
    let px = (SNAP_PX * scale) / run_h.max(1.0);
    if home <= 0.0 {
        return px;
    }
    let half_db = 10f32.powf(0.5 / 20.0);
    let home_t = vol_to_t(home);
    let cap = (vol_to_t(home * half_db) - home_t).min(home_t - vol_to_t(home / half_db));
    px.min(cap.max(0.0))
}

/// How long a clip's shockwave runs from the LED (the owner's fun pass: "a
/// physical shockwave flash ... rather than a boring red block"). The
/// light still holds until cleared, as before. The clip's soft tick is
/// app.rs's, held back while on air (`sounds::set_on_air`).
pub(crate) const CLIP_SHOCK: Duration = Duration::from_millis(320);

/// How far (0..1) a clip's shockwave has run, while it runs.
fn shock(clip_at: Option<Instant>, now: Instant) -> Option<f32> {
    let at = clip_at?;
    if !crate::widgets::animations() {
        return None;
    }
    if let Some(t) = crate::widgets::frozen_shock() {
        return Some(t);
    }
    let p = now.saturating_duration_since(at).as_secs_f32() / CLIP_SHOCK.as_secs_f32();
    (p < 1.0 && crate::widgets::animations()).then_some(p)
}

impl MeterFrame {
    /// A meter with no in-flight transition — e.g. outputs, which don't
    /// have live meter data wired up.
    pub fn still(value: f32) -> Self {
        Self {
            prev: value,
            value,
            since: Instant::now(),
            peak: 0.0,
            rms: -1.0,
            clip_at: None,
            peak_at: None,
        }
    }

    /// What a meter shows at `now`: the interpolated value, or (not
    /// interpolating, `INTERPOLATE_METERS`) simply the newest one.
    pub(crate) fn shown(&self, now: Instant) -> f32 {
        if INTERPOLATE_METERS {
            self.at(now)
        } else {
            self.value
        }
    }

    pub(crate) fn at(&self, now: Instant) -> f32 {
        let t = (now.duration_since(self.since).as_secs_f32() * 1000.0 / METER_INTERP_MS)
            .clamp(0.0, 1.0);
        self.prev + (self.value - self.prev) * t
    }

    pub(crate) fn is_settling(&self, now: Instant) -> bool {
        ((self.value - self.prev).abs() > f32::EPSILON
            && now.duration_since(self.since).as_secs_f32() * 1000.0 < METER_INTERP_MS)
            || shock(self.clip_at, now).is_some()
    }
}

/// The meter and the dB ruler share one column — the meter is a
/// translucent color wash filling the whole column, and the ruler's
/// numbers are drawn centered *on top of* it, like TotalMix — rather
/// than two separate side-by-side strips. The fader itself gets its own
/// column right next to it, sharing the ruler's tick marks (see
/// `draw_ruler`). Narrowed from `30` — that used to be sized for a
/// slim 7px pill *plus* the numbers in their own lane beside it; once
/// the numbers moved to sit on top of a full-width wash instead (see
/// `draw_meter`'s own doc comment — a live screenshot showed this is
/// what the reference actually does), the column only needs to be as
/// wide as the widest 2-digit label, not pill-width + gap + label-width
/// added together. The old, wider value was quietly pushing the fader
/// (and everything to its right — the icon column, the dB readout) well
/// past where the reference puts them.
///
/// These, and every other pixel constant in this file, are the sizes at
/// `scale == 1.0` (`theme::SCALE_DEFAULT`) — every widget here takes a
/// `scale` field/parameter and multiplies it in at both draw time and
/// hit-test time, so the window's adaptive scale (see
/// `app::recompute_ui_scale`) stays in sync between what's drawn and
/// what's clickable.
/// Narrowed again, 22→17, after the numbers-on-top-of-the-wash change
/// above made the wider value's original justification (room for a
/// pill *and* a label side by side) moot — 17px is the tightest this
/// goes before clipping a 2-digit label at `theme::TEXT_SM`, checked
/// via a live screenshot rather than guessed.
///
/// 2026-10-20: the numbers left the meter for a lane of their own. Printed
/// on the wash they collided with the fill (and, on a linked pair, with the
/// gap between the two bars), and the wash had to stay translucent just so
/// they could be read through it. The column is now `[meter][ruler lane]`,
/// the meter fully opaque; the whole column (`lane_w` plus the bar) is still one width, so
/// `layout_x` and the collapsed `vu_meter` keep sizing against one number.
/// The meter's bars (2026-11-03: 14 from 9, as the reference's meter is
/// about a fifth of its strip, a ladder you can read, not a stripe).
const METER_W: f32 = 14.0;
/// The meter's own inset frame round its bars (2026-11-03, the owner's
/// TotalMix screenshots: the meter sits in a frame of its own, with room
/// round it).
const METER_FRAME_PAD: f32 = 2.0;
const LANE_GAP: f32 = 2.0;
/// The strips without the row's numbers have no ruler lane: their scale
/// is drawn inside the meter, as dashes (`draw_meter_ticks`), as on the
/// reference. It was a 16px tick lane.
const RULER_LANE_W: f32 = 0.0;
/// The lane on the strip that carries its row's scale numbers (see
/// `Fader::ruler_labels`): wide enough for "−10" and "−∞" in the Mono face
/// at the 12px floor. 16px since the scale reads as a console fader's
/// engraving does, 0 10 20 40 ∞ with no minus signs (2026-11-03: the
/// "−40"s took 24px, and a row's first strip had no room left between
/// its meter and its fader, which the owner found "too close").
const LABELLED_LANE_W: f32 = 16.0;

/// The ruler lane's width, numbered or ticks only.
fn lane_w(labels: bool) -> f32 {
    if labels {
        LABELLED_LANE_W
    } else {
        RULER_LANE_W
    }
}

/// The meter column's width at scale 1: the numbers' lane (and its gap)
/// on a row's first strip, then the meter in its frame.
fn column_w(labels: bool) -> f32 {
    let lane = if labels { lane_w(true) + LANE_GAP } else { 0.0 };
    lane + METER_W + 2.0 * METER_FRAME_PAD
}
/// The air between the meter's frame and the fader's lane: as much as
/// the strip has room for up to `GAP` (the reference has about this
/// much), and never under `MIN_GAP`, the track moving right toward the
/// side keys first (a row's first strip, with its numbers). It was 4px,
/// and 1px on a row's first strip.
const GAP: f32 = 12.0;
const MIN_GAP: f32 = 8.0;
const TRACK_W: f32 = 26.0;

/// `Fader::layout_x` as a free function: the track's and the meter
/// column's x in a canvas `bounds_width` wide.
fn layout_x(bounds_width: f32, reserved_right: f32, track_from_right: Option<f32>, show_meter: bool, ruler_labels: bool, s: f32) -> (f32, f32) {
    let true_center = (bounds_width + reserved_right) / 2.0;
    let track_x = match track_from_right {
        Some(d) => (bounds_width - (d + TRACK_W) * s).max(0.0),
        None => true_center - (TRACK_W * s) / 2.0,
    };
    if !show_meter {
        // No meter, no `reserved_right` sibling either (see that
        // field's own doc comment) — plain self-centering, exactly
        // the pre-fix behavior for the Matrix view's compact fader.
        return (track_x.max(0.0), 0.0);
    }
    let column_w = column_w(ruler_labels) * s;
    // As much air as there's room for, up to `GAP`.
    let gap = (track_x - column_w).clamp(MIN_GAP * s, GAP * s);
    let meter_x = track_x - column_w - gap;
    if meter_x >= 0.0 {
        return (track_x, meter_x);
    }
    // Short of room: the track moves right (toward the side keys) as
    // far as the canvas allows, and only then does the air close up.
    let right_most = (bounds_width - TRACK_W * s).max(track_x);
    let moved = (track_x - meter_x).min(right_most);
    (moved, (moved - column_w - gap).max(0.0))
}

/// Where a strip fader's meter frame starts and its lane ends, from the
/// canvas's left, for the value box under them (critique #15: the box as
/// wide as the meter and the lane, no wider).
/// Where a strip fader's clip lights sit in its canvas, from its top left:
/// the meter frame's top, down past the lights (the strip's hint over
/// them; the owner, 2026-11-10: "I can click on the red two dots but I
/// dont know whats it for", "no tooltip or anything").
pub(crate) fn clip_lights_zone(bounds_width: f32, track_from_right: f32, ruler_labels: bool, s: f32) -> Rectangle {
    let (_, meter_x) = layout_x(bounds_width, 0.0, Some(track_from_right), true, ruler_labels, s);
    let (frame, _) = meter_column(meter_x, f32::MAX, s, ruler_labels);
    Rectangle::new(frame.position(), Size::new(frame.width, (METER_FRAME_PAD + CLIP_TOP + CLIP_H + 6.0) * s))
}

pub(crate) fn meter_to_lane(bounds_width: f32, track_from_right: f32, ruler_labels: bool, s: f32) -> (f32, f32) {
    let (track_x, meter_x) = layout_x(bounds_width, 0.0, Some(track_from_right), true, ruler_labels, s);
    let lane = if ruler_labels { (lane_w(true) + LANE_GAP) * s } else { 0.0 };
    let cx = track_x + TRACK_W * s / 2.0;
    (meter_x + lane, cx + LANE_W * s / 2.0)
}
pub(crate) const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Gap after the last `WheelScrolled` event that means "this is a new
/// gesture" rather than a continuation — wheel scrolling has no
/// press/release framing to mark that boundary explicitly.
pub(crate) const SCROLL_IDLE: Duration = Duration::from_millis(200);
/// Shift-drag sensitivity reduction: cursor travel over the whole track
/// only moves the value by this fraction of what a normal drag would.
/// Earlier this remapped `range` to a narrow absolute value window instead
/// (cap tracking the cursor 1:1 inside a "zoomed" scale) — that made the
/// cap's screen position discontinuous the instant the drag ended and the
/// range snapped back to full, and moved the unity reference dot along
/// with it. A plain relative-delta scale-down has no such range to snap.
pub(crate) const FINE_SENSITIVITY: f32 = 0.15;
/// The detent at the unity/default mark, in the hand's own travel at
/// `scale == 1.0`, like a detent on a real console fader: a hand passing
/// home is caught there and held until it has moved this far on (however
/// fast it moved), and a hand approaching home is held within
/// `snap_width` of it, never wider than half a dB. Shift goes through.
const SNAP_PX: f32 = 5.0;

/// How far past its mark a driven cap runs as it lands, at its peak, in
/// pixels at scale 1 (`feel::spring`).
const LANDING_PX: f32 = 5.0;

/// The detent's give as it catches the hand's cap, at its peak.
const CATCH_PX: f32 = 2.5;


/// Fader travel is dB-tapered (like real hardware / TotalMix), not linear
/// amplitude — a power curve (t = 1 - x^K, x = db/FLOOR_DB) gives generous
/// resolution near unity and compresses the bottom into silence, without
/// the sharp knee a two-segment mapping produces (which crowded the
/// 0/-6/-10 ruler ticks together). The track spans FLOOR_DB .. +6 dB
/// (TOP_DB), TotalMix's fader range.
const FLOOR_DB: f32 = -60.0;
/// The fader's top: +6 dB (2.0 linear) like TotalMix, not 0 dB.
const TOP_DB: f32 = 6.0;
const TAPER_K: f32 = 0.6;

fn db_to_t(db: f32) -> f32 {
    let db = db.clamp(FLOOR_DB, TOP_DB);
    let x = ((db - TOP_DB) / (FLOOR_DB - TOP_DB)).clamp(0.0, 1.0);
    (1.0 - x.powf(TAPER_K)).clamp(0.0, 1.0)
}

fn t_to_db(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let x = (1.0 - t).powf(1.0 / TAPER_K);
    (TOP_DB + x * (FLOOR_DB - TOP_DB)).clamp(FLOOR_DB, TOP_DB)
}

pub(crate) fn vol_to_t(vol: f32) -> f32 {
    if vol <= 0.0 {
        0.0
    } else {
        db_to_t(20.0 * vol.log10())
    }
}

pub(crate) fn t_to_vol(t: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else {
        (10f32.powf(t_to_db(t) / 20.0)).clamp(0.0, 2.0)
    }
}

/// A press on the fader: where along it, and the cap's extent if pressed
/// on the cap.
pub type OnPress<Message> = Box<dyn Fn(f32, Option<(f32, f32)>) -> Message>;

pub struct Fader<Message> {
    pub value: f32,
    pub range: (f32, f32),
    pub default_value: f32,
    /// Trim Gains (manual 21.3): where the channel's other active routings
    /// sit, drawn as small coral triangles beside the rail that move with
    /// the fader. Empty when Trim Gains is off.
    pub marks: Vec<f32>,
    /// Trim Gains is on for this strip: the slot glows coral, so the fader
    /// says it moves every mix (critique #12: only an 18px T said so).
    pub trim_lit: bool,
    /// The fader sets an FX send (or return): the slot glows lamp amber.
    pub fx_lit: bool,
    pub meter: MeterFrame,
    /// Whether `meter` reflects a real reading on this backend — see
    /// `draw_meter`'s doc comment. Irrelevant when `show_meter` is `false`.
    pub meter_available: bool,
    /// The right channel's own meter, `Some` only for a LINKED stereo
    /// pair's combined strip — splits the single meter column into two
    /// thin side-by-side bars instead of showing just `meter`'s (left/
    /// even channel's) level alone. `None` for every unlinked/mono
    /// strip, unaffected.
    pub meter2: Option<MeterFrame>,
    pub meter2_available: bool,
    /// The clip LED of `meter` / `meter2` is lit: the channel touched
    /// 0 dBFS since the LED was last cleared (it holds until then).
    pub clip: bool,
    pub clip2: bool,
    /// Sent by a click on the meter column while a clip LED is lit.
    pub on_clip_reset: Option<Message>,
    /// How far the channel's dynamics turn it down now (0 or less): light
    /// blue marks across the meter's top, one each 3 dB, as TotalMix
    /// shows them (the owner, 2026-11-10: "I dont see the little lights on
    /// the volume").
    pub reduction_db: f32,
    pub height: f32,
    pub show_meter: bool,
    /// Quick setup's "speak here" band (`quick::BAND`) as a faint bracket
    /// beside the meter: the mic strip's, until the mic guide is done
    /// (critique #56: gain taught where it happens).
    pub speak_band: bool,
    /// The meter rung in the lamp: the mix's own meter while the past-0 dB
    /// card says to watch it (`tricks::names_mix_meter`, critique #58).
    pub meter_named: bool,
    /// Extra width (at `scale == 1.0`... no, already-scaled pixels, same
    /// as every other field here) that exists *outside* this canvas's own
    /// bounds but should still count when centering the track — the strip
    /// card's icon column (gear/EQ/T), a `row!` sibling that shrinks this
    /// canvas's own `Length::Fill` allocation without the canvas ever
    /// knowing it's there. Ignored when `show_meter` is `false` (the
    /// Matrix view's compact fader, which has no such sibling — always
    /// `0.0` from that call site). See `layout_x`'s own doc comment for
    /// why this is needed at all, not just cosmetic tuning.
    pub reserved_right: f32,
    /// When set, the track is placed this far (at scale 1) from the
    /// canvas's right edge instead of centred on the row: the strip uses it
    /// so the lane keeps the same air to its side keys on every strip,
    /// a row's first one included (the reviewer measured 15px on most and
    /// 6px on the first; both are now even).
    pub track_from_right: Option<f32>,
    pub modifiers: Modifiers,
    pub scale: f32,
    /// `false` skips drawing the groove/cap entirely — the click/drag/
    /// wheel/double-click logic all stay fully live (the whole canvas
    /// still counts as "over track", see `layout_x`'s `!show_meter`
    /// branch), just invisible. Used by the Matrix view's crosspoint
    /// cells: a bare numeric dB label (see `label`) is the only visible
    /// thing, with a full drag-to-adjust fader hidden behind it —
    /// "hidden mini-fader" rather than a second, visually-competing
    /// control bolted onto the same cell.
    pub show_track: bool,
    /// Print the dB numbers in the ruler lane. Only the first full strip
    /// of each mixer row does: it stands at the row's left edge, so its
    /// numbers are the one shared scale the whole row reads against. Every
    /// other strip draws the same ticks without numbers.
    pub ruler_labels: bool,
    /// Drawn centered in the canvas when set, regardless of
    /// `show_track` — the Matrix view's own cell text (e.g. "-9.8"),
    /// left empty by every strip fader (`None`).
    pub label: Option<String>,
    pub label_color: Color,
    /// Canvas width when `show_meter` is `false` — ignored otherwise
    /// (a strip's own fader always fills its row, see `fader()`).
    /// Matters for real click/drag hit-testing, not just visuals: the
    /// whole canvas counts as "over track" when there's no meter
    /// column to exclude (`layout_x`'s `!show_meter` branch), so this
    /// is the actual clickable width, not just a cosmetic size. Used
    /// to be hardcoded to `TRACK_W` regardless of caller, leaving the
    /// Matrix view's cells with a ~26px hit target inside a visually
    /// larger 40px cell — small residual gap between "looks clickable"
    /// and "is clickable" that a matching width closes for free.
    pub compact_width: f32,
    pub on_press: OnPress<Message>,
    pub on_drag: Box<dyn Fn(f32) -> Message>,
    pub on_release: Box<dyn Fn() -> Message>,
    pub on_reset: Box<dyn Fn() -> Message>,
}

impl<Message> Fader<Message> {
    /// Left edge of the meter/ruler column and of the track, both in
    /// canvas-local x — laid out left-to-right as `[meter+ruler][gap]
    /// [track]`, matching the real TotalMix reference: the dB numbers are
    /// printed directly on the VU meter itself (see `draw_ruler` drawing
    /// on top of `draw_meter`'s same rect) and that combined column sits
    /// to the *left* of the fader, not the right — confirmed via a live
    /// screenshot at high zoom showing an actual green meter-fill segment
    /// low in that left column, right next to the fader rail. An earlier
    /// pass in this file had these swapped (`[track][meter+ruler]`,
    /// meter on the right) from a shallower read of a narrower crop that
    /// missed the live fill entirely — the user caught it.
    ///
    /// The *group* used to be centered on `bounds_width` alone — correct
    /// for keeping the meter/ruler on-screen (that was this fix's
    /// original point), but it centers the track on this *canvas's* own
    /// bounds, not the strip's. Since the strip's icon column
    /// (`reserved_right`) shrinks the canvas without the canvas knowing,
    /// the true center of the strip's content sits to the *right* of the
    /// canvas's own center — self-centering the group left the track a
    /// couple pixels right of the strip's real center (confirmed via a
    /// live screenshot pixel measurement: cap center 1.5-2px off from the
    /// card's true center), which the user then caught by eye. Centering
    /// the *track specifically* — not the meter+track group — on
    /// `(bounds_width + reserved_right) / 2` fixes that: the meter just
    /// goes wherever `GAP` puts it to the track's left, with a safety
    /// clamp for narrow bounds identical in spirit to the original
    /// negative-x fix this replaced (never let the meter go negative;
    /// shift the whole group right instead of letting it clip).
    fn layout_x(&self, bounds_width: f32) -> (f32, f32) {
        layout_x(bounds_width, self.reserved_right, self.track_from_right, self.show_meter, self.ruler_labels, self.scale)
    }

    /// True if `pos_x` — in the same (parent/absolute) coordinate space as
    /// the `bounds` a `canvas::Program` method receives, hence `bounds_x`
    /// to normalize it — falls on the track side of the meter/track gap.
    /// The meter/ruler column is a read-only VU display, not part of the
    /// control surface, even though it shares this canvas with the track.
    /// Every input handler that cares where on the canvas the cursor is
    /// (press, wheel, the grab cursor) goes through this one spot instead
    /// of repeating the boundary math, so a future change to the layout
    /// constants can't leave one of them checking a stale boundary.
    fn is_over_track(&self, pos_x: f32, bounds_x: f32, bounds_width: f32) -> bool {
        let (track_x, _) = self.layout_x(bounds_width);
        pos_x - bounds_x >= track_x - (GAP / 2.0) * self.scale
    }
}

#[derive(Default)]
pub struct State {
    /// The parts of the drawing that do not move with the signal, each
    /// rebuilt only when its key changes: the dB ruler (size and scale
    /// only) and the rail with its cap (value, default, range, drag). A
    /// mixer has ~70 faders and nearly all of them sit still, yet each
    /// re-tessellated its ruler text and rail on every redraw.
    ruler_cache: canvas::Cache,
    ruler_key: std::cell::Cell<u64>,
    /// The meter's frame, under the live level (see `draw`).
    frame_cache: canvas::Cache,
    frame_key: std::cell::Cell<u64>,
    track_cache: canvas::Cache,
    track_key: std::cell::Cell<u64>,
    dragging: bool,
    last_click: Option<Instant>,
    /// Last cursor Y seen during an active drag. Each move applies the
    /// delta since this point (scaled by `FINE_SENSITIVITY` while Shift is
    /// held) rather than an absolute cursor-to-value mapping, and is
    /// re-read fresh every move — so Shift can be pressed or released
    /// mid-drag and the cap keeps going from wherever it already was,
    /// instead of jumping (which a fixed press-time anchor, or falling
    /// back to absolute cursor tracking, would both cause).
    drag_pos: Option<f32>,
    /// Tapered position accumulated across the current drag. A real mouse
    /// fires several `CursorMoved` events per rendered frame — `self.value`
    /// only reflects the *last frame's* published value, so basing each
    /// event's target on `self.value` silently drops every event but the
    /// last one in a batch (each recomputes from the same stale baseline
    /// instead of building on what the previous event in that same batch
    /// already published). Accumulating here instead, in widget-local
    /// state that persists across every event regardless of render
    /// timing, makes the cap track the cursor 1:1 no matter how events
    /// happen to batch.
    drag_t: Option<f32>,
    /// Same accumulator problem as `drag_t`, for wheel-scroll nudges — a
    /// fast trackpad swipe fires many `Pixels` events per gesture, easily
    /// several per rendered frame. No press/release frames a scroll
    /// gesture the way a drag does, so a gesture boundary is inferred from
    /// an idle gap instead (`SCROLL_IDLE`).
    scroll_t: Option<f32>,
    last_scroll: Option<Instant>,
    /// Displayed (possibly still-easing) cap position, kept separate from
    /// `self.value` (the authoritative target) so a value change that
    /// *didn't* come from this fader's own drag — another channel moving
    /// together in a group selection, a scene load — eases into place
    /// instead of snapping, the same way the VU meter does. Not consulted
    /// while `dragging` is true: the user's own drag already updates on
    /// every mouse event, so layering interpolation on top of that would
    /// only add latency, not smoothness. A jump (a snapshot, undo, a
    /// reset) glides there like a motorised fader (`feel::Glide`).
    glide: Option<super::feel::Glide>,
    /// When the cap last landed on its home (the default level): the ring
    /// it sends out (`feel::splash`).
    splash: Option<Instant>,
    /// When the hand took hold of the cap, while it holds it (`feel`'s
    /// sweat).
    held_since: Option<Instant>,
    /// The hand's own travel this drag, before the detent holds it: leaving
    /// home takes `SNAP_PX` of real travel, however the moves arrive.
    raw_t: Option<f32>,
    /// Where the hand's travel stood when the detent caught it crossing
    /// home: it holds until the hand has moved `SNAP_PX` from there.
    caught: Option<f32>,

    /// A landing driven to a new level, or the detent's give: when it
    /// starts, and its amplitude in pixels (`feel::spring`).
    release: Option<(Instant, f32)>,
}

impl<Message: Clone> canvas::Program<Message> for Fader<Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let locate = |y: f32| -> f32 {
            let (lo, hi) = self.range;
            let run = travel(bounds, self.scale);
            let screen_t = 1.0 - ((y - run.y) / run.height.max(1.0)).clamp(0.0, 1.0);
            let (t_lo, t_hi) = (vol_to_t(lo), vol_to_t(hi));
            t_to_vol(t_lo + screen_t * (t_hi - t_lo))
        };

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_over(bounds)?;
                if !self.is_over_track(pos.x, bounds.x, bounds.width) {
                    // The meter column: a click there clears a lit clip
                    // LED, and otherwise falls through to the strip.
                    return self
                        .on_clip_reset
                        .clone()
                        .map(|m| canvas::Action::publish(m).and_capture());
                }
                let now = Instant::now();
                let is_double = state
                    .last_click
                    .is_some_and(|t| now.duration_since(t) < DOUBLE_CLICK);

                if is_double {
                    state.dragging = false;
                    state.last_click = None;
                    state.drag_pos = None;
                    state.drag_t = None;
                    state.held_since = None;
                    state.splash = Some(now);
                    return Some(canvas::Action::publish((self.on_reset)()).and_capture());
                }
                state.last_click = Some(now);
                state.dragging = true;
                state.held_since = Some(now);

                let value = locate(pos.y);
                state.drag_pos = Some(pos.y);
                state.drag_t = Some(vol_to_t(value));
                state.raw_t = Some(vol_to_t(value));
                state.caught = None;
                // A fresh press starts no old swing.
                state.release = None;
                Some(canvas::Action::publish((self.on_press)(value, None)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    return None;
                }
                let pos = cursor.land().position()?;
                let prev_y = state.drag_pos.unwrap_or(pos.y);
                let run_h = travel(bounds, self.scale).height.max(1.0);
                let screen_dt = -(pos.y - prev_y) / run_h;
                let mult = if self.modifiers.shift() {
                    FINE_SENSITIVITY
                } else {
                    1.0
                };
                let base_t = state.drag_t.unwrap_or_else(|| vol_to_t(self.value));
                // The hand's own travel, which the detent doesn't swallow.
                let prev_raw = state.raw_t.unwrap_or(base_t);
                let raw = (prev_raw + screen_dt * mult).clamp(0.0, 1.0);
                state.raw_t = Some(raw);
                let default_t = vol_to_t(self.default_value);
                let snap_t = snap_width(self.default_value, run_h, self.scale);
                let fine = self.modifiers.shift();
                // The detent: the hand passing home is caught there, however
                // fast it moves, and held until it has travelled `SNAP_PX`
                // on (the reviewer: at half a dB wide, a drag passed it in
                // one move); a hand approaching home is held within
                // `snap_width` of it (never wider than half a dB, so levels
                // near home stay within reach). Shift (fine) goes through.
                let catch_t = (SNAP_PX * self.scale) / run_h;
                let passing = prev_raw != default_t && (prev_raw - default_t) * (raw - default_t) <= 0.0;
                if !fine && passing && state.caught.is_none() {
                    // From no further than the hold past home: a flick
                    // through 0 dB to an end of the travel lets go on that
                    // same move (caught at the end, the hand could never
                    // travel on from it, and the cap stayed at 0 dB).
                    state.caught = Some(raw.clamp(default_t - catch_t, default_t + catch_t));
                }
                if state.caught.is_some_and(|c| fine || (raw - c).abs() >= catch_t) {
                    state.caught = None;
                }
                let t = if !fine && (state.caught.is_some() || (raw - default_t).abs() < snap_t) { default_t } else { raw };
                let now = Instant::now();
                // Landing on home splashes, with the detent's tick
                // (`sounds`), and the cap gives a little as it's caught
                // (`feel::spring`); passing through it does too, but not
                // under Shift, which goes through quietly.
                let crossed = base_t != default_t && (base_t - default_t) * (t - default_t) <= 0.0;
                if crossed && (!fine || t == default_t) {
                    state.splash = Some(now);
                    if t == default_t {
                        let amp = CATCH_PX / super::feel::SPRING_PEAK;
                        state.release = Some((now, if base_t < t { amp } else { -amp }));
                    }
                }

                state.drag_pos = Some(pos.y);
                state.drag_t = Some(t);
                Some(canvas::Action::publish((self.on_drag)(t_to_vol(t))).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return None;
                }
                state.dragging = false;
                state.drag_pos = None;
                state.drag_t = None;
                state.raw_t = None;
                state.caught = None;
                // While dragging, draw() reads self.value directly and
                // never touches `display` — resync it to the just-released
                // position now, so the next external change eases in from
                // here rather than from whatever `display` was cached at
                // before the drag started (which would show as a snap-back
                // glitch the instant dragging ends).
                state.glide = Some(super::feel::Glide::still(self.value));
                state.held_since = None;
                Some(canvas::Action::publish((self.on_release)()).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if crate::widgets::wheel_off() {
                    return None;
                }
                let pos = cursor.position_over(bounds)?;
                if !self.is_over_track(pos.x, bounds.x, bounds.width) {
                    return None;
                }
                // Ctrl+scroll never nudges a value here — held down, it
                // reads as "the user means something other than a fine
                // adjustment," whatever that turns out to be (OS-level
                // zoom, a future shortcut, ...), so leave it uncaptured
                // rather than move the fader underneath it.
                if self.modifiers.control() {
                    return None;
                }
                // Lines are discrete wheel detents (dy is usually ±1 per
                // click); Pixels are a continuous trackpad/high-res stream
                // (many small events per gesture) — they need very
                // different per-event magnitudes or one feels dead and the
                // other feels like it teleports. Stepping in tapered
                // t-space (not raw linear volume) also keeps the nudge feel
                // consistent across the whole dB range instead of vanishing
                // near unity, where a fixed amplitude step is only a
                // fraction of a dB.
                let (dy, base_step) = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => (*y, 0.03),
                    mouse::ScrollDelta::Pixels { y, .. } => (*y, 0.0015),
                };
                if dy == 0.0 {
                    return None;
                }
                let mult = if self.modifiers.shift() { 0.25 } else { 1.0 };
                let now = Instant::now();
                let fresh_gesture = state
                    .last_scroll
                    .is_none_or(|t| now.duration_since(t) > SCROLL_IDLE);
                let base_t = if fresh_gesture {
                    vol_to_t(self.value)
                } else {
                    state.scroll_t.unwrap_or_else(|| vol_to_t(self.value))
                };
                let new_t = (base_t + dy * base_step * mult).clamp(0.0, 1.0);
                state.scroll_t = Some(new_t);
                state.last_scroll = Some(now);
                let value = t_to_vol(new_t);
                Some(canvas::Action::publish((self.on_drag)(value)).and_capture())
            }
            // Keeps redrawing at full display refresh rate while the meter
            // and/or the cap itself are still interpolating — settles back
            // to the normal Tick-driven rate on its own once caught up
            // (see `MeterFrame`).
            canvas::Event::Window(window::Event::RedrawRequested(now)) => {
                let mut still_animating =
                    INTERPOLATE_METERS && self.show_meter && self.meter.is_settling(*now);

                if !state.dragging {
                    let shown = state.glide.map_or(self.value, |g| g.at(*now));
                    let jump = (vol_to_t(self.value) - vol_to_t(shown)).abs() > super::feel::JUMP;
                    let glide = state
                        .glide
                        .get_or_insert_with(|| super::feel::Glide::still(self.value));
                    // Driven there (a recall, Undo, a reset), the cap lands
                    // as a motorised fader does: past its mark and back
                    // (critique #11: the fun belongs to the motor, not to a
                    // hand's release): running on 5px past its mark at the
                    // peak, from 60% through the glide.
                    // A new target, driven (not under a hand: not the
                    // wheel's, not a group's while a fader is held).
                    let by_hand = crate::widgets::hand_on_fader()
                        || state.last_scroll.is_some_and(|t| now.saturating_duration_since(t) < SCROLL_IDLE);
                    let moved = glide.target() != self.value;
                    glide.retarget(self.value, *now, jump);
                    let up = self.value > shown;
                    if moved && jump && !by_hand {
                        state.release = Some((glide.overshoot_at(), LANDING_PX / super::feel::SPRING_PEAK * if up { 1.0 } else { -1.0 }));
                    } else if moved {
                        // A follow step: a landing still to come keeps to
                        // the glide's new end, never firing after it.
                        if let Some((_, amp)) = state.release.filter(|(at, _)| *at > *now) {
                            state.release = Some((glide.overshoot_at(), amp.abs() * if up { 1.0 } else { -1.0 }));
                        }
                    }
                    still_animating |= glide.settling(*now);
                }
                still_animating |= super::feel::splash(state.splash, *now).is_some();
                still_animating |= super::feel::spring(state.release, *now).is_some();
                // A clip's shockwave runs at the display's rate, whatever
                // the meters' own interpolation.
                still_animating |= self.show_meter
                    && (shock(self.meter.clip_at, *now).is_some() || self.meter2.is_some_and(|m| shock(m.clip_at, *now).is_some()));
                still_animating |= state.dragging && super::feel::sweating(state.held_since, *now);

                if still_animating {
                    Some(canvas::Action::request_redraw())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let (track_x, meter_x) = self.layout_x(bounds.width);
        let mut layers: Vec<Geometry> = Vec::with_capacity(4);
        let now = Instant::now();
        let display_value = if state.dragging {
            self.value
        } else {
            state.glide.map(|g| g.at(now)).unwrap_or(self.value)
        };
        // A driven cap's landing, or the detent's give under the hand: in
        // the picture only.
        let display_value = match super::feel::spring(state.release, now) {
            Some(px) => {
                let run_h = travel(bounds, self.scale).height.max(1.0);
                t_to_vol((vol_to_t(display_value) + px * self.scale / run_h).clamp(0.0, 1.0))
            }
            _ => display_value,
        };
        let track_rect = Rectangle::new(
            Point::new(track_x, 0.0),
            Size::new(TRACK_W * self.scale, bounds.height),
        );
        // Under the cursor (anywhere on the track, where a press acts) the
        // cap comes forward, so the fader says it's ready.
        let hovered = self.show_track
            && !crate::widgets::locked()
            && (state.dragging
                || cursor
                    .position_over(bounds)
                    .is_some_and(|pos| self.is_over_track(pos.x, bounds.x, bounds.width)));

        // Static layer under everything: the meter's frame, a cut in the
        // panel with its bevel (a dozen strokes, so drawn once).
        if self.show_meter {
            let key = hash_of(|h| {
                crate::theme::palette_key().hash(h);
                (bounds.width.to_bits(), bounds.height.to_bits(), self.scale.to_bits(), self.ruler_labels).hash(h);
                meter_x.to_bits().hash(h);
            });
            if state.frame_key.replace(key) != key {
                state.frame_cache.clear();
            }
            // Drawn below, as a picture with exact edge smoothing, in the
            // layer under this canvas (`FaderBase`): its rounded corners
            // stepped here. Kept as a cache for the frame's key only.
            let _ = (&state.frame_cache, meter_x);
        }

        // Dynamic layer: the level (and the label, on the cells that have
        // one). Redrawn every time.
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        if self.show_meter {
            let (meter_rect, _) = meter_column(meter_x, bounds.height, self.scale, self.ruler_labels);
            draw_meter(
                &mut frame,
                meter_rect,
                meter_scale_inset(self.scale),
                self.meter,
                self.scale,
                (self.meter_available, self.clip),
                self.meter2
                    .map(|m| (m, self.meter2_available, self.clip2)),
                false,
            );
            if self.reduction_db < -0.5 {
                draw_reduction(&mut frame, meter_rect, self.reduction_db, self.scale);
            }
            if self.speak_band {
                draw_speak_band(&mut frame, meter_rect, self.scale);
            }
            if self.meter_named {
                // Breathing, so the eye finds it (critique #59; still with
                // animations off: `breath` is 0, the ring full).
                let b = crate::motion::breath();
                let ring = Color { a: 1.0 - 0.45 * b, ..theme::readable_on_floors(theme::lamp()) };
                frame.stroke(
                    &Path::rounded_rectangle(meter_rect.position(), meter_rect.size(), (3.0 * self.scale).into()),
                    Stroke::default().with_color(ring).with_width(2.5 * self.scale),
                );
            }
        }
        // The splash and the sweat, round the cap (under it: the cap lies
        // on the layer above).
        if self.show_track {
            let cap = cap_rect(track_rect, display_value, self.range, self.scale);
            if let Some(p) = super::feel::splash(state.splash, now) {
                draw_cap_splash(&mut frame, cap, p, self.scale);
            }
            if let (true, Some(held)) = (state.dragging, state.held_since) {
                if super::feel::sweating(Some(held), now) {
                    super::feel::draw_sweat(&mut frame, held, now, cap.position(), (cap.width, cap.height), self.scale);
                }
            }
        }
        if let Some(label) = &self.label {
            frame.fill_text(canvas::Text {
                content: label.clone(),
                position: Point::new(bounds.width / 2.0, bounds.height / 2.0),
                color: self.label_color,
                size: (theme::TEXT_SM * self.scale).into(),
                align_x: advanced_text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        layers.push(frame.into_geometry());

        // Static layer 1: the ruler's ticks and numbers.
        if self.show_meter {
            let key = hash_of(|h| {
                crate::theme::palette_key().hash(h);
                (
                    bounds.width.to_bits(),
                    bounds.height.to_bits(),
                    self.scale.to_bits(),
                    self.ruler_labels,
                )
                    .hash(h);
                meter_x.to_bits().hash(h);
                (self.meter_available, self.meter2.is_some(), self.meter2_available).hash(h);
            });
            if state.ruler_key.replace(key) != key {
                state.ruler_cache.clear();
            }
            layers.push(state.ruler_cache.draw(renderer, bounds.size(), |frame| {
                let (meter_rect, ruler_rect) =
                    meter_column(meter_x, bounds.height, self.scale, self.ruler_labels);
                if self.ruler_labels {
                    draw_ruler(frame, ruler_rect, self.scale, true);
                }
                if self.meter_available || self.meter2_available {
                    draw_meter_ticks(frame, meter_rect, meter_scale_inset(self.scale), self.scale);
                }
                // The meter's segments, over the live fill below this layer.
                draw_ladders(
                    frame,
                    meter_rect,
                    self.scale,
                    self.meter_available,
                    self.meter2.map(|_| self.meter2_available),
                );
            }));
        }

        // Static layer 2: the rail and its cap, which only move with the
        // value.
        if self.show_track {
            let key = hash_of(|h| {
                crate::theme::palette_key().hash(h);
                (
                    bounds.width.to_bits(),
                    bounds.height.to_bits(),
                    self.scale.to_bits(),
                )
                    .hash(h);
                (display_value.to_bits(), self.default_value.to_bits()).hash(h);
                (self.range.0.to_bits(), self.range.1.to_bits()).hash(h);
                (state.dragging, hovered, track_x.to_bits(), self.trim_lit, self.fx_lit).hash(h);
                for m in &self.marks {
                    m.to_bits().hash(h);
                }
            });
            if state.track_key.replace(key) != key {
                state.track_cache.clear();
            }
            layers.push(state.track_cache.draw(renderer, bounds.size(), |frame| {
                draw_marks(frame, track_rect, &self.marks, self.range, self.scale);
                draw_track(
                    frame,
                    track_rect,
                    display_value,
                    self.default_value,
                    self.range,
                    (state.dragging, hovered, if self.fx_lit { Some(theme::lamp()) } else if self.trim_lit { Some(theme::accent()) } else { None }),
                    self.scale,
                    self.show_meter,
                );
            }));
        }
        layers
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // The cursor says what a press will do: an open hand over the cap,
        // a closed one holding it, crosshairs for a fine move (Shift), a
        // pointing hand on the rest of the track (a click there brings the
        // cap), a pointing hand on a lit clip light (a click clears it),
        // and "not allowed" while the mix is locked.
        if state.dragging {
            return if self.modifiers.shift() {
                mouse::Interaction::Crosshair
            } else {
                mouse::Interaction::Grabbing
            };
        }
        let Some(pos) = cursor.position_over(bounds) else {
            return mouse::Interaction::None;
        };
        if !self.is_over_track(pos.x, bounds.x, bounds.width) {
            let clip_lit = self.clip || self.clip2;
            return if clip_lit && self.on_clip_reset.is_some() {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::None
            };
        }
        if crate::widgets::locked() {
            return mouse::Interaction::NotAllowed;
        }
        let (track_x, _) = self.layout_x(bounds.width);
        let track = Rectangle::new(Point::new(track_x, 0.0), Size::new(TRACK_W * self.scale, bounds.height));
        let shown = state.glide.map_or(self.value, |g| g.at(Instant::now()));
        let cap = cap_rect(track, shown, self.range, self.scale).expand(2.0 * self.scale);
        if cap.contains(Point::new(pos.x - bounds.x, pos.y - bounds.y)) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::Pointer
        }
    }
}

/// Splits the meter/ruler column starting at `x` into the lane the dB
/// numbers sit in and the bar itself, right of it: the bar runs beside the
/// fader rail, as TotalMix puts its level next to the fader, instead of at
/// the card's left edge with the numbers between it and the rail.
///
/// The meter's well is the lane's twin (the owner's reference: the meter
/// and the fader's lane share their top and foot): it stands the lane's
/// margin in from the fader's ends; its scale, and the ruler's, run over
/// the cap's travel inside it (`meter_scale_inset`).
fn meter_column(x: f32, height: f32, scale: f32, labels: bool) -> (Rectangle, Rectangle) {
    let lane = lane_w(labels);
    let run = travel(Rectangle::new(Point::new(x, 0.0), Size::new(0.0, height)), scale);
    let ruler = Rectangle::new(Point::new(x, run.y), Size::new(lane * scale, run.height));
    let margin = (LANE_MARGIN * scale).min(height / 2.0);
    let before = if labels { lane + LANE_GAP } else { 0.0 };
    let frame = Rectangle::new(
        Point::new(x + before * scale, margin),
        Size::new((METER_W + 2.0 * METER_FRAME_PAD) * scale, height - 2.0 * margin),
    );
    (frame, ruler)
}

/// The meter's bars inside its frame.
fn meter_bars(frame: Rectangle, scale: f32) -> Rectangle {
    let pad = (METER_FRAME_PAD * scale).min(frame.width / 2.0).min(frame.height / 2.0);
    Rectangle::new(
        Point::new(frame.x + pad, frame.y + pad),
        Size::new(frame.width - 2.0 * pad, frame.height - 2.0 * pad),
    )
}

/// "Speak here", beside the meter: a thin green bracket from the band's
/// foot to its top (`quick::BAND`, -18 to -6 dBFS), where a voice's peaks
/// should land, as Quick setup's meter shows it.
fn draw_speak_band(frame: &mut Frame, meter: Rectangle, scale: f32) {
    let (top, foot) = speak_band_ys(meter, scale);
    let x = meter.x + meter.width + 2.5 * scale;
    // Quick setup's green for the same band (critique #57: in the lamp's
    // amber it read as a stray line, not "speak here").
    // The meters' own fixed green (Nord's theme green read grey there,
    // Solarized's olive; critique #58), deepened only as far as the floor
    // needs.
    let hue = theme::readable_on_floors(theme::METER_SIGNAL);
    let ink = Color { a: 0.9, ..hue };
    let bracket = Path::new(|b| {
        b.move_to(Point::new(x - 2.0 * scale, top));
        b.line_to(Point::new(x, top));
        b.line_to(Point::new(x, foot));
        b.line_to(Point::new(x - 2.0 * scale, foot));
    });
    frame.stroke(&bracket, Stroke::default().with_color(ink).with_width(1.5 * scale));
}

/// The bracket's top and foot: on the meter's own scale, which stands
/// `meter_scale_inset` in from the well's ends as its ticks and fill do
/// (critique #57: mapped over the whole well, it marked about -20 to -2
/// dB at 1440x900, its top at the red zone).
fn speak_band_ys(meter: Rectangle, scale: f32) -> (f32, f32) {
    let r = meter_scale_rect(meter, meter_scale_inset(scale), scale);
    let (lo, hi) = crate::quick::BAND;
    let y = |db: f32| r.y + r.height - r.height * db_to_t(db);
    (y(hi), y(lo))
}

/// The rectangle a meter's scale runs over: its bars' width, `inset` in
/// from the well's ends.
fn meter_scale_rect(outer: Rectangle, inset: f32, scale: f32) -> Rectangle {
    let bars = meter_bars(outer, scale);
    let inset = inset.clamp(0.0, outer.height / 2.0);
    Rectangle::new(Point::new(bars.x, outer.y + inset), Size::new(bars.width, (outer.height - 2.0 * inset).max(0.0)))
}

/// The scale drawn inside a meter (the reference's dashes, with the 0 dB
/// one longer and brighter): over the bars in `frame`, on the scale that
/// stands `inset` in from the frame's ends. A light neutral, as the bars'
/// well is dark in every theme.
fn draw_meter_ticks(frame: &mut Frame, outer: Rectangle, inset: f32, scale: f32) {
    let bars = meter_bars(outer, scale);
    let r = meter_scale_rect(outer, inset, scale);
    for db in [0.0, -10.0, -20.0, -40.0] {
        let y = (r.y + r.height - r.height * db_to_t(db)).round();
        let (inset_x, alpha) = if db == 0.0 { (0.5 * scale, 0.85) } else { (2.0 * scale, 0.45) };
        frame.fill_rectangle(
            Point::new(bars.x + inset_x, y),
            Size::new((bars.width - 2.0 * inset_x).max(0.0), 1.0),
            Color { a: alpha, ..METER_TICK },
        );
    }
}

/// The meter's printed scale: a light cool grey, for the dark well.
const METER_TICK: Color = Color::from_rgb8(0xa8, 0xb2, 0xbe);

/// How far a strip meter's scale stands in from its well's ends: the
/// well is the lane's twin, the scale is the cap's travel.
fn meter_scale_inset(scale: f32) -> f32 {
    travel_inset(scale) - LANE_MARGIN * scale
}

const METER_RADIUS: f32 = 2.0;
const CLIP_H: f32 = 6.0;
/// How far the clip LED stands in from the top of the meter's well.
const CLIP_TOP: f32 = 2.0;

/// `f32::clamp` panics if `lo > hi` — which a plain `min - margin, max -
/// margin` pair can produce for a single bad frame: iced 0.14 has a layout
/// quirk where a `Canvas` inside a horizontally-scrollable row can receive
/// a stale, much-too-small `bounds` for one frame right after its declared
/// `Length::Fixed` size changes (observed when a window resize changes
/// `ui_scale`, and so every strip's width, mid-session — see
/// `app::recompute_ui_scale`; the next frame recovers on its own).
/// Drawing code must never crash the whole app over a transient bad
/// layout, so every clamp against a widget's own bounds goes through this
/// instead of the raw method.
pub(crate) fn safe_clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if lo <= hi {
        v.clamp(lo, hi)
    } else {
        (lo + hi) / 2.0
    }
}

/// The meter's color zones, bottom to top, as (upper edge in dB, color):
/// signal green up to -6 dB, amber to -3 dB, red above. The last edge is
/// the top of the scale, so every level falls in some zone. Green to -6,
/// as Quick setup's meter and the speak-here band (-18 to -6) say a good
/// voice is: from -12 a voice at the right level read amber, "too hot", in
/// the Mixer (critique #58, the recommended pick: one rule everywhere).
const METER_ZONES: [(f32, Color); 3] = [
    (-6.0, theme::METER_SIGNAL),
    (TM_HOT_DB, theme::METER_ATTENTION),
    (TOP_DB, theme::METER_CLIP),
];

/// Fill opacity. It used to be 0.55 so the ruler's numbers could be read
/// *through* the wash; they have their own lane now (`RULER_LANE_W`), so
/// the fill is all but solid, with just enough give to sit in the dark
/// well instead of glaring.
const FILL_ALPHA: f32 = 0.92;

/// The unlit ladder's strength (`draw_meter_bar`).
const METER_GHOST_ALPHA: f32 = 0.18;
/// The unlit segments above 0 dBFS: a dim warm grey continuing the red
/// zone's ghost (the reference's unlit LEDs there are brownish). A cooler,
/// darker grey still read as "strange black space" (the owner).
const METER_UNLIT: Color = Color::from_rgb8(0xb8, 0x8c, 0x88);
const METER_UNLIT_ALPHA: f32 = 0.22;

/// The meter's LED segments: one segment every `SEGMENT_PITCH` px, split
/// by a 1px line of the well's dark. Drawn once into the ruler's cached
/// layer, which lies over the live fill, so the ladder costs nothing per
/// frame.
const SEGMENT_PITCH: f32 = 4.0;

/// The well behind a meter (and the line between its segments).
const METER_WELL: Color = Color::from_rgb8(0x08, 0x08, 0x0a);
/// The dashes a meter shows where the backend doesn't measure its channel.
const METER_NO_SIGNAL: Color = Color::from_rgb8(0x3a, 0x3a, 0x3e);
/// The clip LED unlit: a dark red at rest.
const CLIP_UNLIT: Color = Color::from_rgb8(0x3a, 0x16, 0x16);

/// The meter's segment lines over `r` (one bar), below the clip LED.
fn draw_ladder(frame: &mut Frame, r: Rectangle, scale: f32) {
    let pitch = (SEGMENT_PITCH * scale).max(2.0);
    let start = r.y + (CLIP_TOP + CLIP_H + 1.0) * scale;
    let ladder = Path::new(|b| {
        let mut y = r.y + r.height - pitch;
        while y > start {
            b.rectangle(Point::new(r.x, y.round()), Size::new(r.width, 1.0));
            y -= pitch;
        }
    });
    frame.fill(&ladder, METER_WELL);
}

/// `draw_ladder` over a meter column `r`, split as `draw_meter` splits it:
/// a pair's two bars each get their own (`right` is the second bar's
/// availability), and a bar that isn't measured keeps its plain dashes.
fn draw_ladders(frame: &mut Frame, outer: Rectangle, scale: f32, available: bool, right: Option<bool>) {
    let r = meter_bars(outer, scale);
    let Some(available2) = right else {
        if available {
            draw_ladder(frame, r, scale);
        }
        return;
    };
    let gap = 1.0 * scale;
    let half_w = ((r.width - gap) / 2.0).max(0.0);
    let size = Size::new(half_w, r.height);
    if available {
        draw_ladder(frame, Rectangle::new(r.position(), size), scale);
    }
    if available2 {
        draw_ladder(frame, Rectangle::new(Point::new(r.x + half_w + gap, r.y), size), scale);
    }
}

/// A full-width wash spanning the whole ruler column — not a slim pill
/// off to one side — precisely so `draw_ruler`'s numbers can be
/// superimposed on it rather than pushed into their own extra lane
/// (which used to widen this whole column well past what the numbers
/// alone need, shoving the fader and everything after it further right
/// than the reference: see `lane_w`). Tints
/// through `METER_ZONES` (green, amber, red), plus a
/// separate clip "LED" above the track that lights up near 0 dBFS.
/// `available` is a per-session backend capability (see `DeviceHandle::
/// has_input_meter`/`has_playback_meters`), not a per-tick reading —
/// when `false`, the channel has no real level data on this backend
/// (e.g. playback meters on every real backend today; input meters too
/// on the ALSA/kernel-driver backend, see `PROTOCOL.md`'s "VU meters:
/// conclusion"). Rendering that as a normal meter pinned at 0 would read
/// as "silence", which is a different claim than "not measured" — so
/// this draws a dashed, uncolored track instead of pretending there's a
/// live reading.
/// Dispatches to one full-width bar, or — when `right` is `Some` — two
/// thin bars side by side sharing `r` (same height as a single bar,
/// half the width each, a small gap between): a linked stereo pair's
/// meter in real TotalMix isn't one wide bar, it's the two mono meters
/// placed edge to edge (user-confirmed against the reference, having
/// caught this session's own earlier gap where a linked pair silently
/// only ever showed its left/even channel's level).
/// A cut in the panel at `r` with corners of `radius` (`theme::Inset`):
/// the lit lip under it, its shade inside, its walls (the top and left in
/// shadow, the bottom and right lit, as light falling from the top left),
/// and its rim (`rim`, or the inset's own).
pub(crate) fn draw_inset(frame: &mut Frame, r: Rectangle, radius: f32, scale: f32, rim: Option<Color>) {
    draw_inset_pen(frame, r, radius, scale, rim);
}

/// `draw_inset` with any `knob_paint::Pen`: the canvas, or a picture drawn
/// with exact edge smoothing (the knob's value box: its rounded corners
/// stepped under the GPU's four samples a pixel; the owner, 2026-11-14).
pub(crate) fn draw_inset_pen(pen: &mut dyn super::knob_paint::Pen, r: Rectangle, radius: f32, scale: f32, rim: Option<Color>) {
    use super::knob_paint::Ink;
    let m = theme::inset();
    let radius = radius.min(r.width / 2.0).min(r.height / 2.0);
    let at = |x: f32, y: f32, w: f32, h: f32| Rectangle::new(Point::new(x, y), Size::new(w.max(0.0), h.max(0.0)));
    pen.rrect(at(r.x, r.y + 1.0 * scale, r.width, r.height), radius, Ink::Solid(m.lip));
    let cut = at(r.x, r.y, r.width, r.height);
    let depth = (6.0 * scale / r.height.max(1.0)).clamp(0.0, 0.5);
    pen.rrect(cut, radius, Ink::Linear(Point::new(r.x, r.y), Point::new(r.x, r.y + r.height), vec![(0.0, m.top), (depth, m.foot), (1.0, m.foot)]));
    // An inner shadow three pixels deep down the top and left walls,
    // fading inward, and a lit line along the bottom and right walls: the
    // cut's depth, as light falling from the top left. Drawn as nested
    // strokes, the canvas has no blur.
    let clear = Color::TRANSPARENT;
    let fade = |c: Color, k: f32| Color { a: c.a * k, ..c };
    let down = |c0: Color, c1: Color| {
        Ink::Linear(
            Point::new(r.x, r.y),
            Point::new(r.x, r.y + r.height),
            vec![
                (0.0, c0),
                ((4.0 * scale / r.height.max(1.0)).clamp(0.0, 0.45), fade(c0, 0.35)),
                (0.5, clear),
                ((1.0 - 3.0 * scale / r.height.max(1.0)).clamp(0.55, 1.0), fade(c1, 0.3)),
                (1.0, c1),
            ],
        )
    };
    let across = |c0: Color, c1: Color| {
        Ink::Linear(Point::new(r.x, r.y), Point::new(r.x + r.width, r.y), vec![(0.0, c0), (0.35, clear), (0.65, clear), (1.0, c1)])
    };
    for (k, depth) in [(1.0f32, 1.0f32), (2.0, 0.55), (3.0, 0.25)] {
        let d = k * 0.9 * scale;
        let wall = at(r.x + d, r.y + d, r.width - 2.0 * d, r.height - 2.0 * d);
        let glint = if k == 1.0 { m.glint } else { clear };
        pen.rrect_stroke(wall, (radius - d).max(0.0), 1.0 * scale, down(fade(m.shadow, depth), glint));
        if m.sides > 0.0 {
            pen.rrect_stroke(wall, (radius - d).max(0.0), 1.0 * scale, across(fade(m.shadow, depth * m.sides), fade(glint, 0.7 * m.sides)));
        }
    }
    pen.rrect_stroke(cut, radius, 1.0, Ink::Solid(rim.unwrap_or(m.rim)));
}

/// The dynamics' turn-down over a meter: light blue marks across the bars
/// from just under the clip lights down, one each 3 dB (up to 24), on
/// whole pixels (critique #31: on a fractional y a mark smeared over two
/// rows, one of three at 2.52; written by Qwen, checked here).
fn draw_reduction(frame: &mut Frame, outer: Rectangle, db: f32, scale: f32) {
    let bars = meter_bars(outer, scale);
    let marks = ((-db / 3.0).floor() as usize).min(8);
    let blue = Color::from_rgb8(0x6c, 0xc4, 0xf0);

    let height = (1.5 * scale).round().max(1.0);
    let step = ((3.0 * scale).round()).max(height + 1.0);
    let first_offset = (10.0 * scale).round();
    let base_y = bars.y.round() + first_offset;

    for k in 0..marks {
        let y = base_y + k as f32 * step;
        frame.fill_rectangle(
            Point::new(bars.x, y),
            Size::new(bars.width, height),
            Color { a: 0.95, ..blue },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_meter(
    frame: &mut Frame,
    outer: Rectangle,
    inset: f32,
    meter: MeterFrame,
    scale: f32,
    (available, clip): (bool, bool),
    right: Option<(MeterFrame, bool, bool)>,
    framed: bool,
) {
    // The frame: a cut like the lane beside it, the bars inside it. A strip
    // fader keeps its frames in a cached layer under this one (`framed`
    // false), as they don't move.
    if framed {
        draw_inset(frame, outer, 4.0 * scale, scale, None);
    }
    let r = meter_bars(outer, scale);
    let inset = inset - METER_FRAME_PAD * scale;
    let now = Instant::now();
    let level = (meter.shown(now), meter.peak, meter.rms, shock(meter.clip_at, now));
    let Some((meter2, available2, clip2)) = right else {
        draw_meter_bar(frame, r, inset, level, scale, available, clip);
        return;
    };
    let level2 = (meter2.shown(now), meter2.peak, meter2.rms, shock(meter2.clip_at, now));
    let gap = 1.0 * scale;
    let half_w = ((r.width - gap) / 2.0).max(0.0);
    let left_rect = Rectangle::new(r.position(), Size::new(half_w, r.height));
    let right_rect = Rectangle::new(
        Point::new(r.x + half_w + gap, r.y),
        Size::new(half_w, r.height),
    );
    draw_meter_bar(frame, left_rect, inset, level, scale, available, clip);
    draw_meter_bar(frame, right_rect, inset, level2, scale, available2, clip2);
}

/// How strong the peak part above an RMS bar is drawn (the RMS part is
/// `FILL_ALPHA`): the same zone colours, lighter, so the loudness reads as
/// the bar and the transients as its glow.
const PEAK_OVER_RMS_ALPHA: f32 = 0.38;

/// What `rms` is shown as, by the RMS display mode (`widgets::rms_mode`):
/// `None` for peaks only (or not measured).
fn shown_rms(rms: f32) -> Option<f32> {
    match crate::widgets::rms_mode() {
        _ if rms < 0.0 => None,
        0 => None,
        1 => Some(rms),
        _ => Some(rms * 1.412_537_5),
    }
}

fn draw_meter_bar(
    frame: &mut Frame,
    well: Rectangle,
    inset: f32,
    (level, peak, rms, shock): (f32, f32, f32, Option<f32>),
    scale: f32,
    available: bool,
    clipping: bool,
) {
    // The well is the whole bar; the scale (the fill, the ghost, the held
    // peak) runs `inset` in from its ends, over the cap's travel.
    let inset = inset.clamp(0.0, well.height / 2.0);
    let r = Rectangle::new(Point::new(well.x, well.y + inset), Size::new(well.width, well.height - 2.0 * inset));
    let l = level.clamp(0.0, 1.0);
    // Peak hold (manual 21.7): a 2px line at the held peak, in the colour
    // of the meter zone it sits in, drawn once the fill (below) is down.
    let held = (available && peak > l + 0.005).then_some(peak.clamp(0.0, 1.0));
    let fill_w = r.width;
    let radius = METER_RADIUS * scale;
    let clip_h = CLIP_H * scale;

    // Background/fill are measured against the *full* `r`, exactly like
    // `draw_ruler`'s ticks and the fader cap next to it — previously
    // this track was inset by the clip-LED's reserved strip, so a
    // signal's fill top landed a few px below its own ruler gridline at
    // every dB value, worst right at 0 dBFS (fixed as its own residual,
    // right after the linear-vs-tapered curve fix below fixed the
    // *shape* of that same mismatch). The clip LED is drawn last (see
    // below) and fully opaque, so it still reads as a distinct control
    // sitting on top of the meter, not blended into it — it just no
    // longer reserves its own dead strip that the fill could never
    // reach.
    // The well: an inset like the lane beside it, a lit lip under its
    // foot, the dark inside, and (drawn last) the lane's rim.
    frame.fill(
        &Path::new(|b| b.rounded_rectangle(well.position(), well.size(), radius.into())),
        METER_WELL,
    );
    let r_dash = well;

    if !available {
        // A handful of short dashes down the middle of the track, in a
        // dim neutral gray — reads as "N/A" without needing text layout
        // inside a canvas this narrow.
        let dash_w = fill_w * 0.5;
        let dash_h = 2.0 * scale;
        let dash_x = r_dash.x + (fill_w - dash_w) / 2.0;
        let count = ((r_dash.height / (dash_h * 2.5)).floor() as usize).max(1);
        for i in 0..count {
            let dash_y = r_dash.y + r_dash.height * (i as f32 + 0.5) / count as f32;
            frame.fill(
                &Path::new(|b| {
                    b.rounded_rectangle(
                        Point::new(dash_x, dash_y - dash_h / 2.0),
                        Size::new(dash_w, dash_h),
                        (dash_h / 2.0).into(),
                    )
                }),
                METER_NO_SIGNAL,
            );
        }
        return;
    }

    // The ladder at rest: every zone faintly there, like a row of unlit
    // LEDs, so the meter reads as an instrument even in silence.
    // The ladder runs the well's whole length, as the reference's does: the
    // green on down to the well's foot, the red up to under the clip LED.
    let well_foot = well.y + well.height;
    // The red ghost stops at 0 dBFS: nothing can light above it, so the
    // segments there stay dark (the reviewer: a quarter of the well was
    // dead dim red, the largest red on screen at rest).
    let red_top = r.y + r.height - r.height * db_to_t(0.0);
    for (i, (lower_db, (upper_db, color))) in [FLOOR_DB, METER_ZONES[0].0, METER_ZONES[1].0]
        .into_iter()
        .zip(METER_ZONES)
        .enumerate()
    {
        let bottom = if i == 0 { well_foot } else { r.y + r.height - r.height * db_to_t(lower_db) };
        let top = if i == METER_ZONES.len() - 1 { red_top } else { r.y + r.height - r.height * db_to_t(upper_db) };
        frame.fill_rectangle(
            Point::new(r.x, top),
            Size::new(fill_w, (bottom - top).max(0.0)),
            Color { a: METER_GHOST_ALPHA, ..color },
        );
    }
    // Above 0 dBFS, up to the clip LED: unlit segments in a neutral grey,
    // so the ladder runs unbroken (left empty it was a black gap, the
    // owner, 2026-11-03), without the red that says "this can light".
    let ladder_top = well.y + (CLIP_TOP + CLIP_H + 1.0) * scale;
    if red_top > ladder_top {
        frame.fill_rectangle(
            Point::new(r.x, ladder_top),
            Size::new(fill_w, red_top - ladder_top),
            Color { a: METER_UNLIT_ALPHA, ..METER_UNLIT },
        );
    }

    if l > 0.0 {
        // `l` is linear amplitude (1.0 = 0 dBFS), but the ruler's
        // ticks — and the fader track right next to this column — are
        // positioned on the *tapered* dB curve (`db_to_t`/`vol_to_t`),
        // not linearly. Filling by raw `l` used to land a -6 dBFS
        // signal's fill top well below the "-6" gridline instead of
        // right at it (checked: `vol_to_t(0.5)` ≈ 0.63, not 0.5) — real
        // signal data only started flowing through here this session
        // (previously every real backend read "N/A"), which is what
        // made this actually matter rather than being a latent bug.
        // Fixed color zones, console-style, instead of tinting the whole
        // bar by its peak: with an opaque fill, a hot signal used to turn
        // the entire column red, so "loud" and "clipping" looked alike.
        // Each zone is filled only as far as the level reaches into it.
        let mut fill_upto = |level: f32, alpha: f32| {
            let top = r.y + r.height - r.height * vol_to_t(level);
            let mut lower_db = FLOOR_DB;
            for (upper_db, color) in METER_ZONES {
                let zone_bottom = r.y + r.height - r.height * db_to_t(lower_db);
                let zone_top = (r.y + r.height - r.height * db_to_t(upper_db)).max(top);
                if zone_top < zone_bottom {
                    frame.fill(
                        &Path::rectangle(
                            Point::new(r.x, zone_top),
                            Size::new(fill_w, zone_bottom - zone_top),
                        ),
                        Color { a: alpha, ..color },
                    );
                }
                lower_db = upper_db;
            }
        };
        // With RMS: the peaks lighter, the loudness solid over them.
        match shown_rms(rms) {
            Some(loud) => {
                fill_upto(l, PEAK_OVER_RMS_ALPHA);
                if loud > 0.0 {
                    fill_upto(loud.min(l), FILL_ALPHA);
                }
            }
            None => fill_upto(l, FILL_ALPHA),
        }
    }

    if let Some(p) = held {
        let db = 20.0 * p.max(1e-6).log10();
        let color = METER_ZONES
            .iter()
            .find(|(upper, _)| db <= *upper)
            .map_or(theme::clip(), |(_, c)| *c);
        let y = r.y + r.height - r.height * vol_to_t(p);
        frame.fill(
            &Path::rectangle(Point::new(r.x, y - scale), Size::new(fill_w, 2.0 * scale)),
            color,
        );
    }

    // Clip LED: the well's top segment, in the band above the scale (as
    // the reference's meter carries its red at the very top), inset a
    // hair from the well's rim and with its corners.
    // Lit while the channel's clip is held (`clipping`), not while the
    // smoothed level shown here is high: a clip lasts a few milliseconds
    // and the smoothed level would miss it.
    let clip_rect = Rectangle::new(
        Point::new(well.x + 1.0 * scale, well.y + CLIP_TOP * scale),
        Size::new((fill_w - 2.0 * scale).max(0.0), clip_h),
    );
    let clip_color = if clipping {
        Color {
            a: FILL_ALPHA,
            ..theme::clip()
        }
    } else {
        CLIP_UNLIT
    };
    if clipping {
        let glow = Rectangle::new(
            Point::new(clip_rect.x - 2.0 * scale, clip_rect.y - 2.0 * scale),
            Size::new(clip_rect.width + 4.0 * scale, clip_h + 4.0 * scale),
        );
        frame.fill(
            &Path::new(|b| {
                b.rounded_rectangle(glow.position(), glow.size(), (radius + 2.0 * scale).into())
            }),
            Color {
                a: 0.35,
                ..theme::clip()
            },
        );
    }
    frame.fill(
        &Path::new(|b| {
            b.rounded_rectangle(clip_rect.position(), clip_rect.size(), (radius - 1.0 * scale).max(0.0).into())
        }),
        clip_color,
    );
    // The clip's shockwave: a ring in the clip red bursting out of the LED
    // and fading, a second, fainter one just behind it; a rounded outline
    // round the LED that grows sideways and down (the LED sits at the
    // canvas's top, where a circle was cut flat: the reviewer). Sharp and
    // red, an alarm's flash, never a sparkle.
    if let Some(p) = shock {
        for (lag, strength) in [(0.0f32, 0.85f32), (0.18, 0.45)] {
            let q = ((p - lag) / (1.0 - lag)).clamp(0.0, 1.0);
            if q <= 0.0 || q >= 1.0 {
                continue;
            }
            let g = 12.0 * scale * (1.0 - (1.0 - q).powi(3));
            let top = (clip_rect.y - g).max(0.5 * scale);
            // Sideways only as far as the canvas has room left of the LED,
            // kept centred on it (a strip without the ruler has little).
            let gx = g.min((clip_rect.x - 0.5 * scale).max(0.0));
            let ring = Rectangle::new(
                Point::new(clip_rect.x - gx, top),
                Size::new(clip_rect.width + 2.0 * gx, clip_rect.y + clip_h + g - top),
            );
            frame.stroke(
                &Path::new(|b| b.rounded_rectangle(ring.position(), ring.size(), (radius + g).into())),
                canvas::Stroke::default()
                    .with_width((2.0 - 1.2 * q) * scale)
                    .with_color(Color { a: strength * (1.0 - q), ..theme::clip() }),
            );
        }
    }
}

/// The slot the cap rides in (it was a 3.5px rail filled to the cap, which
/// read as a UI slider rather than a fader), down the middle of its lane.
const RAIL_W: f32 = 3.0;
/// The lane the cap rides in (`theme::lane`; the owner, 2026-11-03: "look
/// at the slider lane that the nubs are riding. It has padding and inset.
/// Its has some margins to move up and down"): an inset channel wider than
/// the cap by `LANE_PAD` on each side, with rounded ends, standing
/// `LANE_MARGIN` in from the fader's top and foot.
const LANE_PAD: f32 = 3.0;
const LANE_MARGIN: f32 = 1.0;
const LANE_W: f32 = CAP_W + 2.0 * LANE_PAD;
const LANE_RADIUS: f32 = 6.0;
/// The cap: slim and tall, a fader cap rather than a cube (the owner,
/// 2026-11-03: "I don't like the chunky square slider cubes", "Make them
/// taller", then, of ribbed ones, "No look at these caps", holding up a
/// hardware mixer's smooth two-part caps; it was 24 x 20).
const CAP_W: f32 = 20.0;
const CAP_H: f32 = 38.0;
const CAP_RADIUS: f32 = 4.0;

/// How far the scale (the ruler, the meter, the cap's travel) stands in
/// from the fader's top and foot: the lane's margin, its padding and half
/// a cap, so the cap's line reaches every mark, the ends included, while
/// the cap stays inside its lane with room round it. A collapsed strip's
/// meter stands in the same, so it lines up with its neighbours.
pub(crate) fn travel_inset(scale: f32) -> f32 {
    (LANE_MARGIN + LANE_PAD + CAP_H / 2.0) * scale
}

/// The part of `r` (a whole fader's height) the scale runs over.
fn travel(r: Rectangle, scale: f32) -> Rectangle {
    let inset = travel_inset(scale).min(r.height / 2.0);
    Rectangle::new(Point::new(r.x, r.y + inset), Size::new(r.width, r.height - 2.0 * inset))
}
/// Half-length of the unity/default reference tick — wider than the rail
/// so it reads as a mark crossing it, not a dot sitting on it.
const REF_HALF_W: f32 = 7.5;

/// Trim Gains' marks (manual 21.3): a small coral triangle beside the
/// rail at each of the channel's other routings, pointing at the rail,
/// on the same taper as the cap. Drawn under the track, so the cap covers
/// a mark that sits at its own level.
fn draw_marks(frame: &mut Frame, r: Rectangle, marks: &[f32], (lo, hi): (f32, f32), scale: f32) {
    let (t_lo, t_hi) = (vol_to_t(lo), vol_to_t(hi));
    if t_hi <= t_lo {
        return;
    }
    let cx = r.x + r.width / 2.0;
    let (half_h, w) = (3.5 * scale, 5.0 * scale);
    // Beside the slot, inside the lane, pointing at it.
    let tip_x = cx + RAIL_W * scale / 2.0 + 1.5 * scale;
    let run = travel(r, scale);
    for &v in marks.iter().filter(|&&v| v > 0.0) {
        let t = ((vol_to_t(v) - t_lo) / (t_hi - t_lo)).clamp(0.0, 1.0);
        let y = safe_clamp(run.y + run.height - run.height * t, r.y + half_h, r.y + r.height - half_h);
        frame.fill(
            &Path::new(|p| {
                p.move_to(Point::new(tip_x, y));
                p.line_to(Point::new(tip_x + w, y - half_h));
                p.line_to(Point::new(tip_x + w, y + half_h));
                p.close();
            }),
            theme::accent(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_track(
    frame: &mut Frame,
    r: Rectangle,
    value: f32,
    default_value: f32,
    range: (f32, f32),
    (dragging, hovered, slot_lit): (bool, bool, Option<Color>),
    scale: f32,
    // The lane drawn below as a picture (`FaderBase`): only its rim's
    // hover colour here.
    lane_below: bool,
) {
    let rail_w = RAIL_W * scale;
    let cap_w = CAP_W * scale;
    let cap_h = CAP_H * scale;
    let cap_radius = CAP_RADIUS * scale;
    let ref_half_w = REF_HALF_W * scale;

    let (lo, hi) = range;
    let (t_lo, t_hi) = (vol_to_t(lo), vol_to_t(hi));

    let run = travel(r, scale);
    let pos_of = |v: f32| -> f32 {
        let t = if t_hi > t_lo {
            ((vol_to_t(v) - t_lo) / (t_hi - t_lo)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        run.y + run.height - run.height * t
    };

    let cx = r.x + r.width / 2.0;
    let half_cap = cap_h / 2.0;
    let raw_y = pos_of(value);
    let cap_y = safe_clamp(raw_y, r.y + half_cap, r.y + r.height - half_cap);

    // The lane: an inset channel the cap rides in, wider than the cap with
    // padding round it and rounded ends, standing a margin in from the
    // fader's ends. Shade inside, darker at the top where the light can't
    // reach, a dark rim, and a lit lip along its lower edge, as a cut in
    // the panel catches the light. Under the cursor the rim turns
    // `fader`, as a key's does: the lane says this fader is the one the
    // hand is on.
    let lane_w = LANE_W * scale;
    let margin = LANE_MARGIN * scale;
    let (lane_x, lane_y, lane_h) = (cx - lane_w / 2.0, r.y + margin, (r.height - 2.0 * margin).max(0.0));
    let lane_r = (LANE_RADIUS * scale).min(lane_w / 2.0);
    let lane = Rectangle::new(Point::new(lane_x, lane_y), Size::new(lane_w, lane_h));
    if !lane_below {
        draw_inset(frame, lane, lane_r, scale, (dragging || hovered).then(theme::fader_color));
    } else if dragging || hovered {
        frame.stroke(
            &Path::new(|b| b.rounded_rectangle(lane.position(), lane.size(), lane_r.into())),
            Stroke::default().with_color(theme::fader_color()).with_width(1.0),
        );
    }

    // The slot: a thin cut down the lane's middle, short of its ends,
    // shade with a darker line down it and a rim in the control edge, so
    // it holds 3:1 on every ground, the lamp wash too. No fill up to the
    // cap: the cap shows the level, as on hardware.
    let pad = (LANE_PAD + 3.0) * scale;
    let (slot_y, slot_h) = (lane_y + pad, (lane_h - 2.0 * pad).max(0.0));
    let slot = Path::new(|b| {
        b.rounded_rectangle(
            Point::new(cx - rail_w / 2.0, slot_y),
            Size::new(rail_w, slot_h),
            (rail_w / 2.0).into(),
        )
    });
    // Trim Gains on: the slot glows coral (a live state, so it may).
    frame.fill(&slot, slot_lit.unwrap_or_else(theme::slot));
    frame.fill_rectangle(
        Point::new(cx - 0.5 * scale, slot_y + rail_w / 2.0),
        Size::new(1.0 * scale, (slot_h - rail_w).max(0.0)),
        Color { a: 0.55, ..Color::BLACK },
    );
    frame.stroke(
        &slot,
        Stroke::default()
            .with_color(slot_lit.unwrap_or(Color { a: 0.7, ..theme::control_edge() }))
            .with_width(1.0),
    );

    // Unity/default reference — a tick crossing the rail, like the 0 dB
    // mark on a real console strip (a dot read as a blob at this scale;
    // a line reads immediately as "mark on a ruler").
    let ref_y = safe_clamp(pos_of(default_value), r.y, r.y + r.height);
    let ref_tick = Path::line(
        Point::new(cx - ref_half_w, ref_y),
        Point::new(cx + ref_half_w, ref_y),
    );
    frame.stroke(
        &ref_tick,
        Stroke::default()
            .with_color(theme::text_sec())
            .with_width(1.5),
    );

    // The cap: smooth glossy metal in two halves (2026-11-03, the owner,
    // of a hardware mixer's faders: "No look at these caps"). Each half is
    // a rounded block lit from above (a bright lip along its top, the
    // metal, shade at its foot); between them runs a dark groove with the
    // coral line in it, marking the exact position. The sides roll away,
    // a dark hairline rims it, and it stands off the lane on a soft
    // shadow. Under the cursor it comes forward (lighter in a dark theme,
    // darker in a light one), further while held. The metal is
    // `fader_color`, so a light theme's caps are dark metal.
    let cap_left = cx - cap_w / 2.0;
    let cap_top = cap_y - half_cap;
    let rounded = |x: f32, y: f32, w: f32, h: f32, rad: f32| {
        Path::new(move |b| b.rounded_rectangle(Point::new(x, y), Size::new(w, h), rad.into()))
    };

    frame.fill(
        &rounded(cap_left - 1.0 * scale, cap_top + 2.5 * scale, cap_w + 2.0 * scale, cap_h + 0.5 * scale, cap_radius + 1.0 * scale),
        Color { a: 0.18, ..Color::BLACK },
    );
    frame.fill(
        &rounded(cap_left - 0.5 * scale, cap_top + 1.0 * scale, cap_w + scale, cap_h, cap_radius + 0.5 * scale),
        Color { a: 0.3, ..Color::BLACK },
    );
    let metal = if dragging {
        theme::blend(theme::fader_metal(), theme::hover_toward(), 0.25)
    } else if hovered {
        theme::hovered(theme::fader_metal())
    } else {
        theme::fader_metal()
    };
    let shade = |k: f32| theme::blend(metal, Color::BLACK, k);
    let light = |k: f32| theme::blend(metal, Color::WHITE, k);
    let body = rounded(cap_left, cap_top, cap_w, cap_h, cap_radius);
    // The groove's dark shows between the halves.
    frame.fill(&body, shade(0.5));
    let groove = 3.0 * scale;
    let half_h = (cap_h - groove) / 2.0;
    for (top, lip, foot) in [(cap_top, 0.62, 0.16), (cap_y + groove / 2.0, 0.34, 0.3)] {
        let half = rounded(cap_left, top, cap_w, half_h, cap_radius);
        frame.fill(
            &half,
            canvas::gradient::Linear::new(Point::new(cx, top), Point::new(cx, top + half_h))
                .add_stop(0.0, light(lip))
                .add_stop(0.12, light(lip * 0.35))
                .add_stop(0.55, metal)
                .add_stop(1.0, shade(foot)),
        );
        // A glint along its top edge.
        frame.fill_rectangle(
            Point::new(cap_left + cap_radius, top + 1.0 * scale),
            Size::new(cap_w - 2.0 * cap_radius, 1.0),
            Color { a: 0.55, ..Color::WHITE },
        );
    }
    // The sides roll away from the light.
    frame.fill(
        &body,
        canvas::gradient::Linear::new(Point::new(cap_left, cap_y), Point::new(cap_left + cap_w, cap_y))
            .add_stop(0.0, Color { a: 0.22, ..Color::BLACK })
            .add_stop(0.2, Color { a: 0.0, ..Color::BLACK })
            .add_stop(0.8, Color { a: 0.0, ..Color::BLACK })
            .add_stop(1.0, Color { a: 0.26, ..Color::BLACK }),
    );
    frame.fill_rectangle(
        Point::new(cap_left + 1.5 * scale, cap_y - 1.0 * scale),
        Size::new(cap_w - 3.0 * scale, 2.0 * scale),
        theme::accent(),
    );

    frame.stroke(
        &body,
        Stroke::default().with_color(Color { a: 0.45, ..Color::BLACK }).with_width(1.0),
    );
}

/// Where the cap sits on the track `r` at `value`, as `draw_track` draws
/// it (clamped so it never leaves the track).
fn cap_rect(r: Rectangle, value: f32, (lo, hi): (f32, f32), scale: f32) -> Rectangle {
    let (cap_w, cap_h) = (CAP_W * scale, CAP_H * scale);
    let (t_lo, t_hi) = (vol_to_t(lo), vol_to_t(hi));
    let t = if t_hi > t_lo {
        ((vol_to_t(value) - t_lo) / (t_hi - t_lo)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let run = travel(r, scale);
    let y = safe_clamp(run.y + run.height - run.height * t, r.y + cap_h / 2.0, r.y + r.height - cap_h / 2.0);
    Rectangle::new(
        Point::new(r.x + r.width / 2.0 - cap_w / 2.0, y - cap_h / 2.0),
        Size::new(cap_w, cap_h),
    )
}

/// The ring a cap sends out when it lands on its home (`feel::splash`,
/// `p` 0..1): its outline, growing away from it in the accent, fading.
fn draw_cap_splash(frame: &mut Frame, cap: Rectangle, p: f32, scale: f32) {
    let grow = 9.0 * scale * crate::motion::ease_out(p);
    let ring = Path::new(|b| {
        b.rounded_rectangle(
            Point::new(cap.x - grow, cap.y - grow),
            Size::new(cap.width + 2.0 * grow, cap.height + 2.0 * grow),
            (CAP_RADIUS * scale + grow).into(),
        )
    });
    frame.stroke(
        &ring,
        Stroke::default()
            .with_color(Color { a: 0.8 * (1.0 - p), ..theme::accent() })
            .with_width((2.4 - 1.6 * p) * scale),
    );
}

/// Drawn on top of the (translucent, see `FILL_ALPHA`) meter wash, so it
/// needs to stay legible against whatever color is lit behind it —
/// numbers centered in the same rect `draw_meter` just filled, TotalMix-
/// style, rather than pushed into their own lane to one side of it. The
/// tick dash sits at the rect's own right edge — right where the fader
/// track begins right after it (see `Fader::layout_x`), the same
/// "shared between the meter and the fader" position the reference uses.
fn draw_ruler(frame: &mut Frame, r: Rectangle, scale: f32, labels: bool) {
    // -6 is left out: at the 12px floor it runs into -10, and the scale
    // reads cleanly from 0, -10, -20, -40, -60.
    const TICKS: [f32; 5] = [0.0, -10.0, -20.0, -40.0, -60.0];
    // Measured against the *full* height, matching `draw_track`'s own
    // `pos_of` (the fader cap) and `draw_meter`'s fill: all three agree on
    // where a given dB value sits.
    let y_of = |db: f32| {
        let y = r.y + r.height - r.height * db_to_t(db);
        safe_clamp(y, r.y + 4.0 * scale, r.y + r.height - 4.0 * scale)
    };
    // On a short fader (two rows sharing a window) the numbers would touch:
    // 0 and ∞ always stand, a middle one only with a line's room round it.
    let room = theme::TEXT_SM * 1.15 * scale;
    let floor_y = y_of(-60.0);
    let mut last_y = f32::NEG_INFINITY;
    for db in TICKS {
        let y = y_of(db);
        // 0 dB is the reference the cap rests on: one step brighter.
        let color = if db == 0.0 { theme::text_sec() } else { theme::muted() };
        let middle = db != 0.0 && db > FLOOR_DB;
        if labels && middle && (y - last_y < room || floor_y - y < room) {
            continue;
        }
        if labels {
            last_y = y;
            // True minus signs, and the floor named for what it is: the
            // fader's bottom is silence, not -60.
            // Engraved as on a console's fader: the attenuation, no signs.
            let label = if db == 0.0 {
                "0".to_string()
            } else if db <= FLOOR_DB {
                "\u{221e}".to_string()
            } else {
                format!("{}", -db as i32)
            };
            frame.fill_text(canvas::Text {
                content: label,
                position: Point::new(r.x + r.width - 2.0 * scale, y),
                color,
                font: theme::MONO,
                size: (theme::TEXT_SM * scale).into(),
                align_x: advanced_text::Alignment::Right,
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        } else {
            // A short tick at the lane's track-side edge: the same marks
            // as the labelled scale, so a length reads the same anywhere.
            let w = 4.0 * scale;
            frame.fill_rectangle(
                Point::new(r.x + r.width - w, y - 0.5),
                Size::new(w, 1.0),
                Color { a: 0.55, ..color },
            );
        }
    }
}

pub fn fader<'a, Message: Clone + 'a>(fader: Fader<Message>) -> Element<'a, Message> {
    let height = fader.height;
    // With a meter, this is the main strip fader: `Fill` so the canvas
    // spans the strip's actual content width, and `Fader::track_x` centers
    // the track within whatever width that turns out to be. Without one,
    // it's a Matrix-view cell — small and `Fixed`, as before; `Fill` there
    // would blow up inside the Matrix's horizontally-scrollable row, which
    // has no bound on how wide a `Fill` child could claim.
    let width = if fader.show_meter {
        Length::Fill
    } else {
        Length::Fixed(fader.compact_width * fader.scale)
    };
    let base = fader.show_meter.then_some(FaderBase {
        scale: fader.scale,
        ruler_labels: fader.ruler_labels,
        reserved_right: fader.reserved_right,
        track_from_right: fader.track_from_right,
    });
    let top = Canvas::new(fader).width(width).height(Length::Fixed(height));
    match base {
        // The lane's and the meter's cuts in a layer of their own under
        // the fader, as pictures with exact edge smoothing: their rounded
        // corners stepped on the canvas, and a picture there would cover
        // the cap and the bars (pictures draw over a canvas's shapes).
        Some(base) => iced::widget::stack![Canvas::new(base).width(width).height(Length::Fixed(height)), top].into(),
        None => top.into(),
    }
}

/// The strip fader's lane and meter frame, drawn under it (`fader`).
struct FaderBase {
    scale: f32,
    ruler_labels: bool,
    reserved_right: f32,
    track_from_right: Option<f32>,
}

impl<Message> canvas::Program<Message> for FaderBase {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let s = self.scale;
        let (track_x, meter_x) = layout_x(bounds.width, self.reserved_right, self.track_from_right, true, self.ruler_labels, s);
        let mut frame = Frame::new(renderer, bounds.size());
        let key = |what: u8, r: Rectangle| {
            hash_of(|h| {
                (what, r.width.to_bits(), r.height.to_bits(), s.to_bits(), theme::is_dark()).hash(h);
                theme::palette_key().hash(h);
            })
        };
        let pad = 2.0 * s;
        let grow = |r: Rectangle| Rectangle::new(Point::new(r.x - pad, r.y - pad), Size::new(r.width + 2.0 * pad, r.height + 2.0 * pad));
        let (meter_rect, _) = meter_column(meter_x, bounds.height, s, self.ruler_labels);
        super::knob_paint::picture(&mut frame, grow(meter_rect), key(1, meter_rect), |pen, local| {
            let inner = Rectangle::new(Point::new(local.x + pad, local.y + pad), meter_rect.size());
            draw_inset_pen(pen, inner, 4.0 * s, s, None);
        });
        // The lane, as `draw_track` places it.
        let r = Rectangle::new(Point::new(track_x, 0.0), Size::new(TRACK_W * s, bounds.height));
        let lane_w = LANE_W * s;
        let margin = LANE_MARGIN * s;
        let cx = r.x + r.width / 2.0;
        let lane = Rectangle::new(Point::new(cx - lane_w / 2.0, r.y + margin), Size::new(lane_w, (r.height - 2.0 * margin).max(0.0)));
        let lane_r = (LANE_RADIUS * s).min(lane_w / 2.0);
        super::knob_paint::picture(&mut frame, grow(lane), key(2, lane), |pen, local| {
            let inner = Rectangle::new(Point::new(local.x + pad, local.y + pad), lane.size());
            draw_inset_pen(pen, inner, lane_r, s, None);
        });
        vec![frame.into_geometry()]
    }
}

/// The meter+ruler column on its own, with no track/cap and no mouse
/// interaction at all — for a collapsed strip, which trades every control
/// (fader, mute/solo, pan) for a glance-only level readout.
struct VuMeter<Message> {
    level: MeterFrame,
    scale: f32,
    available: bool,
    clip: bool,
    /// The right channel of a linked pair: its level, whether it is a
    /// real reading, and its clip LED. Splits the bar in two, as on a
    /// full strip.
    right: Option<(MeterFrame, bool, bool)>,
    on_clip_reset: Option<Message>,
}

impl<Message: Clone> canvas::Program<Message> for VuMeter<Message> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        // See `Fader`'s identical arm.
        match event {
            // A click clears a lit clip LED (see `Fader::on_clip_reset`).
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                self.on_clip_reset
                    .clone()
                    .map(|m| canvas::Action::publish(m).and_capture())
            }
            canvas::Event::Window(window::Event::RedrawRequested(now))
                if (INTERPOLATE_METERS
                    && (self.level.is_settling(*now)
                        || self.right.is_some_and(|(m, _, _)| m.is_settling(*now))))
                    || shock(self.level.clip_at, *now).is_some()
                    || self.right.is_some_and(|(m, _, _)| shock(m.clip_at, *now).is_some()) =>
            {
                Some(canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        // The bar is centered in the card, under the name and the M/S
        // buttons, which are; the numbers take the margin to its left. The
        // [numbers][bar] block used to be centered instead, which left the
        // bar itself off to the right.
        let bar_w = METER_W * self.scale;
        let bar_x = ((bounds.width - bar_w) / 2.0).max(0.0);
        // Stood in as a fader's scale is, so it lines up with the faders
        // beside it.
        let margin = (LANE_MARGIN * self.scale).min(bounds.height / 2.0);
        let pad = METER_FRAME_PAD * self.scale;
        let meter_rect = Rectangle::new(
            Point::new((bar_x - pad).max(0.0), margin),
            Size::new(bar_w + 2.0 * pad, bounds.height - 2.0 * margin),
        );
        draw_meter(
            &mut frame,
            meter_rect,
            meter_scale_inset(self.scale),
            self.level,
            self.scale,
            (self.available, self.clip),
            // A linked pair shows both channels, the bar split in two as
            // on a full strip: the collapsed card used to show the left
            // one only, so a loud or clipping right channel went unseen.
            self.right,
            true,
        );
        draw_ladders(&mut frame, meter_rect, self.scale, self.available, self.right.map(|(_, a, _)| a));
        // Its scale inside it, as on a full strip's meter.
        draw_meter_ticks(&mut frame, meter_rect, meter_scale_inset(self.scale), self.scale);
        vec![frame.into_geometry()]
    }
}

pub fn vu_meter<'a, Message: Clone + 'a>(
    level: MeterFrame,
    height: f32,
    scale: f32,
    available: bool,
    clip: bool,
    right: Option<(MeterFrame, bool, bool)>,
    on_clip_reset: Option<Message>,
) -> Element<'a, Message> {
    Canvas::new(VuMeter {
        level,
        scale,
        available,
        clip,
        right,
        on_clip_reset,
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

/// A monitor level's meter in the counter (Main, Phones): the pair's two
/// channels as thin bars under its slider, filling left to right on the
/// slider's own dB taper (`vol_to_t`), so a level reads against the slider
/// above it. Zones, well and peak line as on a strip's meter.
pub struct LevelMeter {
    pub left: MeterFrame,
    pub right: Option<MeterFrame>,
    pub scale: f32,
}

/// One bar's height, the gap between the two and the well's padding
/// round them, at scale 1.
const LEVEL_BAR_H: f32 = 4.0;
const LEVEL_BAR_GAP: f32 = 1.0;
const LEVEL_WELL_PAD: f32 = 2.0;

impl LevelMeter {
    /// The meter's height for `bars` bars, its well included.
    pub fn height(bars: usize, scale: f32) -> f32 {
        // The bars, the well's padding round them, and the lit lip under it.
        (bars as f32 * LEVEL_BAR_H + bars.saturating_sub(1) as f32 * LEVEL_BAR_GAP + 2.0 * LEVEL_WELL_PAD + 1.0) * scale
    }
}

impl<Message> canvas::Program<Message> for LevelMeter {
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
        let now = Instant::now();
        let bars: Vec<MeterFrame> = std::iter::once(self.left).chain(self.right).collect();
        let s = self.scale;
        let (bar_h, gap, pad) = (LEVEL_BAR_H * s, LEVEL_BAR_GAP * s, LEVEL_WELL_PAD * s);
        // Under the volume slider, on its travel (`level_fader`): a level
        // here sits under the same level on the slider. The well runs as
        // far as the slider's lane, so the two line up end to end (the
        // owner, 2026-11-10: it stood 15px in from the lane each side); its
        // ends past the travel show the ladder at rest only.
        let inset = crate::widgets::level_fader::travel_inset(s).min(bounds.width / 2.0);
        let (x0, w) = (inset, (bounds.width - 2.0 * inset).max(0.0));
        let lane = crate::widgets::level_fader::lane_inset(s).min(inset);
        // The cut (its lip under it, in the last pixel), and the bars'
        // dark inside it, `pad` in.
        let outer = Rectangle::new(Point::new(lane, 0.0), Size::new((bounds.width - 2.0 * lane).max(0.0), bounds.height - 1.0 * s));
        let well = Rectangle::new(Point::new(outer.x + pad, pad), Size::new((outer.width - 2.0 * pad).max(0.0), outer.height - 2.0 * pad));
        let (bar_l, bar_r) = (well.x + 1.0 * s, well.x + well.width - 1.0 * s);
        // Its well cut into the section, the bars' dark inside it.
        draw_inset(&mut frame, outer, 4.0 * s, s, None);
        frame.fill(
            &Path::new(|b| b.rounded_rectangle(well.position(), well.size(), (2.0 * s).into())),
            METER_WELL,
        );
        for (i, meter) in bars.iter().enumerate() {
            let y = pad + i as f32 * (bar_h + gap);
            // The ladder at rest: every zone faintly there.
            // Above 0 dBFS the unlit segments are grey, as on the strips.
            let zero = x0 + w * db_to_t(0.0);
            frame.fill_rectangle(
                Point::new(zero, y),
                Size::new((bar_r - zero).max(0.0), bar_h),
                Color { a: METER_UNLIT_ALPHA, ..METER_UNLIT },
            );
            let mut lower = FLOOR_DB;
            for (upper, color) in METER_ZONES {
                // The lowest zone from the well's own start.
                let left = if lower == FLOOR_DB { bar_l } else { x0 + w * db_to_t(lower) };
                let right = x0 + w * db_to_t(upper.min(0.0));
                frame.fill_rectangle(Point::new(left, y), Size::new(right - left, bar_h), Color { a: METER_GHOST_ALPHA, ..color });
                lower = upper;
            }
            let level = meter.shown(now).clamp(0.0, 2.0);
            let mut fill_upto = |level: f32, alpha: f32| {
                let reach = x0 + w * vol_to_t(level);
                let mut lower_db = FLOOR_DB;
                for (upper_db, color) in METER_ZONES {
                    let left = x0 + w * db_to_t(lower_db);
                    let right = (x0 + w * db_to_t(upper_db)).min(reach);
                    if right > left {
                        frame.fill(
                            &Path::rectangle(Point::new(left, y), Size::new(right - left, bar_h)),
                            Color { a: alpha, ..color },
                        );
                    }
                    lower_db = upper_db;
                }
            };
            if level > 0.0 {
                match shown_rms(meter.rms) {
                    Some(loud) => {
                        fill_upto(level, PEAK_OVER_RMS_ALPHA);
                        if loud > 0.0 {
                            fill_upto(loud.min(level), FILL_ALPHA);
                        }
                    }
                    None => fill_upto(level, FILL_ALPHA),
                }
            }
            if meter.peak > level + 0.005 {
                let db = 20.0 * meter.peak.max(1e-6).log10();
                let color = METER_ZONES
                    .iter()
                    .find(|(upper, _)| db <= *upper)
                    .map_or(theme::clip(), |(_, c)| *c);
                let x = (x0 + w * vol_to_t(meter.peak) - s).max(0.0);
                frame.fill(&Path::rectangle(Point::new(x, y), Size::new(2.0 * s, bar_h)), color);
            }
        }
        // The LED segments: a line of the well every 3px across the bars.
        let pitch = (SEGMENT_PITCH * s).max(2.0);
        let ladder = Path::new(|b| {
            let mut x = well.x + pitch;
            while x < well.x + well.width - 1.0 {
                b.rectangle(Point::new(x.round(), well.y), Size::new(1.0, well.height));
                x += pitch;
            }
        });
        frame.fill(&ladder, METER_WELL);
        vec![frame.into_geometry()]
    }
}

/// `LevelMeter` as an element as wide as its slider.
pub fn level_meter<'a, Message: 'a>(left: MeterFrame, right: Option<MeterFrame>, scale: f32) -> Element<'a, Message> {
    let bars = 1 + usize::from(right.is_some());
    Canvas::new(LevelMeter { left, right, scale })
        .width(Length::Fill)
        .height(Length::Fixed(LevelMeter::height(bars, scale)))
        .into()
}

/// A monitor output's meter standing upright, for the folded right-hand
/// column (`sidebar`): the pair's two channels side by side, the same
/// zones, RMS and peak line as a strip's meter.
pub struct RailMeter {
    pub left: MeterFrame,
    pub right: Option<MeterFrame>,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for RailMeter {
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
        let r = Rectangle::new(Point::ORIGIN, bounds.size());
        draw_meter(
            &mut frame,
            r,
            0.0,
            self.left,
            self.scale,
            (true, false),
            self.right.map(|m| (m, true, false)),
            true,
        );
        draw_ladders(&mut frame, r, self.scale, true, self.right.map(|_| true));
        vec![frame.into_geometry()]
    }
}

/// `RailMeter` as an element, `width` by `height` (scaled by the caller).
pub fn rail_meter<'a, Message: 'a>(
    left: MeterFrame,
    right: Option<MeterFrame>,
    scale: f32,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(RailMeter { left, right, scale })
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

/// The Quick setup's level meter (`quick`): an input's level large, on a
/// straight dB scale from −60 to 0 dBFS so the target reads as a place,
/// with the band a voice should peak in marked over it ("speak here") and
/// the scale under it. The bar is dim below the band, lit in it, amber
/// above it and red at the top, so its colour says what the step says.
pub struct TargetMeter {
    pub left: MeterFrame,
    pub right: Option<MeterFrame>,
    /// The band, in dBFS (low, high).
    pub band: (f32, f32),
    pub scale: f32,
    /// How lit the band is (0 to 1): up while a voice sits in it.
    pub glow: f32,
}

const TM_BAR_H: f32 = 16.0;
const TM_BAR_GAP: f32 = 2.0;
const TM_PAD: f32 = 3.0;
const TM_LABEL_H: f32 = 18.0;
const TM_SCALE_H: f32 = 18.0;
const TM_FLOOR_DB: f32 = -60.0;
/// From here up a peak is too loud (red), on every meter and in Quick's
/// words: -3 dB, as the strips' meters have it (OpenCode's job 60: Quick's
/// meter went red at -2, the strips' at -3, the same level two colours).
pub const TM_HOT_DB: f32 = -3.0;

impl TargetMeter {
    pub fn height(bars: usize, scale: f32) -> f32 {
        (TM_LABEL_H + bars as f32 * TM_BAR_H + bars.saturating_sub(1) as f32 * TM_BAR_GAP + 2.0 * TM_PAD + 1.0 + TM_SCALE_H) * scale
    }
}

impl<Message> canvas::Program<Message> for TargetMeter {
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
        let now = Instant::now();
        let s = self.scale;
        let bars: Vec<MeterFrame> = std::iter::once(self.left).chain(self.right).collect();
        let n = bars.len() as f32;
        let (x0, w) = (TM_PAD * s + 1.0, (bounds.width - 2.0 * TM_PAD * s - 2.0).max(0.0));
        let x_of = |db: f32| x0 + w * ((db.clamp(TM_FLOOR_DB, 0.0) - TM_FLOOR_DB) / -TM_FLOOR_DB);
        let db_of = |v: f32| 20.0 * v.max(1e-6).log10();
        let top = TM_LABEL_H * s;
        let well_h = n * TM_BAR_H * s + (n - 1.0) * TM_BAR_GAP * s + 2.0 * TM_PAD * s;
        let (bx0, bx1) = (x_of(self.band.0), x_of(self.band.1));
        // The band: a faint field behind the well and a bracket over it,
        // its words centred on top.
        let signal = theme::signal();
        let glow = self.glow.clamp(0.0, 1.0);
        // Round the well, on the floor: on a light one the hue at its
        // readable depth and a little stronger, so a lit band isn't a pastel
        // tint there.
        let (floor_hue, floor_k) = if theme::dark_theme() { (signal, 1.0) } else { (theme::readable_on_floors(signal), 2.0) };
        // A voice in the band lights it: a soft halo round the field, and
        // the field and its bracket brighter (and, over the LEDs, below).
        if glow > 0.0 {
            for k in 1..=3 {
                let e = k as f32 * 2.0 * s;
                frame.fill(
                    &Path::new(|b| {
                        b.rounded_rectangle(
                            Point::new(bx0 - e, top - 4.0 * s - e),
                            Size::new(bx1 - bx0 + 2.0 * e, well_h + 8.0 * s + 2.0 * e),
                            (e + 1.0).into(),
                        )
                    }),
                    Color { a: 0.09 * floor_k * glow, ..floor_hue },
                );
            }
        }
        frame.fill_rectangle(
            Point::new(bx0, top - 4.0 * s),
            Size::new(bx1 - bx0, well_h + 8.0 * s),
            Color { a: (0.18 + 0.14 * glow) * if glow > 0.0 { floor_k } else { 1.0 }, ..if glow > 0.0 { floor_hue } else { signal } },
        );
        let bracket = Path::new(|b| {
            b.move_to(Point::new(bx0, top + 2.0 * s));
            b.line_to(Point::new(bx0, top - 4.0 * s));
            b.line_to(Point::new(bx1, top - 4.0 * s));
            b.line_to(Point::new(bx1, top + 2.0 * s));
        });
        frame.stroke(
            &bracket,
            canvas::Stroke::default()
                // In the hue as it reads on the floor, at rest too (critique
                // #58 B: the raw green at 80% read 2.98 on Daylight).
                .with_color(Color { a: (0.9 + 0.1 * glow).min(1.0), ..floor_hue })
                .with_width((1.5 + 0.5 * glow) * s),
        );
        frame.fill_text(canvas::Text {
            content: "speak here".to_string(),
            position: Point::new((bx0 + bx1) / 2.0, top - 7.0 * s),
            color: theme::readable_on_floors(signal),
            font: theme::SANS_SEMI,
            size: (theme::TEXT_SM * s).into(),
            align_x: advanced_text::Alignment::Center,
            align_y: iced::alignment::Vertical::Bottom,
            ..canvas::Text::default()
        });
        // The well, as the counter's meters have it.
        let outer = Rectangle::new(Point::new(x0 - TM_PAD * s, top), Size::new(w + 2.0 * TM_PAD * s, well_h + 1.0 * s));
        draw_inset(&mut frame, outer, 4.0 * s, s, None);
        let well = Rectangle::new(Point::new(x0 - 1.0 * s, top + TM_PAD * s), Size::new(w + 2.0 * s, well_h - 2.0 * TM_PAD * s));
        frame.fill(&Path::new(|b| b.rounded_rectangle(well.position(), well.size(), (2.0 * s).into())), METER_WELL);
        // Each stretch of the scale in the colour it means here.
        let zones = [
            // Under the band: lit clearly enough to read a quiet voice (the
            // owner, 2026-11-13: at 30% "its hard to see the green lines"),
            // still short of the band's full green.
            // The LEDs in the strip meters' own fixed hues: the well is
            // near-black in every theme, and a light theme's deep green
            // read 3.4:1 on it (critique #47).
            (TM_FLOOR_DB, self.band.0, Color { a: 0.65, ..theme::METER_SIGNAL }),
            (self.band.0, self.band.1, theme::METER_SIGNAL),
            (self.band.1, TM_HOT_DB, theme::METER_ATTENTION),
            (TM_HOT_DB, 0.0, theme::METER_CLIP),
        ];
        for (i, meter) in bars.iter().enumerate() {
            let y = top + TM_PAD * s + i as f32 * (TM_BAR_H + TM_BAR_GAP) * s;
            let bar_h = TM_BAR_H * s;
            for (lo, hi, color) in zones {
                frame.fill_rectangle(Point::new(x_of(lo), y), Size::new(x_of(hi) - x_of(lo), bar_h), Color { a: METER_GHOST_ALPHA, ..color });
            }
            let level = meter.shown(now).clamp(0.0, 2.0);
            if level > 0.0 {
                let reach = x_of(db_of(level));
                for (lo, hi, color) in zones {
                    let (l, r) = (x_of(lo), x_of(hi).min(reach));
                    if r > l {
                        frame.fill_rectangle(Point::new(l, y), Size::new(r - l, bar_h), Color { a: FILL_ALPHA * color.a, ..color });
                    }
                }
            }
            // The band lit inside the well too, over its LEDs, where the
            // eye is (the dark well takes the plain signal in every theme),
            // under the peak marker, which stays white: it's what this
            // step asks to be read.
            if glow > 0.0 {
                frame.fill_rectangle(Point::new(bx0, y), Size::new(bx1 - bx0, bar_h), Color { a: 0.3 * glow, ..theme::METER_SIGNAL });
            }
            if meter.peak > 0.0 {
                let x = (x_of(db_of(meter.peak)) - 1.5 * s).max(x0);
                // White, not the theme's ink: the well is dark in every
                // theme, and Daylight's ink drew a dark notch on it (the
                // reviewer, 2026-11-05).
                frame.fill_rectangle(Point::new(x, y - 1.0 * s), Size::new(3.0 * s, bar_h + 2.0 * s), Color { a: 0.9, ..Color::WHITE });
            }
        }
        // The LED segments.
        let pitch = (SEGMENT_PITCH * s).max(2.0);
        let ladder = Path::new(|b| {
            let mut x = well.x + pitch;
            while x < well.x + well.width - 1.0 {
                b.rectangle(Point::new(x.round(), well.y), Size::new(1.0, well.height));
                x += pitch;
            }
        });
        frame.fill(&ladder, METER_WELL);
        // The scale under it.
        let scale_y = top + well_h + 4.0 * s;
        for db in [-48.0_f32, -36.0, -24.0, -18.0, -12.0, -6.0, 0.0] {
            let x = x_of(db);
            frame.fill_rectangle(Point::new(x - 0.5, scale_y), Size::new(1.0, 3.0 * s), theme::text_sec());
            frame.fill_text(canvas::Text {
                content: if db == 0.0 { "0".to_string() } else { format!("\u{2212}{}", -db as i32) },
                position: Point::new(x, scale_y + 4.0 * s),
                color: theme::text_sec(),
                font: theme::MONO,
                size: (theme::TEXT_SM * s).into(),
                align_x: if db == 0.0 { advanced_text::Alignment::Right } else { advanced_text::Alignment::Center },
                align_y: iced::alignment::Vertical::Top,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

/// `TargetMeter` as an element, as wide as its room.
pub fn target_meter<'a, Message: 'a>(left: MeterFrame, right: Option<MeterFrame>, band: (f32, f32), scale: f32, glow: f32) -> Element<'a, Message> {
    let bars = 1 + usize::from(right.is_some());
    Canvas::new(TargetMeter { left, right, band, scale, glow })
        .width(Length::Fill)
        .height(Length::Fixed(TargetMeter::height(bars, scale)))
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_speak_bracket_marks_minus_18_to_minus_6_on_the_meters_own_scale() {
        // Critique #57: drawn over the whole well, not the scale the fill
        // and ticks use, it marked about -20 to -2 dB.
        use iced::{Point, Rectangle, Size};
        for (h, scale) in [(180.0, 1.0), (320.0, 1.0), (260.0, 1.5)] {
            let meter = Rectangle::new(Point::new(10.0, 40.0), Size::new(14.0, h));
            let r = super::meter_scale_rect(meter, super::meter_scale_inset(scale), scale);
            let at = |db: f32| r.y + r.height - r.height * super::db_to_t(db);
            let (top, foot) = super::speak_band_ys(meter, scale);
            assert!((top - at(-6.0)).abs() < 0.01 && (foot - at(-18.0)).abs() < 0.01, "h {h}: {top}..{foot}");
            assert!(top > at(0.0) + 4.0 * scale, "h {h}: the bracket stays clear of the red zone");
            assert!(r.y > meter.y, "the scale stands in from the well's top");
        }
    }

    #[test]
    fn rms_shows_by_its_mode() {
        crate::widgets::set_rms_mode(0);
        assert_eq!(super::shown_rms(0.5), None);
        crate::widgets::set_rms_mode(1);
        assert_eq!(super::shown_rms(0.5), Some(0.5));
        assert_eq!(super::shown_rms(-1.0), None, "not measured");
        crate::widgets::set_rms_mode(2);
        let up = super::shown_rms(0.5).unwrap();
        assert!((20.0 * (up / 0.5).log10() - 3.0).abs() < 0.01, "+3 dB");
        crate::widgets::set_rms_mode(1);
    }

    use super::*;

    #[test]
    fn meter_fill_curve_is_tapered_not_linear() {
        // Guards `draw_meter`'s fill-height fix: a -6 dBFS signal
        // (linear amplitude 0.5) must land its fill top at the
        // ruler's own tapered "-6" position, not halfway up the
        // column — the bug this fixed used raw linear amplitude
        // directly as the fill fraction, which would make this
        // assertion fail (0.5 vs 0.5, no taper).
        let t = vol_to_t(0.5);
        assert!(
            (t - 0.5).abs() > 0.1,
            "expected the tapered curve to differ meaningfully from linear 0.5, got {t}"
        );
        // And it should match `db_to_t` fed the actual dB value,
        // since that's what the ruler's own ticks use — the whole
        // point is these two agree.
        let expected = db_to_t(20.0 * 0.5f32.log10());
        assert!((t - expected).abs() < 1e-4);
    }

    #[test]
    fn still_frame_is_never_settling() {
        // prev == value: nothing to interpolate, so no reason to keep
        // requesting redraws — this is what silence/idle looks like.
        let f = MeterFrame::still(0.3);
        assert!(!f.is_settling(f.since));
        assert!(!f.is_settling(f.since + Duration::from_millis(10)));
    }

    #[test]
    fn the_detent_never_holds_more_than_half_a_db() {
        let db = |t: f32| 20.0 * t_to_vol(t).log10();
        for run_h in [120.0, 170.0, 290.0] {
            let w = snap_width(1.0, run_h, 1.0);
            let home = vol_to_t(1.0);
            assert!(db(home + w) <= 0.51 && db(home - w) >= -0.51, "{run_h}px: {:.2} to {:.2} dB", db(home - w), db(home + w));
        }
        // On a long fader it stays the hand's 5px.
        assert!((snap_width(1.0, 2000.0, 1.0) - SNAP_PX / 2000.0).abs() < 1e-6);
    }

    #[test]
    fn transitioning_frame_settles_after_the_interp_window() {
        let f = MeterFrame {
            prev: 0.2,
            value: 0.8,
            since: Instant::now(),
            peak: 0.0,
            rms: -1.0,
            clip_at: None,
            peak_at: None,
        };
        assert!(
            f.is_settling(f.since),
            "just started — should still be settling"
        );
        assert!(
            f.is_settling(f.since + Duration::from_millis(10)),
            "mid-transition — should still be settling"
        );
        assert!(
            !f.is_settling(f.since + Duration::from_millis(60)),
            "past the interp window — should have stopped requesting redraws"
        );
    }

    #[test]
    fn at_interpolates_linearly_between_prev_and_value() {
        let f = MeterFrame {
            prev: 0.0,
            value: 1.0,
            since: Instant::now(),
            peak: 0.0,
            rms: -1.0,
            clip_at: None,
            peak_at: None,
        };
        assert_eq!(f.at(f.since), 0.0);
        assert_eq!(f.at(f.since + Duration::from_millis(50)), 1.0);
        let mid = f.at(f.since + Duration::from_millis(25));
        assert!(
            (mid - 0.5).abs() < 0.01,
            "expected ~0.5 at the midpoint, got {mid}"
        );
    }

    #[test]
    fn meter_zones_cover_the_scale_in_order() {
        let mut lower = FLOOR_DB;
        for (upper, _) in METER_ZONES {
            assert!(upper > lower);
            lower = upper;
        }
        assert_eq!(lower, TOP_DB);
    }
}
