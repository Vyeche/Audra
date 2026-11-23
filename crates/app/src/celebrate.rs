//! Rewards for real wins (2026-11-05, the owner: "It should feel rewarding
//! to use it … Like the game bejeweled", and "The sounds should be
//! subtle").
//!
//! The thesis: shaping a mix should feel like a session going well. Each
//! real first (a milestone) lights the room: a burst of the lamp's light and
//! LED sparks round a lit lens, the card itself lit by the lamp as it lands,
//! a few warm words in Audra's own voice, a ladder of milestones that
//! relights in a cascade, the newest last, and three soft notes that start
//! a step higher with each win of the session (the chain, as in a match
//! game, in pitch, never in loudness). The last runs the meters' light
//! test across the desk once more, as a finale.
//!
//! The rules it keeps:
//! - **Real firsts only**, read from live state on the tick (as Quick reads
//!   its steps), never from a click: a snapshot stored, a mic preset saved,
//!   the reverb heard. Nothing celebrates an ordinary press.
//! - **No shower for the past**: the first run seeds the milestones already
//!   true, silently (`milestones_seeded`).
//! - **One card at a time, never over an error**: it waits behind a notice
//!   (as the tricks card does), and a trick's card waits behind it.
//! - **Switchable**: the cards with "Tips, tricks and milestones" (Audra
//!   settings → App), the sparks with Animations, the notes with their own
//!   "Celebration sounds" (and, like every UI sound, the top bar's
//!   speaker).

use std::time::{Duration, Instant};

use iced::widget::canvas::{self, Frame, Path};
use iced::widget::{button, column, container, mouse_area, row, text};
use iced::{Color, Element, Length, Point, Rectangle};

use crate::app::{Message, TuxMix};
use crate::theme;

/// A first worth a celebration: its id (kept in the prefs), its name, what
/// Audra says when it's reached, how to reach it (About lists them), and
/// the live check.
pub struct Milestone {
    pub id: &'static str,
    pub title: &'static str,
    pub praise: &'static str,
    pub how: &'static str,
    pub reached: fn(&TuxMix) -> bool,
}

pub const MILESTONES: [Milestone; 10] = [
    Milestone {
        id: "quick-all-set",
        title: "All set",
        praise: "Mic in, level set, and you hear it: ready when you are.",
        how: "Finish Quick setup's first three steps: your mic, its level, hear it.",
        // Seen happen, in Quick: a step ticked there this session and all
        // three are done (critique #8: it fired on a first launch for a
        // setup already made, read before the meters had moved).
        reached: |s| {
            s.view == crate::app::View::Quick
                && s.quick_rewarded.iter().any(|&r| r)
                && crate::quick::steps(s).iter().all(|st| *st == crate::quick::Step::Done)
        },
    },
    Milestone {
        id: "first-snapshot",
        title: "Your first snapshot",
        praise: "This mix is kept: Alt and its number bring it back.",
        how: "Store a snapshot: Store\u{2026}, then a slot.",
        reached: |s| s.stored_snapshots.iter().any(|&on| on),
    },
    Milestone {
        id: "mic-preset",
        title: "Your mic, remembered",
        praise: "Gain, EQ and all, back in one pick.",
        how: "Save a mic preset in a mic strip's Settings.",
        reached: |s| !s.mic_presets.is_empty(),
    },
    Milestone {
        id: "headphone-mix",
        title: "A mix in your ears",
        praise: "Your mic in the Phones: you hear yourself as you talk.",
        how: "Send your mic into the Phones mix, the Phones turned up.",
        reached: headphone_mix,
    },
    Milestone {
        id: "reverb-heard",
        title: "Reverb in your ears",
        praise: "Your voice in a room of its own.",
        how: "On Reverb & Echo, switch Reverb on and make it heard (needs the Babyface).",
        // The moment your hand made it audible (`fx::watch_heard`), on the
        // unit: "on and nothing blocking it" fired in the simulation, where
        // nothing is heard (OpenCode's job 27).
        reached: |s| s.fx_heard_at.is_some() && !s.device.is_mock(),
    },
    Milestone {
        // The whole chain, end to end: Voice on, and a call or streaming
        // app really recording from it (`dynamics_loop::AppWatch`, on the
        // unit). The owner, 2026-11-12: "keep improving the fun".
        id: "on-the-air",
        title: "On the air",
        praise: "Your call app hears you through Voice.",
        how: "Turn Voice on, then pick \u{201c}Audra processed mic\u{201d} as the mic in your call app (needs the Babyface).",
        reached: |s| s.dyn_heard == Some(true) && !s.device.is_mock(),
    },
    Milestone {
        // A take kept with Rec (issue #28), clean and 2 s or more, on the
        // unit (Rec is inert in the simulation). The owner, 2026-11-12:
        // "keep improving the fun, teaching users".
        id: "first-take",
        title: "Your first take",
        praise: "Recorded and kept: Audra settings \u{2192} App lists your takes.",
        how: "Press Rec, say something, then press Rec again (needs the Babyface).",
        reached: |s| s.take_kept,
    },
    Milestone {
        // A clean take that really held Reverb or Echo (the recorder's own
        // pair, `RecState::Saved::with_fx`), on the unit: your voice and
        // its room together (critique #64's idea).
        id: "first-take-room",
        title: "Room in your take",
        praise: "Your voice and its room, recorded together.",
        how: "Record a take with Reverb or Echo you can hear (needs the Babyface).",
        reached: |s| s.take_kept_room,
    },
    Milestone {
        id: "fx-preset",
        title: "A sound of your own",
        praise: "Your own Reverb or Echo, kept by name.",
        how: "Save a Reverb or Echo preset (Preset \u{25be}, Save as new).",
        reached: |s| !s.fx_presets.reverb.is_empty() || !s.fx_presets.echo.is_empty(),
    },
    Milestone {
        id: "all-tricks",
        title: "Every trick found",
        praise: "Every key and gesture: you play Audra by hand.",
        how: "Find every trick (Help's Keys tab).",
        reached: |s| {
            let (n, of) = crate::tricks::found_count(s);
            n == of
        },
    },
];

/// A mic input in the Phones mix, the Phones not all the way down.
fn headphone_mix(state: &TuxMix) -> bool {
    use tuxmix_core::{ChannelId, RmeDevice};
    let Some(&phones) = state.control_room.assign.phones_pairs().first() else {
        return false;
    };
    let mic_in = (0..2).any(|i| {
        let ch = &state.device.inputs()[i];
        // Above the app's -60 dB floor, so "you hear yourself" is true
        // (job 27: a send of -60 dB counted).
        !ch.mute && state.device.volume(ChannelId::Input(i), phones).unwrap_or(0.0) > 0.001
    });
    let ch = if state.device.outputs_one_per_pair() { phones } else { phones * 2 };
    mic_in && crate::control_room::user_output_level(state, ch) > 0.0
}

/// What is being celebrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reward {
    Milestone(&'static str),
}

/// The card on screen: what, since when, its notes' step in the session's
/// chain (tricks and milestones both climb it), and how many milestones
/// came before it in this run (its words and its burst grow with that).
#[derive(Clone, Copy, Debug)]
pub struct Celebration {
    pub reward: Reward,
    pub at: Instant,
    pub chain: u8,
    pub run: u8,
    /// Its notes have played. A card that waits behind a notice plays them
    /// when it shows (`expire`), not while hidden (the scout: "Your mic,
    /// remembered" rose behind the "saved" notice and showed silently).
    pub sounded: bool,
}

/// How long the card stays (while not pointed at), and how long its burst
/// flies.
pub const SHOW_FOR: Duration = Duration::from_secs(8);
pub const BURST_FOR: Duration = Duration::from_millis(1100);
/// After this long without a win the chain starts again: "two in a row"
/// means two close together, not two hours apart.
pub const CHAIN_RESETS_AFTER: Duration = Duration::from_secs(10 * 60);

/// The highest step a small win (a trick, a guide's or Quick's step)
/// chimes at, and the highest a milestone's rise starts on (the rises
/// rendered and measured go to 7: `sounds::render_ui_sounds`).
pub const SMALL_TOP: u8 = 5;
pub const RISE_TOP: u8 = 7;

/// A win's place in the session: its notes' step (tricks and milestones
/// both climb it) and, for a milestone, how many came before it in this
/// run (only milestones count there, so three tricks found don't make a
/// first milestone "four in a row"). Both start again after
/// `CHAIN_RESETS_AFTER` without a win.
pub fn next_win(state: &mut TuxMix, milestone: bool) -> (u8, u8) {
    if state.last_win.is_some_and(|at| at.elapsed() > CHAIN_RESETS_AFTER) {
        state.chain = 0;
        state.run = 0;
    }
    state.last_win = Some(Instant::now());
    // Counted in half steps: a small win (a trick, a guide's or Quick's
    // step) climbs half a step, a milestone lands a whole step above where
    // they've reached, so exploring never uses up the ladder before a real
    // first (critique #50: after a few tricks a milestone started at the
    // top and sounded no bigger than a trick).
    // Small wins stop at `SMALL_TOP` and milestones at `RISE_TOP`, so a
    // milestone always lands above the last small win's note: uncapped,
    // the fifteenth trick chimed where the next milestone (clamped to the
    // rise's top) started, and later tricks rang above every milestone.
    let step = match (milestone, state.chain) {
        (true, 0) => 0,
        // One above the last small win's note.
        (true, c) => ((c - 1) / 2 + 1).min(RISE_TOP),
        (false, c) => (c / 2).min(SMALL_TOP),
    };
    // A small win never lowers the chain: after a milestone near the top
    // it stays, so the next milestone still lands at or above the last
    // (Strata's review of job 57: one trick dropped the next milestone
    // from the top step to the one under it).
    let cap = 2 * SMALL_TOP + 1;
    state.chain = if milestone {
        (step + 1).saturating_mul(2)
    } else if state.chain >= cap {
        state.chain
    } else {
        state.chain + 1
    };
    let run = state.run;
    if milestone {
        state.run = state.run.saturating_add(1);
    }
    (step, run)
}

/// Whether a win's notes play: their own switch, and the top bar's speaker
/// that every UI sound answers to.
pub fn sounds_on(state: &TuxMix) -> bool {
    state.ui_sounds && state.celebration_sounds
}

fn milestone(id: &str) -> Option<&'static Milestone> {
    MILESTONES.iter().find(|m| m.id == id)
}

pub fn reached(state: &TuxMix, id: &str) -> bool {
    state.milestones.iter().any(|m| m == id)
}

pub fn reached_count(state: &TuxMix) -> (usize, usize) {
    (MILESTONES.iter().filter(|m| reached(state, m.id)).count(), MILESTONES.len())
}

/// On the tick: the first run seeds what is already true, silently; after
/// that, the first milestone newly true is kept and, with the cards on,
/// celebrated (one at a time: the next waits for the card to go).
pub fn check(state: &mut TuxMix) {
    if !state.milestones_seeded {
        state.milestones = MILESTONES.iter().filter(|m| (m.reached)(state)).map(|m| m.id.to_string()).collect();
        state.milestones_seeded = true;
        state.prefs_dirty = true;
        return;
    }
    // After the first run's seeding, which would replace what it keeps.
    first_take_seed(state);
    // One at a time, and never while a guide is under way: its card has the
    // window's foot, and its own notes are playing (`guides`).
    if state.celebration.is_some() || state.guide.is_some() {
        return;
    }
    let Some(m) = MILESTONES.iter().find(|m| !reached(state, m.id) && (m.reached)(state)) else {
        return;
    };
    state.milestones.push(m.id.to_string());
    state.prefs_dirty = true;
    // The last one: the day is kept for Help's finale line.
    if state.milestones_all_on.is_none() && reached_count(state).0 == MILESTONES.len() {
        state.milestones_all_on = crate::recorder::local_stamp().get(..10).map(str::to_string);
    }
    // Their own switch, apart from Tips (the owner, 2026-11-14).
    if state.milestone_cards {
        celebrate(state, Reward::Milestone(m.id));
    }
}

/// Once a session, on the unit: takes already in Rec's folder (kept before
/// this version, or by a take that ended as Audra quit) keep "Your first
/// take" quietly, as a first run keeps what is already true (OpenCode's job
/// 32: it fired for someone who had recorded for months). One look, which
/// stops at the first WAV.
fn first_take_seed(state: &mut TuxMix) {
    if state.takes_looked || state.device.is_mock() {
        return;
    }
    state.takes_looked = true;
    if reached(state, "first-take") {
        return;
    }
    if folder_has_takes(&crate::app::record_folder(state)) {
        state.milestones.push("first-take".into());
        state.prefs_dirty = true;
    }
}

/// Whether `dir` holds a WAV: looked at until the first one.
fn folder_has_takes(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut d| d.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))))
}

/// Puts the card up, its notes a step higher than the session's last. The
/// last of them all runs the meters' light test across the desk once more.
pub fn celebrate(state: &mut TuxMix, reward: Reward) {
    // A milestone takes the foot from a trick card still up or waiting,
    // and its chime: the 22nd trick's card came back for 7 s after "Every
    // trick found", its chime ringing late (the reviewer, 2026-11-14). The
    // trick stays found and listed in Help.
    if matches!(state.trick_card, Some((crate::tricks::Card::Found(_), _))) {
        state.trick_card = None;
        state.trick_chime_due = None;
    }
    let (chain, run) = next_win(state, true);
    let shows = state.notice.is_none();
    state.celebration = Some(Celebration { reward, at: Instant::now(), chain, run, sounded: shows });
    state.celebration_held = false;
    if shows && sounds_on(state) {
        crate::sounds::play(crate::sounds::Sound::Rise(chain.min(7)), state.ui_sound_db - 4.0);
    }
    if state.animations && all_reached(state) {
        state.boot_sweep = Some(Instant::now());
    }
}

/// A guide just finished (`guides::tick`): the milestones it reached are
/// kept now, quietly, and the first is named on the guide's own card, so a
/// win is said once, with one rise. The last still runs the light test.
pub fn fold_into_guide(state: &mut TuxMix) -> Option<&'static str> {
    if !state.milestones_seeded {
        return None;
    }
    let new: Vec<&'static str> = MILESTONES.iter().filter(|m| !reached(state, m.id) && (m.reached)(state)).map(|m| m.id).collect();
    for id in &new {
        state.milestones.push(id.to_string());
    }
    if new.is_empty() {
        return None;
    }
    state.prefs_dirty = true;
    if state.animations && all_reached(state) {
        state.boot_sweep = Some(Instant::now());
    }
    new.first().and_then(|id| milestone(id)).map(|m| m.title)
}

fn all_reached(state: &TuxMix) -> bool {
    let (n, of) = reached_count(state);
    n == of
}

/// On the tick: waiting behind a notice its time (and its burst) doesn't
/// run; then it goes once its time is up, unless pointed at.
/// How long a plain notice is read before a waiting milestone takes its
/// place (`expire`).
const MILESTONE_AFTER_NOTICE: Duration = Duration::from_millis(1500);

/// Whether the notice up is a plain confirmation (not kept for good, no
/// key on it) that has been up long enough to be read, so a reward card
/// waiting behind it may take its place (`expire`, `guides::tick`).
pub(crate) fn plain_notice_read(state: &TuxMix) -> bool {
    state.notice.as_ref().is_some_and(|(_, at)| at.elapsed() >= MILESTONE_AFTER_NOTICE && state.notice_sticky_at != Some(*at))
        && crate::app::notice_key(state).is_none()
}

/// A card's clock for "shown and settled": its burst over.
pub(crate) fn settled_at() -> Instant {
    Instant::now().checked_sub(BURST_FOR).unwrap_or_else(Instant::now)
}

pub fn expire(state: &mut TuxMix) {
    let on = sounds_on(state);
    let volume = state.ui_sound_db - 4.0;
    // A plain confirmation (not kept for good, no key on it) gives way to
    // a milestone not yet shown, once it has been up 1.5 s to be read
    // (critique #50: "Your mic, remembered" came 5 s after the save that
    // earned it, behind the save's own notice).
    if state.celebration.is_some_and(|c| !c.sounded) && plain_notice_read(state) {
        state.notice = None;
    }
    let Some(c) = state.celebration.as_mut() else { return };
    if state.notice.is_some() {
        // Not yet shown: its burst waits at the start. Shown already: held
        // settled, so a later notice doesn't replay the sparks, the ladder
        // and the count when it goes (the reviewer, 2026-11-14).
        c.at = if c.sounded { settled_at() } else { Instant::now() };
        return;
    }
    // Out from behind a notice: its notes now, with the card.
    if !c.sounded {
        c.sounded = true;
        if on {
            crate::sounds::play(crate::sounds::Sound::Rise(c.chain.min(7)), volume);
        }
    }
    if !state.celebration_held && c.at.elapsed() > SHOW_FOR {
        state.celebration = None;
    }
}

/// Whether its burst is flying (the fast timer keeps it moving, `motion`).
pub fn sparking(state: &TuxMix) -> bool {
    state.animations
        && state.notice.is_none()
        && state.celebration.is_some_and(|c| c.at.elapsed() < BURST_FOR)
}

/// A few words that grow with the milestones of this run, as a match
/// game's do, in Audra's calm voice; the last has its own.
fn chain_words(run: u8, all: bool) -> Option<&'static str> {
    if all {
        return Some("Every light on the desk.");
    }
    match run {
        0 => None,
        // Said as what happened (critique #57: "Two in a row" meant
        // nothing to a newcomer): wins close together, within minutes.
        1 => Some("Another one, close behind."),
        2 => Some("Three close together."),
        _ => Some("The room's coming alive."),
    }
}

/// The card at the window's foot (where the tricks card stands), its own
/// card rather than a tip's: the burst round a lit lens, the milestone in
/// the serif and the words that grow with the run, Audra's words for it,
/// the ladder of milestones relighting, a way to the list, and a close.
/// The card lands lit by the lamp (its rim, a wash and a halo of the
/// lamp's light) and keeps a little of that glow while it stands.
pub fn card(state: &TuxMix) -> Option<Element<'_, Message>> {
    if state.notice.is_some() {
        return None;
    }
    let c = state.celebration?;
    let Reward::Milestone(id) = c.reward;
    let m = milestone(id)?;
    let scale = state.ui_scale;
    let t = if !state.animations {
        1.0
    } else if let Some(t) = state.capture_burst_t {
        t
    } else {
        (c.at.elapsed().as_secs_f32() / BURST_FOR.as_secs_f32()).min(1.0)
    };
    let all = all_reached(state);
    let level = if all { 3 } else { c.run.min(3) };
    let badge = canvas::Canvas::new(Burst { t, scale, seed: c.chain, level })
        .width(Length::Fixed(BADGE * scale))
        .height(Length::Fixed(BADGE * scale));
    let lamp = theme::readable_on_floors(theme::lamp());
    // Where the room under a tall window's keys is short (`foot_compact`),
    // the finished guide card's size (critique #10 asked for more than a
    // line): the burst, growing with the run, drawn smaller so its sparks
    // stay in their box; the milestone with the run's words; then the
    // ladder and the count. Only the praise is left to Help.
    // On a short window, one line: a small lit lens, the milestone and
    // the run in a word, and its × (the owner's pick after critique #27:
    // in two rows it hid the page keys at 1280x700; every key stays in
    // view). The ladder and the praise are in Help.
    if crate::app::short_window(state) {
        let glow = 0.3 + 0.7 * (1.0 - t).powf(1.5);
        const LENS: f32 = 26.0;
        let lens = canvas::Canvas::new(Burst { t, scale: scale * LENS / 80.0, seed: c.chain, level })
            .width(Length::Fixed(LENS * scale))
            .height(Length::Fixed(LENS * scale));
        let run = if all {
            Some("every light on".to_string())
        } else {
            match c.run {
                0 => None,
                // Short, so the one-line card holds it (critique #67 A 3:
                // "2 close / together").
                1 | 2 => Some(format!("{} in a row", c.run + 1)),
                _ => Some("on a roll".to_string()),
            }
        };
        let mut spans = vec![iced::widget::span(m.title).font(theme::SERIF).size(theme::TEXT_MD * scale).color(lamp)];
        if let Some(run) = run {
            spans.push(iced::widget::span(no_orphan(&format!(" \u{b7} {run}"))).font(theme::SANS_SEMI).size(theme::TEXT_SM * scale).color(lamp));
        }
        let words = iced::widget::rich_text::<(), Message, iced::Theme, iced::Renderer>(spans).width(Length::Fill);
        let line = row![
            lens,
            words,
            crate::widgets::strip::hint(
                button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                    .padding(0)
                    .width(Length::Fixed(20.0 * scale))
                    .height(Length::Fixed(20.0 * scale))
                    .style(theme::icon_button(false))
                    .on_press(Message::CelebrationClose),
                "Close (Esc; Help lists every milestone)",
                scale,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center);
        let body = container(
            container(line)
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                .width(Length::Fill)
                .style(theme::milestone_card(glow)),
        )
        .style(theme::lamp_halo(glow));
        return Some(
            mouse_area(body)
                .on_enter(Message::CelebrationHeld(true))
                .on_exit(Message::CelebrationHeld(false))
                .into(),
        );
    }
    if crate::app::foot_compact(state) {
        let (n, of) = reached_count(state);
        let shown = if newest_lit(t) { n } else { n.saturating_sub(1) };
        let glow = 0.3 + 0.7 * (1.0 - t).powf(1.5);
        const LENS: f32 = 48.0;
        // Drawn at 48/80 rather than 48/76: at the top level the farthest
        // spark reached past the box by a pixel.
        let lens = canvas::Canvas::new(Burst { t, scale: scale * LENS / 80.0, seed: c.chain, level })
            .width(Length::Fixed(LENS * scale))
            .height(Length::Fixed(LENS * scale));
        let mut words = column![text(no_orphan(m.title)).font(theme::SERIF).size(theme::TEXT_ROW * scale).color(lamp)];
        if let Some(more) = chain_words(c.run, all) {
            words = words.push(text(no_orphan(more)).font(theme::SANS_SEMI).size((theme::TEXT_SM + f32::from(level.min(2))) * scale).color(lamp));
        }
        let head = row![
            lens,
            container(words).width(Length::Fill),
            crate::widgets::strip::hint(
                button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                    .padding(0)
                    .width(Length::Fixed(20.0 * scale))
                    .height(Length::Fixed(20.0 * scale))
                    .style(theme::icon_button(false))
                    .on_press(Message::CelebrationClose),
                "Close (Esc; Help lists every milestone)",
                scale,
            ),
        ]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center);
        let foot = row![
            ladder(state, Some(id), t, scale),
            text(format!("{shown} of {of}")).size(theme::TEXT_SM * scale).color(theme::text_sec()),
        ]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Center);
        let body = container(
            container(column![head, foot].spacing(theme::SPACE_TIGHT * scale))
                .padding([theme::SPACE_TIGHT * scale, theme::SPACE_SM * scale])
                .width(Length::Fill)
                .style(theme::milestone_card(glow)),
        )
        .style(theme::lamp_halo(glow));
        return Some(
            mouse_area(body)
                .on_enter(Message::CelebrationHeld(true))
                .on_exit(Message::CelebrationHeld(false))
                .into(),
        );
    }
    // Laid out for the counter's slot (`app::foot_card`): the burst beside
    // the milestone and the words for the run, Audra's words under them,
    // then the ladder, the count and the way to the list.
    let title = column![text(no_orphan(m.title)).font(theme::SERIF).size(theme::TEXT_ROW * scale).color(lamp)].spacing(theme::SPACE_TIGHT * scale);
    // The run's words on a line of their own under the badge, from the
    // card's edge as the praise is: beside the badge they broke mid-phrase
    // ("Another one, / close behind.") and the card had three left edges
    // (critique #91 A P3-8).
    let run_line: Option<Element<'_, Message>> = chain_words(c.run, all).map(|more| {
        // A step bigger with each win of the run.
        let size = theme::TEXT_SM + f32::from(level.min(2));
        text(no_orphan(more)).font(theme::SANS_SEMI).size(size * scale).color(lamp).into()
    });
    let close = crate::widgets::strip::hint(
        button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
            .padding(0)
            .width(Length::Fixed(20.0 * scale))
            .height(Length::Fixed(20.0 * scale))
            .style(theme::icon_button(false))
            .on_press(Message::CelebrationClose),
        "Close (Esc; milestones can be switched off in Audra settings \u{2192} App)",
        scale,
    );
    // The × at the top right, where the trick and guide cards keep theirs.
    let head = row![
        badge,
        container(title).width(Length::Fill).height(Length::Fixed(BADGE * scale)).align_y(iced::Alignment::Center),
        close,
    ]
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Start);
    // The count takes the newest only once its segment lights.
    let (n, of) = reached_count(state);
    let shown = if newest_lit(t) { n } else { n.saturating_sub(1) };
    let foot = row![
        ladder(state, Some(id), t, scale),
        text(format!("{shown} of {of}")).size(theme::TEXT_SM * scale).color(theme::text_sec()),
        iced::widget::Space::new().width(Length::Fill),
        button(text("Milestones").size(theme::TEXT_SM * scale))
            .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
            .style(theme::page_key(11.0 * scale))
            .on_press(Message::ToggleAbout),
    ]
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Center);
    // The lamp's light on arrival, settling to a standing glow.
    let glow = 0.3 + 0.7 * (1.0 - t).powf(1.5);
    let body = container(
        container(
            {
                let mut rows = column![head].spacing(theme::SPACE_SM * scale);
                if let Some(run_line) = run_line {
                    rows = rows.push(run_line);
                }
                rows.push(text(crate::guides::voiced(state, m.praise)).size(theme::TEXT_SM * scale).color(theme::text_primary())).push(foot)
            },
        )
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .width(Length::Fill)
        .style(theme::milestone_card(glow)),
    )
    .style(theme::lamp_halo(glow));
    Some(
        mouse_area(body)
            .on_enter(Message::CelebrationHeld(true))
            .on_exit(Message::CelebrationHeld(false))
            .into(),
    )
}

/// The badge's box, at scale 1: room for the burst round it.
pub(crate) const BADGE: f32 = 76.0;

/// A stable 0..1 from a spark's index and the burst's seed, so each spark
/// keeps its own reach and speed from frame to frame.
fn jitter(i: u32, seed: u8) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B1) ^ u32::from(seed).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// The burst: a lit LED lens (the keys' material: the lamp's hue lit from
/// above, a bright lip, a darker rim, its light round it) that swells and
/// settles, and the meters' own squares flying out from it, green near and
/// amber far as a meter's ladder runs, each at its own reach and speed,
/// fading. More of them, and further, with each milestone of the run
/// (`level` 0..=3). `t` 0..1 through `BURST_FOR`; at 1 only the lens stays.
pub(crate) struct Burst {
    pub t: f32,
    pub scale: f32,
    pub seed: u8,
    pub level: u8,
}

impl<M> canvas::Program<M> for Burst {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: iced::mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let t = self.t.clamp(0.0, 1.0);
        let out = 1.0 - (1.0 - t).powi(3);
        let fade = (1.0 - t).powf(1.4);
        let spark_fade = (1.0 - t).powf(0.9);
        let lamp = theme::lamp();
        let dark = theme::dark_theme();
        // On a light floor a pale spark reads as confetti: there they take
        // the hue's readable depth.
        let weigh = |hue: Color| if dark { hue } else { theme::readable_on_floors(hue) };
        let (near, far) = (weigh(theme::signal()), weigh(theme::attention()));
        let level = f32::from(self.level.min(3));
        if t < 1.0 {
            // The lamp's light opening under the lens and closing again.
            frame.fill(&Path::circle(c, (12.0 + 18.0 * out) * s), Color { a: 0.24 * fade, ..lamp });
            let n = 18 + 6 * u32::from(self.level.min(3));
            let span = 22.0 + 2.0 * level;
            let turn = f32::from(self.seed) * 0.37;
            for i in 0..n {
                // Its reach, scattered over the whole span (a burst, not a
                // ring), and its own pace.
                let h = jitter(i, self.seed);
                let pace = 0.7 + 0.6 * jitter(i + 97, self.seed);
                let a = turn + (i as f32 + 0.8 * jitter(i + 31, self.seed)) * std::f32::consts::TAU / n as f32;
                let o = 1.0 - (1.0 - (t * pace).min(1.0)).powi(3);
                let along = |k: f32| {
                    let reach = (11.0 + span * (0.2 + 0.8 * h) * k) * s;
                    Point::new(c.x + a.cos() * reach, c.y + a.sin() * reach)
                };
                let w = (2.6 + 1.8 * (1.0 - h)) * (1.0 - 0.35 * t) * s;
                let hue = if h > 0.5 { far } else { near };
                let square = |p: Point, w: f32| Path::rectangle(Point::new(p.x - w / 2.0, p.y - w / 2.0), iced::Size::new(w, w));
                // A dimmer square behind it, where it was a moment ago: an
                // LED trail.
                frame.fill(&square(along(o * 0.72), w * 0.7), Color { a: 0.4 * spark_fade, ..hue });
                frame.fill(&square(along(o), w), Color { a: spark_fade, ..hue });
            }
        }
        // The lens: lit, swelling on arrival, then still.
        let swell = 1.0 + 0.25 * (1.0 - t) * (1.0 - t);
        let r = 9.5 * swell * s;
        frame.fill(&Path::circle(c, r + 6.0 * s), Color { a: 0.16 + 0.2 * (1.0 - t), ..lamp });
        let lip = theme::blend(lamp, Color::WHITE, 0.4 + 0.3 * (1.0 - t));
        let top = theme::blend(lamp, Color::WHITE, 0.16);
        let foot = theme::blend(lamp, Color::BLACK, 0.10);
        frame.fill(
            &Path::circle(c, r),
            canvas::gradient::Linear::new(Point::new(c.x, c.y - r), Point::new(c.x, c.y + r))
                .add_stop(0.0, lip)
                .add_stop(0.22, top)
                .add_stop(1.0, foot),
        );
        frame.stroke(
            &Path::circle(c, r),
            canvas::Stroke::default().with_width(1.2 * s).with_color(theme::blend(lamp, Color::BLACK, 0.4)),
        );
        vec![frame.into_geometry()]
    }
}

/// A small win's sparks on its strip (`motion::win_spark`): the burst's
/// squares, fewer and smaller, flying up and out from the strip's head and
/// falling back a little as they fade, green near and amber far as a
/// meter's ladder runs. No lens and no glow: the lamp's glow on a strip
/// says it feeds the mix (critique #57). `t` 0..1, `None`: nothing drawn.
pub(crate) struct StripSparks {
    pub t: Option<f32>,
    pub scale: f32,
}

/// How many sparks fly, and the head's height they start from (at scale 1).
const STRIP_SPARKS: u32 = 16;
const STRIP_SPARK_FROM: f32 = 34.0;

impl<M> canvas::Program<M> for StripSparks {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: iced::mouse::Cursor) -> Vec<canvas::Geometry> {
        let Some(t) = self.t.filter(|t| *t < 1.0) else { return Vec::new() };
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        let c = Point::new(bounds.width / 2.0, STRIP_SPARK_FROM * s);
        let dark = theme::dark_theme();
        let weigh = |hue: Color| if dark { hue } else { theme::readable_on_floors(hue) };
        let (near, far) = (weigh(theme::signal()), weigh(theme::attention()));
        let fade = (1.0 - t).powf(0.9);
        // Within the strip, however narrow (Strata's review: a floor of
        // 8 px could reach past a sliver of a strip).
        let reach_x = (bounds.width / 2.0 - 4.0 * s).max(0.0);
        for i in 0..STRIP_SPARKS {
            let h = jitter(i, 7);
            let pace = 0.75 + 0.5 * jitter(i + 97, 7);
            let o = 1.0 - (1.0 - (t * pace).min(1.0)).powi(3);
            // Spread across the head, up and out, then a little gravity.
            let side = (i as f32 + 0.5) / STRIP_SPARKS as f32 * 2.0 - 1.0;
            let at = |k: f32| {
                let x = c.x + side * reach_x * (0.55 + 0.45 * h) * k;
                let y = c.y - (8.0 + 24.0 * h) * s * k + 26.0 * s * k * k * t;
                Point::new(x, y.max(2.0 * s))
            };
            // Bigger as they burst, so on a light floor they read as
            // sparks, not dust over the name (critique #58).
            let w = (3.6 + 2.2 * (1.0 - h)) * (1.4 - 0.75 * t.min(0.5) - 0.3 * t) * s;
            let hue = if h > 0.5 { far } else { near };
            let square = |p: Point, w: f32| Path::rectangle(Point::new(p.x - w / 2.0, p.y - w / 2.0), iced::Size::new(w, w));
            frame.fill(&square(at(o * 0.72), w * 0.7), Color { a: 0.4 * fade, ..hue });
            frame.fill(&square(at(o), w), Color { a: fade, ..hue });
        }
        vec![frame.into_geometry()]
    }
}

/// When, in ms into the burst, the ladder's reached segments relight (one
/// after another, Store's sweep: each over 140ms, the next 28ms later),
/// and when the newest lights, last, in the lamp, with the toggles' 220ms
/// bloom.
pub(crate) const CASCADE_FROM_MS: f32 = 120.0;
pub(crate) const SEG_STAGGER_MS: f32 = 28.0;
pub(crate) const SEG_LIGHT_MS: f32 = 140.0;
// With the rise's top note (its notes end ~350 ms after the wait; critique
// #50: at 520 ms the eye's climax came after the ear's).
pub(crate) const NEWEST_AT_MS: f32 = 340.0;
pub(crate) const BLOOM_MS: f32 = 220.0;

pub(crate) fn burst_ms(t: f32) -> f32 {
    t * BURST_FOR.as_secs_f32() * 1000.0
}

/// Whether the newest segment has lit (the count shows it from then on).
pub(crate) fn newest_lit(t: f32) -> bool {
    burst_ms(t) >= NEWEST_AT_MS
}

/// The milestones as a row of LED segments: on arrival the reached ones
/// relight in order, then the one just reached lights last, in the lamp's
/// amber, with a bloom (a cascade).
fn ladder(state: &TuxMix, newest: Option<&str>, t: f32, scale: f32) -> Element<'static, Message> {
    iced::widget::canvas(Segments { lit: fill_from_left(cascade(state, newest, t)), scale })
        .width(Length::Fixed(Segments::width(MILESTONES.len(), scale)))
        .height(Length::Fixed(Segments::height(scale)))
        .into()
}

/// The ladder's segments `t` into the burst.
fn cascade(state: &TuxMix, newest: Option<&str>, t: f32) -> Vec<Seg> {
    let ms = burst_ms(t);
    let mut j = 0.0;
    MILESTONES
        .iter()
        .map(|m| {
            if Some(m.id) == newest {
                let p = ((ms - NEWEST_AT_MS) / SEG_LIGHT_MS).clamp(0.0, 1.0);
                let bloom = if ms < NEWEST_AT_MS { 0.0 } else { 1.0 - ((ms - NEWEST_AT_MS) / BLOOM_MS).clamp(0.0, 1.0) };
                Seg::Newest(p, bloom)
            } else if reached(state, m.id) {
                let from = CASCADE_FROM_MS + j * SEG_STAGGER_MS;
                j += 1.0;
                Seg::Lit(((ms - from) / SEG_LIGHT_MS).clamp(0.0, 1.0))
            } else {
                Seg::Ghost
            }
        })
        .collect()
}

/// `words` with its last two words held together, so a wrap never leaves
/// one word alone on the last line (critique #66 A 6: "Room in your /
/// take", "close / behind.").
pub(crate) fn no_orphan(words: &str) -> String {
    // A trailing space isn't a word break; a pair already held together
    // stays as it is, not three words (OpenCode's job 70).
    let words = words.trim_end();
    match words.rfind(|c: char| c == ' ' || c == '\u{a0}') {
        Some(i) if words[i..].starts_with('\u{a0}') => return words.to_string(),
        _ => {}
    }
    let Some(i) = words.rfind(' ') else { return words.to_string() };
    let glued = format!("{}\u{a0}{}", &words[..i], &words[i + 1..]);
    // A symbol before the last word ("settings → App.", "Main 1/2" is
    // fine) goes with the word before it too (critique #67 A 4).
    let head = &words[..i];
    match head.rfind(' ') {
        Some(j) if head[j + 1..].chars().all(|c| !c.is_alphanumeric()) => {
            format!("{}\u{a0}{}", &glued[..j], &glued[j + 1..])
        }
        _ => glued,
    }
}

/// A ladder filled from the left like a level: what's lit first (in its
/// order), the newest at the end of the run, then the unlit (critique
/// #57: lit in each item's own slot, the gaps read as a meter missing
/// segments). Which item is which stays in Help.
pub fn fill_from_left(segs: Vec<Seg>) -> Vec<Seg> {
    let rank = |s: &Seg| match s {
        Seg::Lit(_) => 0,
        Seg::Newest(..) => 1,
        Seg::Ghost => 2,
    };
    let mut out = segs;
    out.sort_by_key(rank);
    out
}

/// One LED segment: unlit, lit in `signal` (0..1 on its way up), or the
/// newest in the lamp (0..1 up, and its bloom 1..0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Ghost,
    Lit(f32),
    Newest(f32, f32),
}

/// LED segments, with a little room round them for the newest's bloom.
pub struct Segments {
    pub lit: Vec<Seg>,
    pub scale: f32,
}

impl Segments {
    const SEG_W: f32 = 6.0;
    const SEG_H: f32 = 10.0;
    const GAP: f32 = 2.0;
    const ROOM: f32 = 2.0;
    pub fn width(n: usize, scale: f32) -> f32 {
        (n as f32 * (Self::SEG_W + Self::GAP) - Self::GAP + 2.0 * Self::ROOM) * scale
    }
    pub fn height(scale: f32) -> f32 {
        (Self::SEG_H + 2.0 * Self::ROOM) * scale
    }
}

impl<M> canvas::Program<M> for Segments {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: iced::mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = self.scale;
        // A milestone not yet reached: faint, but there (critique #66 A 5,
        // B: 1.45 at 22% on the light Help dialog, the count read by its
        // gaps alone).
        // Firmer on a light card (critique #67 A 6: 2.05 at 40%).
        let ghost = Color { a: if theme::dark_theme() { 0.45 } else { 0.62 }, ..theme::text_sec() };
        let lerp = |a: Color, b: Color, k: f32| Color {
            r: a.r + (b.r - a.r) * k,
            g: a.g + (b.g - a.g) * k,
            b: a.b + (b.b - a.b) * k,
            a: a.a + (b.a - a.a) * k,
        };
        for (i, st) in self.lit.iter().enumerate() {
            let x = (Self::ROOM + i as f32 * (Self::SEG_W + Self::GAP)) * s;
            let y = Self::ROOM * s;
            let (color, bloom) = match *st {
                Seg::Ghost => (ghost, 0.0),
                Seg::Lit(k) => (lerp(ghost, theme::signal(), k), 0.0),
                // The lamp in the ink pulled to read on the floors: raw, it
                // was 1.98:1 on the light card, told from the rest by hue
                // alone (critique #37).
                Seg::Newest(k, b) => (lerp(ghost, theme::readable_on_floors(theme::lamp()), k), b),
            };
            if bloom > 0.0 {
                let e = Self::ROOM * s * bloom;
                frame.fill(
                    &Path::rectangle(Point::new(x - e, y - e), iced::Size::new(Self::SEG_W * s + 2.0 * e, Self::SEG_H * s + 2.0 * e)),
                    Color { a: 0.45 * bloom, ..theme::lamp() },
                );
            }
            if matches!(*st, Seg::Ghost) {
                // Not yet reached: a short bar at the segment's foot, solid,
                // so filled and empty differ by height (critique #68 B), and
                // its middle stays whole pixels wherever the canvas lands
                // (critique #69 B: a hollow 1 px rim smeared over two, 2.36).
                let h = 3.0 * s;
                frame.fill(&Path::rectangle(Point::new(x, y + Self::SEG_H * s - h), iced::Size::new(Self::SEG_W * s, h)), color);
            } else {
                frame.fill(&Path::rectangle(Point::new(x, y), iced::Size::new(Self::SEG_W * s, Self::SEG_H * s)), color);
            }
        }
        vec![frame.into_geometry()]
    }
}

/// A ring, ticked once done (About's lists, a guide's steps).
pub(crate) fn mark(done: bool, scale: f32) -> Element<'static, Message> {
    mark_lit(if done { 1.0 } else { 0.0 }, scale)
}

/// The step a guide is on now: the ring in the accent with a filled centre,
/// so the sentence under the row belongs to one ring (critique #87 A P3-2:
/// steps done out of order left two open rings alike).
pub(crate) fn mark_current(scale: f32) -> Element<'static, Message> {
    let d = 14.0 * scale;
    let ink = theme::readable_on_floors(theme::accent());
    let dot = 6.0 * scale;
    container(container(iced::widget::Space::new()).width(Length::Fixed(dot)).height(Length::Fixed(dot)).style(move |_t: &iced::Theme| container::Style {
        background: Some(ink.into()),
        border: iced::Border { radius: (dot / 2.0).into(), ..iced::Border::default() },
        ..container::Style::default()
    }))
    .width(Length::Fixed(d))
    .height(Length::Fixed(d))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center)
    .style(move |_t: &iced::Theme| container::Style {
        border: iced::Border { color: ink, width: 1.5 * scale, radius: (d / 2.0).into() },
        ..container::Style::default()
    })
    .into()
}

/// A ring on its way to ticked (`lit` 0..1: its rim and its tick come up
/// together), for a guide's steps relighting in a sweep.
pub(crate) fn mark_lit(lit: f32, scale: f32) -> Element<'static, Message> {
    let d = 14.0 * scale;
    let k = lit.clamp(0.0, 1.0);
    let lerp = |a: Color, b: Color| Color {
        r: a.r + (b.r - a.r) * k,
        g: a.g + (b.g - a.g) * k,
        b: a.b + (b.b - a.b) * k,
        a: a.a + (b.a - a.a) * k,
    };
    let rim = lerp(theme::control_edge(), theme::signal());
    let ring = move |color: Color| {
        move |_t: &iced::Theme| container::Style {
            border: iced::Border { color, width: 1.0, radius: (d / 2.0).into() },
            ..container::Style::default()
        }
    };
    if k > 0.0 {
        let tick = theme::readable_on_floors(theme::signal());
        container(crate::widgets::strip_icons::check_icon(Color { a: tick.a * k, ..tick }, scale * 0.8))
            .width(Length::Fixed(d))
            .height(Length::Fixed(d))
            .style(ring(rim))
            .into()
    } else {
        container(iced::widget::Space::new())
            .width(Length::Fixed(d))
            .height(Length::Fixed(d))
            .style(ring(theme::control_edge()))
            .into()
    }
}

/// About's list of milestones: each with a ring that is ticked once
/// reached, and how to reach the others: a map of things worth doing.
pub fn about_list(state: &TuxMix, scale: f32) -> Element<'static, Message> {
    let (n, of) = reached_count(state);
    // Filled from the left like the card's (critique #66 A 5: the two bars
    // counted two ways); the rows below say which is which.
    let lit: Vec<Seg> = fill_from_left(MILESTONES.iter().map(|m| if reached(state, m.id) { Seg::Lit(1.0) } else { Seg::Ghost }).collect());
    let mut list = column![row![
        text("Milestones").font(theme::SERIF).color(theme::text_primary()).size(theme::TEXT_MD * scale),
        iced::widget::Space::new().width(Length::Fill),
        iced::widget::canvas(Segments { lit, scale })
            .width(Length::Fixed(Segments::width(MILESTONES.len(), scale)))
            .height(Length::Fixed(Segments::height(scale))),
        text(format!("{n} of {of} reached")).size(theme::TEXT_SM * scale).color(theme::text_sec()),
    ]
    .spacing(theme::SPACE_SM * scale)
    .align_y(iced::Alignment::Center)]
    .spacing(theme::SPACE_SM * scale);
    // Every one reached: a lasting line, in the lamp's amber (critique
    // #50's "a lasting trace for the finale").
    if n == of {
        let on = state.milestones_all_on.as_deref().map_or(String::new(), |d| format!(", on {d}"));
        list = list.push(
            text(format!("All {of} reached{on}. Every light on the desk."))
                .font(theme::SANS_SEMI)
                .size(theme::TEXT_SM * scale)
                .color(theme::readable_on_floors(theme::lamp())),
        );
    }
    for m in &MILESTONES {
        let done = reached(state, m.id);
        list = list.push(
            row![
                mark(done, scale),
                text(m.title).color(theme::text_primary()).size(theme::TEXT_SM * scale).width(Length::Fixed(140.0 * scale)),
                text(no_orphan(crate::guides::voiced(state, if done { m.praise } else { m.how }))).color(theme::text_sec()).size(theme::TEXT_SM * scale).width(Length::Fill),
            ]
            .spacing(theme::SPACE_SM * scale)
            .align_y(iced::Alignment::Center),
        );
    }
    list.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{new, update};
    use tuxmix_core::RmeDevice;

    #[test]
    fn a_card_behind_a_notice_plays_its_notes_when_it_shows() {
        // The scout: the rise played while the card hid behind a notice.
        let mut state = new(true, None, None);
        check(&mut state);
        state.notice = Some(("Saved".into(), Instant::now()));
        celebrate(&mut state, Reward::Milestone("fx-preset"));
        assert!(state.celebration.is_some_and(|c| !c.sounded), "hidden: not yet");
        expire(&mut state);
        assert!(state.celebration.is_some_and(|c| !c.sounded), "still hidden");
        state.notice = None;
        expire(&mut state);
        assert!(state.celebration.is_some_and(|c| c.sounded), "shown: heard now");
        // Without a notice, at once.
        state.celebration = None;
        celebrate(&mut state, Reward::Milestone("first-snapshot"));
        assert!(state.celebration.is_some_and(|c| c.sounded));
    }

    #[test]
    fn keep_this_as_a_mic_preset_rises_once() {
        // The scout: Keep's own rise, then the first-preset milestone's.
        let mut state = new(true, None, None);
        state.quick_channel = tuxmix_core::ChannelId::Input(0);
        check(&mut state);
        crate::guides::keep_mic_preset(&mut state);
        assert!(reached(&state, "mic-preset"), "kept with Keep's own rise");
        state.notice = None;
        check(&mut state);
        assert!(state.celebration.is_none(), "no second card or rise for it");
    }

    #[test]
    fn a_folder_with_takes_is_seen_and_an_empty_or_missing_one_is_not() {
        // "Your first take" is kept quietly when takes are already there
        // (OpenCode's job 32). A folder of the test's own.
        let dir = std::env::temp_dir().join(format!("audra-takes-seen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!super::folder_has_takes(&dir), "missing");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        assert!(!super::folder_has_takes(&dir), "no WAV");
        std::fs::write(dir.join("2026-11-12 AN1.WAV"), b"x").unwrap();
        assert!(super::folder_has_takes(&dir), "a take");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_first_run_keeps_what_is_already_true_without_a_card() {
        let mut state = new(true, None, None);
        state.mic_presets.push(crate::mic_presets::MicPreset::capture(
            "Old".into(),
            &state.device.inputs()[0].clone(),
            0,
            true,
            None,
        ));
        check(&mut state);
        assert!(reached(&state, "mic-preset"), "seeded");
        assert!(state.celebration.is_none(), "no shower for the past");
    }

    #[test]
    fn a_new_first_is_celebrated_once_and_the_chain_climbs() {
        let mut state = new(true, None, None);
        check(&mut state);
        let (before, _) = reached_count(&state);
        // A Reverb preset saved: a new first.
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        check(&mut state);
        assert_eq!(state.celebration.map(|c| c.reward), Some(Reward::Milestone("fx-preset")));
        assert_eq!(state.celebration.map(|c| c.chain), Some(0));
        assert_eq!(reached_count(&state).0, before + 1);
        // Not again.
        let _ = update(&mut state, Message::CelebrationClose);
        check(&mut state);
        assert!(state.celebration.is_none());
        // The next first in the session starts a step higher.
        state.mic_presets.push(crate::mic_presets::MicPreset::capture(
            "New".into(),
            &state.device.inputs()[0].clone(),
            0,
            true,
            None,
        ));
        if !reached(&state, "mic-preset") {
            check(&mut state);
            assert_eq!(state.celebration.map(|c| c.chain), Some(1));
            assert_eq!(state.celebration.map(|c| c.run), Some(1), "two in a row");
        }
    }

    #[test]
    fn the_last_milestone_keeps_its_day_for_helps_finale() {
        let mut state = new(true, None, None);
        check(&mut state);
        // All but the Reverb preset reached; then that one.
        state.milestones = MILESTONES.iter().filter(|m| m.id != "fx-preset").map(|m| m.id.to_string()).collect();
        state.milestones_all_on = None;
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        check(&mut state);
        assert_eq!(reached_count(&state).0, MILESTONES.len());
        let day = state.milestones_all_on.clone().expect("the day kept");
        assert!(day.len() == 10 && day.as_bytes()[4] == b'-', "a date: {day}");
    }

    #[test]
    fn milestones_have_their_own_switch_apart_from_tips() {
        // The owner, 2026-11-14: "Silence milestones can be a seperate option".
        let mut state = new(true, None, None);
        check(&mut state);
        let _ = update(&mut state, Message::SetTips(false));
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        check(&mut state);
        assert!(state.celebration.is_some(), "Tips off: the win still celebrates");
        let _ = update(&mut state, Message::SetMilestones(false));
        assert!(state.celebration.is_none(), "Milestones off: its card goes");
        let mut quiet = new(true, None, None);
        check(&mut quiet);
        let _ = update(&mut quiet, Message::SetMilestones(false));
        let _ = update(&mut quiet, Message::FxPresetSaveStart(crate::fx::Unit::Echo));
        let _ = update(&mut quiet, Message::FxPresetSave);
        check(&mut quiet);
        assert!(reached(&quiet, "fx-preset") && quiet.celebration.is_none(), "reached, quietly");
    }

    #[test]
    fn a_milestone_takes_a_plain_notices_place_once_it_has_been_read() {
        // Critique #50: it waited 5 s behind the save notice that earned it.
        let mut state = new(true, None, None);
        check(&mut state);
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        check(&mut state);
        assert!(state.celebration.is_some() && state.notice.is_some(), "the save's notice and a milestone waiting");
        expire(&mut state);
        assert!(state.notice.is_some(), "the notice is read first");
        let shown = Instant::now() - Duration::from_secs(2);
        state.notice.as_mut().unwrap().1 = shown;
        expire(&mut state);
        assert!(state.notice.is_none() && state.celebration.is_some_and(|c| c.sounded), "then the milestone");
        // A notice with a key on it stays.
        let mut state = new(true, None, None);
        check(&mut state);
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Reverb));
        let _ = update(&mut state, Message::FxPresetSave);
        check(&mut state);
        // A change Undo takes back, and its notice with the key.
        let _ = update(&mut state, Message::VolumeChanged(tuxmix_core::ChannelId::Input(2), 0, 0.5));
        crate::app::say_undoable(&mut state, "Something Undo takes back.".into(), "Undo it".into());
        let at = Instant::now() - Duration::from_secs(2);
        state.notice.as_mut().unwrap().1 = at;
        state.notice_undo.as_mut().unwrap().0 = at;
        expire(&mut state);
        assert!(state.notice.is_some(), "kept for its key");
    }

    #[test]
    fn tricks_climb_the_pitch_but_not_the_run_and_a_quiet_while_starts_both_again() {
        let mut state = new(true, None, None);
        // Three tricks found: the notes climb, the run doesn't.
        for _ in 0..3 {
            let _ = next_win(&mut state, false);
        }
        // Tricks climb half a step (0, 0, 1); the milestone lands a step
        // above the last (critique #50).
        assert_eq!(next_win(&mut state, true), (2, 0), "a first milestone after tricks is not \"four in a row\"");
        assert_eq!(chain_words(0, false), None);
        // Ten quiet minutes later: back to the start.
        state.last_win = Instant::now().checked_sub(CHAIN_RESETS_AFTER + Duration::from_secs(1));
        assert_eq!(next_win(&mut state, true), (0, 0));
    }

    #[test]
    fn a_milestone_always_lands_above_the_last_small_win_however_many_came_first() {
        // The reviewer, 2026-11-14: after 14 tricks the 15th chimed where
        // the next milestone started, and later tricks rang above it.
        let mut state = new(true, None, None);
        let mut last_small = 0;
        for round in 0..4 {
            for _ in 0..20 {
                last_small = next_win(&mut state, false).0;
                assert!(last_small <= SMALL_TOP, "round {round}: a small win stays under the milestones");
            }
            let (rise, _) = next_win(&mut state, true);
            assert!(rise > last_small && rise <= RISE_TOP, "round {round}: rise {rise} over chime {last_small}");
        }
        // Milestones in a row still climb, to the rise's top.
        let steps: Vec<u8> = (0..4).map(|_| next_win(&mut state, true).0).collect();
        assert!(steps.windows(2).all(|w| w[1] >= w[0]) && *steps.last().unwrap() == RISE_TOP, "{steps:?}");
        // A trick between two milestones at the top never lets the second
        // drop under the first.
        let _ = next_win(&mut state, false);
        assert_eq!(next_win(&mut state, true).0, RISE_TOP, "still at the top after a small win");
    }

    #[test]
    fn the_ladder_relights_in_order_and_the_newest_lights_last() {
        let mut state = new(true, None, None);
        state.milestones = vec!["first-snapshot".into(), "mic-preset".into()];
        state.milestones.push("reverb-heard".into());
        let segs = |t: f32| cascade(&state, Some("reverb-heard"), t);
        // At the start everything is dark; the reached ones light first,
        // the earlier one ahead of the later.
        assert!(segs(0.0).iter().all(|s| match *s {
            Seg::Ghost => true,
            Seg::Lit(k) | Seg::Newest(k, _) => k == 0.0,
        }));
        assert!(matches!(segs(1.0)[4], Seg::Newest(k, b) if k == 1.0 && b == 0.0), "lit, its bloom over");
        let mid = segs(0.2);
        let lit: Vec<f32> = mid.iter().filter_map(|s| if let Seg::Lit(k) = s { Some(*k) } else { None }).collect();
        assert!(lit[0] > lit[1], "in order: {lit:?}");
        assert!(!newest_lit(0.2), "the newest waits for the others");
        assert!(newest_lit(0.55));
        // Shown filled from the left: the two reached, then the newest, then
        // the unlit, with no gaps (critique #57).
        let shown = fill_from_left(segs(1.0));
        assert!(matches!(shown[0], Seg::Lit(_)) && matches!(shown[1], Seg::Lit(_)));
        assert!(matches!(shown[2], Seg::Newest(..)));
        assert!(shown[3..].iter().all(|s| *s == Seg::Ghost));
    }

    #[test]
    fn a_title_never_leaves_one_word_alone() {
        assert_eq!(no_orphan("Room in your take"), "Room in your\u{a0}take");
        assert_eq!(no_orphan("Solo"), "Solo");
        assert_eq!(no_orphan("Room in your take "), "Room in your\u{a0}take", "a trailing space");
        assert_eq!(no_orphan("Room in your\u{a0}take"), "Room in your\u{a0}take", "already held");
        assert_eq!(no_orphan("in Audra settings \u{2192} App."), "in Audra settings\u{a0}\u{2192}\u{a0}App.");
    }

    #[test]
    fn a_win_is_silent_with_the_speaker_struck_off() {
        let mut state = new(true, None, None);
        assert!(sounds_on(&state));
        state.ui_sounds = false;
        assert!(!sounds_on(&state), "the top bar's speaker silences the notes too");
        state.ui_sounds = true;
        state.celebration_sounds = false;
        assert!(!sounds_on(&state));
    }

    #[test]
    fn the_seventh_runs_the_light_test_once_more() {
        let mut state = new(true, None, None);
        state.animations = true;
        state.milestones_seeded = true;
        state.milestones = MILESTONES.iter().map(|m| m.id.to_string()).collect();
        state.boot_sweep = None;
        celebrate(&mut state, Reward::Milestone("all-tricks"));
        assert!(state.boot_sweep.is_some());
        assert_eq!(chain_words(0, true), Some("Every light on the desk."));
    }

    #[test]
    fn the_card_waits_behind_a_notice_and_with_cards_off_it_is_kept_quietly() {
        let mut state = new(true, None, None);
        check(&mut state);
        state.notice = Some(("Something went wrong".into(), Instant::now()));
        let _ = update(&mut state, Message::FxPresetSaveStart(crate::fx::Unit::Echo));
        let _ = update(&mut state, Message::FxPresetSave);
        state.notice = Some(("Something went wrong".into(), Instant::now()));
        check(&mut state);
        assert!(card(&state).is_none(), "never over an error");
        state.notice = None;
        assert!(card(&state).is_some());
        // Cards off: reached, no card.
        let mut quiet = new(true, None, None);
        check(&mut quiet);
        // Milestones off (their own switch); Tips off alone no longer
        // hides them.
        quiet.milestone_cards = false;
        let _ = update(&mut quiet, Message::FxPresetSaveStart(crate::fx::Unit::Echo));
        let _ = update(&mut quiet, Message::FxPresetSave);
        check(&mut quiet);
        assert!(reached(&quiet, "fx-preset") && quiet.celebration.is_none());
    }

    #[test]
    fn a_card_already_shown_comes_back_settled_after_a_notice() {
        // The reviewer, 2026-11-14: any later notice restarted the burst,
        // the ladder and the count when it went.
        let mut state = new(true, None, None);
        state.notice = None;
        celebrate(&mut state, Reward::Milestone("first-snapshot"));
        assert!(state.celebration.is_some_and(|c| c.sounded), "shown at once");
        state.notice = Some(("Saved.".into(), Instant::now()));
        expire(&mut state);
        assert!(state.celebration.unwrap().at.elapsed() >= BURST_FOR, "held settled");
        // One that hasn't shown yet waits at its start.
        let mut fresh = new(true, None, None);
        fresh.notice = Some(("Saved.".into(), Instant::now()));
        celebrate(&mut fresh, Reward::Milestone("first-snapshot"));
        expire(&mut fresh);
        assert!(fresh.celebration.unwrap().at.elapsed() < BURST_FOR, "its burst still to come");
    }
}
