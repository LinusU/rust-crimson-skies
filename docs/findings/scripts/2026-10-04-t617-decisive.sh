#!/bin/zsh
# The decisive measurement campaign for task #617: attribute a deletion burst
# to a process.
#
#   sh docs/findings/scripts/2026-10-04-t617-decisive.sh <first-index> <run-count> [sample-interval] [watch-interval]
#
# Three instruments run for the whole window:
#
#   * sample-procs.py  every pid that appears on the host, with ppid and command
#                      line. A prune pass is not a daemon: it exists only while
#                      it deletes, so sampling the table is the only way to see
#                      it without root (fs_usage and dtrace both need it, and
#                      FSEvents reports no pid).
#   * watch-tree.py    every executable unlinked anywhere under target/debug,
#                      with the stat attributes it had.
#   * run-workspace-test.sh, once per index, the real gate with its own
#                      per-run record, so no run overwrites another's.
#
# A removal burst that lands in the same process-table sample as a child of the
# disk-prune loop shell is attribution rather than correlation.

set -u

first="${1:?usage: decisive.sh <first-index> <run-count> [sample] [watch]}"
count="${2:?usage: decisive.sh <first-index> <run-count> [sample] [watch]}"
sample_interval="${3:-0.4}"
watch_interval="${4:-0.4}"

here="$(cd "$(dirname "$0")" && pwd)"
out="${OUT:-${TMPDIR:-/tmp}/t617}"
root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"

mkdir -p "$out"
cd "$root" || exit 1

python3 "$here/2026-10-04-t617-sample-procs.py" \
  "$sample_interval" "$out/t617.procs.jsonl" &
sampler=$!

python3 "$here/2026-10-04-t617-watch-tree.py" \
  target/debug "$watch_interval" "$out/t617.tree.jsonl" &
watcher=$!

echo "started $(date -u +%Y-%m-%dT%H:%M:%SZ) sampler=$sampler watcher=$watcher" \
  > "$out/t617.decisive.meta"

for i in $(seq 1 "$count"); do
  label="t617-$((first + i - 1))"
  OUT="$out" sh "$here/2026-10-04-t617-run-workspace-test.sh" "$label" \
    >> "$out/decisive.out" 2>&1
  echo "=== finished $label at $(date -u +%Y-%m-%dT%H:%M:%SZ) ===" \
    >> "$out/decisive.out"
done

kill "$sampler" "$watcher" 2>/dev/null
echo "DECISIVE COMPLETE $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$out/decisive.out"
