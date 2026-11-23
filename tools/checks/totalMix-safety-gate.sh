#!/usr/bin/env bash
# The gauntlet's safety gate. A change to the code that decides what plays
# where (levels, mutes, speakers, open mics, the stream, recording) must
# come with a test: a change to crates/app/src/listener_safety_tests.rs or
# a new #[test] somewhere in the diff.
#
#   bash tools/checks/totalMix-safety-gate.sh [BASE]     (BASE: origin/main)
#
# Prints "safety: not touched", "safety: touched, tested" or fails with
# "safety: touched, NO TEST". It reads git only: it never builds or runs
# the app.
set -u
cd "$(dirname "$0")/../.." || exit 2
BASE=${1:-origin/main}
MB=$(git merge-base "$BASE" HEAD 2>/dev/null) || { echo "safety: can't find the base $BASE" >&2; exit 2; }

# The files that decide what plays where. Keep in step with
# docs/gauntlet-plan.md and CONTRIBUTING.md.
SAFETY_RE='^(crates/app/src/(fx|hearfix|control_room|setups|quick|recorder|dynamics_loop|dynamics_panel|resetmix|assist|workspaces|scenes|mic_presets|eq_presets|mixops|osc|midi|mackie|control)\.rs|crates/app/src/control_room/|crates/tuxmix-core/|crates/tuxmix-usb/|driver/)'

changed=$(git diff --name-only "$MB" HEAD)
# The stream and Voice routing live in app.rs among much else: there, the
# functions that move levels for the speakers or the stream count.
app_safety=$(git diff -U0 "$MB" HEAD -- crates/app/src/app.rs | grep -E '^[-+].*(set_level_gently|set_remote_level|fader_glides|step_fader_glides|capped_share|gain_ramps|remote_rise|remote_gain_rise|set_input_gain|set_loopback|OscCommand|voice_off_keeps_the_stream|plain_mic_looped|stream_takes_voice|input_looped|apply_mix_scene|set_volume\(|set_mute\(|set_solo\(|phantom|fx_return|set_fx_)' | head -1)

touched=$(printf '%s\n' "$changed" | grep -E "$SAFETY_RE" || true)
[ -n "$app_safety" ] && touched=$(printf '%s\ncrates/app/src/app.rs (levels, the stream or Voice)\n' "$touched" | sed '/^$/d')

if [ -z "$touched" ]; then
    echo "safety: not touched"
    exit 0
fi

tested=""
printf '%s\n' "$changed" | grep -q '^crates/app/src/listener_safety_tests.rs$' && tested=yes
git diff -U0 "$MB" HEAD -- 'crates/**/*.rs' | grep -qE '^\+\s*#\[test\]' && tested=yes

echo "safety files changed:"
printf '%s\n' "$touched" | sed 's/^/  /'
if [ -n "$tested" ]; then
    echo "safety: touched, tested (the owner reviews every safety change)"
    exit 0
fi
# The maintainer's label after reading the change ("safety: look only"),
# passed in by the workflow: a change in these files that moves no sound.
if [ "${SAFETY_LOOK_ONLY:-}" = true ]; then
    echo "safety: touched, no test, marked look-only by the maintainer"
    exit 0
fi
echo "safety: touched, NO TEST. Add a test that shows what now plays where (crates/app/src/listener_safety_tests.rs has many to copy)." >&2
exit 1
