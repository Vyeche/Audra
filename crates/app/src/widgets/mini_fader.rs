//! The small FX fader beside a strip's main one (the owner, 2026-11-06:
//! "the mini slider is missing with the nub on all of the controllers";
//! "the little bars sliders are send / returns from wet and dry signal").
//! TotalMix keeps the FX send (inputs, playback) or the FX return (outputs)
//! on the strip itself, so the effect's level is seen and set without
//! opening a panel (manual 21.3). A slot with a small cap riding in it,
//! filled up to the cap in the send's or the return's hue while it is on;
//! a drag moves it, Shift finely, the wheel nudges, a double-click turns it
//! off. Where the driver has no such level the slot stands empty and greyed,
//! and its tooltip (the caller's) says why.

use std::time::Instant;

use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use super::fader::{DOUBLE_CLICK, FINE_SENSITIVITY};
use crate::theme;

/// The widget's width at scale 1: the side keys' column, so the whole of
/// it takes the hand (the reviewer: a 12px target), the slot drawn in
/// its middle.
pub const WIDTH: f32 = 18.0;
/// The lane: a cut in the panel as the main fader's is, not a rimmed
/// line (the owner, 2026-11-13, asked for a design pass: the 4px rimmed
/// slot read as a scrollbar).
const SLOT_W: f32 = 6.0;
/// "FX" over the lane, where there is room for it: what the little fader
/// is, without a hover (the owner isn't an audio engineer).
pub(crate) const LABEL_H: f32 = 13.0;
/// From this tall, the label stands over the lane.
// 40: at 150% the mic strips' lanes (shorter by their 48V and gain line)
// fell under 50 and lost the label (critique #55).
pub(crate) const LABEL_FROM_H: f32 = 40.0;
/// The cap: a chunky round knob on the slot, as TotalMix's (the owner,
/// 2026-11-07: "too small. It should be a chunky round button").
const CAP_W: f32 = 16.0;
const CAP_H: f32 = 11.0;
/// A wheel notch's step along the travel.
const WHEEL_STEP: f32 = 0.04;

pub struct MiniFader<Message> {
    /// The level on the faders' taper (`fader::vol_to_t`), 0 (off) to 1.
    pub t: f32,
    /// The fill's hue while on: the send's or the return's.
    pub tint: Color,
    /// `None`: the driver has no such level here, the slot rests greyed.
    pub on_change: Option<Box<dyn Fn(f32) -> Message>>,
    /// A double-click: the level off.
    pub on_reset: Option<Message>,
    /// One step up from a travel position: where a press above the cap
    /// takes it (6 dB at most a click); none, a press there only takes the
    /// cap.
    pub rise_step: Option<fn(f32) -> f32>,
    pub height: f32,
    pub scale: f32,
    pub modifiers: Modifiers,
}

#[derive(Default)]
pub struct State {
    drag: Option<(f32, f32)>,
    last_click: Option<Instant>,
    /// A touchpad's small scroll steps, gathered until they make a notch.
    scroll: f32,
}

impl<Message> MiniFader<Message> {
    /// The travel: where t = 0 sits (its bottom) and its length.
    fn travel(&self, bounds: Rectangle) -> (f32, f32) {
        let inset = CAP_H * self.scale / 2.0 + 1.0;
        let label = self.label_h(bounds);
        (bounds.y + bounds.height - inset, (bounds.height - 2.0 * inset - label).max(1.0))
    }

    /// The room the "FX" label takes at the top, if it has room.
    fn label_h(&self, bounds: Rectangle) -> f32 {
        if bounds.height >= LABEL_FROM_H * self.scale { LABEL_H * self.scale } else { 0.0 }
    }
}

impl<Message: Clone> canvas::Program<Message> for MiniFader<Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let on_change = self.on_change.as_ref()?;
        let (y0, len) = self.travel(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_over(bounds)?;
                let now = Instant::now();
                if state.last_click.is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK) {
                    state.last_click = None;
                    state.drag = None;
                    return self.on_reset.clone().map(|m| canvas::Action::publish(m).and_capture());
                }
                state.last_click = Some(now);
                // On the cap it's taken where it is, and nothing is sent
                // until it moves (a click, or a double-click's first, never
                // nudges the level; the reviewer); on the slot the cap
                // comes to the press.
                let cap_y = y0 - len * self.t;
                if (pos.y - cap_y).abs() <= CAP_H * self.scale {
                    state.drag = Some((pos.y, self.t));
                    return Some(canvas::Action::capture());
                }
                let t = ((y0 - pos.y) / len).clamp(0.0, 1.0);
                // Above the cap a press never jumps the level up: it takes
                // the cap where it is, and a drag raises it by hand (the
                // 6 dB rule: a click put a send from off to 0 dB in one
                // write). Below, the level comes down at once.
                if t > self.t {
                    // One step toward the press, 6 dB at most (critique #49:
                    // a press there did nothing, and nothing said why).
                    let Some(up) = self.rise_step else {
                        state.drag = Some((pos.y, self.t));
                        return Some(canvas::Action::capture());
                    };
                    let to = up(self.t).min(t);
                    state.drag = Some((pos.y, to));
                    return Some(canvas::Action::publish(on_change(to)).and_capture());
                }
                state.drag = Some((pos.y, t));
                Some(canvas::Action::publish(on_change(t)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                let (from_y, from_t) = state.drag?;
                let sens = if self.modifiers.shift() { FINE_SENSITIVITY } else { 1.0 };
                let t = (from_t + (from_y - position.y) / len * sens).clamp(0.0, 1.0);
                state.drag = Some((position.y, t));
                Some(canvas::Action::publish(on_change(t)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take().map(|_| canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_over(bounds)?;
                // A wheel's notch is a step; a touchpad's pixels gather
                // into steps rather than each taking a whole one (the
                // reviewer: a flick ran the send to an end).
                let notches = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => {
                        state.scroll += *y / 40.0;
                        let whole = state.scroll.trunc();
                        state.scroll -= whole;
                        whole
                    }
                };
                if notches == 0.0 {
                    return Some(canvas::Action::capture());
                }
                let step = if self.modifiers.shift() { WHEEL_STEP * FINE_SENSITIVITY } else { WHEEL_STEP };
                let t = (self.t + notches * step).clamp(0.0, 1.0);
                Some(canvas::Action::publish(on_change(t)).and_capture())
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
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let live = self.on_change.is_some();
        let local = Rectangle::new(Point::ORIGIN, bounds.size());
        let (y0, len) = self.travel(local);
        // On a whole pixel, so the lane's 1 px rim draws crisp (critique
        // #48: on a half pixel it spread to two and read under 2:1).
        // The lane a whole number of pixels wide, its edges on half pixels,
        // so its 1 px rim draws crisp at every zoom (critique #49: at 125%
        // a 7.5 px lane put the left rim between pixels, 1.8:1).
        let slot_w = (SLOT_W * s).round();
        // Snapped on the screen's pixels, not the widget's: at 125% the
        // widget itself starts between pixels.
        let left = (bounds.x + local.width / 2.0 - slot_w / 2.0).floor() + 0.5 - bounds.x;
        let cx = left + slot_w / 2.0;
        let fade = |c: Color| if live { c } else { Color { a: c.a * 0.45, ..c } };
        // What it is, over the lane: "FX", quiet.
        let label = self.label_h(local);
        if label > 0.0 {
            frame.fill_text(canvas::Text {
                content: "FX".to_string(),
                position: Point::new(cx, label / 2.0),
                color: fade(theme::text_sec()),
                font: theme::SANS_SEMI,
                size: (10.0 * s).into(),
                align_x: iced::advanced::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        let lane = Rectangle::new(Point::new(cx - slot_w / 2.0, y0 - len - 2.0 * s), Size::new(slot_w, len + 4.0 * s));
        if !live {
            // No such level here: a flat, greyed slot (its tooltip says why).
            let slot = Path::rounded_rectangle(lane.position(), lane.size(), (slot_w / 2.0).into());
            frame.fill(&slot, fade(theme::slot()));
            return vec![frame.into_geometry()];
        }
        // Its rim at 3:1 on the strip's ground (critique #47: the inset's own
        // read 1.1 to 1.9:1, the lane lost on the contrast theme).
        super::fader::draw_inset(&mut frame, lane, slot_w / 2.0, s, Some(theme::resting_key_edge()));
        let t = self.t.clamp(0.0, 1.0);
        let cap_y = y0 - len * t;
        // 0 dB, a notch across the lane as the main fader's: where a send
        // is at unity, so the travel has a scale.
        let unity_y = y0 - len * super::fader::vol_to_t(1.0);
        // In the ink, and past the lane either side, so it reads on the lane
        // and on the ground (critique #47: 1.8 to 2.9:1 at 75% text_sec).
        frame.fill_rectangle(Point::new(cx - 6.0 * s, unity_y - 0.75 * s), Size::new(12.0 * s, 1.5 * s), theme::text_primary());
        // The level, filled up from the bottom of the lane to the cap.
        if t > 0.0 {
            let fill = Path::rounded_rectangle(
                Point::new(cx - slot_w / 2.0 + 1.0 * s, cap_y),
                Size::new(slot_w - 2.0 * s, y0 - cap_y + 1.0 * s),
                ((slot_w - 2.0 * s) / 2.0).into(),
            );
            frame.fill(&fill, Color { a: 0.9, ..self.tint });
        }
        // The cap: a chunky round metal knob, lit from above (a lighter top
        // half), its rim dark, a short centre line in the tint when on.
        let hot = state.drag.is_some() || cursor.is_over(bounds);
        let metal = if hot { theme::hovered(theme::fader_metal()) } else { theme::fader_metal() };
        let (cw, ch) = (CAP_W * s, CAP_H * s);
        let top_left = Point::new(cx - cw / 2.0, cap_y - ch / 2.0);
        // Its shadow, soft: wider and fainter rings a little below it.
        for (k, a) in [(0.0f32, 0.22f32), (1.0, 0.12), (2.0, 0.06)] {
            let e = k * s;
            let shadow = Path::rounded_rectangle(
                Point::new(top_left.x - e, top_left.y + (1.2 + 0.6 * k) * s - e),
                Size::new(cw + 2.0 * e, ch + 2.0 * e),
                (3.0 * s + e).into(),
            );
            frame.fill(&shadow, Color { a, ..Color::BLACK });
        }
        // The main fader's cap in small: squared off, two-tone, split by a
        // line across its whole width (critique #47: a pill with a dash
        // read as a minus button).
        let cap = Path::rounded_rectangle(top_left, Size::new(cw, ch), (3.0 * s).into());
        frame.fill(
            &cap,
            canvas::gradient::Linear::new(Point::new(cx, top_left.y), Point::new(cx, top_left.y + ch))
                .add_stop(0.0, theme::blend(metal, Color::WHITE, 0.3))
                .add_stop(0.5, theme::blend(metal, Color::WHITE, 0.05))
                .add_stop(0.55, theme::blend(metal, Color::BLACK, 0.1))
                .add_stop(1.0, theme::blend(metal, Color::BLACK, 0.3)),
        );
        frame.stroke(&cap, Stroke::default().with_color(theme::blend(metal, Color::BLACK, 0.5)).with_width(1.0 * s));
        // The grip line in the tint while on (the main cap's red line's
        // place), else engraved.
        let line = Path::line(Point::new(cx - cw / 2.0 + 1.0 * s, cap_y), Point::new(cx + cw / 2.0 - 1.0 * s, cap_y));
        let line_color = if t > 0.0 { self.tint } else { theme::blend(metal, Color::BLACK, 0.4) };
        frame.stroke(&line, Stroke::default().with_color(line_color).with_width(1.5 * s));
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if self.on_change.is_none() {
            mouse::Interaction::default()
        } else if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

pub fn mini_fader<'a, Message: Clone + 'a>(f: MiniFader<Message>) -> Element<'a, Message> {
    let (w, h) = (WIDTH * f.scale, f.height * f.scale);
    Canvas::new(f).width(Length::Fixed(w)).height(Length::Fixed(h)).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_travel_runs_bottom_up_inside_the_slot() {
        let f: MiniFader<()> = MiniFader {
            t: 0.0,
            tint: Color::WHITE,
            on_change: None,
            on_reset: None,
            rise_step: None,
            height: 80.0,
            scale: 1.0,
            modifiers: Modifiers::default(),
        };
        let (y0, len) = f.travel(Rectangle::new(Point::ORIGIN, Size::new(WIDTH, 80.0)));
        assert!(y0 > len && y0 <= 80.0, "t = 0 at the bottom: {y0}");
        assert!(y0 - len >= 0.0, "t = 1 inside the top: {}", y0 - len);
    }

    #[test]
    fn a_press_above_the_cap_never_jumps_the_level_up() {
        use canvas::Program;
        let f: MiniFader<f32> = MiniFader {
            t: 0.2,
            tint: Color::WHITE,
            on_change: Some(Box::new(|t| t)),
            on_reset: None,
            rise_step: None,
            height: 120.0,
            scale: 1.0,
            modifiers: Modifiers::default(),
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(WIDTH, 120.0));
        let (y0, len) = f.travel(bounds);
        let press = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        // Near the top, far above the cap: nothing sent, the cap taken.
        let mut state = State::default();
        let top = mouse::Cursor::Available(Point::new(WIDTH / 2.0, y0 - len * 0.95));
        let action = f.update(&mut state, &press, bounds, top);
        assert!(action.is_some_and(|a| a.into_inner().0.is_none()), "no level sent");
        assert_eq!(state.drag.map(|(_, t)| t), Some(0.2), "dragged from where it was");
        // Below the cap: down at once.
        let mut state = State::default();
        let low = mouse::Cursor::Available(Point::new(WIDTH / 2.0, y0 - len * 0.02));
        let action = f.update(&mut state, &press, bounds, low);
        let sent = action.and_then(|a| a.into_inner().0);
        assert!(sent.is_some_and(|t| t < 0.1), "down at once: {sent:?}");
        // With a step (the strips give one: 6 dB), a press above goes one
        // step toward it, never to it.
        let stepped = MiniFader { rise_step: Some(|t: f32| t + 0.1), ..f };
        let mut state = State::default();
        let action = stepped.update(&mut state, &press, bounds, top);
        let sent = action.and_then(|a| a.into_inner().0);
        assert!(sent.is_some_and(|t| (t - 0.3).abs() < 1e-6), "one step up: {sent:?}");
        // FX's own steps are 6 dB: from off and from a level.
        let db = |t: f32| crate::fx::t_send_db(t);
        assert!((db(crate::fx::send_t_up(0.0)) - -54.0).abs() < 0.5, "off to -54 dB");
        let t = crate::fx::send_t(-20.0);
        assert!((db(crate::fx::send_t_up(t)) - -14.0).abs() < 0.5, "-20 to -14 dB");
    }
}
