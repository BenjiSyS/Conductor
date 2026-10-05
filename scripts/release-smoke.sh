#!/usr/bin/env bash
# Start an installed Conductor and check it really runs: the process stays
# alive, it creates its local database, and (on Windows) its window loads.
# Used by the release workflow on every OS; also handy on a new machine.
#
# Usage: scripts/release-smoke.sh <path-to-conductor-executable>
set -euo pipefail

exe="$1"
data="$(mktemp -d)"
export CONDUCTOR_DATA_DIR="$data"
export CONDUCTOR_DEVTOOLS_PORT=9471
# Older builds read the debugging port from the WebView2 variable instead.
export WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$CONDUCTOR_DEVTOOLS_PORT"
echo "Starting: $exe"
echo "Data dir: $data"

if [[ "$(uname -s)" == Linux* ]] && [[ -z "${DISPLAY:-}" ]]; then
  echo "No display: run this under xvfb-run" >&2
  exit 2
fi

"$exe" > "$data/stdout.log" 2> "$data/stderr.log" &
pid=$!

# Stop the app and anything it started (Git Bash needs the Windows PID).
stop() {
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      winpid="$(cat "/proc/$pid/winpid" 2>/dev/null || echo "$pid")"
      taskkill //F //T //PID "$winpid" >/dev/null 2>&1 || true ;;
    *) kill "$pid" 2>/dev/null || true ;;
  esac
}

ok_db=0
for _ in $(seq 1 60); do
  if [[ -f "$data/state.db" ]]; then ok_db=1; break; fi
  if ! kill -0 "$pid" 2>/dev/null; then break; fi
  sleep 1
done

# Give the window time to start; a webview crash ends the process.
sleep 15
alive=0
kill -0 "$pid" 2>/dev/null && alive=1

window=skipped
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    window=fail
    for _ in $(seq 1 30); do
      if curl -fsS "http://127.0.0.1:$CONDUCTOR_DEVTOOLS_PORT/json/list" -o "$data/pages.json" 2>/dev/null \
        && grep -q '"type": *"page"' "$data/pages.json"; then
        window=ok; break
      fi
      sleep 2
    done
    ;;
esac

echo "database created: $ok_db, still running: $alive, window: $window"
# Builds before v0.3.0 can't open the debugging port on every Windows setup;
# for those a missing window check is reported but doesn't fail.
if [[ "$window" == fail && "${SMOKE_WINDOW:-required}" == optional ]]; then
  echo "window check unavailable for this build (warning only)"
  window=unverified
fi
if [[ "$ok_db" != 1 || "$alive" != 1 || "$window" == fail ]]; then
  echo "--- stderr ---"; tail -c 3000 "$data/stderr.log" || true
  echo "--- stdout ---"; tail -c 2000 "$data/stdout.log" || true
  stop
  exit 1
fi

stop
echo "Conductor runs."
