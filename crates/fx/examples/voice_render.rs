//! Runs a recording through the processed mic's chain offline, exactly as
//! the live loop does (`audra_fx::VoiceChain`, 256-sample blocks, settings
//! read between blocks), for `tools/checks/totalMix-voice-quality.py`.
//!
//! Raw 32-bit float mono samples in on stdin, out on stdout:
//!
//!   voice_render [--rate 48000] [--preset NAME] [--noise 0|1|2] [--tune]
//!                [--deess 0|1|2] [--event SECONDS:KEY=VALUE]...
//!
//! `--preset`: a voice preset's name ("Clear Calls", "Podcast"...),
//! "natural" (everything off, the default) or "talking" (the D panel's
//! own starting point). An `--event` changes one setting at a time in the
//! recording, as a click on the panel would: keys `noise`, `tune`,
//! `deess`, `cough` (1 held, 0 let go) and `preset`. At the end one JSON
//! line on stderr says how late the chain made the sound.

use std::io::{Read, Write};

use audra_fx::dynamics::{natural, presets};
use audra_fx::{DynamicsParams, VoiceChain};

const BLOCK: usize = 256;

fn preset(name: &str) -> DynamicsParams {
    match name.to_ascii_lowercase().as_str() {
        "natural" => natural(),
        "talking" => DynamicsParams::default(),
        _ => presets()
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, p)| *p)
            .unwrap_or_else(|| {
                eprintln!("no preset called {name:?}");
                std::process::exit(2)
            }),
    }
}

fn apply(chain: &mut VoiceChain, key: &str, value: &str) {
    let level = || value.parse::<u8>().unwrap_or(0);
    match key {
        "noise" => chain.set_noise(level()),
        "tune" => chain.set_tune(level() > 0),
        "deess" => chain.set_deess(level()),
        "cough" => chain.set_cough(level() > 0),
        "preset" => chain.set_dynamics(preset(value)),
        _ => {
            eprintln!("unknown setting {key:?}");
            std::process::exit(2)
        }
    }
}

fn main() {
    let mut rate = 48_000.0f32;
    let mut start: Vec<(String, String)> = Vec::new();
    let mut events: Vec<(usize, String, String)> = Vec::new();
    let mut params = natural();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut next = || args.next().unwrap_or_default();
        match a.as_str() {
            "--rate" => rate = next().parse().unwrap_or(48_000.0),
            "--preset" => params = preset(&next()),
            "--noise" => start.push(("noise".into(), next())),
            "--deess" => start.push(("deess".into(), next())),
            "--tune" => start.push(("tune".into(), "1".into())),
            "--event" => {
                let e = next();
                let (at, kv) = e.split_once(':').unwrap_or(("0", ""));
                let (k, v) = kv.split_once('=').unwrap_or((kv, "1"));
                let at = (at.parse::<f32>().unwrap_or(0.0) * rate) as usize;
                events.push((at, k.to_string(), v.to_string()));
            }
            _ => {
                eprintln!("unknown argument {a:?}");
                std::process::exit(2)
            }
        }
    }
    events.sort_by_key(|e| e.0);
    let mut chain = VoiceChain::new(rate, params);
    for (k, v) in &start {
        apply(&mut chain, k, v);
    }
    let mut raw = Vec::new();
    std::io::stdin().read_to_end(&mut raw).expect("read stdin");
    let mut x: Vec<f32> = raw.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    let (mut most_noise_ms, mut most_tune_ms) = (0f32, 0f32);
    let mut next_event = 0;
    for (i, block) in x.chunks_mut(BLOCK).enumerate() {
        let at = i * BLOCK;
        while next_event < events.len() && events[next_event].0 < at + BLOCK {
            let (_, k, v) = &events[next_event];
            apply(&mut chain, k, v);
            next_event += 1;
        }
        chain.process(block, None);
        most_noise_ms = most_noise_ms.max(chain.noise_ms());
        most_tune_ms = most_tune_ms.max(chain.tune_ms());
    }
    let out: Vec<u8> = x.iter().flat_map(|v| v.to_le_bytes()).collect();
    std::io::stdout().write_all(&out).expect("write stdout");
    eprintln!("{{\"noise_ms\": {most_noise_ms:.2}, \"tune_ms\": {most_tune_ms:.2}}}");
}
