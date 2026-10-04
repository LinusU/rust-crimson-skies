# F54-X7: what `could not execute process … (never executed)` means, and what removes a test harness on this host

Date: 2026-10-04. Task: #608 F54-X7 "cargo test --workspace intermittently
fails with 'could not execute process ... (never executed)'", opened by
bunny-2 from three occurrences while reviewing #600 on
`rally/600-make-the-cs-net-retry-test-deterministic`. Capabilities used:
ordinary build/test only. No original data, no retail capability, no human
play. This is a finding about the project's own test gate on one
multi-agent machine; it says nothing about the original game.

## The observation

Three times in one session, on three unrelated crates:

```
     Running unittests src/main.rs (target/debug/deps/cs_inspect-c0cbf818ae044ebc)
error: test failed, to rerun pass `-p cs_inspect --bin cs-inspect`

Caused by:
  could not execute process `…/target/debug/deps/cs_inspect-c0cbf818ae044ebc` (never executed)

Caused by:
  No such file or directory (os error 2)
```

No test failed and nothing panicked. The immediate rerun of the same command
was green (342 `test result: ok` lines), and the same head passes CI. It also
hit `cs_xtask` (unit tests of `src/main.rs`, twice) and `cs_formats`
(`--test accept_doclib_conflict`), and it hit a
`-- <prefix> --include-ignored` selection as well, not only the plain
workspace run.

## What the message means (measured here)

`Running <unit> (<path>)` is printed by cargo *before* it spawns the harness.
`(never executed)` and `ENOENT` together say the `exec` of that path failed
because the file was not on disk. Consequences an agent can rely on:

* **No test in that unit ran, so the run says nothing about the code under
  test.** There is no assertion to interpret, no partial credit, and no way to
  read this as a regression. The exit status came from cargo, not from a test.
* **Cargo is fail-fast here.** The first unit it cannot execute ends the run,
  which is why exactly one crate is named per occurrence and the remaining
  ~430 harnesses are never reported.
* **A harness that is already missing when cargo starts is not this error.**
  Cargo's freshness check notices a missing executable and rebuilds it.
  Measured on this machine, same command twice, only the file removed in
  between:

  | Step | Result |
  | --- | --- |
  | `cargo test --workspace --locked --bin cs-inspect` | green, harness built |
  | `rm target/debug/deps/cs_inspect-c0cbf818ae044ebc` | — |
  | `cargo test --workspace --locked --bin cs-inspect` | `Compiling cs_inspect` … `Finished in 0.35s`, then green |

  So the disappearance lands **after** the build phase, while cargo is
  executing harnesses. For this workspace that is not a microsecond window:
  a few hundred harness executables sit in `target/debug/deps` (432 at the time
  of the baseline run below; the count moves as configurations come and go),
  cargo runs several at a time, and the execution phase of a full workspace run
  on this machine lasts minutes. Anything that removes one of those files during that window
  produces this error, in whatever crate happens to be pending next — which is
  exactly the reported pattern (three different crates, one per run).

## What removes a harness executable here

Measured with a 0.4 s unlink watcher (`os.scandir` polling, the only kind
available without root) over `target/debug/deps` during one full
`cargo test --workspace --locked` on this machine, whose build phase rebuilt
stale harnesses:

* **46 harness executables were unlinked and rewritten during that one build
  phase**, and none of them was missing afterwards. Unlink-then-write of the
  previous output is what cargo/rustc do when they replace an artifact. It is
  harmless inside one invocation (the build phase is over before anything is
  executed) and fatal for any *other* consumer of the same directory at that
  moment: exec of a name that was just unlinked is `ENOENT`.
* The same watcher's other removals in that window were rustc's own
  `rustcXXXXXX` temporary directories (178 of them), which never come back and
  are not artifacts.

Ruled out here, with the check that rules each out:

| Hypothesis | How it was ruled out |
| --- | --- |
| Agents share one target dir | Every agent's `.env` sets its own `CARGO_TARGET_DIR`; `lsof` showed no foreign cargo holding this one during the run. The three nested-`cargo` helpers in the suite (`accept_t383`, `accept_t433`, `accept_t440`) each force `CARGO_TARGET_DIR` to a fixture path under `target/t<task>-target-dir-fixtures`, so no test writes into the real target dir. |
| Cargo does not hold the target-dir lock while running tests | It does not: those three tests run a nested `cargo run` during the suite and do not deadlock. So a second cargo in the same directory *can* start rebuilding while the first is executing harnesses — the window above is real, and it is the one shape of concurrent cargo that turns into this error. |
| A test in this suite deletes an artifact of the real target dir | Every `remove_dir_all`/`remove_file` in `crates/**/tests` and `tools/**/tests` was read: each is scoped to a fixture root or to a directory the same test created. The only ones that touch the checkout's `target/` create and remove their own `<name>_<pid>` subdirectory. |
| A stale fingerprint database makes cargo skip a rebuild | The target dir holds 10 fingerprint directories whose executable does not exist. All 10 are metadata-only configurations (`lib<name>-<hash>.rmeta` present, a different metadata hash): `cargo check`/`clippy` builds of the same targets. Cargo runs them, not the metadata. And the measurement above shows cargo rebuilding a genuinely missing executable. |
| Disk full, socket reuse | Reported by the reporter and consistent with what is visible (35 GiB free at 97 % capacity, 11 cores, load average 85-95 from the other agents' builds); neither can produce `ENOENT` on an existing path. Not re-measured by this task. |

The one process on this host that has been observed deleting **individual
files** out of an agent's target directory is the owner's `disk-prune`
maintenance step, which runs every 20 minutes and reports per agent
("pruned old test binaries: bunny-alpha-1: N GiB deleted"). It has already
lost a race with a live build: its own log line at 2026-10-03 23:42 is
`bunny-alpha-1/accept_f35_a_capital_boundary-1a32ff0eae7d09f8: stat: No such
file or directory`, a harness name that had just been replaced underneath it.

**That is a candidate, not a proven attribution.** No fs_usage/fsevents
attribution is available to an agent, the prune window is 20 minutes, and none
of the three reported occurrences happened during this session, so this task did
not observe the deletion itself. Filed as an owner-side task with the evidence
above; it is not needed for the fix below, which is about what the message
*means*.

## Reproduction

Deterministic, one variable: harness executables disappear from
`target/debug/deps` while a normal selection run is in flight. Nothing else
changes — no source, no flags, no environment.

```sh
# A filter no test matches: cargo still executes every harness, but each one
# exits at once, so the run stays in the execution phase without being long.
cargo test --workspace --locked -- accept_f54_x7_deterministic_probe_ --include-ignored &
# ... delete harness-shaped files from target/debug/deps while that runs ...
wait
```

Re-verified on this branch during review with the same one variable, on a
single crate so the window is easy to hit (`cs_app`, 9.5 s, 4 deletions):

```
$ cargo test -p cs_app --locked -- accept_f54_x7_probe_ &
     Running unittests src/lib.rs (target/debug/deps/cs_app-d95888fab103ceeb)
error: test failed, to rerun pass `-p cs_app --lib`

Caused by:
  could not execute process `.../target/debug/deps/cs_app-d95888fab103ceeb accept_f54_x7_probe_` (never executed)

Caused by:
  No such file or directory (os error 2)

exit 101, one `Running` line, 0 `test result:` lines.
```

The run fails with the reported error and stops there:

```
   Compiling cs_app v0.0.0 (/workspace/crates/cs_app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 21.61s
     Running unittests src/lib.rs (target/debug/deps/cs_app-d95888fab103ceeb)
error: test failed, to rerun pass `-p cs_app --lib`

Caused by:
  could not execute process `/…/target/debug/deps/cs_app-d95888fab103ceeb accept_f54_x7_deterministic_probe_ --include-ignored --color never` (never executed)

Caused by:
  No such file or directory (os error 2)
```

exit 101, one `Running` line, no `test result:` line at all. Two details in that
log are worth keeping:

* **The build phase absorbed the deletions that happened before it.** Four
  crates were recompiled (`Finished … in 21.61s`) because their harnesses were
  already gone when cargo checked them. That is cargo's missing-output rebuild
  doing its job, and it is why the same deletion is harmless before the build
  phase and fatal after it.
* **Cargo prints the whole command, not just the path.** With a filter, the
  text between `could not execute process` and `(never executed)` is the
  harness path followed by the test arguments. `cs_xtask` therefore reports
  that command verbatim rather than guessing where the path ends.

Spontaneous rate: **0 in the two full runs measured here**, against 3 in one
session reported on #600, plus further runs recorded in the T496 finding of
2026-10-03 ("several runs aborted with `error: test failed, to rerun pass -p
cs_xtask`"). The T496 note does not give a count, so no total is claimed here.
It is rare, it recurs, and it has never been seen on a fresh CI runner — which
fits a host-specific deleter of files in a long-lived target directory rather
than anything in the code under test.

## Measurements

Machine: 11 cores, load average 80-95 throughout (other agents building), APFS
at 97 % capacity with 35 GiB free, cargo 1.98.1, worktree `bunny-alpha-1` with
its own `CARGO_TARGET_DIR`.

| Run | Command | Exit | Time | `test result: ok` | `never executed` |
| --- | --- | --- | --- | --- | --- |
| Baseline (`origin/main` 4a8844bc, unmodified) | `cargo test --workspace --locked` | 0 | 490 s | 346 | 0 |
| After this task's change | `cargo test --workspace --locked` | 0 | 76 s | 347 | 0 |
| Deterministic reproduction | `cargo test --workspace --locked -- <filter>`, harnesses deleted in flight | 101 | — | 0 | 1 |
| Reviewer's re-verification, whole workspace | `cargo test --workspace --locked` | 0 | ~240 s | 349 | 0 |
| Reviewer's re-verification, single crate | `cargo test -p cs_app --locked -- <filter>`, harnesses deleted in flight | 101 | 9.5 s | 0 | 1 |

The reproduction deleted 371 distinct harness-shaped names and the run died on
the first one it had not yet executed. 30 of those 371 are still absent, and
the following `cargo test --workspace --locked` rebuilt only the two crates it
needed (`cs_xtask`, changed by this task, and `cs_formats`): the rest were
artifacts of other configurations (other feature sets, older commits) that the
workspace run does not use.

Unlink watch (0.4 s polling of `target/debug/deps` during the baseline run,
which rebuilt 46 stale harnesses):

| Measurement | Value |
| --- | --- |
| Distinct harness executables unlinked during the build phase | 46 |
| Of those, still missing once the run was over | 0 |
| rustc temporary directories (`rustcXXXXXX`) unlinked in the same window | 178 |
| Harness-shaped names present in `target/debug/deps` afterwards | 432 |

Sensitivity of this task's fix — `cs_xtask::test_select`'s classification
replaced by `None`, tests re-run:

| Test | Result |
| --- | --- |
| `accept_f54_x7_a_run_whose_harness_vanished_is_reported_as_a_missing_harness` | FAILED |
| `accept_f54_x7_the_diagnosis_says_rerun_once_and_never_call_the_failed_run_green` | FAILED |
| `accept_f54_x7_the_diagnosis_keeps_the_arguments_cargo_appended_to_the_path` | FAILED |
| the three guard tests (precedence, green run, compile error) | passed, as they must |

The same experiment after review added the `ENOENT` discrimination below: with
`harness_fault` reporting every exec failure as absent, exactly the two tests
that pin that discrimination fail
(`accept_f54_x7_only_enoent_is_called_a_vanished_harness`,
`accept_f54_x7_the_diagnosis_never_claims_a_deletion_cargo_did_not_report`); the
six guard tests still pass.

## What changed

The occurrence cannot be eliminated from inside the repository — the file is
removed by something outside the tree. What *can* be removed is the ambiguity,
which is what this task's acceptance criteria ask for when a cause has no
in-repo fix:

* `cs_xtask::test_select` now classifies the message as
  `SelectError::HarnessMissing` instead of letting it fall into the generic
  `SelectError::CargoFailed`. The error names the executable cargo could not
  execute, says in one sentence that no test ran and that this is not a test
  failure, and says the only thing that decides anything is a rerun of the
  identical command — with the instruction to report **both** runs and never the
  rerun alone. That is the "rerun once, never 'the run was green'" rule, in the
  runner every implementing and reviewing agent already uses for its task gate.
* Three conditions are all required, so the classification cannot absorb real
  problems: cargo must have failed, no test may have reported a failure, and
  the log must carry cargo's exact message. A failing test still gets its own,
  more precise error, and a compile error still gets the generic one.
* **The cause is quoted, not assumed.** `could not execute process … (never
  executed)` is printed for *every* failed `exec`; only the OS error on the next
  line says which. `HarnessFault` therefore distinguishes `Absent` (only
  `No such file or directory (os error 2)`) from `OsError(..)` (e.g.
  `Permission denied (os error 13)`, `Exec format error (os error 8)`) and
  `Unattributed` (cargo printed no recognisable OS error). A permission or
  format failure is *not* this finding's vanished-harness class — the file may be
  there and unrunnable — so the message says so and says a repeat of that is a
  real fault to investigate, instead of telling the agent to rerun and hope.
* No test was weakened, skipped or made tolerant, and no assertion anywhere was
  changed. `accept_f54_x7_` in
  `tools/cs_xtask/tests/accept_f54_x7_missing_harness_binary.rs` calls the
  production functions directly; the sensitivity table above shows what fails
  when the classification is removed.

## Checks run on the change

```sh
cargo fmt --all -- --check                                                       # 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings  # 0
cargo test --workspace --locked                              # 0: 3372 passed, 0 failed, 0 never-executed
cargo test --workspace --locked -- accept_f54_x7_ --include-ignored             # 0: 8 passed
```

Re-run on the reviewer's tree after the `ENOENT` discrimination above; the
counts are the whole workspace, not this task's six tests.

## What an agent does when it sees this error

1. Rerun the **identical** command once.
2. If the rerun is green: the rerun is the run of record, and the first run is
   still a failed run. Report both, in that order. Do not write "the suite
   passed" on the strength of the rerun.
3. Never change a test, an assertion or a skip to make this message go away. It
   is not a test result, so there is nothing to fix in the tests.
4. **Read the OS error before believing the class.** This finding is about
   `No such file or directory (os error 2)`. If the same message arrives with
   `Permission denied` or `Exec format error`, the harness was there and is not
   runnable, which is a different problem: rerun once, and if it repeats,
   investigate it as a real fault (`ls -l` and `file` on the named harness)
   rather than filing it against this finding.
5. If it recurs, attach cargo's exact error plus
   `ls target/debug/deps | wc -l` before and after the run and file it against
   this finding. A recurrence that names a *different* harness each time is the
   signature of the external deleter above; a recurrence that always names the
   same harness is a real problem and must be investigated as one.

## Residual limits (not this task)

* The deleter behind the three reported occurrences is **not identified**. What
  is proven here is the mechanism (an accepted-fresh harness that is gone at
  exec time), the window (the whole test-execution phase, minutes long) and the
  two candidate classes (an external prune of old test binaries, task #613
  F54-X8; a second cargo rebuilding the same target directory). Both are
  owner-side and filed as separate tasks.
* The plain `cargo test --workspace --locked` gate prints cargo's own wording,
  which only `cs_xtask test-select` now classifies. `AGENTS.md` is a protected
  path, so the same one-sentence rule belongs there too; the owner has to add it
  (task #614 F54-X9).
* The rate is too low to measure from one session: 0 spontaneous occurrences in
  the two full runs listed above is consistent with anything from "fixed" to "a
  few per agent-day". A claim that the class is gone cannot be made from this
  data, and nothing here claims it.
* Attribution of a deletion needs `fs_usage` or an FSEvents tap, neither of
  which an agent may run here. Without root or a watcher on the pruning job
  itself, "which process removed it" stays open even though "what the message
  means" does not.