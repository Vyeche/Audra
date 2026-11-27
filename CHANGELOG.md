# Changelog

## Unreleased

## 0.1.4 — early preview (2026-11-27)

Room EQ for your speakers, a helper cat that walks about the window, and fixes from your tests.

- **Room EQ for your speakers** (Main's Settings → Room EQ): up to eight EQ bands, a delay and a level for each speaker, for what the computer plays to "Audra Main 1/2". Boosts are held to +6 dB at any frequency, and every change glides, switching off included. Your mic heard straight through the unit isn't tuned.
- **Every switch ticks on and off**, by mouse or keyboard.
- **A helper cat:** a grey tabby with its little speaker walks about the window, resting on the empty stretches (the top bar, the row headings) and never over the strips' controls. Drag it anywhere; poke it and it mews and says a tip you haven't found, or shows your badges. It blinks, sings along with your speakers, purrs when picked up. Switch it off on the View page or in Audra settings → App.
- **Make it heard can be undone in place** (Reverb & Echo): once pressed, its key stays, lit, as **Turn it back down**, which lowers only what Make it heard turned up, even after other changes. With an open mic in the speakers, the move to the headphones still comes first.
- **The top of the right-hand column no longer changes the sound in one click:** "Reverb not heard here · see how" opens Reverb & Echo, where the key is.
- **Control Room heading:** "where you listen" by its name, and "listening keys" over the Dim/Recall column.
- **Safer Undo with the effects:** a snapshot or Undo that brings an FX send back up fades the reverb in instead of bringing it in at once.
- **Small fixes from your tests:** PAD's chip has room beside the name; "Where it plugs in" has one size of icon, lined up; "In recordings" says what you hear stays the same.
- **README:** says which driver Audra's package is (babyface-pro-linux v0.2.2 with Audra's changes).

## 0.1.3 — early preview (2026-11-23)

A Mixer that looks like Audra's own, safer workspaces, and a README that says what Audra adds.

- **A new Mixer look:** each strip's name sits on cream label tape in Audra's serif, the strips stand side by side on one desk, and on wide windows the view tabs say what they're for ("start here", "a mix per output", "every route"). PAD, DIM and LOOP keep their warning colours, and pressing Dim never moves the row.
- **Workspaces keep what they replace:** loading a workspace file or a quick slot keeps your setup from before, and Workspaces → "Bring back the workspace from before" restores it (press again to undo). A file that doesn't fit the unit is refused before anything changes, Dim, Talkback and Live Mode included, and a file without snapshots or layouts leaves yours alone.
- **The "where it goes" map** now warns "open mic · can howl" when an open mic's reverb plays in your speakers.
- **README and wiki:** the README leads with what Audra adds and how it relates to TuxMix, and the pictures and animations show the new look.

## 0.1.2 — early preview (2026-11-23)

Security and safety fixes from a two-reviewer audit of everything that takes input from outside: files you open, OSC, MIDI, the control socket, the USB connection, and the build and release pipeline.

- **Files you open** (snapshots, workspaces, presets, themes) can't change what an input is, push a preamp or EQ past the panel's range, switch LOOP on, change the clock, or crash Audra with a malformed mix. A recall that leaves LOOP off says so.
- **OSC, MIDI and Mackie control** can't switch 48V or LOOP on by themselves (they wait for your own presses), and many small raises glide up like one big one instead of adding up to a loud jump. An OSC message for a channel the unit hasn't got is ignored instead of crashing Audra.
- **Safer by default:** private per-user folders instead of shared `/tmp` names; channel names can't add PipeWire settings; files over 16 MB aren't read; the USB connection no longer frees memory still in use after a fault.
- **Release pipeline:** the public copy and the packages are checked for personal data and secrets, and stop if any turns up. GitHub's CodeQL scanning is on.

## 0.1.1 — early preview (2026-11-23)

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
