# Troubleshooting

The wiki's [Troubleshooting](https://github.com/Vyeche/Audra/wiki/Troubleshooting) page has the full answers. The short version:

| What you see | Check |
|---|---|
| "No device" when Audra starts | The interface is on and in PC mode (no "CC" at power-on); `cat /proc/asound/cards` lists `BabyfacePro` (else `sudo modprobe snd-usb-babyface-pro`); after a kernel update the headers are installed; no other mixer (TuxMix) holds the unit. Audra picks the unit up when it appears. |
| The mix is different after a reboot or replug | Run `sudo alsactl store BabyfacePro` once after your first setup. The unit can't report its mixer back, and the driver resets it on load; ALSA keeps it. |
| One side of the headphones is silent | One mic on AN1 with AN1/2 linked plays on the left only: switch on **AN1 on both sides (AN1 → 2)** in AN1's Settings panel. |
| Reverb is on and nothing is heard | Read the line under the Reverb & Echo page's bar and work down it: the newer driver, the Pro Audio profile, PipeWire's `pw-record` and `pw-play`, the send and return (**Make it heard**), Mute FX. Effects work up to 48 kHz. |
| "Open mic in your speakers" | A mic with 48V on (or 20 dB of gain or more) is sent to your speakers and can howl. **Take it out** removes it from that mix only; listen on headphones instead. |
| Other apps see only two channels | Switch the card's profile to **Pro Audio** in your sound settings (the Reverb & Echo page has a key for it). |
| Glitter on the controls (AMD RX 9000) | Audra turns off the colour compression that causes it; `AUDRA_KEEP_DCC=1` turns that off. |
| "Some settings couldn't be read" | Audra kept what it could from `~/.config/audra/gui.json` and saved the old file as `gui.json.bad`. |

## Known limits

- **The driver needs Linux 6.12 or newer.** On Ubuntu 24.04's original 6.8 kernel it doesn't compile; its hardware-enablement kernel (`linux-generic-hwe-24.04`) is new enough, as is Pop!_OS 24.04's.
- **Not yet tried on a real Babyface** beyond the driver and the basic mixer (see the README's [Early preview](../README.md#early-preview)).
- **The mixer is write-only:** the unit has no readback, so Audra and ALSA keep the state.
- **The unit's MIDI port** isn't available yet: the driver leaves it to `snd-usb-audio`, which doesn't bind it in PC mode.
- **Reverb and Echo** run on the computer (the Babyface has no effects processor) and need the driver from 2026-11-05 on. Only AN1/2 has an FX send. They work up to 48 kHz.
- **Voice and Dynamics** run on the computer too: others hear you about 40 ms late (60 with noise removal), fine for calls and streams, too late for your own ears. You always hear yourself straight, with no delay.
- **The SET button** on the unit switches 48V; it can't be reassigned.

## Still stuck?

Open an issue with the bug form: it asks for what's needed to reproduce it. If it sounded unsafe (feedback, a loud burst), tick that box.
