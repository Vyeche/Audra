//! The strip's drawn icons, in the stroke of the app's other drawn icon
//! (`panel_icon`):
//!
//! - Settings: a wrench, as TotalMix draws a strip's Settings key (it was
//!   three slider rails until 2026-11-06). The UI faces have no such
//!   glyph, so it is drawn; the app's settings have a drawn gear
//!   (`GearIcon`), the device a drawn unit (`DeviceIcon`).
//! - Narrow/wide: a solid triangle on a key, as TotalMix's, pointing left
//!   on a full strip and right on a narrow one. A bare "−" after the dB
//!   value read as part of the number ("−∞ dB −").
//! - Beyond the strip: the floor chevron and the top bar's padlock (Lock
//!   User Interface), in the same stroke.

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Renderer, Theme};

const STROKE: f32 = 1.3;

/// A strip's Settings key as TotalMix draws it (the owner, 2026-11-06:
/// "hardwear inputs has T, wrentch"): a wrench, its handle down to the
/// left, its open jaw up to the right.
pub struct WrenchIcon {
    pub color: Color,
    pub scale: f32,
}

/// The wrench's box, its handle's width and its head's radius at scale 1.
const WRENCH_BOX: f32 = 11.0;
const WRENCH_HANDLE: f32 = 2.2;
const WRENCH_HEAD_R: f32 = 2.6;

impl<Message> canvas::Program<Message> for WrenchIcon {
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
        let s = self.scale;
        let b = WRENCH_BOX * s;
        let o = Point::new((bounds.width - b) / 2.0, (bounds.height - b) / 2.0);
        let at = |x: f32, y: f32| Point::new(o.x + b * x, o.y + b * y);
        let round = |w: f32| {
            Stroke::default()
                .with_color(self.color)
                .with_width(w * s)
                .with_line_cap(canvas::LineCap::Round)
        };
        // The handle, from the foot to the head's rim.
        frame.stroke(&Path::line(at(0.12, 0.88), at(0.56, 0.44)), round(WRENCH_HANDLE));
        // The head: a ring open toward the upper right, along the handle.
        let centre = at(0.68, 0.32);
        let gap = 0.95;
        let mid = -std::f32::consts::FRAC_PI_4;
        let head = Path::new(|p| {
            p.arc(canvas::path::Arc {
                center: centre,
                radius: WRENCH_HEAD_R * s,
                start_angle: iced::Radians(mid + gap),
                end_angle: iced::Radians(mid - gap + std::f32::consts::TAU),
            });
        });
        frame.stroke(&head, round(1.8));
        vec![frame.into_geometry()]
    }
}

/// The wrench as an element filling its button.
pub fn wrench_icon<'a, Message: 'a>(color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(WrenchIcon { color, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// Audra's own settings (the top bar): a gear, eight teeth round a ring
/// with a hole. One glyph opened three panels (the critique, 2026-11-03):
/// the strip's three sliders stay the strip's Settings, the gear is the
/// app's, and the device has its own (`DeviceIcon`).
pub struct GearIcon {
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for GearIcon {
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
        let s = self.scale;
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let ring = Stroke::default().with_color(self.color).with_width(STROKE * s);
        // As big as the padlock beside it (the reviewer: at 11px with a
        // light stroke it was the weakest thing in the cluster).
        frame.stroke(&Path::circle(c, 5.0 * s), ring);
        frame.stroke(&Path::circle(c, 1.9 * s), ring);
        let tooth = Stroke::default().with_color(self.color).with_width(2.4 * s);
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let (dx, dy) = (a.cos(), a.sin());
            frame.stroke(
                &Path::line(
                    Point::new(c.x + dx * 5.4 * s, c.y + dy * 5.4 * s),
                    Point::new(c.x + dx * 7.4 * s, c.y + dy * 7.4 * s),
                ),
                tooth,
            );
        }
        vec![frame.into_geometry()]
    }
}

/// About and its keys and gestures (the top bar): a question mark in a
/// ring, drawn in the gear's stroke and at its size, so it stands with
/// the speaker, padlock and gear (the reviewer, 2026-11-05: a "?" from
/// the text face sat small and light beside them).
pub struct HelpIcon {
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for HelpIcon {
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
        let s = self.scale;
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let line = Stroke::default()
            .with_color(self.color)
            .with_width(STROKE * s)
            .with_line_cap(canvas::LineCap::Round);
        frame.stroke(&Path::circle(c, 7.0 * s), line);
        // The hook: from its left, over the top, round to its foot, then
        // the stem down; the dot under it.
        let hook = Point::new(c.x, c.y - 1.9 * s);
        let r = 2.3 * s;
        let mark = Path::new(|b| {
            b.arc(canvas::path::Arc {
                center: hook,
                radius: r,
                start_angle: iced::Radians(std::f32::consts::PI),
                end_angle: iced::Radians(2.5 * std::f32::consts::PI),
            });
            b.line_to(Point::new(c.x, c.y + 1.6 * s));
        });
        frame.stroke(&mark, line);
        frame.fill(&Path::circle(Point::new(c.x, c.y + 3.9 * s), 1.0 * s), self.color);
        vec![frame.into_geometry()]
    }
}

/// Close (a strip's floating panel): an ×, two strokes in the icons'
/// stroke (the critique, 2026-11-05: the panels had no close of their
/// own; only the strip's key or Esc closed them).
pub struct CloseIcon {
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for CloseIcon {
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
        let s = self.scale;
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let line = Stroke::default()
            .with_color(self.color)
            .with_width(STROKE * s)
            .with_line_cap(canvas::LineCap::Round);
        let r = 3.6 * s;
        frame.stroke(&Path::line(Point::new(c.x - r, c.y - r), Point::new(c.x + r, c.y + r)), line);
        frame.stroke(&Path::line(Point::new(c.x + r, c.y - r), Point::new(c.x - r, c.y + r)), line);
        vec![frame.into_geometry()]
    }
}

/// Done (a Quick setup step): a tick in the icons' stroke, a little
/// heavier, as it stands alone in its badge.
pub struct CheckIcon {
    pub color: Color,
    pub scale: f32,
    /// How much of the tick is drawn, 0 to 1, from its foot (a step just
    /// done in Quick draws its tick in, `motion::TICK_DRAW`).
    pub drawn: f32,
}

impl<Message> canvas::Program<Message> for CheckIcon {
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
        let s = self.scale;
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let (a, m, z) = (
            Point::new(c.x - 5.0 * s, c.y + 0.5 * s),
            Point::new(c.x - 1.5 * s, c.y + 4.0 * s),
            Point::new(c.x + 5.5 * s, c.y - 4.0 * s),
        );
        let drawn = self.drawn.clamp(0.0, 1.0);
        if drawn <= 0.0 {
            return vec![frame.into_geometry()];
        }
        // Along the two strokes, by length, as a pen draws it.
        let first = a.distance(m);
        let along = drawn * (first + m.distance(z));
        let lerp = |p: Point, q: Point, k: f32| Point::new(p.x + (q.x - p.x) * k, p.y + (q.y - p.y) * k);
        let tick = Path::new(|b| {
            b.move_to(a);
            if along <= first {
                b.line_to(lerp(a, m, along / first));
            } else {
                b.line_to(m);
                b.line_to(lerp(m, z, (along - first) / m.distance(z)));
            }
        });
        frame.stroke(
            &tick,
            Stroke::default()
                .with_color(self.color)
                .with_width(2.0 * s)
                .with_line_cap(canvas::LineCap::Round)
                .with_line_join(canvas::LineJoin::Round),
        );
        vec![frame.into_geometry()]
    }
}

/// The tick as an element filling its badge.
pub fn check_icon<'a, Message: 'a>(color: Color, scale: f32) -> iced::Element<'a, Message> {
    check_icon_drawn(color, scale, 1.0)
}

/// The tick, `drawn` of it (0 to 1) from its foot.
pub fn check_icon_drawn<'a, Message: 'a>(color: Color, scale: f32, drawn: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(CheckIcon { color, scale, drawn })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The × as an element filling its button.
pub fn close_icon<'a, Message: 'a>(color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(CloseIcon { color, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The help mark as an element filling its button.
pub fn help_icon<'a, Message: 'a>(color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(HelpIcon { color, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The gear as an element filling its button.
pub fn gear_icon<'a, Message: 'a>(color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(GearIcon { color, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The interface's own settings (clock, optical format; the collapsed
/// column's rail): the unit as a small box, its big knob at the right and
/// its status light at the left, lit in the device's state colour (the
/// reviewer: a box with two dots read as a cassette; the rail's separate
/// status dot is this light now, so the unit and its state read as one).
pub struct DeviceIcon {
    pub color: Color,
    /// The status light's colour (`sidebar::device_state`).
    pub light: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for DeviceIcon {
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
        let s = self.scale;
        let (w, h) = (17.0 * s, 12.0 * s);
        let o = Point::new((bounds.width - w) / 2.0, (bounds.height - h) / 2.0);
        let line = Stroke::default().with_color(self.color).with_width(STROKE * s);
        frame.stroke(
            &Path::new(|p| p.rounded_rectangle(o, iced::Size::new(w, h), (2.5 * s).into())),
            line,
        );
        // The big knob, as on the unit, most of its height.
        frame.stroke(&Path::circle(Point::new(o.x + w - 5.5 * s, o.y + h / 2.0), 3.4 * s), line);
        frame.fill(&Path::circle(Point::new(o.x + w - 5.5 * s, o.y + h / 2.0), 1.0 * s), self.color);
        // The status light.
        frame.fill(&Path::circle(Point::new(o.x + 4.0 * s, o.y + h / 2.0), 1.8 * s), self.light);
        vec![frame.into_geometry()]
    }
}

/// The device icon as an element filling its button, its light in `light`.
pub fn device_icon<'a, Message: 'a>(color: Color, light: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(DeviceIcon { color, light, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The strip's narrow/wide key, as TotalMix draws it (the owner,
/// 2026-11-06: "I like the switch for collapse better in total mix ... It
/// looks more like button"): a solid triangle on the key, pointing left on
/// a full strip (make it narrow) and right on a narrow one (make it wide).
/// It was two chevrons closing on a bar until then.
pub struct CollapseIcon {
    /// Points left: the strip narrows.
    pub narrow: bool,
    pub color: Color,
    pub scale: f32,
    /// The triangle's height and depth at scale 1.
    pub size: (f32, f32),
}

impl<Message> canvas::Program<Message> for CollapseIcon {
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
        let s = self.scale;
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let (h, d) = (self.size.0 * s / 2.0, self.size.1 * s / 2.0);
        let dir = if self.narrow { -1.0 } else { 1.0 };
        let triangle = |dy: f32| {
            Path::new(|p| {
                p.move_to(Point::new(cx + dir * d, cy + dy));
                p.line_to(Point::new(cx - dir * d, cy - h + dy));
                p.line_to(Point::new(cx - dir * d, cy + h + dy));
                p.close();
            })
        };
        frame.fill(&triangle(0.0), self.color);
        vec![frame.into_geometry()]
    }
}

/// The narrow switch's arrow on its cap, 8px tall and 5px deep, lit in
/// `color` (the lamp's: softly while the strip is open, full once folded;
/// the owner's pick from mockups).
pub fn edge_arrow<'a, Message: 'a>(narrow: bool, color: Color, scale: f32) -> iced::Element<'a, Message> {
    // A size up from 8x5 (the owner, 2026-11-10: "hard to see the button").
    iced::widget::canvas(CollapseIcon { narrow, color, scale, size: (10.0, 7.0) })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}


/// A mixer floor's open/closed mark while one row shows at a time: one
/// chevron in the collapse icon's stroke, pointing down on the open floor
/// and right on a folded one, so the floors read as one accordion.
pub struct FloorChevron {
    pub open: bool,
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for FloorChevron {
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
        let s = self.scale;
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let (half, depth) = (FLOOR_CHEVRON_HALF * s, FLOOR_CHEVRON_DEPTH * s);
        let stroke = Stroke::default()
            .with_color(self.color)
            .with_width(STROKE * s)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        let points = if self.open {
            [
                Point::new(cx - half, cy - depth / 2.0),
                Point::new(cx, cy + depth / 2.0),
                Point::new(cx + half, cy - depth / 2.0),
            ]
        } else {
            [
                Point::new(cx - depth / 2.0, cy - half),
                Point::new(cx + depth / 2.0, cy),
                Point::new(cx - depth / 2.0, cy + half),
            ]
        };
        frame.stroke(
            &Path::new(|p| {
                p.move_to(points[0]);
                p.line_to(points[1]);
                p.line_to(points[2]);
            }),
            stroke,
        );
        vec![frame.into_geometry()]
    }
}

/// Half the floor chevron's width across its arms, and its depth, at scale 1.
const FLOOR_CHEVRON_HALF: f32 = 4.5;
const FLOOR_CHEVRON_DEPTH: f32 = 4.5;
/// The floor chevron's box at scale 1.
pub const FLOOR_CHEVRON_BOX: f32 = 14.0;

/// The floor chevron as a fixed-size element.
pub fn floor_chevron<'a, Message: 'a>(open: bool, color: Color, scale: f32) -> iced::Element<'a, Message> {
    let side = FLOOR_CHEVRON_BOX * scale;
    iced::widget::canvas(FloorChevron { open, color, scale })
        .width(iced::Length::Fixed(side))
        .height(iced::Length::Fixed(side))
        .into()
}

/// The Lock User Interface padlock in the top bar: a filled body under a
/// shackle, closed while the mix is locked, swung open to one side while
/// it is not. Drawn, in the stroke of the other icons, not a font glyph.
pub struct LockIcon {
    pub locked: bool,
    pub color: Color,
    pub scale: f32,
    /// How far it's shaken sideways (px at scale 1): the lock holding a
    /// change back (`motion::lock_nudge`).
    pub nudge: f32,
}

impl<Message> canvas::Program<Message> for LockIcon {
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
        let s = self.scale;
        let (cx, cy) = (bounds.width / 2.0 + self.nudge * s, bounds.height / 2.0);
        // The body: 10 x 7, placed so the closed lock (shackle and body,
        // about 11.7 tall) sits centred in its button.
        let (body_w, body_h) = (10.0 * s, 7.0 * s);
        let body_top = cy - 1.2 * s;
        frame.fill(
            &Path::rounded_rectangle(
                Point::new(cx - body_w / 2.0, body_top),
                iced::Size::new(body_w, body_h),
                (1.5 * s).into(),
            ),
            self.color,
        );
        // The shackle: an arch 6 wide over the body. Open, it rises and
        // swings right: the left leg still reaches into the body, the
        // right one stands clear above it.
        let r = 3.0 * s;
        let lift = if self.locked { 0.0 } else { 2.5 * s };
        let shift = if self.locked { 0.0 } else { 2.0 * s };
        let arch_y = body_top - 1.0 * s - lift;
        let stroke = Stroke::default()
            .with_color(self.color)
            .with_width(STROKE * s)
            .with_line_cap(canvas::LineCap::Round);
        frame.stroke(
            &Path::new(|p| {
                let (l, rgt) = (cx - r + shift, cx + r + shift);
                p.move_to(Point::new(l, body_top));
                p.line_to(Point::new(l, arch_y));
                p.arc(canvas::path::Arc {
                    center: Point::new(cx + shift, arch_y),
                    radius: r,
                    start_angle: iced::Radians(std::f32::consts::PI),
                    end_angle: iced::Radians(2.0 * std::f32::consts::PI),
                });
                p.line_to(Point::new(rgt, body_top - lift));
            }),
            stroke,
        );
        vec![frame.into_geometry()]
    }
}

/// The padlock as an element filling its button.
pub fn lock_icon<'a, Message: 'a>(locked: bool, color: Color, scale: f32, nudge: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(LockIcon { locked, color, scale, nudge })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

/// The UI sounds toggle in the top bar: a speaker, with two sound waves
/// while sounds are on and a slash across it while they are off. In the
/// stroke of the other icons.
pub struct NotesIcon {
    pub on: bool,
    pub color: Color,
    pub scale: f32,
}

/// Two beamed notes, struck through when off: Audra's own sounds (the top
/// bar), not a speaker, which in a mixer means the monitors.
impl<Message> canvas::Program<Message> for NotesIcon {
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
        let s = self.scale;
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let stroke = Stroke::default()
            .with_color(self.color)
            .with_width(STROKE * s)
            .with_line_cap(canvas::LineCap::Round);
        // Heads, low left and a step higher right, and their stems.
        let heads = [Point::new(cx - 4.0 * s, cy + 4.5 * s), Point::new(cx + 3.5 * s, cy + 3.0 * s)];
        for h in heads {
            frame.fill(&Path::circle(h, 2.4 * s), self.color);
            frame.stroke(&Path::line(Point::new(h.x + 2.1 * s, h.y), Point::new(h.x + 2.1 * s, h.y - 9.0 * s)), stroke);
        }
        // The beam joining the stems' tops.
        frame.stroke(
            &Path::line(
                Point::new(heads[0].x + 2.1 * s, heads[0].y - 9.0 * s),
                Point::new(heads[1].x + 2.1 * s, heads[1].y - 9.0 * s),
            ),
            Stroke::default().with_color(self.color).with_width(2.6 * s).with_line_cap(canvas::LineCap::Butt),
        );
        if !self.on {
            frame.stroke(&Path::line(Point::new(cx - 7.5 * s, cy - 7.0 * s), Point::new(cx + 8.0 * s, cy + 7.5 * s)), stroke);
        }
        vec![frame.into_geometry()]
    }
}

/// Audra's sounds' notes as an element filling its button.
pub fn notes_icon<'a, Message: 'a>(on: bool, color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(NotesIcon { on, color, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}


/// The map of where sound goes (`flow`), small: two dots at the left
/// joined by two curves into one at the right, as a mix's map draws its
/// sources coming in. Beside the placard's "Fed by …" line, so it reads as
/// something to press (the owner's yes, after critique #25: the line
/// looked like plain text). Drawn fanning in, matching the words (the
/// owner's pick after critique #26: fanning out it read as "share").
pub struct MapIcon {
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for MapIcon {
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
        let s = self.scale;
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let stroke = Stroke::default()
            .with_color(self.color)
            .with_width(1.3 * s)
            .with_line_cap(canvas::LineCap::Round);
        let hub = Point::new(cx + 4.5 * s, cy);
        let ends = [Point::new(cx - 4.5 * s, cy - 3.5 * s), Point::new(cx - 4.5 * s, cy + 3.5 * s)];
        for end in ends {
            // A curve with flat ends, as the map's paths are.
            let mid = (hub.x + end.x) / 2.0;
            let path = Path::new(|b| {
                b.move_to(hub);
                b.bezier_curve_to(Point::new(mid, hub.y), Point::new(mid, end.y), end);
            });
            frame.stroke(&path, stroke);
            frame.fill(&Path::circle(end, 1.6 * s), self.color);
        }
        frame.fill(&Path::circle(hub, 2.0 * s), self.color);
        vec![frame.into_geometry()]
    }
}

pub fn map_icon<'a, Message: 'a>(color: Color, scale: f32) -> iced::Element<'a, Message> {
    let d = 14.0 * scale;
    iced::widget::canvas(MapIcon { color, scale })
        .width(iced::Length::Fixed(d))
        .height(iced::Length::Fixed(d))
        .into()
}

/// A scroll location marker's end arrow (manual 21.5.4): a chevron
/// pointing at a bar, "all the way to this end", left for the first strip
/// and right for the last. In the stroke of the other icons.
pub struct ScrollEndIcon {
    pub to_end: bool,
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for ScrollEndIcon {
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
        let s = self.scale;
        let (cx, cy) = (bounds.width / 2.0, bounds.height / 2.0);
        let side = if self.to_end { 1.0 } else { -1.0 };
        let stroke = Stroke::default()
            .with_color(self.color)
            .with_width(STROKE * s)
            .with_line_cap(canvas::LineCap::Round)
            .with_line_join(canvas::LineJoin::Round);
        let bar_x = cx + side * 3.5 * s;
        frame.stroke(
            &Path::line(Point::new(bar_x, cy - 4.0 * s), Point::new(bar_x, cy + 4.0 * s)),
            stroke,
        );
        let tip = cx + side * 1.0 * s;
        let back = tip - side * 3.5 * s;
        frame.stroke(
            &Path::new(|p| {
                p.move_to(Point::new(back, cy - 3.5 * s));
                p.line_to(Point::new(tip, cy));
                p.line_to(Point::new(back, cy + 3.5 * s));
            }),
            stroke,
        );
        vec![frame.into_geometry()]
    }
}

/// The end arrow as an element filling its button.
pub fn scroll_end_icon<'a, Message: 'a>(to_end: bool, color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(ScrollEndIcon { to_end, color, scale })
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .into()
}

// The wrench fits the strip's side keys (18px square at scale 1) with
// room round it.
const _: () = assert!(WRENCH_BOX <= 18.0 - 4.0);

/// An EQ band's filter shape, drawn (the owner's reference shows the shape
/// where Audra wrote its name): a flat line for Off, a bell, a low shelf
/// (raised at the left), a high shelf (raised at the right).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterShape {
    Off,
    Bell,
    LowShelf,
    HighShelf,
}

pub struct FilterIcon {
    pub shape: FilterShape,
    pub color: Color,
    pub scale: f32,
}

impl<Message> canvas::Program<Message> for FilterIcon {
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
        let (w, h) = (bounds.width, bounds.height);
        let (left, right) = (1.5 * self.scale, w - 1.5 * self.scale);
        let (top, mid) = (2.0 * self.scale, h * 0.62);
        let x = |k: f32| left + (right - left) * k;
        let path = Path::new(|b| match self.shape {
            FilterShape::Off => {
                b.move_to(Point::new(left, mid));
                b.line_to(Point::new(right, mid));
            }
            FilterShape::Bell => {
                b.move_to(Point::new(left, mid));
                b.line_to(Point::new(x(0.22), mid));
                b.bezier_curve_to(Point::new(x(0.38), mid), Point::new(x(0.42), top), Point::new(x(0.5), top));
                b.bezier_curve_to(Point::new(x(0.58), top), Point::new(x(0.62), mid), Point::new(x(0.78), mid));
                b.line_to(Point::new(right, mid));
            }
            FilterShape::LowShelf => {
                b.move_to(Point::new(left, top));
                b.line_to(Point::new(x(0.3), top));
                b.bezier_curve_to(Point::new(x(0.5), top), Point::new(x(0.5), mid), Point::new(x(0.7), mid));
                b.line_to(Point::new(right, mid));
            }
            FilterShape::HighShelf => {
                b.move_to(Point::new(left, mid));
                b.line_to(Point::new(x(0.3), mid));
                b.bezier_curve_to(Point::new(x(0.5), mid), Point::new(x(0.5), top), Point::new(x(0.7), top));
                b.line_to(Point::new(right, top));
            }
        });
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(self.color)
                .with_width(STROKE * self.scale)
                .with_line_cap(canvas::LineCap::Round)
                .with_line_join(canvas::LineJoin::Round),
        );
        vec![frame.into_geometry()]
    }
}

/// A `FilterIcon` as an element, 24 x 12 at scale 1.
pub fn filter_icon<'a, Message: 'a>(shape: FilterShape, color: Color, scale: f32) -> iced::Element<'a, Message> {
    iced::widget::canvas(FilterIcon { shape, color, scale })
        .width(iced::Length::Fixed(24.0 * scale))
        .height(iced::Length::Fixed(12.0 * scale))
        .into()
}
