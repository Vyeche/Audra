# Architecture

How Audra is put together, for anyone changing it. [`PLAN.md`](../PLAN.md) has the decisions and history behind it.

## The parts

| Path | What | Licence |
|---|---|---|
| `driver/` | The `snd-usb-babyface-pro` kernel driver: the unit's audio and its mixer as ALSA controls | GPL-2.0-only |
| `crates/tuxmix-core/` | The device model: channels, the mixer, scenes, meters, the ALSA backend and the simulated device (`mock.rs`) | MIT |
| `crates/tuxmix-usb/` | A USB backend for the unit's proprietary mode, kept for prototyping | GPL-3.0-or-later |
| `crates/fx/` (`audra-fx`) | The sound processing Audra does on the computer: reverb, echo, dynamics, noise removal, de-essing, pitch correction | GPL-3.0-only |
| `crates/app/` (`audra`) | The app: an [iced](https://iced.rs) GUI and everything around it | GPL-3.0-only |

`driver/`, `tuxmix-core` and `tuxmix-usb` come from ismail-bahloul's projects as git subtrees. Change them only in ways upstream could take.

## From a fader to the unit

1. A fader, knob or key sends a `Message` (`crates/app/src/app.rs`, `pub enum Message`).
2. `update` handles it: it changes the app's state and calls the device (`state.device`, a `DeviceHandle`: `Real` is the ALSA backend, `Mock` the simulation).
3. The ALSA backend writes ALSA controls; the driver turns them into USB control requests to the unit.
4. The unit can't report its mixer back, so the app's state is the truth while it runs, and ALSA's saved state (`alsactl store`) keeps it across reboots. The app's own preferences are in `~/.config/audra/gui.json` (`prefs.rs`).

`view` draws the window from the state. Nothing in `view` changes anything.

## What runs where

- **The UI thread** runs `update` and `view`, the animations (`motion.rs`) and the meters' timer.
- **Reverb and Echo** (`fx_loop.rs`, with `crates/fx`): the unit mixes an FX send from AN1/2 and hands it to the computer on two extra capture channels; Audra runs the effects and plays the result back into the unit's FX return channels. It needs the driver that opens those channels and the Pro Audio profile.
- **Voice and Dynamics** (`dynamics_loop.rs`): you keep hearing your mic straight through the unit; what apps hear is processed on the computer and offered as a virtual mic, "Audra processed mic". On an output, apps play to a virtual speaker and Audra shapes what they play.
- **Recording** (`recorder.rs`): one press writes the chosen inputs to a WAV file, as RME's DURec would on bigger units.
- **Remote control**, all optional: MIDI and Mackie Control (`midi.rs`, `mackie.rs`), OSC on 127.0.0.1 (`osc.rs`, `--osc`), and a local socket for automation and tests (`control.rs`, `--control`; `tools/mcp/audra_mcp.py` wraps it).

## Safety rules the code keeps

Audra routes mics, often with 48V phantom power on, to speakers, headphones and streams, so some rules hold everywhere. `crates/app/src/listener_safety_tests.rs` tests them.

- **Levels go down at once and up gently.** A raised level glides (`set_level_gently`); a snapshot, Undo or workspace that would raise something by more than 6 dB slides it up.
- **An open mic never goes into the speakers by Audra's own hand.** Quick setup, the ready-made setups and "Fix it" send it to the headphones; the Now Mixing card warns when you put it there yourself.
- **An open mic's reverb never plays in the speakers unsaid.** `fx::mic_reverb_hazard` checks the speakers whatever mix is being edited; "Make it heard" moves the reverb to the headphones.
- **The stream keeps your voice.** Switching Voice off puts the plain mic back in the mix to your apps; Undo that would take it out says so.
- **Nothing reaches the desktop from a test.** Tests use the simulation; captures draw off screen.

## Drawing without a window

`--screenshot FILE.png` (`main.rs`, `capture_offscreen`) runs the app for a moment, draws one frame off screen and quits. No window appears, but it needs a display connection: a desktop session, or `xvfb-run` on a machine without one (a CI runner, a container). `--open NAME` sets up a state first (a panel open, a guide under way, a hazard); `AUDRA_PROBE="press X Y"` with `AUDRA_PROBE_APPLY=1` presses a spot and applies what the press does, with anything that would reach the desktop (sounds, dialogs, links, recording) held back. The gauntlet's screenshots and the review captures are made this way.
