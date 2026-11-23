# Getting started

Audra is an early preview. Turn your speakers and headphones down before the first start, and read the README's [Early preview](../README.md#early-preview) section.

## 1. Try it without the unit

The simulation needs no Babyface and no driver, and nothing you do in it reaches any hardware:

```sh
sudo apt install build-essential pkg-config libasound2-dev   # Ubuntu, Pop!_OS
curl -fsSL https://sh.rustup.rs | sh                          # Rust, if you don't have it
cargo run --release -p audra -- --mock
```

The first build takes a few minutes. `AUDRA_MOCK_DEMO=1` in front starts the simulation with a small studio already set up.

## 2. Install for real

You need the interface in **PC mode** (at power-on the meters show nothing; if they show **CC**, hold SELECT and DIM while plugging in the USB cable), and Linux 6.12 or newer with DKMS and your kernel's headers for the driver (on Ubuntu 24.04: `sudo apt install linux-generic-hwe-24.04`, then reboot).

- **From packages** (Pop!_OS, Ubuntu): `packaging/build-deb.sh`, then `sudo apt install ./target/debian/audra_*.deb ./target/debian/snd-usb-babyface-pro-dkms_*.deb`, then replug the interface or reboot.
- **By hand:** the README's [kernel driver](../README.md#1-install-the-kernel-driver) and [build](../README.md#2-build-and-run) steps.

Check the driver: `cat /proc/asound/cards` should list `BabyfacePro`.

## 3. The first ten minutes

1. **Quick setup** opens on the first start. Pick your mic, set its level with the meter's "speak here" band, and choose where you hear it (your headphones are the safe choice with a mic that has 48V on).
2. **"What are you doing?"** at the top of Quick setup has ready-made setups: a podcast or call, a headphone mix, recording, streaming, a call on your speakers. One press sets it up; one Undo takes it back; a card says what changed.
3. **The Mixer** works like TotalMix: pick an output (the mix you're editing is named at the top right), then set each input's and playback's level in that mix.
4. **After your first setup**, run `sudo alsactl store BabyfacePro` once, so a replug before the next shutdown doesn't lose your mix (the interface can't report its mixer back).

## Where next

- [The wiki](https://github.com/Vyeche/Audra/wiki): every part of the window.
- [Troubleshooting](troubleshooting.md): no device, no reverb, one silent side, and more.
- Help in the app (**?** in the top bar, or F1): guided tasks that tick off as you do them.
