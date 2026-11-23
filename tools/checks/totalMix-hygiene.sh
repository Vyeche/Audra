#!/usr/bin/env bash
# The gauntlet's hygiene check: no secrets, no internal files (the public
# copy's drop list), no file over 5 MB, in what a change adds.
#
#   bash tools/checks/totalMix-hygiene.sh [BASE]     (BASE: origin/main)
#
# Reads git only.
set -u
cd "$(dirname "$0")/../.." || exit 2
BASE=${1:-origin/main}
MB=$(git merge-base "$BASE" HEAD 2>/dev/null) || { echo "hygiene: can't find the base $BASE" >&2; exit 2; }
fail=0

# Names as they are, not quoted (git quotes non-ASCII names by default,
# and a quoted name fails every test below).
added=$(git -c core.quotepath=off diff --name-only --diff-filter=AM "$MB" HEAD)

# 1. Internal files: publish/drop-list.txt in the working repo,
# .github/internal-paths.txt in the public copy. The files the public copy
# keeps (publish/keep-list.txt, .github/keep-paths.txt) are exempt.
# In the private working repo (publish/ exists) internal files belong, so
# that check runs in the public copy only.
if [ -d publish ]; then
    DROP=/nonexistent; KEEP=/nonexistent
    echo "hygiene: the working repo (internal files belong here; that check runs in the public copy)"
else
    DROP=.github/internal-paths.txt; KEEP=.github/keep-paths.txt
fi
if [ -f "$KEEP" ]; then
    added=$(printf '%s\n' "$added" | grep -vxF -f <(grep -v '^#' "$KEEP" | sed '/^$/d') || true)
fi
if [ -f "$DROP" ]; then
    while IFS= read -r pat; do
        case "$pat" in ''|'#'*) continue ;; esac
        hit=$(printf '%s\n' "$added" | while IFS= read -r f; do
            case "$pat" in
                */) [ "${f#"$pat"}" != "$f" ] && echo "$f" ;;
                *) case "$f" in $pat) echo "$f" ;; esac ;;
            esac
        done)
        if [ -n "$hit" ]; then
            echo "hygiene: an internal file ($DROP: $pat):" >&2
            printf '%s\n' "$hit" | sed 's/^/  /' >&2
            fail=1
        fi
    done < "$DROP"
fi

# 2. Big files.
while IFS= read -r f; do
    [ -n "$f" ] && [ -f "$f" ] || continue
    size=$(stat -c %s "$f")
    if [ "$size" -gt 5242880 ]; then
        echo "hygiene: $f is $((size / 1048576)) MB (over 5 MB)" >&2
        fail=1
    fi
done <<EOF_ADDED
$added
EOF_ADDED

# 3. Secrets in the added lines: private keys, common token shapes,
# passwords written into URLs.
secrets=$(git diff -U0 "$MB" HEAD | grep -E '^\+' | grep -nE \
    -e '-----BEGIN [A-Z ]*PRIVATE KEY-----' \
    -e 'gh[pousr]_[A-Za-z0-9]{36,}' \
    -e 'github_pat_[A-Za-z0-9_]{40,}' \
    -e 'glpat-[A-Za-z0-9_-]{20,}' \
    -e 'xox[abpr]-[A-Za-z0-9-]{10,}' \
    -e 'AKIA[0-9A-Z]{16}' \
    -e 'sk-[A-Za-z0-9]{32,}' \
    -e '[a-z]+://[^/[:space:]:@]+:[^/[:space:]@]+@' | head -5)
if [ -n "$secrets" ]; then
    echo "hygiene: something that looks like a secret in the added lines:" >&2
    printf '%s\n' "$secrets" | cut -c1-120 >&2
    fail=1
fi

if [ "$fail" = 0 ]; then
    echo "hygiene: ok"
fi
exit "$fail"
