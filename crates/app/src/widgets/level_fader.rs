//! The Main and Phones volume in the right-hand column (2026-11-03, the
//! owner: "the volume slider ... also needs a lot of attention"): the
//! strip fader laid on its side, in the same materials. An inset lane with
//! a slot down it, the two-part metal cap riding in it (its halves left and
//! right of a groove holding the coral position line), the 0 dB mark across
//! the slot. It answers the hand as a strip fader does: a press on the cap
//! takes hold of it, a press on the lane brings the cap there, Shift drags
//! finely, the wheel nudges, a double-click goes back to 0 dB; under the
//! cursor it comes forward, it glides on a jump, splashes at 0 dB and
//! sweats when held too long. Its meter (`fader::level_meter`) runs under
//! the cap's travel, so a level on the meter sits under the same level on
//! the slider.

use std::time::Instant;

use iced::keyboard::Modifiers;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::{mouse, window, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

use super::fader::{DOUBLE_CLICK, FINE_SENSITIVITY, SCROLL_IDLE};
use super::feel::{self, Glide};
use crate::theme;

/// The widget's height at scale 1.
const HEIGHT: f32 = 28.0;
/// The cap: along the travel, and across it.
const CAP_ALONG: f32 = 30.0;
const CAP_ACROSS: f32 = 18.0;
const CAP_RADIUS: f32 = 4.0;
/// The lane: wider than the cap by `LANE_PAD` each side, standing
/// `LANE_MARGIN` in from the widget's ends.
const LANE_PAD: f32 = 3.0;
const LANE_MARGIN: f32 = 1.0;
const LANE_RADIUS: f32 = 6.0;
const SLOT: f32 = 3.0;
/// How near the 0 dB mark (in px) a drag snaps to it.
const SNAP_PX: f32 = 3.0;

/// How far the lane stands in from each end; the meter's well under the
/// slider stands in the same, so the two line up (the owner, 2026-11-10:
/// the meter "doesnt line up with the zip slider").
pub fn lane_inset(scale: f32) -> f32 {
    LANE_MARGIN * scale
}

/// How far the travel stands in from each end: the lane's margin and
/// padding and half a cap. The meter under the slider stands in the same.
pub fn travel_inset(scale: f32) -> f32 {
    (LANE_MARGIN + LANE_PAD + CAP_ALONG / 2.0) * scale
}

pub struct LevelFader<Message> {
    /// The level, on the faders' taper (`vol_to_t`), 0 to 1.
    pub t: f32,
    /// Where a double-click goes (0 dB).
    pub default_t: f32,
    /// Dim is on: the position line is the lamp's.
    pub dimmed: bool,
    pub scale: f32,
    pub modifiers: Modifiers,
    /// A drag's steps.
    pub on_change: Box<dyn Fn(f32) -> Message>,
    /// A press on the lane, off the cap, bringing the cap there; and a
    /// double-click's reset. Apart from `on_change` so a click that turns
    /// a level far up can glide there (`app::set_level_gently`; the
    /// critique, 2026-11-05: the cap glided while the sound jumped).
    pub on_press: Box<dyn Fn(f32) -> Message>,
    pub on_reset: Message,
    pub on_release: Message,
}

#[derive(Default)]
pub struct State {
    cache: canvas::Cache,
    cache_key: std::cell::Cell<u64>,
    dragging: bool,
    last_click: Option<Instant>,
    drag_x: Option<f32>,
    drag_t: Option<f32>,
    scroll_t: Option<f32>,
    last_scroll: Option<Instant>,
    glide: Option<Glide>,
    splash: Option<Instant>,
    held_since: Option<Instant>,
}

impl<Message> LevelFader<Message> {
    /// The travel along `bounds`: where t = 0 sits, and its length.
    fn travel(&self, bounds: Rectangle) -> (f32, f32) {
        let inset = travel_inset(self.scale).min(bounds.width / 2.0);
        (bounds.x + inset, (bounds.width - 2.0 * inset).max(1.0))
    }

    /// The cap at `t`, in `bounds`' coordinates.
    fn cap(&self, bounds: Rectangle, t: f32) -> Rectangle {
        let (x0, len) = self.travel(bounds);
        let (along, across) = (CAP_ALONG * self.scale, CAP_ACROSS * self.scale);
        let cx = x0 + len * t.clamp(0.0, 1.0);
        Rectangle::new(
            Point::new(cx - along / 2.0, bounds.y + bounds.height / 2.0 - across / 2.0),
            Size::new(along, across),
        )
    }

    fn shown(&self, state: &State, now: Instant) -> f32 {
        if state.dragging {
            self.t
        } else {
            state.glide.map_or(self.t, |g| g.at(now))
        }
    }
}

impl<Message: Clone> canvas::Program<Message> for LevelFader<Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let (x0, len) = self.travel(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_over(bounds)?;
                let now = Instant::now();
                if state.last_click.is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK) {
                    state.last_click = None;
                    state.dragging = false;
                    state.held_since = None;
                    state.splash = Some(now);
                    return Some(canvas::Action::publish(self.on_reset.clone()).and_capture());
                }
                state.last_click = Some(now);
                state.dragging = true;
                state.held_since = Some(now);
                state.drag_x = Some(pos.x);
                // On the cap, it's taken hold of where it is; elsewhere on
                // the lane, the cap comes to the press.
                let on_cap = self.cap(bounds, self.t).expand(2.0 * self.scale).contains(pos);
                let t = if on_cap { self.t } else { ((pos.x - x0) / len).clamp(0.0, 1.0) };
                state.drag_t = Some(t);
                if on_cap {
                    Some(canvas::Action::capture())
                } else {
                    Some(canvas::Action::publish((self.on_press)(t)).and_capture())
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if !state.dragging {
                    return None;
                }
                let pos = cursor.land().position()?;
                let prev = state.drag_x.unwrap_or(pos.x);
                let mult = if self.modifiers.shift() { FINE_SENSITIVITY } else { 1.0 };
                let base = state.drag_t.unwrap_or(self.t);
                let mut t = (base + (pos.x - prev) / len * mult).clamp(0.0, 1.0);
                if (t - self.default_t).abs() < SNAP_PX * self.scale / len {
                    t = self.default_t;
                }
                if base != self.default_t && (base - self.default_t) * (t - self.default_t) <= 0.0 {
                    state.splash = Some(Instant::now());
                }
                state.drag_x = Some(pos.x);
                state.drag_t = Some(t);
                Some(canvas::Action::publish((self.on_change)(t)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if !state.dragging {
                    return None;
                }
                state.dragging = false;
                state.drag_x = None;
                state.drag_t = None;
                state.held_since = None;
                state.glide = Some(Glide::still(self.t));
                Some(canvas::Action::publish(self.on_release.clone()).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if !cursor.is_over(bounds) || crate::widgets::wheel_off() || self.modifiers.control() {
                    return None;
                }
                let (dy, step) = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => (*y, 0.03),
                    mouse::ScrollDelta::Pixels { y, .. } => (*y, 0.0015),
                };
                if dy == 0.0 {
                    return None;
                }
                let mult = if self.modifiers.shift() { 0.25 } else { 1.0 };
                let now = Instant::now();
                let fresh = state.last_scroll.is_none_or(|at| now.duration_since(at) > SCROLL_IDLE);
                let base = if fresh { self.t } else { state.scroll_t.unwrap_or(self.t) };
                let t = (base + dy * step * mult).clamp(0.0, 1.0);
                state.scroll_t = Some(t);
                state.last_scroll = Some(now);
                Some(canvas::Action::publish((self.on_change)(t)).and_capture())
            }
            canvas::Event::Window(window::Event::RedrawRequested(now)) => {
                let mut busy = feel::splash(state.splash, *now).is_some();
                if state.dragging {
                    busy |= feel::sweating(state.held_since, *now);
                } else {
                    let shown = state.glide.map_or(self.t, |g| g.at(*now));
                    let glide = state.glide.get_or_insert_with(|| Glide::still(self.t));
                    glide.retarget(self.t, *now, (self.t - shown).abs() > feel::JUMP);
                    busy |= glide.settling(*now);
                }
                busy.then(canvas::Action::request_redraw)
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
        let t = self.shown(state, now);
        let hovered = !crate::widgets::locked() && (state.dragging || cursor.is_over(bounds));
        let local = Rectangle::new(Point::ORIGIN, bounds.size());
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (t.to_bits(), self.default_t.to_bits(), self.scale.to_bits()).hash(&mut h);
            (bounds.width.to_bits(), bounds.height.to_bits()).hash(&mut h);
            (state.dragging, hovered, self.dimmed).hash(&mut h);
            theme::palette_key().hash(&mut h);
            h.finish()
        };
        if state.cache_key.replace(key) != key {
            state.cache.clear();
        }
        let still = state.cache.draw(renderer, bounds.size(), |frame| {
            self.draw_lane(frame, local, (state.dragging, hovered));
            self.draw_cap(frame, self.cap(local, t), (state.dragging, hovered));
        });
        let splash = feel::splash(state.splash, now);
        let sweat = state.dragging && feel::sweating(state.held_since, now);
        if splash.is_none() && !sweat {
            return vec![still];
        }
        let mut frame = Frame::new(renderer, bounds.size());
        let cap = self.cap(local, t);
        if let Some(p) = splash {
            let grow = 7.0 * self.scale * crate::motion::ease_out(p);
            frame.stroke(
                &Path::new(|b| {
                    b.rounded_rectangle(
                        Point::new(cap.x - grow, cap.y - grow),
                        Size::new(cap.width + 2.0 * grow, cap.height + 2.0 * grow),
                        (CAP_RADIUS * self.scale + grow).into(),
                    )
                }),
                Stroke::default()
                    .with_color(Color { a: 0.8 * (1.0 - p), ..theme::accent() })
                    .with_width((2.2 - 1.4 * p) * self.scale),
            );
        }
        if let (true, Some(held)) = (sweat, state.held_since) {
            feel::draw_sweat(&mut frame, held, now, cap.position(), (cap.width, cap.height), self.scale);
        }
        vec![still, frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.dragging {
            return if self.modifiers.shift() { mouse::Interaction::Crosshair } else { mouse::Interaction::Grabbing };
        }
        let Some(pos) = cursor.position_over(bounds) else {
            return mouse::Interaction::None;
        };
        if crate::widgets::locked() {
            mouse::Interaction::NotAllowed
        } else if self.cap(bounds, self.shown(state, Instant::now())).expand(2.0 * self.scale).contains(pos) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::Pointer
        }
    }
}

impl<Message> LevelFader<Message> {
    /// The lane, the slot and the 0 dB mark across it, as on a strip fader
    /// laid on its side (the light still falls from above).
    fn draw_lane(&self, frame: &mut Frame, r: Rectangle, (dragging, hovered): (bool, bool)) {
        let s = self.scale;
        let across = (CAP_ACROSS + 2.0 * LANE_PAD) * s;
        let (x, y, w) = (r.x + LANE_MARGIN * s, r.y + (r.height - across) / 2.0, r.width - 2.0 * LANE_MARGIN * s);
        let radius = (LANE_RADIUS * s).min(across / 2.0);
        super::fader::draw_inset(
            frame,
            Rectangle::new(Point::new(x, y), Size::new(w, across)),
            radius,
            s,
            (dragging || hovered).then(theme::fader_color),
        );
        let pad = (LANE_PAD + 3.0) * s;
        let (sx, sw, sh) = (x + pad, (w - 2.0 * pad).max(0.0), SLOT * s);
        let sy = r.y + r.height / 2.0 - sh / 2.0;
        let slot = Path::new(|b| b.rounded_rectangle(Point::new(sx, sy), Size::new(sw, sh), (sh / 2.0).into()));
        frame.fill(&slot, theme::slot());
        frame.stroke(&slot, Stroke::default().with_color(Color { a: 0.7, ..theme::control_edge() }).with_width(1.0));
        // 0 dB, across the slot.
        let (x0, len) = self.travel(r);
        let mark_x = x0 + len * self.default_t;
        let half = 6.0 * s;
        frame.stroke(
            &Path::line(Point::new(mark_x, r.y + r.height / 2.0 - half), Point::new(mark_x, r.y + r.height / 2.0 + half)),
            Stroke::default().with_color(theme::text_sec()).with_width(1.5),
        );
    }

    /// The cap on its side: two halves, left and right of a groove holding
    /// the position line (the lamp's while Dim is on), each lit from above,
    /// the ends rolling away, a dark hairline round it, on a soft shadow.
    fn draw_cap(&self, frame: &mut Frame, cap: Rectangle, (dragging, hovered): (bool, bool)) {
        let s = self.scale;
        let radius = CAP_RADIUS * s;
        let rounded = |x: f32, y: f32, w: f32, h: f32, r: f32| {
            Path::new(move |b| b.rounded_rectangle(Point::new(x, y), Size::new(w, h), r.into()))
        };
        frame.fill(
            &rounded(cap.x - 0.5 * s, cap.y + 1.5 * s, cap.width + s, cap.height + 0.5 * s, radius + 0.5 * s),
            Color { a: 0.32, ..Color::BLACK },
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
        let body = rounded(cap.x, cap.y, cap.width, cap.height, radius);
        frame.fill(&body, shade(0.5));
        let groove = 3.0 * s;
        let half_w = (cap.width - groove) / 2.0;
        for left in [cap.x, cap.x + half_w + groove] {
            let half = rounded(left, cap.y, half_w, cap.height, radius);
            frame.fill(
                &half,
                canvas::gradient::Linear::new(Point::new(left, cap.y), Point::new(left, cap.y + cap.height))
                    .add_stop(0.0, light(0.55))
                    .add_stop(0.14, light(0.18))
                    .add_stop(0.55, metal)
                    .add_stop(1.0, shade(0.3)),
            );
            frame.fill_rectangle(
                Point::new(left + radius * 0.6, cap.y + 1.0 * s),
                Size::new((half_w - 1.2 * radius).max(0.0), 1.0),
                Color { a: 0.5, ..Color::WHITE },
            );
        }
        frame.fill(
            &body,
            canvas::gradient::Linear::new(Point::new(cap.x, cap.y), Point::new(cap.x + cap.width, cap.y))
                .add_stop(0.0, Color { a: 0.18, ..Color::BLACK })
                .add_stop(0.15, Color { a: 0.0, ..Color::BLACK })
                .add_stop(0.85, Color { a: 0.0, ..Color::BLACK })
                .add_stop(1.0, Color { a: 0.22, ..Color::BLACK }),
        );
        let cx = cap.x + cap.width / 2.0;
        frame.fill_rectangle(
            Point::new(cx - 1.0 * s, cap.y + 1.5 * s),
            Size::new(2.0 * s, cap.height - 3.0 * s),
            if self.dimmed { theme::lamp() } else { theme::accent() },
        );
        frame.stroke(&body, Stroke::default().with_color(Color { a: 0.45, ..Color::BLACK }).with_width(1.0));
    }
}

/// A `LevelFader` as an element, the column's width.
pub fn level_fader<'a, Message: Clone + 'a>(fader: LevelFader<Message>) -> Element<'a, Message> {
    let h = HEIGHT * fader.scale;
    Canvas::new(fader).width(Length::Fill).height(Length::Fixed(h)).into()
}

#[cfg(test)]
mod tests {
    use super::super::fader::{t_to_vol, vol_to_t};
    use super::*;

    fn fader(t: f32) -> LevelFader<()> {
        LevelFader {
            t,
            default_t: vol_to_t(1.0),
            dimmed: false,
            scale: 1.0,
            modifiers: Modifiers::default(),
            on_change: Box::new(|_| ()),
            on_press: Box::new(|_| ()),
            on_reset: (),
            on_release: (),
        }
    }

    #[test]
    fn the_cap_runs_the_travel_end_to_end_inside_the_lane() {
        let f = fader(0.0);
        let r = Rectangle::new(Point::ORIGIN, Size::new(260.0, HEIGHT));
        let low = f.cap(r, 0.0);
        let high = f.cap(r, 1.0);
        assert!(low.x >= LANE_MARGIN + LANE_PAD - 0.01, "room before the cap at the bottom: {}", low.x);
        assert!(high.x + high.width <= r.width - LANE_MARGIN - LANE_PAD + 0.01, "and at the top");
        let (x0, len) = f.travel(r);
        assert!((low.center_x() - x0).abs() < 0.01 && (high.center_x() - (x0 + len)).abs() < 0.01);
    }

    #[test]
    fn the_taper_round_trips() {
        for v in [0.1f32, 0.5, 1.0, 1.5] {
            assert!((t_to_vol(vol_to_t(v)) - v).abs() < 1e-3, "{v}");
        }
    }
}
