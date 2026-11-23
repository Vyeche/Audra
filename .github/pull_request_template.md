## What changed and why

<!-- In plain words. Link the issue it closes: "Closes #12". -->

## The gauntlet

<!-- Run `bash tools/checks/totalMix-gauntlet.sh` and paste its last line
     (target/gauntlet/summary.txt), for example:
     GAUNTLET: PASS (build ok, tests ok, lints ok, safety ok, hygiene ok; 861 tests passed) at abc1234 against origin/main -->

```
GAUNTLET: ...
```

## How it was verified

- [ ] `bash tools/checks/totalMix-gauntlet.sh` passed (its line is above)
- [ ] Tried in the simulation (`cargo run --release -p audra -- --mock`)
- [ ] Tried on a real Babyface Pro FS (driver version: )
- [ ] N/A (docs, templates or pictures only)

## Safety (fill in if the safety gate says "touched")

<!-- What can now play where: which inputs reach the speakers, the
     headphones or the stream, and at what level. Name the test that shows
     it (crates/app/src/listener_safety_tests.rs). -->

## Screenshots

<!-- If something on screen changed: before and after, from
     `bash tools/checks/totalMix-gauntlet.sh --shots` (target/gauntlet/shots)
     or the pull request's "screenshots" download. -->

## AI-assisted?

- [ ] No
- [ ] Yes, with: <!-- which tool or model -->

- [ ] I removed local paths, hostnames, user names and tokens from this text and from any logs.
