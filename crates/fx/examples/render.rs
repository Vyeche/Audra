//! Renders a listening sample of every Reverb and Echo type to a WAV file,
//! so the sound can be judged before it runs on the unit:
//!
//!     cargo run -p audra-fx --example render -- /tmp/audra-fx.wav
//!     cargo run -p audra-fx --example render -- /tmp/presets.wav presets
//!
//! The second form plays the factory presets instead of the bare types.
//!
//! Each section: a short dry phrase of plucked tones, then the same phrase
//! through one type (wet and dry together, as a send would be heard), then
//! a little silence. A spoken line is in the console for each.

use audra_fx::{EchoKind, EchoParams, FxEngine, ReverbKind, ReverbParams};
use std::io::Write;

const SR: f32 = 48_000.0;

/// A plucked tone: a decaying sine with a little second harmonic.
fn pluck(freq: f32, len_s: f32) -> Vec<f32> {
    (0..(len_s * SR) as usize)
        .map(|i| {
            let t = i as f32 / SR;
            let env = (-t * 6.0).exp();
            0.35 * env * ((2.0 * std::f32::consts::PI * freq * t).sin() + 0.3 * (4.0 * std::f32::consts::PI * freq * t).sin())
        })
        .collect()
}

/// Four plucks, a short phrase.
fn phrase() -> Vec<f32> {
    let mut out = Vec::new();
    for f in [220.0, 277.2, 329.6, 440.0] {
        out.extend(pluck(f, 0.25));
    }
    out
}

fn section(fx: &mut FxEngine, out: &mut Vec<f32>) {
    let dry = phrase();
    // Dry, then wet+dry, then the tail.
    for s in &dry {
        out.push(*s);
        out.push(*s);
    }
    out.extend(std::iter::repeat_n(0.0, (0.3 * SR) as usize * 2));
    fx.reset();
    let tail = (3.0 * SR) as usize;
    let mut input = Vec::with_capacity((dry.len() + tail) * 2);
    for s in &dry {
        input.push(*s);
        input.push(*s);
    }
    input.extend(std::iter::repeat_n(0.0, tail * 2));
    let mut wet = vec![0.0; input.len()];
    fx.process(&input, &mut wet);
    for (d, w) in input.iter().zip(&wet) {
        out.push((d + w).clamp(-1.0, 1.0));
    }
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "audra-fx.wav".into());
    let mut fx = FxEngine::new(SR);
    let mut samples = Vec::new();
    if std::env::args().nth(2).as_deref() == Some("presets") {
        let at = |s: &Vec<f32>| s.len() as f32 / 2.0 / SR;
        for (name, p) in audra_fx::presets::reverbs() {
            println!("{:5.1} s  Reverb: {name}", at(&samples));
            fx.reverb_on = true;
            fx.echo_on = false;
            fx.reverb.set_params(p);
            section(&mut fx, &mut samples);
        }
        for (name, p) in audra_fx::presets::echoes() {
            println!("{:5.1} s  Echo: {name}", at(&samples));
            fx.reverb_on = false;
            fx.echo_on = true;
            fx.echo.set_params(p);
            section(&mut fx, &mut samples);
        }
        write_wav(&path, &samples).expect("write the WAV");
        println!("Wrote {path} ({:.0} s)", at(&samples));
        return;
    }
    for kind in ReverbKind::ALL {
        println!("Reverb: {}", kind.name());
        fx.reverb_on = true;
        fx.echo_on = false;
        fx.reverb.set_params(ReverbParams { kind, ..ReverbParams::default() });
        section(&mut fx, &mut samples);
    }
    for kind in EchoKind::ALL {
        println!("Echo: {}", kind.name());
        fx.reverb_on = false;
        fx.echo_on = true;
        fx.echo.set_params(EchoParams { kind, ..EchoParams::default() });
        section(&mut fx, &mut samples);
    }
    write_wav(&path, &samples).expect("write the WAV");
    println!("Wrote {path} ({:.0} s)", samples.len() as f32 / 2.0 / SR);
}

/// 16-bit stereo PCM WAV, no dependencies.
fn write_wav(path: &str, samples: &[f32]) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data_len = (samples.len() * 2) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&2u16.to_le_bytes())?; // stereo
    f.write_all(&(SR as u32).to_le_bytes())?;
    f.write_all(&(SR as u32 * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())?;
    }
    Ok(())
}
