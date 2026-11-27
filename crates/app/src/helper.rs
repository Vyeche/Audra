//! Audra's helper cat (the owner, 2026-11-25: "a cute animated
//! assistant"; 2026-11-26, from three cats: "the one with speaker is
//! good"; 2026-11-27: "the cat should always be there and walk around, have
//! lip sync", "make it work on all windows and the cat should move around
//! and be everywhere it wants", "the cat can be dragged too", "poked and
//! interacted, it can give tips, show badges").
//!
//! A grey tabby with its little speaker, drawn in code like the knobs and
//! meters, over the whole window in a layer of its own (`overlay`), under
//! the notices, panels and dialogs. It catches the pointer only on its own
//! body: every other press, drag, wheel and hover reaches the mixer under
//! it (checked with the probe: a mute beside it mutes, a wheel beside it
//! moves the fader).
//!
//! It rests only on perches, empty stretches the layout marks as it draws
//! them (`widgets::overhang::perch`): the top bar's empty middle and the
//! middle of each row's heading. Resting, it sits on its speaker; it walks
//! along a perch, and goes to another by walking off the end and popping up
//! there, so it never crosses the strips. Drag it anywhere: let go near a
//! perch it sits there, elsewhere it stays a while, then pops back. Poke it
//! and it wobbles and says a tip you haven't found yet, or shows your
//! badges. Its mouth moves while its words are new (lip sync) and sings
//! along with Main's level; it blinks, glances, hops when a card comes. With
//! Animations off it sits still (still draggable). Its own switch (Audra
//! settings → App, and the View page). Notes in docs/helper-character.md.

use iced::widget::canvas::{self, path, Frame, Geometry, Path, Stroke, Text};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use iced::{mouse, Color, Element, Length, Point, Radians, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::{Message, TuxMix};
use crate::theme;

/// The perch ids (`widgets::overhang::perch`).
pub const PERCH_TOP_BAR: u8 = 0;

/// A row heading's perch id, by the heading's name.
pub fn perch_of_heading(name: &str) -> u8 {
    match name {
        "Hardware inputs" => 1,
        "Software playback" => 2,
        "Hardware outputs" => 3,
        _ => 4,
    }
}

/// The roaming cat's size against its first drawing: about 54 px tall
/// walking at 100%, about 70 on its speaker (the owner, 2026-11-27: "the
/// cat is too small").
const SIZE: f32 = 1.15;

/// A pose: what the animation moves. 0 is at rest.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    /// 0 open, 1 shut.
    pub blink: f32,
    /// -1 looking left, 1 right.
    pub look: f32,
    /// 0 a small smile, 1 a big happy one.
    pub joy: f32,
    /// How far up a hop has it, in points (0 sitting).
    pub hop: f32,
    /// The woofer's push, 0 still to 1 the loudest.
    pub pulse: f32,
    /// The mouth open, 0 closed (its little "w") to 1 wide.
    pub mouth: f32,
    /// Facing left (drawn mirrored), when walking that way.
    pub mirror: bool,
    /// A walk's step, in radians (its paws and its bob); 0 standing.
    pub step: f32,
}

/// The cat's life: on which perch and where along it (0 to 1), the walk
/// under way, a perch it's leaving for, when it popped up, when it rests
/// until, where it's held or was dropped, when it was poked, and what it's
/// saying.
#[derive(Clone, Debug)]
pub struct CatState {
    perch: Option<u8>,
    x: f32,
    walk: Option<(f32, f32, Instant)>,
    leaving_for: Option<u8>,
    popped: Option<Instant>,
    next_walk: Option<Instant>,
    held: Option<Point>,
    dropped: Option<(Point, Instant)>,
    poked: Option<Instant>,
    pokes: u32,
    bubble: Option<(String, Instant)>,
}

impl Default for CatState {
    fn default() -> Self {
        CatState { perch: None, x: 0.5, walk: None, leaving_for: None, popped: None, next_walk: None, held: None, dropped: None, poked: None, pokes: 0, bubble: None }
    }
}

/// How long the bubble stays after a poke.
const BUBBLE: f32 = 6.0;
/// How long a pop takes when it appears on a perch.
const POP: f32 = 0.3;
/// How long the wobble takes when poked.
const WOBBLE: f32 = 0.6;
/// How long it stays where it was dropped off a perch before popping back.
const DROPPED_FOR: f32 = 10.0;

fn still(state: &TuxMix) -> bool {
    !state.animations || crate::app::CAPTURE_RUN.load(std::sync::atomic::Ordering::Relaxed)
}

/// The usable perches now: at least 90 px wide at scale 1 (a cat and room
/// to walk).
fn perches(state: &TuxMix) -> Vec<(u8, Rectangle)> {
    usable(state.ui_scale)
}

/// The perches drawn lately that are wide enough, without the inputs'
/// heading: right under the top bar, the cat would stand over its keys.
fn usable(s: f32) -> Vec<(u8, Rectangle)> {
    let min = 90.0 * s;
    crate::widgets::overhang::perches().into_iter().filter(|(id, r)| r.width >= min && *id != 1).collect()
}

/// Where the cat's feet stand on perch `r` at `x` (0 to 1 along it): the
/// foot of its row's empty stretch.
fn feet_on(r: Rectangle, x: f32, s: f32) -> Point {
    let margin = 20.0 * s;
    let span = (r.width - 2.0 * margin).max(0.0);
    // The top bar's stretch starts at the window's top: the cat stands a
    // little under the bar's foot there, so its ears aren't cut off.
    let foot = if r.y < 1.0 { r.y + r.height + 9.0 * s } else { r.y + r.height - 1.0 * s };
    Point::new(r.x + margin + x.clamp(0.0, 1.0) * span, foot)
}

/// How long a walk across `frac` of a perch `width` px wide takes: about
/// 70 px a second, at least 0.8 s.
fn walk_secs(frac: f32, width: f32, s: f32) -> f32 {
    (frac.abs() * width / (70.0 * s)).clamp(0.8, 6.0)
}

/// A number from 0 to 1 from the clock, for the cat's choices.
fn roll(now: Instant, salt: u64) -> f32 {
    hash01(now.saturating_duration_since(epoch()).as_millis() as u64 ^ salt)
}

/// On the tick: a walk ends where it was going (or off its perch's end, to
/// pop up on another), the next starts after a rest of 4 to 12 s, a dropped
/// cat pops back to a perch after a while. Nothing moves with Animations
/// off or in a capture; the cat still finds a perch to sit on.
pub fn tick(state: &mut TuxMix, now: Instant) {
    if !state.helper_cat {
        return;
    }
    let s = state.ui_scale;
    let here = perches(state);
    let mut cat = state.cat.clone();
    // A perch that went (a row folded): the cat pops up on another.
    if cat.perch.is_none_or(|p| !here.iter().any(|(id, _)| *id == p)) && cat.held.is_none() && cat.dropped.is_none() {
        cat.perch = here.first().map(|(id, _)| *id);
        cat.x = 0.5;
        cat.walk = None;
        cat.leaving_for = None;
        // Only when there's somewhere to pop up (Fable's review: with no
        // perch, a page with no headings, it popped every tick and kept the
        // tick fast for good).
        cat.popped = (!still(state) && cat.perch.is_some()).then_some(now);
    }
    if let Some((_, at)) = cat.dropped.filter(|_| still(state)) {
        // With Animations off it still goes back to a perch, without a pop.
        if now.saturating_duration_since(at).as_secs_f32() >= DROPPED_FOR {
            cat.dropped = None;
            cat.perch = here.first().map(|(id, _)| *id);
        }
    }
    if still(state) || cat.held.is_some() {
        cat.walk = None;
        cat.leaving_for = None;
        state.cat = cat;
        return;
    }
    if let Some((_, at)) = cat.dropped {
        if now.saturating_duration_since(at).as_secs_f32() >= DROPPED_FOR {
            cat.dropped = None;
            let pick = here.get((roll(now, 3) * here.len() as f32) as usize).or(here.first());
            cat.perch = pick.map(|(id, _)| *id);
            cat.x = roll(now, 4);
            cat.popped = Some(now);
        }
        state.cat = cat;
        return;
    }
    let width = cat.perch.and_then(|p| here.iter().find(|(id, _)| *id == p)).map_or(200.0, |(_, r)| r.width);
    if let Some((from, to, start)) = cat.walk {
        if now.saturating_duration_since(start).as_secs_f32() >= walk_secs(to - from, width, s) {
            cat.x = to;
            cat.walk = None;
            cat.next_walk = Some(now + Duration::from_secs_f32(4.0 + 8.0 * roll(now, 1)));
            // Off the end: up on the perch it was going to, at the end it
            // comes in from, and in a few steps.
            if let Some(next) = cat.leaving_for.take() {
                cat.perch = Some(next);
                cat.x = if to > 0.5 { 0.0 } else { 1.0 };
                cat.popped = Some(now);
                if state.ui_sounds {
                    crate::sounds::play(crate::sounds::Sound::Chirp, state.ui_sound_db - 6.0);
                }
                let inward = if cat.x == 0.0 { 0.2 + 0.5 * roll(now, 5) } else { 0.8 - 0.5 * roll(now, 5) };
                cat.walk = Some((cat.x, inward, now + Duration::from_secs_f32(POP)));
                cat.next_walk = None;
            }
        }
        state.cat = cat;
        return;
    }
    match cat.next_walk {
        None => cat.next_walk = Some(now + Duration::from_secs(3)),
        Some(at) if now >= at => {
            let others: Vec<u8> = here.iter().map(|(id, _)| *id).filter(|id| Some(*id) != cat.perch).collect();
            if !others.is_empty() && roll(now, 6) < 0.35 {
                // Off to another perch: to the nearer end of this one first.
                let end = if cat.x < 0.5 { 0.0 } else { 1.0 };
                cat.leaving_for = Some(others[(roll(now, 7) * others.len() as f32) as usize % others.len()]);
                cat.walk = Some((cat.x, end, now));
            } else {
                let to = (cat.x + 0.25 + 0.5 * roll(now, 8)) % 1.0;
                cat.walk = Some((cat.x, to, now));
            }
            cat.next_walk = None;
        }
        Some(_) => {}
    }
    state.cat = cat;
}

/// Whether anything of the cat moves now: the tick runs at its fast rate
/// only then (`app::tick_interval`): a walk, a pop, a wobble, words being
/// said, a drag.
pub fn moving(state: &TuxMix) -> bool {
    if !state.helper_cat || still(state) {
        return false;
    }
    let now = Instant::now();
    let fresh = |at: Option<Instant>, d: f32| at.is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() < d);
    let c = &state.cat;
    c.walk.is_some() || c.held.is_some() || fresh(c.popped, POP) || fresh(c.poked, WOBBLE) || fresh(c.bubble.as_ref().map(|b| b.1), TALK)
}

/// The pointer on the cat: a poke (a wobble and a tip or badges), a drag
/// (it follows), a drop (onto a perch if near one, else where it is).
pub fn on_pointer(state: &mut TuxMix, message: &Message) {
    let now = Instant::now();
    let s = state.ui_scale;
    match *message {
        Message::CatPoke => {
            state.cat.held = None;
            if state.ui_sounds {
                crate::sounds::play(crate::sounds::Sound::Meow, state.ui_sound_db);
            }
            state.cat.poked = Some(now);
            let words = poke_words(state, state.cat.pokes);
            state.cat.pokes = state.cat.pokes.wrapping_add(1);
            state.cat.bubble = Some((words, now));
        }
        Message::CatDrag(x, y) => {
            if state.cat.held.is_none() && state.ui_sounds {
                crate::sounds::play(crate::sounds::Sound::Purr, state.ui_sound_db);
            }
            state.cat.held = Some(Point::new(x, y));
            state.cat.walk = None;
            state.cat.leaving_for = None;
        }
        Message::CatDrop(x, y) => {
            state.cat.held = None;
            let at = Point::new(x, y);
            let near = perches(state).into_iter().find(|(_, r)| {
                let feet = feet_on(*r, 0.5, s);
                (at.y - feet.y).abs() < 30.0 * s && at.x >= r.x && at.x <= r.x + r.width
            });
            match near {
                Some((id, r)) => {
                    state.cat.perch = Some(id);
                    let margin = 20.0 * s;
                    state.cat.x = ((at.x - r.x - margin) / (r.width - 2.0 * margin).max(1.0)).clamp(0.0, 1.0);
                    state.cat.dropped = None;
                }
                None => state.cat.dropped = Some((at, now)),
            }
            state.cat.next_walk = Some(now + Duration::from_secs(5));
        }
        _ => {}
    }
}

/// What the cat says when poked: a tip you haven't found yet, then your
/// badges, in turn.
fn poke_words(state: &TuxMix, n: u32) -> String {
    if n.is_multiple_of(2) {
        let unfound: Vec<&crate::tricks::Trick> = crate::tricks::TRICKS.iter().filter(|t| !crate::tricks::found(state, t.id)).collect();
        if let Some(t) = unfound.get((n / 2) as usize % unfound.len().max(1)) {
            return format!("Tip: {}", t.words);
        }
    }
    let (m, of) = crate::celebrate::reached_count(state);
    let (t, tof) = crate::tricks::found_count(state);
    format!("Your badges: {m} of {of} milestones, {t} of {tof} tricks found")
}

/// Where the cat is, as the overlay is built: worked out into a point when
/// it's drawn (`Spot::feet`), after the perches it stands on have been
/// drawn that frame.
#[derive(Clone, Copy, Debug)]
enum Spot {
    /// At a point in the window (pinned for a capture, held, or dropped).
    At(Point),
    /// On a perch (the first one there is when none is chosen yet), at `x`,
    /// or walking from `x` to `to` for `secs` so far.
    Perch { id: Option<u8>, x: f32, walk: Option<(f32, f32)> },
}

impl Spot {
    /// The feet's point, whether walking, facing left, the step, and
    /// whether it sits on its speaker (not on the inputs' heading, where
    /// the speaker would lift it into the top bar's keys).
    fn feet(&self, s: f32) -> Option<(Point, bool, bool, f32, bool)> {
        match *self {
            Spot::At(p) => {
                let walking = pinned_walking();
                Some((p, walking, false, if walking { pinned_clock().unwrap_or(0.0) * 9.0 } else { 0.0 }, !walking))
            }
            Spot::Perch { id, x, walk } => {
                let here = usable(s);
                let (pid, r) = id.and_then(|p| here.iter().find(|(i, _)| *i == p)).or(here.first())?;
                // In the top bar it sits without its speaker: the bar is
                // only so tall.
                let speaker = *pid != PERCH_TOP_BAR;
                Some(match walk {
                    Some((to, secs)) => {
                        let k = (secs / walk_secs(to - x, r.width, s)).clamp(0.0, 1.0);
                        let e = k * k * (3.0 - 2.0 * k);
                        (feet_on(*r, x + (to - x) * e, s), true, to < x, secs * 9.0, false)
                    }
                    None => (feet_on(*r, x, s), false, false, 0.0, speaker),
                })
            }
        }
    }
}

/// The cat over the whole window, with what it's saying beside it.
pub fn overlay<'a>(state: &TuxMix) -> Element<'a, Message> {
    let now = Instant::now();
    let s = state.ui_scale;
    let c = &state.cat;
    let spot = if let Some(at) = pinned_at().or(c.held).or(c.dropped.map(|d| d.0)) {
        Spot::At(at)
    } else {
        let walk = c.walk.filter(|(_, _, start)| now >= *start).map(|(from, to, start)| (from, to, now.saturating_duration_since(start).as_secs_f32()));
        match walk {
            Some((from, to, secs)) => Spot::Perch { id: c.perch, x: from, walk: Some((to, secs)) },
            None => Spot::Perch { id: c.perch, x: c.x, walk: None },
        }
    };
    let mut pose = pose_now(state, now);
    let age = |at: Option<Instant>| at.map(|at| now.saturating_duration_since(at).as_secs_f32());
    if let Some(a) = age(c.poked).filter(|a| *a < WOBBLE) {
        pose.hop = hop_height(a * HOP / WOBBLE);
        pose.joy = 1.0;
    }
    let bubble = pinned_say().map(|w| (w, 0.3)).or_else(|| c.bubble.as_ref().filter(|(_, at)| now.saturating_duration_since(*at).as_secs_f32() < BUBBLE).map(|(w, at)| (w.clone(), now.saturating_duration_since(*at).as_secs_f32())));
    if let Some((_, a)) = &bubble {
        if *a < TALK {
            pose.mouth = pose.mouth.max(talk_mouth(*a));
        }
    }
    let pop = age(c.popped).map_or(1.0, |a| (a / POP).clamp(0.0, 1.0));
    canvas::Canvas::new(Roamer { spot, held_at: c.held, pose, pop, bubble, scale: s })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A capture's cat pinned in window pixels (`AUDRA_CAT_AT="x,y"`: where
/// its feet stand), for the pass-through test and pictures.
fn pinned_at() -> Option<Point> {
    static PIN: OnceLock<Option<Point>> = OnceLock::new();
    *PIN.get_or_init(|| {
        let v = std::env::var("AUDRA_CAT_AT").ok()?;
        let mut it = v.split(',').map(|p| p.trim().parse::<f32>().ok());
        Some(Point::new(it.next()??, it.next()??))
    })
}

/// A capture's pinned cat walking (`AUDRA_CAT_AT="x,y,1"`).
fn pinned_walking() -> bool {
    std::env::var("AUDRA_CAT_AT").ok().and_then(|v| v.split(',').nth(2).map(|w| w.trim() == "1")).unwrap_or(false)
}

/// A capture's pinned words in its bubble (`AUDRA_CAT_SAY`), said for a
/// second so its mouth is open.
fn pinned_say() -> Option<String> {
    std::env::var("AUDRA_CAT_SAY").ok().filter(|w| !w.is_empty())
}

/// A capture's pinned clock (`AUDRA_CAT="t"`, seconds), for its blinks.
fn pinned_clock() -> Option<f32> {
    static PIN: OnceLock<Option<f32>> = OnceLock::new();
    *PIN.get_or_init(|| std::env::var("AUDRA_CAT").ok()?.split(',').next()?.trim().parse().ok())
}

struct Roamer {
    spot: Spot,
    /// Where it was last held (a release outside the window lands there).
    held_at: Option<Point>,
    pose: Pose,
    /// How far it has popped up, 0 to 1.
    pop: f32,
    /// What it's saying, and for how long.
    bubble: Option<(String, f32)>,
    scale: f32,
}

/// What the pointer can catch: the cat's body and its speaker, the lower
/// 34 px above its feet. Its head may stand over the strips above a
/// heading; a press there goes on through to them, never to the cat.
fn cat_box(feet: Point, s: f32) -> Rectangle {
    Rectangle { x: feet.x - 22.0 * s, y: feet.y - 34.0 * s, width: 44.0 * s, height: 36.0 * s }
}

/// The pointer's hold on the cat: where it was pressed, and whether it has
/// moved far enough to be a drag.
#[derive(Default)]
pub struct Hold {
    from: Option<Point>,
    dragged: bool,
}

impl canvas::Program<Message> for Roamer {
    type State = Hold;

    fn update(&self, hold: &mut Hold, event: &canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let pos = cursor.position_in(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let (feet, pos) = (self.spot.feet(self.scale)?.0, pos?);
                if !cat_box(feet, self.scale).contains(pos) {
                    return None;
                }
                *hold = Hold { from: Some(pos), dragged: false };
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let from = hold.from?;
                let pos = cursor.land().position_in(bounds)?;
                if !hold.dragged && from.distance(pos) < 5.0 * self.scale {
                    return Some(canvas::Action::capture());
                }
                hold.dragged = true;
                // Held by its middle: the feet a little under the pointer.
                Some(canvas::Action::publish(Message::CatDrag(pos.x, pos.y + 18.0 * self.scale)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                hold.from?;
                let dragged = hold.dragged;
                *hold = Hold::default();
                // Let go outside the window: where it was last held.
                let at = cursor.land().position_in(bounds).unwrap_or_else(|| self.held_at.unwrap_or(Point::new(bounds.width / 2.0, bounds.height / 2.0)));
                Some(canvas::Action::publish(if dragged { Message::CatDrop(at.x, at.y + 18.0 * self.scale) } else { Message::CatPoke }).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, _hold: &Hold, renderer: &Renderer, _theme: &Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let Some((feet, walking, mirror, step, on_speaker)) = self.spot.feet(s) else { return vec![f.into_geometry()] };
        let pose = Pose { mirror, step, ..self.pose };
        let m = SIZE * self.pop.max(0.05);
        let line = Stroke::default().with_color(ink()).with_width(1.4 * s);
        let feet = Point::new(feet.x, feet.y - pose.hop * s);
        if walking || !on_speaker {
            let k = 0.78 * m;
            let bob = 1.0 * pose.step.sin().abs();
            let c = Point::new(feet.x, feet.y - 26.0 * k * s - bob * s);
            cat_walking_tail(&mut f, c, s * m, pose);
            cat(&mut f, c, s, k, pose, line, &grey_fur());
        } else {
            // Sitting on its speaker: the speaker's foot at the feet.
            let sm = s * m;
            let c = Point::new(feet.x, feet.y - 26.0 * sm);
            speaker_box(&mut f, c, sm, pose, line, true);
            let up = Point::new(c.x, c.y - 7.0 * sm);
            cat(&mut f, Point::new(up.x, up.y - 5.0 * sm), sm, 0.78, pose, line, &grey_fur());
        }
        if let Some((words, _)) = &self.bubble {
            bubble(&mut f, feet, words, s, bounds.size());
        }
        vec![f.into_geometry()]
    }

    fn mouse_interaction(&self, hold: &Hold, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if hold.dragged {
            return mouse::Interaction::Grabbing;
        }
        match (self.spot.feet(self.scale), cursor.position_in(bounds)) {
            (Some((feet, ..)), Some(pos)) if cat_box(feet, self.scale).contains(pos) => mouse::Interaction::Grab,
            _ => mouse::Interaction::None,
        }
    }
}

/// The speech bubble beside the cat: its words in lines of at most 34
/// letters, above it, kept inside the window.
fn bubble(f: &mut Frame, feet: Point, words: &str, s: f32, size: Size) {
    let mut lines: Vec<String> = Vec::new();
    for word in words.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= 34 => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    let lh = theme::TEXT_SM * 1.35 * s;
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * theme::TEXT_SM * 0.55 * s + 20.0 * s;
    let h = lines.len() as f32 * lh + 14.0 * s;
    let mut x = feet.x - w / 2.0;
    x = x.clamp(6.0 * s, (size.width - w - 6.0 * s).max(6.0 * s));
    let mut y = feet.y - 58.0 * s - h;
    let below = y < 4.0 * s;
    if below {
        y = feet.y + 10.0 * s;
    }
    let card = Path::rounded_rectangle(Point::new(x, y), Size::new(w, h), (8.0 * s).into());
    let tip_x = feet.x.clamp(x + 12.0 * s, x + w - 12.0 * s);
    let tail = Path::new(|b| {
        if below {
            b.move_to(Point::new(tip_x - 6.0 * s, y + 1.0));
            b.line_to(Point::new(tip_x, y - 8.0 * s));
            b.line_to(Point::new(tip_x + 6.0 * s, y + 1.0));
        } else {
            b.move_to(Point::new(tip_x - 6.0 * s, y + h - 1.0));
            b.line_to(Point::new(tip_x, y + h + 8.0 * s));
            b.line_to(Point::new(tip_x + 6.0 * s, y + h - 1.0));
        }
        b.close();
    });
    f.fill(&card, theme::surface());
    f.fill(&tail, theme::surface());
    f.stroke(&card, Stroke::default().with_color(theme::tip_card_edge()).with_width(theme::SPLIT_EDGE_W));
    for (i, l) in lines.iter().enumerate() {
        f.fill_text(Text {
            content: l.clone(),
            position: Point::new(x + 10.0 * s, y + 7.0 * s + i as f32 * lh),
            color: theme::text_primary(),
            size: (theme::TEXT_SM * s).into(),
            ..Text::default()
        });
    }
}

/// When the cat's clock started: its blinks and glances are worked out
/// from the time since, so they need no state of their own.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// A number from 0 to 1 that looks random but is the same for the same `n`.
fn hash01(n: u64) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    (x % 10_000) as f32 / 10_000.0
}

/// How long a blink keeps the eyes shut.
const BLINK: f32 = 0.18;
/// How long a hop takes when a card comes.
const HOP: f32 = 0.5;

/// Whether the eyes are shut `t` seconds into the cat's clock: one blink
/// somewhere in each 4 s, and now and then a second right after it, each
/// `length` long (a tick's worth when Audra redraws slowly, so it's seen).
pub fn blinking(t: f32, length: f32) -> bool {
    let n = (t / 4.0).floor().max(0.0) as u64;
    let start = n as f32 * 4.0 + 0.4 + 3.0 * hash01(n);
    let shut = |at: f32| t >= at && t < at + length;
    shut(start) || (hash01(n ^ 0x55) < 0.25 && shut(start + 0.32))
}

/// Where the eyes look `t` seconds in: mostly at the bubble, now and then
/// at you, now and then away, a new glance every 3 s.
pub fn glance(t: f32) -> f32 {
    let r = hash01((t / 3.0).floor().max(0.0) as u64 ^ 0xA5A5);
    if r < 0.55 {
        0.6
    } else if r < 0.85 {
        0.0
    } else {
        -0.4
    }
}

/// How long the cat talks when a card comes, in seconds.
const TALK: f32 = 2.4;

/// The mouth while talking, `age` seconds after its card came: syllables
/// about eight a second, fading at the end.
pub fn talk_mouth(age: f32) -> f32 {
    if !(0.0..TALK).contains(&age) {
        return 0.0;
    }
    let syllable = (age * 13.0).sin().abs();
    let fade = ((TALK - age) / 0.4).clamp(0.0, 1.0);
    (0.2 + 0.8 * syllable) * fade
}

/// The mouth singing along with the speakers' level (0 to 1): shut under a
/// quiet level, wide at a loud one.
pub fn sing_mouth(level: f32) -> f32 {
    ((level - 0.12) * 1.8).clamp(0.0, 1.0)
}

/// The hop's height `age` seconds after a card came (points, before scale).
pub fn hop_height(age: f32) -> f32 {
    if (0.0..HOP).contains(&age) {
        3.0 * (std::f32::consts::PI * age / HOP).sin()
    } else {
        0.0
    }
}

/// The pose now: still with Animations off (looking at its bubble).
fn pose_now(state: &TuxMix, now: Instant) -> Pose {
    // A milestone's card: the big smile; a trick just found: a wide one.
    let milestone = state.celebration.as_ref().filter(|_| state.milestone_cards);
    let joy = if milestone.is_some() {
        1.0
    } else if matches!(state.trick_card, Some((crate::tricks::Card::Found(_), _))) {
        0.7
    } else {
        0.35
    };
    let pin = pinned_clock();
    if pin.is_none() && (!state.animations || state.capture_burst_t.is_some() || crate::app::CAPTURE_RUN.load(std::sync::atomic::Ordering::Relaxed)) {
        return Pose { look: 0.6, joy, ..Pose::default() };
    }
    // In f64, wrapped, so the schedule keeps its precision over days.
    let t = pin.unwrap_or_else(|| (now.saturating_duration_since(epoch()).as_secs_f64() % 100_000.0) as f32);
    // The age of the card shown, for the hop: a milestone's ahead of a
    // trick's, which waits behind it re-stamped (Fable's review).
    let came = milestone.map(|c| c.at).or(state.trick_card.map(|(_, at)| at));
    let hop = came.map_or(0.0, |at| hop_height(now.saturating_duration_since(at).as_secs_f32()));
    let blink_len = BLINK.max(crate::app::tick_interval(state).as_secs_f32());
    let level = speakers_level(state);
    // Lip sync: talking while its card's words are new, else singing along
    // with what the speakers play.
    let age = came.map(|at| now.saturating_duration_since(at).as_secs_f32());
    let mouth = match age {
        Some(a) if a < TALK => talk_mouth(a),
        _ => sing_mouth(level),
    };
    Pose { blink: if blinking(t, blink_len) { 1.0 } else { 0.0 }, look: glance(t), joy, hop, pulse: level, mouth, ..Pose::default() }
}

/// The speakers' level now, 0 to 1: the louder side of Main.
fn speakers_level(state: &TuxMix) -> f32 {
    let main = state.control_room.assign.main;
    let ch = if state.device.outputs_one_per_pair() { main } else { main * 2 };
    let side = |i: usize| state.output_meters.get(i).map_or(0.0, |m| m.frame().value);
    let level = if state.device.outputs_one_per_pair() { side(ch) } else { side(ch).max(side(ch + 1)) };
    level.clamp(0.0, 1.0)
}

/// The sticker rim behind the cat's outline: light on every theme, so its
/// dark outline reads on a dark ground and the rim on a light one (the
/// owner, 2026-11-27: "the contrast is hard to see on all themes").
fn rim() -> Stroke<'static> {
    Stroke::default().with_color(Color { r: 0.98, g: 0.97, b: 0.95, a: 0.95 })
}

fn ink() -> Color {
    // The outline: dark on every theme, over the light sticker rim
    // (`rim`), so the cat reads on any ground.
    Color { r: 0.13, g: 0.12, b: 0.14, a: 0.95 }
}

/// How wide the sticker rim is, all told (it shows outside the outline).
fn rim_w(s: f32) -> f32 {
    5.5 * s
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

const METAL: Color = Color { r: 0.24, g: 0.25, b: 0.28, a: 1.0 };
const SILVER: Color = Color { r: 0.78, g: 0.80, b: 0.83, a: 1.0 };
const PUPIL: Color = Color { r: 0.10, g: 0.09, b: 0.11, a: 1.0 };


/// Two round eyes with a light in each, looking `look`, shut by `blink`.
fn eyes(f: &mut Frame, left: Point, right: Point, r: f32, pose: Pose, s: f32) {
    for at in [left, right] {
        if pose.blink > 0.6 {
            let shut = Path::new(|b| {
                b.move_to(Point::new(at.x - r, at.y));
                b.quadratic_curve_to(Point::new(at.x, at.y + r * 0.7), Point::new(at.x + r, at.y));
            });
            f.stroke(&shut, Stroke::default().with_color(PUPIL).with_width(1.8 * s));
            continue;
        }
        f.fill(&Path::circle(at, r), Color::WHITE);
        f.stroke(&Path::circle(at, r), Stroke::default().with_color(ink()).with_width(1.2 * s));
        let p = Point::new(at.x + pose.look * r * 0.35, at.y + r * 0.1);
        f.fill(&Path::circle(p, r * 0.58), PUPIL);
        f.fill(&Path::circle(Point::new(p.x - r * 0.2, p.y - r * 0.22), r * 0.2), Color::WHITE);
    }
}

fn cheeks(f: &mut Frame, left: Point, right: Point, s: f32) {
    let blush = Color { a: 0.45, ..theme::accent() };
    for at in [left, right] {
        f.fill(&Path::new(|b| b.ellipse(path::arc::Elliptical { center: at, radii: Vector::new(3.6 * s, 2.2 * s), rotation: Radians(0.0), start_angle: Radians(0.0), end_angle: Radians(std::f32::consts::TAU) })), blush);
    }
}

fn ellipse(at: Point, rx: f32, ry: f32) -> Path {
    Path::new(|b| b.ellipse(path::arc::Elliptical { center: at, radii: Vector::new(rx, ry), rotation: Radians(0.0), start_angle: Radians(0.0), end_angle: Radians(std::f32::consts::TAU) }))
}

const PINK: Color = Color { r: 0.95, g: 0.62, b: 0.66, a: 1.0 };
const WHITE_FUR: Color = Color { r: 0.97, g: 0.96, b: 0.94, a: 1.0 };

/// A cat's colours.
struct Fur {
    coat: Color,
    /// A white bib and muzzle (the tuxedo), if any.
    bib: bool,
    /// Tabby stripes on the head, if any.
    stripes: Option<Color>,
}

/// One cat: ears, head, body, paws and face, sitting with its centre at
/// `c`; `scale_by` shrinks it (the speaker's cat sits on its speaker). Its
/// tail is each cat's own, drawn before this.
fn cat(f: &mut Frame, c: Point, s: f32, k: f32, pose: Pose, line: Stroke, fur: &Fur) {
    let u = s * k;
    // Mirrored when it faces left (its look follows).
    let dir = if pose.mirror { -1.0 } else { 1.0 };
    let at = |x: f32, y: f32| Point::new(c.x + dir * x * u, c.y + y * u);
    let pose = Pose { look: pose.look * dir, ..pose };
    // The sticker rim first, round the whole silhouette: body, paws, ears,
    // head.
    let lift_of = |i: usize| if pose.step != 0.0 { 2.2 * (pose.step + i as f32 * std::f32::consts::PI).sin().max(0.0) } else { 0.0 };
    let ear_of = |side: f32| {
        Path::new(|b| {
            b.move_to(at(side * 15.5, -11.0));
            b.line_to(at(side * 13.0, -27.0));
            b.line_to(at(side * 3.5, -18.5));
            b.close();
        })
    };
    let halo = rim().with_width(rim_w(s)).with_line_join(canvas::LineJoin::Round);
    f.stroke(&ellipse(at(0.0, 12.0), 15.0 * u, 12.5 * u), halo);
    for (i, x) in [-6.5f32, 6.5].into_iter().enumerate() {
        f.stroke(&ellipse(at(x, 23.0 - lift_of(i)), 4.6 * u, 3.0 * u), halo);
    }
    for side in [-1.0f32, 1.0] {
        f.stroke(&ear_of(side), halo);
    }
    f.stroke(&ellipse(at(0.0, -6.0), 17.5 * u, 14.0 * u), halo);
    // The body, a sitting loaf, and the bib on it.
    let body = ellipse(at(0.0, 12.0), 15.0 * u, 12.5 * u);
    f.fill(&body, fur.coat);
    f.stroke(&body, line);
    if fur.bib {
        f.fill(&ellipse(at(0.0, 14.0), 8.0 * u, 9.0 * u), WHITE_FUR);
    }
    // The front paws.
    for (i, x) in [-6.5f32, 6.5].into_iter().enumerate() {
        // Walking, the paws step in turn.
        let lift = if pose.step != 0.0 { 2.2 * (pose.step + i as f32 * std::f32::consts::PI).sin().max(0.0) } else { 0.0 };
        let paw = ellipse(at(x, 23.0 - lift), 4.6 * u, 3.0 * u);
        f.fill(&paw, if fur.bib { WHITE_FUR } else { fur.coat });
        f.stroke(&paw, line);
    }
    // The ears, outer then inner, behind the head.
    for side in [-1.0f32, 1.0] {
        let ear = Path::new(|b| {
            b.move_to(at(side * 15.5, -11.0));
            b.line_to(at(side * 13.0, -27.0));
            b.line_to(at(side * 3.5, -18.5));
            b.close();
        });
        f.fill(&ear, fur.coat);
        f.stroke(&ear, line);
        let inner = Path::new(|b| {
            b.move_to(at(side * 13.5, -14.0));
            b.line_to(at(side * 12.3, -23.5));
            b.line_to(at(side * 6.5, -18.0));
            b.close();
        });
        f.fill(&inner, PINK);
    }
    // The head.
    let head = ellipse(at(0.0, -6.0), 17.5 * u, 14.0 * u);
    f.fill(&head, fur.coat);
    f.stroke(&head, line);
    if let Some(stripe) = fur.stripes {
        for (x, len) in [(-4.5, 5.0), (0.0, 6.5), (4.5, 5.0)] {
            f.stroke(&Path::line(at(x, -19.5), at(x * 0.8, -19.5 + len)), Stroke::default().with_color(stripe).with_width(2.0 * u).with_line_cap(canvas::LineCap::Round));
        }
    }
    if fur.bib {
        // The white muzzle.
        f.fill(&ellipse(at(0.0, 2.5), 8.5 * u, 5.5 * u), WHITE_FUR);
    }
    eyes(f, at(-7.0, -5.5), at(7.0, -5.5), 5.0 * u, pose, s);
    cheeks(f, at(-12.0, 1.5), at(12.0, 1.5), s);
    // The nose and the little "w" mouth.
    let nose = Path::new(|b| {
        b.move_to(at(-2.0, -0.5));
        b.line_to(at(2.0, -0.5));
        b.line_to(at(0.0, 1.6));
        b.close();
    });
    f.fill(&nose, PINK);
    if pose.mouth > 0.12 {
        // Open: talking or singing, a round mouth with a little tongue.
        let m = pose.mouth;
        let open = ellipse(at(0.0, 4.2 + 1.0 * m), (2.0 + 1.4 * m) * u, (1.2 + 2.6 * m) * u);
        f.fill(&open, PUPIL);
        f.fill(&ellipse(at(0.0, 5.4 + 2.0 * m), (1.4 + 0.8 * m) * u, (0.7 + 0.9 * m) * u), PINK);
    } else {
        let mouth = Path::new(|b| {
            let open = 1.0 + 0.8 * pose.joy;
            b.move_to(at(-3.6, 2.4));
            b.quadratic_curve_to(at(-1.8, 2.4 + 2.2 * open), at(0.0, 2.0));
            b.quadratic_curve_to(at(1.8, 2.4 + 2.2 * open), at(3.6, 2.4));
        });
        f.stroke(&mouth, Stroke::default().with_color(PUPIL).with_width(1.3 * s).with_line_cap(canvas::LineCap::Round));
    }
    // The whiskers.
    let whisker = Stroke::default().with_color(Color { a: 0.55, ..ink() }).with_width(0.9 * s);
    for side in [-1.0f32, 1.0] {
        for dy in [-1.0, 1.6] {
            f.stroke(&Path::line(at(side * 9.5, 1.0 + dy * 0.6), at(side * 21.0, dy * 2.2)), whisker);
        }
    }
}

const COAT: Color = Color { r: 0.62, g: 0.65, b: 0.70, a: 1.0 };

fn grey_fur() -> Fur {
    Fur { coat: COAT, bib: false, stripes: Some(mix(COAT, Color::BLACK, 0.25)) }
}

/// The cat's tail when it's out walking: up behind it, curled, on the side
/// it walks away from.
fn cat_walking_tail(f: &mut Frame, at: Point, s: f32, pose: Pose) {
    let dir = if pose.mirror { 1.0 } else { -1.0 };
    let u = s * 0.78;
    let sway = 2.0 * (pose.step * 0.5).sin();
    let tail = Path::new(|b| {
        b.move_to(Point::new(at.x + dir * 10.0 * u, at.y + 18.0 * u));
        b.bezier_curve_to(
            Point::new(at.x + dir * 24.0 * u, at.y + 16.0 * u),
            Point::new(at.x + dir * (24.0 + sway) * u, at.y - 4.0 * u),
            Point::new(at.x + dir * (18.0 + sway) * u, at.y - 8.0 * u),
        );
    });
    f.stroke(&tail, rim().with_width(6.6 * s + rim_w(1.0) * 0.8).with_line_cap(canvas::LineCap::Round));
    f.stroke(&tail, Stroke::default().with_color(ink()).with_width(6.6 * s).with_line_cap(canvas::LineCap::Round));
    f.stroke(&tail, Stroke::default().with_color(COAT).with_width(4.2 * s).with_line_cap(canvas::LineCap::Round));
}

/// The little speaker: the box, the woofer (pulsing with the level) and
/// the tweeter; with the cat's tail down its front when the cat's on it.
fn speaker_box(f: &mut Frame, c: Point, s: f32, pose: Pose, line: Stroke, tail_down: bool) {
    // Up a little: the speaker under the cat makes it the tallest.
    let c = Point::new(c.x, c.y - 7.0 * s);
    let coat = COAT;
    // The speaker, under the cat: the box, the woofer and the tweeter.
    let box_top = c.y + 16.0 * s;
    let cab = Path::rounded_rectangle(Point::new(c.x - 17.0 * s, box_top), Size::new(34.0 * s, 17.0 * s), (3.0 * s).into());
    f.stroke(&cab, rim().with_width(rim_w(s / SIZE.max(0.5) * 0.9)));
    f.fill(&cab, METAL);
    f.stroke(&cab, line);
    let woofer = Point::new(c.x + 5.0 * s, box_top + 8.5 * s);
    // The cone pushes out with the speakers' level.
    // Visible at 100% (Fable's review: under half a pixel before).
    f.fill(&Path::circle(woofer, 6.0 * s), mix(METAL, Color::BLACK, 0.4));
    f.stroke(&Path::circle(woofer, (6.0 + 0.9 * pose.pulse) * s), Stroke::default().with_color(SILVER).with_width(1.0 * s));
    f.fill(&Path::circle(woofer, 2.0 * s * (1.0 + 0.7 * pose.pulse)), SILVER);
    f.fill(&Path::circle(Point::new(c.x - 9.0 * s, box_top + 8.5 * s), 2.4 * s), SILVER);
    if !tail_down {
        return;
    }
    // The tail, down the speaker's front.
    let tail = Path::new(|b| {
        b.move_to(Point::new(c.x - 9.0 * s, c.y + 14.0 * s));
        b.bezier_curve_to(Point::new(c.x - 16.0 * s, c.y + 16.0 * s), Point::new(c.x - 18.0 * s, c.y + 24.0 * s), Point::new(c.x - 13.0 * s, c.y + 29.0 * s));
    });
    f.stroke(&tail, rim().with_width(6.6 * s + rim_w(1.0) * 0.8).with_line_cap(canvas::LineCap::Round));
    f.stroke(&tail, Stroke::default().with_color(ink()).with_width(6.6 * s).with_line_cap(canvas::LineCap::Round));
    f.stroke(&tail, Stroke::default().with_color(coat).with_width(4.2 * s).with_line_cap(canvas::LineCap::Round));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_blinks_about_once_every_four_seconds_and_briefly() {
        // In 400 s at 10 ms steps: a blink per 4 s window, a few doubled.
        let steps = 40_000;
        let shut = (0..steps).filter(|&i| blinking(i as f32 * 0.01, BLINK)).count() as f32 * 0.01;
        let blinks = 100.0;
        assert!(shut >= blinks * BLINK * 0.9 && shut <= blinks * BLINK * 1.4, "shut {shut} s in 400 s");
    }

    #[test]
    fn it_mostly_looks_at_its_bubble() {
        let at_bubble = (0..1000).filter(|&i| glance(i as f32 * 3.0) > 0.5).count();
        assert!((450..650).contains(&at_bubble), "{at_bubble} of 1000 glances at the bubble");
    }

    #[test]
    fn it_talks_then_stops_and_sings_only_with_sound() {
        assert!(talk_mouth(0.1) > 0.2 && talk_mouth(TALK + 0.1) == 0.0);
        assert!((0..24).any(|i| talk_mouth(i as f32 * 0.1) > 0.8), "it opens wide while talking");
        assert_eq!(sing_mouth(0.05), 0.0, "quiet: shut");
        assert!(sing_mouth(0.7) > 0.9, "loud: wide");
    }

    #[test]
    fn a_resting_cat_leaves_the_tick_slow() {
        let state = crate::app::new(true, None, None);
        assert!(!moving(&state), "no walk, no fast tick");
    }

    #[test]
    fn a_poke_says_a_tip_then_the_badges() {
        let mut state = crate::app::new(true, None, None);
        on_pointer(&mut state, &Message::CatPoke);
        assert!(state.cat.bubble.as_ref().is_some_and(|(w, _)| w.starts_with("Tip: ")));
        on_pointer(&mut state, &Message::CatPoke);
        assert!(state.cat.bubble.as_ref().is_some_and(|(w, _)| w.starts_with("Your badges:")));
    }

    #[test]
    fn a_drop_away_from_perches_stays_then_goes_back() {
        let mut state = crate::app::new(true, None, None);
        on_pointer(&mut state, &Message::CatDrag(400.0, 300.0));
        assert!(state.cat.held.is_some());
        on_pointer(&mut state, &Message::CatDrop(400.0, 300.0));
        assert!(state.cat.held.is_none() && state.cat.dropped.is_some(), "no perch drawn in a test: it stays where dropped");
    }

    #[test]
    fn only_a_press_on_the_cat_is_caught() {
        // Everything else passes through to the mixer under the layer (a
        // mute beside the cat still mutes: checked with the probe too).
        let roamer = Roamer { spot: Spot::At(Point::new(300.0, 200.0)), held_at: None, pose: Pose::default(), pop: 1.0, bubble: None, scale: 1.0 };
        let bounds = Rectangle { x: 0.0, y: 0.0, width: 1280.0, height: 725.0 };
        let press = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let mut hold = Hold::default();
        let beside = canvas::Program::<Message>::update(&roamer, &mut hold, &press, bounds, mouse::Cursor::Available(Point::new(360.0, 190.0)));
        assert!(beside.is_none(), "a press beside the cat isn't caught");
        let on = canvas::Program::<Message>::update(&roamer, &mut hold, &press, bounds, mouse::Cursor::Available(Point::new(300.0, 180.0)));
        assert!(on.is_some(), "a press on the cat is");
        let wheel = canvas::Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 } });
        let mut idle = Hold::default();
        assert!(canvas::Program::<Message>::update(&roamer, &mut idle, &wheel, bounds, mouse::Cursor::Available(Point::new(300.0, 180.0))).is_none(), "a wheel is never caught");
    }

    #[test]
    fn walks_take_their_time() {
        assert!((walk_secs(1.0, 140.0, 1.0) - 2.0).abs() < 1e-6);
        assert!((walk_secs(0.01, 140.0, 1.0) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_hop_lands() {
        assert_eq!(hop_height(-0.1), 0.0);
        assert!(hop_height(HOP / 2.0) > 2.9);
        assert_eq!(hop_height(HOP), 0.0);
    }
}
