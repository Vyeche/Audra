# Research: Linux support for the Babyface Pro FS

Research notes on whether a TotalMix FX–style mixer can run on **Pop!_OS** for an **RME Babyface Pro FS**: what the hardware exposes, what Linux already supports, and what's still missing.

> **Written 2026-10-30, kept as it was then.** This survey is what led to [`PLAN.md`](../PLAN.md) and to Audra; the [README](../README.md) describes the app. Some details have since changed: the driver and TuxMix were installed and the open questions marked resolved below were answered. The "This machine" table is a snapshot from before that work.
>
> Not affiliated with RME. *TotalMix*, *TotalMix FX*, *Babyface Pro* and *Babyface Pro FS* are trademarks of RME Intelligent Audio Solutions (manual, "Trademarks").

## Short answer

**It's possible, and most of the work already exists.** Two community efforts cover the Babyface Pro FS, one for each of its USB modes:

1. **Proprietary mode (PC mode, `2a39:3fc0`)**, the mode this unit is in now. An out-of-tree ALSA driver, [`snd-usb-babyface-pro`](https://github.com/ismail-bahloul/babyface-pro-linux), plus a TotalMix-style GUI, [TuxMix](https://github.com/ismail-bahloul/TuxMix). The author reports all 12 channels at 32–192 kHz and most of the DSP mixer, including EQ and meters. Both projects are about two months old. The driver was sent to ALSA as a v7 patch series on 2026-10-27 and has not been merged.
2. **Class Compliant mode (`2a39:3fb0`)**, which works with the stock Pop!_OS kernel today. Audio goes through `snd-usb-audio`, and a mixer quirk built into the kernel exposes routing, master volumes, preamp gain, 48V, PAD, input sensitivity and clock as ALSA controls. Several small GUIs sit on top. **No shipped Linux tool drives EQ, low cut, reverb or echo in this mode.** The EQ/low-cut protocol is documented (it's the SysEx the iPad app sends, written up on the oscmix wiki), but nobody has implemented it.

**Right now the Babyface does nothing on this machine.** It enumerates in proprietary mode, no kernel driver binds to any of its six interfaces, and ALSA has no card for it.

What the links that started this turned out to be:

- [oscmix](https://github.com/michaelforney/oscmix) does **not** support the Babyface Pro. Its device files cover the Fireface UCX II and 802. [Issue #4](https://github.com/michaelforney/oscmix/issues/4) is "Fireface 802 support". The Babyface issue is [#9](https://github.com/michaelforney/oscmix/issues/9). It's still open; the protocol is documented on the wiki, but no code has been merged.
- [RME forum thread 42994](https://forum.rme-audio.de/viewtopic.php?id=42994) asks for an official Linux TotalMix. It has no technical content, and RME closed it on 2026-02-17 ("please discuss this elsewhere").
- [RME forum thread 38917](https://forum.rme-audio.de/viewtopic.php?id=38917) is the oscmix announcement thread. Babyface Pro FS owners have been asking for support there since December 2024.

## This machine

| | |
|---|---|
| OS / kernel | Pop!_OS 24.04 LTS (noble), `7.2.6-76070206-generic` |
| Audio stack | PipeWire 1.6.8, WirePlumber 0.5.12, `pw-jack` |
| Driver build prerequisites | Secure Boot **off**, DKMS 3.0.11, headers for the running kernel installed |
| Toolchain | gcc/make, Rust (cargo), Python 3, Node. Missing: meson, ninja, cmake, `libasound2-dev`, `libusb-1.0-0-dev` |
| Device | `2a39:3fc0` "Babyface Pro", USB 2.0 high speed, 6 interfaces, **no driver bound** |

[`hardware-snapshot.md`](hardware-snapshot.md) has the full descriptors, the interface map and the kernel module details.

## The two USB personalities

Depending on its mode, the Babyface Pro FS presents a completely different USB device.

| | Proprietary / PC mode | Class Compliant (CC) mode |
|---|---|---|
| USB ID | `2a39:3fc0` (this unit, today) | `2a39:3fb0` |
| How to tell | Nothing shown at power-on | Meters show **CC** at every power-on |
| Audio on Linux | Needs the out-of-tree `snd-usb-babyface-pro` (DKMS). PCM runs on the **interrupt** endpoints of interface 5 | Stock `snd-usb-audio` (USB Audio Class 2.0) |
| Mixer control path | Vendor requests on EP0, either through the driver's ALSA controls or directly over libusb (TuxMix's USB backend) | Vendor requests on EP0 (the kernel's `bbfpro` quirk, exposed as ALSA controls), **or** SysEx over USB-MIDI (the iPad app's protocol) |
| EQ on Linux | Yes, through driver + TuxMix (hardware-validated, according to its author) | Not implemented anywhere; protocol documented |
| Protocol reference | [`tools/usbdump/PROTOCOL.md`](https://github.com/ismail-bahloul/babyface-pro-linux/blob/main/tools/usbdump/PROTOCOL.md) in the driver repo | [`sound/usb/mixer_quirks.c`](https://github.com/torvalds/linux/blob/master/sound/usb/mixer_quirks.c) (`snd_bbfpro_*`) and the oscmix wiki's [Babyface Pro page](https://github.com/michaelforney/oscmix/wiki/Babyface-Pro) |
| RME's position | The mode RME's Windows/macOS drivers and TotalMix FX use | "There will be no (Total) Mix and no effects" on a computer; full control only from TotalMix FX for iPad (manual §30) |

RME's statement is about its own software. In CC mode the DSP is still in the unit, and Linux reaches part of it through the kernel quirk.

### Switching modes

From the official [Babyface Pro FS manual](https://www.rme-audio.de/downloads/bface_pro_fs_e.pdf), §33.2:

> Hold down the keys SELECT and DIM while inserting the USB cable. During the boot process the level meters show CC for about 2 seconds, confirming the CC mode has been activated. The Babyface Pro stays in CC mode until this procedure has been performed again, it will then show PC.

With an external power supply (or USB bus power without a data connection), holding SELECT + DIM for two seconds toggles the mode at any time.

## What the hardware can do

According to the manual (§4, §33), the unit has 12 inputs and 12 outputs:

- 4 analog inputs: 2 mic/line XLR, plus inputs 3/4
- 4 analog outputs: main XLR 1/2, plus 3/4, which feed both phones jacks
- 8 channels of ADAT or 2 of S/PDIF on optical

It runs up to 192 kHz and has one MIDI I/O. The mixer is a 288-crosspoint TotalMix (12 outputs × 24 sources). **TotalMix FX adds 3-band EQ, Low Cut, Reverb and Echo.** No dynamics section is listed for this model.

## Feature coverage on Linux

| Feature | PC mode: driver + TuxMix | CC mode: stock kernel quirk | CC mode: iPad SysEx protocol |
|---|---|---|---|
| Audio 12 × 12, up to 192 kHz | ✅ (2–12 ch, 32–192 kHz) | ✅ `snd-usb-audio` | n/a |
| Routing matrix | ✅ "6x14 crosspoint matrix" (6 outputs × 14 sources, per driver docs) | ✅ 12 outputs × (12 inputs + 12 playback) | 📄 |
| Output master volumes | ✅ 6 masters + mutes | ✅ `Main-Out *` × 12 | 📄 |
| Preamp gain, 48V, PAD | ✅ | ✅ (gain and main-out volume need kernel ≥ 6.12) | 📄 |
| Line input sensitivity / reference level | ✅ | ✅ | ❓ |
| Clock source, optical ADAT / S/PDIF | ✅ | ✅ | 📄 |
| 3-band EQ | ✅ | ❌ | 📄 (host computes the biquad coefficients) |
| Low cut | ✅ protocol decoded; in TuxMix's EQ panel | ❌ | 📄 |
| Reverb / Echo | ❌ FX send only. These effects run on the host CPU, and the driver doesn't expose the FX channels 12/13 yet ([PLAN.md](../PLAN.md#phase-4-reverb-and-echo)) | ❌ | ❓ |
| Loopback | ✅ | ❌ | 📄 |
| M/S, width, AN1>2, input link, varispeed | ✅ | ❌ | partly (AN1>2) |
| Level meters | ✅ in TuxMix | ❌ | 📄 (RMS + peak) |
| Front-panel buttons / encoder readback | ✅ read-only ALSA controls | ❌ controls are write-only | 📄 |
| Hardware MIDI port | ⚠️ unclear, see [open questions](#open-questions-unverified) | ✅ `snd-usb-audio` | n/a |

✅ implemented · 📄 protocol documented, not implemented in any shipped Linux tool · ❌ not available · ❓ unknown

## Existing projects

| Project | Mode | What it is | Activity | License |
|---|---|---|---|---|
| [ismail-bahloul/babyface-pro-linux](https://github.com/ismail-bahloul/babyface-pro-linux) | PC | Kernel driver `snd-usb-babyface-pro`, DKMS-ready; packaged only for the AUR | Created 2026-08-27; v0.2.2 on 2026-10-27; v7 patches sent upstream | GPL-2.0 |
| [ismail-bahloul/TuxMix](https://github.com/ismail-bahloul/TuxMix) | PC (driver or libusb), ALSA | Rust GUI (iced) and TUI (ratatui): Quick/Mixer/Matrix views, scenes, tray, OSC; ships `.deb` and `.rpm` | v0.3.5 on 2026-10-29 | MIT / GPL-3.0 |
| [jspx-projects/tuxmix-fireface](https://github.com/jspx-projects/tuxmix-fireface) | PC | TuxMix fork extended to the Fireface UC/UCX | Created 2026-10-28 | GPL-2.0 / GPL-3.0 / MIT |
| Linux `snd-usb-audio` `bbfpro` quirk | CC | In-tree ALSA mixer controls over EP0 vendor requests | Present in this machine's 7.2.6 module | GPL-2.0 |
| [djshiye/audius](https://github.com/djshiye/audius) | CC | Python GTK4/libadwaita panel: listen presets, dim, scenes, fade-in safety; targets Fedora | 2026-10-17 | MIT |
| [slowtec/bbfpromix](https://github.com/slowtec/bbfpromix) | CC | Rust/GTK3 port of bbfpromix | 2025-09 | GPL-3.0 |
| [MrBollie/bbfpromix](https://github.com/MrBollie/bbfpromix) | CC | Original C/GTK mixer front end | Unmaintained since 2022 | none stated |
| [stistrup/rme-control-gui](https://github.com/stistrup/rme-control-gui), [rme-control-cli](https://github.com/stistrup/rme-control-cli) | CC | Tauri/Vue GUI (its author no longer uses it) and a Rust CLI | 2026-03 | none stated |
| [michaelforney/oscmix](https://github.com/michaelforney/oscmix) | CC, other models | OSC daemon speaking RME's SysEx over USB-MIDI, with GTK and web UIs. **No Babyface support** | Active (Aug 2026) | see repo |

Caveats for the PC-mode stack:

- Both repos are one developer's work. They're weeks old, versioned 0.x, and release almost daily.
- Open TuxMix issues: [#23](https://github.com/ismail-bahloul/TuxMix/issues/23) "In proprietary mode, channels not linked properly, audio bleeds from one to another", and [#20](https://github.com/ismail-bahloul/TuxMix/issues/20), which covers protocol differences on the original (non-FS) Babyface Pro.
- The firmware has no mixer readback, so the driver caches mixer state and restores it on re-probe and resume.
- A third-party kernel module runs with full kernel privileges. Read the source before installing it.

## Paths forward on this machine (not executed)

### Path A: stay in proprietary mode (driver + TuxMix)

This is the closest fit to the goal: it's the only path with EQ and meters today.

```sh
# 1. Get the driver at the release tag and review it (it runs in the kernel)
git clone --branch v0.2.2 https://github.com/ismail-bahloul/babyface-pro-linux
cd babyface-pro-linux            # read tools/kernel/*.c and *.h first

# 2. Build and install with DKMS (from docs/install.md; version must match dkms.conf)
sudo cp -r tools/kernel /usr/src/snd-usb-babyface-pro-0.2.2
sudo dkms add     -m snd-usb-babyface-pro -v 0.2.2
sudo dkms install -m snd-usb-babyface-pro -v 0.2.2
sudo modprobe snd-usb-babyface-pro

# 3. Check
cat /proc/asound/cards
amixer -c <card> controls

# 4. TuxMix: .deb from the v0.3.5 release, verified against SHA256SUMS
gh release download v0.3.5 -R ismail-bahloul/TuxMix -p 'tuxmix_*.deb' -p SHA256SUMS
sha256sum -c --ignore-missing SHA256SUMS
sudo apt install ./tuxmix_0.3.5-1_amd64.deb
```

Pop!_OS notes:

- Secure Boot is off, so the DKMS module doesn't need MOK signing.
- `dkms.conf` sets `AUTOINSTALL="yes"`, so the module rebuilds when System76 ships a new kernel, as long as that kernel's headers are installed.

TuxMix can also drive the mixer over libusb with no kernel module, using its udev rule. That gives mixer control only: PipeWire has no audio device without the driver.

### Path B: switch to CC mode (stock kernel)

This needs no out-of-tree code, but you lose EQ, FX and meters.

1. Unplug USB, hold SELECT + DIM, and plug it back in. The meters should show **CC**.
2. `lsusb -d 2a39:` should now show `3fb0`, and `cat /proc/asound/cards` should list the Babyface.
3. Mix with `alsamixer -c <card>` or one of the CC-mode GUIs above.

Behaviour other CC-mode users have reported:

- The kernel controls are write-only: the device never reports state back, so front-panel knob changes are invisible to Linux tools (audius README).
- Changing an input gain can drop the main/phones volume until that volume is touched again (rme-control-cli README).
- CC mode defaults to AutoSync at 44.1 kHz. With no digital input, it follows the host's sample rate (manual §33.1).

The same SELECT + DIM procedure switches the unit back to PC mode.

## Where this repo could add something

These are options, not decisions:

1. **Contribute to the PC-mode stack.** Test the driver and TuxMix on Pop!_OS 24.04 with kernel 7.2 and report back, or package the driver as a Debian/Ubuntu DKMS `.deb` (only an AUR package exists today; TuxMix already ships a `.deb`).
2. **CC-mode EQ, low cut and meters over SysEx.** This is the one gap with a documented protocol and no implementation ([oscmix #9](https://github.com/michaelforney/oscmix/issues/9) and the [wiki page](https://github.com/michaelforney/oscmix/wiki/Babyface-Pro)). It could become a Babyface backend in oscmix or a standalone tool, and it would give a fuller mixer on the stock kernel with no out-of-tree module. The oscmix maintainer has said the Babyface is "so different from other Fireface interfaces" that supporting it will take architectural work.
3. **Reverb and Echo.** None of the sources found document this protocol. It would take new capture work: USBPcap against the Windows driver in PC mode, or the iPad app through a USB-MIDI gadget or proxy, as described on the oscmix [Capture](https://github.com/michaelforney/oscmix/wiki/Capture) page.
4. **PipeWire/WirePlumber glue** for either mode: a profile and channel names.
5. **A new front end** on top of either mode's ALSA controls. This is the most work, and it overlaps TuxMix and audius.

## Open questions (unverified)

- ~~**Kernel 7.2 build.**~~ Resolved 2026-10-30: `snd-usb-babyface-pro` 0.2.2 builds through DKMS on `7.2.6-76070206-generic` and creates the card. A source review found no red flags.
- ~~**TuxMix on noble.**~~ Resolved 2026-10-30: the 0.3.5 `.deb` installs cleanly (`libasound2t64` satisfies `libasound2`) and drives the unit.
- **MIDI in PC mode.** The driver deliberately leaves the USB-MIDI interface (IF2) for `snd-usb-audio` to claim. On this machine `snd-usb-audio` does not bind it, because that driver matches on AudioControl interfaces and PC mode exposes only a MIDIStreaming interface. The hardware MIDI port may not work in PC mode without an extra quirk.
- **SysEx in PC mode.** Does the unit accept the iPad's SysEx protocol in PC mode as well? Unknown.
- **Reverb/Echo control.** None of the sources found document it, in either mode.

## Sources

- RME, *Babyface Pro FS User's Guide* v1.5: §4 (features), §30 (modes), §32–33 (Class Compliant operation, switching modes), Trademarks. <https://www.rme-audio.de/downloads/bface_pro_fs_e.pdf>
- Linux kernel, `sound/usb/mixer_quirks.c`, "RME Babyface Pro (FS)" section and `USB_ID(0x2a39, 0x3fb0)` dispatch. <https://github.com/torvalds/linux/blob/master/sound/usb/mixer_quirks.c>
- oscmix wiki: [Babyface Pro](https://github.com/michaelforney/oscmix/wiki/Babyface-Pro), [Protocol](https://github.com/michaelforney/oscmix/wiki/Protocol), [Capture](https://github.com/michaelforney/oscmix/wiki/Capture)
- oscmix issues: [#4 Fireface 802 support](https://github.com/michaelforney/oscmix/issues/4), [#9 Babyface Pro support](https://github.com/michaelforney/oscmix/issues/9)
- RME forum: [38917 (oscmix)](https://forum.rme-audio.de/viewtopic.php?id=38917), [42994 (Linux TotalMix request, closed)](https://forum.rme-audio.de/viewtopic.php?id=42994)
- babyface-pro-linux: [features](https://github.com/ismail-bahloul/babyface-pro-linux/blob/main/docs/features.md), [install](https://github.com/ismail-bahloul/babyface-pro-linux/blob/main/docs/install.md), [UPSTREAM.md](https://github.com/ismail-bahloul/babyface-pro-linux/blob/main/tools/kernel/UPSTREAM.md), [LINUX-VALIDATION.md](https://github.com/ismail-bahloul/babyface-pro-linux/blob/main/LINUX-VALIDATION.md)
- TuxMix: [README](https://github.com/ismail-bahloul/TuxMix), [guide](https://github.com/ismail-bahloul/TuxMix/blob/main/docs/guide.md), [releases](https://github.com/ismail-bahloul/TuxMix/releases)
- READMEs of the CC-mode projects listed above
