#!/usr/bin/env bash
# The gauntlet, run locally: the same checks a pull request goes through on
# GitHub (.github/workflows/gauntlet.yml). Run it before you push.
#
#   bash tools/checks/totalMix-gauntlet.sh [--shots] [BASE]
#
# BASE: the branch your change is against (origin/main). --shots also draws
# the screenshots (a release build, a few minutes). Output: target/gauntlet/,
# with summary.txt holding the one line to paste into a pull request.
#
# Nothing here opens a window, plays a sound, touches PipeWire or talks to
# a Babyface: the tests run against the simulation.
set -u
cd "$(dirname "$0")/../.." || exit 2
SHOTS=0
BASE=origin/main
for a in "$@"; do
    case "$a" in
        --shots) SHOTS=1 ;;
        *) BASE=$a ;;
    esac
done
OUT=target/gauntlet
mkdir -p "$OUT"
results=()
ok=1
lane() {  # lane NAME COMMAND...
    local name=$1; shift
    echo "== $name"
    if "$@" > "$OUT/$name.log" 2>&1; then
        results+=("$name ok")
        tail -n 3 "$OUT/$name.log"
    else
        results+=("$name FAILED")
        ok=0
        tail -n 20 "$OUT/$name.log"
    fi
}
lane build cargo build --workspace --locked
lane tests cargo test --workspace --locked
lane lints bash tools/checks/totalMix-lints.sh
lane safety bash tools/checks/totalMix-safety-gate.sh "$BASE"
lane hygiene bash tools/checks/totalMix-hygiene.sh "$BASE"
if [ "$SHOTS" = 1 ]; then
    lane release cargo build --release -p audra --locked
    lane shots bash tools/checks/totalMix-gauntlet-shots.sh target/release/audra "$OUT/shots"
fi
passed=$(grep -ho 'test result: ok. [0-9]* passed' "$OUT/tests.log" | awk '{s += $4} END {print s + 0}')
joined=$(printf '%s, ' "${results[@]}"); joined=${joined%, }
line="GAUNTLET: $([ "$ok" = 1 ] && echo PASS || echo FAIL) ($joined; $passed tests passed) at $(git rev-parse --short HEAD) against $BASE"
echo "$line" | tee "$OUT/summary.txt"
[ "$ok" = 1 ]
