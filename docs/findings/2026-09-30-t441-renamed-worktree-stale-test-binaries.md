# T441: a renamed worktree reuses test binaries with a stale baked-in path

## Symptom

`cargo test --workspace --locked` fails in a crate whose test derives the
workspace root from `env!("CARGO_MANIFEST_DIR")`, for example
`cs_formats` `pe_resources_tests::accept_f12_g_no_engine_path_reads_the_type_255_payload`:

```
reading /…/<old-checkout-name>/crates/cs_formats/src/pe_resources.rs: No such file or directory (os error 2)
```

The path is the checkout's *previous* name. `cargo test -p cs_formats --lib`
can pass while the workspace run fails, because only the workspace run reuses
the stale artefact.

## Cause

About 25 tests in `crates/` read source files relative to the compiled
`CARGO_MANIFEST_DIR`. That path is baked into the test binary when it is
built. When an agent renames or recreates its worktree but keeps the
per-checkout `target/` (or points `CARGO_TARGET_DIR` at the old one), the
fingerprints still match, cargo reuses the old binary and the path no longer
exists. CI runs on clean runners and is unaffected. No production behaviour is
affected.

## What to do

A failing test that names a path outside your current checkout is an
environment artefact, not a regression in the diff under review. Fix the
environment, never the test:

1. `cargo clean -p <crate>` for the crate that failed, or
2. use a fresh `CARGO_TARGET_DIR` (or delete `target/`) after any rename or
   recreation of a worktree,

then rerun the full checks. Do not weaken or edit the test to get green
(AGENTS rule 6).

## Not done here

The harness could avoid carrying `target/` across a rename; that is
harness-side and not changed by this task. The protected `AGENTS.md` is not
edited, so this finding is the place agents should be pointed to.
