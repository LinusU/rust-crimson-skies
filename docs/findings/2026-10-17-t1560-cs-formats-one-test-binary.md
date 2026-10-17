# #1560: cs_formats links one integration-test binary

Date: 2026-10-17. Convention: `docs/findings/2026-10-17-t1542-one-test-binary-per-crate.md`.
Capabilities: ordinary build/test; `CS_GAME_DIR` set for the `--include-ignored` run.

`crates/cs_formats` now sets `autotests = false` with one `[[test]]` target
(`cs_formats_tests`, `tests/main.rs`). The 21 former targets (16 `accept_*.rs`
files, `bm`, `interp`, `rof`, `string_id`, `evidence_report_f10_b_gamez`, and
the directory targets `corpus`, `gamez`, `script_raw`, `texture`, `zbd`) are
modules; `tests/common/` is one shared module. Each directory target's
`main.rs` became `mod.rs`.

## Edits the shared binary needed

- Ten files had `mod common;`; as a submodule that would look for
  `tests/<file>/common.rs`, so each is now `use crate::common;`.
- `corpus/{bindings,fixtures,tests}.rs` addressed siblings as `crate::spans`
  etc.; they now use `super::`.
- `include_bytes!`/`include_str!` are relative to the declaring file, which did
  not move, so they are unchanged.

## Test names

`cargo test -p cs_formats --locked -- --list --include-ignored`:

| | before | after |
| --- | --- | --- |
| listed names (unit + integration) | 420 | 422 |

Stripping module prefixes, the lists are identical except the two added
`accept_test_bin_rest_formats_*` tests (manifest declares exactly one target;
every `tests/` entry is a module of `main.rs`). Nothing deleted, skipped or
weakened.

## Process-dependent tests

None needed a fix. The evidence harnesses (`evidence_report_*`, 8 tests, all
`#[ignore]`) spawn `git`/`rustc` and read `CS_EVIDENCE_*` variables; they locate
files with `CARGO_MANIFEST_DIR` and do not depend on the executable's name. With
`--include-ignored` and `CS_GAME_DIR` set, 368 pass and exactly those 8 fail with
`CS_EVIDENCE_DIR is not set`, as they did when separate binaries (they only run
through their module-doc sequence).

`cs_xtask test-layout` reports 55 sources reached for cs_formats.
