//! A rotary knob — TotalMix-style value control (gain, pan) as an
//! alternative to a linear track. Drag vertically to turn it (a literal
//! circular drag is fiddly with a 2D mouse, so — like every DAW's own
//! virtual knobs, this included — "up" and "down" turn it, not tracing
//! an arc), shift-drag for fine adjustment, scroll wheel to nudge,
//! double-click to reset to default. Same interaction shape as
//! [`super::fader::Fader`], just driving an angle instead of a cap
//! position.

use iced::advanced::text;
use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Canvas, Geometry, Path, Stroke};
use iced::{alignment, mouse, window, Color, Element, Length, Radians, Rectangle, Renderer, Theme};

use std::time::Instant;

use super::fader::{DOUBLE_CLICK, FINE_SENSITIVITY, SCROLL_IDLE};

/// How long the pointer rests on a knob before the wheel turns it.
const WHEEL_DWELL: std::time::Duration = std::time::Duration::from_millis(300);
use super::feel::{self, Glide};
use crate::theme;
use super::knob_paint::{Ink, Pen};

/// TotalMix 2.0-style knob: a flat dark disc with a single position tick
/// and its value printed in the middle, rather than a colored arc + needle
/// — see `Bus_design.png`. Sized to read as a real focal point on the
/// strip (roughly what the reference shows relative to the card width),
/// not a small accent.
const DIAMETER: f32 = 42.0;
const TICK_LEN: f32 = 5.0;
/// The tick extends past `DIAMETER`'s own edge (see `draw`); the canvas
/// element's actual box (see `knob()`) is padded by this on every side so
/// it doesn't get clipped by the widget's own layout bounds — the drawn
/// circle itself (radius, face, tick — all still sized off `DIAMETER`) is
/// unchanged, just centered in a slightly bigger box.
const MARGIN: f32 = TICK_LEN + 1.0;
/// Vertical drag distance (at `scale == 1.0`) for a full sweep across
/// `range` — independent of the knob's own on-screen size, the same way a
/// real trim pot's turn radius has nothing to do with how far a mouse
/// drags it.
const DRAG_PX: f32 = 150.0;
/// Sweep geometry: clockwise from the positive x-axis (screen convention,
/// so 0=3 o'clock, 90°=6 o'clock/down, 180°=9 o'clock, 270°=12 o'clock).
/// Starting at 135° (lower-left, ~7:30) and sweeping 270° clockwise lands
/// on 45° (lower-right, ~4:30) — the standard knob shape, gap centered on
/// 6 o'clock, indicator pointing straight up at the midpoint value.
const START_DEG: f32 = 135.0;
const SWEEP_DEG: f32 = 270.0;

/// Where a knob shows its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Readout {
    /// On the cap's top (the EQ's knobs, the gain and trim knobs, where a
    /// grid has no room beside them).
    #[default]
    OnFace,
    /// In an inset box beside it, the knob over the box's left end (the
    /// strip's pan; 2026-11-03, the owner's reference: a clean dome with a
    /// long pointer, its value in a little display at its side). Reverses
    /// the earlier "value in the middle" (`Bus_design.png`) for the strip.
    Beside,
    /// Under it (the EQ's knobs, as the reference prints them).
    Below,
}

pub struct Knob<Message> {
    pub value: f32,
    /// Where the value shows (`Readout`).
    pub readout: Readout,
    pub range: (f32, f32),
    /// Printed centered inside the knob face — the reference design has no
    /// separate value readout below it, the knob doubles as the readout.
    pub label: String,
    /// Draws a filled orange arc from the 12-o'clock (center-of-range)
    /// position round to the current value, like the real TotalMix pan
    /// knob (confirmed in a live screenshot: e.g. "L57" shows a gold arc
    /// swept from top toward the left). Only meaningful for knobs whose
    /// range has a centered default — `false` for gain/freq/Q/low-cut,
    /// where TotalMix draws just the plain tick.
    pub arc_from_center: bool,
    /// Where the knob rests, when that isn't its start or (with
    /// `arc_from_center`) its middle: the arc runs from here, and passing
    /// it splashes. Width's +1.00 ("as recorded"; the owner's pick from
    /// mockups, 2026-11-10: nothing lit at rest).
    pub home: Option<f32>,
    /// `false` renders a dimmed, inert knob that ignores all mouse
    /// input — for a slot the reference visually has a knob in, but
    /// `tuxmix-core`/`tuxmix-usb` has no real control backing yet (e.g.
    /// an Output strip's own pan/balance — see `widgets/strip.rs`'s
    /// `full_strip` doc comment on why that one specifically has no
    /// working control). A knob that's just visually absent (a blank
    /// spacer) reads as "control not built yet"; a knob that's present
    /// but greyed out reads as "not applicable to this channel" — a
    /// clearer, more honest signal than either a working-looking fake or
    /// nothing at all.
    pub interactive: bool,
    /// `true` maps `value`↔drag-position logarithmically instead of
    /// linearly — for frequency knobs (EQ band freq, low-cut freq),
    /// where `range` spans 20-20,000 Hz. A linear mapping there
    /// compresses almost the entire musically-useful range (20 Hz-2
    /// kHz — bass through low-mid, most of what EQ work actually
    /// targets) into a sliver of the knob's drag travel, while the top
    /// octave (10-20 kHz) alone eats roughly a quarter of it. Every
    /// other knob (pan/gain/Q/pitch/width/trim) stays linear —
    /// correct there, since those values are already additive (dB,
    /// percent, a linear pan position) rather than multiplicative like
    /// frequency. Requires `range` to be strictly positive on both
    /// ends; falls back to linear otherwise rather than taking `ln`
    /// of a non-positive number.
    pub log_scale: bool,
    /// Colors the rim, the tick and the arc: the EQ's band color, so a
    /// knob reads as belonging to its band and to its curve in the plot,
    /// as in TotalMix. `None` is the usual neutral knob.
    pub tint: Option<Color>,
    pub modifiers: Modifiers,
    pub scale: f32,
    pub on_change: Box<dyn Fn(f32) -> Message>,
    pub on_reset: Box<dyn Fn() -> Message>,
}

#[derive(Default)]
pub struct State {
    /// The knob's drawing, rebuilt only when `cache_key` changes (see
    /// `draw`).
    cache: canvas::Cache,
    cache_key: std::cell::Cell<u64>,
    dragging: bool,
    last_click: Option<Instant>,
    /// See `Fader::State::drag_pos` — re-read fresh every move rather than
    /// anchored at press time, so Shift can toggle mid-drag without a jump.
    drag_pos: Option<f32>,
    /// See `Fader::State::drag_t` — accumulated across the drag rather
    /// than re-derived from `self.value`, so a batch of several
    /// `CursorMoved` events per rendered frame doesn't drop all but the
    /// last one.
    drag_t: Option<f32>,
    scroll_t: Option<f32>,
    last_scroll: Option<Instant>,
    /// Since when the pointer has rested on the knob: a wheel turns it
    /// only after `WHEEL_DWELL` there (critique #20: a knob sliding under
    /// a still pointer while its panel scrolled took the wheel, and a live
    /// mic's gain moved ~10 dB).
    hover_since: Option<Instant>,
    /// See `Fader::State::glide` — eases in value changes that didn't
    /// come from this knob's own drag (group selection, scene load).
    glide: Option<Glide>,
    /// When the knob last landed on its home (its centre, or a reset):
    /// the ring it sends out (`feel::splash`).
    splash: Option<Instant>,
    /// When the knob was last turned against an end of its travel, and
    /// which way (+1 past the top, -1 past the bottom): the cap's shudder
    /// (`feel::spring`), with the end stop's bump (`sounds::Stop`).
    stop: Option<(Instant, f32)>,
}

impl<Message> Knob<Message> {
    /// Where the arc starts and the splash sits, as a share of the turn:
    /// `home`, else the middle for a centred knob, else the start.
    fn rest_t(&self) -> f32 {
        match self.home {
            Some(h) => self.value_to_t(h),
            None if self.arc_from_center => 0.5,
            None => 0.0,
        }
    }

    fn value_to_t(&self, value: f32) -> f32 {
        let (lo, hi) = self.range;
        if hi <= lo {
            return 0.0;
        }
        if self.log_scale && lo > 0.0 {
            let (log_lo, log_hi) = (lo.ln(), hi.ln());
            ((value.max(f32::MIN_POSITIVE).ln() - log_lo) / (log_hi - log_lo)).clamp(0.0, 1.0)
        } else {
            ((value - lo) / (hi - lo)).clamp(0.0, 1.0)
        }
    }

    fn t_to_value(&self, t: f32) -> f32 {
        let (lo, hi) = self.range;
        let t = t.clamp(0.0, 1.0);
        if self.log_scale && lo > 0.0 && hi > lo {
            let (log_lo, log_hi) = (lo.ln(), hi.ln());
            (log_lo + t * (log_hi - log_lo)).exp()
        } else {
            lo + t * (hi - lo)
        }
    }
}

fn angle_of(t: f32) -> Radians {
    Radians((START_DEG + t.clamp(0.0, 1.0) * SWEEP_DEG).to_radians())
}

/// The knob: a turned cap (2026-11-03, the owner, of a hardware mixer's
/// knobs: "The nubs and sliders and buttons should look" like these; the
/// earlier ring with a dot on it read as a progress widget, not a knob):
///
/// - **The cap** is the knob: dark anodised metal in every theme
///   (`theme::cap_body`), a conical skirt lit from above (bright at the
///   top, in shadow at the bottom) round a flat top lit the other way, so
///   it reads as a turned part standing on the strip, with its shadow
///   under it.
/// - **A pointer** on the cap's rim at the value, in the cap's own ink,
///   engraved (a dark line under it).
/// - **A value ring** hugging the cap: a recessed track over the whole
///   sweep, and the value as an arc over it in the accent (or the knob's
///   tint), from the low end, or from the centre for a pan knob.
/// - **The label** on the cap's top, clear of the pointer.
///
/// An inert knob draws faded but solid: its colours are mixed toward the
/// ground rather than drawn see-through (the owner, 2026-11-13, in a light
/// theme: the pan box showed through a no-preamp strip's knob, "strange
/// transparency"). Only its shadows stay light.
/// How far toward white the dome's top reads under its label (`draw_dome`'s
/// light), measured from the captures.
const DOME_TOP_LIGHT: f32 = 0.36;

fn draw_knob<Message>(frame: &mut canvas::Frame, knob: &Knob<Message>, t: f32, dragging: bool, hovered: bool) {
    let s = knob.scale;
    let c = frame.center();
    let dim = if knob.interactive { 1.0 } else { 0.4 };
    let a = |col: Color, alpha: f32| Color { a: col.a * alpha * dim, ..col };
    let ring_r = (DIAMETER / 2.0 + 0.5) * s;
    let ring_w = 3.0 * s;
    // The cap, with a hair of ground between it and the ring.
    let face_r = (DIAMETER / 2.0 - 2.0) * s;
    // The flat top inside the skirt, wide enough for "1.00k".
    let top_r = face_r - 3.0 * s;
    let tint = knob.tint.unwrap_or(theme::accent());

    // The track: the whole sweep, a groove cut round the cap (a tinted
    // knob keeps its colour in it, so an EQ band is known by it even at
    // zero). The edge colour under the shade keeps it readable on a light
    // theme's pale ground and on the lamp wash. An inert one's is solid,
    // a pale groove. Under the cursor the track comes up, so the knob says
    // it's ready.
    let ground = theme::surface();
    let up = if hovered { 1.5 } else { 1.0 };
    let track = match (knob.tint, knob.interactive) {
        (Some(t), true) => a(t, 0.38 * up),
        (None, true) => a(theme::control_edge(), 0.40 * up),
        (tint, false) => theme::blend(ground, tint.unwrap_or_else(theme::control_edge), 0.35),
    };
    // The skirt: a cone lit from above. An inert one solid, its metal
    // washed toward the ground. The top: a dome, a little brighter while
    // it's held.
    let body = if hovered { theme::hovered(theme::cap_body()) } else { theme::cap_body() };
    let body = if knob.interactive { body } else { theme::blend(body, ground, 0.6) };
    let lift: f32 = if dragging { 0.10 } else { 0.0 };
    let turn = rib_turn(angle_of(t).0);
    let rest = knob.rest_t();
    let ink = theme::ink_on(theme::cap_body());
    let color = pointer_lit(ink, dragging, hovered);
    // All of it one picture with exact edge smoothing (`knob_paint`), the
    // label over it.
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        body_key(2, face_r, s, body, lift, dim, turn).hash(&mut h);
        ((t * 4000.0).round() as i32, (rest * 4000.0).round() as i32, dragging, hovered, knob.interactive).hash(&mut h);
        [track, tint].map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]).hash(&mut h);
        h.finish()
    };
    super::knob_paint::body(frame, c, ring_r + ring_w + 2.0 * s, key, |pen, c| {
        let (a0, a1) = (angle_of(0.0).0, angle_of(1.0).0);
        if knob.interactive {
            pen.arc(c, ring_r, a0, a1, ring_w + 1.0 * s, Color { a: 0.45, ..Color::BLACK });
        }
        pen.arc(c, ring_r, a0, a1, ring_w, track);
        // The value: an arc from where the knob rests to where it is.
        if knob.interactive {
            let (lo, hi) = if t < rest { (t, rest) } else { (rest, t) };
            if hi - lo > 0.004 {
                pen.arc(c, ring_r, angle_of(lo).0, angle_of(hi).0, ring_w, a(tint, 1.0));
            }
        }
        // The cap's shadow on the strip (`soft_shadow`; the owner's
        // "choppy").
        soft_shadow(pen, c, face_r, s, dim);
        draw_skirt(pen, c, face_r, top_r, turn, body, dim);
        feather(pen, c, face_r, body, s);
        draw_dome(pen, c, top_r, theme::blend(body, Color::WHITE, 0.08), lift, dim);
        // Held, its middle lights green (the owner, 2026-11-03: "when
        // dragging the knobs, the middle should light up green").
        if dragging {
            light_up(pen, c, top_r, s);
        }
        // The top's lower lip catches the light the skirt's upper half does.
        pen.arc(c, top_r + 0.6 * s, std::f32::consts::PI * 0.15, std::f32::consts::PI * 0.85, 1.0 * s, Color { a: 0.10 * dim, ..Color::WHITE });
        pen.ring(c, face_r, 1.0 * s, Ink::Solid(Color { a: 0.65 * dim, ..Color::BLACK }));

        // The pointer: engraved on the rim, from the top's edge out across
        // the skirt, so the label on the top stays clear.
        let angle = angle_of(t).0;
        let (dx, dy) = (angle.cos(), angle.sin());
        let along = |r: f32, off: f32| iced::Point::new(c.x + dx * r, c.y + dy * r + off);
        let (inner, outer) = (face_r * 0.66, face_r - 1.4 * s);
        pen.line(along(inner, 0.9 * s), along(outer, 0.9 * s), 2.6 * s, Color { a: 0.55 * dim, ..Color::BLACK });
        if dragging || hovered {
            pen.line(along(inner, 0.0), along(outer, 0.0), 4.6 * s, Color { a: 0.35 * dim, ..color });
        }
        pen.line(along(inner, 0.0), along(outer, 0.0), (if dragging { 2.6 } else { 2.2 }) * s, a(color, 1.0));
    });

    // The label sits on the dome's lit top, about a third of the way to
    // white from its body (measured: Solarized #224b53 → #718b90 under the
    // digits), so its ink is chosen for that, not for the body (critique #82
    // B: the cream "38" read 3.28 on Audra's dome, 3.17 on Solarized's).
    let top = theme::blend(body, Color::WHITE, DOME_TOP_LIGHT);
    // Pure black or white, whichever reads more: the dome's light runs
    // across the digits, so the margin matters more than the hue.
    let label_ink = if theme::contrast(Color::BLACK, top) >= theme::contrast(Color::WHITE, top) { Color::BLACK } else { Color::WHITE };
    frame.fill_text(canvas::Text {
        content: knob.label.clone(),
        position: c,
        color: a(label_ink, 1.0),
        size: (theme::TEXT_SM * s).into(),
        align_x: text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

/// A convex dome of `body` at `c`, radius `r`, lit from above as the
/// skirt round it is (2026-11-03, the owner: the knobs "still look pretty
/// flat"; then the glint at the top left "is not doing it any good"). The
/// canvas has linear gradients only, so: the cap lit from the top, a soft
/// round highlight built from discs drifting toward the light, a broad
/// soft sheen across its top, and its rim lit above and shaded below.
/// `lift` brightens it (held); `dim` fades an inert one's light and shade,
/// its body solid (a washed `body` says it's inert).
fn draw_dome(pen: &mut dyn Pen, c: iced::Point, r: f32, body: Color, lift: f32, dim: f32) {
    let at = |dx: f32, dy: f32| iced::Point::new(c.x + dx * r, c.y + dy * r);
    let lit = |k: f32| theme::blend(body, Color::WHITE, (k + lift).min(1.0));
    let shade = |k: f32| theme::blend(body, Color::BLACK, k);
    pen.disc(c, r, Ink::Linear(at(-0.2, -1.0), at(0.2, 1.0), vec![(0.0, lit(0.34)), (0.45, lit(0.06)), (1.0, shade(0.5))]));
    // Round: lightest a little above the middle, falling off toward the
    // edge all the way round, as a convex cap turns away from the light.
    // Discs shrinking toward the light, then the edge darkened in rings.
    for i in 0..9 {
        let k = i as f32 / 9.0;
        pen.disc(at(0.0, -0.22 * k), r * (0.92 - 0.075 * i as f32), Ink::Solid(Color { a: 0.04 * dim, ..Color::WHITE }));
    }
    for (d, alpha) in [(1.0f32, 0.3f32), (2.0, 0.18), (3.0, 0.1), (4.0, 0.05)] {
        pen.ring(c, (r - d).max(0.0), 1.2, Ink::Solid(Color { a: alpha * dim, ..Color::BLACK }));
    }
    // The sheen: very faint, broad across the top.
    pen.disc(
        at(0.0, -0.4),
        0.55 * r,
        Ink::Linear(at(0.0, -0.95), at(0.0, -0.15), vec![(0.0, Color { a: 0.12 * dim, ..Color::WHITE }), (1.0, Color { a: 0.0, ..Color::WHITE })]),
    );
    // The rim: lit above, in shadow below, and a dark edge.
    pen.ring(
        c,
        r - 0.6,
        1.2,
        Ink::Linear(
            at(0.0, -1.0),
            at(0.0, 1.0),
            vec![(0.0, Color { a: 0.35 * dim, ..Color::WHITE }), (0.5, Color { a: 0.0, ..Color::WHITE }), (1.0, Color { a: 0.45 * dim, ..Color::BLACK })],
        ),
    );
    // Its edge: on a dark theme a fine dark line; on a light one a broad
    // soft shade where the dome meets the skirt, since a 1 px near-black
    // circle on pale metal steps round the cap under the GPU's four
    // samples a pixel (the owner, 2026-11-14: "they look pixilated";
    // measured with tools/checks/totalMix-knob-ring.py).
    if theme::is_dark() {
        pen.ring(c, r, 1.0, Ink::Solid(Color { a: 0.55 * dim, ..Color::BLACK }));
    } else {
        pen.ring(c, r + 0.4, 2.2, Ink::Solid(Color { a: 0.2 * dim, ..Color::BLACK }));
    }
}

/// A conical skirt round a dome, radius `r`, its dome `inner`: bright at
/// the top where it faces the light, dark at its foot, and knurled: fine
/// engraved ribs round it (a grip, the owner: "a cap that I could turn")
/// that turn with the knob (`turn`, its pointer's angle), so turning it
/// visibly turns the cap.
fn draw_skirt(pen: &mut dyn Pen, c: iced::Point, r: f32, inner: f32, turn: f32, body: Color, dim: f32) {
    // On a light theme: one clean rim, a filled disc a little wider than
    // the skirt in the cap's own deeper tone, so the silhouette is a single
    // smoothed edge; and a skirt that stays metal to its foot (it fell to
    // 65% black, a dark smudge under every cap on a pale strip; the owner,
    // 2026-11-14: "still look bad in light view").
    let dark = theme::is_dark();
    if !dark {
        pen.disc(c, r + 0.5, Ink::Solid(Color { a: dim.max(0.6), ..theme::blend(body, Color::BLACK, 0.42) }));
    }
    let (top, mid, foot) = if dark { (0.3, 0.15, 0.65) } else { (0.45, 0.04, 0.3) };
    pen.disc(
        c,
        if dark { r } else { r - 0.6 },
        Ink::Linear(
            iced::Point::new(c.x, c.y - r),
            iced::Point::new(c.x, c.y + r),
            vec![
                (0.0, theme::blend(body, Color::WHITE, top)),
                (0.35, theme::blend(body, Color::BLACK, mid * dim)),
                (1.0, theme::blend(body, Color::BLACK, foot * dim)),
            ],
        ),
    );
    const RIBS: usize = 28;
    let (from, to) = (inner + 0.8, r - 1.2);
    if to > from {
        let ribs = |offset: f32| -> Vec<(iced::Point, iced::Point)> {
            (0..RIBS)
                .map(|i| {
                    let angle = turn + offset + i as f32 * std::f32::consts::TAU / RIBS as f32;
                    let (dx, dy) = (angle.cos(), angle.sin());
                    (iced::Point::new(c.x + dx * from, c.y + dy * from), iced::Point::new(c.x + dx * to, c.y + dy * to))
                })
                .collect()
        };
        // On a light theme's silver the ribs are a whisper (the reviewer:
        // at 1.6:1 they read as a dark serrated ring, noise on every knob).
        let dark = theme::is_dark();
        pen.segments(&ribs(0.0), if dark { 1.0 } else { 1.6 }, Color { a: (if dark { 0.32 } else { 0.055 }) * dim, ..Color::BLACK });
        if dark {
            pen.segments(&ribs(0.05), 0.8, Color { a: 0.1 * dim, ..Color::WHITE });
        }
    }
    // The skirt's foot line, a pixel inside the silhouette and soft on a
    // light ground: exactly on the edge, a 1px near-black circle stepped
    // round the knob like beads under the GPU's 4x smoothing (critique
    // #47, the owner's "choppy").
    // Dark themes only: on a light one it was the knob's worst stepped
    // ring (2026-11-14, measured), and the skirt's own gradient ends dark.
    let foot_a = 0.7;
    if theme::is_dark() {
        pen.ring(c, r - 1.0, 1.0, Ink::Solid(Color { a: foot_a * dim, ..Color::BLACK }));
    }
}

/// The ribs' turn, in steps of a 24th of the gap between two ribs, so a
/// turning knob reuses a few dozen pictures (`knob_paint::body`).
fn rib_turn(turn: f32) -> f32 {
    let step = std::f32::consts::TAU / 28.0 / 24.0;
    (turn / step).round() * step
}

/// Everything a knob body's picture depends on (`knob_paint::body`).
fn body_key(kind: u8, r: f32, s: f32, body: Color, lift: f32, dim: f32, turn: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    kind.hash(&mut h);
    for v in [r, s, body.r, body.g, body.b, body.a, lift, dim, turn.rem_euclid(std::f32::consts::TAU / 28.0)] {
        v.to_bits().hash(&mut h);
    }
    (theme::is_dark(), theme::palette_key()).hash(&mut h);
    h.finish()
}

/// A held knob's middle, touched with green (`theme::signal`).
fn light_up(pen: &mut dyn Pen, c: iced::Point, r: f32, s: f32) {
    // Subtle (the owner: a solid green face was "too intense"): the
    // faintest green over the face and a thin ring at its edge; the
    // pointer's green says the rest.
    let green = theme::signal();
    pen.disc(c, r, Ink::Solid(Color { a: 0.1, ..green }));
    pen.ring(c, r - 0.5 * s, 1.2 * s, Ink::Solid(Color { a: 0.45, ..green }));
}

/// The dial's centre and radius in a `Readout::Beside` knob's frame: at
/// the left, as big as `DIAMETER`, centred on the row.
fn beside_dial(frame: &canvas::Frame, scale: f32) -> (iced::Point, f32) {
    let r = DIAMETER / 2.0 * scale;
    (iced::Point::new(MARGIN * scale + r, frame.height() / 2.0), r)
}

/// The value box beside a `Readout::Beside` dial: from under the dial's
/// right half to the row's end, a little over half the dial's height.
fn beside_box(frame: &canvas::Frame, c: iced::Point, r: f32, scale: f32) -> Rectangle {
    let h = 24.0 * scale;
    let x = c.x + 0.45 * r;
    Rectangle::new(iced::Point::new(x, c.y - h / 2.0), iced::Size::new((frame.width() - 1.0 * scale - x).max(0.0), h))
}

/// The pointer's colour on a cap of `body`: the accent where it reads at
/// 3:1, else pulled toward the cap's ink until it does (a light theme's
/// deep accent on a dark cap).
fn pointer_on(body: Color, hue: Color) -> Color {
    (0..=10)
        .map(|k| theme::blend(hue, theme::ink_on(body), k as f32 / 10.0))
        .find(|c| theme::contrast(*c, body) >= 3.0)
        .unwrap_or_else(|| theme::ink_on(body))
}

/// A `Readout::Beside` knob (the owner's reference): a dome at the left,
/// a thick dark bezel round a convex face lit from the top left, a long
/// pointer in the accent (or the knob's tint), and the value arc on the
/// bezel; its value in an inset box that runs from under the dome's right
/// half to the row's end, in the Mono, `text_primary` (the Fixed Slot
/// rule; the reference's green is `signal`'s here). An inert knob draws
/// faded: its words at 40%, its dial solid and washed (`draw_dial`).
fn draw_knob_beside<Message>(frame: &mut canvas::Frame, knob: &Knob<Message>, t: f32, dragging: bool, hovered: bool) {
    let s = knob.scale;
    let dim = if knob.interactive { 1.0 } else { 0.4 };
    let a = |col: Color, alpha: f32| Color { a: col.a * alpha * dim, ..col };
    let (c, r) = beside_dial(frame, s);

    // The box, under the dome: an inset like the lane and the meter.
    let bx = beside_box(frame, c, r, s);
    // As a picture with exact edge smoothing, drawn before the dial's so it
    // stays under it (pictures draw in order, above the canvas's shapes;
    // the owner, 2026-11-14: its rounded corners looked "pixelated").
    let rim = hovered.then(|| a(theme::fader_color(), 1.0));
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (3u8, s.to_bits(), theme::palette_key(), theme::is_dark()).hash(&mut h);
        rim.map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]).hash(&mut h);
        h.finish()
    };
    let pad = 2.0 * s;
    let outer = Rectangle::new(iced::Point::new(bx.x - pad, bx.y - pad), iced::Size::new(bx.width + 2.0 * pad, bx.height + 2.0 * pad));
    super::knob_paint::picture(frame, outer, key, |pen, local| {
        let inner = Rectangle::new(iced::Point::new(local.x + pad, local.y + pad), bx.size());
        super::fader::draw_inset_pen(pen, inner, 6.0 * s, s, rim);
    });
    let visible_from = c.x + r + 1.0 * s;
    frame.fill_text(canvas::Text {
        content: knob.label.clone(),
        position: iced::Point::new((visible_from + bx.x + bx.width) / 2.0, c.y),
        // An inert one's "C" quieter but solid (critique #51: at 40% it read
        // 2.3:1 in light themes).
        color: if knob.interactive { theme::text_primary() } else { theme::text_sec() },
        font: theme::MONO,
        size: (theme::TEXT_NUM * s).into(),
        align_x: text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
    draw_dial(frame, knob, t, dragging, hovered, c, r);
}

/// A `Readout::Below` knob (the EQ's; the owner's reference prints each
/// value under its knob): the dial in the top square, its value under it
/// in the Mono, `text_primary`.
fn draw_knob_below<Message>(frame: &mut canvas::Frame, knob: &Knob<Message>, t: f32, dragging: bool, hovered: bool) {
    let s = knob.scale;
    let dim = if knob.interactive { 1.0 } else { 0.4 };
    let r = (DIAMETER / 2.0 - 1.0) * s;
    let c = iced::Point::new(frame.width() / 2.0, BOX_SIZE * s / 2.0);
    draw_dial(frame, knob, t, dragging, hovered, c, r);
    frame.fill_text(canvas::Text {
        content: knob.label.clone(),
        position: iced::Point::new(c.x, BOX_SIZE * s + BELOW_TEXT * s / 2.0 - 2.0 * s),
        color: Color { a: dim, ..theme::text_primary() },
        font: theme::MONO,
        size: (theme::TEXT_SM * s).into(),
        align_x: text::Alignment::Center,
        align_y: alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

/// The room under a `Readout::Below` knob for its value, at scale 1.
pub(crate) const BELOW_TEXT: f32 = 14.0;

/// A knob's dial at `c`, radius `r` (the strip's and the EQ's): its
/// shadow, the knurled skirt, the value arc on the bezel (and a band's
/// ring, for a centred knob with a tint: the EQ's gain knobs, as the
/// reference rings them in their band's colour), the domed face, and the
/// long pointer.
fn draw_dial<Message>(
    frame: &mut canvas::Frame,
    knob: &Knob<Message>,
    t: f32,
    dragging: bool,
    hovered: bool,
    c: iced::Point,
    r: f32,
) {
    let s = knob.scale;
    let dim = if knob.interactive { 1.0 } else { 0.4 };
    let a = |col: Color, alpha: f32| Color { a: col.a * alpha * dim, ..col };
    let hue = knob.tint.unwrap_or(theme::accent());
    // The dome's shadow, then the bezel: a thick dark ring, lit a little
    // along its top.
    // Soft: rings of falling darkness, not two hard discs (the owner,
    // 2026-11-13: the knobs' edges looked "choppy ... pixilated"; two
    // hard-edged discs drew a stepped double outline under each cap).
    let body = if hovered { theme::hovered(theme::cap_body()) } else { theme::cap_body() };
    // An inert dial (a strip that can't pan) solid, its metal washed toward
    // the ground, never see-through: the value box under it showed through
    // (the owner, 2026-11-13, in a light theme: "strange transparency").
    let body = if knob.interactive { body } else { theme::blend(body, theme::surface(), 0.55) };
    let face_r = r - 4.8 * s;
    let lift: f32 = if dragging { 0.1 } else { 0.0 };
    let up = if hovered { 1.5 } else { 1.0 };
    // An inert dial's pointer in the cap's own ink, not the accent
    // (critique #46: in light themes its red still read as live).
    let color = if knob.interactive { pointer_lit(pointer_on(body, hue), dragging, hovered) } else { theme::ink_on(body) };
    let turn = rib_turn(angle_of(t).0);
    let rest = knob.rest_t();
    // The whole dial (shadow, knurled skirt, the value on the bezel, the
    // dome, the pointer) as one picture drawn with exact edge smoothing
    // (`knob_paint`; the owner, 2026-11-14: "pixilated around the rim of
    // each nub ... all themes"). Pictures sit above a canvas's shapes, so
    // nothing of the dial is left on the canvas but the value's text.
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        body_key(1, r, s, body, lift, dim, turn).hash(&mut h);
        ((t * 4000.0).round() as i32, (rest * 4000.0).round() as i32, dragging, hovered, knob.interactive, knob.arc_from_center).hash(&mut h);
        knob.tint.map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]).hash(&mut h);
        h.finish()
    };
    super::knob_paint::body(frame, c, r + 6.0 * s, key, |pen, c| {
        soft_shadow(pen, c, r, s, dim);
        draw_skirt(pen, c, r, face_r, turn, body, dim);
        feather(pen, c, r, body, s);

        // The value on the bezel: a faint track over the sweep, the value
        // as an arc over it, from the low end or (pan) the centre.
        let ring = r - 2.4 * s;
        pen.arc(c, ring, angle_of(0.0).0, angle_of(1.0).0, 2.0 * s, a(theme::control_edge(), 0.28 * up));
        if knob.interactive {
            let (lo, hi) = if t < rest { (t, rest) } else { (rest, t) };
            if hi - lo > 0.004 {
                pen.arc(c, ring, angle_of(lo).0, angle_of(hi).0, 2.0 * s, a(hue, 1.0));
            }
        }
        // A band's ring: the EQ's gain knobs wear their band's colour.
        if let (Some(tint), true) = (knob.tint, knob.arc_from_center) {
            pen.ring(c, r - 0.8 * s, 1.6 * s, Ink::Solid(a(tint, 0.9)));
        }

        // The face: convex, lit from the top left, a fine highlight on its
        // rim; held, it lights.
        draw_dome(pen, c, face_r, theme::blend(body, Color::WHITE, 0.08), lift, dim);
        if dragging {
            light_up(pen, c, face_r, s);
        }
        pen.arc(c, face_r - 1.0 * s, std::f32::consts::PI * 1.1, std::f32::consts::PI * 1.6, 1.0 * s, Color { a: 0.18 * dim, ..Color::WHITE });

        // The pointer: long, from near the middle to the face's rim.
        let angle = angle_of(t).0;
        let (dx, dy) = (angle.cos(), angle.sin());
        let along = |k: f32| iced::Point::new(c.x + dx * face_r * k, c.y + dy * face_r * k);
        if dragging || hovered {
            pen.line(along(0.22), along(0.86), 5.0 * s, Color { a: 0.35 * dim, ..color });
        }
        pen.line(along(0.22), along(0.86), (if dragging { 3.0 } else { 2.6 }) * s, a(color, 1.0));
    });
}

/// A cap's shadow on the strip: discs a little below it, each a little
/// wider and fainter, so its edge fades over a few pixels instead of
/// stepping (`dim` an inert one's).
fn soft_shadow(pen: &mut dyn Pen, c: iced::Point, r: f32, s: f32, dim: f32) {
    // On a light theme tight and close, a contact shadow: the wide one
    // read as a grey haze round every cap on a pale strip (2026-11-14).
    let (rings, drop, spread, alpha) = if theme::is_dark() { (6, 1.6, 2.0, 0.16) } else { (4, 0.8, 1.0, 0.13) };
    for i in 0..rings {
        let k = i as f32 / (rings - 1) as f32;
        pen.disc(
            iced::Point::new(c.x, c.y + (if theme::is_dark() { 1.2 } else { 0.8 } + drop * k) * s),
            r + (0.2 + spread * k) * s,
            Ink::Solid(Color { a: (alpha - (alpha - 0.03) * k) * dim, ..Color::BLACK }),
        );
    }
}

/// A half-pixel feather round a cap's silhouette in its own lower colour,
/// so the edge's stair-steps (the GPU smooths at four samples a pixel)
/// read as a soft round line.
fn feather(pen: &mut dyn Pen, c: iced::Point, r: f32, body: Color, s: f32) {
    // Not on a light theme: there its two thin rings added steps rather
    // than hiding them (2026-11-14, measured).
    if !theme::is_dark() {
        return;
    }
    // Between the cap and the ground it stands on, so it softens the step
    // rather than darkening it (critique #47).
    let edge = theme::blend(theme::blend(body, Color::BLACK, 0.2), theme::surface(), 0.4);
    pen.ring(c, r - 0.1 * s, 0.8 * s, Ink::Solid(Color { a: 0.55, ..edge }));
    pen.ring(c, r + 0.45 * s, 0.9 * s, Ink::Solid(Color { a: 0.18, ..edge }));
}

/// The pointer's colour while the knob is in use (2026-11-03, the owner:
/// "if I'm interacting with the knob, the vertical bar should turn green
/// ... Or whatever color is the accent color of it, it should become
/// brighter"): under the cursor its own colour brightens; while it's
/// turned it goes green (`signal`), as its middle lights.
fn pointer_lit(rest: Color, dragging: bool, hovered: bool) -> Color {
    if dragging {
        theme::blend(theme::signal(), Color::WHITE, 0.3)
    } else if hovered {
        theme::blend(rest, Color::WHITE, 0.4)
    } else {
        rest
    }
}

/// A `Readout::Beside` knob landing on its home (`feel::splash`, `p` 0..1):
/// its bezel flashes in the accent (or tint), and a ring leaves it.
fn draw_splash_beside<Message>(frame: &mut canvas::Frame, knob: &Knob<Message>, p: f32) {
    let s = knob.scale;
    let (c, r) = beside_dial(frame, s);
    let hue = knob.tint.unwrap_or(theme::accent());
    let fade = 1.0 - p;
    frame.stroke(
        &Path::circle(c, r - 2.4 * s),
        Stroke::default().with_color(Color { a: 0.6 * fade * fade, ..hue }).with_width(2.4 * s),
    );
    let e = crate::motion::ease_out(p);
    frame.stroke(
        &Path::circle(c, r + (MARGIN - 0.5) * s * e),
        Stroke::default().with_color(Color { a: 0.8 * fade, ..hue }).with_width((2.2 - 1.4 * p) * s),
    );
}

/// The ring a knob sends out when it lands on its home (`feel::splash`,
/// `p` 0..1): from the cap's edge out past the value ring, fading, with a
/// flash of the whole track in the accent (or tint) as it leaves.
fn draw_splash<Message>(frame: &mut canvas::Frame, knob: &Knob<Message>, p: f32) {
    let s = knob.scale;
    // On the dial, which sits in the top square of a `Readout::Below`
    // knob (its value is printed under it): the frame's centre was below
    // the dial (the owner: "the splash effect isnt centered").
    let c = match knob.readout {
        Readout::Below => iced::Point::new(frame.width() / 2.0, BOX_SIZE * s / 2.0),
        _ => frame.center(),
    };
    let hue = knob.tint.unwrap_or(theme::accent());
    let e = crate::motion::ease_out(p);
    let fade = 1.0 - p;
    let ring_r = (DIAMETER / 2.0 + 0.5) * s;
    frame.stroke(
        &Path::new(|b| {
            b.arc(canvas::path::Arc {
                center: c,
                radius: ring_r,
                start_angle: angle_of(0.0),
                end_angle: angle_of(1.0),
            })
        }),
        Stroke::default()
            .with_line_cap(canvas::LineCap::Round)
            .with_color(Color { a: 0.55 * fade * fade, ..hue })
            .with_width(3.0 * s),
    );
    let from = (DIAMETER / 2.0 - 2.0) * s;
    let to = BOX_SIZE / 2.0 * s - 1.0 * s;
    frame.stroke(
        &Path::circle(c, from + (to - from) * e),
        Stroke::default()
            .with_color(Color { a: 0.8 * fade, ..hue })
            .with_width((2.4 - 1.6 * p) * s),
    );
}

impl<Message> canvas::Program<Message> for Knob<Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if !self.interactive {
            return None;
        }
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_over(bounds)?;
                let now = Instant::now();
                let is_double = state
                    .last_click
                    .is_some_and(|t| now.duration_since(t) < DOUBLE_CLICK);

                if is_double {
                    state.dragging = false;
                    state.last_click = None;
                    state.drag_pos = None;
                    state.drag_t = None;
                    state.splash = Some(now);
                    return Some(canvas::Action::publish((self.on_reset)()).and_capture());
                }
                state.last_click = Some(now);
                state.dragging = true;
                state.drag_pos = Some(pos.y);
                state.drag_t = Some(self.value_to_t(self.value));
                // A new press may shudder once against an end again.
                state.stop = None;
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    if !cursor.is_over(bounds) {
                        state.hover_since = None;
                    } else if state.hover_since.is_none() {
                        state.hover_since = Some(Instant::now());
                    }
                    return None;
                }
                let pos = cursor.land().position()?;
                let prev_y = state.drag_pos.unwrap_or(pos.y);
                let screen_dt = -(pos.y - prev_y) / (DRAG_PX * self.scale);
                let mult = if self.modifiers.shift() {
                    FINE_SENSITIVITY
                } else {
                    1.0
                };
                let base_t = state.drag_t.unwrap_or_else(|| self.value_to_t(self.value));
                let wanted = base_t + screen_dt * mult;
                let t = wanted.clamp(0.0, 1.0);
                // Turned into an end (arriving, as the end stop's bump plays,
                // or pushing on against it): the cap shudders, once a press.
                // Not where the knob rests (Width's +1.00 is also its end):
                // home splashes there, it doesn't hit a wall (critique #22).
                let home_end = self.home.is_some() && ((wanted > 1.0 && self.rest_t() >= 1.0) || (wanted < 0.0 && self.rest_t() <= 0.0));
                if (wanted > 1.0 || wanted < 0.0) && state.stop.is_none() && !home_end {
                    state.stop = Some((Instant::now(), if wanted > 1.0 { 1.0 } else { -1.0 }));
                }
                // Through the centre of a centred knob (pan, an EQ band's
                // gain): it splashes, with the detent's tick.
                let rest = self.rest_t();
                if (self.arc_from_center || self.home.is_some()) && base_t != rest && (base_t - rest) * (t - rest) <= 0.0 {
                    state.splash = Some(Instant::now());
                }
                state.drag_pos = Some(pos.y);
                state.drag_t = Some(t);
                Some(canvas::Action::publish((self.on_change)(self.t_to_value(t))).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return None;
                }
                state.dragging = false;
                state.drag_pos = None;
                state.drag_t = None;
                state.glide = Some(Glide::still(self.value));
                None
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !cursor.is_over(bounds) || crate::widgets::wheel_off() {
                    return None;
                }
                // Ctrl+scroll never nudges a value here — see the
                // identical guard in `Fader::update`.
                if self.modifiers.control() {
                    return None;
                }
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
                // A new turn needs the pointer to have rested here first; one
                // under way carries on. Otherwise the wheel scrolls the panel.
                if fresh_gesture && state.hover_since.is_none_or(|t| now.duration_since(t) < WHEEL_DWELL) {
                    return None;
                }
                let base_t = if fresh_gesture {
                    self.value_to_t(self.value)
                } else {
                    state.scroll_t.unwrap_or_else(|| self.value_to_t(self.value))
                };
                let t = (base_t + dy * base_step * mult).clamp(0.0, 1.0);
                state.scroll_t = Some(t);
                state.last_scroll = Some(now);
                Some(canvas::Action::publish((self.on_change)(self.t_to_value(t))).and_capture())
            }
            // See `Fader::update`'s identical arm.
            canvas::Event::Window(window::Event::RedrawRequested(now)) => {
                let splashing = feel::splash(state.splash, *now).is_some() || feel::spring(state.stop, *now).is_some();
                if state.dragging {
                    return splashing.then(canvas::Action::request_redraw);
                }
                let shown = self.value_to_t(state.glide.map_or(self.value, |g| g.at(*now)));
                let jump = (self.value_to_t(self.value) - shown).abs() > feel::JUMP;
                let glide = state.glide.get_or_insert_with(|| Glide::still(self.value));
                glide.retarget(self.value, *now, jump);
                (glide.settling(*now) || splashing).then(canvas::Action::request_redraw)
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
        let now = Instant::now();
        let display_value = if state.dragging {
            self.value
        } else {
            state.glide.map(|g| g.at(now)).unwrap_or(self.value)
        };
        // Against an end stop the cap shudders back from it: about 11
        // degrees at its swing's peak (the spring's envelope peaks near
        // 0.64), in the picture only.
        let display_value = match (feel::spring(state.stop, now), state.stop) {
            (Some(d), Some((_, dir))) => self.t_to_value(self.value_to_t(display_value) - dir * d.abs() * (17.0 / SWEEP_DEG)),
            _ => display_value,
        };
        let hovered = self.interactive
            && !crate::widgets::locked()
            && (state.dragging || cursor.is_over(bounds));
        // Everything below is a pure function of these: so the geometry is
        // built once and reused until one of them changes. A mixer has
        // dozens of knobs and nearly all of them sit still, yet each was
        // re-tessellated on every redraw (a measurable slice of the frame).
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            display_value.to_bits().hash(&mut h);
            self.label.hash(&mut h);
            self.scale.to_bits().hash(&mut h);
            (state.dragging, self.interactive, self.arc_from_center, hovered, self.readout).hash(&mut h);
            self.home.map(f32::to_bits).hash(&mut h);
            self.tint
                .map(|c| [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()])
                .hash(&mut h);
            (bounds.width.to_bits(), bounds.height.to_bits()).hash(&mut h);
            crate::theme::palette_key().hash(&mut h);
            h.finish()
        };
        if state.cache_key.replace(key) != key {
            state.cache.clear();
        }
        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let t = self.value_to_t(display_value);
            match self.readout {
                Readout::OnFace => draw_knob(frame, self, t, state.dragging, hovered),
                Readout::Beside => draw_knob_beside(frame, self, t, state.dragging, hovered),
                Readout::Below => draw_knob_below(frame, self, t, state.dragging, hovered),
            }
        });
        match feel::splash(state.splash, now) {
            Some(p) => {
                let mut frame = canvas::Frame::new(renderer, bounds.size());
                match self.readout {
                    Readout::OnFace | Readout::Below => draw_splash(&mut frame, self, p),
                    Readout::Beside => draw_splash_beside(&mut frame, self, p),
                }
                vec![geometry, frame.into_geometry()]
            }
            None => vec![geometry],
        }
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        // The cursor says what the knob will do: an open hand over it, a
        // closed one turning it, crosshairs for a fine turn (Shift), and
        // "not allowed" over an inert knob or while the mix is locked.
        if state.dragging {
            if self.modifiers.shift() {
                mouse::Interaction::Crosshair
            } else {
                mouse::Interaction::Grabbing
            }
        } else if !cursor.is_over(bounds) {
            mouse::Interaction::None
        } else if !self.interactive || crate::widgets::locked() {
            mouse::Interaction::NotAllowed
        } else {
            mouse::Interaction::Grab
        }
    }
}

/// A knob's total footprint (at `scale == 1.0`).
pub(crate) const BOX_SIZE: f32 = DIAMETER + MARGIN * 2.0;

pub fn knob<'a, Message: Clone + 'a>(knob: Knob<Message>) -> Element<'a, Message> {
    let size = BOX_SIZE * knob.scale;
    // Beside: the whole row, the knob at its left and the box after it,
    // all one drag target.
    let (width, height) = match knob.readout {
        Readout::OnFace => (Length::Fixed(size), size),
        Readout::Beside => (Length::Fill, size),
        Readout::Below => (Length::Fixed(size), size + BELOW_TEXT * knob.scale),
    };
    Canvas::new(knob)
        .width(width)
        .height(Length::Fixed(height))
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_knob_label_reads_on_the_domes_lit_top_in_every_theme() {
        // Critique #82 B: the cream "38" read 3.28 / 3.17 on the dome's top.
        use crate::theme;
        use iced::Color;
        for t in theme::THEMES.iter() {
            theme::set_palette(t.palette);
            let top = theme::blend(theme::cap_body(), Color::WHITE, super::DOME_TOP_LIGHT);
            let ink = if theme::contrast(Color::BLACK, top) >= theme::contrast(Color::WHITE, top) { Color::BLACK } else { Color::WHITE };
            assert!(theme::contrast(ink, top) >= 4.5, "{}: {:.2}", t.id, theme::contrast(ink, top));
        }
    }

    use super::*;

    fn test_knob(range: (f32, f32), log_scale: bool) -> Knob<()> {
        Knob {
            readout: Readout::OnFace,
            tint: None,
            value: range.0,
            range,
            label: String::new(),
            arc_from_center: false,
            home: None,
            interactive: true,
            log_scale,
            modifiers: Modifiers::default(),
            scale: 1.0,
            on_change: Box::new(|_| ()),
            on_reset: Box::new(|| ()),
        }
    }

    #[test]
    fn log_scale_midpoint_t_is_the_geometric_not_arithmetic_mean() {
        // A linear-mapped 20-20,000 Hz knob would put t=0.5 at 10,010 Hz
        // (the arithmetic mean) — exactly the bug this field fixes: the
        // entire bass/low-mid range (20 Hz-2 kHz) would be squeezed into
        // a sliver of the knob's travel. Logarithmic mapping puts t=0.5
        // at the *geometric* mean instead (~632 Hz for this range).
        let k = test_knob((20.0, 20_000.0), true);
        let midpoint = k.t_to_value(0.5);
        let geometric_mean = (20.0f32 * 20_000.0).sqrt();
        assert!(
            (midpoint - geometric_mean).abs() < 1.0,
            "expected ~{geometric_mean} (geometric mean), got {midpoint}"
        );
        assert!(
            midpoint < 10_010.0 - 1000.0,
            "midpoint {midpoint} should be far below the arithmetic mean 10,010"
        );
    }

    #[test]
    fn log_scale_round_trips_value_and_t() {
        let k = test_knob((20.0, 20_000.0), true);
        for hz in [20.0, 100.0, 1000.0, 5000.0, 20_000.0] {
            let t = k.value_to_t(hz);
            let back = k.t_to_value(t);
            assert!((back - hz).abs() / hz < 0.001, "hz={hz} round-tripped to {back}");
        }
    }

    #[test]
    fn non_log_knob_stays_linear() {
        // A pan/gain-style knob (log_scale: false) must be completely
        // unaffected — the midpoint is the plain arithmetic mean.
        let k = test_knob((-100.0, 100.0), false);
        assert_eq!(k.t_to_value(0.5), 0.0);
    }

    #[test]
    fn angle_at_zero_is_start_angle() {
        let a = angle_of(0.0);
        assert!((a.0 - START_DEG.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn angle_at_one_is_full_sweep() {
        let a = angle_of(1.0);
        assert!((a.0 - (START_DEG + SWEEP_DEG).to_radians()).abs() < 1e-5);
    }

    #[test]
    fn angle_at_half_is_the_midpoint() {
        let a = angle_of(0.5);
        let expected = (START_DEG + SWEEP_DEG / 2.0).to_radians();
        assert!((a.0 - expected).abs() < 1e-5);
    }

    #[test]
    fn angle_clamps_out_of_range_t() {
        assert_eq!(angle_of(-1.0).0, angle_of(0.0).0);
        assert_eq!(angle_of(2.0).0, angle_of(1.0).0);
    }
}
