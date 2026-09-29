# TEST-SPEED: acceptance tests no longer spawn cargo on this workspace

Date: 2026-09-29. Task: #375 TEST-SPEED-nested-cargo "Stop acceptance tests
from running nested full-workspace cargo test". Capabilities used: ordinary
build/test only (`CS_GAME_DIR` not needed).

## What was wrong

`docs/findings/2026-09-23-f00-c-positive-test-discovery-and-synthetic-smoke.md`
documented that `accept_f00_c_*` tests exercise the F00-C selection gate end
to end — which means a running test spawns `cargo test` in the same
workspace. The probe at the time showed nested runs serialize instead of
deadlocking; what it did not measure is the cost once the workspace grew.
Each nested `cargo test --workspace --locked -- <filter>` re-validated every
workspace target (including `rustdoc --test` for every crate) while
serialized on the same target-directory lock the outer run needs. Observed
upstream: a full run in agent-3 took over 40 minutes and the session lost
its claim.

A `ps` poll (every 5 s) during a baseline `cargo test --workspace --locked`
on this checkout caught the nested invocations, all with cwd in this
workspace:

```
cargo test --workspace --locked -- accept_f00_b_ --include-ignored --color never
cargo test --workspace --locked -- accept_f00_c_zz_no_such_test --include-ignored --color never
cargo test --workspace --locked -- accept_f00_b_workspace_pins_the_intended_baseline --exact --include-ignored --color never
cargo test --workspace --locked -- accept_f00_c_zz_no_such_test_exact --exact --include-ignored --color never
cs_xtask test-select --prefix accept_f00_c_ci_workflow_runs_fmt --workspace-root <this checkout>
  └ cargo test --workspace --locked -- accept_f00_c_ci_workflow_runs_fmt --include-ignored --color never
  └ cargo test --workspace --locked -- accept_f00_c_ci_workflow_runs_fmt_clippy_and_workspace_tests --exact --include-ignored --color never
```

(sources: `select_tests` and `verify_exact` calls and the `cs_xtask
test-select` end-to-end test in
`tools/cs_xtask/tests/accept_f00_c_test_selection.rs`)

## The change

The gate is still exercised end to end — real `cargo test`, real harness
output, real exit codes — but against throwaway fixture workspaces under
`target/` (gitignored, reaped by `cargo clean`):

- `accept_f00_c_test_selection.rs`: `fixture(name, test_source)` writes a
  self-contained workspace (own `[workspace]` table so nested cargo never
  resolves to this checkout, a hand-written `Cargo.lock` so the gate's
  `--locked` holds, one `tests/accept_fixture.rs`). `select_tests`,
  `verify_exact` and the `cs_xtask test-select` command test all point at
  fixtures now. A fixture run is ~0.5 s cold, ~0.1 s warm.
- `accept_f00_d_workspace_bootstrap.rs`: `cargo metadata` (no build, no
  target lock — but still a cargo process on this workspace) now runs on
  `manifest_copy`, a copy of the verbatim root manifest, member manifests
  and `Cargo.lock` plus stub `src/lib.rs`/`src/main.rs` targets. Cargo
  still resolves exactly the ten required members, so the "cargo agrees
  with the gate" cross-check is unchanged.
- New `accept_f00_c_gate_tests_stay_off_this_workspace`: scans
  `tools/cs_xtask/tests/*.rs` and fails if any cargo-spawning call
  (`select_tests`, `verify_exact`, `run_gate`, a `test-select` CLI with
  `--prefix`, or a direct `Command::new(&cargo)`) is driven against this
  workspace's root — the tripwire for this exact regression.

Nothing in production code changed; `cs_xtask test-select` still runs
`cargo test --workspace` on the real workspace when an agent invokes the
gate by hand — that is the gate's job. Only the test suite stopped doing
it inside `cargo test`.

## Measured wall time (`cargo test --workspace --locked`, warm build)

| run | real | user | sys | tests |
|-----|------|------|-----|-------|
| before (origin/main @ 1c7809c) | 83.77 s | 126.38 s | 45.20 s | 741 in 102 targets |
| after | 9.28 s | 9.34 s | 4.17 s | 742 in 102 targets |

Same machine, same warm target directory, `/usr/bin/time -p`. The +1 test
is the new tripwire. The ~74 s saved here is the serialized cost of the
nested full-workspace runs; on colder or more contended builds the gap is
larger (the reported stall was >40 min when several nested runs queued on
the target lock).

A 0.2 s `ps`/`lsof` poll during `cargo test -p cs_xtask --locked` after the
change shows every nested cargo running with its cwd inside
`target/f00-c-test-select-fixtures/*` — none on this workspace:

```
cargo test --workspace --locked -- accept_fixture_ --include-ignored --color never
    cwd=target/f00-c-test-select-fixtures/prefix-selection
cargo test --workspace --locked -- accept_fixture_alpha --exact --include-ignored --color never
    cwd=target/f00-c-test-select-fixtures/exact-re-run
cargo test --workspace --locked -- accept_fixture_zz_no_such_test --include-ignored --color never
    cwd=target/f00-c-test-select-fixtures/empty-selection
cs_xtask test-select --prefix accept_fixture_ --workspace-root target/f00-c-test-select-fixtures/test-select-command
```

## What still holds

Every behaviour the old tests proved still has a failing-when-broken test:

- prefix discovery runs and counts real tests (`accept_fixture_alpha` is
  discovered, counted and logged from real harness output),
- a zero-match selection is `SelectError::Empty`, an empty prefix
  `EmptyPrefix` (rejected before the workspace is even consulted),
- `--exact` re-runs pass a real test and reject an unknown name
  (`ExactEmpty`),
- `test-select` exits 0/1/2 end to end through the binary,
- CI gate contents and pinned dependencies are file reads, unchanged,
- cargo's own view of the member list is still cross-checked — on the
  manifest copy.
