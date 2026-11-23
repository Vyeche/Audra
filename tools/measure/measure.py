#!/usr/bin/env python3
"""Play a test signal into the Babyface and record a loopback, to measure what
the mixer does: levels, left/right, echo repeats, reverb decay, EQ curves.

Works on Windows (TotalMix FX) and on Linux (Audra, PipeWire/ALSA): devices
are found by name, not by number. Needs Python 3 with numpy and sounddevice
(`pip install numpy sounddevice`; on Linux also the PortAudio library,
`sudo apt install libportaudio2`).

How to record what an output plays: switch on that output's loopback
(TotalMix: the output's settings panel; Audra: the same). The output's mix
then replaces the input with the same number in the recording, e.g. Phones
(3/4) loopback -> record "Analog (3+4)" on Windows, channels 3/4 on Linux.

Examples
  measure.py devices                         list audio devices
  measure.py beep  NAME --rec "Analog (3+4)"   150 ms 1 kHz beep, levels per 100 ms
  measure.py sweep NAME --rec "Analog (3+4)"   log sweep 20 Hz-20 kHz, saves the response
  measure.py noise NAME --rec "Analog (3+4)" --secs 4
  measure.py record NAME --rec "Analog (3+4)" --secs 30   record only (claps, voice)
  measure.py compare A B                     EQ curve: response B minus response A (dB)
  measure.py decay NAME                      reverb decay of every clap in NAME.wav

Options
  --play NAME   playback device (default: the Babyface's first output pair)
  --rec NAME    recording device (substring of its name)
  --ch A,B      channels to record (1-based; default 1,2 of that device)
  --level DB    test signal level in dBFS (default -30; never above -12)
  --secs S      length (default 3)
  --out DIR     where WAVs go (default: references/measurements next to this repo)
"""
import argparse, os, sys, wave
import numpy as np

SR = 44100
LEFT_ONLY = False
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_OUT = os.path.normpath(os.path.join(HERE, "..", "..", "references", "measurements"))


def db(a):
    return 20 * np.log10(np.maximum(np.asarray(a, float), 1e-9))


def find(sd, name, kind):
    """First device whose name contains `name` (case-insensitive) and has inputs/outputs.
    Prefers the host API the OS handles best (MME on Windows, ALSA/PipeWire on Linux)."""
    key = "max_input_channels" if kind == "in" else "max_output_channels"
    hits = [(i, d) for i, d in enumerate(sd.query_devices())
            if name.lower() in d["name"].lower() and d[key] > 0]
    if not hits:
        sys.exit(f"no {kind}put device matching {name!r}; run `measure.py devices`")
    pref = ["MME", "ALSA", "JACK", "Windows WASAPI", "Windows DirectSound"]
    hits.sort(key=lambda h: pref.index(sd.query_hostapis(h[1]["hostapi"])["name"])
              if sd.query_hostapis(h[1]["hostapi"])["name"] in pref else 99)
    return hits[0][0]


def save(path, x):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with wave.open(path, "wb") as w:
        w.setnchannels(x.shape[1]); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype("<i2").tobytes())


def load(path):
    with wave.open(path) as w:
        n = w.getnchannels()
        return np.frombuffer(w.readframes(w.getnframes()), "<i2").reshape(-1, n) / 32768


def signal(kind, secs, level):
    n = int(secs * SR); g = 10 ** (min(level, -12) / 20)
    x = np.zeros(n)
    if kind == "beep":
        t = np.arange(int(0.15 * SR)) / SR
        b = np.sin(2 * np.pi * 1000 * t)
        f = int(0.005 * SR); b[:f] *= np.linspace(0, 1, f); b[-f:] *= np.linspace(1, 0, f)
        x[int(0.2 * SR):int(0.2 * SR) + len(b)] = b
    elif kind == "sweep":
        T = secs - 0.7; t = np.arange(int(T * SR)) / SR
        f1, f2 = 20.0, 20000.0; K = T / np.log(f2 / f1)
        s = np.sin(2 * np.pi * f1 * K * (np.exp(t / K) - 1))
        f = int(0.01 * SR); s[:f] *= np.linspace(0, 1, f); s[-f:] *= np.linspace(1, 0, f)
        x[int(0.2 * SR):int(0.2 * SR) + len(s)] = s
    elif kind == "tone":                    # 2 s 1 kHz sine, starts rising at 0.2 s (for polarity)
        t = np.arange(int(2.0 * SR)) / SR
        x[int(0.2 * SR):int(0.2 * SR) + len(t)] = np.sin(2 * np.pi * 1000 * t)
    elif kind == "noise":
        rng = np.random.default_rng(1)
        s = rng.standard_normal(int((secs - 0.7) * SR)); s /= np.abs(s).max()
        x[int(0.2 * SR):int(0.2 * SR) + len(s)] = s
    right = np.zeros_like(x) if LEFT_ONLY else x
    return np.stack([x, right], 1).astype(np.float32) * g


def run(a):
    import sounddevice as sd
    if a.cmd == "devices":
        print(sd.query_devices()); return
    rec_dev = find(sd, a.rec, "in")
    ch = [int(c) for c in a.ch.split(",")]
    n = int(a.secs * SR) + int(0.4 * SR)
    chunks = []
    def cb(indata, frames, time, status):
        chunks.append(indata[:, [c - 1 for c in ch]].copy())
    with sd.InputStream(device=rec_dev, channels=max(ch), samplerate=SR, callback=cb):
        if a.cmd != "record":
            sig = signal(a.cmd, a.secs, a.level)
            play_dev = find(sd, a.play, "out")
            sd.play(sig, SR, device=play_dev); sd.wait()
        sd.sleep(int(a.secs * 1000) if a.cmd == "record" else 400)
    x = np.concatenate(chunks)[:n]
    path = os.path.join(a.out, f"{a.name}.wav"); save(path, x); print("saved", path)
    report(x)
    if a.cmd == "tone":
        polarity(x)
    if a.cmd == "sweep":
        response(x, os.path.join(a.out, f"{a.name}-response.csv"), a.secs)


def report(x):
    print("time   peak dBFS  L | R   (100 ms steps, only where above the floor)")
    env = [db(np.abs(x[i:i + 4410]).max(0)) for i in range(0, len(x), 4410)]
    floor = np.median([e.max() for e in env])
    for i, e in enumerate(env):
        if e.max() > floor + 6:
            print(f"{i/10:5.1f}s  {e[0]:6.1f} | {e[1]:6.1f}")
    print(f"floor about {floor:.1f} dBFS")


def polarity(x):
    """Each channel's tone level (loudest 100 ms) and polarity: the sign of the
    first sample above 30 % of its peak (the tone starts rising, so + = upright)."""
    for c, name in enumerate(["L", "R"]):
        s = x[:, c]
        pk = np.abs(s).max()
        lvl = max(db(np.sqrt(np.mean(s[i:i + 4410] ** 2)) * np.sqrt(2)) for i in range(0, len(s) - 4410, 4410))
        if pk < 10 ** (-80 / 20):
            print(f"  {name}: silent"); continue
        first = s[np.argmax(np.abs(s) > 0.3 * pk)]
        print(f"  {name}: {lvl:6.1f} dBFS  {'upright' if first > 0 else 'INVERTED'}")


def response(x, csv, secs):
    """Magnitude response from the recorded sweep, in 1/6-octave bands (dB re. full scale)."""
    m = x.mean(1)
    sig = signal("sweep", secs, 0)[:, 0]
    X = np.fft.rfft(m, 2 ** int(np.ceil(np.log2(len(m)))))
    S = np.fft.rfft(sig, len(X) * 2 - 2)
    H = np.abs(X) / np.maximum(np.abs(S), 1e-9)
    f = np.fft.rfftfreq(len(X) * 2 - 2, 1 / SR)
    centers = 20 * 2 ** (np.arange(0, 60) / 6); centers = centers[centers < 20000]
    rows = []
    for c in centers:
        sel = (f > c / 2 ** (1 / 12)) & (f < c * 2 ** (1 / 12))
        rows.append((c, float(db(np.sqrt(np.mean(H[sel] ** 2))))))
    with open(csv, "w") as fh:
        fh.write("hz,db\n"); fh.writelines(f"{c:.1f},{v:.2f}\n" for c, v in rows)
    print("saved", csv)


def compare(a):
    def rd(p):
        return np.loadtxt(os.path.join(a.out, f"{p}-response.csv"), delimiter=",", skiprows=1)
    A, B = rd(a.name), rd(a.name2)
    print("   Hz    change dB")
    for (f, va), (_, vb) in zip(A, B):
        if f >= 25:
            print(f"{f:7.0f}  {vb - va:+6.1f}")


def decay(a):
    x = load(os.path.join(a.out, f"{a.name}.wav")).mean(1)
    hop = int(0.01 * SR)
    env = np.array([db(np.sqrt(np.mean(x[i:i + hop] ** 2)) + 1e-12) for i in range(0, len(x) - hop, hop)])
    floor = np.median(env); i = 0
    while i < len(env):
        if env[i] > floor + 30:
            pk = i + int(np.argmax(env[i:i + 5])); top = env[pk]
            def t_to(drop):
                j = pk
                while j < len(env) and env[j] > top - drop: j += 1
                return (j - pk) / 100
            t20 = t_to(20); t30 = t_to(30) if top - 30 > floor + 3 else None
            rt = (3 * t20) if t30 is None else 2 * t30
            print(f"clap at {pk/100:6.2f}s  peak {top:6.1f} dB  -20 dB after {t20:.2f}s"
                  + (f"  -30 dB after {t30:.2f}s" if t30 else "") + f"  => RT60 about {rt:.2f}s")
            i = pk + int(t20 * 100) + 50
        else:
            i += 1
    print(f"floor about {floor:.1f} dB RMS")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("cmd", choices=["devices", "beep", "tone", "sweep", "noise", "record", "compare", "decay"])
    p.add_argument("--left", action="store_true", help="play the test signal on the left channel only")
    p.add_argument("name", nargs="?", default="test")
    p.add_argument("name2", nargs="?")
    p.add_argument("--play", default="Babyface")
    p.add_argument("--rec", default="Babyface")
    p.add_argument("--ch", default="1,2")
    p.add_argument("--level", type=float, default=-30.0)
    p.add_argument("--secs", type=float, default=3.0)
    p.add_argument("--out", default=DEFAULT_OUT)
    a = p.parse_args()
    LEFT_ONLY = a.left
    if a.cmd == "compare": compare(a)
    elif a.cmd == "decay": decay(a)
    else: run(a)
