# Changelog

## Unreleased

### Removed
- **Width, MS Processor Switch and the four Trim Volume controls**, and
  their replays at probe and resume. Each wrote fixed levels into
  crosspoint cells behind the crosspoint cache, so the cache (and every
  application reading it) said one thing while the unit played another,
  and a later write of the cached value was skipped:
  - the Width replay put AN1/2 into Main at -6 dB on both sides on every
    probe and resume (measured: Main's loopback carried the mic with
    AN1/2 shown off; silent once the four cells were written to 0);
  - Trim wrote the trim level into all four AN-pair Main cells (moving it
    back to 0 put both inputs in Main at 0 dB);
  - MS Processor muted AN2 and its release put AN2's Phones level into
    Main.
  Width, M/S and trim gains are the mixer application's, written through
  the crosspoint controls (own and Cross cells, Raw Crosspoint), as
  TotalMix computes them host-side.

### Changed
- **Stereo Split Switch** is a flag only: it no longer writes fixed levels
  into the pair's Main cells (0 dB left and nothing right, split).
- **Crosspoint top 0x4000** (issue #13): TotalMix's +6 dB on a stereo or
  hard-panned source (the cell's unity is 0x2000). The controls' dB scale
  is kept (0x16a0 still reads 0 dB; the top now +9.03), so applications
  written for the old 0x2d41 top behave the same.
- **Input EQ** (issue #24): the high shelf's a1 had the wrong sign (a +5 dB
  shelf at 5 kHz came out -16 dB below 2 kHz and +22 dB at 20 kHz); each
  strip now writes its own channel only (it wrote strip x 2 and + 1, so
  AN1's EQ also reached AN2, and AN2's IN3/IN4); word 13 is the product of
  every active band's b0/a0, as TotalMix sends it (was the last band's).
  `eq_selftest.c`'s reference had the same sign and is fixed with it.
- **FX send block cleared** at start and at every state restore (all 52
  cells, 0x0138-0x016b, as TotalMix writes them), before the sends that
  are set: the unit powers up with the FX return in its own send, which
  fed Reverb and Echo back into themselves.

### Added
- **Raw Crosspoint** and **DSP Block** (issue #15). They read back as a
  no-op (cell -1; an all-zero block), which a write skips, so an alsactl
  restore replays nothing. (They were write-only at first, and alsa-lib's
  simple mixer then refused to load the card: "Operation not permitted",
  for Audra and WirePlumber alike.) "Raw Crosspoint" writes one signed value to any cell
  (0..0x3FF) with request 0x12, for output phase and M/S proc, which
  TotalMix does host-side as negative crosspoint values. "DSP Block"
  uploads 64 bytes as they are on bulk OUT 0x0A, the EQ message, for the
  output EQ whose coefficients the application computes.
- **FX send on every source pair** (issue #14): "<pair> FX Send Volume"
  for AN3/4, AS1/2, ADAT3/4, ADAT5/6, ADAT7/8 and PB1-PB6, beside AN1/2's
  "FX Send Volume". Each writes `0x0138 + idx_l` and `0x0153 + idx_r`
  (the cells Windows session 3 captured), 0..0x1000 with 0x1000 = the
  send's 0 dB, cached and put back on every state restore.
- **FX Engine Switch**: arms the unit's FX pair (`0x14` with wValue
  0x0C00, wIndex 0; `0x13` to disarm), as TotalMix does when Reverb or
  Echo switches on (Windows session 3, issue #16). Without it the unit
  ignored the FX return in OUT words 12/13, so reverb and echo were never
  heard. Sent while the stream runs, and again at every stream start while
  on.
- **Phase on every strip source**: "<src> Phase Switch" for the stereo
  sources (AS1/2, ADAT3/4-7/8, PB1-PB6), two values each (ØL, ØR), beside
  AN1-4's one, as TotalMix has phase on every input and playback (#7).
- **Cross Playback Volume**, one per stereo source (AS1/2, ADAT3/4-7/8,
  PB1-PB6, the FX return) and output pair (index 0-5): the source's
  crossed cells, its left channel into the pair's R side and its right
  channel into the L side (R-base + idx_l, L-base + idx_r; PROTOCOL.md
  "MONO and PHASE are crosspoint writes"). TotalMix builds Mono, pan of
  a split pair and the crossed Matrix cells from these; Audra's Mono uses
  them. Off by default; the existing controls keep their names and
  numbers. AN1-4 need none: a single channel's one control already
  reaches both sides.
- **FX send and return channels** (Reverb/Echo, which the unit leaves to
  the host, manual 21.6): the PCM opens up to 14 channels. Capture
  channels 13/14 carry the FX send the unit mixes (frame ch12/13 of the
  IN stream), playback channels 13/14 the FX return the host sends back
  (frame ch12/13 of the OUT stream); the copy loops already mapped app
  channel n to frame word n. 12-channel opens work as before, and at
  96/192 kHz the frame has no FX words (they read as silence).
- **FX Return Playback Volume**, one per output pair (index 0-5): the FX
  return as a mixer source (idx 24/25, PROTOCOL.md "FX RETURN"),
  written by the crosspoint formula like every other source. Off at
  start, unlike the playback sources, so a reverb is never in the
  monitors until asked for. The 14 strip sources keep their controls
  and numbering (`out * 14 + src`), so saved alsactl state still
  matches. The FX return's cross registers (offsets 24/25) are cleared
  with the others, or it would come back in mono.

### Changed
- Phase writes only the channel's own cells, never the crossed ones, as
  TotalMix does (Windows session 3, issue #12): rewriting the crossed
  cells on a stereo source leaked the inverted channel into the other side.
- Phase is applied on every crosspoint write (`bf_phased()` in
  `bf_xpoint_write`, the Cross controls and the cross restore): the
  channel's cells negated, two's complement as TotalMix writes it
  (0x2000 -> 0xE000). The toggle-time negation it replaces inverted only
  AN1-4's L cell (a centred mic stayed un-inverted on the right) and was
  undone by the next fader move. The state restore needs no separate
  phase pass any more.
- The cross registers are written from the new controls' cache on every
  state restore and default mixer (`bf_crosspoint_apply_cross`, was
  `bf_crosspoint_clear_cross`), instead of zeroed, which would have
  undone a Mono at every restore. With the controls untouched they are
  still written off.

### Fixed
- A 48V switch-on could be lost until the unit was reset: the control
  skipped the write when its cached state already matched, so a stale
  cache (the unit's 48V off without the driver knowing) swallowed the
  request. It now always sends the preamp state, which is idempotent,
  and reports a change only when the value changed. Seen with AN1/2
  stereo-linked in Audra. Local fix; not in the `patches/` series yet.

## 0.2.2 (2026-10-27)

### Fixed
- The front panel's OUT wheel moved ADAT7/8 when `Opt` was selected. It
  moves AS1/2, the first optical pair, which is the same mixer output in
  ADAT and S/PDIF mode, so the target no longer depends on the Optical
  Out Format setting (#12).

### Changed
- The series under `patches/` is the one submitted upstream on
  2026-10-27, `[PATCH v7 0/8]`, rebased on next-20261026.

## 0.2.1 (2026-10-27)

### Fixed
- The crosspoint map had AN1/2 and PH3/4 swapped and shifted every pair
  after them, so a mix into one output landed in the next pair's submix.
  It is now `0x34 * pair + 0x1A * side + source`, checked on an FS with a
  per-pair loopback matrix (#36, from the captures in #31).
- The EQ's alpha is `sin(w0 / (2Q))`, as TotalMix computes it, which
  removes the high-frequency warping the notes still listed as open (#35,
  #31).

### Changed
- From a 128-frame period the driver keeps one and a half periods of URBs
  in flight, splitting the period in two. Through PipeWire the round trip
  drops from 554 to 490 frames at 128 and from 1066 to 938 at 256 (#37).
- Documentation: the latency page explains the settings and the measured
  round trip, `tools/delay-sweep.sh` measures it, and the places where the
  captures in #31 disagree with ours are marked disputed (#32, #33, #34).

## 0.2.0 (2026-10-23)

### Added
- The PCM core now sees playback underruns, so a late write is reported
  instead of silently swallowed (thanks to davfre, #25). `tools/pcmlate`
  checks it.
- Capture is exposed in the device's own channel order; `tools/` reads the
  AS1/2 loopback on capture channels 4/5.
- The low-latency modprobe template follows the period-following URBs and
  the latest soak results.
- The card defaults to 256 frames through PipeWire and follows the PipeWire
  quantum through the updated WirePlumber rule.

### Changed
- `pcmxrun` primes playback before restarting after an xrun.
- Documentation: shorter README, with install, latency and features moved to
  `docs/`; real-time low-period soak results (32/24/16 frames clean, xruns
  are system events).
- Upstream: the v7 series is cut on next-20261002.

## 0.1.1 (2026-10-18)

Initial tagged release, see the git history.
