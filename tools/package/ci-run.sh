#!/usr/bin/env bash
# CI: runs a command; when it fails, its errors become an annotation, which anyone can read on the
# run's page (job logs need a token).
log="$(mktemp)"
"$@" 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
if [ "$status" -ne 0 ]; then
    msg="$(grep -E -A6 '^error|^test .* FAILED|panicked at|^failures:|fatal error|^ld: |^clang: error|^Undefined symbols' "$log" | grep -v '^--$' | cut -c1-240 | tail -n 45 | awk 'BEGIN { ORS = "%0A" } { gsub(/%/, "%25"); print }')"
    echo "::error title=$1 failed::$msg"
fi
exit "$status"
