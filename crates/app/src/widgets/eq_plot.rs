//! Interactive frequency-response editor for the hardware EQ: the three
//! parametric bands plus the low cut, drawn on a log-frequency axis.
//!
//! The curve is a visual model, not a bit-exact copy of the device's own
//! fixed-point coefficients: the bands use the standard RBJ biquads at the
//! current sample rate, and the low cut is a Butterworth high-pass of
//! order `slope / 6`. That is enough to see the shape and where a band
//! sits, which is what a mixer's EQ plot is for; the device still does the
//! real filtering.
//!
//! Drag a band handle to set its frequency and gain at the same time,
//! scroll over a handle to change its Q, and double-click a handle to
//! reset that band. The low-cut handle at the bottom edge moves the
//! cutoff; its slope stays in the buttons below the plot.

use std::f32::consts::PI;
use std::time::Instant;

use iced::advanced::text as advanced_text;
use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{mouse, Color, Point, Rectangle, Renderer, Size, Theme};
use tuxmix_core::channel::{EqBand, EqBandType, InputEq};

use super::fader::{safe_clamp, DOUBLE_CLICK, SCROLL_IDLE};
use crate::theme;

// The EQ's ranges in RME's own mixer for this unit (docs/ui-spec.md §4.4,
// from the Babyface Pro FS manual). The hardware registers take more
// (±24 dB, Q 0.05–10, a low cut up to 20 kHz); Audra keeps to the manual's
// ranges, so a setting means what it means in TotalMix. The plot's axis
// stays ±24 dB, so overlapping bands that sum past 20 dB still show.

/// Band gain, either way.
pub(crate) const GAIN_LIMIT_DB: f32 = 20.0;
/// Band Q.
pub(crate) const Q_RANGE: (f32, f32) = (0.7, 5.0);
/// Low-cut frequency while the low cut is on (0 Hz is "off").
pub(crate) const LOW_CUT_RANGE_HZ: (u16, u16) = (20, 500);

pub(crate) fn clamp_gain(db: f32) -> f32 {
    db.clamp(-GAIN_LIMIT_DB, GAIN_LIMIT_DB)
}

pub(crate) fn clamp_q(q: f32) -> f32 {
    q.clamp(Q_RANGE.0, Q_RANGE.1)
}

/// A low-cut frequency in range; 0 (off) stays 0.
pub(crate) fn clamp_low_cut(hz: u16) -> u16 {
    if hz == 0 {
        0
    } else {
        hz.clamp(LOW_CUT_RANGE_HZ.0, LOW_CUT_RANGE_HZ.1)
    }
}

/// Plot height at scale 1.
/// 64 until the EQ on/off button moved onto the plot (`eq_popover`): the
/// row it freed went to the curve.
const PLOT_H: f32 = 82.0;
const MARGIN_LEFT: f32 = 26.0;
pub(crate) const MARGIN_RIGHT: f32 = 6.0;
pub(crate) const MARGIN_TOP: f32 = 6.0;
/// Room under the screen for the frequencies, and a little air after them.
const MARGIN_BOTTOM: f32 = 18.0;

const DB_MIN: f32 = -24.0;
const DB_MAX: f32 = 24.0;
const F_MIN: f32 = 20.0;
const F_MAX: f32 = 20_000.0;

/// Samples along the curve. Enough for the Q=10 peaks to read as peaks at
/// any window width without rebuilding the path per pixel.
const CURVE_POINTS: usize = 180;

const HANDLE_R: f32 = 6.0;
const HIT_R: f32 = 13.0;
const LOW_CUT_HANDLE_R: f32 = 5.0;

/// Callbacks for direct manipulation in the plot. They carry one message
/// per interaction so a drag updates frequency and gain in a single
/// `update`, instead of two state changes arriving separately.
pub struct EqPlotActions<Message> {
    pub on_band: Box<dyn Fn(usize, u16, f32) -> Message>,
    pub on_band_reset: Box<dyn Fn(usize) -> Message>,
    pub on_band_q: Box<dyn Fn(usize, f32) -> Message>,
    pub on_low_cut: Box<dyn Fn(u16) -> Message>,
}

/// What moves behind the curve: the sound now (`live_spectrum`), and on an
/// input the voice's pitch and its low end lately (the tuner's).
#[derive(Default)]
pub struct EqLive {
    /// dB per column, log spaced over the plot's frequency span.
    pub spectrum: Option<Vec<f32>>,
    /// The voice's pitch now, Hz, and its note ("A2").
    pub voice: Option<(f32, String)>,
    /// The voice's low end lately, Hz.
    pub lowest: Option<f32>,
}

pub fn eq_plot<'a, Message: 'a>(
    eq: InputEq,
    sample_rate: u32,
    live: EqLive,
    scale: f32,
    modifiers: Modifiers,
    actions: EqPlotActions<Message>,
) -> iced::Element<'a, Message> {
    canvas::Canvas::new(EqPlot {
        eq,
        sample_rate,
        spectrum: live.spectrum,
        voice: live.voice,
        lowest: live.lowest,
        scale,
        modifiers,
        actions,
    })
    .width(iced::Length::Fill)
    .height(iced::Length::Fixed(PLOT_H * scale))
    .into()
}

#[derive(Default)]
pub struct State {
    drag: Option<Drag>,
    last_click: Option<(Drag, Instant)>,
    last_band: Option<(u16, f32)>,
    last_low_cut: Option<u16>,
    scroll_q: Option<f32>,
    scroll_band: Option<usize>,
    last_scroll: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Band(usize),
    LowCut,
}

struct EqPlot<Message> {
    eq: InputEq,
    sample_rate: u32,
    /// The sound now (`live_spectrum`): dB per column, log spaced over the
    /// plot's own frequency span, drawn grey behind the curve.
    spectrum: Option<Vec<f32>>,
    /// The voice's pitch and note, and its low end lately (`EqLive`).
    voice: Option<(f32, String)>,
    lowest: Option<f32>,
    scale: f32,
    modifiers: Modifiers,
    actions: EqPlotActions<Message>,
}

impl<Message> EqPlot<Message> {
    fn plot_rect(&self, size: Size) -> Rectangle {
        let scale = self.scale;
        Rectangle::new(
            Point::new(MARGIN_LEFT * scale, MARGIN_TOP * scale),
            Size::new(
                (size.width - (MARGIN_LEFT + MARGIN_RIGHT) * scale).max(1.0),
                (size.height - (MARGIN_TOP + MARGIN_BOTTOM) * scale).max(1.0),
            ),
        )
    }

    fn band_handle(&self, band: &EqBand, plot: Rectangle) -> Point {
        let r = HANDLE_R * self.scale;
        Point::new(
            safe_clamp(freq_to_x(band.freq_hz as f32, plot), plot.x + r, plot.x + plot.width - r),
            safe_clamp(db_to_y(band.gain_db.clamp(DB_MIN, DB_MAX), plot), plot.y + r, plot.y + plot.height - r),
        )
    }

    fn low_cut_handle(&self, plot: Rectangle) -> Point {
        let r = LOW_CUT_HANDLE_R * self.scale;
        Point::new(
            safe_clamp(freq_to_x(self.eq.low_cut_freq_hz as f32, plot), plot.x + r, plot.x + plot.width - r),
            plot.y + plot.height,
        )
    }

    fn hit(&self, plot: Rectangle, pos: Point) -> Option<Drag> {
        let mut best: Option<(f32, Drag)> = None;
        for (i, band) in self.eq.bands.iter().enumerate() {
            if band.band_type == EqBandType::Off {
                continue;
            }
            let point = self.band_handle(band, plot);
            let distance = (point.x - pos.x).hypot(point.y - pos.y);
            if distance <= HIT_R * self.scale
                && best.is_none_or(|(best_distance, _)| distance < best_distance)
            {
                best = Some((distance, Drag::Band(i)));
            }
        }

        // A switched-off low cut has no handle to grab.
        if !self.eq.low_cut_on() {
            return best.map(|(_, drag)| drag);
        }
        let point = self.low_cut_handle(plot);
        let distance = (point.x - pos.x).hypot(point.y - pos.y);
        if distance <= (LOW_CUT_HANDLE_R + 4.0) * self.scale
            && best.is_none_or(|(best_distance, _)| distance < best_distance)
        {
            best = Some((distance, Drag::LowCut));
        }

        best.map(|(_, drag)| drag)
    }
}

impl<Message> canvas::Program<Message> for EqPlot<Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let plot = self.plot_rect(bounds.size());

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = local_point(bounds, cursor.position_over(bounds)?);
                let target = self.hit(plot, pos)?;
                let now = Instant::now();
                let is_double = state.last_click.is_some_and(|(last, at)| {
                    last == target && now.duration_since(at) < DOUBLE_CLICK
                });

                if is_double {
                    state.last_click = None;
                    state.drag = None;
                    state.last_band = None;
                    state.last_low_cut = None;
                    let message = match target {
                        Drag::Band(band) => (self.actions.on_band_reset)(band),
                        Drag::LowCut => (self.actions.on_low_cut)(20),
                    };
                    return Some(canvas::Action::publish(message).and_capture());
                }

                state.last_click = Some((target, now));
                state.drag = Some(target);
                state.last_band = None;
                state.last_low_cut = None;
                state.scroll_q = None;
                state.scroll_band = None;
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let target = state.drag?;
                let pos = local_point(bounds, cursor.land().position()?);
                match target {
                    Drag::Band(band) => {
                        let (freq, gain) = band_at(plot, pos);
                        if state.last_band == Some((freq, gain)) {
                            return None;
                        }
                        state.last_band = Some((freq, gain));
                        Some(
                            canvas::Action::publish((self.actions.on_band)(band, freq, gain))
                                .and_capture(),
                        )
                    }
                    Drag::LowCut => {
                        let freq = low_cut_at(plot, pos);
                        if state.last_low_cut == Some(freq) {
                            return None;
                        }
                        state.last_low_cut = Some(freq);
                        Some(canvas::Action::publish((self.actions.on_low_cut)(freq)).and_capture())
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take()?;
                state.last_band = None;
                state.last_low_cut = None;
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if crate::widgets::wheel_off() {
                    return None;
                }
                let pos = local_point(bounds, cursor.position_over(bounds)?);
                let Drag::Band(band) = self.hit(plot, pos)? else {
                    return None;
                };
                if self.modifiers.control() {
                    return None;
                }
                let (dy, base_step) = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => (*y, 0.12),
                    mouse::ScrollDelta::Pixels { y, .. } => (*y, 0.02),
                };
                if dy == 0.0 {
                    return None;
                }
                let mult = if self.modifiers.shift() { 0.25 } else { 1.0 };
                let now = Instant::now();
                let fresh_gesture = state
                    .last_scroll
                    .is_none_or(|last| now.duration_since(last) > SCROLL_IDLE)
                    || state.scroll_band != Some(band);
                let base = if fresh_gesture {
                    self.eq.bands[band].q
                } else {
                    state.scroll_q.unwrap_or(self.eq.bands[band].q)
                };
                let q = clamp_q(base * 2f32.powf(dy * base_step * mult));
                state.scroll_q = Some(q);
                state.scroll_band = Some(band);
                state.last_scroll = Some(now);
                if (q - self.eq.bands[band].q).abs() < 0.001 {
                    return None;
                }
                Some(canvas::Action::publish((self.actions.on_band_q)(band, q)).and_capture())
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
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let scale = self.scale;
        let plot = self.plot_rect(bounds.size());

        // A dark screen in every theme (a display, as the meters are),
        // glowing by band across the frequencies (the owner's reference:
        // "It has a red, green, and blue area"): red low, green in the
        // middle, blue high, deeper toward the foot.
        frame.fill_rectangle(Point::new(plot.x, plot.y), Size::new(plot.width, plot.height), SCREEN);
        let [low, mid, high] = theme::eq_band();
        frame.fill_rectangle(
            Point::new(plot.x, plot.y),
            Size::new(plot.width, plot.height),
            canvas::gradient::Linear::new(Point::new(plot.x, plot.y), Point::new(plot.x + plot.width, plot.y))
                .add_stop(0.0, Color { a: 0.42, ..low })
                .add_stop(0.22, Color { a: 0.3, ..low })
                .add_stop(0.5, Color { a: 0.28, ..mid })
                .add_stop(0.78, Color { a: 0.3, ..high })
                .add_stop(1.0, Color { a: 0.42, ..high }),
        );
        frame.fill_rectangle(
            Point::new(plot.x, plot.y),
            Size::new(plot.width, plot.height),
            canvas::gradient::Linear::new(Point::new(plot.x, plot.y), Point::new(plot.x, plot.y + plot.height))
                .add_stop(0.0, Color { a: 0.35, ..Color::BLACK })
                .add_stop(0.45, Color { a: 0.0, ..Color::BLACK })
                .add_stop(1.0, Color { a: 0.3, ..Color::BLACK }),
        );

        let grid = Stroke::default()
            .with_color(Color { a: 0.14, ..Color::WHITE })
            .with_width(scale);
        for db in [-24.0, -12.0, 0.0, 12.0, 24.0] {
            let y = db_to_y(db, plot);
            let zero = db == 0.0;
            frame.stroke(
                &Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                if zero {
                    Stroke::default()
                        .with_color(Color { a: 0.32, ..Color::WHITE })
                        .with_width(scale)
                } else {
                    grid
                },
            );
        }
        for f in [
            20.0, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0,
        ] {
            let x = freq_to_x(f, plot);
            frame.stroke(
                &Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                grid,
            );
        }

        // The sound now, grey from the floor (the owner, 2026-11-10, with
        // FabFilter Pro-Q's analyzer as the picture): on its own scale,
        // -90 dBFS at the foot to -10 at the top, under the grid's EQ dB.
        if let Some(cols) = self.spectrum.as_ref().filter(|c| c.len() >= 2) {
            let n = cols.len() as f32;
            let (top_db, foot_db) = (-10.0f32, -90.0f32);
            let y_of = |db: f32| plot.y + plot.height * ((top_db - db.clamp(foot_db, top_db)) / (top_db - foot_db));
            let x_of = |i: usize| plot.x + plot.width * (i as f32 + 0.5) / n;
            let shape = Path::new(|b| {
                b.move_to(Point::new(plot.x, plot.y + plot.height));
                for (i, db) in cols.iter().enumerate() {
                    b.line_to(Point::new(x_of(i), y_of(*db)));
                }
                b.line_to(Point::new(plot.x + plot.width, plot.y + plot.height));
                b.close();
            });
            frame.fill(&shape, Color { a: 0.16, ..Color::WHITE });
            let edge = Path::new(|b| {
                for (i, db) in cols.iter().enumerate() {
                    let p = Point::new(x_of(i), y_of(*db));
                    if i == 0 {
                        b.move_to(p);
                    } else {
                        b.line_to(p);
                    }
                }
            });
            // Its edge a little under the 0 dB line's brightness, so that
            // line still reads through it.
            frame.stroke(&edge, Stroke::default().with_color(Color { a: 0.24, ..Color::WHITE }).with_width(scale));
        }
        // The voice's low end lately, dashed: a Low Cut above it thins the
        // voice. And its pitch now, a line with its note at the top.
        if let Some(f) = self.lowest.filter(|f| (F_MIN..F_MAX).contains(f)) {
            let x = freq_to_x(f, plot);
            let dash = 3.0 * scale;
            let mut y = plot.y;
            while y < plot.y + plot.height {
                frame.stroke(
                    &Path::line(Point::new(x, y), Point::new(x, (y + dash).min(plot.y + plot.height))),
                    Stroke::default().with_color(Color { a: 0.75, ..SCREEN_INK }).with_width(scale),
                );
                y += dash * 2.0;
            }
        }
        if let Some((f, note)) = self.voice.as_ref().filter(|(f, _)| (F_MIN..F_MAX).contains(f)) {
            let x = freq_to_x(*f, plot);
            // Readable on this fixed dark screen in every theme (critique
            // #30: tuned for the theme's own ground, Daylight's rust read
            // 2.7:1 here).
            let mark = screen_readable(theme::accent());
            frame.stroke(
                &Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                Stroke::default().with_color(Color { a: 0.85, ..mark }).with_width(1.5 * scale),
            );
            frame.fill_text(Text {
                content: note.clone(),
                position: Point::new(x + 3.0 * scale, plot.y + 2.0 * scale),
                color: mark,
                size: (theme::TEXT_SM * scale).into(),
                ..Text::default()
            });
        }

        let rate = if self.sample_rate == 0 {
            48_000.0
        } else {
            self.sample_rate as f32
        };

        // Individual band shapes first, dimmed, so it is obvious which
        // handle is responsible for which part of the combined curve.
        for (i, band) in self.eq.bands.iter().enumerate() {
            if band.band_type == EqBandType::Off {
                continue;
            }
            let points = response_points(plot, |f| band_db(band, f, rate));
            frame.stroke(
                &path_from(&points),
                Stroke::default()
                    .with_color(Color {
                        a: 0.28,
                        ..self.handle_color(state, i)
                    })
                    .with_width(scale),
            );
        }

        if self.eq.low_cut_on() {
            draw_low_cut(&mut frame, &self.eq, plot, scale, state.drag);
        }

        let points = response_points(plot, |f| response_db(&self.eq, f, rate));
        // The curve in the bands' colours, low to high, as the reference
        // draws it; grey while bypassed.
        // Split hard where the bands' regions meet (about 300 Hz and
        // 3 kHz), as the reference's line is, rather than fading.
        let split = |hz: f32| ((freq_to_x(hz, plot) - plot.x) / plot.width.max(1.0)).clamp(0.01, 0.99);
        let (a, b) = (split(300.0), split(3_000.0));
        let style = if self.eq.enabled {
            canvas::Style::Gradient(
                canvas::gradient::Linear::new(Point::new(plot.x, plot.y), Point::new(plot.x + plot.width, plot.y))
                    .add_stop(0.0, low)
                    .add_stop(a, low)
                    .add_stop(a + 0.002, mid)
                    .add_stop(b, mid)
                    .add_stop(b + 0.002, high)
                    .add_stop(1.0, high)
                    .into(),
            )
        } else {
            canvas::Style::Solid(SCREEN_INK)
        };
        frame.stroke(
            &path_from(&points),
            Stroke { style, width: 1.8 * scale, ..Stroke::default() },
        );

        for (i, band) in self.eq.bands.iter().enumerate() {
            if band.band_type == EqBandType::Off {
                continue;
            }
            let point = self.band_handle(band, plot);
            let active = state.drag == Some(Drag::Band(i));
            let handle_color = self.handle_color(state, i);
            let radius = if active {
                (HANDLE_R + 1.0) * scale
            } else {
                HANDLE_R * scale
            };
            frame.fill(&Path::circle(point, radius), theme::bg_deep());
            frame.stroke(
                &Path::circle(point, radius),
                Stroke::default()
                    .with_color(handle_color)
                    .with_width(if active { 1.8 } else { 1.3 } * scale),
            );
            if active {
                frame.stroke(
                    &Path::circle(point, radius + 2.0 * scale),
                    Stroke::default()
                        .with_color(theme::accent())
                        .with_width(scale),
                );
            }
            frame.fill_text(Text {
                content: (i + 1).to_string(),
                position: point,
                // On the handle's `bg_deep` disc (critique #27: a band's
                // hue read at 3.6:1 there in Nord and Solarized).
                color: theme::readable_on(handle_color, theme::text_primary(), &[theme::bg_deep()]),
                size: (theme::TEXT_SM * scale).into(),
                align_x: advanced_text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
        }

        // The labels borrow the frame mutably; scope them so the frame is
        // free again for `into_geometry` below.
        {
            let mut label = |content: String, position: Point, align_x, align_y| {
                frame.fill_text(Text {
                    content,
                    position,
                    color: theme::text_sec(),
                    size: (theme::TEXT_SM * scale).into(),
                    align_x,
                    align_y,
                    ..Text::default()
                });
            };
            label(
                "+24".into(),
                Point::new(MARGIN_LEFT * scale - 3.0 * scale, plot.y),
                advanced_text::Alignment::Right,
                iced::alignment::Vertical::Top,
            );
            label(
                "0".into(),
                Point::new(MARGIN_LEFT * scale - 3.0 * scale, db_to_y(0.0, plot)),
                advanced_text::Alignment::Right,
                iced::alignment::Vertical::Center,
            );
            label(
                "-24".into(),
                Point::new(MARGIN_LEFT * scale - 3.0 * scale, plot.y + plot.height),
                advanced_text::Alignment::Right,
                iced::alignment::Vertical::Bottom,
            );
            // No "20k": it is one octave from "10k", narrower than either
            // label on a strip-wide plot, and the plot's right edge is 20 kHz.
            for f in [20.0, 100.0, 1_000.0, 10_000.0] {
                label(
                    freq_label(f),
                    Point::new(freq_to_x(f, plot), plot.y + plot.height + 2.0 * scale),
                    advanced_text::Alignment::Center,
                    iced::alignment::Vertical::Top,
                );
            }
            if !self.eq.enabled {
                // A plate of the screen behind it, so the live analyser and
                // a band's wash passing under it don't take its contrast
                // (critique #64 B: 3.74-3.94 in Nord, Solarized, Contrast).
                let size = theme::TEXT_SM * scale;
                let (w, h) = (size * 0.62 * 6.0 + 4.0 * scale, size + 2.0 * scale);
                frame.fill_rectangle(
                    Point::new(plot.x + plot.width - 3.0 * scale - w + 2.0 * scale, plot.y + plot.height - 3.0 * scale - h),
                    Size::new(w, h),
                    Color { a: 0.9, ..SCREEN },
                );
                frame.fill_text(Text {
                    content: "BYPASS".into(),
                    position: Point::new(plot.x + plot.width - 3.0 * scale, plot.y + plot.height - 3.0 * scale),
                    color: SCREEN_INK,
                    size: (theme::TEXT_SM * scale).into(),
                    align_x: advanced_text::Alignment::Right,
                    align_y: iced::alignment::Vertical::Bottom,
                    ..Text::default()
                });
            }
            if let Some(target) = state.drag {
                let (content, x, color) = match target {
                    Drag::Band(band) => {
                        let b = self.eq.bands[band];
                        (
                            format!(
                                "{}  {}  {:+.1} dB  Q {:.2}",
                                band + 1,
                                freq_label(b.freq_hz as f32),
                                b.gain_db,
                                b.q
                            ),
                            plot.x + 4.0 * scale,
                            theme::eq_band()[band.min(theme::eq_band().len() - 1)],
                        )
                    }
                    Drag::LowCut => (
                        format!("LC  {}", freq_label(self.eq.low_cut_freq_hz as f32)),
                        plot.x + 4.0 * scale,
                        theme::accent(),
                    ),
                };
                frame.fill_text(Text {
                    content,
                    position: Point::new(x, plot.y + 3.0 * scale),
                    color: screen_readable(color),
                    size: (theme::TEXT_SM * scale).into(),
                    align_x: advanced_text::Alignment::Left,
                    align_y: iced::alignment::Vertical::Top,
                    ..Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        match cursor.position_over(bounds) {
            Some(pos) => match self.hit(self.plot_rect(bounds.size()), local_point(bounds, pos)) {
                Some(Drag::LowCut) => mouse::Interaction::ResizingHorizontally,
                Some(Drag::Band(_)) => mouse::Interaction::Grab,
                None => mouse::Interaction::None,
            },
            None => mouse::Interaction::None,
        }
    }
}

impl<Message> EqPlot<Message> {
    fn handle_color(&self, _state: &State, band: usize) -> Color {
        theme::eq_band()[band.min(theme::eq_band().len() - 1)]
    }
}

fn band_at(plot: Rectangle, pos: Point) -> (u16, f32) {
    let t = ((pos.x - plot.x) / plot.width.max(1.0)).clamp(0.0, 1.0);
    let freq = F_MIN * (F_MAX / F_MIN).powf(t);
    let db = DB_MAX - ((pos.y - plot.y) / plot.height.max(1.0)).clamp(0.0, 1.0) * (DB_MAX - DB_MIN);
    let db = (db * 10.0).round() / 10.0;
    let db = if db.abs() < 0.15 { 0.0 } else { db };
    (
        freq.round().clamp(F_MIN, F_MAX) as u16,
        clamp_gain(db),
    )
}

fn low_cut_at(plot: Rectangle, pos: Point) -> u16 {
    let t = ((pos.x - plot.x) / plot.width.max(1.0)).clamp(0.0, 1.0);
    clamp_low_cut((F_MIN * (F_MAX / F_MIN).powf(t)).round().clamp(F_MIN, F_MAX) as u16)
}

fn response_points(plot: Rectangle, db_at: impl Fn(f32) -> f32) -> Vec<Point> {
    (0..=CURVE_POINTS)
        .map(|i| {
            let t = i as f32 / CURVE_POINTS as f32;
            let f = F_MIN * (F_MAX / F_MIN).powf(t);
            let db = db_at(f).clamp(DB_MIN, DB_MAX);
            Point::new(plot.x + plot.width * t, db_to_y(db, plot))
        })
        .collect()
}

fn path_from(points: &[Point]) -> Path {
    Path::new(|p| {
        if let Some(first) = points.first() {
            p.move_to(*first);
        }
        for point in points.iter().skip(1) {
            p.line_to(*point);
        }
    })
}

fn draw_low_cut(frame: &mut Frame, eq: &InputEq, plot: Rectangle, scale: f32, drag: Option<Drag>) {
    let x = safe_clamp(
        freq_to_x(eq.low_cut_freq_hz as f32, plot),
        plot.x + LOW_CUT_HANDLE_R * scale,
        plot.x + plot.width - LOW_CUT_HANDLE_R * scale,
    );
    let color = if drag == Some(Drag::LowCut) {
        theme::accent()
    } else {
        Color { a: 0.55, ..SCREEN_INK }
    };
    let dash = 3.0 * scale;
    let gap = 2.0 * scale;
    let mut y = plot.y;
    while y < plot.y + plot.height {
        let end = (y + dash).min(plot.y + plot.height);
        frame.stroke(
            &Path::line(Point::new(x, y), Point::new(x, end)),
            Stroke::default().with_color(color).with_width(scale),
        );
        y += dash + gap;
    }

    let radius = LOW_CUT_HANDLE_R * scale;
    let triangle = Path::new(|p| {
        p.move_to(Point::new(x, plot.y + plot.height - radius));
        p.line_to(Point::new(x - radius, plot.y + plot.height));
        p.line_to(Point::new(x + radius, plot.y + plot.height));
        p.close();
    });
    frame.fill(&triangle, color);
}

fn db_to_y(db: f32, plot: Rectangle) -> f32 {
    plot.y + plot.height * (DB_MAX - db) / (DB_MAX - DB_MIN)
}

fn local_point(bounds: Rectangle, pos: Point) -> Point {
    Point::new(pos.x - bounds.x, pos.y - bounds.y)
}

fn freq_to_x(f: f32, plot: Rectangle) -> f32 {
    let t = (f / F_MIN).log10() / (F_MAX / F_MIN).log10();
    plot.x + plot.width * t
}

fn freq_label(f: f32) -> String {
    if f >= 1_000.0 {
        format!("{}k", f / 1_000.0)
    } else {
        format!("{f:.0}")
    }
}

/// Combined response in dB at `f`, summing the enabled bands and the low
/// cut. Bands are independent biquads, so their magnitudes multiply and
/// their dB values add.
fn response_db(eq: &InputEq, f: f32, rate: f32) -> f32 {
    let bands = eq
        .bands
        .iter()
        .map(|band| band_db(band, f, rate))
        .sum::<f32>();
    bands + low_cut_db(eq, f)
}

fn band_db(band: &EqBand, f: f32, rate: f32) -> f32 {
    if band.band_type == EqBandType::Off {
        return 0.0;
    }
    let (b, a) = band_coeffs(band, rate);
    let w = 2.0 * PI * f / rate;
    let (num_re, num_im) = biquad_response(&b, w);
    let (den_re, den_im) = biquad_response(&a, w);
    let num = (num_re * num_re + num_im * num_im).sqrt();
    let den = (den_re * den_re + den_im * den_im).sqrt();
    20.0 * (num / den).max(1e-9).log10()
}

fn band_coeffs(band: &EqBand, rate: f32) -> ([f32; 3], [f32; 3]) {
    let freq = (band.freq_hz as f32).clamp(10.0, rate * 0.49);
    let q = band.q.max(0.01);
    let gain = band.gain_db;
    let a = 10f32.powf(gain / 40.0);
    let w0 = 2.0 * PI * freq / rate;
    let cos = w0.cos();
    let sin = w0.sin();
    let alpha = sin / (2.0 * q);

    match band.band_type {
        EqBandType::Off => ([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        EqBandType::Bell => (
            [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
        ),
        EqBandType::LowShelf => {
            let sqrt_a = a.sqrt();
            let alpha = sin / 2.0 * 2f32.sqrt();
            (
                [
                    a * ((a + 1.0) - (a - 1.0) * cos + 2.0 * sqrt_a * alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - 2.0 * sqrt_a * alpha),
                ],
                [
                    (a + 1.0) + (a - 1.0) * cos + 2.0 * sqrt_a * alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - 2.0 * sqrt_a * alpha,
                ],
            )
        }
        EqBandType::HighShelf => {
            let sqrt_a = a.sqrt();
            let alpha = sin / 2.0 * 2f32.sqrt();
            (
                [
                    a * ((a + 1.0) + (a - 1.0) * cos + 2.0 * sqrt_a * alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - 2.0 * sqrt_a * alpha),
                ],
                [
                    (a + 1.0) - (a - 1.0) * cos + 2.0 * sqrt_a * alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - 2.0 * sqrt_a * alpha,
                ],
            )
        }
    }
}

/// Real and imaginary parts of the numerator/denominator at `w`.
fn biquad_response(c: &[f32; 3], w: f32) -> (f32, f32) {
    let re = c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos();
    let im = -(c[1] * w.sin() + c[2] * (2.0 * w).sin());
    (re, im)
}

/// Butterworth high-pass of order `slope / 6`, normalized to 0 dB well
/// above the cutoff and -3 dB at it.
fn low_cut_db(eq: &InputEq, f: f32) -> f32 {
    if !eq.low_cut_on() {
        return 0.0;
    }
    let order = (eq.low_cut_slope_db_oct.max(6) / 6) as i32;
    let r = f / eq.low_cut_freq_hz.max(1) as f32;
    let mag = r.powi(order) / (1.0 + r.powi(2 * order)).sqrt();
    20.0 * mag.max(1e-9).log10()
}

/// `hue` readable on the screen's top edge wherever it stands: the screen,
/// its three bands' glow and the dark wash over them (critique #27: a
/// drag's readout read at 2.9:1 on Daylight). Lifted toward the screen's
/// ink, so it stays light on the dark screen in every theme.
pub(crate) fn screen_readable(hue: Color) -> Color {
    let [low, mid, high] = theme::eq_band();
    let top = |band: Color, a: f32| {
        let lit = theme::blend(SCREEN, band, a);
        theme::blend(lit, Color::BLACK, 0.35)
    };
    theme::readable_on(hue, SCREEN_INK, &[SCREEN, top(low, 0.42), top(mid, 0.28), top(high, 0.42)])
}

/// The graph's screen: dark in every theme, as a display is.
pub(crate) const SCREEN: Color = Color::from_rgb(0.045, 0.05, 0.06);
/// Words and the bypassed curve on the screen: a light grey for the dark
/// screen, whatever the theme.
pub(crate) const SCREEN_INK: Color = Color::from_rgb(0.72, 0.75, 0.79);

#[cfg(test)]
mod tests {
    use super::{
        band_at, db_to_y, freq_to_x, low_cut_at, low_cut_db, response_db, DB_MAX, DB_MIN,
        GAIN_LIMIT_DB, LOW_CUT_RANGE_HZ,
    };
    use iced::{Point, Rectangle, Size};
    use tuxmix_core::channel::{EqBand, EqBandType, InputEq};

    #[test]
    fn the_screens_ink_reads_on_the_screen() {
        // Critique #26's detector gap: the words drawn on the dark screen
        // in every theme ("BYPASS", the scale) had no contrast check.
        let words = crate::theme::contrast(super::SCREEN_INK, super::SCREEN);
        assert!(words >= 7.0, "screen words at {words:.1}:1, under 7:1");
        // The low cut's dashed line, at 55% over the screen: a graphic,
        // so 3:1.
        let a = 0.55;
        let mix = |i: f32, s: f32| a * i + (1.0 - a) * s;
        let (i, s) = (super::SCREEN_INK, super::SCREEN);
        let line = iced::Color::from_rgb(mix(i.r, s.r), mix(i.g, s.g), mix(i.b, s.b));
        let ratio = crate::theme::contrast(line, s);
        assert!(ratio >= 3.0, "the low cut's line at {ratio:.1}:1, under 3:1");
    }

    fn bell(gain_db: f32) -> InputEq {
        let mut eq = InputEq::default();
        eq.bands[0] = EqBand {
            band_type: EqBandType::Bell,
            freq_hz: 1_000,
            q: 1.0,
            gain_db,
        };
        eq
    }

    #[test]
    fn a_squeezed_plot_places_its_handles_without_panicking() {
        // The owner's crash, 2026-11-04: while the EQ panel slid open its
        // plot was narrower than a handle, and keeping the handle inside
        // it asked `f32::clamp` for a minimum above its maximum.
        let plot = super::EqPlot::<()> {
            eq: bell(6.0),
            sample_rate: 48_000,
            spectrum: None,
            voice: None,
            lowest: None,
            scale: 1.0,
            modifiers: iced::keyboard::Modifiers::default(),
            actions: super::EqPlotActions {
                on_band: Box::new(|_, _, _| ()),
                on_band_reset: Box::new(|_| ()),
                on_band_q: Box::new(|_, _| ()),
                on_low_cut: Box::new(|_| ()),
            },
        };
        let tiny = Rectangle::new(Point::new(10.0, 10.0), Size::new(4.0, 3.0));
        for band in &plot.eq.bands {
            let p = plot.band_handle(band, tiny);
            assert!(p.x.is_finite() && p.y.is_finite());
        }
        assert!(plot.low_cut_handle(tiny).x.is_finite());
    }

    #[test]
    fn bell_peaks_at_its_own_gain() {
        let eq = bell(12.0);
        let db = response_db(&eq, 1_000.0, 48_000.0);
        assert!((db - 12.0).abs() < 0.2, "bell at 1 kHz read {db} dB");
    }

    #[test]
    fn a_default_eq_is_flat_at_one_khz() {
        let eq = InputEq::default();
        let db = response_db(&eq, 1_000.0, 48_000.0);
        assert!(db.abs() < 0.01, "default EQ read {db} dB at 1 kHz");
    }

    #[test]
    fn a_switched_off_low_cut_does_not_touch_the_response() {
        let eq = InputEq { low_cut_freq_hz: 0, ..InputEq::default() };
        assert_eq!(low_cut_db(&eq, 20.0), 0.0);
        assert_eq!(low_cut_db(&eq, 5_000.0), 0.0);
    }

    #[test]
    fn low_cut_uses_the_expected_slope() {
        let eq = InputEq { low_cut_freq_hz: 1_000, low_cut_slope_db_oct: 24, ..InputEq::default() };
        let db = low_cut_db(&eq, 500.0);
        assert!((db + 24.0).abs() < 1.0, "24 dB/oct low cut read {db} dB");
    }

    #[test]
    fn zero_db_sits_in_the_vertical_middle() {
        let plot = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 80.0));
        let middle = db_to_y((DB_MIN + DB_MAX) / 2.0, plot);
        assert!((middle - 60.0).abs() < 0.001);
    }

    #[test]
    fn plot_mapping_round_trips_a_band_position() {
        let plot = Rectangle::new(Point::new(10.0, 20.0), Size::new(200.0, 100.0));
        let pos = Point::new(freq_to_x(1_000.0, plot), db_to_y(6.0, plot));
        let (freq, gain) = band_at(plot, pos);
        assert!((freq as f32 - 1_000.0).abs() < 1.0, "read {freq} Hz");
        assert!((gain - 6.0).abs() < 0.1, "read {gain} dB");
    }

    #[test]
    fn low_cut_mapping_keeps_to_its_range() {
        let plot = Rectangle::new(Point::new(10.0, 20.0), Size::new(200.0, 100.0));
        assert_eq!(low_cut_at(plot, Point::new(plot.x, plot.y)), LOW_CUT_RANGE_HZ.0);
        // Dragged to the plot's right edge, it stops at 500 Hz.
        assert_eq!(
            low_cut_at(plot, Point::new(plot.x + plot.width, plot.y)),
            LOW_CUT_RANGE_HZ.1
        );
    }

    #[test]
    fn dragging_a_band_past_the_gain_range_stops_at_20_db() {
        let plot = Rectangle::new(Point::new(10.0, 20.0), Size::new(200.0, 100.0));
        let (_, top) = band_at(plot, Point::new(plot.x + 50.0, plot.y));
        let (_, bottom) = band_at(plot, Point::new(plot.x + 50.0, plot.y + plot.height));
        assert_eq!((top, bottom), (GAIN_LIMIT_DB, -GAIN_LIMIT_DB));
    }
}
