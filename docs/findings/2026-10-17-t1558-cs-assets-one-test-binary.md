# #1558: cs_assets builds one integration-test binary

Date: 2026-10-17. Convention: `docs/findings/2026-10-17-t1542-one-test-binary-per-crate.md`.
Capabilities used: `retail` (local run with `CS_GAME_DIR`).

## What changed

`crates/cs_assets/Cargo.toml` sets `autotests = false` with one `[[test]]`
(`cs_assets_tests`, `tests/main.rs`). The 35 former `tests/*.rs` files and
`tests/common/mod.rs` are modules of `tests/main.rs`; a dev-dependency on
`cs_xtask` lets the new `accept_test_bin_rest_assets_` test run the production
`test_layout::check_member` on this crate.

## Test names

`cargo test -p cs_assets --locked -- --list --include-ignored`: **249 names
before, 249 after**; stripping the module prefix leaves identical sorted lists.
Integration binaries: 35 → 1. The new acceptance test makes it 250.

## Process-dependent tests, fixed

- `mod common;` in 19 files became `use crate::common;` (one shared module).
- `accept_f15_b_atomic_cache` and `accept_f15_d_cold_warm_restart` re-run their
  own binary with `--exact <name>`; the names are now module-qualified
  (`accept_f15_b_atomic_cache::…`, `accept_f15_d_cold_warm_restart::…`).
- `accept_f15_e` locates `cs-inspect` from `current_exe()` two levels up; unchanged
  and still correct.

## Verification

`--include-ignored --skip evidence_report` with `CS_GAME_DIR`: 207 passed, 0
failed (includes both child-process kill tests). The 12 `evidence_report_*`
harnesses need the environment of their documented evidence sequence and panic
without it, exactly as before the migration; they were not run here.
