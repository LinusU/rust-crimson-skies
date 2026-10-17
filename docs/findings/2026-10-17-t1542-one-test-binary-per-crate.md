# #1542: one integration-test binary per crate — the convention, the guard, and cs_types as the first instance

Date: 2026-10-17. Task: **#1542** "One integration-test binary per crate:
guard, convention and cs_types". Follow-ups: **#1543** (cs_app, 249 test
files) and **#1544** (cs_content, cs_sim, cs_assets, cs_script, cs_formats,
cs_net, cs_xtask). Capabilities used: ordinary build/test only — no
`CS_GAME_DIR` read, no evidence report required.

## 1. Why the layout changes at all

Every `crates/<crate>/tests/*.rs` file is its own Cargo test target, so
`cargo test` links one executable per file. On the owner's laptop macOS
XProtect scans each freshly linked executable the first time it is launched,
measured at **~50-70 s per binary on 2026-10-17**; a workspace-wide run links
~530 of them, and CI has separately run the runner out of disk and killed a
linker with SIGBUS at ~380 engine-linked test binaries
(`docs/findings/2026-10-06-t696-ci-runner-disk-budget.md`). The tests
themselves run in seconds. The cost is the number of executables.

## 2. The convention

A migrated crate sets `autotests = false` and declares exactly one
integration-test target:

```toml
[package]
name = "cs_types"
…
autotests = false

[[test]]
name = "cs_types_tests"
path = "tests/main.rs"
```

Every former `tests/<file>.rs` becomes a module of `tests/main.rs`
(`mod accept_f01_a_claim_records;`), and an existing directory-style target
(`tests/<name>/main.rs`) becomes `tests/<name>/mod.rs` under `mod <name>;`.
Shared helper modules stay modules of the same binary. Test prefixes keep
working untouched because libtest's filter is a **substring** match: the
prefix `accept_t583_` selects `accept_t583_splitmix64_state::accept_t583_…`
just as it selected the top-level name before.

`AGENTS.md` rule 6 now states the convention for new work.

## 3. cs_types, the first migration

`crates/cs_types` grew `tests/main.rs` (six `mod` declarations) and the
`autotests = false` + single `[[test]]` pair above; nothing else moved and no
test was renamed, weakened or deleted.

Test names listed by `cargo test -p cs_types --locked -- --list
--include-ignored`:

| | before | after |
| --- | --- | --- |
| integration targets | 6 | 1 |
| integration test names | 30 | 30 (each prefixed with its module path) |
| unit-test names (`content::tests`, `input::tests`, `space::tests`) | 20 | 20 (unchanged) |
| **total names listed** | **50** | **50** |

Stripping the six added module prefixes (`accept_f01_a_claim_records::`,
`accept_f02_a_manifest_and_profile_schema::`, `accept_t335_splitmix64::`,
`accept_t337_verified_original_methods::`, `accept_t397_identity_contract::`,
`accept_t583_splitmix64_state::`) makes the after-list identical to the
before-list, name for name. Two task prefixes still select exactly the same
tests on the migrated crate:

```text
cargo test -p cs_types --locked -- accept_f01_a_ --include-ignored
  → 9 passed; 0 failed   (accept_f01_a_claim_records::accept_f01_a_…)
cargo test -p cs_types --locked -- accept_t583_ --include-ignored
  → 2 passed; 0 failed   (accept_t583_splitmix64_state::accept_t583_…)
```

The `--exact` half of the task-selection gate is unaffected: it re-runs the
names the selection run discovered, and those are now module-qualified names
that libtest runs alone exactly as before. Run against the real workspace on
this host end to end:

```text
cs_xtask test-select --prefix accept_t583_
selected accept_t583_older_versioned_records_are_refused                    (cs_script)
selected accept_t583_save_record_stores_and_continues_the_stream_state      (cs_script)
selected accept_t583_restore_refuses_an_out_of_domain_rng_state             (cs_script)
selected accept_t583_splitmix64_state::accept_t583_draws_since_recovers_the_exact_draw_index
selected accept_t583_splitmix64_state::accept_t583_new_of_state_continues_the_same_stream_bit_for_bit
test-select: prefix "accept_t583_" selected 5 test(s) (5 passed, 0 failed, 0 ignored)
test-select: 5 test(s) re-ran alone with --exact and passed     (exit 0)
```

The two migrated-crate names are module-qualified and both were re-run alone
with `--exact` and passed, so the command the contract names does not need a
change for the new layout.

## 4. Measured wall time, before and after

Method, on the owner's laptop (macOS aarch64, per-worktree
`CARGO_TARGET_DIR`, `dev` profile with `debug = "line-tables-only"`), for the
crate only, with the crate's own artifacts removed first:

```sh
cargo clean -p cs_types
time cargo test -p cs_types --locked
```

Two runs in the pre-migration layout (the same tree with `tests/main.rs` and
the manifest's `autotests = false` / `[[test]]` removed, then restored) and
three in the migrated one:

| | integration binaries linked and exec'd | `cargo clean -p cs_types` removes | wall time of build + run |
| --- | --- | --- | --- |
| before (6 files) | 6 + 1 unit harness | 69-89 files, 23.4 MiB | **2.85 s**, **3.67 s** (one run each; median ~3.3 s) |
| after (1 file) | 1 + 1 unit harness | 55-79 files, 18.1 MiB | **1.81 s**, **1.82 s**, **1.81 s** |

The honest reading of this measurement: the crate links **one executable
instead of six**, and its wall time is ~1.8 s against ~2.9-3.7 s on this host
— a saving of roughly 5 MiB of linked output and 1-2 s here. `cs_types` is
deliberately dependency-free, so its six binaries are small and no 50-70 s
XProtect pause was visible in this run; the pause is per *executable*, so the
convention's payoff scales with the number of files a crate has and with what
each binary links. This measurement is *not* evidence for the ~9-hour
workspace figure — that belongs to `cs_app`'s 249 engine-linked files, which
#1543 migrates. What it does show is the unit of the saving: one link, one
executable, one scan per crate per replaced file.

## 5. The guard, and where it runs

`cs_xtask::test_layout` (`tools/cs_xtask/src/test_layout.rs`,
`cs_xtask test-layout`) is the gate. For every `[workspace] members` entry
whose manifest sets `autotests = false` it:

1. requires exactly one `[[test]]` target (`path`, or cargo's default
   `tests/<name>.rs`), and that its source file exists;
2. walks the module tree from that file the way rustc resolves it —
   `mod name;` → `name.rs` then `name/mod.rs`, `#[path = "…"]` honoured, a
   file module inside an inline `mod outer { … }` block resolved one directory
   deeper (mod-rs vs non-mod-rs files are distinguished), comments, string
   literals and char literals lexed away first;
3. reports every `.rs` file under the crate's `tests/` directory the walk did
   not reach, **naming the stray file and the `tests/main.rs` to declare it
   in** (and noting that a former `tests/<name>/main.rs` target becomes
   `tests/<name>/mod.rs`).

A member that has not migrated is left alone, because its files each really
are their own binary. A `[workspace] members` entry that is a glob is
reported as *unchecked* rather than silently passed.

**Enforcement.** The guard's acceptance test
`tools/cs_xtask/tests/accept_test_bin_guard.rs` (prefix
`accept_test_bin_guard_`, 6 tests) runs it against the real workspace inside
CI's ordinary `cargo test --workspace`, so the check needs no workflow edit.
It also creates a temporary `crates/cs_types/tests/stray.rs`, asserts the
guard fails and names both the file and the `main.rs`, removes it again (a
`Drop` guard, so a panicking run cannot leave the tree dirty) and asserts the
guard passes again. Fixture workspaces cover a stray top-level file, a former
directory-style target, a declared directory module, `#[path]`, a nested
`mod` reached from a named file, comments/strings that merely mention `mod`,
an unmigrated member, `autotests = false` with zero or two `[[test]]` targets,
and a `[[test]]` target whose file is gone. The `test-layout` command is
driven end to end for its exit codes (0 clean, 1 stray or unusable workspace,
2 a flag belonging to another subcommand).

`.github/workflows/ci.yml` was **not** touched: it names no `--test` target,
so the migration changes nothing for it.

## 6. Limits and follow-ups

- `cs_xtask::footprint` (`report-test-disk`, #696) still derives its plan of
  integration targets from the filesystem — `tests/<name>.rs` and
  `tests/<name>/main.rs` — so a migrated crate is **over-counted** there
  (`cs_types`: 7 integration targets in the report against 1 real cargo
  target). The report is a measurement, not a gate, and its own suite still
  passes because both sides of its assertions use the same enumeration; the
  manifest's `autotests = false` + `[[test]]` pair is now the source of truth
  and the module should read it. Filed as a follow-up rather than fixed here
  (another task's module and acceptance suite).
- Only `cs_types` is migrated. Every other crate still has one binary per
  test file until #1543/#1544 land, so the workspace-wide link and scan
  count is barely reduced yet.
- The guard checks reachability of `tests/` sources; it cannot know that the
  tests reached still *run* (that is the ordinary test suite's job), and it
  does not police a crate's `src/` module tree.

## 7. Sources

- `docs/findings/2026-10-06-t696-ci-runner-disk-budget.md` — per-binary link
  cost on the CI runner and the disk/SIGBUS failures.
- `crates/cs_types/Cargo.toml`, `crates/cs_types/tests/main.rs`,
  `tools/cs_xtask/src/test_layout.rs`,
  `tools/cs_xtask/tests/accept_test_bin_guard.rs`, `AGENTS.md` rule 6 —
  the convention, the guard, its tests and the rule.
- Local measurements above (2026-10-17, this host).
