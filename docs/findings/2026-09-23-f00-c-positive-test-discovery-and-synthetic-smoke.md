# F00-C: positive test discovery, the synthetic smoke, and what stays unknown

Date: 2026-09-23. Task: F00-C "Install CI plus task-specific positive-test
discovery" (`specs/F00-workspace-toolchain-and-first-executable.md`).
Capabilities used: ordinary build/test only (`CS_GAME_DIR` not needed).

## CI was already installed; it is guarded, not edited

`.github/workflows/ci.yml` exists and is owner-maintained (protected path), so
this task changed nothing under `.github/`. The workflow already runs, once a
`Cargo.toml` exists:

| gate | workflow command |
|---|---|
| fmt | `cargo fmt --all -- --check` |
| clippy | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` |
| tests | `cargo test --workspace --locked` |

Because an agent cannot edit that file, the check lives in the owner path
instead: `tools/cs_xtask/src/ci.rs` (`verify_workflow`, `verify_workflow_file`,
`verify_workspace_workflow`) and the `cs_xtask verify-ci` command read the real
workflow and fail, naming the gate, if one of the three commands disappears —
including clippy losing `-- -D warnings`. `accept_f00_c_ci_gates.rs` proves the
positive case against the committed file and the negative cases against the
same file with a gate removed.

Per the owner's note on the task, task-specific test discovery is **not** a CI
step: CI runs the whole suite once, and each agent/reviewer runs the prefix
gate locally.

## How the local discovery gate behaves

`docs/contracts/CLI-EVIDENCE.md` requires that a task prefix resolves to at
least one real test and that every discovered test passes again alone with
`--exact`. `tools/cs_xtask/src/test_select.rs` implements exactly that:

1. `cargo test --workspace --locked -- <prefix> --include-ignored --color never`
   (with `NO_COLOR=1`, `CARGO_TERM_COLOR=never`, so harness output parses the
   same locally and in CI), captured as evidence in `Selection::log`.
2. Names and counts are read from the harness's own `test <name> ... ok` /
   `... FAILED` lines and `test result:` summaries, summed over every target.
3. Classification is strict: a failing test is `SelectionFailed` (it outranks
   the nonzero cargo status, so the failing test is named rather than hidden
   behind "cargo failed"), a cargo failure with no failing test is
   `CargoFailed` quoting the log tail, a zero selection is `Empty`, and only an
   all-green nonempty selection passes.
4. `run_gate` re-runs every discovered name with `--exact --include-ignored`
   and requires it to be selected and to pass, so an unrelated test that merely
   embeds the prefix cannot rescue the selection.

Run it as `cargo run -p cs_xtask --locked -- test-select --prefix <prefix>`
(exit 0 gate passed, 1 gate failed, 2 invalid request).

## A running test can invoke cargo in the same workspace

The `accept_f00_c_*` tests exercise the gate end to end, which means a test
spawns `cargo test` while the outer `cargo test` is executing. This was probed
before it was relied on, because a held build-directory lock would deadlock.

Evidence: a temporary test `tools/cs_xtask/tests/zz_nested_cargo_probe.rs`
spawned `cargo test --workspace --locked -- accept_f00_b_pinned_dependencies
--include-ignored` from inside `cargo test -p cs_xtask --test
zz_nested_cargo_probe`; it finished in 3.25 s wall, exit 0, with the inner run
reporting `test result: ok`. Cargo releases the build-directory lock before it
executes test binaries, so nested runs serialize instead of deadlocking. The
probe file was deleted after this note was written; the finding stands on the
observed run above.

Consequence for future agents: the nested runs inside `accept_f00_c_` tests
deliberately select `accept_f00_b_*` and `accept_f00_c_ci_*`, never an
`accept_f00_c_` prefix that would recurse into the tests that spawn cargo.

## Trace format of the synthetic smoke

`cs --synthetic --headless --ticks <n> --trace <file>` writes JSON Lines: one
header line declaring `kind`, `provenance: SYNTHETIC`, `tick_hz` and
`requested_ticks`, then one sample per tick from 0 through `<n>` (so
`n + 1` records, the last one at the requested tick). A non-finite component
would be written as `null`, never as `NaN`/`inf`, which are not JSON numbers.
A trace that cannot be opened, written, flushed or synced fails the run with
exit 1; invalid requests fail with exit 2 before any file is created.

## Compatibility with the F00-B parser contract

`accept_f00_b_cli_parse_classifies_flags_without_touching_the_environment`
asserts that `parse(["--synthetic", "--ticks"])` (a flag whose value is the
last thing typed) stays `CliRequest::Unsupported { args }`. F00-C therefore
classifies *structurally* incomplete or unknown vectors as `Unsupported`, and
*complete but unusable* requests — `--ticks abc`, `--synthetic` without
`--headless`, `--synthetic --headless` without `--ticks` — as the new
`CliRequest::Invalid { reason }`, whose reason names the offending flag. Both
exit 2.

## Unknown / deliberately not implemented

* **`--seed`** appears exactly once in the whole pack: the example
  `cs --synthetic --headless --ticks 600 --seed 1 --trace …` in
  `docs/contracts/CLI-EVIDENCE.md`. No spec defines what the seed selects for
  the asset-free synthetic scene (the fixture body is deterministic constants
  in `cs_types::SyntheticBodySpec`). Accepting the flag without defined
  semantics would be a no-op stub, so `--seed` is currently rejected as
  unsupported input (exit 2) and the semantics are filed as a follow-up task
  rather than guessed here.
* **Windowed `--synthetic`** (without `--headless`) is rejected with exit 2:
  this stage has no window, renderer or GPU path, and a fake empty window is
  explicitly forbidden by the CLI evidence contract.
* Retail run modes (`--cs-path … --mission …`, `--input-replay`, `--cam`,
  `--screenshot`, `--profile-dir`) remain documented-but-unsupported until
  their own stages land; `--help` lists them as exit 2.
