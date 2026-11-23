# Rules for AI helpers working on Audra

These apply to any AI agent (Claude, Codex, Copilot, local models, others) that writes code, issues or pull requests for this repository. People are welcome to follow them too.

## Before you open a pull request

1. Run the gauntlet: `bash tools/checks/totalMix-gauntlet.sh` (add `--shots` if anything on screen changed).
2. Paste its last line from `target/gauntlet/summary.txt` into the pull request, as it came out. Never write "tests pass" without that output.
3. Fill in the pull request form, including "AI-assisted? Yes, with: …".

## Safety

Audra routes microphones (often with 48V phantom power) to speakers, headphones and streams. A mistake can cause feedback or a loud burst.

- A change to what plays where, levels, mutes, solo, Mute FX, the FX returns, the stream or recording needs a test that shows the result: see `crates/app/src/listener_safety_tests.rs`. The gauntlet's safety gate checks for one.
- Lower levels may land at once; raised levels glide (`set_level_gently`). Keep it that way.
- Never send an open mic (48V, or high gain) into the speakers, and never let its reverb play there.

## Tests and the machine

- Tests and checks use the simulation (`--mock`) and `--screenshot`. They never open a window, play a sound, restart or reconfigure PipeWire, or write to a real Babyface.
- Write outputs under `target/`.

## Public text is public

Issues, pull requests and their comments are public and permanent.

- Before posting, remove local paths, hostnames, IP addresses, user names, email addresses and tokens from your text and from any logs.
- Use made-up names in examples and fixtures.
- Don't paste other people's copyrighted material.

## Style

- Plain English in the interface, for people who aren't audio engineers: say what a control does for them.
- Follow the code around you (long lines are fine; formatting isn't checked).
- Comments say why, with the issue or review that asked for it.
