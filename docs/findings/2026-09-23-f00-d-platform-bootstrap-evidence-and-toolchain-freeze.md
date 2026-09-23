# F00-D: platform bootstrap evidence and the frozen toolchain

Date: 2026-09-23. Task: F00-D "Run platform bootstrap evidence and freeze
toolchain" (`specs/F00-workspace-toolchain-and-first-executable.md`).
Capabilities used: ordinary build/test only — no `CS_GAME_DIR`, GPU, audio or
network capability was needed or claimed, so no `acceptance.json` evidence
report is produced (`docs/contracts/CLI-EVIDENCE.md`, evidence-bound tasks
only). All commands below were run from the workspace root
(`/Users/linus/coding/rust-crimson-skies/agent-1`, macOS, aarch64) on the
candidate branch `rally/4-run-platform-bootstrap-evidence-and-free` based on
`aa838d366b537730d057432ddf8e0ad2c6b3dcd8` (tree
`6411ac7c4f3b2f9eb3446e8d83b2bbfd1ab6cdf1`).

## What this stage added

* `tools/cs_xtask/src/bootstrap.rs` — the platform bootstrap gate:
  `REQUIRED_MEMBERS` (the ten members the F00 deliverable names),
  `workspace_members` (parses `[workspace] members`, multi-line or inline),
  `verify_workspace` (four checks: every required member listed explicitly,
  each listed member has a `[package]` manifest, `crate::pins` verifies the
  frozen pins, `crate::ci` verifies the workflow gates) and the
  `BootstrapError` variants that name what failed.
* `cs_xtask verify-bootstrap [--workspace-root <dir>]` — the gate on the
  command line (exit 0 passed, 1 failed, 2 usage).
* `tools/cs_xtask/tests/accept_f00_d_workspace_bootstrap.rs` — six
  `accept_f00_d_*` tests (see "Test sensitivity").

No protected path, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` or
`.github/` file was modified; the workspace shape, pins and workflow were
already correct, this stage *gates* them.

## Why a static gate is required: real cargo stays green when a member disappears

The minimum acceptance scenario is "remove a required workspace member and
prove the gate fails". Before writing the gate the behaviour of the real
cargo commands was measured, in a throwaway worktree
(`git worktree add --detach private/f00-d-probe aa838d3`, edited only
`[workspace] members`, deleted with `git worktree remove --force` afterwards;
nothing from it was committed):

| command (probe worktree, exit code as observed) | result |
|---|---|
| `cargo metadata --no-deps --locked` with `tools/cs_inspect` removed | **exit 0**, 9 packages — `cs_inspect` simply gone |
| `cargo metadata --no-deps --locked` with `crates/cs_types` removed | **exit 0**, 10 packages — `cs_types` still a member, re-added implicitly because it is a path dependency of `cs_app`, so the `members` list itself keeps lying |
| `cargo check --locked --offline -p cs_types` with `tools/cs_inspect` removed | **exit 0** — no lockfile complaint |
| `cargo test --workspace --locked` (CI's exact test gate) with `tools/cs_inspect` removed | **exit 0**, 30 passed / 0 failed — **CI green** |

The green test run is the measured regression: compared with the same
command on the unmodified candidate (40 passed), the following four
`cs_inspect` acceptance tests never ran and nothing failed:

* `accept_f00_a_cs_inspect_cli_without_command_fails_with_diagnostic`
* `accept_f00_b_cs_inspect_unknown_command_still_fails`
* `accept_f00_b_cs_inspect_help_exits_zero_without_installation`
* `accept_f00_b_cs_inspect_version_exits_zero_without_installation`

(`cs_inspect` appears 0 times in the probe log's test output; the remaining
differences are this task's own six tests.) Consequences encoded in the
gate:

* membership must be **explicit**: globs (`crates/*`) do not satisfy
  `REQUIRED_MEMBERS`, because cargo's implicit path-dependency membership
  means the printed list is the only place the deliverable is recorded;
* the gate checks the **list**, not the build: cargo cannot be trusted to
  fail here at all.

Probe observations also recorded: `rustc`/`cargo` 1.98.1 link of the `cs`
binary emits `ld: __eh_frame section too large (max 16MB) ... performance of
exception handling might be affected` (`#[warn(linker_messages)]`). This is
**pre-existing at the base commit** (identical output in the unmodified
probe worktree) and is a linker note, not a lint, so clippy `-D warnings`
stays green; it is filed as follow-up task #336 rather than fixed in this
stage.

## Frozen toolchain and pins, as observed on disk

| item | value | source |
|---|---|---|
| `rustc` | `1.98.1 (48a229cea 2026-09-01)`, LLVM 22.1.8, host `aarch64-apple-darwin` | `rustc -Vv` |
| `cargo` | `1.98.1 (797e8a9bc 2026-08-05)` | `cargo -V` |
| active toolchain | `1.98.1-aarch64-apple-darwin (overridden by .../rust-toolchain.toml)` | `rustup show active-toolchain` |
| `rust-toolchain.toml` channel | `1.98.1` — exact patch, ≥ declared MSRV | `rust-toolchain.toml` |
| workspace `rust-version` (MSRV) | `1.98` | `Cargo.toml [workspace.package]` |
| `bevy` in `Cargo.lock` | `0.19.1` | intended `0.19.x` (S01/S11 baseline) |
| `avian3d` in `Cargo.lock` | `0.7.0` | intended `0.7.x` |
| edition / resolver | 2024 / 3 | `Cargo.toml` |

## cargo metadata of the candidate workspace

`cargo metadata --no-deps --locked --format-version 1` (exit 0) resolves
exactly the ten required members, each with the manifest path the gate
checks:

```
cs_app     crates/cs_app/Cargo.toml        cs_net     crates/cs_net/Cargo.toml
cs_assets  crates/cs_assets/Cargo.toml     cs_script  crates/cs_script/Cargo.toml
cs_content crates/cs_content/Cargo.toml    cs_sim     crates/cs_sim/Cargo.toml
cs_formats crates/cs_formats/Cargo.toml    cs_types   crates/cs_types/Cargo.toml
cs_inspect tools/cs_inspect/Cargo.toml     cs_xtask   tools/cs_xtask/Cargo.toml
```

`accept_f00_d_cargo_metadata_sees_every_required_member` asserts this
cross-check (count = 10, every required manifest path present) so the static
list cannot drift away from what cargo really builds.

## Actual command logs (all from the workspace root)

| command | exit | observed output |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | no output |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | no diagnostics; probe (below) confirms the new test target is actually linted |
| `cargo test --workspace --locked` | 0 | 32 `test result: ok` summaries, **40 passed, 0 failed** |
| `cargo test --workspace --locked -- accept_f00_d_ --include-ignored` | 0 | **6 tests**, all passed |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f00_d_` | 0 | `selected` ×6; `prefix "accept_f00_d_" selected 6 test(s) (6 passed, 0 failed, 0 ignored)`; `6 test(s) re-ran alone with --exact and passed` |
| `cargo run -p cs_xtask --locked -- verify-bootstrap` | 0 | `10 required workspace members are listed ... each with a [package] manifest`; `pins frozen — bevy 0.19.1, avian3d 0.7.0, workspace rust-version 1.98, toolchain 1.98.1`; `.github/workflows/ci.yml keeps cargo fmt, cargo clippy with -D warnings and the workspace test suite` |
| `cargo run -p cs_xtask --locked -- verify-ci` | 0 | workflow still runs the three gates |
| `rustc -Vv`, `cargo -V`, `rustup show active-toolchain` | 0 | versions in the table above |

Clippy coverage probe: after injecting `let deliberately_unused_probe = 1;`
into the new `accept_f00_d_workspace_bootstrap.rs` test target,
`cargo clippy --workspace --all-targets --all-features --locked --
-D warnings` **failed** with `error: unused variable: `deliberately_unused_probe``
(`-D unused-variables` implied by `-D warnings`, target `test
"accept_f00_d_workspace_bootstrap"`). The probe line was removed and clippy
returned to exit 0, so the gate demonstrably sees this stage's tests.

## Test sensitivity (`accept_f00_d_*`, 6 tests)

| test | production code exercised | fails when… |
|---|---|---|
| `accept_f00_d_bootstrap_gate_passes_on_this_workspace` | `bootstrap::verify_workspace` on the real workspace | the gate is removed/stubbed (no report to assert: ten members, exact `1.98.1` channel, MSRV series) |
| `accept_f00_d_removing_a_required_member_fails_the_gate` | same, on self-contained fixtures under `target/f00-d-bootstrap-fixtures/` | **minimum scenario**: `tools/cs_inspect` or `crates/cs_types` removed from `members`, or the list replaced by globs — each must return `Err(MissingMember)` naming the member |
| `accept_f00_d_missing_or_incomplete_member_manifest_is_reported` | same | a listed member's manifest is deleted (`MissingManifest`) or is not a package (`NotAPackage`) and the check stops noticing |
| `accept_f00_d_unfrozen_pins_or_a_lost_ci_gate_fail_the_gate` | same + `pins`, `ci` composition | `rust-toolchain.toml` deleted, lockfile moved to bevy `0.20.0`, or a workflow gate dropped — each must fail **this** gate |
| `accept_f00_d_verify_bootstrap_command_reports_and_fails_loudly` | `cs_xtask verify-bootstrap` binary | exit 0 without the three report lines, or exit 0 on a broken workspace instead of exit 1 naming `tools/cs_inspect`; bad option must stay exit 2 |
| `accept_f00_d_cargo_metadata_sees_every_required_member` | `bootstrap::REQUIRED_MEMBERS` cross-checked against `cargo metadata --no-deps --locked` | the required list and cargo's resolved workspace disagree (count or paths) |

Each failure test first builds a fixture that **passes**, then applies one
mutation, so the assertion is about the mutation rather than a broken
fixture. Fixtures are plain files (ten stub `[package]` manifests, a two-pin
`Cargo.lock`, `rust-toolchain.toml`, a workflow built from
`ci::REQUIRED_GATES`' own needles) — nothing synthetic claims retail
behaviour.

## Sources

`specs/F00-workspace-toolchain-and-first-executable.md` (F00-D section and
the evidence/completion rules); `docs/contracts/CLI-EVIDENCE.md` (prefix
discovery, negative tests, evidence-record minimum); `AGENTS.md`. No original
data was read; `CS_GAME_DIR` was not used.
