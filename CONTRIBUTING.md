# Contributing to Audra

Thanks for helping. Audra is an early preview: a Linux mixer for the RME Babyface Pro FS in the spirit of TotalMix FX. **It has been tried on one Babyface (the author's)**, so reports from people with other units and setups are worth a lot.

## Reporting a problem or an idea

Use the issue forms. They ask for what happened, the steps, whether it was the simulation or a real unit, and your driver and system. If a sound could hurt ears or gear (feedback, a sudden loud burst, a mic in the speakers), tick "Sounds wrong or unsafe": those go first.

## Sending a change: the gauntlet

Every pull request goes through the same checks, and the maintainer approves every merge. Run them yourself first:

```sh
bash tools/checks/totalMix-gauntlet.sh            # build, tests, lints, safety gate, hygiene
bash tools/checks/totalMix-gauntlet.sh --shots    # also draws ~20 screenshots (target/gauntlet/shots)
```

Paste the last line (`target/gauntlet/summary.txt`) into the pull request.

| Check | What it asks | Blocks a merge? |
|---|---|---|
| build | `cargo build --workspace` on a clean Ubuntu 24.04 | yes |
| tests | `cargo test --workspace`, the speaker, mic and stream safety tests included | yes |
| lints | Clippy's "correctness" and "suspicious" checks | yes |
| safety gate | a change to what plays where comes with a test | yes |
| hygiene | no secrets, no internal files, nothing over 5 MB | yes |
| screenshots | the app drawn off screen, attached for the reviewer | no |
| merge freeze | merges paused for a release; never your fault | while on |

To build you need Rust (stable) and, on Debian, Ubuntu or Pop!_OS: `sudo apt install build-essential pkg-config libasound2-dev libudev-dev`.

### The safety gate

Audra decides what plays in speakers, headphones and a stream, at what level, with mics that may have phantom power on. A mistake there can mean feedback or a loud burst. So a change to those parts must add or change a test that shows what now plays where. `crates/app/src/listener_safety_tests.rs` has many to copy. The gate watches `fx.rs`, `hearfix.rs`, `control_room`, `setups.rs`, `quick.rs`, the recorder, Voice, resets, the files Audra opens (workspaces, snapshots, mic and EQ presets), the remote controls (OSC, MIDI, Mackie, the control socket), the driver crates, and the level and stream code in `app.rs`.

### Rules for tests and checks

- Tests run against the simulation (`--mock`). They never open a window, play a sound, touch PipeWire or write to a real Babyface.
- Screenshots use `--screenshot`, which draws off screen and quits.
- Don't add other people's material (manuals, screenshots of TotalMix, recordings) unless its licence allows it.

## Working with an AI helper

AI-assisted pull requests are welcome; say so in the form. `AGENTS.md` has the rules for AI helpers. In short: run the gauntlet and paste its real output; never claim a test passed without it; keep local paths, hostnames, user names and tokens out of pull request text and logs; use made-up names in examples.

## Conduct

Be kind. [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) has the details; security problems go through [SECURITY.md](SECURITY.md).

## Licence

Audra is GPL-3.0-only. By sending a change you agree it is released under the same licence.
