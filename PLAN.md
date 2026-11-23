# Plan

> Issue numbers here (#5, issue #28) refer to the project's earlier private tracker, not this repository's issues.

Build a full-featured hardware mixer for the **RME Babyface Pro FS in its regular (proprietary, `2a39:3fc0`) USB mode** on Pop!_OS. It should work and look the way the Windows mixer does. The app is new, but it builds on existing open-source work instead of re-deriving the USB protocol.

Last updated 2026-10-30.

## Decisions

| Decision | Choice |
|---|---|
| Device mode | Regular/proprietary mode only. Class Compliant mode is out of scope |
| New app license | GPL-3.0-only. TuxMix's GUI widget code (GPL-3.0-only) may be reused with attribution |
| UI toolkit | Rust + iced 0.14. The desktop is COSMIC, which is built on iced, and `tuxmix-core` is Rust |
| GUI base | Upstream TuxMix GUI v0.3.5 (GPL-3.0-only, Babyface-native), imported and evolved toward the Windows mixer's layout and behaviour per `docs/ui-spec.md`. Ideas are borrowed from the tuxmix-fireface fork's panels (Control Room, inline EQ, reverb/echo). This replaces the hand-built scaffold, which the user found plain and unfamiliar. Decided 2026-10-30 |
| Name and look | **Audra** (chosen 2026-10-30): gentle, warm, a woman's name built on *aud-*, "to hear". The palette is a warm plum-charcoal dark with a rose-gold accent, champagne faders and ivory text, plus an original "A" mark whose crossbar is a fader cap. It follows mixer layout conventions (rows, strips, Control Room, matrix) in our own visual design. No RME artwork, and no "TotalMix", "Babyface" or "RME" in the product name (RME trademarks) |

## Architecture

| Path | Origin | License | Role |
|---|---|---|---|
| `driver/` | [babyface-pro-linux](https://github.com/ismail-bahloul/babyface-pro-linux) v0.2.2, git subtree | GPL-2.0-only | Kernel driver `snd-usb-babyface-pro`: PCM and mixer as ALSA controls |
| `crates/tuxmix-core/` | [TuxMix](https://github.com/ismail-bahloul/TuxMix) v0.3.5 `tuxmix-core/`, subtree split | MIT | Device model, ALSA backend, simulated device, scenes, meters |
| `crates/tuxmix-usb/` | TuxMix v0.3.5 `tuxmix-usb/`, subtree split | GPL-3.0-or-later | libusb-free USB backend. Keeps core building unmodified; used for FX-channel prototyping |
| `crates/app/` | TuxMix v0.3.5 `tuxmix-gui/`, subtree split, then evolved | GPL-3.0-only | **Audra**, the GUI (binary `audra`) |
| `crates/fx/` | new (Phase 4) | GPL-3.0-only | Host-side Reverb/Echo |

TuxMix's own GUI, TUI and `tuxmix-sys` are not imported.

## Phases

### Phase 0: run the existing stack. Done 2026-10-30

- The driver is installed through DKMS from `driver/tools/kernel` on kernel `7.2.6-76070206-generic`. Card `BabyfacePro` appears, and PipeWire sees it.
- TuxMix 0.3.5 is installed from its `.deb` as a reference to compare against.
- PipeWire picked the `stereo-fallback` profile. `pro-audio` exposes all 12 channels.

### Phase 1: skeleton. Done 2026-10-30

- Driver, `tuxmix-core` and `tuxmix-usb` imported as subtrees, with history preserved.
- Cargo workspace, plus the upstream license texts next to the imported crates.
- `babyface-mixer` scaffold: three rows of strips editing one submix at a time. `--mock` uses the simulated device; `--screenshot <png>` saves the window.

### Phase 2: mixer parity

- Stereo-pair strips with a channel-linking model.
- Fader law in dB. Expose or mirror core's `fader_raw_to_db` curve, and keep any change upstream-friendly.
- Level meters (core `pw_meter`, `capture_meter`).
- Pan, mute, solo, phase, width, M/S.
- Input settings: gain, 48V, PAD, sensitivity/reference level, AN1>2, link.
- EQ panel (3 bands, low cut) with a response curve.
- Matrix view.
- Output settings: loopback, cue.
- Device settings: clock, sample rate, optical ADAT/S/PDIF, pitch/varispeed.
- Our own visual design, with the screenshots checked against the reference.

### Phase 3: Windows mixer features built on existing device calls

Most of these are app logic over calls core already has. Exceptions, from `docs/ui-spec.md` §13:
- **Mono** needs cross crosspoints, which the driver doesn't expose.
- **Mute FX** needs the FX-return crosspoints from Phase 4.
- **Talkback, Mono, External Input and Mute FX** showed no USB writes in the Windows captures, so how Windows does them on the wire is unconfirmed.

- Control Room: Main and Phones outputs, Dim, Recall, Mono, Talkback, Speaker B, External Input, Mute FX, Cue/PFL. **Assign done 2026-11-03** (Main, Main B, Phones 1/2, Cue plays on, in Audra settings). **Speaker B and Link done 2026-11-03**: the manual says only that Speaker B "switches audio from Main Out to Main Out B"; Audra reads it, with the RME forum, as one of the two playing at a time with Main B playing Main's mix (Main B's own submix is kept but not heard), Link ganging the two masters, and Dim, Recall, External Input, the DIM button and "Cue plays on Main" following whichever pair plays. **Talkback to flagged outputs and Listenback done 2026-11-03**: a TALKBACK toggle in an output's Settings sends Talkback there as to the Phones (manual 21.3). Listenback (21.8.1) has its settings (mic, none by default as in the manual, and Main dim); the manual gives it no button, so a LISTEN key in the counter's third row is Audra's choice, shown only once a mic is chosen. While on, the mic plays on the playing monitor pair at its own fader in Main's submix (0 dB when that is off), the rest lowered by the Listenback dim.
- Submix view, copy/paste/mirror/clear submix. **Copy, paste, clear and mirror done 2026-11-03** (`mixops.rs`), for submixes and for input/playback mixes. Differs from the manual on purpose: the actions sit in each strip's Settings panel, which a right-click opens, instead of a right-click menu (iced has no context menu); a direct edit of a mirror's target ends the mirror (the manual does not say); "paste FX" is not offered until there is FX to paste.
- Snapshots, mute/solo/fader groups, layout presets, scroll markers. **Snapshot names and files done 2026-11-03** (manual 21.5.2): right-click a slot to name it (a double-click recalled it first); Store… then a slot stores, in TotalMix's order, without recalling; "Save to file…" / "Open a file…" in the Snapshots tab keep one snapshot as a `.audrasnap` file (opening loads it into the mix; Store puts it in a slot). **Ctrl-click folding done 2026-11-03**: Ctrl-click on the collapse button folds or opens the strip and every strip to its right. **Channel Layout done 2026-11-03** (manual 21.5.3): Hide in a strip's Settings panel, a Channels chip list in the Layouts tab, "n hidden" in a row's head, show: layout / all / in mix (the manual's All and Sub); the mixer and the matrix leave hidden channels out, which keep playing; layouts, prefs and workspaces keep the set. A linked strip hides both channels; Main and the Phones always show. Keeping a channel from MIDI and OSC while the mixer shows it: done 2026-11-03 (the Channels list's "for: remote"). **RMS metering done 2026-11-03** (manual 21.5.1, 21.7): the taps measure RMS beside the peak; every meter shows a solid RMS bar under lighter peaks; Settings → Meters has RMS and RMS +3 dB, and **Over** (the OVR sample count, 1 to 10; done 2026-11-03). **Matrix Mono Mode done 2026-11-03** (manual 22): "edit pairs: stereo / mono" beside the Matrix's title; mono moves only the cell's own channel of a stereo pair. **Snapshot "do not load" done 2026-11-03** (Settings, Snapshots): a load can leave the Main (and Main B) volume, or the Main and Phones volumes, as they are. The manual's third choice, Control Room settings, is left out on purpose: Audra keeps those in its prefs, never in a snapshot, so a snapshot cannot change them.
- Preferences, hotkeys, MIDI remote control, OSC.
- Front-panel integration: the driver exposes buttons, wheel and DIM as read-only controls.

### Phase 4: Reverb and Echo

The unit computes these on the **host CPU**, not its DSP (manual §21.6). The 14-channel USB frame carries the FX send on IN ch12/13 and the FX return on OUT ch12/13 (`driver/tools/usbdump/PROTOCOL.md`, "FX channels = frame ch12/13"). The driver currently **does not expose** ch12/13.

1. Driver patch: expose FX send and return to ALSA, for example as a second PCM device or extra channels. Prototype first through `tuxmix-usb` if that's faster. Offer the change upstream.
2. `crates/fx`: our own reverb types and echo types (stereo, cross, pong), running between those channels through PipeWire. These are new algorithms and won't sound identical to RME's.
3. Reverb/Echo panels, FX send/return in the strips, Mute FX.

### Not applicable on Linux

DAW mode, ASIO Direct Monitoring, "Store for all users" and DIGICheck are Windows/ASIO concepts. Network remote control (TotalMix Remote) is covered by OSC.

## Driver and core gaps

These are from the feature map in `docs/ui-spec.md` §13, where 16 rows are marked `missing`. They are features the Windows mixer has that the driver or core can't drive yet. Each needs protocol or driver work, ideally sent upstream:

- **Cross crosspoints** (L→R / R→L per source). Needed for panning split digital and playback channels, Mono, and the crossed matrix cells. The driver exposes a 6×14 own-side matrix only.
- **Per-strip processing beyond the analog inputs.** Width, M/S and FX Send exist only on AN1/2. Phase and EQ/Low Cut exist only on AN1–4; none on AS/ADAT inputs, playback or outputs.
- **Missing core setters:** EQ for Record (no driver control either). Reference-level "Boost" is done (2026-11-03, issue #5): the ALSA backend sets the driver's "Instrument Ref Level" by item name, and reads all three states back.
- **Missing readbacks:** optical input lock, sync and rate; post-EQ metering; front-panel button hold durations (Recall on a 2 s DIM hold, storing on a 2 s SET hold).
- **Front-panel ownership:** the driver acts on SET, MIX and OUT itself. A report-only mode would let the app map them as hotkeys.
- **DIM press stays inside core:** `tuxmix-core`'s ALSA panel poll consumes the driver's "DIM Button Press Count" and toggles the driver's fixed Phones dim itself (`babyface.rs`). For the unit's DIM to follow the Control Room's Main dim, core must hand the press to the app instead, as an event or hook. That's a small API addition worth offering upstream, after asking the user.

App-side decisions the spec raises:
- **Cue** is a strip button on outputs that replaces Solo. The ALSA backend has no `set_cue`, so implement it over `set_volume`.
- **Solo:** done 2026-11-03. Solo is the app's own (`control_room::set_solo`), on the current submix only, with Exclusive Solo as an option (on by default, as before). Core's `set_solo` (global, always exclusive) is no longer called. A strip's **Mute** stays global, as the manual has it (p. 49).
- **PFL / Live Mode:** done 2026-11-03 (manual 21.7, "PFL replaces Solo"): View tab "S button: solo | PFL". In PFL the Cue target plays only the strips in PFL, at 0 dB before their faders and mute (a linked pair hard left and right, a mono strip centred), Talkback still lowering it on a Phones; Exclusive Solo/PFL applies. Switching the mode releases whatever Solo or PFL was on; snapshots, undo and quit release PFL like solo. The manual gives no pan or level for PFL; 0 dB and the pair's own stereo image are Audra's reading of "pre fader".
- **Trim Gains:** done 2026-11-03 (manual 21.3, 21.5.1): with it on, a fader step into the current submix moves the channel's other active routings by the same dB (a routing at −∞ stays; a fader starting at −∞ passes nothing on); outputs flagged NO TRIM (their Settings panel) are left alone. On per strip with TRIM GAINS in the strip's Settings panel, or for every strip with the View tab's "trim gains: all" (the manual's View Options Trim). The strip's settings icon stays lit while it is on. Differs from the manual on purpose: the manual's per-strip T button is Trim Gains, but Audra's T button opens the hardware input trim, a real device control (`set_trim`) kept as it is, so Trim Gains sits in the Settings panel instead. The manual's triangles for the hidden routings are drawn (2026-11-03): small coral triangles beside the rail at each other active routing, moving with the fader. Coral rather than the manual's orange, because ember orange means 48V in Audra's palette and coral already marks Trim Gains as engaged (the lit settings icon). The cap stays the current routing's level rather than the highest one's (the manual's choice), so no white triangle is needed.
- **Lock User Interface:** done 2026-11-03 (manual 21.7, "freeze the current mix state. Faders, buttons and knobs relating to the mix state cannot be moved anymore"): a padlock in the top bar, beside the device line, so it shows from every view; lit and closed while locked, and remembered. Locked, every mix change is held, whether it comes from a control, a key, a snapshot or OSC: faders, pan, mute, solo, EQ and EQ presets, input gain, 48V and the other preamp switches, copy/paste/clear, undo and redo. A short notice says why nothing moved and where to unlock. Still working: the Control Room (Dim, Recall, Cue, Talkback, Ext In, Speaker B, Assign), PFL in Live Mode (it only listens), the device clock and sample rate, views, settings, renaming, storing a snapshot (it changes nothing), and the window. The manual's password option is Windows-only and left out. Switching Live Mode releases every solo, so it's held while anything is soloed. Audra's reading: preamp gain and 48V count as "knobs relating to the mix state", so a stray click can't change a live mic. RECALL counts as monitoring, like DIM, even though it moves Main's level: the unit's own volume knob moves Main whatever the app does, because the hardware isn't locked.
- **Reset Mix:** done 2026-11-03 (manual 21.10), at the foot of Audra settings. Each entry takes two clicks, the first arming it (amber, "Click again") for 4 s, so a stray click mid-session can't silence the monitoring (TotalMix keeps them two clicks deep in a menu). Every entry but the names is one undo step. Audra's readings: Straight playback rewrites only the playback channels' routing (inputs are "Clear all submixes" territory) and leaves output faders alone, as the manual says. Total Reset is straight playback with everything also on Main, every other routing cleared, channel effects off, every mute, solo, phase invert and loopback off, and every Control Room override released, and it leaves the output faders alone: a reset never turns anything up. Clear channel effects covers EQ, low cut and Width (Reverb and Echo are Phase 4). The ADM entry is Windows-only and left out.
- **Mixer Views preferences:** done 2026-11-03 (manual 21.7), in Audra settings (MIXER): centre the pan when a pair is split; no double-click fader reset; mouse wheel scrolls the page instead of moving faders, knobs and EQ handles; layouts keep the open strip panels (a layout file from before still loads). The FX Send options wait for Phase 4.
- **Motion and sound:** done 2026-11-03, at the maintainer's request (Audra settings, MOTION AND SOUND). Animations, on by default: a row that opens grows its faders into the room (260 ms), and a toggle or Control Room key that lights flares like an LED and settles (220 ms); nothing runs continuously. UI sounds, on by default (the maintainer's choice, changed from off on 2026-11-03): clicks for on and off, a soft tap for every other press, a detent tick when a fader crosses 0 dB or a pan crosses the centre, and a soft chime when moving to another view, row or mix, with a volume. A speaker toggle in the top bar turns them on and off in one click (2026-11-03). They are synthesized in the app and played with PipeWire's `pw-play` (no audio crate), as a Notification stream named "Audra UI sounds" so the desktop can route them away from the interface being monitored; a stuck player is ended after 2 s.
- **Level meter preferences:** Peak Hold done 2026-11-03 (manual 21.7; Audra settings, METERS): each meter holds its peak as a 2 px line, in the colour of the zone it sits in, for 0.1 to 9.9 s (default 1 s), then drops to the current reading. The manual's other two, full-scale samples for OVR and RMS +3 dB, wait on sample-level meter data: the capture and PipeWire paths report one peak per block, so the clip light takes the first full-scale reading and there is no RMS to shift. The Settings note says so.
- **Scroll location markers:** done 2026-11-03 (manual 21.5.4). While a strip row is wider than the window, its head carries the manual's four: to the first strip, markers 1 and 2 (right-click stores where the row is, a click goes back there), to the last strip. Side by side, inputs and playback scroll as one and both heads carry the same set. The manual keeps them in the workspace; Audra has none yet, so they are kept with the preferences. On the way: the top bar's device chip now steps down (the state word alone, then the name alone) instead of being cut off at large zoom.
- **Workspaces:** done 2026-11-03 (manual 21.9, 21.10). The Workspaces panel (W, or "Workspaces…" in the Snapshots tab) has the manual's 30 quick slots, each with Load and Store, plus opening and saving a `.audraws` file anywhere. A workspace holds the mix, the 8 snapshots, the groups (which are kept nowhere else), the layouts, the EQ presets, the channel names, the collapsed strips, the row, solo, Live Mode and Trim Gains settings, the mirrors and the scroll markers; window, zoom and Audra's own settings stay with the computer. Audra's readings: Load takes two clicks, as it replaces snapshots, layouts and groups that Undo does not cover (the mix itself is one Undo step, as the manual says); a load keeps Main (and the Phones) as the Snapshots setting says, so it never jumps the monitoring; Alt+1–8 stay on the snapshots (TotalMix puts workspaces there). The simulation keeps its slots in memory.
- **Key Commands:** done 2026-11-03 (manual 21.10, Audra settings → KEY COMMANDS). F4 to F8 and the unit's DIM button can each be set to Dim, Recall, Talkback, External Input, Speaker B, Listenback, the mute or solo master, or locking the mix. A key sends its button's own message, so the lock and undo treat it the same way. Every function toggles: the unit reports DIM as a press count with no release, so hold-to-talk can't be offered for it, and the F keys follow the same rule. F4 to F8 start free; DIM starts on Dim, as before. SET, MIX and OUT stay with the driver (issue #3).
- **Free routing:** done 2026-11-03 (manual 21.3, 21.5.1, Routing Mode). The View tab's "routing: submix | free": in submix (the default) every strip edits the placard's mix; in free, each input and playback strip has a routing field, the output its fader and pan edit, listing every output with a dot on the ones it already reaches (the manual's arrows). A strip not set follows the placard. Each strip's pick lasts for the session.
- **MIDI remote control (generic):** done 2026-11-03 (manual 24; the maintainer chose notes and CC first, Mackie later). Audra is an ALSA sequencer client "Audra" with an input port, connected by itself to every MIDI controller present and to each one plugged in later (through the announce port). In Audra settings → MIDI: switch it on, pick a target (a strip's fader, mute or solo, the Main or Phones level, or a Key Commands function), press Learn and move a control. A control change moves a fader on its own taper (127 = the fader's top); a note toggles a mute, solo or function on press, a control change sets it (64 and up = on). A binding sends its target's own message, so the lock and undo apply. Uses the `alsa` crate tuxmix-core already used. **Mackie Control: done 2026-11-03** (`mackie.rs`): a chosen surface drives a block of eight strips, with banking, function keys, and feedback (motor faders, lights, rings, the display, meters) through a second sequencer client; checked with aseqdump, not on a real surface. An extender (left or right, sixteen strips) followed the same day. Still to come: the unit's own MIDI port.
- **Groups (differs from the manual, kept on purpose, 2026-11-03):** the manual's mute and solo groups are buttons you push, and a mute group mutes its members in the current submix only (p. 50). Audra's groups are links instead: with M1 (or S1, or 1) lit, pressing one member's Mute (or Solo, or moving its fader) acts on every member, through the same button. So a mute group mutes globally, like the strip Mute it gangs, and a solo group already acts on the current submix only, like solo. Why kept: one rule (a group gangs the strip buttons) instead of two kinds of mute, membership is set from the normal multi-select, and no new strip state is needed to show "muted here by a group" next to the strip's own Mute. The cost the manual warns about: a mute group can silence its members on every output, Main included. Revisit if that bites.
- **Dim** is a configurable cut on the assigned Main output. The driver's `Dim Switch` is a fixed −20 dB on the Phones only.
- **The SET button** toggles 48V in the driver. In the manual, SET only switches phantom in stand-alone mode; with a computer attached it's a hotkey. Decide whether to change the driver.
- **Ranges:** done 2026-11-03: the app holds EQ to the manual's ranges (±20 dB, Q 0.7–5, Low Cut 20–500 Hz), though the driver takes more (`widgets/eq_plot.rs`).
- **EQ presets:** done 2026-11-03 (`eq_presets.rs`, manual 21.3): 16 user slots shared by every channel, a factory set of Audra's own (14, like the manual's), Reset, rename by saving under a new name. A preset never switches the EQ or Low Cut on or off. Differs from the manual on purpose: the slots live in the prefs, since Audra has no workspaces; Import/Export (added with the user's yes to `rfd`, the desktop portal's file dialog) read and write Audra's own `.audraeq` JSON, not RME's undocumented `.tmeq`.

## Constraints and known issues

- **Mixer state is write-only:** the firmware has no mixer readback. The driver caches state and restores it on re-probe and resume, and loading the driver resets the mixer to its defaults.
- **SET button:** pressing SET on the unit toggles 48V on the selected mic (driver behaviour; see Driver and core gaps).
- **Mono mic on both lanes:** a mic on input 1 records on the left channel only. The fix is the unit's own **AN1→2** feature (manual §21.3: copies input 1 to channel 2 "so a microphone is audible on both channels of a stereo recording"). It needs the AN1/2 pair in stereo mode: ALSA `AN1/2 Link` on, then `AN 1>2` on. Verified 2026-10-30: the raw `Babyface Pro Stereo` capture went from L only (correlation −0.002) to identical L/R (correlation 1.000). Neither switch rewrites crosspoints in the driver, so monitoring levels are unchanged.
  - No PipeWire virtual sources are needed. A two-stage `module-remap-source` chain also works, but it was dropped: the user wants nothing scattered across the system.
  - **Persistence comes from ALSA, not from the app.** The driver resets the mixer on every load, but every setting is an ALSA control. Pop!_OS's `alsa-restore.service` runs `alsactl store` at shutdown and `alsactl restore` at boot, and `90-alsa-restore.rules` restores a card when it appears. TuxMix's ALSA backend relies on this deliberately: its `restore_auto_scene` re-applies only channel names. Until the first shutdown after the card was added, a replug comes back at the driver's defaults; `sudo alsactl store BabyfacePro` closes that gap.
  - **The app's job:** expose the input setup clearly (48V, gain, PAD, link, AN1→2 per input) and offer start-with-desktop (already in the tray module).
- **Upstream issues** to watch: TuxMix [#23](https://github.com/ismail-bahloul/TuxMix/issues/23) (channel bleed in proprietary mode) and [#20](https://github.com/ismail-bahloul/TuxMix/issues/20) (non-FS protocol differences).
- **MIDI port:** the driver leaves the USB-MIDI interface to `snd-usb-audio`, which doesn't bind it in this mode, so hardware MIDI isn't available yet.

## Packaging

**`.deb` packages: done 2026-11-03.** `packaging/build-deb.sh` builds `audra` and `snd-usb-babyface-pro-dkms` (README, "Install from packages"). Still to do: an AppStream file (for the store listing), a signed apt repository or release uploads, and deciding on Flatpak.

## Themes

**Built-in themes: done 2026-11-03** (the maintainer asked for "light, gray, contrast, blue, red, monokai etc themes and ... an easy way to switch"). Nine palettes: Audra, Daylight, Graphite, High contrast, Midnight, Crimson, Monokai, Nord, Solarized. Switched from the View tab's grid of tiles, each drawn in its own theme's colours, one click, remembered. Step 2 below is done: the colour roles are accessors (`theme::text_sec()` and so on) reading a per-thread current palette that every draw sets from the choice, and iced's own theme follows it. Every theme holds the same contrast rules (a test). The meters keep Audra's bright zone colours in every theme; the wordmark is recoloured to each theme's ink and accent.

**The theme by time of day: done 2026-11-03** (the maintainer: "Allow color of theme to change by time of system. It should be a setting"): Audra settings → Appearance, a day and a night theme with their start times; the window fades between them (`themetime`).

**Themes people make themselves: done 2026-11-03.** `~/.config/audra/themes/<name>.json` (a built-in theme to start from and the colours it changes) loads into the picker, checked with the built-in themes' rules (`theme::contrast_issues`); one that falls short loads with an amber dot and says where. Save as your own, Open folder and Reload sit under the tiles.

## UI reference

[`docs/ui-spec.md`](docs/ui-spec.md) is the functional inventory of every view, control and interaction to build in Phases 2–4. It maps each feature to a device call, host-only logic, FX work or "not applicable". It was compiled from:

- the *Babyface Pro FS User's Guide*, chapters 21–26 (which features this unit has)
- RME's online TotalMix FX documentation, for UI behaviour in detail: [user interface](https://docs.rme-audio.com/aoxd/810-1c_totalmix_user_interface/) and the chapters linked from it, the [TotalMix FX overview](https://rme-audio.de/totalmix-fx.html), and the [RME manuals index](https://docs.rme-audio.com/shared/)

## Updating the imported code

```sh
# Driver: pull a new upstream tag, then reinstall through DKMS under the new version
git subtree pull --prefix=driver https://github.com/ismail-bahloul/babyface-pro-linux <tag>

# tuxmix-core / tuxmix-usb: re-split upstream, then pull the split branch
git clone https://github.com/ismail-bahloul/TuxMix /tmp/TuxMix && cd /tmp/TuxMix
git subtree split --prefix=tuxmix-core <tag> -b split-core
git subtree split --prefix=tuxmix-usb  <tag> -b split-usb
cd -
git subtree pull --prefix=crates/tuxmix-core /tmp/TuxMix split-core
git subtree pull --prefix=crates/tuxmix-usb  /tmp/TuxMix split-usb
```

Keep local changes inside the imported directories small and suitable to send upstream, so these pulls stay clean.
