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

Five results:

1. **The reported signature reproduces spontaneously** — `could not execute
   process … (never executed)` / `No such file or directory (os error 2)` — in
   ordinary `cargo test --workspace --locked` runs on an unmodified
   `origin/main` checkout, without any second cargo, without any test failure,
   and while the deleting pass is provably not cargo.
2. **The deleting command was captured in the act**, with its arguments:
   `prune-stale-bins.sh --delete …/bunny-alpha-2/target/debug/deps …`, a
   0.5 s child of the owner's `disk-prune-loop.sh`, seen holding that directory
   in its argv 0.19 s before nine executables inside it were observed gone.
   Bursts recur on the loop's own 1200 s period — four of them, 20 min apart,
   to the second. The unlink syscalls themselves were not traced; see the
   attribution section for exactly what is observed and what is inferred.
3. **Whether a run dies is decided by one thing only: whether it reaches a
   doomed harness before the prune does.** Two runs of the identical command on
   the identical tree, where the prune deleted the *same nine file names*, one
   died and one passed, differing only in scheduling order.
4. **Every crate is exposed, `test = false` cannot help, and this workspace's
   own test suite cannot cause it.** A harness of `cs_inspect` — a crate with
   92 real `#[test]`s in `src/` — was the victim in one measured occurrence, so
   the #610 fix does not generalise and, more importantly, would not have
   prevented this occurrence even if it had.
5. **The measured rate is a floor.** Only the `ENOENT` form was counted. The
   same class also kills a harness outright when the file is replaced rather
   than removed, and nothing here looked for that.

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
| `2026-10-04-t617-timestamper.py` | the line-timestamp filter the run script pipes cargo's output through |
| `2026-10-04-t617-decisive.sh` | the campaign driver: runs the gate N times with the sampler and the tree watcher up for the whole window |

The sampler is the attribution instrument. `fs_usage`, `dtrace` and
`sudo`-gated tools are unavailable to an agent on this host, and FSEvents
reports no pid, so attribution has to come from sampling the process table: a
prune pass is not a daemon, it exists only while it deletes.

## Reproduction: spontaneous, on an unmodified tree

Four occurrences, all from `cargo test --workspace --locked` on
`origin/main` (`2676355d`) and on this branch, with this checkout's own
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

**Occurrences 3 and 4** — this task's own required `cargo test --workspace
--locked` checks, both on this branch. Each died on a harness after hundreds of
tests had already passed, and each was green on the immediate rerun of the
identical command:

| # | Victim | Build phase | Tests passed before dying | Rerun |
| --- | --- | --- | --- | --- |
| 3 | `accept_f48_d_crash_recovery_matrix-be042b1c8f1a3c3e` | 1.88 s | 553 | exit 0, 349 groups, 3391 passed |
| 4 | `accept_doclib_conflict-d84789a84d5b4f8d` | 3 m 13 s | 2018 | exit 0, 352 groups, 3412 passed |

Both victims are binaries a prune burst had already deleted once earlier in
this session — occurrence 3's is in `run-2`'s burst, occurrence 4's in
`baseline`'s — and both died during the execution phase, after the build phase
had already accepted every harness as fresh. Occurrence 3's build phase took
**1.88 s** (cargo rebuilt nothing at all); occurrence 4's took **3 m 13 s**,
because the rebase brought in main's newer `capital` work. That difference is
immaterial: in both cases the deletion fell after the freshness check, which is
the only thing the mechanism requires. Both runs are reported in the Rally
handover in order, the failed one first.

Occurrence 4 is worth one extra note: it died on
`accept_doclib_conflict-d84789a84d5b4f8d`, which occurrence 1's burst had also
deleted. That test target currently exists in the directory as **four**
hashes (`-3bad038241d20373`, `-5f1a24873210ff70`, `-9d2bc9e4b069a8ca`,
`-d84789a84d5b4f8d`) — and a green run of the current plan schedules **all
four**, so these are not stale leftovers of an older commit: one test target is
genuinely built into four configurations, and all four are live in every run.
Which configurations produce them (feature sets, profiles, doctest variants)
was not determined and is not this finding's subject. It is recorded because
it means the exposure is *larger* than a one-harness-per-target count
suggests: a single target can put four separate binaries in front of the
prune.

`cs_inspect` carries 92 `#[test]`s in `src/` (11 modules, `campaign.rs` 2,
`catalog.rs` 12, `config.rs` 18, `handling.rs` 3, `interp.rs` 14, `resolve.rs`
5, `rof.rs` 9, `routes.rs` 3, `script_discovery.rs` 10, `textures.rs` 8,
`zbd.rs` 8), so `test = false` is not available for it and, as measured below,
would not have helped.

## The unlink is observed directly, not inferred

`watch-deps.py` records every executable that disappears from
`target/debug/deps`. The first two occurrences above were each preceded by an
unlink burst, in the same run, while cargo was executing harnesses:

| Run | Removals | Of those, named by a `Running` line of that run | Died? |
| --- | --- | --- | --- |
| `baseline` (06:21:42Z) | 9 | 1 | yes, on the one it named |
| `run-2` (06:41:29Z) | 6 | 6 | yes |
| runs 1, 3, 4, 5, 6, 7 | 0 | — | no |

The middle column is a name match against the run's own `Running` lines, and
that match is over-strict in one direction and therefore a floor, not a count
of exposed binaries: cargo names a test unit by a hash-suffixed path, so an
executable the sweep took that this run's plan did not name is a build of that
target from some other revision, not evidence that the sweep avoids scheduled
work. Read it as "how many of these were provably in this run's plan", never as
"how many were test binaries". The `1` here and the `7` quoted further down for
`t617-8` are not in conflict and must not be read as one number: the two runs
are a *different revision's* plan. `main` moved from `2676355d` to `b2088566`
between them, over `crates/{cs_app,cs_content,cs_net,cs_sim}`, which changes
the hashed paths cargo schedules. The nine names the sweep took span both
revisions' builds, which is why one run can name one of them and the next run
seven.

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
* **The sweep is over the directory, not over what the run needs.** It took the
  119 MiB `cs` application binary, which `cargo test` never execs at all, in
  the same burst as three `accept_*` harnesses and two `cs_inspect` ones. A
  prune that knew which binaries the run was about to need would not have taken
  `cs`. Which of its victims the run reaches next is therefore chance, and the
  `run-2` burst is the other side of that coin: all 6 of its victims *were*
  named by that run's plan, and all 6 were still pending.

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
line**, and the nine unlinks at t=1074.073 came 0.19 s later; the pass ended
0.5 s after that with the next `sleep 1200`.

Stated at the precision the instruments actually reach: what is *observed* is
one process holding that directory in its argv 0.19 s before nine of its
executables were observed absent, with no other candidate in the same window.
What is *inferred* is that the unlink syscalls came from that pid, because no
`unlink` was traced — `fs_usage` and `dtrace` need root, FSEvents reports no
pid, and a poller cannot see a syscall. Both timestamps come from 0.4 s
pollers, so the gap is 0.19 s ± one interval; a gap *smaller* than the sampling
interval still bounds the sampler as having seen the pass alive immediately
before the files went. That is weaker than a syscall trace and much stronger
than the "candidate, not proven" wording AGENTS.md uses for this class, which
is why this document states which of the two it is offering.

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

### The same class has a second symptom, already in the repository

This document counts only the `ENOENT` form, so **4 in 24 is a floor, not the
rate of the class.** `docs/findings/2026-10-04-t610-cs-xtask-tests-under-concurrent-cargo.md`
records the other one and it is quoted in `tools/cs_xtask/Cargo.toml:11-19`: when
the file that goes away under a harness is *replaced* rather than merely
removed, macOS SIGKILLs the already-running binary (its ad-hoc code signature
does not survive the relink) and the run dies without any `ENOENT` line at all.
The two symptoms therefore discriminate, and that is useful: `ENOENT` means the
path was unlinked and not written back, which is what the attributed burst
did. Every `ENOENT` occurrence here was found by looking for `ENOENT`; nothing
in this campaign looked for a killed harness, so any SIGKILL death among the
same 24 instrumented runs went uncounted.

That in-repo comment also states a **different cause** for the same signature —
"a second cargo writing the same target directory" — which is ruled out above
for every occurrence measured here. The two explanations are not reconciled in
the repository, so #620 was filed for that, since editing `tools/cs_xtask/` is
outside this task's owner paths.

### The same burst, one run dying and one run surviving

That attributed burst is also a controlled experiment, because it deleted the
**same nine file names** the `baseline` burst deleted an hour earlier —
including `accept_doclib_conflict-5f1a24873210ff70`, the exact binary the
`baseline` occurrence died on. (`baseline` died on that one; the other eight
were never reached by that run, so "the same nine" is a statement about the
sweep's input set, not about nine deaths.)

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
directory`). That candidate is now a named process, seen with its arguments
while it ran.

**This is the owner's own maintenance job, outside this repository.** No file
in this workspace removes another executable from `target/debug/deps`; see the
ruled-out table.

## Why no in-repo fix exists, and why `test = false` cannot become one

`test = false` on a crate with real unit tests would delete real tests from the
gate, which is exactly what AGENTS.md rule 6 forbids. It is therefore not
available for the seven crates that carry unit tests in `src/`:

| Crate | `#[test]`s in `src/` |
| --- | --- |
| `cs_content` | 251 |
| `cs_sim` | 127 |
| `cs_inspect` | 92 |
| `cs_app` | 85 |
| `cs_formats` | 36 |
| `cs_assets` | 24 |
| `cs_types` | 20 |
| **total** | **635** |
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

**All six were named by that run's own plan**, including three integration
suites and the `cs_inspect --lib` harness. The #610 fix removes
`cs_xtask`'s two empty harnesses from the plan, which makes `cs_xtask` immune
to being *named* — it left `cs_inspect --lib` named here. The number of
harnesses in the plan is not the exposure; the directory is. A prune that
deletes every executable it finds in `target/debug/deps` will find ~370 of them
whatever the plan contains — measured after the last run of this task, against
a 342-unit plan.

The one thing `test = false` genuinely bought was fewer *victims*, which is a
real effect — but a smaller number of dice, not a different game.

### The exposed surface, measured from a green run's own plan

Counting the `Running` lines of one green `cargo test --workspace --locked` run
gives the part of the directory the prune draws from that this repository can
control — a subset of what it actually sweeps, as the paragraph above says.
That run scheduled **342 test units** on the rebased tree, of which **11 were
`--lib`/`--bin` unit-test harnesses** — the same 11 before the rebase, on 339
units:

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

So after #610 the plan still carries 9 unit-test harnesses belonging to the 7
crates that have real unit tests in `src/` (`cs_inspect` contributing two),
plus 1 from `cs_script` and 1 from `cs_net`, which have none. Every one of
those 7 real-test crates carries at least 20 `#[test]`s, so `test = false` is
unavailable on all of them; `cs_net`/`cs_script` are the only members where it
would even be legitimate, and they hold no tests to lose.

**This is the scope fact #610 could not reach, stated exactly:** of the 10
workspace members, `test = false` is legitimate on 3 (`cs_net`, `cs_script`,
and the `cs_xtask` it was already applied to) and covers 0 real tests. The
other 7 members, holding 635 `#[test]`s between them, stay in the plan and stay
exposed.

## Ruled out here, with the check that rules each out

| Hypothesis | How it was ruled out |
| --- | --- |
| A second cargo writing this target dir | No second cargo exists in it. `lsof` over the whole tree returned only this task's own watcher and its own test run, and the only other `cargo` processes on the host had their cwd in `bunny-2` and `swe2-max-1`, whose `.env` sets `CARGO_TARGET_DIR` to their own checkout (checked for `bunny-2`, `swe2-max-1` and `bunny-alpha-1`: all three differ from this one). The attributed burst makes it moot anyway: the deleting command is not cargo. |
| Cargo's own relinking is being mistaken for the prune | It is distinguishable, and both were observed in the same log. A cargo relink has `age_ctime_s ≈ 0` against an old `age_mtime_s` — that is the `target/debug/cs` unlink at t=957.178 (`age_mtime_s: 2533.7`, `age_ctime_s: 130.5`), and the file was back 1.3 s later. Every prune victim has `age_mtime_s == age_ctime_s` and never came back. The t=1074.073 burst is nine files, all `age_mtime_s == age_ctime_s`, all absent afterwards. |
| A test in this suite deletes an artifact of the real target dir | Every `remove_file` / `remove_dir_all` under `crates/**` and `tools/**` was enumerated at `2676355d`: **165 call sites, 106 in `tests/` and 59 in `src/`** (the same 165 at this branch's merge base `b2088566` and at its head, so nothing moved underneath the count). One of the 165 lines names anything matching `target`, `debug`, `deps` or `CARGO_TARGET_DIR` — `accept_t440_live_foreign_target_dir.rs:287`, `fs::remove_dir_all(&registry_target)`, which removes `<fixture>/<name>-survivor/target-registry`, a directory the test created inside its own `fixtures_root()`. None of the 165 reaches `target/debug` or `target/debug/deps`; the rest are scoped to a fixture root, a `TempDir`, or a scratch directory the same test created, and since #610 each fixture root carries a `std::process::id()` component so two suite processes cannot delete each other's trees. The one scratch path that does live under the real `target/` is `target/t610-transient-fixtures/<pid>/`, which is not `deps/`. |
| Cargo itself deleted them | Ruled out three ways. (a) Cargo's own churn is unlink-then-rewrite: the F54-X7 measurement saw 46 harnesses unlinked during a build phase and all 46 present afterwards, and the one cargo relink observed here (`target/debug/cs`, t=957.178) was back 1.3 s later. (b) Not one of the 24 files removed across the three `deps/` bursts (9 + 6 + 9) came back over six further minutes of watching. (c) The t=1074.073 burst was captured with `prune-stale-bins.sh --delete` running; the other two were not, because the first campaign ran no process sampler, and they are attributed to the same loop on its 1200 s cadence rather than by direct observation. Cargo does not delete an artifact it is about to execute. |
| A test in the suite runs `cargo clean` | `cargo clean` appears in the repository only as text that is printed, never as a command that is run: the two format strings in `tools/cs_xtask/src/target_dir.rs:158,178` (the remediation the stale/foreign-dir gate puts in its error message) and the assertions on that text in `accept_t433_stale_target_dir.rs` and `accept_t440_live_foreign_target_dir.rs`. Every occurrence was read. No source file invokes it, no `Command` is built from it, and `--target-dir` appears nowhere in any test or source file except inside those strings. |
| Disk pressure caused it | `/System/Volumes/Data` was at 97 % capacity with 34 GiB free throughout. Low free space is a real constraint on this host, but it cannot produce `ENOENT` on a path that existed a moment earlier: the unlink is observed directly, and the file is absent afterwards. |
| The failures are a real test failure being misread | No `test result: FAILED` line appears in any occurrence's log — occurrences 3 and 4 reached 553 and 2018 passing tests respectively before dying. `test_select::classify_missing_harness` requires exactly this (a cargo failure, zero reported test failures, cargo's own message) before it will call it a vanished harness, and it did not fire here because these runs were not routed through `test-select`. |
| `test = false` on the empty harnesses would have prevented it | Directly contradicted by the `run-2` burst: all six of its victims were named by that run's plan, and `cs_xtask` was not among them. |

## Rate

Two campaigns plus this task's own check runs: **24 instrumented**
`cargo test --workspace --locked` runs, plus the 2 uninstrumented reruns AGENTS.md
requires after a failed one — **26 invocations of the command in total**. All on
unmodified `origin/main` (`2676355d`) or on this branch (which changes only
`docs/findings/`), all with this checkout's own `CARGO_TARGET_DIR`,
`loadavg` 80-111, `/System/Volumes/Data` at 97 % with 34 GiB free:

| Campaign | Instrumented runs | Failures |
| --- | --- | --- |
| first (per-run watchers only) | 8 | **2** |
| attributed (0.4 s process sampler + target-dir watcher) | 14 | 0 |
| this task's own required checks | 2 | **2** |
| total instrumented | **24** | **4** |
| reruns of the identical command after each failure | 2 (uninstrumented) | 0 |

The four failures are the four occurrences quoted above. The 14-run campaign
had a prune burst land *inside* a run (`t617-8`) and survived it, because that
run had already executed the doomed files — see the controlled comparison
above. So the rate is better read per burst than per run:

| Prune burst | A run in flight? | Outcome |
| --- | --- | --- |
| 06:22:47Z | yes | **exit 101**, died on a victim it had not reached |
| 06:42:50Z | yes | **exit 101**, died on a victim it had not reached |
| 07:02:50Z | no | nothing to kill |
| 07:22:51Z | yes (`t617-8`) | **exit 0**, 349 `test result: ok` — it had passed them |

**Three bursts had a run in flight; two of those runs died.** The
execution phase of a warm full workspace run on this machine is **117-149 s
(median 122 s)** — measured from the first `Running` line to the last
`test result: ok` across the 14 green runs. A prune fires every 1200 s and
takes ~0.5 s to sweep five checkouts' `deps/` directories.

The rate is therefore not a property of the code at all. It is the product of
two host quantities — a 122 s window in which a run can be killed, and a sweep
every 1200 s — and the position of the sweep within that window. A run that
starts just after a sweep has a 122 s window to finish before the next one; a
run that starts just before has almost none. That is exactly why this looks
random from the inside, and it is why a single green run is not evidence that
the class is gone.

## What was *not* established

* **The prune's selection rule.** This is the one open question, and the data
  narrows it without settling it. Recorded victim mtime ages, per burst:

  | Burst | Victim mtime ages | Executables that survived in the same directory |
  | --- | --- | --- |
  | baseline (06:22:47Z) | 1976-2053 s (33-34 min) | 3 min to 90 h old, 367 files |
  | run-2 (06:42:50Z) | 3097-3203 s (52-53 min) | same |
  | attributed (07:22:51Z) | 2651-2655 s (44 min, all within 5 s) | same |

  So the rule is **not** "oldest first": files 90 hours old survive every pass
  while files 33-53 minutes old are taken, and the 100+ MiB binaries survive
  while a 1.0 MiB one does not. Nor is it "anything older than N minutes".
  What the victims do share is a **narrow mtime window per burst** — all nine
  in the attributed burst were linked within 5 s of each other — rather than
  being spread across the directory's age range. Whether that window means
  "recently linked and not yet claimed by a finished run", or is a hash-prefix
  or name rule, is **unknown**. Reading `prune-stale-bins.sh` settles it, and
  that file is outside every checkout, so it is the owner's to read. Do not
  record a guessed rule.

  One claim this finding does **not** make: that a freshly linked harness is
  safe. An earlier reading of occurrence 3's 1.88 s build phase suggested the
  victim had just been relinked, which would have shown freshness is not the
  discriminator. That reading was wrong — a 1.88 s build phase means cargo
  rebuilt nothing, and the file's mtime is from the *green rerun*, hours after
  it was linked. The narrow-window observation above stands; a rule inferred
  from it does not. (Occurrence 4 does not rescue the claim either: its build
  phase was 3 m 13 s and its victim was still one the prune had taken hours
  earlier.)

* **Why four hashes of one test target coexist and are all live.**
  `accept_doclib_conflict` is present as `-3bad038241d20373`,
  `-5f1a24873210ff70`, `-9d2bc9e4b069a8ca` and `-d84789a84d5b4f8d`, and a
  green run of the current plan schedules all four. Cargo derives that hash
  from the unit's metadata, so these are four build configurations of one
  target — feature sets, profiles, or something else. Which, was not
  determined. Recorded because it means a single test target can put several
  distinct binaries in front of the prune, so the exposed population is larger
  than a one-harness-per-target count suggests; and because it means
  occurrence 4's victim was not an abandoned artifact but a binary the run was
  going to execute.
* **Whether the prune intends to be safe here.** The F54-X7 finding quotes its
  own log line failing on a file that had just been replaced underneath it, so
  the job already races live builds. Whether it is *supposed* to skip live
  target directories is a question for the owner, not something this repository
  can answer.
* **Anything about the original game.** No original data was read.

## What a reader cannot check without re-running the campaign

The campaign's raw output — `.env`, `.log`, `.deps.jsonl`, `.tree.jsonl`,
`.procs.jsonl` for every run — was written outside the repository and is not
committed. Every number above is therefore a *report*, not a checked-in
artifact, and it cannot be re-derived from this tree; it can only be re-derived
by re-running the campaign, which is what the scripts are for. Two numbers here
were independently re-checked by review against the tree instead, and are
called out because they are the ones a reader would most want to trust: the
`#[test]`-per-crate table (counted again on the branch head: 251/127/92/85/36/
24/20, `cs_net`/`cs_script`/`cs_xtask` at 0) and the
`remove_file`/`remove_dir_all` census (165, not the 163 first recorded — see the
ruled-out table). Anything else quoted here is the implementer's record of what
it saw on one machine on one day.

## Rule 6 status

**This task has no test prefix and this document satisfies criterion 1 alone**,
which the task description allows explicitly: *"If the only honest outcome is a
findings document, that alone satisfies criterion 1 — say so rather than
inventing a mechanism."* `cargo test --workspace --locked -- accept_t617_
--include-ignored` discovers **0 tests**, and that is the accurate report rather
than a gap to be papered over. Both ways of manufacturing a prefix were rejected
on the merits: a Rust test that only wraps a markdown file does not exercise
production code, and rule 6's Python-only carve-out needs the task description
to name a nonempty selection, which this one does not. #615 records that gap in
rule 6 itself; until it is closed, an authorized findings-only task cannot also
satisfy the prefix rule, and no agent should pretend otherwise.

## Recommendation

This belongs to #613 (F54-X8), which is filed and blocked, and this task adds
the reproduction and the attribution it needs. In preference order:

1. **Make the prune skip a `deps/` directory that has a live `cargo test`
   executing in it.** That removes the class outright and costs nothing else,
   and it is the only option that does not trade something away. The check is
   cheap: the pass already runs `stat` over the directory.
2. **Serialise the agents** if the prune cannot be changed. This removes the
   concurrency that makes the window reachable at all, at the cost of wall
   clock on a host that is already at load 80-110.
3. **Free disk.** `/System/Volumes/Data` at 97 % / 34 GiB free is a real
   problem for builds in its own right, and the prune is presumably a response
   to it. But it is not the cause of *this* failure and should not be recorded
   as the fix.
4. **Accept the flake and document it.** Then the rerun rule in AGENTS.md and
   `cs_xtask::test_select` is the whole mitigation, and it is already
   implemented: rerun the identical command once, report both runs in order,
   never the rerun alone. This is what happens today, and this task's own
   check run is an instance of it: occurrence 3 failed, the identical rerun was
   green, and both are reported above.

Two things this repository should **not** do:

* **Set `test = false` anywhere else.** It would delete real tests from the
  gate, and the measurement above shows it would not remove the failure.
* **Treat a green rerun as proof the class is gone.** The 14-run campaign had
  zero failures and still had a prune burst land inside a live run. The rate
  is a scheduling coincidence, so it will read as "fixed" at exactly the wrong
  moments.

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

The scripts write their output to `$TMPDIR/t617/` (override with `OUT=`), which
is outside the repository. They were run from this checkout with
`CARGO_TARGET_DIR` pointing at its `target/`, on macOS: they use `ps`, `lsof`,
`pgrep`, `df -k /System/Volumes/Data` and `sysctl -n vm.loadavg`, none of
which exist on Linux.

Six scripts are committed. Four of the six are instruments and were each
smoke-tested after being written: `timestamper.py` prefixes a line correctly,
`sample-procs.py` wrote 848 records in 2 s at a 0.2 s interval, and both
watchers (`watch-deps.py`, `watch-tree.py`) emit their `{"event": "start", …}`
record against a fixture directory containing one executable. The two shell
drivers were not separately smoke-tested; they are exercised by the campaigns
that produced every number above, which is the stronger test and is recorded
here rather than claimed as a unit check.

### Reading the sampler's output

`sample-procs.py` writes one JSON object per line: `{"event": "new", …}` for a
pid seen for the first time, and `{"event": "table", …}` with the full process
list every 30 s. To find the prune passes, filter for the long-lived
`disk-prune-loop` shells' children:

```sh
python3 - <<'PY'
import json, os
path = os.path.join(os.environ["TMPDIR"], "t617/t617.procs.jsonl")
for line in open(path):
    event = json.loads(line)
    if event["event"] == "new" and "prune-stale-bins" in event["args"]:
        print(event["t"], event["pid"], event["ppid"], event["args"])
PY
```

The path has to come from `$TMPDIR`: the scripts write to `${TMPDIR:-/tmp}/t617`,
and on macOS `$TMPDIR` is a per-user `…/T/` directory, so the literal
`/tmp/t617/…` opens nothing.

That filter is what produced the `prune-stale-bins.sh --delete` line quoted
above. `watch-tree.py`'s log lines carry the `path`, `inode`, `nlink` and the
mtime/ctime/atime ages, which is what distinguishes a prune reaping a file
(ages equal, untouched since link) from cargo replacing one (`age_ctime_s`
near zero while `age_mtime_s` is old — the `target/debug/cs` unlink at
t=957.178 in the same log is that second case, cargo's own relink, not the
prune).
