//! The EQ panel's tuner needle (the owner, 2026-11-10: a voice tuner): how
//! far the voice is from its nearest note, -50 to +50 cents across, the
//! middle marked, the dot green when it's in tune. Display only.

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Renderer, Theme};

use crate::theme;

/// Within this many cents a note counts as in tune.
pub const IN_TUNE_CENTS: f32 = 10.0;

pub struct Needle {
    /// Cents off the nearest note, when there is a voice.
    pub cents: Option<f32>,
    pub scale: f32,
}

/// The needle as an element, as wide as it's given.
pub fn needle<'a, Message: 'a>(cents: Option<f32>, height: f32, scale: f32) -> iced::Element<'a, Message> {
    canvas::Canvas::new(Needle { cents, scale }).width(iced::Length::Fill).height(iced::Length::Fixed(height)).into()
}

impl<Message> canvas::Program<Message> for Needle {
    type State = ();

    fn draw(&self, _state: &(), renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let (w, h) = (bounds.width, bounds.height);
        let r = 4.0 * s;
        let (x0, x1) = (r, (w - r).max(r));
        let mid_y = h / 2.0;
        let x_of = |c: f32| x0 + (x1 - x0) * (c.clamp(-50.0, 50.0) + 50.0) / 100.0;
        let faint = theme::blend(theme::surface(), theme::text_sec(), 0.35);
        frame.stroke(&Path::line(Point::new(x0, mid_y), Point::new(x1, mid_y)), Stroke::default().with_color(faint).with_width(s));
        // The in-tune band, then ticks at -25, 0 and +25.
        let band = Color { a: 0.18, ..theme::METER_SIGNAL };
        frame.fill_rectangle(Point::new(x_of(-IN_TUNE_CENTS), mid_y - 3.0 * s), iced::Size::new(x_of(IN_TUNE_CENTS) - x_of(-IN_TUNE_CENTS), 6.0 * s), band);
        for (c, tall) in [(-25.0, 3.0), (0.0, 5.0), (25.0, 3.0)] {
            let x = x_of(c);
            frame.stroke(&Path::line(Point::new(x, mid_y - tall * s), Point::new(x, mid_y + tall * s)), Stroke::default().with_color(theme::text_sec()).with_width(s));
        }
        if let Some(c) = self.cents.filter(|c| c.is_finite()) {
            // Readable on the panel in every theme (critique #30: the
            // meters' green read 1.8:1 on Daylight's white).
            let ground = [theme::surface()];
            let color = if c.abs() <= IN_TUNE_CENTS {
                theme::readable_on(theme::METER_SIGNAL, theme::text_primary(), &ground)
            } else {
                theme::readable_on(theme::attention(), theme::text_primary(), &ground)
            };
            frame.fill(&Path::circle(Point::new(x_of(c), mid_y), r), color);
        }
        vec![frame.into_geometry()]
    }
}
