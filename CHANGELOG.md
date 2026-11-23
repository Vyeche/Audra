# Changelog

## 0.1.1 — (unreleased)

Fixes and small helpers after the first preview.

- **Copy these details for a bug report** (Help → About): the version, the device, the kernel, the system and the driver on the clipboard, ready for the issue form. Nothing personal goes in.
- **Fixed:** closing Audra while it drove the unit over USB directly (no kernel driver loaded) could crash it on the way out. The same fix as TuxMix's (ismail-bahloul/TuxMix#56).
- Libraries updated (log, rand, strum, alsa, rfd); the gauntlet now also runs HOL Guard's repository scan and checks the workflows themselves.

## 0.1.0 — early preview (2026-11-23, v0.1.0-preview)

The first public preview, for people willing to test. It has been tried on one Babyface Pro FS, the author's, on one machine; other units and systems haven't been tried yet. See the README's [Early preview](README.md#early-preview) section before you start.

What's in it:
- **Mixing like TotalMix FX:** input, playback and output rows, one submix per output, the Control Room (Main and Phones, Dim, Mono, Talkback, Cue, Speaker B, Cough), the routing Matrix, snapshots, groups, layouts and workspaces.
- **Quick setup:** your mic, its level and where you hear it, in three checked steps; ready-made setups for a podcast or call, a headphone mix, recording, streaming and a call on your speakers.
- **Reverb and Echo** run on the computer through the unit's FX send and return (the Babyface has no effects processor), with an open mic's reverb kept out of the speakers.
- **Voice and Dynamics:** what others hear from your mic, shaped on the computer (presets, noise removal, a level coach), while you hear yourself with no delay.
- **Rec:** recording to WAV files, as DURec does on bigger units.
- **Safety:** levels glide up and drop at once; open mics are kept out of the speakers by Audra's own setups and warned about anywhere else; the stream keeps your voice when Voice goes off.
- **Help that teaches:** guided tasks, milestones, plain words for every output, ten colour themes including High contrast, and a full keyboard path.
- **The gauntlet:** every pull request is built, tested (about 870 tests, the safety tests included), checked for secrets and internal files, and photographed off screen.
