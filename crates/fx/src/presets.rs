//! Factory presets: settings of our own, under our own names (the manual's
//! "Factory" list, BF §21.6; `docs/ui-spec.md` §7: "We ship our own set
//! and our own names"). Each is a starting point a user can load, turn and
//! save as their own.
//!
//! Volumes are the wet level into the FX return; the return in each
//! output still decides how much is heard, so none of these is loud on
//! its own.

use crate::echo::{EchoKind, EchoParams};
use crate::reverb::{ReverbKind, ReverbParams};

/// The factory reverbs, in the order the menu lists them: the gentlest
/// first, for a voice that should just sound less dry.
pub fn reverbs() -> Vec<(&'static str, ReverbParams)> {
    let d = ReverbParams::default();
    vec![
        (
            "Just a touch",
            ReverbParams { kind: ReverbKind::SmallRoom, pre_delay_ms: 5.0, room_scale: 0.6, low_cut_hz: 200.0, high_cut_hz: 7_000.0, smooth: 50.0, width: 60.0, volume_db: -10.0, ..d },
        ),
        (
            "Close vocal",
            ReverbParams { kind: ReverbKind::MediumRoom, pre_delay_ms: 25.0, room_scale: 0.9, low_cut_hz: 150.0, high_cut_hz: 10_000.0, smooth: 70.0, width: 80.0, volume_db: -3.0, ..d },
        ),
        (
            "Small studio",
            ReverbParams { kind: ReverbKind::SmallRoom, pre_delay_ms: 10.0, room_scale: 1.0, low_cut_hz: 120.0, high_cut_hz: 12_000.0, smooth: 60.0, volume_db: -2.0, ..d },
        ),
        (
            "Warm wood",
            ReverbParams { kind: ReverbKind::Warm, pre_delay_ms: 15.0, room_scale: 1.0, low_cut_hz: 100.0, high_cut_hz: 8_000.0, smooth: 60.0, volume_db: -3.0, ..d },
        ),
        (
            "Big hall",
            ReverbParams { kind: ReverbKind::LargeRoom, pre_delay_ms: 40.0, room_scale: 2.2, low_cut_hz: 100.0, high_cut_hz: 9_000.0, smooth: 80.0, volume_db: -4.0, ..d },
        ),
        (
            "Bright and long",
            ReverbParams { kind: ReverbKind::Long, pre_delay_ms: 20.0, time_s: 2.2, high_damp_hz: 14_000.0, low_cut_hz: 180.0, high_cut_hz: 16_000.0, smooth: 70.0, volume_db: -4.0, ..d },
        ),
        (
            "Tight slap",
            ReverbParams { kind: ReverbKind::Slap, pre_delay_ms: 0.0, room_scale: 1.0, low_cut_hz: 150.0, high_cut_hz: 10_000.0, smooth: 40.0, volume_db: -4.0, ..d },
        ),
        (
            "Outer space",
            ReverbParams { kind: ReverbKind::Space, pre_delay_ms: 60.0, room_scale: 2.5, low_cut_hz: 120.0, high_cut_hz: 9_000.0, smooth: 90.0, volume_db: -5.0, ..d },
        ),
        (
            "Gated drums",
            ReverbParams { kind: ReverbKind::Gated, pre_delay_ms: 0.0, room_scale: 1.5, attack_ms: 5.0, hold_ms: 180.0, release_ms: 60.0, low_cut_hz: 100.0, volume_db: -2.0, ..d },
        ),
        (
            "Swell",
            ReverbParams { kind: ReverbKind::Envelope, pre_delay_ms: 10.0, room_scale: 1.8, attack_ms: 200.0, hold_ms: 200.0, release_ms: 300.0, volume_db: -3.0, ..d },
        ),
    ]
}

/// The factory echoes, short to long.
pub fn echoes() -> Vec<(&'static str, EchoParams)> {
    let d = EchoParams::default();
    vec![
        ("Slapback", EchoParams { kind: EchoKind::Stereo, delay_ms: 110.0, feedback: 10.0, width: 60.0, high_cut_hz: 6_000.0, volume_db: -6.0 }),
        ("Dark tape", EchoParams { kind: EchoKind::Stereo, delay_ms: 280.0, feedback: 50.0, high_cut_hz: 3_000.0, volume_db: -7.0, ..d }),
        ("Ping-pong", EchoParams { kind: EchoKind::Pong, delay_ms: 300.0, feedback: 45.0, width: 100.0, high_cut_hz: 7_000.0, volume_db: -6.0 }),
        ("Dotted eighth at 120", EchoParams { kind: EchoKind::Stereo, delay_ms: 375.0, feedback: 40.0, high_cut_hz: 8_000.0, volume_db: -8.0, ..d }),
        ("Cross trails", EchoParams { kind: EchoKind::Cross, delay_ms: 420.0, feedback: 55.0, high_cut_hz: 5_000.0, volume_db: -8.0, ..d }),
        ("Quarter at 120", EchoParams { kind: EchoKind::Stereo, delay_ms: 500.0, feedback: 35.0, high_cut_hz: 8_000.0, volume_db: -8.0, ..d }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_factory_preset_is_in_range_and_named_once() {
        let r = reverbs();
        for (name, p) in &r {
            assert_eq!(p.clamped(), *p, "{name} stays within the knobs' ranges");
        }
        let e = echoes();
        for (name, p) in &e {
            assert_eq!(p.clamped(), *p, "{name} stays within the knobs' ranges");
        }
        let mut names: Vec<&str> = r.iter().map(|(n, _)| *n).chain(e.iter().map(|(n, _)| *n)).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "no two presets share a name");
    }

    #[test]
    fn the_first_reverb_is_the_gentlest() {
        let r = reverbs();
        let quietest = r.iter().map(|(_, p)| p.volume_db).fold(f32::INFINITY, f32::min);
        assert_eq!(r[0].1.volume_db, quietest);
    }
}
