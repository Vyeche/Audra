//! The EQ panel's live spectrum (the owner, 2026-11-10: "As I'm talking,
//! it's moving around the wave, so it's easier to shape the sound how I
//! like it. Same thing for the EQ", with FabFilter Pro-Q's analyzer as the
//! picture): which pitches the sound has right now, drawn grey behind the
//! curve, so a band goes where the boom or the harshness is.
//!
//! Every tick, for the open EQ panel: the last samples of its input (one,
//! or a pair averaged) from the unit's meter tap, or on an output what
//! the computer plays into it (the playback tap; the unit mixes the
//! inputs in itself, out of the computer's sight), through `spectrum`
//! onto the plot's own log-frequency axis, a rise shown at once and a
//! fall eased. With no EQ panel open the taps keep nothing. In the
//! simulation an input sounds like a voice at its meter's level, so the
//! picture moves there too.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tuxmix_core::ChannelId;

use crate::app::TuxMix;
use crate::spectrum::{self, Smoother};
use crate::widgets::strip::FlyoutKind;

/// Columns across the plot (log spaced, as its frequency axis).
pub const COLS: usize = 160;
/// The plot's frequency span (`eq_plot`'s).
pub const F_LO: f32 = 20.0;
pub const F_HI: f32 = 20_000.0;
/// How fast a peak falls back, in dB a second.
const FALL_DB_PER_S: f32 = 48.0;

/// The open EQ panel's spectrum as it moves, and on an input the voice's
/// pitch (the owner, 2026-11-10: a voice tuner, "useful when using the EQ
/// and the microphone").
pub struct Live {
    pub cid: ChannelId,
    smoother: Smoother,
    at: Instant,
    /// The pitches found lately, newest last (`PITCH_KEEP`).
    pitches: VecDeque<(Instant, f32)>,
}

/// How long a note stays shown after the voice stops.
const PITCH_HOLD: Duration = Duration::from_millis(400);
/// How far back "your lowest note lately" looks.
const PITCH_KEEP: Duration = Duration::from_secs(10);

/// The voice now, for the EQ panel's tuner.
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    /// The pitch now (the middle of the last few found), Hz.
    pub hz: f32,
    /// Its nearest note ("A2") and how far off, in cents (+ above).
    pub note: String,
    pub cents: f32,
    /// The voice's low end lately (its lowest tenth), Hz, once there is
    /// enough to say.
    pub lowest: Option<f32>,
}

/// One step a tick (`Message::Tick`).
pub fn tick(state: &mut TuxMix) {
    // One panel, the same one every tick, should two be open (a set has no
    // order of its own).
    let open = state.flyout_open.iter().filter(|(_, k)| *k == FlyoutKind::Eq).map(|(c, _)| *c).min_by_key(|c| format!("{c:?}"));
    let got = match open {
        Some(ChannelId::Input(i)) => {
            state.playback_tap.scope_channels(&[]);
            let pair = crate::autoset::pair_of(state, i);
            state.device.input_scope(&pair)
        }
        Some(ChannelId::Output(o)) => {
            let _ = state.device.input_scope(&[]);
            let pair = crate::dynamics_panel::out_pair_of(state, o);
            state.playback_tap.scope_channels(&pair);
            let (samples, rate) = state.playback_tap.scope_recent();
            (!samples.is_empty() && rate > 0).then_some((samples, rate))
        }
        _ => {
            let _ = state.device.input_scope(&[]);
            state.playback_tap.scope_channels(&[]);
            None
        }
    };
    let (Some(cid), Some((samples, rate))) = (open, got) else {
        state.eq_live = None;
        return;
    };
    let pitch = matches!(cid, ChannelId::Input(_)).then(|| pitch_at_48k(&samples, rate)).flatten();
    let bins = spectrum::magnitudes_db(&samples);
    let cols = spectrum::columns(&bins, rate.max(1) as f32, COLS, F_LO, F_HI);
    let now = Instant::now();
    let live = match state.eq_live.take() {
        Some(l) if l.cid == cid => l,
        _ => Live { cid, smoother: Smoother::new(COLS), at: now, pitches: VecDeque::new() },
    };
    let mut live = live;
    let dt = now.duration_since(live.at).as_secs_f32();
    live.at = now;
    live.smoother.step(&cols, dt, FALL_DB_PER_S);
    if let Some(hz) = pitch {
        live.pitches.push_back((now, hz));
    }
    while live.pitches.front().is_some_and(|(at, _)| now.duration_since(*at) > PITCH_KEEP) {
        live.pitches.pop_front();
    }
    state.eq_live = Some(live);
}

/// The pitch of `samples` taken at `rate`: above 48 kHz they're averaged
/// down to about 48 kHz first, so the search costs the same at any rate.
fn pitch_at_48k(samples: &[f32], rate: u32) -> Option<f32> {
    let k = (rate as f32 / 48_000.0).round().max(1.0) as usize;
    if k == 1 {
        return audra_fx::pitch::detect(samples, rate as f32);
    }
    let down: Vec<f32> = samples.chunks_exact(k).map(|c| c.iter().sum::<f32>() / k as f32).collect();
    audra_fx::pitch::detect(&down, rate as f32 / k as f32)
}

/// The voice's low end lately in strip `cid`'s EQ panel (its lowest
/// tenth over `PITCH_KEEP`), shown on after the voice stops.
pub fn lowest_for(state: &TuxMix, cid: ChannelId) -> Option<f32> {
    let live = state.eq_live.as_ref().filter(|l| l.cid == cid)?;
    let mut all: Vec<f32> = live.pitches.iter().map(|(_, hz)| *hz).collect();
    if all.len() < 10 {
        return None;
    }
    all.sort_by(f32::total_cmp);
    Some(all[all.len() / 10])
}

/// The voice in strip `cid`'s EQ panel now: the middle of the last five
/// pitches found, while the latest is under `PITCH_HOLD` old.
pub fn voice_for(state: &TuxMix, cid: ChannelId) -> Option<Voice> {
    let live = state.eq_live.as_ref().filter(|l| l.cid == cid)?;
    let (last, _) = live.pitches.back()?;
    if last.elapsed() > PITCH_HOLD {
        return None;
    }
    let mut recent: Vec<f32> = live.pitches.iter().rev().take(5).map(|(_, hz)| *hz).collect();
    recent.sort_by(f32::total_cmp);
    let hz = recent[recent.len() / 2];
    let (note, cents) = audra_fx::pitch::note_of(hz)?;
    let mut all: Vec<f32> = live.pitches.iter().map(|(_, hz)| *hz).collect();
    all.sort_by(f32::total_cmp);
    let lowest = (all.len() >= 10).then(|| all[all.len() / 10]);
    Some(Voice { hz, note, cents, lowest })
}

/// The columns (dB, log spaced `F_LO`..`F_HI`) for strip `cid`'s EQ plot,
/// while its panel is the open one and there is sound to show.
pub fn columns_for(state: &TuxMix, cid: ChannelId) -> Option<Vec<f32>> {
    state.eq_live.as_ref().filter(|l| l.cid == cid).map(|l| l.smoother.values().to_vec())
}

/// The EQ panel's tuner row (an input's): the note, the needle, how far
/// off in plain words, and a word when the Low Cut sits above the voice's
/// low end (it would thin the voice).
pub fn tuner_row<'a>(state: &'a TuxMix, cid: ChannelId, eq: &tuxmix_core::channel::InputEq, width: f32, scale: f32) -> iced::Element<'a, crate::app::Message> {
    use crate::theme;
    use iced::widget::{column, container, row, text};
    use iced::Length;
    let voice = voice_for(state, cid);
    let (note, words) = match voice.as_ref() {
        Some(v) => {
            let c = v.cents.round();
            let how = if c.abs() <= crate::widgets::tuner::IN_TUNE_CENTS {
                "in tune".to_string()
            } else if c > 0.0 {
                // In words, not cents (critique #52).
                // Against the note, not a verdict on the voice (critique
                // #87 A P3-6).
                format!("a little above {}", v.note)
            } else {
                format!("a little below {}", v.note)
            };
            (v.note.clone(), format!("{} Hz \u{b7} {how}", v.hz.round()))
        }
        None => ("\u{2013}".to_string(), "Play, sing or talk to see your note".to_string()),
    };
    // On the EQ panel's grid (the owner: "i see alignment issues too"): the
    // label in its column, everything else from where the band keys start
    // to where they end.
    let note_h = 20.0 * scale;
    let label = container(text("Note").size(theme::TEXT_SM * scale).color(theme::text_sec())).height(Length::Fixed(note_h)).center_y(Length::Fixed(note_h)).width(Length::Fixed(46.0 * scale));
    let line = row![
        container(text(note).font(theme::SANS_SEMI).size(theme::TEXT_MD * scale)).width(Length::Fixed(38.0 * scale)),
        crate::widgets::tuner::needle(voice.as_ref().map(|v| v.cents), 14.0 * scale, scale),
    ]
    .spacing(theme::SPACE_SM * scale)
    .height(Length::Fixed(note_h))
    .align_y(iced::Alignment::Center);
    // Two lines that are always there, one line each, so nothing below
    // moves as you start and stop talking (the owner, 2026-11-10: "I dont
    // like how the bouncing of all the text happens when i start speaking
    // and yellow text pushes everything down"): your note in words, and
    // your low end against the Low Cut, amber only when the cut is above
    // it. The low end stays shown after you stop.
    let room = width - (2.0 * (theme::SPACE_MD + 6.0) + 46.0 + theme::SPACE_SM) * scale;
    let one_line = |words: String, color: iced::Color| {
        text(crate::app::fit_label(&words, room, scale))
            .size(theme::TEXT_SM * scale)
            .color(color)
            .wrapping(iced::advanced::text::Wrapping::None)
    };
    let cut = f32::from(eq.low_cut_freq_hz);
    let (low_words, low_color) = match lowest_for(state, cid) {
        None => ("Low end: not measured yet".to_string(), theme::muted()),
        Some(low) if eq.low_cut_on() && cut > low * 0.9 => {
            (format!("Low cut thins your voice: try {:.0} Hz", (low * 0.7).max(20.0)), theme::attention())
        }
        Some(low) if eq.low_cut_on() => (format!("Low end {low:.0} Hz \u{b7} Low cut sits below it"), theme::muted()),
        Some(low) => (format!("Low end {low:.0} Hz \u{b7} Low cut off"), theme::muted()),
    };
    let col = row![label, column![line, one_line(words, theme::muted()), one_line(low_words, low_color)].spacing(theme::SPACE_HAIRLINE)]
        .spacing(theme::SPACE_SM * scale)
        .align_y(iced::Alignment::Start);
    crate::widgets::strip::hint(
        col,
        "Your voice's note now, like a guitar tuner: the dot sits in the middle when you're on the note. The line on the graph is your pitch; the dashed line, your voice's low end lately (keep the Low cut under it)",
        scale,
    )
}

/// Auto-Tune's switch (the owner: "Auto tune should be a toggle"), in an
/// input's D panel (their pick after critique #30: "Move it to D"): for
/// what others hear, on the processed mic, with the dynamics.
pub fn autotune_switch<'a>(state: &'a TuxMix, cid: ChannelId, pad_x: f32, scale: f32) -> iced::Element<'a, crate::app::Message> {
    let left = crate::autoset::pair_of(state, match cid {
        ChannelId::Input(i) => i,
        _ => 0,
    })[0];
    let here = state.dyn_input == Some(left);
    let on = state.tune_on && here;
    // In the simulation the panel's own line already says it needs the
    // unit; not twice (the second opinion).
    let note = if state.device.is_mock() {
        "Pulls your voice onto the nearest note for others.".to_string()
    } else if !on && state.tune_on && state.dyn_input.is_some() {
        let other = crate::app::strip_display_name(state, ChannelId::Input(state.dyn_input.unwrap_or(0)));
        format!("Runs in Voice on {other} now: turning it on here asks before moving it.")
    } else if !on {
        "Nudges your voice onto the nearest note for others; you hear yourself as you are.".to_string()
    } else {
        match state.dyn_loop.as_ref().and_then(|l| l.tune_note()) {
            Some(n) => {
                let hz = 440.0 * 2f32.powf((n - 69) as f32 / 12.0);
                format!("On: pulling to {}.", audra_fx::pitch::note_of(hz).map(|(name, _)| name).unwrap_or_default())
            }
            None => "On: sing or talk.".to_string(),
        }
    };
    crate::widgets::strip::hint(
        crate::dynamics_panel::power_switch_padded("Auto-Tune", "Auto-Tune".to_string(), note, None, on, crate::app::Message::AutoTune(cid, !on), pad_x, scale),
        "Pulls your voice onto the nearest note, gently, in what others hear (\u{201c}Audra processed mic\u{201d} in the app, and Rec). The EQ's tuner shows the note. You hear yourself as you are: through the computer it would come back too late for your ears",
        scale,
    )
}

/// The simulation's stand-in for an input: a voice-like sound (a low hum
/// of harmonics shaped by three formants, gliding, with a little breath)
/// at `level` (0..1, the mock meter's), `n` samples at 48 kHz, `t`
/// seconds in.
pub fn mock_voice(level: f32, n: usize, t: f32) -> Vec<f32> {
    let rate = 48_000.0f32;
    let level = level.clamp(0.0, 1.0);
    let f0 = 125.0 + 18.0 * (t * 1.7).sin();
    let formant = |f: f32| {
        let bump = |c: f32, w: f32| (-((f - c) / w).powi(2)).exp();
        0.08 + bump(550.0, 180.0) + 0.7 * bump(1_400.0, 300.0) + 0.45 * bump(2_600.0, 450.0)
    };
    let harmonics: Vec<(f32, f32)> = (1..=60)
        .map(|k| k as f32 * f0)
        .take_while(|f| *f < 9_000.0)
        .map(|f| (f, formant(f) / (f / f0).powf(0.5)))
        .collect();
    let norm: f32 = harmonics.iter().map(|(_, a)| a).sum::<f32>().max(1e-6);
    // A small fixed-seed noise for the breath.
    let mut seed = 0x2545_f491_u32 ^ (t * 1000.0) as u32;
    let start = t * rate;
    (0..n)
        .map(|i| {
            let time = (start + i as f32) / rate;
            let voiced: f32 = harmonics.iter().map(|(f, a)| a * (std::f32::consts::TAU * f * time).sin()).sum::<f32>() / norm;
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.02;
            level * (voiced + noise)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulated_voice_has_its_formants_and_follows_the_level() {
        let quiet = mock_voice(0.0, spectrum::FFT_N, 1.0);
        assert!(quiet.iter().all(|v| *v == 0.0), "silent at level 0");
        let v = mock_voice(0.5, spectrum::FFT_N, 1.0);
        assert!(v.iter().all(|s| s.is_finite() && s.abs() <= 0.6));
        let cols = spectrum::columns(&spectrum::magnitudes_db(&v), 48_000.0, COLS, F_LO, F_HI);
        let at = |hz: f32| ((hz / F_LO).ln() / (F_HI / F_LO).ln() * COLS as f32) as usize;
        assert!(cols[at(550.0)] > cols[at(12_000.0)] + 20.0, "a voice's middle over its far highs");
    }
}
