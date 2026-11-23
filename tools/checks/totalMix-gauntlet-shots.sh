#!/usr/bin/env bash
# The gauntlet's screenshots: about twenty screens of the simulation drawn
# off screen (--screenshot: no window opens, no sound plays, nothing touches
# PipeWire or a Babyface), for a reviewer to see what a change did.
#
#   bash tools/checks/totalMix-gauntlet-shots.sh [BIN] [OUT]
#
# BIN: target/release/audra; OUT: target/gauntlet/shots. Each run starts
# from an empty settings folder under OUT, never your own.
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=${1:-target/release/audra}
OUT=${2:-target/gauntlet/shots}
[ -x "$BIN" ] || { echo "shots: no $BIN (cargo build --release -p audra)" >&2; exit 2; }
mkdir -p "$OUT"
fails=0
# --screenshot opens no window but needs a display connection: on a machine
# without a desktop (a CI runner, a container) a virtual one.
RUN=()
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
    command -v xvfb-run >/dev/null || { echo "shots: no display; install xvfb (xvfb-run)" >&2; exit 2; }
    RUN=(xvfb-run -a)
fi
shot() {  # shot NAME SIZE VIEW [OPEN] [TAB]
    local name=$1 size=$2 view=$3 open=${4:-} tab=${5:-}
    local cfg="$OUT/cfg-$name"
    rm -rf "$cfg"; mkdir -p "$cfg"
    local args=(--mock --view "$view" --size "$size" --screenshot "$OUT/$name.png")
    [ -n "$open" ] && args+=(--open "$open")
    [ -n "$tab" ] && args+=(--tab "$tab")
    XDG_CONFIG_HOME="$PWD/$cfg" AUDRA_MOCK_DEMO=1 timeout 90 "${RUN[@]}" "$BIN" "${args[@]}" > "$OUT/$name.log" 2>&1
    if [ -s "$OUT/$name.png" ]; then echo "shot $name: ok"; else echo "shot $name: FAILED (see $OUT/$name.log)"; fails=$((fails + 1)); fi
    rm -rf "$cfg"
}
shot mixer 1440x900 mixer
shot mixer-small 1280x700 mixer
shot quick 1440x900 quick
shot matrix 1440x900 matrix
shot settings 1440x900 mixer settings
shot settings-monitor 1440x900 mixer settings-monitor
shot fx 1440x900 mixer fx-heard fx
shot fx-unheard 1440x900 mixer fx-unheard fx
shot fx-hazard 1440x900 mixer fx-hazard fx
shot fx-held 1440x900 mixer fx-held
shot voice 1440x900 mixer dyn
shot phones-loudness 1440x900 mixer phones-dyn
shot eq 1440x900 mixer output-eq
shot open-mic 1440x900 mixer open-mic
shot setup-stream 1440x900 quick setup-stream
shot quick-speakers 1440x900 quick quick-speakers
shot workspaces 1440x900 mixer workspaces
shot device 1440x900 mixer device
shot talkback 1440x900 mixer talkback-on
shot guide 1440x900 mixer guide-step
echo "shots: $fails failed"
[ "$fails" = 0 ]
