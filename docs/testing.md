# Testing

Everything here runs without a Babyface, opens no window and plays no sound.

## The test suite

```sh
cargo test -p audra             # the app: about 870 tests, under a second once built
cargo test --workspace          # every crate
```

| Where | What it holds |
|---|---|
| `crates/app/src/listener_safety_tests.rs` | What plays where, and how loud: levels that glide up and drop at once, solo and Mute FX letting go, open mics kept out of the speakers, their reverb, the stream keeping your voice, Undo after all of these. **A change to what plays where adds a test here.** |
| `#[cfg(test)] mod tests` in each module | The module's own behaviour: Quick setup's steps, the guides, the placard's words, the theme's contrast (every theme's text and edges are checked against their contrast targets), presets, the recorder, the OSC and MIDI parsers, and more. |
| `crates/fx/` | The effects and the voice chain. |
| `crates/tuxmix-core/` | The device model and the simulation. |

Tests run against the simulation (`app::new(true, …)`, `MockBabyfacePro`). It keeps every write the app makes (`log_writes`), so a test can check what reached the "unit" and in what order.

## The simulation by hand

```sh
cargo run --release -p audra -- --mock                    # the app, no unit needed
AUDRA_MOCK_DEMO=1 cargo run --release -p audra -- --mock  # with a small studio set up
```

## Screenshots off screen

```sh
cargo run --release -p audra -- --mock --screenshot target/mixer.png
cargo run --release -p audra -- --mock --open settings --screenshot target/settings.png
bash tools/checks/totalMix-gauntlet-shots.sh               # about twenty screens into target/gauntlet/shots
```

No window appears, but `--screenshot` needs a display connection: on a machine without a desktop, put `xvfb-run -a` in front (the gauntlet's script does this by itself). `--open` names a state to set up first (`fx-heard`, `fx-hazard`, `phones-dyn`, `quick-speakers`, `setup-stream` and many more: `OpenPanel` in `crates/app/src/main.rs`). Use a separate settings folder (`XDG_CONFIG_HOME=…`) so a capture never reads or writes your own.

## The gauntlet

Every pull request runs the same checks as:

```sh
bash tools/checks/totalMix-gauntlet.sh            # build, tests, lints, safety gate, hygiene
bash tools/checks/totalMix-gauntlet.sh --shots    # and the screenshots
```

Paste the last line (`target/gauntlet/summary.txt`) into your pull request. [CONTRIBUTING.md](../CONTRIBUTING.md) lists what each check asks.

## The voice chain's sound

`tools/checks/totalMix-voice-quality.py` runs real speech and singing through the voice chain with the code that ships and checks each result against written bars (the README's Development section has the commands). Its recordings are the EBU's SQAM test CD, allowed for research only, so they are downloaded into `target/` and never committed.

## On the unit

Things only a real Babyface can show (the driver, levels in the room, latency, feedback) are tested by hand. If you have one, the bug form's "Real Babyface Pro FS" choice and your driver version help the most.
