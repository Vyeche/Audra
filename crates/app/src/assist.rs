//! Assist notes (Learning pass 1, round B): when the mix shows a problem
//! Audra can fix, the counter's slot says so in one line with a **Fix it**
//! key. An input clipping is offered 6 dB less gain; a source playing into
//! no mix at all is offered the Phones at a safe −20 dB, shown (critique
//! #16's P1, the owner's choice: at 0 dB, in a mix not on screen, a hot
//! line source could blast the headphones). × hides that note for the
//! session. One at a time, after a guide, a milestone or a trick, before
//! "Next for you" (`app::foot_card`).

use std::time::{Duration, Instant};

use iced::widget::{button, container, row, text};
use iced::{Element, Length};
use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};
use crate::theme;

/// A problem Audra can offer to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Assist {
    /// Input `idx` clipped and has gain to give back.
    Clipping(usize),
    /// This strip has signal but plays in no mix.
    Unheard(ChannelId),
    /// An output you listen on (Main or a Phones pair, by pair) is silent
    /// while its mix has signal: muted, or (a Phones) all the way down.
    Silent(usize),
}

/// The level "Fix it" brings a Phones turned all the way down up to.
pub const SILENT_UP_DB: f32 = -20.0;

/// Output pair `pair`'s first channel.
pub(crate) fn pair_ch(state: &TuxMix, pair: usize) -> usize {
    if state.device.outputs_one_per_pair() { pair } else { pair * 2 }
}

#[cfg(test)]
pub fn pair_ch_for_tests(state: &TuxMix, pair: usize) -> usize {
    pair_ch(state, pair)
}

/// Why output pair `pair` is silent, if it is: muted, or down.
fn silence(state: &TuxMix, pair: usize) -> Option<bool> {
    let ch = pair_ch(state, pair);
    let muted = state.device.outputs().get(ch).is_some_and(|o| o.mute);
    let down = crate::control_room::user_output_level(state, ch) <= 0.0;
    if muted {
        Some(true)
    } else if down {
        Some(false)
    } else {
        None
    }
}

/// Whether output pair `pair` is muted or all the way down (`hearfix`).
pub fn silenced(state: &TuxMix, pair: usize) -> bool {
    silence(state, pair).is_some()
}

/// How much gain "Fix it" takes off a clipping input, in dB.
pub const GAIN_STEP: u32 = 6;
/// The level "Fix it" sends an unheard source to the Phones (or Main, with
/// no Phones assigned) at: −20 dB,
/// gliding in, never full level unasked.
pub const UNHEARD_DB: f32 = -20.0;
/// A source above this peak (about −40 dBFS) has signal.
const HEARD: f32 = 0.01;
/// Heard within this long, a source counts as playing.
pub const HEARD_FOR: Duration = Duration::from_secs(5);

/// Notes when sources last had signal (the tick calls this), so a note
/// about an unheard source doesn't flicker between words and syllables.
pub fn sweep(state: &mut TuxMix) {
    // The simulation's meters are decoration (random peaks), not signal:
    // counted, every capture showed a "Heads up" about nothing (OpenCode's
    // job 26). Tests note signal themselves.
    if state.device.is_mock() {
        return;
    }
    let now = Instant::now();
    for (i, m) in state.input_meters.iter().enumerate() {
        if m.frame().peak > HEARD {
            state.signal_heard.insert(ChannelId::Input(i), now);
        }
    }
    for (i, m) in state.playback_meters.iter().enumerate() {
        if m.frame().peak > HEARD {
            state.signal_heard.insert(ChannelId::Playback(i), now);
        }
    }
}

/// The number of output pairs, the mixes a source can play in.
fn mix_count(state: &TuxMix) -> usize {
    let n = state.device.outputs().len();
    if state.device.outputs_one_per_pair() { n } else { n / 2 }
}

/// Whether strip `cid` (or its linked partner) plays in any mix.
fn in_any_mix(state: &TuxMix, cid: ChannelId) -> bool {
    let partner = match cid {
        ChannelId::Input(i) if state.device.input_pair_linked(i / 2 * 2) => Some(ChannelId::Input(i ^ 1)),
        ChannelId::Playback(i) => Some(ChannelId::Playback(i ^ 1)),
        _ => None,
    };
    (0..mix_count(state)).any(|out| {
        crate::app::feeds_mix(state, cid, out) || partner.is_some_and(|p| crate::app::feeds_mix(state, p, out))
    })
}

/// The note to show now, if any: a clipping input first (it's loud now),
/// then a source nobody hears. None while the unit is locked, Tips are
/// off, or every such note was set aside.
pub fn current(state: &TuxMix) -> Option<Assist> {
    if !state.tips || state.ui_locked {
        return None;
    }
    let wanted = |a: &Assist| !state.assist_set_aside.contains(a);
    let clipping = state.input_meters.iter().enumerate().find_map(|(i, m)| {
        let ch = state.device.inputs().get(i)?;
        let gain = ch.gain?;
        (m.clipped && gain >= GAIN_STEP).then_some(Assist::Clipping(i)).filter(wanted)
    });
    if clipping.is_some() {
        return clipping;
    }
    let now = Instant::now();
    // An output you listen on stays silent while its mix has signal (the
    // last of Learning pass 1's notes: "silent listening"). Never Main: an
    // unmute brings the speakers back up, and that unasked could make a mic
    // howl (job 26; it had offered Main an unmute). Not while a level is
    // still on its way up (a glide, a snapshot, Dim letting go), when the
    // pair only looks silent.
    let assign = &state.control_room.assign;
    let listened = std::iter::once(assign.monitor(state.control_room.speaker_b)).chain(assign.phones.iter().flatten().copied());
    let fed = |pair: usize| {
        crate::app::mixer_strip_order(state).into_iter().any(|cid| {
            !matches!(cid, ChannelId::Output(_))
                && state.signal_heard.get(&cid).is_some_and(|at| now.duration_since(*at) < HEARD_FOR)
                && crate::app::feeds_mix(state, cid, pair)
        })
    };
    let main = assign.monitor(state.control_room.speaker_b);
    let settling = !state.level_ramps.is_empty() || !state.fader_glides.is_empty() || crate::control_room::is_easing(state);
    let silent = listened
        .filter(|&pair| !settling && pair != main && silence(state, pair).is_some())
        .find(|&pair| fed(pair))
        .map(Assist::Silent)
        .filter(wanted);
    if silent.is_some() {
        return silent;
    }
    crate::app::mixer_strip_order(state).into_iter().find_map(|cid| {
        if matches!(cid, ChannelId::Output(_)) {
            return None;
        }
        let heard = state.signal_heard.get(&cid).is_some_and(|at| now.duration_since(*at) < HEARD_FOR);
        let muted = match cid {
            ChannelId::Input(i) => state.device.inputs().get(i).is_some_and(|c| c.mute),
            ChannelId::Playback(i) => state.device.playbacks().get(i).is_some_and(|c| c.mute),
            ChannelId::Output(_) => true,
        };
        // Only with a Phones pair to send it to: never the speakers, where
        // a mic could howl (critique #17).
        let phones = state.control_room.assign.phones.iter().flatten().next().is_some();
        (phones && heard && !muted && !in_any_mix(state, cid)).then_some(Assist::Unheard(cid)).filter(wanted)
    })
}

/// The Phones pair "Fix it" sends an unheard source to: the first Phones
/// assigned (no note is offered without one, `current`; the Main fallback
/// is only for the label's sake).
pub fn phones_pair(state: &TuxMix) -> usize {
    state.control_room.assign.phones.iter().flatten().next().copied().unwrap_or(state.control_room.assign.main)
}

/// What "Fix it" does for `assist`, as the messages a hand would send, so
/// undo, glides and the unit's own rules apply as they do to a hand.
pub fn fix(state: &TuxMix, assist: Assist) -> Vec<Message> {
    match assist {
        Assist::Clipping(i) => {
            let gain = state.device.inputs().get(i).and_then(|c| c.gain).unwrap_or(0);
            vec![Message::Gain(i, gain.saturating_sub(GAIN_STEP)), Message::ClipReset(ChannelId::Input(i), false)]
        }
        // At −20 dB, and the Phones mix shown, so the change is on screen.
        Assist::Silent(pair) => {
            let ch = pair_ch(state, pair);
            if silence(state, pair) == Some(true) {
                // Unmuted from silence, then gliding up to its own level
                // (the level audit: unmuted as it was, the Phones came back
                // at full level in one write).
                let level = state.device.volume(ChannelId::Output(ch), pair).unwrap_or(0.0);
                vec![
                    Message::MatrixSet(ChannelId::Output(ch), pair, 0.0, false),
                    Message::Mute(ChannelId::Output(ch), false),
                    Message::MatrixSet(ChannelId::Output(ch), pair, level, false),
                ]
            } else {
                vec![Message::MatrixSet(ChannelId::Output(ch), pair, 10f32.powf(SILENT_UP_DB / 20.0), false)]
            }
        }
        Assist::Unheard(cid) => {
            let phones = phones_pair(state);
            vec![
                Message::SelectOutput(phones),
                Message::MatrixSet(cid, phones, 10f32.powf(UNHEARD_DB / 20.0), false),
            ]
        }
    }
}

/// The note's words and its key's label.
fn words(state: &TuxMix, assist: Assist) -> (String, String) {
    match assist {
        Assist::Clipping(i) => (
            // In plain words, the studio's word kept in brackets (critique
            // #95 A P3-1).
            format!("{} is too loud and distorts", crate::app::strip_display_name(state, ChannelId::Input(i))),
            format!("Fix it: gain down {GAIN_STEP} dB"),
        ),
        Assist::Silent(pair) => {
            let mix = crate::sidebar::mix_name(state, pair);
            if silence(state, pair) == Some(true) {
                (format!("{mix} is muted while its mix plays: unmute it?"), "Fix it: unmute".to_string())
            } else {
                (format!("{mix} is all the way down while its mix plays: turn it up?"), format!("Fix it: up to \u{2212}{} dB", SILENT_UP_DB.abs()))
            }
        }
        Assist::Unheard(cid) => {
            let kind = match cid {
                ChannelId::Input(_) => "input",
                ChannelId::Playback(_) => "playback",
                ChannelId::Output(_) => "output",
            };
            (
                format!("{} ({kind}) has signal but plays in no mix", crate::app::strip_display_name(state, cid)),
                format!("Fix it: {} at \u{2212}{} dB", crate::sidebar::mix_said(state, phones_pair(state)), UNHEARD_DB.abs()),
            )
        }
    }
}

/// The note in the counter's slot: one line, Fix it and ×.
pub fn card(state: &TuxMix) -> Option<Element<'_, Message>> {
    let assist = current(state)?;
    let scale = state.ui_scale;
    let (said, key) = words(state, assist);
    let lamp = theme::readable_on_floors(theme::lamp());
    let line = iced::widget::rich_text::<(), Message, iced::Theme, iced::Renderer>([
        iced::widget::span("Heads up:").font(theme::SANS_SEMI).color(lamp),
        iced::widget::span(" "),
        iced::widget::span(said).color(theme::text_primary()),
    ])
    .size(theme::TEXT_SM * scale)
    .width(Length::Fill);
    Some(
        // The words across the card with × beside them, the key under
        // them (critique #35's capture: a long "Fix it: Phones 3/4 at
        // -20 dB" squeezed the words into a column a word wide).
        container(
            iced::widget::column![
                row![
                    line,
                    crate::widgets::strip::hint(
                        button(crate::widgets::strip_icons::close_icon(theme::text_sec(), scale))
                            .padding(0)
                            .width(Length::Fixed(20.0 * scale))
                            .height(Length::Fixed(20.0 * scale))
                            .style(theme::icon_button(false))
                            .on_press(Message::AssistSetAside(assist)),
                        "Not now (for this session)",
                        scale,
                    ),
                ]
                .spacing(theme::SPACE_SM * scale)
                .align_y(iced::Alignment::Start),
                button(text(key).size(theme::TEXT_SM * scale).font(theme::SANS_SEMI))
                    .padding([theme::SPACE_TIGHT * scale, theme::SPACE_MD * scale])
                    .style(theme::page_key(11.0 * scale))
                    .on_press(Message::AssistFix(assist)),
            ]
            .spacing(theme::SPACE_TIGHT * scale),
        )
        .padding([theme::SPACE_SM * scale, theme::SPACE_MD * scale])
        .width(Length::Fill)
        .style(theme::trick_card)
        .into(),
    )
}
