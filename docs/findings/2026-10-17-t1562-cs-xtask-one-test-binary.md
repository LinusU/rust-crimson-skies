# #1562: cs_xtask links one integration-test binary

Date: 2026-10-17. Convention: `docs/findings/2026-10-17-t1542-one-test-binary-per-crate.md`.
Capabilities: ordinary build/test.

`tools/cs_xtask` now sets `autotests = false` with one `[[test]]` target
(`cs_xtask_tests`, `tests/main.rs`). The 16 former `tests/accept_*.rs` targets
are modules. The guard's own acceptance test (`accept_test_bin_guard.rs`) is one
of them and still runs the guard on the real workspace.

## Edits the shared binary needed

Two tests re-execute the test binary with `--exact <test name>`, which matches
the whole test path including the module. Each now derives the path from
`module_path!()`:

- `accept_t430_ci_disk_budget` (`cs_xtask_t430_panic_helper` child);
- `accept_t437_runtime_workspace_root` (the `..._follows_the_working_directory`
  child).

The other `CARGO_BIN_EXE_cs_xtask` users spawn the binary, not the test
executable, and needed no change.

## Test names

`cargo test -p cs_xtask --locked -- --list --include-ignored`:

| | before | after |
| --- | --- | --- |
| listed tests | 123 | 124 |

Stripping module prefixes the lists are identical except the added
`accept_test_bin_rest_xtask_*` test (manifest declares exactly one target; every
`tests/` entry is a module of `main.rs`). Nothing deleted, skipped or weakened.
All 124 pass with `--include-ignored`; `cs_xtask test-layout` reports 18 sources
reached for cs_xtask.
