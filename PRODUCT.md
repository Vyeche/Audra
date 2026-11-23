# Product

> Issue numbers here (#5, issue #28) refer to the project's earlier private tracker, not this repository's issues.

<!-- impeccable:product-schema 1 -->

## Platform

desktop

## Users

- **Primary:** people who own an RME audio interface and use Linux. This starts with the **RME Babyface Pro FS** in its regular (proprietary) USB mode and grows to other RME interfaces over time. The product owner is the first user, on Pop!_OS 24.04 with COSMIC.
- **Their jobs:**
  - voice, podcasting and streaming (a mic into calls, streams and recordings)
  - recording music, with zero-latency headphone mixes
  - mixing and listening on speakers and headphones
  - audio for video and content creation
- **Their situation:** RME's own mixer software, TotalMix FX, exists only for Windows and macOS (plus an iPad app in Class Compliant mode). On Linux, these owners otherwise can't reach the unit's onboard DSP mixer in regular mode.

## Product Purpose

Audra controls the hardware mixer of RME interfaces on Linux. It drives the unit's onboard DSP mixer through the `snd-usb-babyface-pro` kernel driver, so the interface works on Linux the way it does with RME's Windows mixer:
- routing and submixes
- input gain, 48V and PAD
- EQ and low cut
- loopback
- the Control Room: Main, Phones, Dim, Talkback

Success means three things:
- parity with the Windows mixer's feature set for the unit (`docs/ui-spec.md`)
- a workflow that people coming from TotalMix recognise immediately
- reliable behaviour on real hardware that never leaves the listener's monitoring in a bad state

## Positioning

- **Regular mode:** it works with the unit's full channel count and DSP mixer. The other Linux tools only cover Class Compliant mode (`bbfpromix`, `audius`, `rme-control-gui/cli`).
- **Built on credited work:** the `snd-usb-babyface-pro` driver and TuxMix by Ismaïl Bahloul (GPL-3.0). Audra's own contribution is the Windows-mixer workflow (Control Room and the full functional spec), its identity, and Pop!_OS integration.
- **Independent:** not affiliated with or endorsed by RME.

## Operating Context

- **Environment:** a native Linux desktop app (Rust + iced 0.14). Pop!_OS 24.04 / COSMIC on Wayland comes first, with the PipeWire / WirePlumber audio stack.
- **Daily use:** it runs beside DAWs, streaming software and call apps. It lives in the tray and can start with the desktop.
- **Shared control:** the unit's front-panel buttons (DIM, SET, the wheel) and the app both act on the same mixer.
- **No readback:** the hardware can't report its mixer state. The driver and the app are the source of truth, so every temporary override (Dim, Talkback, External Input) must be recoverable from the app's own state, including after a crash or replug.
- **Persistence:** Pop!_OS keeps mixer settings across reboots through standard ALSA state saving (`alsactl store`/`restore`).

## Capabilities and Constraints

- **Today:**
  - a three-row mixer (hardware inputs, software playback, hardware outputs), with the active submix highlighted
  - matrix view and EQ
  - scenes, snapshots, groups and layouts
  - a Control Room section (layout stage)
  - per-input gain, 48V, PAD and AN1→2
  - a simulated device for use without hardware
- **Constraints:**
  - The driver gaps are tracked in `PLAN.md`: no cross crosspoints (blocks Mono), per-strip width / phase / EQ only on some inputs, and FX channels 12/13 not exposed.
  - Reverb and Echo run on the host CPU on this unit (manual §21.6), so Audra has to provide them itself.
- **Terminology:** submix, crosspoint, Control Room, Main, Phones, Dim, Recall, Talkback, External Input, Cue, loopback, AN1→2.
- **Release:** public, with installable packages (`.deb` for Pop!_OS/Ubuntu, possibly Flatpak). The driver ships through DKMS.
- **Undecided:**
  - which RME models come after the Babyface Pro FS
  - Flatpak alongside `.deb` or not
  - whether the driver is merged upstream (a v7 series was sent to ALSA on 2026-10-27)

## Brand Commitments

- **Name:** **Audra**, chosen by the maintainer. It's a woman's name built on *aud-*, "to hear".
- **Character the maintainer asked for:** warm, bright, gentle and approachable, with a cool, attractive edge.
- **Trademarks:** no RME trademarks in the product name (TotalMix, Babyface and RME are RME's), and no RME artwork. Stating compatibility ("for the RME Babyface Pro FS") is fine.
- **Credit:** About credits TuxMix (GPL-3.0) as the base of the mixer.
- **Themes:** people must be able to create their own themes (planned; see `PLAN.md`).

## Evidence on Hand

- **Project documents:**
  - `docs/ui-spec.md`: functional inventory of the Windows mixer for this unit
  - `PLAN.md`: architecture, phases and driver gaps
  - `README.md`: Linux landscape research
  - `docs/hardware-snapshot.md`: USB descriptors
  - `log.md`: change log
- **Test hardware:** the maintainer's Babyface Pro FS with a Neumann condenser mic on input 1.
- **RME manual:** kept local at `docs/bface_pro_fs_e.pdf` and not committed (RME's copyright).
- **Absent:** no users, testimonials, benchmarks, screenshots from other people or press yet. Do not fabricate any.

## Product Principles

1. **Familiar to TotalMix users.** Keep the same layout and workflow conventions, so people moving from Windows feel at home. The visual design is Audra's own.
2. **Never hurt the listener or the gear.** Overrides must always be recoverable on write-only hardware, and risky hardware actions (48V, gain jumps) need care and confirmation.
3. **Honest about limits.** A feature the hardware or driver can't do yet is never faked. On a strip, a control that can't work isn't drawn and the strip's Settings panel says why in a line (critique #12, the maintainer: "Hide what can't work"); a Control Room key a TotalMix user reaches for (Speaker B until a Main B is assigned; Mono before the 2026-11-09 driver; Cough until Voice runs) stands greyed with its reason (the maintainer, 2026-11-07: "Keep it greyed").
4. **Keep the system clean.** The app owns its behaviour instead of scattering config across the system, and prefers standard Linux mechanisms (ALSA state, PipeWire).
5. **Open and credited.** GPL, upstream-friendly changes, and visible credit to the projects it's built on.
