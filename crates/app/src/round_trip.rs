//! The round trip a live effect would add (issue #27): how long a sample
//! takes from the unit, through a loop on the computer, back into the unit.
//! Measured silently on the real unit only, by hand
//! (`cargo test -- --ignored live_round_trip`), with the unit set up first
//! (by the caller, and put back after): the computer's ADAT 7/8 playback
//! routed only to the optical output (nothing plugged in), and that output
//! looped back to the computer. A quiet click (−20 dBFS) goes out on ADAT
//! 7/8 playback; the loop counts the frames until it comes back on the
//! ADAT 7/8 recording channels.
//!
//! The loop runs in lockstep, one block in for one block out, as a live
//! effect would, so what it measures is what an effect would add: the
//! capture's latency, the cushion ahead of playback, and playback's.

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use tuxmix_core::pw_meter::{self, TapKind};

    const CHANNELS: usize = 14;
    /// ADAT 7/8: the playback pair out, and the recording pair back in.
    const CLICK_CHANNELS: [usize; 2] = [10, 11];
    const CLICK: f32 = 0.1;

    fn args(target: u32, rate: u32, latency: usize, node: &str, category: &str) -> Vec<String> {
        vec![
            "--target".into(),
            target.to_string(),
            "--raw".into(),
            "--format".into(),
            "f32".into(),
            "--rate".into(),
            rate.to_string(),
            "--channels".into(),
            CHANNELS.to_string(),
            "--channel-map".into(),
            (0..CHANNELS).map(|i| format!("AUX{i}")).collect::<Vec<_>>().join(","),
            "--latency".into(),
            latency.to_string(),
            "--media-category".into(),
            category.into(),
            "--properties".into(),
            format!("node.name={node} node.dont-reconnect=true node.dont-fallback=true node.dont-move=true stream.dont-remix=true"),
            "-".into(),
        ]
    }

    /// The round trip in frames at a requested `latency` (frames), and the
    /// rate; `None` if the click never came back.
    fn measure(latency: usize, cushion_blocks: usize) -> Option<(usize, u32)> {
        let src = pw_meter::discover(TapKind::Source)?;
        let sink = pw_meter::discover(TapKind::Sink)?;
        if src.channels < CHANNELS || sink.channels < CHANNELS {
            eprintln!("needs the Pro Audio profile with 14 channels");
            return None;
        }
        let rate = src.rate;
        let mut rec = Command::new("pw-record")
            .args(args(src.index, rate, latency, "audra-roundtrip-in", "Capture"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut play = Command::new("pw-play")
            .args(args(sink.index, rate, latency, "audra-roundtrip-out", "Playback"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut out = rec.stdout.take()?;
        let mut inp = play.stdin.take()?;
        let width = CHANNELS * 4;
        let silence = vec![0u8; latency * width];
        for _ in 0..cushion_blocks {
            inp.write_all(&silence).ok()?;
        }
        let mut block = vec![0u8; latency * width];
        let (mut read_frames, mut clicked_at) = (0usize, None::<usize>);
        let started = Instant::now();
        let mut found = None;
        while started.elapsed() < Duration::from_secs(4) {
            if out.read_exact(&mut block).is_err() {
                break;
            }
            // Look for the click coming back.
            if let Some(at) = clicked_at {
                for f in 0..latency {
                    let s = f32::from_le_bytes(block[f * width + CLICK_CHANNELS[0] * 4..][..4].try_into().unwrap());
                    if s.abs() > CLICK * 0.3 {
                        found = Some(read_frames + f - at);
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            read_frames += latency;
            // One block out for each block in; the click once, after half a
            // second of settling.
            let mut next = silence.clone();
            if clicked_at.is_none() && started.elapsed() > Duration::from_millis(500) {
                for c in CLICK_CHANNELS {
                    next[c * 4..c * 4 + 4].copy_from_slice(&CLICK.to_le_bytes());
                }
                clicked_at = Some(read_frames);
            }
            if inp.write_all(&next).is_err() {
                break;
            }
        }
        let _ = rec.kill();
        let _ = play.kill();
        let _ = rec.wait();
        let _ = play.wait();
        found.map(|f| (f, rate))
    }

    #[test]
    #[ignore]
    fn live_round_trip() {
        for latency in [256, 128, 64] {
            for cushion in [2, 1] {
                match measure(latency, cushion) {
                    Some((frames, rate)) => eprintln!(
                        "latency {latency} frames, cushion {cushion}: round trip {frames} frames = {:.1} ms",
                        frames as f64 * 1000.0 / f64::from(rate)
                    ),
                    None => eprintln!("latency {latency} frames, cushion {cushion}: no click back"),
                }
            }
        }
    }
}
