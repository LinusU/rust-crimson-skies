# T619: the four `accept_doclib_conflict` hashes were four crates, not four configurations

Task #619 (`T617-LIVE-MULTIHASH`). Read-only investigation; no `deps/` file was
touched and no test, lint or `test = false` was changed.

## Cause

`accept_doclib_conflict` was not one test target built four ways. It was four
different test targets with the same file name, one per package:

* `crates/cs_app/tests/accept_doclib_conflict.rs`
* `crates/cs_content/tests/accept_doclib_conflict.rs`
* `crates/cs_formats/tests/accept_doclib_conflict.rs`
* `crates/cs_sim/tests/accept_doclib_conflict.rs`

Checked against `origin/main` as of the task's creation (2026-10-04): all four
files exist, and none of the four crates set `autotests = false`, so cargo
auto-discovered each as its own integration-test executable named
`accept_doclib_conflict`. Cargo's unit metadata includes the package id, so the
four get four different hashes in the shared `target/debug/deps`. Features,
profile and target triple do not differ; the differing metadata input is the
**package**. The task's "cs_app/cs_content" framing and #617's "feature sets,
profiles, or something else" are both superseded by this.

The same holds for every other `accept_*` name shared across crates, which is
why the finding in #617 saw several multi-hash names.

## Intended?

Not by design, but harmless as a naming collision. It is already gone: the
2026-10-17 tasks (`1195581c2` cs_content, `e54eb6a93` cs_formats, `1d3a8af5b`
cs_sim, and the earlier cs_app consolidation) set `autotests = false` and made
each former file a `mod` of the crate's `tests/main.rs`.

Current state, from `cargo metadata --no-deps --locked`: 14 integration-test
targets in the workspace, no test-target name occurs in more than one package,
and no target named `accept_doclib_conflict` exists. The workspace therefore
builds one binary per declared test target. Nothing further needs removing, and
`cs_xtask test-layout` keeps new top-level `tests/*.rs` files from reappearing.

## Consequence for #617 / #613

The "several binaries per target in front of the prune" correction in
`2026-10-04-t617-cargo-test-enoent-is-external.md` described the pre-
consolidation layout. After the consolidation the exposed population is one
harness per `[[test]]` target (plus lib/bin unit-test harnesses), with no
per-target multiplication. Stale pre-consolidation hashes may still sit in an
old `target/debug/deps` until cleaned, but they are no longer scheduled.
