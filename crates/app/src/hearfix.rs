//! "I can't hear it" (the owner's pick of 2026-11-21): one press that looks
//! for what Audra itself does to keep a mic or the computer's sound out of
//! the mix you listen on, and undoes it. Every step goes through `update`,
//! as a hand would, so the glides, the unit's rules and Undo apply; the
//! whole press is one undo step (`Message::HearFix`).

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::{Message, TuxMix};

/// Where a send that was off comes in: low enough to be gentle, high enough
/// to be heard.
pub const SEND_DB: f32 = -10.0;

/// An output below this reads as turned down, not just quiet.
pub const LOW_DB: f32 = -30.0;

/// Where an output turned far down comes up to.
pub const UP_DB: f32 = -20.0;

/// One thing found, as the messages that undo it and the words that say so.
pub struct Found {
    pub steps: Vec<Message>,
    pub said: String,
    /// Only takes a mic out (nothing brought back).
    pub takeout: bool,
}

fn vol(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// What keeps `cid` (the source Quick setup works on) and the computer's
/// sound out of output pair `pair`, in the order a person would check.
pub fn find(state: &TuxMix, cid: ChannelId, pair: usize) -> Vec<Found> {
    let mut out = Vec::new();
    let name = |c: ChannelId| crate::app::short_label(&crate::app::strip_display_name(state, c)).to_string();
    // Mics and the speakers (critique #101 A P1-1, #102 A P2-1, P3-1, P3-3,
    // P3-4): no mic is ever sent into them by this press, and every hot mic
    // sent there (muted or not, whichever the source, each side of a linked
    // pair) is taken out first, one cell at a time so no other mix moves;
    // then the speakers can come up safely.
    let open = open_mic_on_speakers(state, cid, pair);
    let mix = crate::sidebar::mix_said(state, pair);
    let hot = hot_mics_in(state, pair);
    if !hot.is_empty() {
        let mut names: Vec<String> = Vec::new();
        for &c in &hot {
            let n = name(c);
            if !names.contains(&n) {
                names.push(n);
            }
        }
        out.push(Found {
            steps: hot.iter().map(|&c| Message::HearFixCellOff(c, pair)).collect(),
            said: format!("{} taken out of {mix} (an open mic there can howl)", names.join(" and ")),
            takeout: true,
        });
    }
    // The output itself: muted or all the way down (the Heads up note's own
    // fix, which unmutes from silence and glides up), or turned far down.
    if crate::assist::silenced(state, pair) {
        out.push(Found {
            steps: crate::assist::fix(state, crate::assist::Assist::Silent(pair)),
            said: format!("{} turned up", capitalised(&mix)),
            takeout: false,
        });
    } else {
        let ch = ChannelId::Output(crate::assist::pair_ch(state, pair));
        if state.device.volume(ch, pair).unwrap_or(1.0) < vol(LOW_DB) {
            out.push(Found {
                steps: vec![Message::MatrixSet(ch, pair, vol(UP_DB), false)],
                said: format!("{} turned up to \u{2212}{}\u{a0}dB", capitalised(&mix), UP_DB.abs()),
                takeout: false,
            });
        }
    }
    // Dim on the speakers.
    let assign = state.control_room.assign;
    if state.control_room.is_dimmed() && (pair == assign.main || Some(pair) == assign.main_b) {
        out.push(Found { steps: vec![Message::DimToggle], said: "Dim switched off".to_string(), takeout: false });
    }
    // Another strip soloed, so everything else is held back.
    let soloed: Vec<ChannelId> = crate::app::all_channel_ids(state).into_iter().filter(|&c| crate::app::channel_is_soloed(state, c)).collect();
    if !soloed.is_empty() && !soloed.contains(&cid) {
        // Letting go of the solo brings back the speakers too: a mic held
        // out of them by it comes out first (critique #103 A P2-2: the
        // headphones press let a hot mic back into the speakers).
        let monitor = crate::control_room::monitor_pair(state);
        if !speakers(state, pair) {
            let hot = hot_mics_in(state, monitor);
            if !hot.is_empty() {
                let names: Vec<String> = hot.iter().map(|&c| name(c)).fold(Vec::new(), |mut v, n| {
                    if !v.contains(&n) {
                        v.push(n);
                    }
                    v
                });
                out.push(Found {
                    steps: hot.iter().map(|&c| Message::HearFixCellOff(c, monitor)).collect(),
                    said: format!("{} taken out of {} (an open mic there can howl)", names.join(" and "), crate::sidebar::mix_said(state, monitor)),
                    takeout: false,
                });
            }
        }
        out.push(Found { steps: vec![Message::GlobalSoloToggle], said: "Solo switched off".to_string(), takeout: false });
    }
    // The source, then the computer's sound: muted, or sent nowhere here
    // (the sends said in one phrase: "AN1 and computer audio sent to …").
    let mut sent: Vec<String> = Vec::new();
    let mut send_steps: Vec<Message> = Vec::new();
    for c in [cid, ChannelId::Playback(0)] {
        if c == cid && (!matches!(c, ChannelId::Input(_)) || open) {
            continue;
        }
        let who = if c == ChannelId::Playback(0) { "the computer's main sound".to_string() } else { name(c) };
        let p = crate::app::strip_params(state, c, pair);
        if p.mute {
            out.push(Found { steps: vec![Message::Mute(c, false)], said: format!("{who} unmuted"), takeout: false });
        }
        if state.device.volume(c, pair).unwrap_or(0.0) <= 0.0 {
            send_steps.push(Message::MatrixSet(c, pair, vol(SEND_DB), false));
            sent.push(who);
        }
    }
    if !sent.is_empty() {
        out.push(Found { steps: send_steps, said: format!("{} sent to {mix} at \u{2212}{}\u{a0}dB", sent.join(" and "), SEND_DB.abs()), takeout: false });
    }
    out
}

/// `cid` is a hot mic (48V on, or its gain at 20 dB or more) and `pair` is
/// the speakers: turning them up would play it into the room.
pub fn open_mic_on_speakers(state: &TuxMix, cid: ChannelId, pair: usize) -> bool {
    // Any mic input, hot or not yet: one sent in at low gain with 48V off
    // was powered a moment later by step 1's own advice (#102 A P3-1).
    speakers(state, pair) && matches!(cid, ChannelId::Input(_)) && crate::app::strip_params(state, cid, pair).has_48v
}

fn speakers(state: &TuxMix, pair: usize) -> bool {
    let assign = state.control_room.assign;
    pair == assign.main || assign.main_b == Some(pair)
}

/// Every hot mic channel (48V on, or its gain at 20 dB or more) sent into
/// `pair` when it is the speakers, muted or not, each side on its own.
fn hot_mics_in(state: &TuxMix, pair: usize) -> Vec<ChannelId> {
    if !speakers(state, pair) {
        return Vec::new();
    }
    (0..state.device.inputs().len())
        .map(ChannelId::Input)
        .filter(|&c| {
            let p = crate::app::strip_params(state, c, pair);
            // Any mic, hot or not yet: 48V is a click away (#103 A P3-2).
            p.has_48v && state.device.volume(c, pair).unwrap_or(0.0) > 0.0
        })
        .collect()
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The notice after a press: what was done, or where to look when Audra
/// found nothing of its own in the way.
pub fn summary(found: &[Found], mix: &str, mic_kept_out: bool, mic_in_phones: bool) -> String {
    // Only mics taken out: the speakers were fine (critique #102 A P2-2:
    // "Fixed" read as if something came back). Where the mic is heard, as
    // it is now (critique #103 A P2-1: said always, and false when muted).
    if !found.is_empty() && found.iter().all(|f| f.takeout) {
        let said: Vec<&str> = found.iter().map(|f| f.said.as_str()).collect();
        let mic = if mic_in_phones {
            "your mic is heard in your headphones"
        } else {
            "to hear your mic, pick your headphones in Quick setup, step 3"
        };
        return format!("{}. Computer audio plays in {mix}; {mic}.", capitalised(&said.join(", ")));
    }
    if found.is_empty() {
        // Only the output asked about (critique #101 A P3-1).
        let what = if mic_kept_out { "computer audio reaches" } else { "your mic and computer audio reach" };
        let kept = if mic_kept_out { " Your mic stays out of them: an open mic there can howl." } else { "" };
        return format!(
            "Nothing in Audra is holding it back: {what} {mix}, and it is turned up.{kept} Check the volume and cable of {mix}, and that your app plays to the Babyface."
        );
    }
    let said: Vec<&str> = found.iter().map(|f| f.said.as_str()).collect();
    format!("Fixed: {}.", said.join(", "))
}
