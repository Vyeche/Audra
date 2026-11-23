//! Timings of the paths that run many times a second (the owner,
//! 2026-11-03: "check for any performance issues run a few benchmarks").
//! Ignored by a normal `cargo test`; run them with
//!
//! ```sh
//! cargo test -p audra bench -- --ignored --nocapture --test-threads=1
//! ```
//!
//! They time building the view (each tick rebuilds it), the tick itself,
//! a fader drag step, a theme fade step and the scene capture behind undo
//! and snapshots, on the simulated device. Layout and drawing are iced's
//! and are measured on the running app instead (see log.md).

use std::time::{Duration, Instant};

use iced::window;
use tuxmix_core::ChannelId;

use crate::app::{new, update, view, Message, RowMode, TuxMix, View};

/// Runs `f` `n` times after a warm-up and prints the mean and the worst.
fn time(label: &str, n: u32, mut f: impl FnMut()) -> Duration {
    for _ in 0..n / 10 + 1 {
        f();
    }
    let mut worst = Duration::ZERO;
    let start = Instant::now();
    for _ in 0..n {
        let t = Instant::now();
        f();
        worst = worst.max(t.elapsed());
    }
    let mean = start.elapsed() / n;
    println!("{label:<44} mean {:>8.1} µs   worst {:>8.1} µs", mean.as_secs_f64() * 1e6, worst.as_secs_f64() * 1e6);
    mean
}

fn studio() -> TuxMix {
    let mut state = new(true, None, None);
    state.window_width = 1280.0;
    state.window_size = Some((1280.0, 700.0));
    state.sidebar_open = true;
    state
}

#[test]
#[ignore]
fn bench_hot_paths() {
    let id = window::Id::unique();
    println!();

    // Building the whole view: what every tick costs before iced lays it out.
    for (label, view_kind, rows) in [
        ("view: Mixer, one row open (1280x700)", View::Mixer, RowMode::Auto),
        ("view: Mixer, all three rows", View::Mixer, RowMode::Three),
        ("view: Quick", View::Quick, RowMode::Auto),
        ("view: Matrix", View::Matrix, RowMode::Auto),
    ] {
        let mut state = studio();
        state.view = view_kind;
        state.row_mode = rows;
        let mean = time(label, 300, || {
            let _ = std::hint::black_box(view(&state, id));
        });
        // At the 30 Hz tick this is the share of one core spent on it.
        println!("{:<44} = {:.1}% of a core at 30 Hz", "", mean.as_secs_f64() * 30.0 * 100.0);
    }
    {
        let mut state = studio();
        state.show_settings = true;
        time("view: Mixer with Audra settings open", 300, || {
            let _ = std::hint::black_box(view(&state, id));
        });
    }

    // The tick: meters, the lamp, the theme by time of day, the clock.
    let mut state = studio();
    time("update: Tick", 2000, || {
        let _ = update(&mut state, Message::Tick);
    });
    let mut state = studio();
    state.theme_schedule.on = true;
    time("update: Tick, theme by time of day on", 2000, || {
        let _ = update(&mut state, Message::Tick);
    });

    // A fader drag: one step of the gesture (the undo step is opened once).
    let mut state = studio();
    let mut v = 0.2f32;
    time("update: VolumeChanged (a drag step)", 2000, || {
        v = if v > 0.9 { 0.2 } else { v + 0.001 };
        let _ = update(&mut state, Message::VolumeChanged(ChannelId::Input(0), 0, v));
    });

    // The fast motion tick, and a theme fade's blend.
    let mut state = studio();
    time("update: CollapseTick", 2000, || {
        let _ = update(&mut state, Message::CollapseTick);
    });
    let mut state = studio();
    state.theme_fade = Some((*crate::theme::themes()[1].palette, Instant::now(), Duration::from_secs(60)));
    time("themetime::shown_palette mid-fade", 20000, || {
        let _ = std::hint::black_box(crate::themetime::shown_palette(&state));
    });

    // The scene behind undo, snapshots and the auto-save.
    let state = studio();
    time("control_room::capture_scene", 2000, || {
        let _ = std::hint::black_box(crate::control_room::capture_scene(&state));
    });
    let scene = crate::control_room::capture_scene(&state);
    time("scene to JSON (auto-save, snapshot file)", 500, || {
        let _ = std::hint::black_box(scene.to_json());
    });
    time("prefs_of + JSON (preferences write)", 500, || {
        let _ = std::hint::black_box(serde_json::to_string(&crate::app::prefs_of(&state)));
    });
}
