# T432: CI runner disk exhaustion is resolved on main by T430

Verification only; no second fix was added.

## Evidence

* Cause: `rust-lld` SIGBUS at the last link (`Doc-tests cs_app`), see
  `2026-09-30-t430-rust-lld-sigbus-in-ci.md`.
* Fix: T430 (commit 9b47348) set `[profile.dev] debug = "line-tables-only"` in
  the root `Cargo.toml`.
* CI workflow runs on `main` after the fix, read with `gh run list` /
  `gh run view --log`:

| run | head | result |
| --- | --- | --- |
| 36765394390 | b4c6262 | success |
| 36769369617 | 6088114 | success |
| 36772691811 | 6a9bd9b | success |

* Run 36772691811 `cargo test` log contains
  `test crates/cs_app/src/livery.rs - livery (line 49) ... ok` and the full set
  of `Doc-tests` sections (cs_net, cs_script, cs_sim, cs_types, cs_xtask), so
  the whole step completed and no job died without a log.
* The `rust` job prints no `df` output, so a free-space margin was **not**
  measured here. The T430 finding holds the object-size measurements. Unknown:
  the absolute free bytes left at the last link. Adding a `df -h` step needs
  `.github/` (protected) and is an owner decision.

## Unmet criteria

* Criterion 2 (a branch adding one new `cs_app` test target is green) was not
  re-run by this task; #417 is in review and is the natural check case.
* Criterion 4 (df margin) is unmeasured, as above.
