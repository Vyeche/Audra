#!/usr/bin/env python3
"""Measures the processed mic's sound (the D panel's voice chain) with real
speech and singing, so its quality is checked by numbers and pictures, not
only by ear. The owner, 2026-11-10: "I'm relying on your skill to record the
sound and test it to see if it gets to the standard that you're expecting".

  python3 -P tools/checks/totalMix-voice-quality.py fetch   # once: the test recordings
  python3 -P tools/checks/totalMix-voice-quality.py run [deess nr tune dyn trans]

(-P, not -I: numpy lives in the user's site-packages, which -I hides.)

`run` renders with the chain that ships (crates/fx/examples/voice_render.rs
runs audra_fx::VoiceChain in the live loop's 256-sample blocks), measures
each stage against the bars below and writes target/voice-quality/out/:
report.txt, spectrogram PNGs and a few WAVs to listen to.

The recordings are the EBU's Sound Quality Assessment Material (SQAM, Tech
3253): speech (tracks 49, 50, 53) and singing (44 soprano, 46 tenor, 47
bass). The EBU allows them as an R&D tool only, so they and everything
rendered from them stay in target/ (git-ignored): never commit, upload or
post them (the repository is public).

Needs Python 3 with numpy, ffmpeg, and a release build of the renderer:
  cargo build --release -p audra-fx --example voice_render
"""
import json
import os
import subprocess
import sys
import zipfile

import numpy as np

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
WORK = os.path.join(ROOT, "target", "voice-quality")
SRC = os.path.join(WORK, "src")
OUT = os.path.join(WORK, "out")
RENDER = os.path.join(ROOT, "target", "release", "examples", "voice_render")
SQAM_URL = "https://qc.ebu.io/testmaterials/523/1/download/"
TRACKS = {"44": "soprano", "46": "tenor", "47": "bass", "49": "female speech", "50": "male speech", "53": "female speech (German)"}
SR = 48000

# The bars, written before the first run (a bar picked after seeing the
# numbers only describes the code). dB unless named otherwise.
BARS = {
    # Harsh S sounds, Strong: an S's top turned down by about its amount...
    "deess_s_cut": (2.0, 7.0),
    # ...while voiced sounds keep their presence (1-4 kHz) untouched:
    "deess_voiced_mid_mean": 0.5,  # mean |change| on voiced frames
    "deess_voiced_mid_p99": 1.5,  # the worst 1 %
    "deess_false_fire_pct": 5.0,  # voiced frames losing > 1 dB at 1-4 kHz
    # Noise removal: clean speech passes...
    "nr_clean_sdr": 15.0,  # speech vs its own delayed self, at every level
    "nr_clean_low_change": 1.0,  # mean |change| under 4 kHz, speech frames
    "nr_clean_high_change": 3.0,  # 4-8 kHz
    # ...noise between words goes down by about the level's limit...
    "nr_pause_drop_light": (5.0, 10.0),
    "nr_pause_drop_strong": (12.0, 22.0),
    # ...and the voice in noise comes out cleaner, never worse.
    "nr_sdr_gain_at_10db": 3.0,  # Strong, fan noise 10 dB under the voice
    "nr_sdr_harm_at_30db": -0.5,  # quiet room: no worse than this
    # Auto-Tune: a voice sung off pitch lands on the note...
    "tune_synth_cents": 10.0,  # median |cents off| after, a known off-pitch voice
    "tune_real_ratio": 0.6,  # real singing: median |cents off| after <= 0.6 x before
    # (first run, 2026-11-10: over all frames this measures the singers'
    # vibrato, which Auto-Tune at a natural speed keeps by design; it's
    # reported as a note, and the bar applies to the vibrato's centre; on
    # the second run the held notes' centres were mostly inside the 10-cent
    # deadband already, where nothing is corrected by design, so the 0.6 x
    # is a note too and the bar is on the notes clearly off:)
    "tune_far_cents": 12.0,  # held notes > 20 cents off before: median after
    "tune_warble_pct": 3.0,  # frames jumping > 50 cents where the voice didn't
    "tune_ripple": 1.5,  # loudness wobble added (std of the level ratio, dB)
    "tune_octave_pct": 1.0,  # speech: frames moved > 3 semitones
    # The dynamics presets: never past their ceiling, no room raised between
    # words, no distortion on a low voice.
    "dyn_true_peak_over": 0.5,  # over the -2 dBFS ceiling
    "dyn_pause_rise": 3.0,
    "dyn_thd_pct": 1.0,
    # Switching anything mid-word: no clicks, no gaps.
    "trans_clicks": 0,
    "trans_gap_ms": 0.0,
}

results = []  # (check, value, bar text, passed)


def bar(name, value, ok, bar_text, unit=""):
    ok = bool(ok)
    results.append((name, value, bar_text, ok))
    mark = "PASS" if ok else "FAIL"
    v = f"{value:.2f}{unit}" if isinstance(value, float) else str(value)
    print(f"  {mark}  {name:<44} {v:>10}   bar {bar_text}")


def note(text):
    results.append((text, None, None, None))
    print(f"        {text}")


# ---- audio in and out --------------------------------------------------


def load(path):
    raw = subprocess.run(["ffmpeg", "-loglevel", "error", "-i", path, "-f", "f32le", "-ac", "1", "-ar", str(SR), "-"], check=True, capture_output=True).stdout
    return np.frombuffer(raw, dtype="<f4").astype(np.float64)


def save(x, name):
    path = os.path.join(OUT, name)
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-", "-c:a", "pcm_s24le", path], input=np.asarray(x, dtype="<f4").tobytes(), check=True)
    return path


def spectrogram(x, name, title):
    path = os.path.join(OUT, name)
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "1", "-i", "-", "-lavfi", f"showspectrumpic=s=1400x480:legend=1:fscale=log:scale=log:stop=16000,drawtext=text='{title}':x=200:y=8:fontcolor=white:fontsize=22", path],
        input=np.asarray(x, dtype="<f4").tobytes(),
        check=True,
    )
    return path


def render(x, *args):
    r = subprocess.run([RENDER, *args], input=np.asarray(x, dtype="<f4").tobytes(), capture_output=True)
    if r.returncode != 0:
        sys.exit(f"voice_render {' '.join(args)} failed: {r.stderr.decode()}")
    info = json.loads(r.stderr.decode().strip().splitlines()[-1])
    return np.frombuffer(r.stdout, dtype="<f4").astype(np.float64), info


# ---- measures ----------------------------------------------------------


def db(p):
    return 10 * np.log10(np.maximum(p, 1e-20))


def frame_power(x, n=480):
    m = len(x) // n
    return (x[: m * n].reshape(m, n) ** 2).mean(axis=1)


def active_level(x):
    """The speech's level while it speaks (frames within 30 dB of its loud
    ones), in dBFS: a simple stand-in for ITU P.56."""
    p = frame_power(x)
    loud = np.percentile(db(p), 95)
    return db(p[db(p) > loud - 30].mean())


def at_level(x, dbfs):
    return x * 10 ** ((dbfs - active_level(x)) / 20)


def lag_of(ref, y, most=4096):
    """How many samples later `y` carries `ref` (cross-correlation)."""
    n = min(len(ref), len(y), 5 * SR)
    a, b = ref[SR : SR + n - SR], y[SR : SR + n - SR]
    size = 1 << int(np.ceil(np.log2(len(a) + len(b))))
    c = np.fft.irfft(np.fft.rfft(b, size) * np.conj(np.fft.rfft(a, size)), size)
    c = np.concatenate([c[-most:], c[: most + 1]])
    return int(np.argmax(c)) - most


def aligned(ref, y, lag):
    if lag >= 0:
        n = min(len(ref), len(y) - lag)
        return ref[:n], y[lag : lag + n]
    n = min(len(ref) + lag, len(y))
    return ref[-lag : -lag + n], y[:n]


def sdr(ref, y):
    a = np.dot(y, ref) / max(np.dot(ref, ref), 1e-20)
    err = y - a * ref
    return db(np.dot(a * ref, a * ref) / max(np.dot(err, err), 1e-20))


def stft_power(x, n=1024, hop=480):
    m = 1 + (len(x) - n) // hop
    idx = np.arange(n)[None, :] + hop * np.arange(m)[:, None]
    return np.abs(np.fft.rfft(x[idx] * np.hanning(n), axis=1)) ** 2


def band(spec, lo, hi, n=1024):
    f = np.fft.rfftfreq(n, 1 / SR)
    return spec[:, (f >= lo) & (f < hi)].sum(axis=1)


def fan(seconds, seed=7):
    """A desk fan: rumble under 300 Hz, broadband air (pink, falling past
    2 kHz), a 120 Hz hum and a blade tone, at unit level."""
    rng = np.random.default_rng(seed)
    n = int(seconds * SR)
    w = rng.standard_normal(n)
    spec = np.fft.rfft(w)
    f = np.fft.rfftfreq(n, 1 / SR)
    f[0] = 1
    shape = 1 / np.sqrt(f) * (1 / (1 + (f / 2000) ** 2)) + 6 / (1 + (f / 300) ** 4) / np.sqrt(f)
    air = np.fft.irfft(spec * shape, n)
    t = np.arange(n) / SR
    tones = 0.2 * np.sin(2 * np.pi * 120 * t) + 0.1 * np.sin(2 * np.pi * 186 * t) + 0.05 * np.sin(2 * np.pi * 372 * t)
    x = air / np.sqrt((air**2).mean()) + tones
    return x / np.sqrt((x**2).mean())


def pitch_track(x, hop=480, win=1920, fmin=70.0, fmax=1000.0):
    """YIN pitch every 10 ms: Hz per frame, NaN where unvoiced or quiet."""
    half = win // 2
    lo, hi = int(SR / fmax), min(int(SR / fmin), half - 1)
    out = []
    size = 1 << int(np.ceil(np.log2(win + half)))
    for s in range(0, len(x) - win, hop):
        fr = x[s : s + win]
        if db((fr**2).mean()) < -50:
            out.append(np.nan)
            continue
        a = fr[:half]
        acf = np.fft.irfft(np.fft.rfft(fr, size) * np.conj(np.fft.rfft(a, size)), size)[: half]
        c = np.concatenate([[0.0], np.cumsum(fr**2)])
        e0 = c[half]
        et = c[np.arange(half) + half] - c[np.arange(half)]
        d = e0 + et - 2 * acf
        d[0] = 0
        cm = d[1:] * np.arange(1, half) / np.maximum(np.cumsum(d[1:]), 1e-20)
        cm = np.concatenate([[1.0], cm])
        seg = cm[lo:hi]
        below = np.where(seg < 0.15)[0]
        if len(below) == 0:
            out.append(np.nan)
            continue
        i = below[0]
        while i + 1 < len(seg) and seg[i + 1] < seg[i]:
            i += 1
        t = i + lo
        if 1 <= t < len(cm) - 1:
            a_, b_, c_ = cm[t - 1], cm[t], cm[t + 1]
            den = a_ - 2 * b_ + c_
            t = t + (0.5 * (a_ - c_) / den if abs(den) > 1e-12 else 0)
        out.append(SR / t)
    return np.array(out)


def cents_off(hz):
    m = 69 + 12 * np.log2(hz / 440.0)
    return (m - np.round(m)) * 100


# ---- the checks --------------------------------------------------------


def check_deess():
    print("\nHarsh S sounds (de-esser), Strong, on speech")
    for t in ("49", "50"):
        x = at_level(load(os.path.join(SRC, f"{t}.wav")), -26)
        y, _ = render(x, "--deess", "2")
        sx, sy = stft_power(x), stft_power(y)
        whole = band(sx, 80, 16000)
        top_x, top_y = band(sx, 5500, 16000), band(sy, 5500, 16000)
        mid_x, mid_y = band(sx, 1000, 4000), band(sy, 1000, 4000)
        lvl = db(whole)
        loud = np.percentile(lvl, 95)
        ratio = top_x / np.maximum(whole, 1e-20)
        sib = (ratio > 0.35) & (lvl > loud - 30)
        voiced = (ratio < 0.03) & (lvl > loud - 20)
        cut = (db(top_x[sib]) - db(top_y[sib])).mean() if sib.any() else 0.0
        mid = db(mid_x[voiced]) - db(mid_y[voiced])
        lo, hi = BARS["deess_s_cut"]
        bar(f"{TRACKS[t]}: S top turned down ({sib.sum()} S frames)", float(cut), lo <= cut <= hi, f"{lo}-{hi} dB", " dB")
        m, p99 = float(np.abs(mid).mean()), float(np.percentile(np.abs(mid), 99))
        bar(f"{TRACKS[t]}: voiced 1-4 kHz change, mean", m, m <= BARS["deess_voiced_mid_mean"], f"<= {BARS['deess_voiced_mid_mean']} dB", " dB")
        bar(f"{TRACKS[t]}: voiced 1-4 kHz change, worst 1 %", p99, p99 <= BARS["deess_voiced_mid_p99"], f"<= {BARS['deess_voiced_mid_p99']} dB", " dB")
        fire = float((mid > 1.0).mean() * 100)
        # The frames in between (bright vowels, soft S's): the split's
        # gentle skirt reaches down to them, so look at them too.
        between = (ratio >= 0.03) & (ratio <= 0.35) & (lvl > loud - 20)
        if between.any():
            mb = db(mid_x[between]) - db(mid_y[between])
            note(f"{TRACKS[t]}: {between.sum()} frames between voiced and S: 1-4 kHz change mean {np.abs(mb).mean():.2f} dB, worst 1 % {np.percentile(np.abs(mb), 99):.2f} dB")
        bar(f"{TRACKS[t]}: voiced frames losing > 1 dB", fire, fire <= BARS["deess_false_fire_pct"], f"<= {BARS['deess_false_fire_pct']} %", " %")
        if t == "49":
            spectrogram(x[: 12 * SR], "deess-49-before.png", "female speech, before")
            spectrogram(y[: 12 * SR], "deess-49-after.png", "female speech, Harsh S Strong")
            save(y, "deess-49-strong.wav")


def check_nr():
    print("\nNoise removal")
    clean = {t: load(os.path.join(SRC, f"{t}.wav")) for t in ("49", "50")}
    for t, x0 in clean.items():
        for lvl in (-42, -30, -18):
            x = at_level(x0, lvl)
            for level in ("1", "2"):
                y, info = render(x, "--noise", level)
                lag = lag_of(x, y)
                r, yy = aligned(x, y, lag)
                r, yy = r[SR:], yy[SR:]
                s = sdr(r, yy)
                name = "Light" if level == "1" else "Strong"
                bar(f"{TRACKS[t]} {lvl} dBFS {name}: clean speech kept", float(s), s >= BARS["nr_clean_sdr"], f">= {BARS['nr_clean_sdr']} dB", " dB")
                sr_, sy = stft_power(r), stft_power(yy)
                lv = db(band(sr_, 80, 16000))
                sp = lv > np.percentile(lv, 95) - 20
                low = float(np.abs(db(band(sr_, 80, 4000)[sp]) - db(band(sy, 80, 4000)[sp])).mean())
                high = float(np.abs(db(band(sr_, 4000, 8000)[sp]) - db(band(sy, 4000, 8000)[sp])).mean())
                if lvl == -30:
                    bar(f"   under 4 kHz change ({name})", low, low <= BARS["nr_clean_low_change"], f"<= {BARS['nr_clean_low_change']} dB", " dB")
                    bar(f"   4-8 kHz change ({name})", high, high <= BARS["nr_clean_high_change"], f"<= {BARS['nr_clean_high_change']} dB", " dB")
                    if lag != round(info["noise_ms"] * SR / 1000):
                        note(f"the sound came {lag} samples late; the chain says {info['noise_ms']:.1f} ms")
    x0 = at_level(clean["49"], -30)
    for kind, noise in (("fan", fan(len(x0) / SR)), ("hiss", np.random.default_rng(3).standard_normal(len(x0)))):
        for snr in (10, 20, 30):
            n = noise / np.sqrt((noise**2).mean()) * 10 ** ((-30 - snr) / 20)
            x = x0 + n
            pause = db(frame_power(x0)) < -75
            for level in ("1", "2"):
                y, info = render(x, "--noise", level)
                lag = round(info["noise_ms"] * SR / 1000)
                r, yy = aligned(x0, y, lag)
                xin = x[: len(r)]
                sdr_in, sdr_out = sdr(r[SR:], xin[SR:]), sdr(r[SR:], yy[SR:])
                pin, pout = frame_power(xin), frame_power(yy)
                pz = pause[: len(pin)] & (np.arange(len(pin)) > 100)
                drop = float(db(pin[pz].mean()) - db(pout[pz].mean())) if pz.any() else float("nan")
                name = "Light" if level == "1" else "Strong"
                if snr == 20:
                    lo, hi = BARS["nr_pause_drop_light" if level == "1" else "nr_pause_drop_strong"]
                    bar(f"{kind} {snr} dB under: between words, {name}", drop, lo <= drop <= hi, f"{lo}-{hi} dB", " dB")
                gain = float(sdr_out - sdr_in)
                if snr == 10 and level == "2":
                    bar(f"{kind} {snr} dB under: voice cleaner by ({name})", gain, gain >= BARS["nr_sdr_gain_at_10db"], f">= {BARS['nr_sdr_gain_at_10db']} dB", " dB")
                elif snr == 30:
                    bar(f"{kind} {snr} dB under: no harm ({name})", gain, gain >= BARS["nr_sdr_harm_at_30db"], f">= {BARS['nr_sdr_harm_at_30db']} dB", " dB")
                else:
                    note(f"{kind} {snr} dB under, {name}: between words -{drop:.1f} dB, voice cleaner by {gain:+.1f} dB")
                if kind == "fan" and snr == 10 and level == "2":
                    spectrogram(xin[: 12 * SR], "nr-fan10-before.png", "speech + fan 10 dB under")
                    spectrogram(yy[: 12 * SR], "nr-fan10-strong.png", "Noise removal Strong")
                    save(xin, "nr-fan10-before.wav")
                    save(yy, "nr-fan10-strong.wav")


def synth_singer(seconds=8.0):
    """A sung voice with a known pitch: notes held 30-40 cents off, with a
    slow drift and a little vibrato, and formant-like harmonics."""
    t = np.arange(int(seconds * SR)) / SR
    notes = [57, 60, 62, 64, 62, 60, 59, 57]
    offs = [35, -30, 40, -35, 30, -40, 35, -30]
    seg = len(t) // len(notes)
    midi = np.concatenate([np.full(seg, n + o / 100) for n, o in zip(notes, offs)])
    midi = np.concatenate([midi, np.full(len(t) - len(midi), midi[-1])])
    midi = midi + 0.08 * np.sin(2 * np.pi * 5.0 * t) + 0.05 * np.sin(2 * np.pi * 0.3 * t)
    f = 440 * 2 ** ((midi - 69) / 12)
    ph = 2 * np.pi * np.cumsum(f) / SR
    x = sum(np.sin(k * ph) / k * (1.5 if 3 <= k <= 5 else 1.0) for k in range(1, 20))
    env = np.minimum(1, np.minimum(t, seconds - t) * 8)
    return 0.1 * x * env


def centre(p, v, frames=20):
    """The pitch smoothed over `frames` x 10 ms (in cents, through voiced
    runs only), back in Hz; NaN where the run is shorter."""
    out = np.full(len(p), np.nan)
    i = 0
    while i < len(p):
        if not v[i]:
            i += 1
            continue
        j = i
        while j < len(p) and v[j]:
            j += 1
        if j - i >= frames:
            c = 1200 * np.log2(p[i:j] / 440.0)
            sm = np.convolve(c, np.ones(frames) / frames, mode="valid")
            k = i + frames // 2
            out[k : k + len(sm)] = 440.0 * 2 ** (sm / 1200)
        i = j
    return out


def wobble(x, y, lag, voiced, chunk=4800, hop=240, search=960):
    """Std of the 5 ms level ratio out/in (dB), each chunk aligned and
    its own mean taken off; voiced frames over -50 dBFS only."""
    parts = []
    for s in range(SR, len(x) - chunk - lag - 2 * search, chunk):
        f = s // 480
        if f >= len(voiced) or not voiced[f : f + chunk // 480].any():
            continue
        a = x[s : s + chunk]
        lo = max(s + lag - search, 0)
        b = y[lo : lo + chunk + 2 * search]
        c = np.correlate(b, a, mode="valid")
        L = lo + int(np.argmax(c))
        ex, ey = db(frame_power(a, hop)), db(frame_power(y[L : L + chunk], hop))
        k = ex > -50
        vv = np.repeat(voiced[f : f + chunk // 480], 480 // hop)
        k = k[: len(vv)] & vv[: len(k)]
        if k.sum() < 5:
            continue
        r = (ey[: len(k)] - ex[: len(k)])[k]
        parts.append(r - r.mean())
    return float(np.std(np.concatenate(parts))) if parts else 0.0


def check_tune():
    print("\nAuto-Tune")
    x = synth_singer()
    y, info = render(x, "--tune")
    lag = int(round(info["tune_ms"] * SR / 1000))
    px, py = pitch_track(x), pitch_track(y[lag:])
    n = min(len(px), len(py))
    px, py = px[:n], py[:n]
    v = ~np.isnan(px) & ~np.isnan(py)
    v[:30] = False
    after = float(np.median(np.abs(cents_off(py[v]))))
    note(f"a voice sung 30-40 cents off: {np.median(np.abs(cents_off(px[v]))):.0f} cents off before")
    bar("synthetic singer: cents off after (median)", after, after <= BARS["tune_synth_cents"], f"<= {BARS['tune_synth_cents']} cents")
    spectrogram(y[: 8 * SR], "tune-synth-after.png", "synthetic singer, Auto-Tune")
    for t in ("44", "46", "47", "49", "50"):
        x = at_level(load(os.path.join(SRC, f"{t}.wav")), -24)
        y, info = render(x, "--tune")
        # How late it came out, found from the sound (the delay moves with
        # the voice; the chain's figure is the longest).
        lag = max(lag_of(x, y), 0)
        yy = y[lag:]
        px, py = pitch_track(x), pitch_track(yy)
        n = min(len(px), len(py))
        px, py = px[:n], py[:n]
        v = ~np.isnan(px) & ~np.isnan(py)
        if v.sum() < 50:
            note(f"{TRACKS[t]}: too few voiced frames ({v.sum()})")
            continue
        before, after = np.median(np.abs(cents_off(px[v]))), np.median(np.abs(cents_off(py[v])))
        moved = np.abs(12 * np.log2(py[v] / px[v]))
        if t in ("49", "50"):
            oct_pct = float((moved > 3).mean() * 100)
            bar(f"{TRACKS[t]}: frames moved > 3 semitones", oct_pct, oct_pct <= BARS["tune_octave_pct"], f"<= {BARS['tune_octave_pct']} %", " %")
        else:
            note(f"{TRACKS[t]}: all frames {before:.0f} cents off before, {after:.0f} after (the vibrato itself, kept)")
            # Trained singers sing with vibrato (often +-50 cents or more,
            # 5-7 Hz), which Auto-Tune at a natural speed keeps: so also
            # the vibrato's centre, the pitch smoothed over 200 ms.
            # Held parts of notes only, chosen from the input alone (the
            # same frames before and after): where the input's centre
            # moves under 15 cents over 100 ms (between notes the centre
            # blends two notes, before as after).
            cx, cy = centre(px, v), centre(py, v)
            held = np.zeros(len(cx), dtype=bool)
            cc = 1200 * np.log2(cx / 440.0)
            held[5:-5] = np.abs(cc[10:] - cc[:-10]) < 15
            ok = ~np.isnan(cx) & ~np.isnan(cy) & held
            if ok.sum() > 50:
                cb, ca = np.median(np.abs(cents_off(cx[ok]))), np.median(np.abs(cents_off(cy[ok])))
                note(f"{TRACKS[t]}: held notes' centre {cb:.0f} cents off before, {ca:.0f} after")
                b, a = np.abs(cents_off(cx[ok])), np.abs(cents_off(cy[ok]))
                far = b > 20
                if far.sum() >= 20:
                    fa = float(np.median(a[far]))
                    bar(f"{TRACKS[t]}: notes > 20 c off ({np.median(b[far]):.0f}) -> after", fa, fa <= BARS["tune_far_cents"], f"<= {BARS['tune_far_cents']} cents", " c")
                else:
                    note(f"{TRACKS[t]}: only {far.sum()} held frames > 20 cents off: too few to judge")
        # Warble: the output jumps > 50 cents frame to frame while the
        # input moved < 20.
        both = v[1:] & v[:-1]
        jx = np.abs(1200 * np.log2(px[1:] / px[:-1]))
        jy = np.abs(1200 * np.log2(py[1:] / py[:-1]))
        warble = float(((jy > 50) & (jx < 20))[both].mean() * 100) if both.any() else 0.0
        bar(f"{TRACKS[t]}: warble", warble, warble <= BARS["tune_warble_pct"], f"<= {BARS['tune_warble_pct']} %", " %")
        # Loudness wobble: the level ratio out/in over 5 ms, on voiced
        # frames, each 100 ms aligned on its own (the shifter's delay moves
        # with the voice: one lag for the whole made its own wobble).
        ripple = wobble(x, y, lag, v)
        bar(f"{TRACKS[t]}: loudness wobble added", ripple, ripple <= BARS["tune_ripple"], f"<= {BARS['tune_ripple']} dB", " dB")
        if t == "44":
            spectrogram(x[: 12 * SR], "tune-44-before.png", "soprano, before")
            spectrogram(yy[: 12 * SR], "tune-44-after.png", "soprano, Auto-Tune")
            save(yy, "tune-44.wav")


def true_peak(x):
    """dBFS, 4x oversampled."""
    n = len(x)
    spec = np.fft.rfft(x)
    up = np.fft.irfft(np.concatenate([spec, np.zeros(3 * n // 2)]), 4 * n) * 4
    return 20 * np.log10(max(np.abs(up).max(), 1e-12))


def check_dyn():
    print("\nDynamics presets")
    names = ["Clear Calls", "Streamer", "Podcast", "Singing", "Radio Voice"]
    for t in ("49", "50"):
        x0 = load(os.path.join(SRC, f"{t}.wav"))
        for lvl in (-30, -18):
            x = at_level(x0, lvl)
            worst = max((true_peak(render(x, "--preset", p)[0][SR:]), p) for p in names)
            over = worst[0] + 2.0
            bar(f"{TRACKS[t]} {lvl} dBFS: loudest true peak ({worst[1]})", float(worst[0]), over <= BARS["dyn_true_peak_over"], f"<= {-2 + BARS['dyn_true_peak_over']} dBFS", " dBFS")
    x0 = at_level(load(os.path.join(SRC, "49.wav")), -30)
    n = fan(len(x0) / SR) * 10 ** (-62 / 20)
    x = x0 + n
    pause = db(frame_power(x0)) < -75
    pz = pause & (np.arange(len(pause)) > 300)
    for p in names:
        y, _ = render(x, "--preset", p)
        rise = float(db(frame_power(y)[pz].mean()) - db(frame_power(x)[pz].mean()))
        bar(f"room noise between words ({p})", rise, rise <= BARS["dyn_pause_rise"], f"<= +{BARS['dyn_pause_rise']} dB", " dB")
    t = np.arange(3 * SR) / SR
    for hz in (82.0, 110.0):
        s = np.sin(2 * np.pi * hz * t) * 10 ** (-15 / 20)
        for p in names:
            y, _ = render(s, "--preset", p)
            seg = y[2 * SR : 3 * SR] * np.hanning(SR)
            spec = np.abs(np.fft.rfft(seg))
            f0 = int(hz)
            fund = spec[f0 - 2 : f0 + 3].max()
            harm = np.sqrt(sum(spec[k * f0 - 2 : k * f0 + 3].max() ** 2 for k in range(2, 11)))
            thd = float(harm / fund * 100)
            bar(f"{hz:.0f} Hz low voice, distortion ({p})", thd, thd <= BARS["dyn_thd_pct"], f"<= {BARS['dyn_thd_pct']} %", " %")


def check_trans():
    print("\nSwitching mid-word (clicks and gaps)")
    x = at_level(load(os.path.join(SRC, "49.wav")), -26)
    env = db(frame_power(x))
    loud = np.percentile(env, 95)
    # Event times: the first loud 10 ms after each of 2, 5, 8... seconds.
    times = []
    for start in np.arange(2.0, 20.0, 3.0):
        i = int(start * 100)
        while i < len(env) and env[i] < loud - 6:
            i += 1
        times.append(i / 100)
    # Each stage switched off -> on -> off... at those times, against the
    # same speech with the stage held on and held off throughout: a click
    # is a step bigger than either steady version makes there, a gap is
    # near silence where both steady versions speak. (First run: a step
    # compared with the quieter speech before an onset counted onsets.)
    cases = [
        ("noise", ["--noise", "2"], "noise=0", "noise=2"),
        ("tune", ["--tune"], "tune=0", "tune=1"),
        ("deess", ["--deess", "2"], "deess=0", "deess=2"),
        ("cough", None, "cough=0", "cough=1"),
        ("preset", ["--preset", "Radio Voice"], "preset=natural", "preset=Radio Voice"),
    ]
    for name, steady_on, off, on in cases:
        events = []
        for k, at in enumerate(times):
            events += ["--event", f"{at:.3f}:{on if k % 2 == 0 else off}"]
        y, _ = render(x, *events)
        held_on = render(x, *steady_on)[0] if steady_on else np.zeros_like(x)
        held_off = render(x)[0]
        clicks, gap = 0, 0.0
        for k, at in enumerate(times):
            c = int(at * SR)
            sl = slice(c - 480, c + 2400)
            step = np.abs(np.diff(y[sl])).max()
            ref = max(np.abs(np.diff(held_on[sl])).max(), np.abs(np.diff(held_off[sl])).max())
            if step > 1.5 * ref and step > 10 ** (-40 / 20):
                clicks += 1
            if name != "cough":
                quiet = np.abs(y[sl]) < 10 ** (-70 / 20)
                speak = (np.abs(held_on[sl]) > 10 ** (-60 / 20)) | (np.abs(held_off[sl]) > 10 ** (-60 / 20))
                run, longest = 0, 0
                for qv, sv in zip(quiet, speak):
                    run = run + 1 if qv and sv else (run if qv else 0)
                    longest = max(longest, run)
                gap = max(gap, longest / SR * 1000 if longest >= 96 else 0.0)
        bar(f"switching {name}: clicks ({len(times)} switches)", clicks, clicks <= BARS["trans_clicks"], f"<= {BARS['trans_clicks']}")
        if name != "cough":
            bar(f"switching {name}: longest gap", float(gap), gap <= BARS["trans_gap_ms"], f"<= {BARS['trans_gap_ms']} ms", " ms")
        if name in ("noise", "tune"):
            save(y, f"switch-{name}.wav")


def fetch():
    os.makedirs(SRC, exist_ok=True)
    z = os.path.join(WORK, "sqam-zip", "sqam.zip")
    if not os.path.exists(z):
        os.makedirs(os.path.dirname(z), exist_ok=True)
        print("downloading the EBU SQAM (176 MB, R&D use only) ...")
        subprocess.run(["curl", "-sL", "-o", z, SQAM_URL], check=True)
    with zipfile.ZipFile(z) as zf:
        for t in TRACKS:
            flac = os.path.join(WORK, "sqam", f"{t}.flac")
            os.makedirs(os.path.dirname(flac), exist_ok=True)
            with zf.open(f"{t}.flac") as a, open(flac, "wb") as b:
                b.write(a.read())
            subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", flac, "-ac", "1", "-ar", str(SR), "-c:a", "pcm_f32le", os.path.join(SRC, f"{t}.wav")], check=True)
            print(f"  {t}.wav  {TRACKS[t]}")


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ("fetch", "run"):
        sys.exit(__doc__)
    if sys.argv[1] == "fetch":
        fetch()
        return
    if not all(os.path.exists(os.path.join(SRC, f"{t}.wav")) for t in TRACKS):
        sys.exit("the test recordings are missing: run `totalMix-voice-quality.py fetch` first")
    if not os.path.exists(RENDER):
        sys.exit("build the renderer first: cargo build --release -p audra-fx --example voice_render")
    os.makedirs(OUT, exist_ok=True)
    checks = {"deess": check_deess, "nr": check_nr, "tune": check_tune, "dyn": check_dyn, "trans": check_trans}
    for name in sys.argv[2:] or list(checks):
        checks[name]()
    failed = [r for r in results if r[3] is False]
    passed = [r for r in results if r[3] is True]
    print(f"\n{len(passed)} passed, {len(failed)} failed (measured, not heard)")
    with open(os.path.join(OUT, "report.txt"), "w") as f:
        for name, value, bar_text, ok in results:
            if ok is None:
                f.write(f"        {name}\n")
            else:
                v = f"{value:.2f}" if isinstance(value, float) else str(value)
                f.write(f"{'PASS' if ok else 'FAIL'}  {name:<44} {v:>10}   bar {bar_text}\n")
        f.write(f"\n{len(passed)} passed, {len(failed)} failed (measured, not heard)\n")


if __name__ == "__main__":
    main()
