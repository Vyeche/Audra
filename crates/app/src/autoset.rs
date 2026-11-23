//! AutoSet (issue #26; the owner, 2026-11-10, comparing the Babyface Pro FS
//! with the Fireface UCX II / UFX III: "AutoSet Preamp Guard: Not
//! Available"): with it on for an input, a peak near the top (within 1 dB
//! of full scale) lowers that input's preamp gain by 1 dB, again if it keeps
//! happening, never below 0 dB and never up. RME's acts inside the unit in
//! the same instant; Audra sees the meters about 30 times a second, so it
//! reacts a moment after a loud peak: it stops clipping from coming back,
//! not the very first one.
//!
//! Each step goes through `update` as a hand's would (`Message::Gain`), so
//! Undo takes it back and the unit's rules apply, and the column says it.

use std::time::{Duration, Instant};

use tuxmix_core::RmeDevice;

use crate::app::TuxMix;

/// A peak at or over this (−1 dBFS) counts as too near the top.
pub const LEVEL: f32 = 0.891;
/// One step down, in dB.
pub const STEP: u32 = 1;
/// The least time between two steps on one input, so one loud burst lowers
/// it once rather than on every tick of it.
pub const WAIT: Duration = Duration::from_millis(400);

/// Whether AutoSet is on for input `idx`.
pub fn is_on(state: &TuxMix, idx: usize) -> bool {
    state.autoset.contains(&idx)
}

/// The inputs a switch at `idx` acts on: both sides of a linked pair.
pub fn pair_of(state: &TuxMix, idx: usize) -> Vec<usize> {
    let l = idx & !1;
    if state.device.input_pair_linked(l / 2) && state.device.inputs().len() > l + 1 {
        vec![l, l + 1]
    } else {
        vec![idx]
    }
}

/// The steps to take now, as (input, new gain), one per input or linked
/// pair whose meter came near the top since the last look and whose last
/// step is at least `WAIT` ago. Clears the meters' "near the top" marks it
/// reads, whether it steps or not, so a peak during the wait isn't counted
/// twice.
pub fn steps(state: &mut TuxMix) -> Vec<(usize, u32)> {
    let now = Instant::now();
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let on: Vec<usize> = state.autoset.iter().copied().collect();
    // Marks on inputs without AutoSet are dropped at once, so switching it
    // on never acts on a peak from before.
    for (i, m) in state.input_meters.iter_mut().enumerate() {
        if !state.autoset.contains(&i) {
            m.hot = false;
        }
    }
    // The lock holds the mix still, gain included (`locked_out`): AutoSet
    // waits too, and reads no marks meanwhile.
    if state.ui_locked {
        for m in state.input_meters.iter_mut() {
            m.hot = false;
        }
        return out;
    }
    for idx in on {
        if seen.contains(&idx) {
            continue;
        }
        let group = pair_of(state, idx);
        seen.extend(group.iter().copied());
        let hot = group.iter().any(|&i| state.input_meters.get(i).is_some_and(|m| m.hot));
        for &i in &group {
            if let Some(m) = state.input_meters.get_mut(i) {
                m.hot = false;
            }
        }
        if !hot {
            continue;
        }
        let waited = group.iter().all(|i| state.autoset_last.get(i).is_none_or(|at| now.duration_since(*at) >= WAIT));
        if !waited {
            continue;
        }
        for &i in &group {
            let Some(gain) = state.device.inputs().get(i).and_then(|c| c.gain) else { continue };
            if gain == 0 {
                continue;
            }
            out.push((i, gain.saturating_sub(STEP)));
            state.autoset_last.insert(i, now);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{apply_prefs, new, prefs_of, update, Message};

    fn hot(state: &mut TuxMix, idx: usize) {
        state.input_meters[idx].hot = true;
    }

    #[test]
    fn off_it_never_touches_the_gain() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::Gain(0, 30));
        hot(&mut state, 0);
        assert!(steps(&mut state).is_empty());
    }

    #[test]
    fn on_a_peak_near_the_top_lowers_the_gain_a_db_and_waits() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::Gain(0, 30));
        let _ = update(&mut state, Message::SetAutoSet(0, true));
        hot(&mut state, 0);
        let s = steps(&mut state);
        assert!(s.contains(&(0, 29)), "{s:?}");
        assert!(!state.input_meters[0].hot, "the mark is read once");
        // Hot again at once: it waits.
        hot(&mut state, 0);
        assert!(steps(&mut state).iter().all(|&(i, _)| i != 0));
        // After the wait, another step.
        for at in state.autoset_last.values_mut() {
            *at -= WAIT;
        }
        let _ = update(&mut state, Message::Gain(0, 29));
        hot(&mut state, 0);
        assert!(steps(&mut state).contains(&(0, 28)));
    }

    #[test]
    fn a_peak_from_before_it_was_on_never_counts() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::Gain(0, 30));
        hot(&mut state, 0);
        let _ = update(&mut state, Message::SetAutoSet(0, true));
        assert!(steps(&mut state).is_empty(), "the old mark was dropped");
        // Nor while the mix is locked.
        state.ui_locked = true;
        hot(&mut state, 0);
        assert!(steps(&mut state).is_empty());
    }

    #[test]
    fn it_never_goes_below_zero_or_up() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::Gain(0, 0));
        let _ = update(&mut state, Message::SetAutoSet(0, true));
        hot(&mut state, 0);
        assert!(steps(&mut state).iter().all(|&(i, _)| i != 0), "at 0 dB nothing to lower");
    }

    #[test]
    fn the_tick_applies_it_says_so_and_undo_takes_it_back() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::Gain(0, 30));
        let _ = update(&mut state, Message::SetAutoSet(0, true));
        crate::app::apply_autoset(&mut state, &[(0, 29)]);
        assert_eq!(state.device.inputs()[0].gain, Some(29));
        assert!(state.notice.as_ref().is_some_and(|(m, _)| m.contains("AutoSet")));
        let _ = update(&mut state, Message::Undo);
        assert_eq!(state.device.inputs()[0].gain, Some(30));
    }

    #[test]
    fn a_linked_pair_steps_together() {
        let mut state = new(true, None, None);
        // IN3/4 stands as one strip in the simulation.
        if !state.device.input_pair_linked(1) {
            return;
        }
        let _ = update(&mut state, Message::Gain(2, 10));
        let _ = update(&mut state, Message::Gain(3, 10));
        let _ = update(&mut state, Message::SetAutoSet(2, true));
        assert!(is_on(&state, 2) && is_on(&state, 3), "one switch for the pair");
        // Either side's peak steps both, a dB each.
        let gain = |i: usize| state.device.inputs()[i].gain.unwrap_or(0);
        let (l, r) = (gain(2), gain(3));
        assert!(l > 0 && r > 0);
        hot(&mut state, 3);
        let s = steps(&mut state);
        assert!(s.contains(&(2, l - 1)) && s.contains(&(3, r - 1)), "{s:?}");
    }

    #[test]
    fn it_is_kept_between_starts() {
        let mut state = new(true, None, None);
        let _ = update(&mut state, Message::SetAutoSet(1, true));
        let mut back = new(true, None, None);
        apply_prefs(&mut back, prefs_of(&state));
        assert!(is_on(&back, 1));
        assert!(prefs_of(&new(true, None, None)).autoset.is_none(), "nothing written while off");
    }
}
