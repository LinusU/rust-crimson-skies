# #1544: the remaining crates link one integration-test binary each

Date: 2026-10-17. Task: **#1544** (integration step after #1556 cs_content, #1557 cs_sim,
#1558 cs_assets, #1559 cs_script, #1560 cs_formats, #1561 cs_net, #1562 cs_xtask). Capabilities
used: plain build/test only.

## Result

Each of the seven crates sets `autotests = false` and declares one `[[test]]` target at
`tests/main.rs`. `accept_test_bin_rest_` (in `tools/cs_xtask/tests/accept_test_bin_rest_workspace.rs`
and the per-crate `accept_test_bin_rest_xtask`) asserts that through the guard's library functions
(`test_layout::check_member` / `check_workspace`), so un-migrating a crate or orphaning a test file
fails it.

| crate | integration binaries before | after |
| --- | --- | --- |
| cs_content | 112 | 1 |
| cs_sim | 64 | 1 |
| cs_assets | 35 | 1 |
| cs_script | 24 | 1 |
| cs_formats | 21 | 1 |
| cs_net | 18 | 1 |
| cs_xtask | 15 | 1 |
| **these seven** | **289** | **7** |

Per-crate `--list --include-ignored` before/after counts, and the tests that needed fixing for
sharing a process, are recorded in the findings of the per-crate tasks.

## Not met, and why

* **The whole workspace is not migrated yet.** `crates/cs_app` (task #1543) and `tools/cs_inspect`
  still auto-discover their test files (`autotests = false` absent), so the guard leaves them
  alone and the workspace-wide binary count is still dominated by cs_app. `cs_inspect` has no
  migration task that I could find; it needs one.
* **No workspace-wide compile/wall-time numbers.** AGENTS.md forbids a local
  `cargo test --workspace` (hours of macOS scan time), and those figures only mean something once
  cs_app is migrated. They should be taken from CI's logs on the commit that lands #1543.
