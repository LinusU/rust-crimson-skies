#!/bin/zsh
# One instrumented `cargo test --workspace --locked` run, for task #617.
#
#   sh docs/findings/scripts/2026-10-04-t617-run-workspace-test.sh <label>
#
# Run it from a checkout root. Everything it writes goes to $OUT (default
# $TMPDIR/t617), never into the repository.
#
# It records, into $OUT/<label>.*:
#   .env          environment before and after the run: head, branch, rustc,
#                 CARGO_TARGET_DIR, free space on the data volume, load average,
#                 the executable count in target/debug/deps, free space again,
#                 the exit status, and how many `test result: ok`,
#                 `test result: FAILED` and `never executed` lines the run printed
#   .log          the run's own output, every line prefixed with elapsed seconds
#                 and a wall clock, which is what puts the build/execution phase
#                 boundary and a failure on a shared time axis
#   .deps.jsonl   every executable that appeared or disappeared in
#                 target/debug/deps during the run, with the size and mtime age
#                 it had (watch-deps.py)
#   .procs        every cargo/rustc pid seen on the host with its working
#                 directory, so a second cargo writing *this* target directory
#                 would show up here

set -u

label="${1:?usage: run-workspace-test.sh <label>}"
root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
out="${OUT:-${TMPDIR:-/tmp}/t617}"
here="$(cd "$(dirname "$0")" && pwd)"

mkdir -p "$out"
cd "$root" || exit 1

# Executables in deps/, which is where cargo leaves the test harnesses it execs.
count_execs() {
  ls -l target/debug/deps 2>/dev/null |
    awk '$1 ~ /^-.*x/ && $9 !~ /\.(d|rlib|rmeta|so|dylib|a|o|json|lock)$/' |
    wc -l | tr -d ' '
}

free_kb() { df -k /System/Volumes/Data 2>/dev/null | awk 'NR==2{print $4}'; }

{
  echo "label=$label"
  echo "started=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "head=$(git rev-parse HEAD)"
  echo "branch=$(git rev-parse --abbrev-ref HEAD)"
  echo "rustc=$(rustc -V)"
  echo "cargo_target_dir=${CARGO_TARGET_DIR:-unset}"
  echo "df_data_avail_kb=$(free_kb)"
  echo "loadavg=$(sysctl -n vm.loadnet 2>/dev/null || sysctl -n vm.loadavg)"
  echo "execs_before=$(count_execs)"
  echo "deps_entries_before=$(ls target/debug/deps | wc -l | tr -d ' ')"
} > "$out/$label.env"

python3 "$here/2026-10-04-t617-watch-deps.py" \
  target/debug/deps 0.25 "$out/$label.deps.jsonl" &
watcher=$!

(
  while true; do
    for pid in $(pgrep -x cargo; pgrep -x rustc); do
      cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
      echo "$(date +%s) pid=$pid cwd=$cwd"
    done
    sleep 5
  done
) > "$out/$label.procs" &
sampler=$!

set -o pipefail
cargo test --workspace --locked 2>&1 |
  python3 "$here/2026-10-04-t617-timestamper.py" "$out/$label.log" > /dev/null
exit_code=$?
set +o pipefail

sleep 1
kill "$watcher" "$sampler" 2>/dev/null
wait 2>/dev/null

{
  echo "exit=$exit_code"
  echo "finished=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "df_data_avail_kb=$(free_kb)"
  echo "execs_after=$(count_execs)"
  echo "deps_entries_after=$(ls target/debug/deps | wc -l | tr -d ' ')"
  echo "test_result_ok=$(grep -c 'test result: ok' "$out/$label.log")"
  echo "test_result_failed=$(grep -c 'test result: FAILED' "$out/$label.log")"
  echo "never_executed=$(grep -c 'never executed' "$out/$label.log")"
  echo "build_phase_finished_at=$(grep -m1 'Finished' "$out/$label.log" | cut -d' ' -f1-2)"
  echo "first_running_at=$(grep -m1 -E ' +Running ' "$out/$label.log" | cut -d' ' -f1-2)"
  echo "running_units=$(grep -c -E ' +Running ' "$out/$label.log")"
} >> "$out/$label.env"

cat "$out/$label.env"
