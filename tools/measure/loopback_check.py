#!/usr/bin/env python3
"""Quick loopback checks on Linux with PipeWire's pw-play/pw-record, no
packages needed (measure.py needs PortAudio and numpy; this needs neither).

  loopback_check.py tone FILE.wav [CH]   a 14-channel WAV: 1 kHz, -30 dBFS, 2 s,
                                          on output channel CH only (0 = PB1 L)
  loopback_check.py levels FILE.wav      each channel's loudest 100 ms in dBFS,
                                          and its polarity: the sign of the first
                                          sample above 30 % of its peak (the tone
                                          starts rising, so + is upright, - is
                                          inverted)

Record with the Babyface in the pro-audio profile, for example:

  pw-record --target alsa_input.usb-RME_Babyface_Pro_...pro-input-0 \\
            --channels 14 --format s32 --rate 48000 rec.wav &
  pw-play --target alsa_output.usb-RME_Babyface_Pro_...pro-output-0 tone.wav

With an output's loopback on, its mix comes back on the inputs with the same
numbers (Phones 3/4 -> ch3/ch4). Keep the level low: someone may be wearing
the headphones.
"""
import array
import math
import sys
import wave

RATE = 48000
CH = 14


def tone(path: str, channel: int = 0) -> None:
    secs, amp = 2.0, 10 ** (-30 / 20)
    n = int(RATE * secs)
    data = array.array("i", bytes(4 * n * CH))
    for i in range(n):
        data[i * CH + channel] = int(amp * math.sin(2 * math.pi * 1000 * i / RATE) * 2147483647)
    with wave.open(path, "wb") as w:
        w.setnchannels(CH)
        w.setsampwidth(4)
        w.setframerate(RATE)
        w.writeframes(data.tobytes())


def levels(path: str) -> None:
    with wave.open(path, "rb") as w:
        ch, width = w.getnchannels(), w.getsampwidth()
        raw = w.readframes(w.getnframes())
    if width != 4:
        sys.exit(f"expected 32-bit samples, got {8 * width}-bit")
    data = array.array("i")
    data.frombytes(raw)
    n = len(data) // ch
    win = RATE // 10
    for c in range(ch):
        col = [data[k * ch + c] / 2147483648.0 for k in range(n)]
        best = 0.0
        for start in range(0, n - win, win):
            s = sum(x * x for x in col[start:start + win:4])
            best = max(best, s / (win / 4))
        db = 10 * math.log10(best) if best > 0 else -200.0
        peak = max((abs(x) for x in col), default=0.0)
        sign = ""
        if db > -80 and peak > 0:
            first = next((x for x in col if abs(x) > 0.3 * peak), 0.0)
            sign = "  upright" if first > 0 else "  INVERTED"
        print(f"ch{c + 1:2d}: {db:7.1f} dBFS{sign}")


if __name__ == "__main__":
    if len(sys.argv) < 3 or sys.argv[1] not in ("tone", "levels"):
        sys.exit(__doc__)
    if sys.argv[1] == "tone":
        tone(sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 0)
    else:
        levels(sys.argv[2])
