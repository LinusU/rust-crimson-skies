# #617: the ENOENT-on-exec failure is external to the repository, and it is measurable on demand

Date: 2026-10-04. Task: #617 "cargo test --workspace can die with ENOENT on exec
of a unit-test binary of a crate that has real unit tests (cs_inspect --lib),
which #610's cs_xtask fix does not cover". Capabilities used: ordinary
build/test only — no `CS_GAME_DIR`, no GPU, audio or network capability, so no
`acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`). Machine:
macOS 26.6.2 aarch64, rustc 1.98.1, APFS at 97 % capacity, load average 80-110
from the other agents' builds, `bunny-alpha-2` checkout with its own
`CARGO_TARGET_DIR`.

This is a finding about the project's own test gate on one multi-agent machine.
It says nothing about the original game.

## Summary

The failure reproduces spontaneously on demand here, at a measurable rate, and
the deleting process is identified. It is not a bug in this repository, it is
not fixed by anything in this repository, and no `test = false` anywhere in
this workspace would prevent it.

Three results, each measured below:

1. **The reported signature reproduces spontaneously** — `could not execute
   process … (never executed)` / `No such file or directory (os error 2)` — in
   ordinary `cargo test --workspace --locked` runs on an unmodified
   `origin/main` checkout, without any second cargo, without any test failure,
   and while the deleting pass is provably not cargo.
2. **The deleting command was captured in the act**, with its arguments:
   `prune-stale-bins.sh --delete …/bunny-alpha-2/target/debug/deps …`, a
   0.5 s child of the owner's `disk-prune-loop.sh`, appearing 0.19 s before
   the nine files it deleted went away. Bursts recur on the loop's own 1200 s
   period — four of them, 20 min apart, to the second.
3. **Whether a run dies is decided by one thing only: whether it reaches a
   doomed harness before the prune does.** Two runs of the identical command on
   the identical tree, where the prune deleted the *same nine files*, one died
   and one passed, differing only in scheduling order.
4. **Every crate is exposed, `test = false` cannot help, and this workspace's
   own test suite cannot cause it.** A harness of `cs_inspect` — a crate with
   92 real `#[test]`s in `src/` — was the victim in one measured occurrence, so
   the #610 fix does not generalise and, more importantly, would not have
   prevented this occurrence even if it had.

The honest consequence for the owner is a decision, not a patch: this is task
#613 (F54-X8), which is already filed and blocked. What this task adds is the
reproduction on demand and the attribution, so the owner can decide between
stopping the prune from touching live target directories, serialising the
agents, freeing disk, or accepting the flake with the documented rerun rule.

## What was run

Every number in this document comes from an instrumented run of the real gate.
The instruments are in `docs/findings/scripts/`:

| Script | What it records |
| --- | --- |
| `2026-10-04-t617-run-workspace-test.sh` | one `cargo test --workspace --locked` with timestamps on every output line, `df` and load average before and after, the executable count in `target/debug/deps` before and after, and the number of `test result: ok` / `never executed` lines |
| `2026-10-04-t617-watch-deps.py` | every executable that appears or disappears in `target/debug/deps` during a run, with the size and mtime age it had |
| `2026-10-04-t617-watch-tree.py` | the same over `target/debug` recursively, with inode, link count and mtime/ctime/atime ages |
| `2026-10-04-t617-sample-procs.py` | every pid that appears on the host, with its ppid and full command line, on a tight interval |

The last one is the attribution instrument. `fs_usage`, `dtrace` and
`sudo`-gated tools are unavailable to an agent on this host, and FSEvents
reports no pid, so attribution has to come from sampling the process table: a
prune pass is not a daemon, it exists only while it deletes.

## Reproduction: spontaneous, on an unmodified tree

Two occurrences, both from `cargo test --workspace --locked` on
`origin/main` (`2676355d`), branch `rally/617-…`, with this checkout's own
`CARGO_TARGET_DIR` and no other cargo in that directory.

**Occurrence 1** — run started 2026-10-04T06:21:42Z, died at t=83.75 s:

```
    83.63      Running unittests src/lib.rs (target/debug/deps/cs_content-4762c26f54374704)
    83.75 error: test failed, to rerun pass `-p cs_content --test accept_doclib_conflict`

    Caused by:
      could not execute process `/…/target/debug/deps/accept_doclib_conflict-5f1a24873210ff70` (never executed)

    Caused by:
      No such file or directory (os error 2)
```

**Occurrence 2** — run started 2026-10-04T06:41:29Z, died at t=148.79 s, and
the victim is the crate this task is about:

```
   148.79 error: test failed, to rerun pass `-p cs_inspect --lib`

    Caused by:
      could not execute process `/…/target/debug/deps/cs_inspect-e4bc649912dd2984` (never executed)

    Caused by:
      No such file or directory (os error 2)
```

`cs_inspect` carries 92 `#[test]`s in `src/` (11 modules, `campaign.rs` 2,
`catalog.rs` 12, `config.rs` 18, `handling.rs` 3, `interp.rs` 14, `resolve.rs`
5, `rof.rs` 9, `routes.rs` 3, `script_discovery.rs` 10, `textures.rs` 8,
`zbd.rs` 8), so `test = false` is not available for it and, as measured below,
would not have helped.

## The unlink is observed directly, not inferred

`watch_deps.py` records every executable that disappears from
`target/debug/deps`. Both occurrences above were preceded by an unlink burst,
in the same run, while cargo was executing harnesses:

| Run | Removals | Of those, in this run's own plan | Died? |
| --- | --- | --- | --- |
| `baseline` (06:21:42Z) | 9 | 1 | yes, on the one that was in the plan |
| `run-2` (06:41:29Z) | 6 | 6 | yes |
| runs 1, 3, 4, 5, 6, 7 | 0 | — | no |

The `baseline` burst, at t=65.404 s of the run, in one poll interval:

```
{"event": "removed", "t": 65.404, "name": "accept_doclib_conflict-3bad038241d20373", "last_size": 1214120, "last_mtime_age_s": 2012.8}
{"event": "removed", "t": 65.404, "name": "accept_doclib_conflict-5f1a24873210ff70", "last_size": 1214232, "last_mtime_age_s": 2026.8}
{"event": "removed", "t": 65.404, "name": "accept_doclib_conflict-d84789a84d5b4f8d", "last_size": 1197720, "last_mtime_age_s": 2052.8}
{"event": "removed", "t": 65.404, "name": "accept_f00_a_cli_smoke-74aedd76b562eae5", "last_size": 1119720, "last_mtime_age_s": 2010.8}
{"event": "removed", "t": 65.404, "name": "accept_f00_b_cli_help-2e3fd5c40f27f019", "last_size": 1129992, "last_mtime_age_s": 2017.8}
{"event": "removed", "t": 65.404, "name": "accept_f48_d_crash_recovery_matrix-511240aab3de1191", "last_size": 2365792, "last_mtime_age_s": 2010.8}
{"event": "removed", "t": 65.404, "name": "cs-6328c84296c392b8", "last_size": 124842640, "last_mtime_age_s": 1975.8}
{"event": "removed", "t": 65.404, "name": "cs_inspect-8a7a51aff306b943", "last_size": 13444872, "last_mtime_age_s": 2015.8}
{"event": "removed", "t": 65.404, "name": "cs_inspect-c0cbf818ae044ebc", "last_size": 1025384, "last_mtime_age_s": 2012.8}
```

Two details in that burst matter:

* **Nothing was re-added.** Over the following six minutes of watching there
  were zero `added` events, so these were deletions, not the
  unlink-then-rewrite that cargo/rustc do when they replace an artifact. This
  is the difference from the build-phase churn measured in the F54-X7 finding,
  where 46 harnesses were unlinked and all 46 came back.
* **The 8 files outside the plan were deleted too.** `accept_f00_a_cli_smoke`,
  `accept_f00_b_cli_help`, `accept_f48_d_crash_recovery_matrix`, both
  `cs_inspect` harnesses and the 119 MiB `cs` binary are not in that run's
  plan. So this is not a targeted removal of "the binary cargo is about to
  exec"; it is a sweep over the directory, and which of its victims is
  scheduled next is chance.

## Attribution: the prune loop, exactly

The owner's prune runs as a `screen` session:

```
SCREEN -dmS disk-prune bash /private/tmp/claude-501/…/scratchpad/disk-prune-loop.sh
```

Sampling the process table shows its structure: two long-lived shells (pids
66027, 87266) each hold a `sleep 1200`, and between two sleeps the loop forks a
short-lived child that *is* the pass.

### The attribution, caught in the act

One campaign ran the real gate 14 times while a 0.4 s process sampler and a
0.4 s target-dir watcher ran for the whole window. At **t=1073.886 s** the
sampler recorded the pass, and at **t=1074.073 s** — 0.19 s later — nine
`target/debug/deps` executables vanished. The command lines are in the same
log:

```
t=1073.886 pid=89108 ppid=66027  bash …/scratchpad/disk-prune-loop.sh
t=1073.886 pid=89109 ppid=89108  /bin/bash …/scratchpad/prune-stale-bins.sh --delete \
      /Users/linus/coding/rust-crimson-skies/bunny-2/target/debug/deps \
      /Users/linus/coding/rust-crimson-skies/bunny-alpha-1/target/debug/deps \
      /Users/linus/coding/rust-crimson-skies/bunny-alpha-2/target/debug/deps \
      /Users/linus/coding/rust-crimson-skies/deepseek-1/target/debug/deps \
      /Users/linus/coding/rust-crimson-skies/swe2-max-1/target/debug/deps
t=1073.886 pid=89110 ppid=89108  sed s|/Users/linus/coding/rust-crimson-skies/||; s|/target/debug/deps||
t=1073.886 pid=89168 ppid=89109  (stat)
```

**`bunny-alpha-2/target/debug/deps` is the third directory on that command
line.** The nine unlinks at t=1074.073 were that pass's work, and the pass
ended 0.5 s later with the next `sleep 1200`. This is not correlation and not a
candidate: it is the deleting command, its arguments, and the deleted files in
one time window.

The deleted set, with the stat attributes it had at deletion:

```
{"t": 1074.073, "path": "…/deps/accept_doclib_conflict-3bad038241d20373", "size": 1214120,   "inode": 407529103, "nlink": 1, "age_mtime_s": 2654.5, "age_ctime_s": 2654.5}
{"t": 1074.073, "path": "…/deps/accept_doclib_conflict-5f1a24873210ff70", "size": 1214232,   "inode": 407529102, "nlink": 1, "age_mtime_s": 2654.5, "age_ctime_s": 2654.5}
{"t": 1074.073, "path": "…/deps/accept_doclib_conflict-d84789a84d5b4f8d", "size": 1197720,   "inode": 407529101, "nlink": 1, "age_mtime_s": 2654.5, "age_ctime_s": 2654.5}
{"t": 1074.073, "path": "…/deps/accept_f00_a_cli_smoke-74aedd76b562eae5",  "size": 1119720,   "inode": 407529059, "nlink": 1, "age_mtime_s": 2655.3, "age_ctime_s": 2655.3}
{"t": 1074.073, "path": "…/deps/accept_f00_b_cli_help-2e3fd5c40f27f019",    "size": 1129992,   "inode": 407529082, "nlink": 1, "age_mtime_s": 2654.9, "age_ctime_s": 2654.9}
{"t": 1074.073, "path": "…/deps/accept_f48_d_crash_recovery_matrix-5112…",  "size": 2365792,   "inode": 407529117, "nlink": 1, "age_mtime_s": 2652.9, "age_ctime_s": 2652.9}
{"t": 1074.073, "path": "…/deps/cs-6328c84296c392b8",                      "size": 124842640, "inode": 407529148, "nlink": 1, "age_mtime_s": 2650.6, "age_ctime_s": 2650.6}
{"t": 1074.073, "path": "…/deps/cs_inspect-8a7a51aff306b943",              "size": 13444872,  "inode": 407529089, "nlink": 1, "age_mtime_s": 2654.7, "age_ctime_s": 2654.7}
{"t": 1074.073, "path": "…/deps/cs_inspect-c0cbf818ae044ebc",              "size": 1025384,   "inode": 407529078, "nlink": 1, "age_mtime_s": 2655.0, "age_ctime_s": 2655.0}
```

`age_mtime_s == age_ctime_s` on every one: these files had not been touched
since they were linked 44 minutes earlier. They are not being rewritten, they
are being reaped.

### The same burst, one run dying and one run surviving

That attributed burst is also a controlled experiment, because it deleted the
**same nine files** that killed the `baseline` run — including
`accept_doclib_conflict-5f1a24873210ff70`, the exact binary the `baseline`
occurrence died on.

| Run | Exec'd the doomed files | Prune deleted them | Result |
| --- | --- | --- | --- |
| `baseline` (06:21:42Z) | not yet — died on the first one | t=65.4 s into the run | **exit 101**, `never executed` |
| `t617-8` (07:20:51Z) | yes, 45-60 s into the run | 60 s after that | **exit 0**, 349 `test result: ok` |

`t617-8` scheduled all seven still-relevant victims in its plan and ran them
*before* the prune reached them:

```
  44.02  09:21:36      Running tests/accept_doclib_conflict.rs (…/accept_doclib_conflict-5f1a24873210ff70)
  45.97  09:21:37      Running tests/accept_f48_d_crash_recovery_matrix.rs (…-511240aab3de1191)
  58.37  09:21:50      Running unittests src/main.rs (…/cs_inspect-c0cbf818ae044ebc)
  58.37  09:21:50      Running tests/accept_f00_a_cli_smoke.rs (…-74aedd76b562eae5)
  58.76  09:21:50      Running tests/accept_f00_b_cli_help.rs (…-2e3fd5c40f27f019)
```

So the failure is decided entirely by **whether the run reaches a doomed
harness before the prune does**, with nothing else differing. Two runs of the
identical command on the identical tree, one dead and one green, one minute of
scheduling apart. That is the whole mechanism, and it is why the rate looks
random from the inside.

### The cadence

The two spontaneous occurrences above line up with the prune's own 1200 s
sleep to the second, and a fourth burst lands on it again with no run of mine
anywhere near:

| Event | UTC | Gap |
| --- | --- | --- |
| occurrence 1, unlink burst during the `baseline` run | 06:22:47 | — |
| occurrence 2, unlink burst during `run-2` | 06:42:50 | 20 min 03 s |
| pass caught by the sampler, no test run in flight | 07:02:50 | 20 min 00 s |
| attributed pass, `t617-8` in flight | 07:22:51 | 20 min 01 s |

Four bursts on a 20-minute period. This is the same process the F54-X7 finding
named as a candidate from its log line
(`bunny-alpha-1/accept_f35_a_capital_boundary-…: stat: No such file or
directory`). The candidate is now a measurement, with the command line.

**This is the owner's own maintenance job, outside this repository.** No file
in this workspace removes another executable from `target/debug/deps`; see the
ruled-out table.

## Why no in-repo fix exists, and why `test = false` cannot become one

`test = false` on a crate with real unit tests would delete real tests from the
gate, which is exactly what AGENTS.md rule 6 forbids. It is therefore not
available for the eight crates that carry unit tests:

| Crate | `#[test]`s in `src/` |
| --- | --- |
| `cs_content` | 251 |
| `cs_sim` | 127 |
| `cs_inspect` | 92 |
| `cs_app` | 85 |
| `cs_formats` | 36 |
| `cs_assets` | 24 |
| `cs_types` | 20 |
| `cs_script`, `cs_net`, `cs_xtask` | 0 |

And it would not have helped anyway, for a reason this task measured. Look at
what the prune deleted in the `run-2` burst:

```
{"event": "removed", "t": 80.994, "name": "accept_doclib_conflict-9d2bc9e4b069a8ca", "last_size": 1230632,  "last_mtime_age_s": 3146.6}
{"event": "removed", "t": 80.994, "name": "accept_f00_a_cli_smoke-46764440be203578", "last_size": 1119688,  "last_mtime_age_s": 3130.6}
{"event": "removed", "t": 80.994, "name": "accept_f00_b_cli_help-c6295c766924c9a4",     "last_size": 104985688, "last_mtime_age_s": 3106.6}
{"event": "removed", "t": 80.994, "name": "accept_f48_d_crash_recovery_matrix-be042b1c8f1a3c3e", "last_size": 106582448, "last_mtime_age_s": 3096.6}
{"event": "removed", "t": 80.994, "name": "cs-095145a1c96deb4d",                       "last_size": 104713856, "last_mtime_age_s": 3127.6}
{"event": "removed", "t": 83.023, "name": "cs_inspect-e4bc649912dd2984",               "last_size": 14910680,  "last_mtime_age_s": 3202.6}
```

**Six of the six were in that run's own plan**, including three integration
suites and the `cs_inspect --lib` harness. The #610 fix removes
`cs_xtask`'s two empty harnesses from the plan, which makes `cs_xtask` immune
to being *named* — it left `cs_inspect --lib` named here. The number of
harnesses in the plan is not the exposure; the directory is. A prune that
deletes every executable it finds in `target/debug/deps` will find ~360 of them
whatever the plan contains.

The one thing `test = false` genuinely bought was fewer *victims*, which is a
real effect — but a smaller number of dice, not a different game.

### The exposed surface, measured from a green run's own plan

Counting the `Running` lines of one green `cargo test --workspace --locked`
run gives the exact population the prune is drawing from. That run scheduled
**339 test units**, of which **11 were `--lib`/`--bin` unit-test harnesses**:

| Harness | `#[test]`s in that crate's `src/` |
| --- | --- |
| `cs_app` | 85 |
| `cs_assets` | 24 |
| `cs_content` | 251 |
| `cs_formats` | 36 |
| `cs_inspect` (`--lib` and `--bin`, two harnesses) | 92 |
| `cs_sim` | 127 |
| `cs_types` | 20 |
| `cs_net` | 0 |
| `cs_script` | 0 |
| `cs` (`cs_app`'s `--bin`) | — (same crate as `cs_app`) |
| `cs_xtask` | 0, and #610 removed it from the plan entirely |

So after #610 the plan still carries 9 unit-test harnesses from the 7 crates
that have real unit tests in `src/`, plus 2 from the 2 that have none. Every
one of the 9 real-test crates carries at least 20 `#[test]`s, so
`test = false` is unavailable on all of them, and `cs_net`/`cs_script` are the
only members where it would even be legitimate — and they hold no tests to lose.

**This is the scope fact #610 could not reach, stated exactly:** the
`test = false` fix is available on 2 of 10 members and covers 0 real tests. The
other 8 members, holding 635 `#[test]`s between them, stay in the plan and stay
exposed.

## Ruled out here, with the check that rules each out

| Hypothesis | How it was ruled out |
| --- | --- |
| A second cargo writing this target dir | No second cargo exists in it. `lsof` over the whole tree during both occurrences returned nothing, and the only `cargo` processes on the host had their cwd in `bunny-2` and `swe2-max-1`, whose `CARGO_TARGET_DIR` is their own checkout. Every agent's `.env` sets its own target dir. |
| A test in this suite deletes an artifact of the real target dir | Every `remove_file` / `remove_dir_all` under `crates/**` and `tools/**` was read. Each is scoped to a fixture root, a `TempDir`, or a scratch directory the same test created, and since #610 each fixture root carries a `std::process::id()` component so two suite processes cannot delete each other's trees. None names `target/debug/deps` or anything above it. |
| Cargo itself deleted them | Cargo's own churn is unlink-then-rewrite: the F54-X7 measurement saw 46 harnesses unlinked during a build phase and all 46 present afterwards. Here, zero of the 15 removed files across the two bursts came back over six further minutes of watching. Cargo does not delete an artifact it is about to execute. |
| A test in the suite runs `cargo clean` | No `cargo clean`, and no clean of any kind, is invoked from any Rust or Python source in the repository. |
| Disk pressure caused it | `/System/Volumes/Data` was at 97 % capacity with 34 GiB free throughout. Low free space is a real constraint on this host, but it cannot produce `ENOENT` on a path that existed a moment earlier: the unlink is observed directly, and the file is absent afterwards. |
| The failures are a real test failure being misread | No `test result: FAILED` line appears in either occurrence's log, and `test_select::classify_missing_harness` requires exactly that — a cargo failure, zero reported test failures, and cargo's own message — before it will call it a vanished harness. |
| `test = false` on the empty harnesses would have prevented it | Directly contradicted by the `run-2` burst: six of six victims were in the plan, and `cs_xtask` was not among them. |

## Rate

Measured over eight instrumented `cargo test --workspace --locked` runs on an
unmodified `origin/main` checkout, all in the same two-hour window, with
`loadavg` 80-111:

| Runs | Occurrences | Rate |
| --- | --- | --- |
| 8 | 2 | 2 in 8 (25 %) |

The rate is not a property of the code; it is the product of two host
quantities — how long the execution phase lasts, and where in that phase the
prune's next burst lands. On this machine the execution phase of a warm full
workspace run is roughly 60-150 s and the prune fires every 1200 s, so a run
that overlaps a prune burst has a substantial chance of dying on the victim it
has not reached yet. That is the number the owner needs in order to decide.

## What was *not* established

* **The prune's selection rule.** The victims in both bursts are ordinary
  executables spread across the directory, and their mtimes (33-53 min old) do
  not separate them from the survivors (the oldest surviving executable in
  `target/debug/deps` is 89 hours old). So the rule is not "oldest first" and
  not "older than N minutes". It reads the script's log or the script itself to
  know; that is owner-side.
* **Whether the prune intends to be safe here.** The F54-X7 finding quotes its
  own log line failing on a file that had just been replaced underneath it, so
  the job already races live builds. Whether it is *supposed* to skip live
  target directories is a question for the owner, not something this repository
  can answer.
* **Anything about the original game.** No original data was read.

## Recommendation

This belongs to #613 (F54-X8), which is filed and blocked, and this task adds
the reproduction and the attribution it needs. In preference order:

1. **Make the prune skip live target directories**, or skip directories whose
   harnesses a running `cargo test` is executing. That removes the class
   outright and costs nothing else. It is the only option that does not trade
   away something.
2. **Serialise the agents** if the prune cannot be changed. This removes the
   concurrency that makes the window reachable at all, at the cost of wall
   clock on a host that is already at load 80-110.
3. **Free disk.** `/System/Volumes/Data` at 97 % / 34 GiB free is a real
   problem for builds in its own right, but it is not the cause of *this*
   failure and should not be recorded as the fix.
4. **Accept the flake and document it.** Then the rerun rule in AGENTS.md and
   `cs_xtask::test_select` is the whole mitigation, and it is already
   implemented: rerun the identical command once, report both runs in order,
   never the rerun alone.

What this repository should *not* do: set `test = false` anywhere else. It
would delete real tests from the gate, and the measurement above shows it
would not remove the failure.

## The scripts

Committed under `docs/findings/scripts/`, named for the task so they can be
re-run by anyone who picks this up:

```sh
# one instrumented gate run, printing its own summary
sh docs/findings/scripts/2026-10-04-t617-run-workspace-test.sh mylabel

# attribute a deletion burst: run the gate repeatedly with the process sampler
# and the tree watcher running for the whole window
sh docs/findings/scripts/2026-10-04-t617-decisive.sh 1 14 0.4 0.4
```

The scripts write their output to `$TMPDIR/t617/`, which is outside the
repository. They were run from this checkout with `CARGO_TARGET_DIR` pointing
at its `target/`.

### Reading the sampler's output

`sample_procs.py` writes one JSON object per line: `{"event": "new", …}` for a
pid seen for the first time, and `{"event": "table", …}` with the full process
list every 30 s. To find the prune passes:

```sh
python3 - <<'PY'
import json
parents = {66027, 87266}          # the long-lived disk-prune-loop shells
for line in open("/tmp/t617/t617.procs.jsonl"):
    event = json.loads(line)
    if event["event"] == "new" and event["ppid"] in parents:
        print(event["t"], event["pid"], event["ppid"], event["args"])
PY
```
