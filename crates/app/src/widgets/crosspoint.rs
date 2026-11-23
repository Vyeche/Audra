//! One Matrix crosspoint: a gain cell, drawn and driven the way TotalMix's
//! own Matrix is (docs.rme-audio.com, "The Matrix").
//!
//! * Drag up/down from the cell to change its gain. The drag is *relative*:
//!   pressing never moves the value, and the full -inf..+6 dB range takes
//!   `DRAG_TRAVEL` pixels, not the cell's own 20. Shift drags finely.
//! * Ctrl+click toggles the crosspoint between unrouted and 0 dB ("Two
//!   green 0.0 dB field pop in, another click removes them").
//! * Double-click recalls the caller's reset value.
//! * The mouse wheel is left alone, so it scrolls the grid. These cells
//!   used to be hidden faders, which took every wheel event (a wheel
//!   scroll over the grid rewrote crosspoints instead of scrolling) and
//!   mapped the whole range onto the cell's 20px height.
//! * Drawn as keys and cuts in a plate, after TotalMix: a cut = no routing,
//!   a raised key + number = any gain, lit in the fader caps' metal at 0 dB
//!   (green is the meters' own), in mute blue when the source is muted,
//!   in clip red when its phase is inverted.

use std::time::Instant;

use iced::advanced::text as advanced_text;
use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::theme;
use crate::widgets::fader::{t_to_vol, vol_to_t, DOUBLE_CLICK, FINE_SENSITIVITY};

/// Pointer travel, at scale 1, that sweeps the whole fader range.
const DRAG_TRAVEL: f32 = 150.0;
/// Within this many dB of 0 a crosspoint counts as unity (green).
const UNITY_DB: f32 = 0.05;

pub struct Crosspoint<Message> {
    /// The stored linear gain the gestures start from; `0.0` = unrouted.
    pub value: f32,
    /// What the cell shows and is colored by, when that differs from
    /// `value`: a mono source's crosspoint is one stored gain plus a pan,
    /// decoded per side for display, while a drag keeps moving the stored
    /// gain (starting from the displayed number would jump it).
    pub shown: f32,
    /// `false` for the side of an output pair the source has no hardware
    /// path to: drawn as an empty well, inert.
    pub interactive: bool,
    pub muted: bool,
    pub phase: bool,
    /// An output's own volume (the Outputs row), not a send: never lit at
    /// unity.
    pub master: bool,
    /// Whether the app holds this cell as the hovered one. The cell
    /// compares the pointer against this, not against a flag of its own:
    /// during a drag the pointer crosses other cells, whose enter/exit
    /// messages move the app's hover away, and a local flag would then
    /// never announce this cell again when the pointer comes back.
    pub hovered: bool,
    /// A patch's pulse passing through this cell, 0..1 (`motion::matrix_pulse`,
    /// Fun pass 2): drawn as a glow round it, over the cached cell.
    pub pulse: f32,
    pub modifiers: Modifiers,
    pub scale: f32,
    pub reset_value: f32,
    /// A one-click set: a double-click's reset (`true`) or Ctrl+click's
    /// 0 dB / off. Drags go to `on_change`.
    pub on_set: Box<dyn Fn(f32, bool) -> Message>,
    pub on_change: Box<dyn Fn(f32) -> Message>,
    /// End of a press/drag gesture (closes its undo step).
    pub on_release: Message,
    /// The pointer entered (`true`) or left the cell.
    pub on_hover: Box<dyn Fn(bool) -> Message>,
}

#[derive(Default)]
pub struct State {
    pressed: bool,
    dragging: bool,
    last_y: Option<f32>,
    drag_t: f32,
    last_click: Option<Instant>,
    /// The cell as last drawn, and what it showed: redrawn only when that
    /// changes. A grid of ~300 cells used to tessellate every cell, text
    /// included, on every frame (30 a second while meters move), which
    /// made the Matrix twice as costly as the Mixer.
    cache: canvas::Cache,
    drawn: std::cell::Cell<u64>,
}

/// The cell text: bare dB with one decimal, signed when above unity.
pub fn cell_text(vol: f32) -> Option<String> {
    (vol > f32::EPSILON).then(|| {
        let db = 20.0 * vol.log10();
        if db.abs() < UNITY_DB {
            "0.0".to_string()
        } else if db > 0.0 {
            format!("+{db:.1}")
        } else {
            format!("{db:.1}")
        }
    })
}

/// The hue a cell's key lights in, if any: the source's mute or phase
/// over any level, then a unity send in the fader caps' metal, silver on
/// a light theme (the reviewer: in the fader colour Daylight's unity keys
/// were near-black slabs; green is the meters' signal). An output's own
/// volume (`master`) never lights at unity: 0 dB is where every master
/// rests, and the Outputs row lit as one long bar.
fn lit_hue(routed: bool, unity: bool, muted: bool, phase: bool, master: bool) -> Option<Color> {
    if !routed {
        None
    } else if muted {
        Some(theme::mute_color())
    } else if phase {
        Some(theme::clip())
    } else if unity && !master {
        Some(theme::fader_metal())
    } else {
        None
    }
}

fn rounded(r: Rectangle, radius: f32) -> Path {
    Path::new(move |p| p.rounded_rectangle(r.position(), r.size(), radius.into()))
}

/// An unrouted crosspoint: a shallow cut in the panel, as the strips'
/// lanes and value boxes are (`theme::inset`): shade inside, its top wall
/// in shadow, the panel's lit lip under it. Drawn lighter than a lane, as
/// there are three hundred of them. Under the cursor it comes forward.
/// Translucent throughout, so the selected mix's lamp band behind the
/// grid shows through it.
fn draw_cut(frame: &mut Frame, r: Rectangle, radius: f32, scale: f32, hovered: bool) {
    let m = theme::inset();
    let dark = theme::dark_theme();
    let fade = |c: Color, k: f32| Color { a: c.a * k, ..c };
    let shade = |a: f32| Color { a, ..Color::BLACK };
    let down = |stops: &[(f32, Color)]| -> canvas::Style {
        let mut g = canvas::gradient::Linear::new(Point::new(r.x, r.y), Point::new(r.x, r.y + r.height));
        for &(at, c) in stops {
            g = g.add_stop(at.clamp(0.0, 1.0), c);
        }
        canvas::Style::Gradient(g.into())
    };
    // How far, as a share of the cut's height, its top wall's shadow
    // reaches in.
    let reach = |px: f32| (px * scale / r.height.max(1.0)).clamp(0.0, 0.45);
    // The panel's lit lip along the cut's foot, just under it.
    let foot_y = r.y + r.height + 0.5 * scale;
    frame.stroke(
        &Path::line(Point::new(r.x + radius, foot_y), Point::new(r.x + r.width - radius, foot_y)),
        Stroke::default().with_color(fade(m.lip, if dark { 0.6 } else { 1.0 })).with_width(scale),
    );
    let cut = rounded(r, radius);
    // Daylight's is a flat grey cut, the tone doing the work (the reviewer:
    // a shaded one was the white keys' own tone, so key and cut differed
    // only by their rims).
    let (top, foot) = if dark { (fade(m.top, 0.9), fade(m.foot, 0.7)) } else { (shade(0.10), shade(0.10)) };
    frame.fill(&cut, canvas::Fill { style: down(&[(0.0, top), (1.0, foot)]), ..canvas::Fill::default() });
    if hovered {
        frame.fill(&cut, Color { a: if dark { 0.10 } else { 0.06 }, ..theme::hover_toward() });
    }
    // The top wall in shadow, the bottom one catching a little light.
    let wall = rounded(
        Rectangle::new(
            Point::new(r.x + 0.5 * scale, r.y + 0.5 * scale),
            Size::new(r.width - scale, r.height - scale),
        ),
        (radius - 0.5 * scale).max(0.0),
    );
    frame.stroke(
        &wall,
        Stroke {
            style: down(&[
                (0.0, if dark { fade(m.shadow, 0.8) } else { shade(0.30) }),
                (reach(3.0), Color::TRANSPARENT),
                (0.8, Color::TRANSPARENT),
                (1.0, if dark { fade(m.glint, 0.6) } else { Color::TRANSPARENT }),
            ]),
            width: scale,
            ..Stroke::default()
        },
    );
    // A rim all round in a dark theme; none in a light one, where a dark
    // ring turned the cut into a raised pill. Under the cursor, the fader
    // colour's ring in both.
    if hovered {
        frame.stroke(&cut, Stroke::default().with_color(theme::fader_color()).with_width(1.0));
    } else if dark {
        // On High contrast's black the cut and its rim vanished (critique
        // #84 B: an empty cell 1.02-1.07 against the panel): there, a rim a
        // control's edge needs, 3:1.
        let stark = theme::contrast(theme::text_primary(), theme::surface()) >= 18.0;
        // High contrast holds rims at 4.5 (critiques #109-#111 B: 3.53; #112 B:
        // 4.30 on the selected column's lighter band).
        let rim = if stark { theme::blend(theme::surface(), theme::text_primary(), 0.62) } else { fade(m.rim, 0.8) };
        // Two pixels there: one, on a half pixel, read 3.77 only at its
        // corners (critique #85 A P3-7); light enough for the selected
        // column's grey too (#86 A P3-9: 2.00-2.51 there), yet no
        // louder than a set cell's own rim (#87 B: 6.28 against 4.18-5.66).
        frame.stroke(&cut, Stroke::default().with_color(rim).with_width(if stark { 2.0 } else { 1.0 }));
    }
}

/// A routed crosspoint: a low key standing on the panel, lit from above
/// with its shadow under it, as the strips' keys are; `lit` in a hue when
/// the send is at unity or its source is muted or phase-inverted. Returns
/// the ink its number is printed in.
#[allow(clippy::too_many_arguments)]
fn draw_key(
    frame: &mut Frame,
    r: Rectangle,
    radius: f32,
    scale: f32,
    lit: Option<Color>,
    hovered: bool,
    pressed: bool,
) -> Color {
    let dark = theme::dark_theme();
    let shade = |a: f32| Color { a, ..Color::BLACK };
    let face = rounded(r, radius);
    // Its shadow on the panel along its foot (a line, not a block under
    // it: the face is translucent, so the lamp band behind shows through).
    let foot_y = r.y + r.height + 0.5 * scale;
    frame.stroke(
        &Path::line(Point::new(r.x + radius * 0.5, foot_y), Point::new(r.x + r.width - radius * 0.5, foot_y)),
        Stroke::default().with_color(shade(if dark { 0.55 } else { 0.18 })).with_width(scale),
    );
    let (top, foot, edge, ink) = match lit {
        Some(hue) => {
            let hue = if hovered { theme::hovered(hue) } else { hue };
            let (top, foot) = theme::key_lit_colors(hue, 0.0, pressed);
            (top, foot, theme::key_lit_edge(hue), theme::ink_on(theme::blend(top, foot, 0.5)))
        }
        None => {
            let (top, foot) = theme::key_rest_colors(hovered, pressed);
            let edge = if hovered { theme::fader_color() } else { theme::control_edge() };
            (top, foot, edge, theme::text_primary())
        }
    };
    frame.fill(
        &face,
        canvas::gradient::Linear::new(Point::new(r.x, r.y), Point::new(r.x, r.y + r.height))
            .add_stop(0.0, top)
            .add_stop(1.0, foot),
    );
    // The front edge of its top face catching the light.
    if !pressed {
        let lip = Path::line(
            Point::new(r.x + radius, r.y + 1.0),
            Point::new(r.x + r.width - radius, r.y + 1.0),
        );
        let glint = if lit.is_some() { 0.35 } else if dark { 0.10 } else { 0.7 };
        frame.stroke(&lip, Stroke::default().with_color(Color { a: glint, ..Color::WHITE }).with_width(1.0));
    }
    frame.stroke(&face, Stroke::default().with_color(edge).with_width(1.0));
    ink
}

/// The Ctrl+click toggle: a routed crosspoint goes off, an unrouted one
/// to 0 dB.
pub fn toggled(value: f32) -> f32 {
    if value > f32::EPSILON {
        0.0
    } else {
        1.0
    }
}

/// One drag step: `dy` pixels (down = positive), in tapered fader space so
/// the feel is even across the dB range.
pub fn dragged(start_t: f32, dy: f32, scale: f32, fine: bool) -> f32 {
    let travel = DRAG_TRAVEL * scale.max(0.1);
    let mult = if fine { FINE_SENSITIVITY } else { 1.0 };
    (start_t - dy / travel * mult).clamp(0.0, 1.0)
}

impl<Message: Clone> canvas::Program<Message> for Crosspoint<Message> {
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
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.pressed {
                    let y = cursor.land().position()?.y;
                    let last = state.last_y.replace(y).unwrap_or(y);
                    if y == last {
                        return None;
                    }
                    state.dragging = true;
                    state.drag_t =
                        dragged(state.drag_t, y - last, self.scale, self.modifiers.shift());
                    return Some(
                        canvas::Action::publish((self.on_change)(t_to_vol(state.drag_t)))
                            .and_capture(),
                    );
                }
                let over = cursor.is_over(bounds);
                (over != self.hovered).then(|| canvas::Action::publish((self.on_hover)(over)))
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_over(bounds)?;
                let now = Instant::now();
                let double = state
                    .last_click
                    .is_some_and(|t| now.duration_since(t) < DOUBLE_CLICK);
                state.last_click = if double { None } else { Some(now) };
                state.pressed = true;
                state.dragging = false;
                state.last_y = Some(pos.y);
                state.drag_t = vol_to_t(self.value);
                let value = if double {
                    self.reset_value
                } else if self.modifiers.control() {
                    toggled(self.value)
                } else {
                    // A plain press only arms a drag: nothing changes, so
                    // nothing is published (an unchanged value would still
                    // open an empty undo step). Captured all the same, so
                    // the page underneath does not clear the selection.
                    return Some(canvas::Action::request_redraw().and_capture());
                };
                state.drag_t = vol_to_t(value);
                Some(canvas::Action::publish((self.on_set)(value, double)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.pressed {
                    return None;
                }
                state.pressed = false;
                state.dragging = false;
                state.last_y = None;
                Some(canvas::Action::publish(self.on_release.clone()).and_capture())
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
        let routed = self.interactive && self.shown > f32::EPSILON;
        let unity = routed && (20.0 * self.shown.log10()).abs() < UNITY_DB;
        // What lights a routed cell, as a key lights: the source's mute or
        // phase over any level, then a unity send in the fader's own colour
        // (green is the meters' signal, so 0 dB no longer borrows it).
        let lit = lit_hue(routed, unity, self.muted, self.phase, self.master);
        let hovered = self.interactive && self.hovered;
        let dragging = state.dragging;
        let label = self.interactive.then(|| cell_text(self.shown)).flatten();

        // Everything the drawing depends on, so the cached cell is redrawn
        // exactly when one of them changes (a theme fade included).
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (routed, self.interactive, hovered, dragging).hash(&mut h);
            lit.map(|c| c.into_rgba8()).hash(&mut h);
            label.hash(&mut h);
            (bounds.width.to_bits(), bounds.height.to_bits(), self.scale.to_bits()).hash(&mut h);
            crate::theme::palette_key().hash(&mut h);
            h.finish()
        };
        if state.drawn.replace(key) != key {
            state.cache.clear();
        }
        let scale = self.scale;
        let interactive = self.interactive;
        let geometry = state.cache.draw(renderer, bounds.size(), |frame: &mut Frame| {
            // No hardware path: bare panel, nothing to take hold of (a
            // patch there read as one more cut, and the grid as a
            // checkerboard).
            if !interactive {
                return;
            }
            // A pixel of panel round each cell, so the grid reads as keys
            // and cuts set into a plate, not one slab of boxes.
            let m = scale.max(1.0);
            let body = Rectangle::new(
                Point::new(m, m * 0.5),
                Size::new(bounds.width - 2.0 * m, bounds.height - 2.0 * m),
            );
            let radius = theme::RADIUS_XS * scale;
            let text_color = if !routed {
                draw_cut(frame, body, radius, scale, hovered);
                None
            } else {
                Some(draw_key(frame, body, radius, scale, lit, hovered, dragging))
            };
            if dragging {
                frame.stroke(
                    &rounded(body, radius),
                    Stroke::default().with_color(theme::accent()).with_width(1.0),
                );
            }
            if let (Some(label), Some(color)) = (&label, text_color) {
                frame.fill_text(canvas::Text {
                    content: label.clone(),
                    position: Point::new(body.center_x(), body.center_y()),
                    color,
                    // The readout face, as every value in Audra: the
                    // decimals line up down a column.
                    font: theme::MONO,
                    size: (theme::TEXT_SM * scale).into(),
                    align_x: advanced_text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        });
        if self.pulse <= 0.0 || !self.interactive {
            return vec![geometry];
        }
        // The pulse over the cell: a ring in the accent with a soft wash,
        // as bright as the pulse.
        let mut glow = Frame::new(renderer, bounds.size());
        let m = self.scale.max(1.0);
        let body = Rectangle::new(Point::new(m, m * 0.5), Size::new(bounds.width - 2.0 * m, bounds.height - 2.0 * m));
        let ring = rounded(body, theme::RADIUS_XS * self.scale);
        let p = self.pulse.clamp(0.0, 1.0);
        glow.fill(&ring, Color { a: 0.22 * p, ..theme::accent() });
        glow.stroke(&ring, Stroke::default().with_color(Color { a: 0.9 * p, ..theme::accent() }).with_width(1.5 * self.scale));
        vec![geometry, glow.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if !self.interactive {
            return mouse::Interaction::None;
        }
        if state.pressed || cursor.is_over(bounds) {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::None
        }
    }
}

pub fn crosspoint<'a, Message: Clone + 'a>(
    cell: Crosspoint<Message>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(cell)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unity_send_lights_in_metal_never_green_and_a_master_stays_quiet() {
        // (routed, unity, muted, phase, master)
        assert_eq!(lit_hue(true, true, false, false, false), Some(theme::fader_metal()));
        assert_ne!(lit_hue(true, true, false, false, false), Some(theme::signal()), "green means signal");
        assert_eq!(lit_hue(true, false, false, false, false), None, "any other gain: a key at rest");
        assert_eq!(lit_hue(true, true, false, false, true), None, "an output's own 0 dB is its rest");
        assert_eq!(lit_hue(true, true, true, false, true), Some(theme::mute_color()), "a muted master still says so");
        assert_eq!(lit_hue(true, false, false, true, false), Some(theme::clip()));
        assert_eq!(lit_hue(false, false, true, true, false), None, "unrouted: a cut, never lit");
    }

    #[test]
    fn cell_text_signs_gain_above_unity_and_blanks_unrouted() {
        assert_eq!(cell_text(0.0), None);
        assert_eq!(cell_text(1.0).as_deref(), Some("0.0"));
        assert_eq!(cell_text(2.0).as_deref(), Some("+6.0"));
        assert_eq!(cell_text(0.5).as_deref(), Some("-6.0"));
    }

    #[test]
    fn ctrl_click_toggles_between_off_and_unity() {
        assert_eq!(toggled(0.0), 1.0);
        assert_eq!(toggled(0.4), 0.0);
    }

    #[test]
    fn a_drag_needs_real_travel_to_sweep_the_range() {
        // A 20px move (one cell height) used to sweep everything.
        let t = dragged(0.5, -20.0, 1.0, false);
        assert!(t > 0.5 && t < 0.7, "20px up moves {t}");
        assert_eq!(dragged(0.5, -DRAG_TRAVEL, 1.0, false), 1.0);
        assert_eq!(dragged(0.5, DRAG_TRAVEL, 1.0, false), 0.0);
        // Fine drag moves much less for the same travel.
        assert!(dragged(0.5, -20.0, 1.0, true) - 0.5 < (t - 0.5) / 2.0);
    }
}
