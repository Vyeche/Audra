//! How the drawn controls (knobs, faders) answer the hand (2026-11-03, the
//! owner: "Ui should give more feedback to the user", "The animation should
//! be subtle and fun"):
//!
//! - **A glide.** A value that jumps from elsewhere (a snapshot, undo, a
//!   reset, a double-click) travels there like a motorised fader, over
//!   `GLIDE`, easing out. Small steps (a group or a remote surface following
//!   a drag) follow at once, so followers never trail the hand.
//! - **A splash.** A control landing on its home (0 dB, the centre, a
//!   reset) sends a ring out from itself, over `SPLASH`.
//! - **A landing.** A cap driven to a new level runs on past its mark and
//!   settles (`spring`, `Glide::overshoot_at`), as a motorised fader does;
//!   the same spring gives the detent's catch and a knob's end stop.
//! - **Sweat.** A fader held still in the hand for `SWEAT_AFTER` breaks
//!   into a sweat: a drop forms at the cap's shoulder and runs down, one
//!   side then the other, until it's let go.
//!
//! Each is short or tied to the hand, and "Animations" off in Audra
//! settings turns them all off (`widgets::animations`).

use std::time::{Duration, Instant};

use iced::widget::canvas::{self, Frame, Path};
use iced::{Color, Point, Radians};

use crate::motion::{ease_out, progress};
use crate::theme;

/// How long a jump takes to arrive.
pub const GLIDE: Duration = Duration::from_millis(260);
/// How long a small step takes (about one tick).
const FOLLOW: Duration = Duration::from_millis(50);
/// A change of more than this share of the travel is a jump.
pub const JUMP: f32 = 0.04;
/// How long a splash runs.
pub const SPLASH: Duration = Duration::from_millis(420);
/// How long a fader is held before it sweats.
pub const SWEAT_AFTER: Duration = Duration::from_secs(5);
/// One drop's life: forming, then running down.
const SWEAT_DROP: Duration = Duration::from_millis(1100);

/// A displayed value on its way to the real one.
#[derive(Debug, Clone, Copy)]
pub struct Glide {
    from: f32,
    to: f32,
    since: Instant,
    length: Duration,
}

impl Glide {
    /// Where the glide is going.
    pub fn target(&self) -> f32 {
        self.to
    }

    /// When a landing's overshoot starts (`spring`): 60% through the
    /// glide, so the cap runs on past its mark as it arrives rather than
    /// stopping and then twitching (the reviewer: a quartic ease-out stands
    /// still for its last 65ms).
    pub fn overshoot_at(&self) -> Instant {
        self.since + self.length.mul_f32(0.6)
    }

    /// Resting at `value`.
    pub fn still(value: f32) -> Self {
        Self { from: value, to: value, since: Instant::now(), length: Duration::ZERO }
    }

    /// Where it shows at `now`.
    pub fn at(&self, now: Instant) -> f32 {
        if self.length.is_zero() {
            return self.to;
        }
        self.from + (self.to - self.from) * ease_out(progress(self.since, self.length, now))
    }

    /// Whether it's still on its way (so the widget keeps redrawing).
    pub fn settling(&self, now: Instant) -> bool {
        !self.length.is_zero() && (self.to - self.from).abs() > f32::EPSILON && progress(self.since, self.length, now) < 1.0
    }

    /// Heads for `value` from wherever it shows now: a glide when `jump`
    /// (and animations are on), a quick follow otherwise.
    pub fn retarget(&mut self, value: f32, now: Instant, jump: bool) {
        if (self.to - value).abs() <= f32::EPSILON {
            return;
        }
        let from = self.at(now);
        let length = if jump && crate::widgets::animations() { GLIDE } else { FOLLOW };
        *self = Self { from, to: value, since: now, length };
    }
}

/// How long a landing takes to settle (`spring`).
pub const SPRING: Duration = Duration::from_millis(300);

/// The envelope's peak (`spring`): an `amp` shows as this much of it.
pub const SPRING_PEAK: f32 = 0.64;

/// A cap's landing (the owner, 2026-11-06: "the animations ... for fun
/// factor needs another pass"; critique #11: the fun belongs to the motor):
/// `amp` (at scale 1; positive is up) swinging past its place and back
/// twice and dying away, peaking at `SPRING_PEAK` of it, as a motorised
/// fader lands; also a knob's end stop and the detent's give. The picture
/// only: the level was set exactly. `None` once it has settled, or with
/// Animations off.
pub fn spring(release: Option<(Instant, f32)>, now: Instant) -> Option<f32> {
    let (at, amp) = release?;
    let p = progress(at, SPRING, now);
    if p >= 1.0 || !crate::widgets::animations() {
        return None;
    }
    Some(amp * (-4.0 * p).exp() * (p * std::f32::consts::TAU * 2.0).sin())
}

/// How far (0..1) a splash that started `at` has run, while it runs.
pub fn splash(at: Option<Instant>, now: Instant) -> Option<f32> {
    let at = at?;
    let p = progress(at, SPLASH, now);
    (p < 1.0 && crate::widgets::animations()).then_some(p)
}

/// Whether the hand has held a control long enough to sweat, and so the
/// widget must keep redrawing.
pub fn sweating(held_since: Option<Instant>, now: Instant) -> bool {
    crate::widgets::animations() && held_since.is_some_and(|at| now.saturating_duration_since(at) >= SWEAT_AFTER)
}

/// The sweat on a fader cap whose top-left corner is `cap` and size
/// `(w, h)`: one drop at a time, each forming at the cap's right shoulder
/// (the left one crowds the meter) and running down past its side as it
/// fades, the next a hair further out.
pub fn draw_sweat(frame: &mut Frame, held_since: Instant, now: Instant, cap: Point, (w, h): (f32, f32), scale: f32) {
    let since = now.saturating_duration_since(held_since + SWEAT_AFTER).as_secs_f32();
    let life = SWEAT_DROP.as_secs_f32();
    let n = (since / life).floor() as u32;
    // Three drops a hold, then it rests (critique #50: riding a level
    // through a take, it dripped on for as long as the fader was held).
    if n >= 3 {
        return;
    }
    let p = (since - n as f32 * life) / life;
    let further = if n.is_multiple_of(2) { 0.0 } else { 1.5 * scale };
    // Forming over the first quarter (swelling), then running down.
    let (grow, run) = if p < 0.25 { (p / 0.25, 0.0) } else { (1.0, (p - 0.25) / 0.75) };
    let fall = run * run * (h + 8.0 * scale);
    // Clear of the cap's side (the cap is drawn over it).
    let x = cap.x + w + 4.5 * scale + further;
    let y = cap.y + 1.0 * scale + fall;
    let alpha = (1.0 - run).powf(0.7);
    drop_at(frame, Point::new(x, y), (0.55 + 0.45 * grow) * scale, alpha);
}

/// A teardrop centred on `at`, `size` times its natural size.
fn drop_at(frame: &mut Frame, at: Point, size: f32, alpha: f32) {
    let r = 2.8 * size;
    let tip = Point::new(at.x, at.y - 4.4 * size);
    let centre = Point::new(at.x, at.y + 0.8 * size);
    // The round foot first: iced's `arc` starts a sub-path of its own, so
    // the point is drawn on from where the arc ends.
    let shape = Path::new(|b| {
        b.arc(canvas::path::Arc {
            center: centre,
            radius: r,
            start_angle: Radians(0.0),
            end_angle: Radians(std::f32::consts::PI),
        });
        b.line_to(tip);
        b.close();
    });
    // The room's ink, so a drop reads on any theme, with a glint.
    let ink = theme::text_primary();
    frame.fill(&shape, Color { a: 0.78 * alpha, ..ink });
    frame.fill(
        &Path::circle(Point::new(centre.x - 0.7 * size, centre.y - 0.4 * size), 0.7 * size),
        Color { a: 0.7 * alpha, ..theme::ink_on(ink) },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jump_glides_and_a_step_follows() {
        let now = Instant::now();
        let mut g = Glide::still(0.0);
        g.retarget(1.0, now, true);
        assert_eq!(g.length, GLIDE);
        let half = g.at(now + GLIDE / 2);
        assert!(half > 0.5 && half < 1.0, "eases out: most of the way at half time ({half})");
        assert_eq!(g.at(now + GLIDE), 1.0);
        assert!(!g.settling(now + GLIDE));

        g.retarget(1.01, now + GLIDE, false);
        assert_eq!(g.length, FOLLOW, "a small step follows at once");
    }

    #[test]
    fn a_new_target_starts_from_where_it_shows() {
        let now = Instant::now();
        let mut g = Glide::still(0.0);
        g.retarget(1.0, now, true);
        let mid = g.at(now + GLIDE / 4);
        g.retarget(0.0, now + GLIDE / 4, true);
        assert!((g.at(now + GLIDE / 4) - mid).abs() < 1e-6, "no jump when it turns round");
    }

    #[test]
    fn a_splash_runs_once() {
        let at = Instant::now();
        assert_eq!(splash(Some(at), at), Some(0.0));
        assert!(splash(Some(at), at + SPLASH).is_none());
        assert!(splash(None, at).is_none());
    }

    #[test]
    fn sweat_waits_for_a_long_hold() {
        let at = Instant::now();
        assert!(!sweating(Some(at), at + SWEAT_AFTER / 2));
        assert!(sweating(Some(at), at + SWEAT_AFTER));
        assert!(!sweating(None, at + SWEAT_AFTER));
    }
}
