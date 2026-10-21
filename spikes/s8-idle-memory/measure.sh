#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Spike S8: idle memory of the spike app in each mode (open / hide / destroy), on macOS and Linux.
# Sums the whole process family: the app plus its webview processes (WebKit XPC services on
# macOS, WebKitWebProcess/WebKitNetworkProcess children on Linux). Windows: measure.ps1.
#
#   bash spikes/s8-idle-memory/measure.sh        # RUNS=3 IDLE=30 by default
# Linux needs a display: run it under `xvfb-run -a`.
set -euo pipefail
cd "$(dirname "$0")"

BIN=src-tauri/target/release/kipple-spike-s8
RUNS="${RUNS:-3}"
IDLE="${IDLE:-30}"   # seconds after the hide/destroy (which happens 8 s after start)
OS="$(uname)"
mkdir -p out
RAW="out/raw-$OS.tsv"
: > "$RAW"

descendants() { # pid -> pid and all its descendants
  echo "$1"
  for child in $(ps -o pid= --ppid "$1" 2>/dev/null); do descendants "$child"; done
}

webkit_pids() { # macOS: WebKit service processes (they're launchd's children, not ours)
  ps -axo pid=,comm= | awk '/com\.apple\.WebKit\./ {print $1}'
}

measure() { # pid -> "processes rss_kib private_kib"
  local pid=$1 pids rss=0 priv=0 n=0
  if [ "$OS" = Darwin ]; then
    pids="$pid $(comm -13 <(sort <<<"$WEBKIT_BEFORE") <(webkit_pids | sort))"
  else
    pids="$(descendants "$pid")"
  fi
  for p in $pids; do
    kill -0 "$p" 2>/dev/null || continue
    n=$((n + 1))
    rss=$((rss + $(ps -o rss= -p "$p" | tr -d ' ')))
    if [ "$OS" = Darwin ]; then
      # Physical footprint (what Activity Monitor shows). Apple's WebKit services need sudo
      # (passwordless on CI); without it they count by RSS only.
      fp=$({ vmmap -summary "$p" 2>/dev/null || sudo -n vmmap -summary "$p" 2>/dev/null || true; } |
        awk '/^Physical footprint:/ {print $3; exit}')
      case "$fp" in
        *K) priv=$((priv + ${fp%K})) ;;
        *M) priv=$((priv + $(awk "BEGIN {printf \"%d\", ${fp%M} * 1024}"))) ;;
        *G) priv=$((priv + $(awk "BEGIN {printf \"%d\", ${fp%G} * 1048576}"))) ;;
      esac
    else
      # PSS: shared pages split fairly between the processes that use them.
      priv=$((priv + $(awk '/^Pss:/ {print $2}' "/proc/$p/smaps_rollup" 2>/dev/null || echo 0)))
    fi
  done
  echo "$n $rss $priv"
}

stop_tree() { # kill the app and everything it started; leftovers break the next run on Linux
  local pids
  if [ "$OS" = Darwin ]; then pids="$1"; else pids="$(descendants "$1")"; fi
  kill $pids 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    kill -0 $pids 2>/dev/null || return 0
    sleep 0.5
  done
  kill -9 $pids 2>/dev/null || true
}

pid=""
trap '[ -n "$pid" ] && stop_tree "$pid"' EXIT

for mode in ${MODES:-open hide destroy}; do
  for run in $(seq "$RUNS"); do
    WEBKIT_BEFORE="$( [ "$OS" = Darwin ] && webkit_pids || true )"
    S8_MODE=$mode "$BIN" 2>"out/$mode-$run.log" &
    pid=$!
    sleep $((8 + IDLE))
    read -r n rss priv < <(measure "$pid")
    # Only count runs where the hide/destroy really happened (the app logs it).
    valid=1
    [ "$mode" = open ] || grep -q "^S8: $mode -> Ok" "out/$mode-$run.log" || valid=0
    if [ "$valid" = 0 ] && [ "$OS" = Linux ] && command -v gdb >/dev/null; then
      sudo -n gdb -p "$pid" -batch -ex 'thread apply all bt 25' > "out/$mode-$run.stacks" 2>&1 || true
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$mode" "$run" "$n" "$rss" "$priv" "$valid" | tee -a "$RAW"
    stop_tree "$pid"; wait "$pid" 2>/dev/null || true
    pid=""
    sleep 2
  done
done

# Median per mode, in MiB.
second=$([ "$OS" = Darwin ] && echo "Physical footprint" || echo "PSS")
{
  echo "### $OS ($RUNS runs, ${IDLE}s idle, median)"
  echo
  echo "| Mode | Processes | RSS (MiB) | $second (MiB) | Valid runs |"
  echo "|---|---|---|---|---|"
  for mode in open hide destroy; do
    valid=$(awk -F'\t' -v m="$mode" '$1 == m && $6 == 1' "$RAW" | wc -l | tr -d ' ')
    awk -F'\t' -v m="$mode" '$1 == m && $6 == 1' "$RAW" | sort -t$'\t' -k4 -n | awk -F'\t' -v n="$valid" -v runs="$RUNS" '
      NR == int((n + 1) / 2) { printf "| %s | %d | %.0f | %.0f | %d/%d |\n", $1, $3, $4 / 1024, $5 / 1024, n, runs }'
  done
} | tee "out/results-$OS.md"
