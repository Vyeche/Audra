//! The theme by the time of day (the owner, 2026-11-03: "Allow color of
//! theme to change by time of system. It should be a setting").
//!
//! With it on, a day theme and a night theme each start at a time of the
//! system clock (Daylight from 07:00, Audra from 19:00 to start with), and
//! the window fades from one to the other when the time comes: slowly, like
//! the light in a room. A theme picked by hand fades in too, quicker. A
//! click on a theme tile while the schedule is on picks the theme for the
//! part of the day it is now.

use std::time::{Duration, Instant};

use crate::app::TuxMix;
use crate::theme::{self, Palette};

/// How long the scheduled change takes: dusk, not a switch.
pub const FADE_SCHEDULED: Duration = Duration::from_millis(2400);
/// How long a theme picked by hand takes to arrive.
pub const FADE_PICKED: Duration = Duration::from_millis(450);

/// The day and night themes and when each starts, in minutes after
/// midnight of the system's local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSchedule {
    pub on: bool,
    pub day: usize,
    pub night: usize,
    pub day_from: u16,
    pub night_from: u16,
}

impl Default for ThemeSchedule {
    fn default() -> Self {
        Self {
            on: false,
            day: theme::theme_index("light"),
            night: 0,
            day_from: 7 * 60,
            night_from: 19 * 60,
        }
    }
}

impl ThemeSchedule {
    /// Whether `minutes` after midnight is in the day part. With the night
    /// starting before the day (a night owl's "day" from 20:00 to 06:00),
    /// the day runs across midnight; with both at the same time it is
    /// always day.
    pub fn is_day(&self, minutes: u16) -> bool {
        let (d, n) = (self.day_from, self.night_from);
        match d.cmp(&n) {
            std::cmp::Ordering::Less => minutes >= d && minutes < n,
            std::cmp::Ordering::Greater => minutes >= d || minutes < n,
            std::cmp::Ordering::Equal => true,
        }
    }

    /// The theme for `minutes` after midnight.
    pub fn theme_at(&self, minutes: u16) -> usize {
        if self.is_day(minutes) {
            self.day
        } else {
            self.night
        }
    }
}

/// Minutes after midnight on the system clock, in local time.
pub fn local_minutes() -> u16 {
    // SAFETY: `time` with a null pointer only returns the time, and
    // `localtime_r` writes the broken-down time into our own `tm`.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 12 * 60;
        }
        (tm.tm_hour * 60 + tm.tm_min).clamp(0, 24 * 60 - 1) as u16
    }
}

/// "07:00" for 420 minutes.
pub fn clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The theme the window should show now: the schedule's, or the one picked.
pub fn wanted(state: &TuxMix) -> usize {
    wanted_at(state, local_minutes())
}

fn wanted_at(state: &TuxMix, minutes: u16) -> usize {
    // The desktop's light or dark first, where it says one (issue #21,
    // the owner's yes): the day theme for light, the night theme for dark.
    let desktop = state.theme_desktop.then_some(state.desktop_dark).flatten();
    let index = if let Some(dark) = desktop {
        if dark {
            state.theme_schedule.night
        } else {
            state.theme_schedule.day
        }
    } else if state.theme_schedule.on {
        state.theme_schedule.theme_at(minutes)
    } else {
        state.theme
    };
    index.min(theme::themes().len() - 1)
}

/// The desktop's light or dark setting, through the XDG Settings portal
/// (`org.freedesktop.appearance` `color-scheme`; COSMIC, GNOME and KDE
/// answer it), now and on every change: `Some(true)` dark, `Some(false)`
/// light, `None` no preference or no portal. Runs while "Follow the
/// desktop's light and dark" is on.
pub fn desktop_scheme() -> impl iced::futures::Stream<Item = crate::app::Message> {
    use ashpd::desktop::settings::{ColorScheme, Settings};
    use iced::futures::{SinkExt, StreamExt};
    iced::stream::channel(4, |mut out: iced::futures::channel::mpsc::Sender<crate::app::Message>| async move {
        let said = |c: ColorScheme| match c {
            ColorScheme::PreferDark => Some(true),
            ColorScheme::PreferLight => Some(false),
            ColorScheme::NoPreference => None,
        };
        let Ok(settings) = Settings::new().await else {
            let _ = out.send(crate::app::Message::DesktopScheme(None)).await;
            return;
        };
        let now = settings.color_scheme().await.ok().and_then(said);
        let _ = out.send(crate::app::Message::DesktopScheme(now)).await;
        let Ok(mut changes) = settings.receive_color_scheme_changed().await else {
            return;
        };
        while let Some(c) = changes.next().await {
            if out.send(crate::app::Message::DesktopScheme(said(c))).await.is_err() {
                return;
            }
        }
    })
}

/// Moves the window toward the theme it should show, fading over `length`
/// (at once with the animations off). Called on every tick, so the
/// schedule's change happens within a tick of its time.
pub fn follow(state: &mut TuxMix, length: Duration) {
    follow_at(state, length, local_minutes());
}

pub(crate) fn follow_at(state: &mut TuxMix, length: Duration, minutes: u16) {
    let want = wanted_at(state, minutes);
    if want == state.theme_shown {
        return;
    }
    // From whatever shows this instant, mid-fade included, so a second
    // change never jumps.
    let from = shown_palette(state);
    state.theme_shown = want;
    state.theme_fade = state.animations.then(|| (from, Instant::now(), length));
}

/// The palette to draw with now: the shown theme's, or the blend while a
/// fade is in flight.
pub fn shown_palette(state: &TuxMix) -> Palette {
    let target = *theme::themes()[state.theme_shown.min(theme::themes().len() - 1)].palette;
    match state.theme_fade {
        Some((from, at, length)) => {
            let t = crate::motion::progress(at, length, Instant::now());
            // Smooth both ways (ease in and out): a change of light, not
            // an arrival.
            let eased = t * t * (3.0 - 2.0 * t);
            theme::blend_palettes(&from, &target, eased)
        }
        None => target,
    }
}

/// Whether a fade is in flight (the fast redraw timer runs).
pub fn fading(state: &TuxMix) -> bool {
    state
        .theme_fade
        .is_some_and(|(_, at, length)| crate::motion::progress(at, length, Instant::now()) < 1.0)
}

/// Drops a finished fade.
pub fn prune(state: &mut TuxMix) {
    if state.theme_fade.is_some() && !fading(state) {
        state.theme_fade = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{apply_prefs, new, prefs_of, update, Message};

    #[test]
    fn day_and_night_split_the_clock() {
        let s = ThemeSchedule { on: true, day: 1, night: 0, day_from: 7 * 60, night_from: 19 * 60 };
        assert_eq!(s.theme_at(6 * 60 + 59), 0);
        assert_eq!(s.theme_at(7 * 60), 1);
        assert_eq!(s.theme_at(18 * 60 + 59), 1);
        assert_eq!(s.theme_at(19 * 60), 0);
        // A day that runs across midnight.
        let owl = ThemeSchedule { day_from: 20 * 60, night_from: 6 * 60, ..s };
        assert_eq!(owl.theme_at(23 * 60), 1);
        assert_eq!(owl.theme_at(3 * 60), 1);
        assert_eq!(owl.theme_at(12 * 60), 0);
        assert!(ThemeSchedule { day_from: 300, night_from: 300, ..s }.is_day(0));
        assert_eq!(clock(7 * 60 + 5), "07:05");
        assert!(local_minutes() < 24 * 60);
    }

    #[test]
    fn the_window_follows_the_clock_with_a_fade() {
        let mut state = new(true, None, None);
        state.theme_schedule = ThemeSchedule { on: true, ..ThemeSchedule::default() };
        follow_at(&mut state, FADE_SCHEDULED, 12 * 60);
        assert_eq!(state.theme_shown, state.theme_schedule.day);
        assert!(state.theme_fade.is_some(), "it fades, not snaps");
        follow_at(&mut state, FADE_SCHEDULED, 21 * 60);
        assert_eq!(state.theme_shown, state.theme_schedule.night);
        // Off: the theme picked by hand.
        state.theme_schedule.on = false;
        state.theme = 3;
        follow_at(&mut state, FADE_PICKED, 21 * 60);
        assert_eq!(state.theme_shown, 3);
        // Without animations, at once.
        state.animations = false;
        state.theme = 4;
        follow_at(&mut state, FADE_PICKED, 21 * 60);
        assert!(state.theme_fade.is_none());
        assert_eq!(shown_palette(&state), *theme::themes()[4].palette);
    }

    #[test]
    fn a_tile_picks_the_theme_for_the_part_of_the_day_it_is() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetThemeAuto(true));
        let nord = theme::theme_index("nord");
        let _ = update(&mut state, Message::SetTheme(nord));
        let s = state.theme_schedule;
        assert!(s.day == nord || s.night == nord, "the slot for now took it");
        assert_eq!(state.theme_shown, nord);
    }

    #[test]
    fn the_schedule_is_kept() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetThemeAuto(true));
        let _ = update(&mut state, Message::SetThemeNight(theme::theme_index("midnight")));
        let _ = update(&mut state, Message::SetThemeNightFrom(21 * 60 + 30));
        assert!(state.prefs_dirty);
        let mut back = new(true, None, None);
        apply_prefs(&mut back, prefs_of(&state));
        assert_eq!(back.theme_schedule, state.theme_schedule);
        // Off and default: nothing written.
        let plain = new(true, None, None);
        let p = prefs_of(&plain);
        assert!(p.theme_auto.is_none() && p.theme_night.is_none());
    }

    #[test]
    fn the_desktops_light_and_dark_pick_the_day_and_night_themes() {
        // Issue #21: "Follow the desktop's light and dark".
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetThemeAuto(true));
        let _ = update(&mut state, Message::SetThemeDesktop(true));
        assert!(!state.theme_schedule.on, "one of the two follows at a time");
        let (day, night) = (state.theme_schedule.day, state.theme_schedule.night);
        let _ = update(&mut state, Message::DesktopScheme(Some(true)));
        assert_eq!(wanted(&state), night);
        let _ = update(&mut state, Message::DesktopScheme(Some(false)));
        assert_eq!(wanted(&state), day);
        assert_eq!(state.theme_shown, day);
        // No preference: the theme picked.
        let _ = update(&mut state, Message::DesktopScheme(None));
        assert_eq!(wanted(&state), state.theme);
        // Kept across a restart.
        let mut back = new(true, None, None);
        apply_prefs(&mut back, prefs_of(&state));
        assert!(back.theme_desktop);
    }

    #[test]
    fn a_blend_runs_from_one_theme_to_the_other() {
        let (a, b) = (theme::themes()[0].palette, theme::themes()[1].palette);
        assert_eq!(theme::blend_palettes(a, b, 0.0), *a);
        assert_eq!(theme::blend_palettes(a, b, 1.0), *b);
    }
}
