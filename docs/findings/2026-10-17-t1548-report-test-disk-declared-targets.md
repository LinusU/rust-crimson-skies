# T1548: `report-test-disk` reads declared `[[test]]` targets

Follow-up to `2026-10-17-t1542-one-test-binary-per-crate.md` §6.

## Change

`cs_xtask::footprint::integration_test_targets` now reads the member's
manifest. When it sets `autotests = false`, the member's integration targets are
exactly its `[[test]]` tables (`path`, else cargo's default `tests/<name>.rs`,
via `cs_xtask::test_layout::declared_test_paths`); the module files
`tests/main.rs` reaches are not targets. Without `autotests = false` the
`tests/` enumeration is unchanged. The single target's binary is still matched
by the `.d` sidecar that names `tests/main.rs`, so its measurement is unchanged.

## Corrected numbers (migrated crates, 2026-10-17, this host)

| crate | integration targets before | after | cargo links |
| --- | --- | --- | --- |
| `cs_types` | 7 (6 former files + `tests/main.rs`) | 1 | 1 |

`cs_types` now plans 2 targets (1 integration + 1 unit harness), all measured.
Other migrated crates (`cs_content`, `cs_formats`, `cs_script`, `cs_sim`) were
already one file under `tests/` apart from modules, and report 2 planned each.
Unmigrated crates are reported as before.

## Tests

`accept_t696_a_migrated_crate_is_one_integration_target` fails when the
`autotests = false` branch is removed (the migrated fixture would plan three
targets). The workspace-wide oracle in
`accept_t696_the_workspaces_test_binaries_are_measured_not_guessed` now applies
the same rule. That test needs a workspace-wide build in the target directory
(migrated crates' `tests/main.rs` binaries must exist) to hold full coverage.
