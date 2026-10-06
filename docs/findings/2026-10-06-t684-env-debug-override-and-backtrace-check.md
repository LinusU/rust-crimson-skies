# #684: the host's `CARGO_PROFILE_DEV_DEBUG=0` vs the T430 backtrace check

Date: 2026-10-06. Task: #684 (T430-B) "Stop the host's
`CARGO_PROFILE_DEV_DEBUG=0` from making accept_t430_a fail locally".
Capabilities used: ordinary build/test only, so no `acceptance.json` is
produced (`docs/contracts/CLI-EVIDENCE.md`). Machine: macOS aarch64, rustc
1.98.1, the shared multi-agent build host — same one as
`2026-10-04-t617-cargo-test-enoent-is-external.md` and
`2026-10-06-t691-macos-backtrace-line-tables.md`, which this finding should
be read after.

This is a finding about the project's own test gate on one host. It says
nothing about the original game.

## The trap

The shared agent environment exports `CARGO_PROFILE_DEV_DEBUG=0`
(`common.env`, a deliberate owner choice of 2026-10-05 so five agents' build
directories fit the disk). Cargo's environment overrides the committed
`[profile.dev] debug = "line-tables-only"`, and `test` inherits `dev`'s
resolved value, so every local `cargo test --workspace --locked` on this host
built `accept_t430_ci_disk_budget` with no line tables and failed exactly one
test: `accept_t430_a_panic_backtrace_names_the_file_and_line` ("no backtrace
frame named a line of accept_t430_ci_disk_budget.rs"). Reproduced verbatim
before this change:

```
$ CARGO_PROFILE_DEV_DEBUG=0 cargo test -p cs_xtask --test accept_t430_ci_disk_budget
…
test accept_t430_a_panic_backtrace_names_the_file_and_line ... FAILED
test result: FAILED. 12 passed; 1 failed; 0 ignored; …
```

CI was never affected: `.github/workflows/ci.yml` pins
`CARGO_PROFILE_DEV_DEBUG: line-tables-only` and
`CARGO_PROFILE_TEST_DEBUG: line-tables-only` on the `rust` job, so the
binaries it builds carry line tables and the artifact assertion is real
there. The failure only looked like a code defect locally — which is the
trap: an implementer under delivery pressure could "fix" it by weakening the
assertion or the committed profile, breaking the disk budget T430 exists to
protect (AGENTS.md rule 6 forbids answering it that way).

## Measured: how the env vars reach a test binary

The resolution `env_debug_level` implements was measured, not assumed:

| Environment | `accept_t430_a_…` result (pre-change binary) |
| --- | --- |
| `CARGO_PROFILE_DEV_DEBUG=0` | FAILED — helper frames carry no location |
| `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=line-tables-only` | ok — `test` resolves its own pin and does not inherit `dev`'s `0` |

So for a test binary the order is: `CARGO_PROFILE_TEST_DEBUG`, then a stated
`[profile.test] debug`, then `CARGO_PROFILE_DEV_DEBUG`, then `[profile.dev]
debug`, then rustc's default `2`. The task's option-1 wording named only the
two env vars; the manifest `test` row is in the resolver so a future
`[profile.test]` in `Cargo.toml` is attributed to the manifest, not the
environment.

## The chosen option: 1 (report instead of asserting), and it is recorded here

The task listed three options and marked the choice "owner's call — do not
pick one silently". Option 1 is implemented, and this record is the loud
part: the owner sees the decision in this file, in the test's doc comments
and in the review summary, not just in a diff.

Why option 1 over the others:

* Option 2 (better failure text) was already half-built by #691 —
  `line_table_diagnosis` names `CARGO_PROFILE_DEV_DEBUG` in the failure — but
  it leaves every local run on this host red, so the trap the task asks to
  remove stays armed: each new agent still has to learn the failure is
  environmental.
* Option 3 (documentation alone) leaves the failure message identical and
  does not by itself satisfy "no misleading failure". It is kept anyway —
  this file is the documentation.
* Option 1 is the only one that stops `accept_t430_a` failing locally: when
  the environment decided the binary's debug level, the artifact cannot
  testify about the committed profile at all, so the test reports the
  override and does not assert on it.

What the change is *not*: the skipped-sub-check case #691's review rejected.
That was a host file deletion with no measured occurrence, excused
unconditionally; this is a measured, deliberate environment override, and
only the artifact half stands down — the manifest and the CI workflow are
still asserted, and the artifact assertion still runs everywhere the
environment did not decide the level.

## What stays asserted, so nothing is loosened

* `accept_t430_the_workspace_keeps_backtrace_line_numbers` still requires the
  committed `[profile.dev] debug = "line-tables-only"`, whatever the
  environment says — an env override cannot launder a broken manifest,
  because the manifest is read, not the artifact.
* New `accept_t430_b_ci_workflow_pins_line_tables_for_the_rust_job` reads
  `.github/workflows/ci.yml` (owner-maintained, so asserted where it is read,
  not edited) and requires both `CARGO_PROFILE_*_DEBUG` pins to state
  `line-tables-only`. That is what makes "CI is where the artifact check is
  real" a tested claim rather than a comment.
* The artifact check itself is untouched for every resolution the
  environment did not decide: under `CARGO_PROFILE_*_DEBUG=line-tables-only`
  (CI), under no override, and under a manifest-stated level it runs and
  still fails when a frame in this file cannot be located — including the
  #691 condition, debug-map objects deleted after the link.
* An override that *keeps* line tables (`1`, `2`, `limited`, `full`,
  `line-tables-only`, `line-directives-only`) never stands the check down:
  the binary can carry the frames, so it must.

`environment_overrode_line_tables` reports via `eprintln!` and the test
passes; the report names the variable, why it is not a defect, that CI pins
`line-tables-only`, and the command to run the assertion locally under the
pin.

## Verified on this host

* `CARGO_PROFILE_DEV_DEBUG=0 cargo test -p cs_xtask --test accept_t430_ci_disk_budget`:
  **before** the change `accept_t430_a_…` failed as above; **after**, the
  whole file is green (the test reports the override instead of asserting —
  visible under `--nocapture`).
* `CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`: the **whole**
  suite under the host's override — 0 failures, exit 0 (a cold rebuild, so
  the run also re-measured the precedence claim end to end).
* `CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only`
  (the CI environment): the artifact assertion runs and passes, and
  `cargo test --workspace --locked` is fully green under it too.
* Negative case, acceptance bullet 3: the `debug = 0` binary run by hand
  under `CARGO_PROFILE_DEV_DEBUG=line-tables-only` still **fails** the
  artifact assertion — the env pin keeps the check in force and a binary
  without line tables does not pass it.
* `cargo test --workspace --locked -- accept_t430_ --include-ignored`: all
  15 selected tests pass under the pinned environment; under
  `CARGO_PROFILE_DEV_DEBUG=0` the same selection on `cs_xtask` passes 15/15.

## Not covered

* `RUSTFLAGS`/`CARGO_ENCODED_RUSTFLAGS` spellings of `-C debuginfo=` or
  `-C strip` are still reported by `line_table_diagnosis` on failure but do
  not stand the check down — only the two profile variables do. No host here
  sets them.
* A stale binary run by hand under a different environment than it was built
  with reads the *current* variables — cargo rebuilds on profile-env change,
  so this cannot arise through `cargo test`, which is how the suite runs.
* Only `cargo test`'s own profiles are resolved: a `cargo test --release`
  under `CARGO_PROFILE_RELEASE_DEBUG=0`/`CARGO_PROFILE_BENCH_DEBUG=0` would
  still fail the artifact assertion, reported by `line_table_diagnosis`.
  The suite's required commands do not use `--release`.

## Postscript (added in review, 2026-10-06)

`common.env` no longer exports `CARGO_PROFILE_DEV_DEBUG=0`: it now exports
`line-tables-only`, and its own comment records that `debug = 0` was tried on
2026-10-05 and reverted because it broke this test and still did not fit the
disk. The trap it armed is gone at the source; the stand-down above keeps the
test honest for any environment that repeats that experiment — including
sessions started before the revert and any future host that trades line
tables for disk again.

Two rows of the resolution table were re-measured in review with a scratch
crate (`/tmp`, since `Cargo.toml` is owner-maintained): a manifest that
states `[profile.test] debug = "line-tables-only"` built a test binary whose
frames kept `file:line` even under `CARGO_PROFILE_DEV_DEBUG=0` — the stated
`[profile.test]` row really does outrank the dev environment variable — and
`[profile.dev] debug = "line-directives-only"` produced frames that still
name `./src/lib.rs:6`, which is why `keeps_line_tables` classifies it as
keeping the line program.
