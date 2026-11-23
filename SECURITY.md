# Security Policy

Audra is a local mixer app with a kernel driver. It makes no internet connections and sends nothing anywhere. What it does have:

- **A kernel driver** (`driver/`, `snd-usb-babyface-pro`), which runs with full rights inside Linux. A bug there can crash the machine. Reports about it are the most important kind.
- **Optional local controls**, all off unless you switch them on: the automation socket (`--control`, a Unix socket only your own user can reach), the OSC control surface (it listens on this computer only, 127.0.0.1), and MIDI.
- **Files it reads**: its settings, snapshots, workspaces, presets and colour themes (JSON). A crafted file that crashes or freezes Audra is worth reporting.
- **Sound safety**: anything that can make Audra play a sudden loud sound, or send an open microphone to the speakers without saying so, is treated as a security issue too.

## Supported versions

Only the latest preview release gets fixes. Please reproduce a problem on the latest release first.

## Reporting a vulnerability

- **GitHub:** use [private vulnerability reporting](../../security/advisories/new) on this repository (preferred: it stays private until fixed), or open a regular issue if you're comfortable with it being public.
- **What to include:** the Audra version (top left of the window), the driver version (`dkms status`), your distribution and kernel, steps to reproduce, and any log (`RUST_LOG=debug audra`). Remove your user name, paths and hostnames from logs.
- **What to expect:** a reply within a week, a fix in the next release for anything confirmed, and credit in the release notes unless you'd rather not.

Out of scope: RME's own software and firmware, PipeWire and ALSA themselves (report those to their projects), and anything that needs an attacker to already run code as your user.
