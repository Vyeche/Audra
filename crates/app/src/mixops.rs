//! Copy, paste, clear and mirror (manual 23.2–23.5).
//!
//! - An output's **submix**: every input's and playback's send into that
//!   output pair (level and pan). Copy it, paste it onto another output,
//!   clear it, or mirror it: paste, then keep the other output in step
//!   with every later change to this one. Each output keeps its own
//!   master fader (the manual: "their main volume ... stay fully
//!   independent").
//! - An input's or playback's **mix**: its sends into every output. Copy
//!   it and paste it onto a channel of the same kind, or clear it.
//!
//! The manual offers these from a right-click menu; here they sit in each
//! strip's Settings panel, which a right-click on the strip opens. Every
//! paste and clear is one undo step. A direct edit of a mirror's target
//! ends the mirror, so the user's own change is never overwritten by the
//! next change to the source (the manual does not say; this is Audra's
//! choice).

use tuxmix_core::{ChannelId, RmeDevice};

use crate::app::TuxMix;

/// What Copy put on the mixer's clipboard.
#[derive(Debug, Clone, PartialEq)]
pub enum MixClip {
    /// Output pair `from`'s submix: (level, pan) per source channel, inputs
    /// then playbacks.
    Submix { from: usize, sends: Vec<(f32, i8)> },
    /// A strip's mix: per channel of the strip (one, or two for a linked
    /// pair), its (level, pan) into every output pair.
    Channel { from: ChannelId, sends: Vec<Vec<(f32, i8)>> },
}

/// A mirror: output pair `to` follows output pair `from`'s submix.
pub type Mirror = (usize, usize);

/// Every source channel: inputs, then playbacks.
fn sources(state: &TuxMix) -> Vec<ChannelId> {
    (0..state.device.inputs().len())
        .map(ChannelId::Input)
        .chain((0..state.device.playbacks().len()).map(ChannelId::Playback))
        .collect()
}

/// The output pair an output strip stands for.
pub fn output_pair(state: &TuxMix, ch: usize) -> usize {
    if state.device.outputs_one_per_pair() {
        ch
    } else {
        ch / 2
    }
}

/// The channels a strip covers: a linked pair's two, else one.
fn strip_channels(state: &TuxMix, cid: ChannelId) -> Vec<ChannelId> {
    match cid {
        ChannelId::Input(i) if state.device.input_pair_linked(i / 2) => {
            vec![ChannelId::Input(i & !1), ChannelId::Input(i | 1)]
        }
        ChannelId::Playback(c) if state.device.playback_linked(c / 2) => {
            vec![ChannelId::Playback(c & !1), ChannelId::Playback(c | 1)]
        }
        _ => vec![cid],
    }
}

/// How far a pasted pan may move (of -100..100) with its send left up: a
/// quarter of the way moves either side by a few dB at most.
const PAN_JUMP: i16 = 25;

fn send(state: &TuxMix, c: ChannelId, out: usize) -> (f32, i8) {
    (
        state.device.volume(c, out).unwrap_or(0.0),
        state.device.pan(c, out).unwrap_or(0),
    )
}

/// A send pasted, mirrored or set by a ready-made setup: a rise of more
/// than `RAMP_OVER_DB` glides up (`set_send_gently`), a drop or small rise
/// lands at once, the pan at once (the level audit: written raw, a send off
/// in the Phones and up in Main came in at full level in one write).
///
/// A pan that moves far on a send that is heard would put one side from
/// silence to full in that one write, whatever the level does (the safety
/// audit of 2026-11-17: hard left in the Phones, pasted hard right at the
/// same level). There the send goes down first, takes its pan in silence
/// and glides back up.
fn write_send(state: &mut TuxMix, c: ChannelId, out: usize, (level, pan): (f32, i8)) {
    state.fader_glides.remove(&(c, out));
    let (was, was_pan) = send(state, c, out);
    let heard = |v: f32| v > 0.001;
    if heard(was) && heard(level) && (i16::from(pan) - i16::from(was_pan)).abs() > PAN_JUMP {
        let _ = crate::app::write_send(state, out, |d| d.set_volume(c, out, 0.0));
    }
    crate::app::set_send_gently_via(state, c, out, level, std::time::Instant::now(), crate::app::GlideVia::Copied);
    crate::app::write_send(state, out, |d| {
        let _ = d.set_pan(c, out, pan);
    });
}

/// Copy: the strip's submix (an output) or mix (an input or playback).
pub fn copy(state: &mut TuxMix, cid: ChannelId) {
    state.mix_clip = Some(match cid {
        ChannelId::Output(ch) => {
            let from = output_pair(state, ch);
            let sends = sources(state).iter().map(|&c| send(state, c, from)).collect();
            MixClip::Submix { from, sends }
        }
        _ => {
            let pairs = state.device.output_pair_count();
            let sends = strip_channels(state, cid)
                .iter()
                .map(|&c| (0..pairs).map(|out| send(state, c, out)).collect())
                .collect();
            MixClip::Channel { from: cid, sends }
        }
    });
}

/// Whether the clipboard can be pasted onto `cid`: a submix onto another
/// output, a mix onto another channel of the same kind.
pub fn can_paste(state: &TuxMix, cid: ChannelId) -> bool {
    match (&state.mix_clip, cid) {
        (Some(MixClip::Submix { from, .. }), ChannelId::Output(ch)) => *from != output_pair(state, ch),
        (Some(MixClip::Channel { from, .. }), _) => {
            let same_kind = matches!(
                (from, cid),
                (ChannelId::Input(_), ChannelId::Input(_)) | (ChannelId::Playback(_), ChannelId::Playback(_))
            );
            same_kind && !strip_channels(state, cid).contains(from)
        }
        _ => false,
    }
}

/// Writes `sends` (per source) into output pair `to`, through the model.
fn write_submix(state: &mut TuxMix, to: usize, sends: &[(f32, i8)]) {
    for (c, s) in sources(state).into_iter().zip(sends.iter().copied()) {
        write_send(state, c, to, s);
    }
    crate::control_room::after_mix_change(state, Some(to));
    follow_mirrors_from(state, to);
}

/// Paste the clipboard onto `cid`. Pasting onto a mirror's target ends
/// that mirror.
pub fn paste(state: &mut TuxMix, cid: ChannelId) {
    if !can_paste(state, cid) {
        return;
    }
    match (state.mix_clip.clone(), cid) {
        (Some(MixClip::Submix { sends, .. }), ChannelId::Output(ch)) => {
            let to = output_pair(state, ch);
            stop_mirror_into(state, to);
            write_submix(state, to, &sends);
        }
        (Some(MixClip::Channel { sends, .. }), _) => {
            let pairs = state.device.output_pair_count();
            for (side, c) in strip_channels(state, cid).into_iter().enumerate() {
                let Some(from) = sends.get(side).or(sends.last()) else {
                    continue;
                };
                for (out, &s) in from.iter().enumerate().take(pairs) {
                    write_send(state, c, out, s);
                }
            }
            crate::control_room::after_mix_change(state, None);
            for out in 0..pairs {
                follow_mirrors_from(state, out);
            }
        }
        _ => {}
    }
}

/// Clear: every send into an output's submix, or a channel's every send.
pub fn clear(state: &mut TuxMix, cid: ChannelId) {
    match cid {
        ChannelId::Output(ch) => {
            let to = output_pair(state, ch);
            stop_mirror_into(state, to);
            let silent = vec![(0.0, 0); sources(state).len()];
            write_submix(state, to, &silent);
        }
        _ => {
            let pairs = state.device.output_pair_count();
            for c in strip_channels(state, cid) {
                for out in 0..pairs {
                    let pan = state.device.pan(c, out).unwrap_or(0);
                    write_send(state, c, out, (0.0, pan));
                }
            }
            crate::control_room::after_mix_change(state, None);
            for out in 0..pairs {
                follow_mirrors_from(state, out);
            }
        }
    }
}

/// Mirror: pastes the copied submix onto output `cid` and keeps it in
/// step with its source from now on.
pub fn mirror(state: &mut TuxMix, cid: ChannelId) {
    let (Some(MixClip::Submix { from, .. }), ChannelId::Output(ch)) = (state.mix_clip.clone(), cid) else {
        return;
    };
    let to = output_pair(state, ch);
    if from == to {
        return;
    }
    // One level only: a pair that mirrors another is not a source itself,
    // so mirrors never chain or loop.
    if state.mirrors.iter().any(|&(_, t)| t == from) {
        return;
    }
    stop_mirror_into(state, to);
    state.mirrors.retain(|&(f, _)| f != to);
    let sends: Vec<_> = sources(state).iter().map(|&c| send(state, c, from)).collect();
    write_submix(state, to, &sends);
    state.mirrors.push((from, to));
    state.prefs_dirty = true;
}

/// The output pair `to` mirrors, if any.
pub fn mirrored_from(state: &TuxMix, to: usize) -> Option<usize> {
    state.mirrors.iter().find(|&&(_, t)| t == to).map(|&(f, _)| f)
}

/// Ends the mirror into output pair `to`; its submix stays as it is.
pub fn stop_mirror_into(state: &mut TuxMix, to: usize) {
    let before = state.mirrors.len();
    state.mirrors.retain(|&(_, t)| t != to);
    if state.mirrors.len() != before {
        state.prefs_dirty = true;
    }
}

/// Copies output pair `from`'s whole submix onto every pair mirroring it.
fn follow_mirrors_from(state: &mut TuxMix, from: usize) {
    let targets: Vec<usize> = state.mirrors.iter().filter(|&&(f, _)| f == from).map(|&(_, t)| t).collect();
    for to in targets {
        let sends: Vec<_> = sources(state).iter().map(|&c| send(state, c, from)).collect();
        for (c, s) in sources(state).into_iter().zip(sends) {
            write_send(state, c, to, s);
        }
        crate::control_room::after_mix_change(state, Some(to));
    }
}

/// After the user changed strip `cid`'s send into output pair `out` (a
/// fader, pan or matrix cell): a mirror of `out` follows, and a direct
/// edit of a mirror's target ends that mirror.
pub fn after_send_edit(state: &mut TuxMix, cid: ChannelId, out: usize) {
    if matches!(cid, ChannelId::Output(_)) {
        return;
    }
    stop_mirror_into(state, out);
    let targets: Vec<usize> = state.mirrors.iter().filter(|&&(f, _)| f == out).map(|&(_, t)| t).collect();
    if targets.is_empty() {
        return;
    }
    let channels = strip_channels(state, cid);
    for to in targets {
        for &c in &channels {
            let s = send(state, c, out);
            write_send(state, c, to, s);
        }
        crate::control_room::after_mix_change(state, Some(to));
    }
}
