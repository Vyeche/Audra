#!/usr/bin/env bash
# The gauntlet's lints: the compiler's and Clippy's checks that catch real
# mistakes (Clippy's "correctness" and "suspicious" groups, as errors).
# The project doesn't follow rustfmt's layout, so formatting isn't checked.
#
#   bash tools/checks/totalMix-lints.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
cargo clippy --workspace --all-targets --locked -- \
    -A clippy::all \
    -D clippy::correctness \
    -D clippy::suspicious
