#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Spike S4: generates synthetic trees and benchmarks scanning them into SQLite.
#   ./run.sh                     # 100k and 1M files
#   SIZES="100000" ./run.sh      # only 100k
#   DROP_CACHES=1 ./run.sh       # Linux only: cold-cache first scan (needs sudo)
# Output: out/results-<os>.md
set -euo pipefail
cd "$(dirname "$0")"
[ "$(uname -s)" = Darwin ] && export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

SIZES="${SIZES:-100000 1000000}"
case "$(uname -s)" in
  Darwin) OS=macos; MODES="lstat" ;;
  Linux) OS=linux; MODES="lstat" ;;
  MINGW*|MSYS*|CYGWIN*) OS=windows; MODES="std handle dirinfo" ;;
  *) echo "unsupported OS" >&2; exit 1 ;;
esac

cargo build --release --quiet
BIN="target/release/kipple-spike-scan"
mkdir -p out
report="out/results-$OS.md"
{
  echo "## $OS · $(uname -m) · $(date '+%Y-%m-%d %H:%M')"
  echo
  echo "CPU threads: $(getconf _NPROCESSORS_ONLN 2>/dev/null || echo "$NUMBER_OF_PROCESSORS")"
  echo
} > "$report"

for n in $SIZES; do
  tree="out/tree-$n"
  if [ ! -d "$tree" ]; then
    start=$(date +%s)
    "$BIN" gen "$tree" "$n"
    echo "Generated $n files in $(( $(date +%s) - start )) s" | tee -a "$report"
  fi
  for mode in $MODES; do
    if [ "${DROP_CACHES:-0}" = 1 ] && [ "$OS" = linux ]; then
      sync && sudo sh -c 'echo 3 > /proc/sys/vm/drop_caches' && echo "(page cache dropped: cold first scan)" >> "$report"
    fi
    echo >> "$report"
    echo "### $n files · mode \`$mode\`" >> "$report"
    echo >> "$report"
    "$BIN" bench "$tree" "$mode" | tee -a "$report"
  done
done
echo
echo "Saved to $report"
