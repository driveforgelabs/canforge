#!/usr/bin/env bash
# Hot reload for canforge: rebuild whenever a source file is saved and swap the running app for the
# new build. The old window stays up while it compiles, and a failed build leaves it running (the
# compiler errors are printed here). The open page is restored after each restart.
#
#   scripts/dev.sh                 # simulator
#   scripts/dev.sh --replay examples/datalog.csv
#
# GPUI has no live code patching, so this is a fast restart, not an in-place reload: state other
# than the open page (selected code, scroll position, a running job) starts fresh.
set -u
cd "$(dirname "$0")/.."

export CANFORGE_DEV=1
args=("$@")
[ ${#args[@]} -eq 0 ] && args=(--simulate)

stamp() { find src Cargo.toml ../candriver/crates -name '*.rs' -o -name Cargo.toml 2>/dev/null | xargs stat -c %Y 2>/dev/null | sort -n | tail -1; }

pid=""
stop() { [ -n "$pid" ] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null; pid=""; }
trap 'stop; exit 0' INT TERM

last=""
while true; do
  now=$(stamp)
  if [ "$now" != "$last" ]; then
    last=$now
    echo "── building…"
    if cargo build --quiet 2>&1; then
      stop
      ./target/debug/canforge "${args[@]}" &
      pid=$!
      echo "── running (pid $pid)"
    else
      echo "── build failed, keeping the previous version running"
    fi
  fi
  # the window was closed: leave
  if [ -n "$pid" ] && ! kill -0 "$pid" 2>/dev/null; then echo "── app closed"; exit 0; fi
  sleep 0.5
done
