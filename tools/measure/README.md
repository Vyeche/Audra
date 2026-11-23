# Measuring the Babyface

`measure.py` plays a test signal into the Babyface and records an output's **loopback**, so what the mixer does can be measured instead of only heard: levels, left/right, echo repeats, reverb decay, EQ curves. It finds devices **by name**, so the same commands run on Windows (TotalMix FX) and on Linux (Audra).

The results from the Windows session are in `docs/totalmix-windows-findings.md` ("Measured with loopback" and "Measured round 2"). The recordings and CSVs are in `references/measurements/`.

## Set it up

It needs Python 3 with `numpy` and `sounddevice`. The packages go in their own virtual environment (venv), so the system Python stays untouched. The venv is about 70 MB.

### Windows (the owner's PC)

Python 3.12 is installed (`py -0p` lists it). A Claude Code session builds the venv in its scratchpad folder. That folder is temporary, so rebuild it at the start of a session that needs it:

```powershell
$v = "<scratchpad>\venv"          # the session's scratchpad directory
py -3.12 -m venv $v
& "$v\Scripts\python.exe" -m pip install -q numpy sounddevice
& "$v\Scripts\python.exe" -c "import sounddevice, numpy; print('ok')"
```

For a venv that survives sessions, use `C:\Users\Vye\.venvs\audra-measure` instead of the scratchpad.

Run scripts with `-I` (isolated mode), for example `& "$v\Scripts\python.exe" -I tools\measure\measure.py devices`.

### Linux (Pop!_OS)

```bash
sudo apt install libportaudio2 python3-venv
python3 -m venv ~/.venvs/audra-measure
~/.venvs/audra-measure/bin/pip install numpy sounddevice
~/.venvs/audra-measure/bin/python tools/measure/measure.py devices
```

## Use it

1. **Turn on the loopback of the output you want to measure.** In TotalMix FX it's the output's settings panel (gear) → **loopback**; in Audra, the same key in the output's Settings. The output's mix then replaces the input with the same number in the recording:
   - Phones (3/4) → record **"Analog (3+4)"** on Windows, channels 3/4 on Linux.
   - Main (1/2) → record **"Analog (1+2)"**. This replaces the mic in the recording while it's on.
2. **Play and record.** Windows examples (playback 1/2 is called "Speakers (RME Babyface Pro)" there):

   ```
   measure.py beep  t1 --play "Speakers (RME" --rec "Analog (3+4)"            levels per 100 ms
   measure.py sweep eq-off --play "Speakers (RME" --rec "Analog (3+4)" --level -35 --secs 6
   measure.py sweep eq-on  --play "Speakers (RME" --rec "Analog (3+4)" --level -35 --secs 6
   measure.py compare eq-off eq-on                                             EQ curve in dB
   measure.py record claps --rec "Analog (3+4)" --secs 45                      claps or voice
   measure.py decay claps                                                      reverb RT60 per clap
   ```

   On Linux, run `measure.py devices` first and use the names it lists for `--play` and `--rec`. With Audra's pro-audio profile, choose the channels with `--ch 3,4`.
3. **Set the loopback off again** afterwards.

### Keep it safe for the person listening

- The signal level is capped at −12 dBFS, and the default is −30. The owner may be wearing headphones.
- Never raise the mic gain or the Phones fader for a test.
- To test the FX, turn the send and the return up, then set both back to −∞.
- For the decay test, the person claps once, firmly, then stays quiet for about 8 s, three times. The script needs no clicks, only claps.

### Known quirks (Windows)

- Use the **MME** devices, which the script prefers. The WASAPI devices refused 2-channel streams ("Invalid number of channels").
- One long noise run hung Windows playback and recorded nothing. Short beeps and sweeps were reliable.
- The first FX test from a playback channel showed no echo, and a retest did. If the effect seems missing, retest once before drawing conclusions.

## Driving TotalMix FX from a session (Windows)

The screenshots in `references/tm-win-*.png` were taken and TotalMix was clicked through with `tools/windows/screen.ps1`. It has small Win32 helpers for clicks, the mouse wheel, keys and screenshots, in screen coordinates. Load it with `. tools\windows\screen.ps1`, then call `Click x y`, `Wheel x y notches`, `Key 0x1B`, and `SnapS path x y w h scale`. Save screenshots into `references/`, not the temp folder.

Lessons from the session:
- Clicking anything on an output strip (gear, EQ, name) also selects that output's mix. Re-select Phones 1 afterwards.
- Press Escape only when a menu is actually open. A stray Escape cancels the focused dialog, including the Fireface USB Settings window.
- Snapshot and layout **store**: press store, then the slot's small **lamp key** on the left. Clicking the slot's name doesn't store.
- Recall on the control room is a single press: it sets Main to the recall level (−10 dB), and pressing it again doesn't undo it.
- Mouse wheel steps: FX send/return about 1 dB a notch (the first notch from −∞ jumps to about −64.5 dB); pan about 2 a notch; Main fader 0.4–0.5 dB a notch near 0 dB; EQ gain about 1 dB a notch, EQ frequency 2–3 % a notch.

## More Windows tools (session 3, 2026-11-08)

- `tools/windows/usbcap.ps1` finds the Babyface's USBPcap hub itself (it moved from USBPcap4 to USBPcap1 after a reboot) and writes `FAILED` to `references/usbcap/status.txt` when nothing is recorded. Use `-Session` for several captures under one admin prompt: write `NAME SNAP` to `next.txt` to start (SNAP 0 = full packets for audio), `stop.flag` to stop, `quit.flag` to end. The capture file is locked while recording, so check it after stopping.
- `tools/windows/pcap_check.py CAP actions.log`: for each logged step, what TotalMix sent (control writes, bulk 0x0A messages), and which steps sent nothing. Run it before leaving Windows.
- `tools/windows/pcap_audio_words.py CAP actions.log`: the level of each of the 14 audio words per step, OUT and IN (needs a full-packet capture).
- `measure.py tone NAME --left`: a 2 s tone on the left only, with each side's level and polarity (upright or inverted).
