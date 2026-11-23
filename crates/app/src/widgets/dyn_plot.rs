//! The Dynamics panel's amplitude display, as RME TotalMix FX draws it: a
//! dark square with a grid, the input->output curve, a white dot for the
//! input level now, and light-blue marks for the gain reduction now.
//!
//! Display only: no mouse interaction, no state. The curve is given as 61
//! output values (dB) for inputs -60..0 dB in 1 dB steps; segments below the
//! expander threshold read green, above the compressor threshold red, and
//! the meter to the right shows how far it turns down now.

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Renderer, Size, Theme};

use crate::theme;
use crate::widgets::eq_plot::{SCREEN, SCREEN_INK};

/// What the display shows; everything in dB.
pub struct DynPlot {
    /// Output level for each input level from -60 to 0 dB in 1 dB steps (61 values).
    pub curve: Vec<f32>,
    /// Under this input level the curve is drawn green (the expander), when on.
    pub exp_threshold: Option<f32>,
    /// Over this input level the curve is drawn red (the compressor), when on.
    pub comp_threshold: Option<f32>,
    /// The input level now (white dot on the curve at this input level), when known.
    pub dot: Option<f32>,
    /// How far it turns down now (0 or less); drawn as light-blue marks.
    pub reduction_db: f32,
    /// Auto Level's ceiling (minus its headroom), when on: a dashed line
    /// the output never passes.
    pub ceiling: Option<f32>,
    /// The square's side, in logical pixels (already scaled).
    pub side: f32,
    pub scale: f32,
}

pub fn dyn_plot<'a, Message: 'a>(plot: DynPlot) -> iced::Element<'a, Message> {
    let side = plot.side;
    let scale = plot.scale;
    canvas::Canvas::new(plot)
        .width(iced::Length::Fixed(side + 10.0 * scale))
        .height(iced::Length::Fixed(side))
        .into()
}

impl<Message> canvas::Program<Message> for DynPlot {
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
        let scale = self.scale;

        // The square: input -60..0 dB left to right, output -60..0 dB bottom
        // to top. Values outside are clamped to the edges.
        let side = self.side.max(1.0);
        let sq = Rectangle::new(Point::new(0.0, 0.0), Size::new(side, side));

        frame.fill_rectangle(Point::new(sq.x, sq.y), Size::new(sq.width, sq.height), SCREEN);

        // Grid: lines every 12 dB both ways (-48, -36, -24, -12), plus the
        // unity diagonal as a faint line.
        let grid_color = theme::blend(SCREEN, SCREEN_INK, 0.12);
        let grid = Stroke::default().with_color(grid_color).with_width(scale);
        for db in [-48.0, -36.0, -24.0, -12.0] {
            let x = db_to_x(db, sq);
            frame.stroke(&Path::line(Point::new(x, sq.y), Point::new(x, sq.y + sq.height)), grid);
            let y = db_to_y(db, sq);
            frame.stroke(&Path::line(Point::new(sq.x, y), Point::new(sq.x + sq.width, y)), grid);
        }
        // The unity diagonal (input == output).
        frame.stroke(
            &Path::line(Point::new(sq.x, sq.y + sq.height), Point::new(sq.x + sq.width, sq.y)),
            grid,
        );

        // A 1px border around the square.
        let border_color = theme::blend(SCREEN, SCREEN_INK, 0.25);
        frame.stroke(
            &Path::rectangle(Point::new(sq.x, sq.y), Size::new(sq.width, sq.height)),
            Stroke::default().with_color(border_color).with_width(scale),
        );

        // The curve as a polyline through the 61 points, colored by segment.
        let curve_width = 2.0 * scale;
        let n = self.curve.len();
        if n >= 2 {
            for i in 0..n - 1 {
                let in_a = -60.0 + i as f32;
                let in_b = -60.0 + (i + 1) as f32;
                let out_a = self.curve[i].clamp(-60.0, 0.0);
                let out_b = self.curve[i + 1].clamp(-60.0, 0.0);

                let color = segment_color(in_a, self.exp_threshold, self.comp_threshold);

                frame.stroke(
                    &Path::line(Point::new(db_to_x(in_a, sq), db_to_y(out_a, sq)), Point::new(db_to_x(in_b, sq), db_to_y(out_b, sq))),
                    Stroke::default().with_color(color).with_width(curve_width),
                );
            }
        }

        // Auto Level's ceiling, dashed in the turn-down's light blue: the
        // output never goes over it.
        if let Some(c) = self.ceiling {
            let y = db_to_y(c.clamp(-60.0, 0.0), sq);
            let dash = 4.0 * scale;
            let mut x = sq.x;
            let blue = Color::from_rgb8(0x6c, 0xc4, 0xf0);
            while x < sq.x + sq.width {
                let to = (x + dash).min(sq.x + sq.width);
                frame.stroke(&Path::line(Point::new(x, y), Point::new(to, y)), Stroke::default().with_color(blue).with_width(scale));
                x += dash * 2.0;
            }
        }

        // The dot: white circle on the curve at the current input level.
        if let Some(x_db) = self.dot {
            let x_clamped = x_db.clamp(-60.0, 0.0);
            let y_db = interp_curve(&self.curve, x_clamped).clamp(-60.0, 0.0);
            let center = Point::new(db_to_x(x_clamped, sq), db_to_y(y_db, sq));
            let radius = 3.5 * scale;

            // White on the dark themes, as TotalMix draws it; the theme's
            // ink on a light one, where white would vanish.
            frame.fill(&Path::circle(center, radius), SCREEN_INK);
            frame.stroke(
                &Path::circle(center, radius),
                Stroke::default().with_color(SCREEN).with_width(scale),
            );
        }

        // The meter: a thin bar right of the square.
        let gap = 4.0 * scale;
        let meter_w = 6.0 * scale;
        let meter_x = sq.x + sq.width + gap;
        let meter_rect = Rectangle::new(Point::new(meter_x, sq.y), Size::new(meter_w, sq.height));

        frame.fill_rectangle(
            Point::new(meter_rect.x, meter_rect.y),
            Size::new(meter_rect.width, meter_rect.height),
            SCREEN,
        );
        frame.stroke(
            &Path::rectangle(Point::new(meter_rect.x, meter_rect.y), Size::new(meter_rect.width, meter_rect.height)),
            Stroke::default().with_color(border_color).with_width(scale),
        );

        // Light-blue marks for gain reduction: one 2*scale-high mark every 3 dB.
        let reduction = self.reduction_db.min(0.0);
        if reduction < 0.0 {
            let mark_color = Color::from_rgb8(0x6c, 0xc4, 0xf0);
            let mark_h = 2.0 * scale;
            let db_per_mark = 3.0;
            // The bar spans 24 dB: more is the full bar, never past it.
            let total_marks = ((reduction.abs() / db_per_mark).floor() as i32).min((24.0 / db_per_mark) as i32);

            for m in 0..total_marks {
                // Marks from the top down. The bar spans 0..-24 dB over its height.
                let frac_top = (m as f32 * db_per_mark) / 24.0;
                let y = meter_rect.y + frac_top * meter_rect.height;

                frame.fill_rectangle(
                    Point::new(meter_rect.x, y),
                    Size::new(meter_rect.width, mark_h),
                    mark_color,
                );
            }
        }

        vec![frame.into_geometry()]
    }
}

/// The D panel's trace: the last few seconds of the level (input dB),
/// newest at the right, against the thresholds, with the turn-down hanging
/// light blue from the top (the owner, 2026-11-10: a wave that "moves
/// around when you talk"). Display only.
pub struct DynTrace {
    /// (input level dB, turn-down dB) per reading, oldest first.
    pub points: Vec<(f32, f32)>,
    /// How many readings the width holds.
    pub len: usize,
    pub exp_threshold: Option<f32>,
    pub comp_threshold: Option<f32>,
    /// Auto Level's ceiling, when on.
    pub ceiling: Option<f32>,
    /// Its height, in logical pixels (already scaled); as wide as it's given.
    pub height: f32,
    pub scale: f32,
}

/// The trace as an element.
pub fn dyn_trace<'a, Message: 'a>(trace: DynTrace) -> iced::Element<'a, Message> {
    let h = trace.height;
    canvas::Canvas::new(trace).width(iced::Length::Fill).height(iced::Length::Fixed(h)).into()
}

/// The turn-down the trace's height holds, in dB.
const TRACE_REDUCTION_DB: f32 = 24.0;

impl<Message> canvas::Program<Message> for DynTrace {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let r = Rectangle::new(Point::ORIGIN, bounds.size());
        frame.fill_rectangle(r.position(), r.size(), SCREEN);
        let border = theme::blend(SCREEN, SCREEN_INK, 0.25);
        let y_of = |db: f32| r.height - (db.clamp(-60.0, 0.0) + 60.0) / 60.0 * r.height;
        let n = self.len.max(2);
        let step = r.width / (n - 1) as f32;
        // Newest at the right edge; a short history starts part way in.
        let x0 = r.width - step * (self.points.len().saturating_sub(1)) as f32;
        if self.points.len() >= 2 {
            // The level, filled from the floor.
            let level = Path::new(|b| {
                b.move_to(Point::new(x0, r.height));
                for (i, (db, _)) in self.points.iter().enumerate() {
                    b.line_to(Point::new(x0 + step * i as f32, y_of(*db)));
                }
                b.line_to(Point::new(r.width, r.height));
                b.close();
            });
            frame.fill(&level, Color { a: 0.45, ..SCREEN_INK });
            // The turn-down, light blue from the top.
            let blue = Color::from_rgb8(0x6c, 0xc4, 0xf0);
            if self.points.iter().any(|(_, gr)| *gr < -0.5) {
                let down = Path::new(|b| {
                    b.move_to(Point::new(x0, 0.0));
                    for (i, (_, gr)) in self.points.iter().enumerate() {
                        let d = (-gr).clamp(0.0, TRACE_REDUCTION_DB) / TRACE_REDUCTION_DB * r.height;
                        b.line_to(Point::new(x0 + step * i as f32, d));
                    }
                    b.line_to(Point::new(r.width, 0.0));
                    b.close();
                });
                frame.fill(&down, Color { a: 0.55, ..blue });
            }
        }
        // The thresholds and the ceiling, dashed across.
        // Each dash on a dark edge, so it shows over the level and the
        // turn-down as well as on the screen (critique #30: the red line
        // vanished exactly when the voice rose over it).
        let dashed = |frame: &mut Frame, db: f32, color: Color| {
            let y = y_of(db);
            let dash = 4.0 * s;
            let mut x = 0.0;
            while x < r.width {
                let seg = Path::line(Point::new(x, y), Point::new((x + dash).min(r.width), y));
                frame.stroke(&seg, Stroke::default().with_color(Color { a: 0.85, ..SCREEN }).with_width(3.0 * s));
                frame.stroke(&seg, Stroke::default().with_color(color).with_width(1.5 * s));
                x += dash * 2.0;
            }
        };
        if let Some(t) = self.comp_threshold {
            dashed(&mut frame, t, Color::from_rgb8(0xe0, 0x4a, 0x5a));
        }
        if let Some(t) = self.exp_threshold {
            dashed(&mut frame, t, Color::from_rgb8(0x5c, 0xc8, 0x6a));
        }
        if let Some(c) = self.ceiling {
            dashed(&mut frame, c, Color::from_rgb8(0x6c, 0xc4, 0xf0));
        }
        frame.stroke(&Path::rectangle(r.position(), r.size()), Stroke::default().with_color(border).with_width(s));
        vec![frame.into_geometry()]
    }
}

/// Map an input dB value (-60..0) to x within the square.
fn db_to_x(db: f32, sq: Rectangle) -> f32 {
    let t = (db + 60.0) / 60.0;
    sq.x + t * sq.width
}

/// Map an output dB value (-60..0) to y within the square (bottom to top).
fn db_to_y(db: f32, sq: Rectangle) -> f32 {
    let t = (db + 60.0) / 60.0;
    sq.y + sq.height - t * sq.height
}

/// Color a curve segment by its start input level.
fn segment_color(in_db: f32, exp_threshold: Option<f32>, comp_threshold: Option<f32>) -> Color {
    if let Some(exp) = exp_threshold {
        if in_db < exp {
            return Color::from_rgb8(0x5c, 0xc8, 0x6a); // green
        }
    }
    if let Some(comp) = comp_threshold {
        if in_db > comp {
            return Color::from_rgb8(0xe0, 0x4a, 0x5a); // red
        }
    }
    SCREEN_INK // white-ish
}

/// Linearly interpolate the curve at an input dB value.
fn interp_curve(curve: &[f32], in_db: f32) -> f32 {
    if curve.is_empty() {
        return 0.0;
    }
    let idx = (in_db + 60.0).clamp(0.0, (curve.len() - 1) as f32);
    let lo = idx.floor() as usize;
    let hi = (lo + 1).min(curve.len() - 1);
    let frac = idx - lo as f32;
    curve[lo] * (1.0 - frac) + curve[hi] * frac
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Rectangle, Size};

    #[test]
    fn db_to_x_maps_minus_60_to_left_edge() {
        let sq = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 100.0));
        assert!((db_to_x(-60.0, sq) - 10.0).abs() < 0.001);
    }

    #[test]
    fn db_to_x_maps_zero_to_right_edge() {
        let sq = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 100.0));
        assert!((db_to_x(0.0, sq) - 110.0).abs() < 0.001);
    }

    #[test]
    fn db_to_y_maps_minus_60_to_bottom_edge() {
        let sq = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 100.0));
        assert!((db_to_y(-60.0, sq) - 120.0).abs() < 0.001);
    }

    #[test]
    fn db_to_y_maps_zero_to_top_edge() {
        let sq = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 100.0));
        assert!((db_to_y(0.0, sq) - 20.0).abs() < 0.001);
    }

    #[test]
    fn interp_curve_at_known_points() {
        let curve: Vec<f32> = (0..61).map(|i| -60.0 + i as f32).collect();
        assert!((interp_curve(&curve, -60.0) + 60.0).abs() < 0.001);
        assert!((interp_curve(&curve, 0.0) - 0.0).abs() < 0.001);
        assert!((interp_curve(&curve, -30.0) + 30.0).abs() < 0.001);
    }

    #[test]
    fn interp_curve_between_points() {
        let curve: Vec<f32> = vec![0.0; 61];
        // All zeros, so interpolation is always zero.
        assert!((interp_curve(&curve, -45.5) - 0.0).abs() < 0.001);
    }

    #[test]
    fn segment_color_below_exp_is_green() {
        let c = segment_color(-40.0, Some(-30.0), None);
        assert_eq!(c, Color::from_rgb8(0x5c, 0xc8, 0x6a));
    }

    #[test]
    fn segment_color_above_comp_is_red() {
        let c = segment_color(-10.0, None, Some(-20.0));
        assert_eq!(c, Color::from_rgb8(0xe0, 0x4a, 0x5a));
    }

    #[test]
    fn segment_color_between_thresholds_is_primary() {
        let c = segment_color(-25.0, Some(-30.0), Some(-20.0));
        assert_eq!(c, crate::widgets::eq_plot::SCREEN_INK);
    }

    #[test]
    fn segment_color_no_thresholds_is_primary() {
        let c = segment_color(-35.0, None, None);
        assert_eq!(c, crate::widgets::eq_plot::SCREEN_INK);
    }
}
