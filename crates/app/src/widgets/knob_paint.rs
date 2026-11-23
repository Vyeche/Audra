//! The knobs' round body drawn on the CPU with exact edge smoothing, as a
//! picture the canvas then places (the owner, 2026-11-14, twice: the light
//! theme's knobs "look pixilated", "still look bad"). The GPU smooths a
//! canvas's edges at four samples a pixel, so every strong round edge on a
//! 42 px cap stepped; the same knob drawn by tiny-skia measured 27% smoother
//! round its rings (`tools/checks/totalMix-knob-ring.py`, 4.34 to 3.15).
//!
//! The drawing itself is written once, against `Pen`: `canvas::Frame` is one
//! (the vector path, for anything left on the canvas) and `Raster` the other
//! (a tiny-skia pixmap at twice the size, so it also holds up when zoomed
//! and is boxed down to one pixel exactly at 100%). Pictures are cached by
//! everything that changes them, so a still knob is never redrawn.

use iced::widget::canvas::{self, Path, Stroke};
use iced::{Color, Point};
use std::cell::RefCell;
use std::collections::HashMap;

/// What a shape is filled or stroked with.
pub enum Ink {
    Solid(Color),
    /// A linear gradient from one point to another, its stops (0 to 1).
    Linear(Point, Point, Vec<(f32, Color)>),
}

/// The few strokes a knob's body is made of.
pub trait Pen {
    fn disc(&mut self, c: Point, r: f32, ink: Ink);
    fn ring(&mut self, c: Point, r: f32, width: f32, ink: Ink);
    fn segments(&mut self, segs: &[(Point, Point)], width: f32, color: Color);
    /// An arc from `a0` to `a1` (radians, clockwise from +x as the canvas
    /// draws them), round-capped.
    fn arc(&mut self, c: Point, r: f32, a0: f32, a1: f32, width: f32, color: Color);
    /// A straight line, round-capped.
    fn line(&mut self, a: Point, z: Point, width: f32, color: Color);
    /// A rounded rectangle, filled, or stroked along its outline.
    fn rrect(&mut self, r: iced::Rectangle, radius: f32, ink: Ink);
    fn rrect_stroke(&mut self, r: iced::Rectangle, radius: f32, width: f32, ink: Ink);
}

fn canvas_linear(p0: Point, p1: Point, stops: &[(f32, Color)]) -> canvas::gradient::Linear {
    stops.iter().fold(canvas::gradient::Linear::new(p0, p1), |g, (at, c)| g.add_stop(*at, *c))
}

impl Pen for canvas::Frame {
    fn disc(&mut self, c: Point, r: f32, ink: Ink) {
        let path = Path::circle(c, r);
        match ink {
            Ink::Solid(color) => self.fill(&path, color),
            Ink::Linear(p0, p1, stops) => self.fill(&path, canvas_linear(p0, p1, &stops)),
        }
    }
    fn ring(&mut self, c: Point, r: f32, width: f32, ink: Ink) {
        let path = Path::circle(c, r);
        let stroke = match ink {
            Ink::Solid(color) => Stroke::default().with_color(color),
            Ink::Linear(p0, p1, stops) => Stroke { style: canvas::Style::Gradient(canvas_linear(p0, p1, &stops).into()), ..Stroke::default() },
        };
        self.stroke(&path, stroke.with_width(width));
    }
    fn segments(&mut self, segs: &[(Point, Point)], width: f32, color: Color) {
        let path = Path::new(|b| {
            for (a, z) in segs {
                b.move_to(*a);
                b.line_to(*z);
            }
        });
        self.stroke(&path, Stroke::default().with_color(color).with_width(width));
    }
    fn arc(&mut self, c: Point, r: f32, a0: f32, a1: f32, width: f32, color: Color) {
        let path = Path::new(|b| {
            b.arc(canvas::path::Arc { center: c, radius: r, start_angle: iced::Radians(a0), end_angle: iced::Radians(a1) })
        });
        self.stroke(&path, Stroke::default().with_line_cap(canvas::LineCap::Round).with_color(color).with_width(width));
    }
    fn line(&mut self, a: Point, z: Point, width: f32, color: Color) {
        self.stroke(&Path::line(a, z), Stroke::default().with_line_cap(canvas::LineCap::Round).with_color(color).with_width(width));
    }
    fn rrect(&mut self, r: iced::Rectangle, radius: f32, ink: Ink) {
        let path = Path::new(|b| b.rounded_rectangle(r.position(), r.size(), radius.max(0.0).into()));
        match ink {
            Ink::Solid(color) => self.fill(&path, color),
            Ink::Linear(p0, p1, stops) => self.fill(&path, canvas_linear(p0, p1, &stops)),
        }
    }
    fn rrect_stroke(&mut self, r: iced::Rectangle, radius: f32, width: f32, ink: Ink) {
        let path = Path::new(|b| b.rounded_rectangle(r.position(), r.size(), radius.max(0.0).into()));
        let stroke = match ink {
            Ink::Solid(color) => Stroke::default().with_color(color),
            Ink::Linear(p0, p1, stops) => Stroke { style: canvas::Style::Gradient(canvas_linear(p0, p1, &stops).into()), ..Stroke::default() },
        };
        self.stroke(&path, stroke.with_width(width));
    }
}

/// A rounded rectangle's outline for tiny-skia (corner arcs as cubics).
fn ts_rrect(r: iced::Rectangle, radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x, r.y, r.width.max(0.0), r.height.max(0.0));
    let rad = radius.max(0.0).min(w / 2.0).min(h / 2.0);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    // The cubic's handle for a quarter circle.
    let k = 0.552_284_8 * rad;
    let mut b = tiny_skia::PathBuilder::new();
    b.move_to(x + rad, y);
    b.line_to(x + w - rad, y);
    b.cubic_to(x + w - rad + k, y, x + w, y + rad - k, x + w, y + rad);
    b.line_to(x + w, y + h - rad);
    b.cubic_to(x + w, y + h - rad + k, x + w - rad + k, y + h, x + w - rad, y + h);
    b.line_to(x + rad, y + h);
    b.cubic_to(x + rad - k, y + h, x, y + h - rad + k, x, y + h - rad);
    b.line_to(x, y + rad);
    b.cubic_to(x, y + rad - k, x + rad - k, y, x + rad, y);
    b.close();
    b.finish()
}

/// A tiny-skia pixmap drawn in the knob frame's own coordinates (`origin`
/// its top left there), `SS` pixels to one.
pub struct Raster {
    pub pixmap: tiny_skia::Pixmap,
    transform: tiny_skia::Transform,
}

/// Pixels drawn per screen pixel at 100%.
pub const SS: f32 = 2.0;

fn ts_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(c.r.clamp(0.0, 1.0), c.g.clamp(0.0, 1.0), c.b.clamp(0.0, 1.0), c.a.clamp(0.0, 1.0))
        .unwrap_or(tiny_skia::Color::TRANSPARENT)
}

fn ts_paint(ink: &Ink) -> Option<tiny_skia::Paint<'static>> {
    let shader = match ink {
        Ink::Solid(c) => tiny_skia::Shader::SolidColor(ts_color(*c)),
        Ink::Linear(p0, p1, stops) => tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(p0.x, p0.y),
            tiny_skia::Point::from_xy(p1.x, p1.y),
            stops.iter().map(|(at, c)| tiny_skia::GradientStop::new(*at, ts_color(*c))).collect(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::Transform::identity(),
        )?,
    };
    Some(tiny_skia::Paint { shader, anti_alias: true, ..tiny_skia::Paint::default() })
}

impl Raster {
    /// A pixmap `size` screen pixels square, its top left at `origin` in the
    /// frame's coordinates.
    pub fn new(origin: Point, size: f32) -> Option<Self> {
        Self::sized(origin, iced::Size::new(size, size))
    }

    /// A pixmap of `size` screen pixels, its top left at `origin`.
    pub fn sized(origin: Point, size: iced::Size) -> Option<Self> {
        let pixmap = tiny_skia::Pixmap::new((size.width * SS).ceil() as u32, (size.height * SS).ceil() as u32)?;
        let transform = tiny_skia::Transform::from_row(SS, 0.0, 0.0, SS, -origin.x * SS, -origin.y * SS);
        Some(Raster { pixmap, transform })
    }

    fn stroke_path(&mut self, path: &tiny_skia::Path, paint: &tiny_skia::Paint, width: f32, round: bool) {
        let stroke = tiny_skia::Stroke {
            width,
            line_cap: if round { tiny_skia::LineCap::Round } else { tiny_skia::LineCap::Butt },
            ..tiny_skia::Stroke::default()
        };
        self.pixmap.stroke_path(path, paint, &stroke, self.transform, None);
    }

    /// The pixels, straight (not premultiplied) RGBA, as iced takes them.
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = self.pixmap.data().to_vec();
        for px in out.as_chunks_mut::<4>().0 {
            let a = px[3] as u32;
            if a > 0 && a < 255 {
                for c in &mut px[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
}

impl Pen for Raster {
    fn disc(&mut self, c: Point, r: f32, ink: Ink) {
        if r <= 0.0 {
            return;
        }
        let (Some(path), Some(paint)) = (tiny_skia::PathBuilder::from_circle(c.x, c.y, r), ts_paint(&ink)) else { return };
        self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, self.transform, None);
    }
    fn ring(&mut self, c: Point, r: f32, width: f32, ink: Ink) {
        if r <= 0.0 {
            return;
        }
        let (Some(path), Some(paint)) = (tiny_skia::PathBuilder::from_circle(c.x, c.y, r), ts_paint(&ink)) else { return };
        self.stroke_path(&path, &paint, width, false);
    }
    fn segments(&mut self, segs: &[(Point, Point)], width: f32, color: Color) {
        let mut b = tiny_skia::PathBuilder::new();
        for (a, z) in segs {
            b.move_to(a.x, a.y);
            b.line_to(z.x, z.y);
        }
        let (Some(path), Some(paint)) = (b.finish(), ts_paint(&Ink::Solid(color))) else { return };
        self.stroke_path(&path, &paint, width, false);
    }
    fn arc(&mut self, c: Point, r: f32, a0: f32, a1: f32, width: f32, color: Color) {
        // As line segments, fine enough to be round at any size here.
        let steps = (((a1 - a0).abs() * r) / 1.5).ceil().max(4.0) as usize;
        let mut b = tiny_skia::PathBuilder::new();
        for i in 0..=steps {
            let a = a0 + (a1 - a0) * i as f32 / steps as f32;
            let (x, y) = (c.x + r * a.cos(), c.y + r * a.sin());
            if i == 0 {
                b.move_to(x, y);
            } else {
                b.line_to(x, y);
            }
        }
        let (Some(path), Some(paint)) = (b.finish(), ts_paint(&Ink::Solid(color))) else { return };
        self.stroke_path(&path, &paint, width, true);
    }
    fn line(&mut self, a: Point, z: Point, width: f32, color: Color) {
        let mut b = tiny_skia::PathBuilder::new();
        b.move_to(a.x, a.y);
        b.line_to(z.x, z.y);
        let (Some(path), Some(paint)) = (b.finish(), ts_paint(&Ink::Solid(color))) else { return };
        self.stroke_path(&path, &paint, width, true);
    }
    fn rrect(&mut self, r: iced::Rectangle, radius: f32, ink: Ink) {
        let (Some(path), Some(paint)) = (ts_rrect(r, radius), ts_paint(&ink)) else { return };
        self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, self.transform, None);
    }
    fn rrect_stroke(&mut self, r: iced::Rectangle, radius: f32, width: f32, ink: Ink) {
        let (Some(path), Some(paint)) = (ts_rrect(r, radius), ts_paint(&ink)) else { return };
        self.stroke_path(&path, &paint, width, false);
    }
}

thread_local! {
    /// Two generations: when the new one fills, it becomes the old one, so
    /// a knob being turned (a new picture each step) never throws out the
    /// still ones' pictures all at once.
    static PICTURES: RefCell<(HashMap<u64, iced::widget::image::Handle>, HashMap<u64, iced::widget::image::Handle>)> =
        RefCell::new((HashMap::new(), HashMap::new()));
}

const GENERATION: usize = 512;

/// Draws a picture of `bounds` onto `frame` (`body`'s rectangular kin):
/// `paint` draws in the frame's coordinates as given its rectangle at
/// `local` (the same size). Pictures sit above the canvas's shapes, so
/// only what nothing is drawn over belongs in one.
pub fn picture(frame: &mut canvas::Frame, bounds: iced::Rectangle, key: u64, paint: impl FnOnce(&mut dyn Pen, iced::Rectangle)) {
    let origin = Point::new(bounds.x.floor(), bounds.y.floor());
    let q = |v: f32| (v * 4.0).round() / 4.0;
    let local = iced::Rectangle::new(Point::new(q(bounds.x - origin.x), q(bounds.y - origin.y)), bounds.size());
    let size = iced::Size::new((local.x + bounds.width + 1.0).ceil(), (local.y + bounds.height + 1.0).ceil());
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (key, local.x.to_bits(), local.y.to_bits(), bounds.width.to_bits(), bounds.height.to_bits(), 1u8).hash(&mut h);
        h.finish()
    };
    let handle = match cached(key) {
        Some(h) => h,
        None => {
            let Some(mut raster) = Raster::sized(Point::ORIGIN, size) else {
                paint(frame, bounds);
                return;
            };
            paint(&mut raster, local);
            let handle = iced::widget::image::Handle::from_rgba(raster.pixmap.width(), raster.pixmap.height(), raster.rgba());
            keep(key, &handle);
            handle
        }
    };
    let mut image = iced::advanced::image::Image::new(handle);
    image.filter_method = iced::advanced::image::FilterMethod::Linear;
    image.snap = false;
    frame.draw_image(iced::Rectangle::new(origin, size), image);
}

fn cached(key: u64) -> Option<iced::widget::image::Handle> {
    PICTURES.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(h) = p.0.get(&key) {
            return Some(h.clone());
        }
        // From the old generation: kept, in the new one.
        let h = p.1.remove(&key)?;
        p.0.insert(key, h.clone());
        Some(h)
    })
}

fn keep(key: u64, handle: &iced::widget::image::Handle) {
    PICTURES.with(|p| {
        let mut p = p.borrow_mut();
        if p.0.len() >= GENERATION {
            let new = std::mem::take(&mut p.0);
            p.1 = new;
        }
        p.0.insert(key, handle.clone());
    });
}

/// Draws a knob body onto `frame`: `paint` draws it (with any `Pen`) in
/// coordinates where its centre is `c`; `reach` is how far from the centre
/// it may draw (its shadow included). `key` must change whenever what
/// `paint` draws would (colours, sizes, the ribs' turn, the theme). The
/// picture is drawn once per key and kept.
pub fn body(frame: &mut canvas::Frame, c: Point, reach: f32, key: u64, paint: impl FnOnce(&mut dyn Pen, Point)) {
    // Whole pixels for the picture's corner, so at 100% each screen pixel
    // is exactly four of the picture's (`SS` 2): the centre's fraction, in
    // quarter pixels, goes into the key.
    let origin = Point::new((c.x - reach).floor(), (c.y - reach).floor());
    let local = Point::new(c.x - origin.x, c.y - origin.y);
    let q = |v: f32| (v * 4.0).round() / 4.0;
    let local = Point::new(q(local.x), q(local.y));
    let size = (2.0 * reach + 1.0).ceil();
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        (local.x.to_bits(), local.y.to_bits(), size.to_bits()).hash(&mut h);
        h.finish()
    };
    let cached = PICTURES.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(h) = p.0.get(&key) {
            return Some(h.clone());
        }
        // From the old generation: kept, in the new one.
        let h = p.1.remove(&key)?;
        p.0.insert(key, h.clone());
        Some(h)
    });
    let handle = match cached {
        Some(h) => h,
        None => {
            let Some(mut raster) = Raster::new(Point::ORIGIN, size) else {
                // No pixmap (a zero size): the vector path, as before.
                paint(frame, c);
                return;
            };
            paint(&mut raster, local);
            let px = raster.pixmap.width();
            let handle = iced::widget::image::Handle::from_rgba(px, px, raster.rgba());
            PICTURES.with(|p| {
                let mut p = p.borrow_mut();
                if p.0.len() >= GENERATION {
                    let new = std::mem::take(&mut p.0);
                    p.1 = new;
                }
                p.0.insert(key, handle.clone());
            });
            handle
        }
    };
    let mut image = iced::advanced::image::Image::new(handle);
    image.filter_method = iced::advanced::image::FilterMethod::Linear;
    image.snap = false;
    frame.draw_image(iced::Rectangle::new(origin, iced::Size::new(size, size)), image);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raster_disc_is_round_and_smooth_edged() {
        let mut r = Raster::new(Point::ORIGIN, 20.0).unwrap();
        r.disc(Point::new(10.0, 10.0), 8.0, Ink::Solid(Color::BLACK));
        let rgba = r.rgba();
        let w = r.pixmap.width() as usize;
        let alpha = |x: usize, y: usize| rgba[(y * w + x) * 4 + 3];
        assert_eq!(alpha(20, 20), 255, "solid in the middle");
        assert_eq!(alpha(1, 1), 0, "clear outside");
        // Somewhere along the edge, partly covered: smoothed, not stepped.
        let edge: Vec<u8> = (0..w).map(|x| alpha(x, 9)).filter(|a| *a > 0 && *a < 255).collect();
        assert!(!edge.is_empty(), "the edge is smoothed");
    }
}
