//! Acceptance stage F50-A: the complete mission binding/coverage records
//! (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-A`, and the owner ruling of 2026-09-28 in
//! `specs/README.md`).
//!
//! The minimum scenario of this stage is AC01's shape — "Run all mission
//! dependency closures and assert none are silently omitted" — over the
//! engine-independent records, plus the three integrity rules the stage is
//! judged on: the denominator is frozen from the declared inventory, a
//! missing/unknown/unsupported child stays in the totals and never counts
//! as ready, and cycle/duplicate/dangling identity failures are reported.
//!
//! Every value in these files is authored here: newly authored synthetic
//! content plus the new-engine work-order inventory committed at
//! `missions/bindings/campaign-inventory.tsv`. No original game data, no
//! `CS_GAME_DIR` access, no gameplay claim — synthetic fixtures prove the
//! schema and its validation only, never the campaign (F50 owner ruling,
//! 2026-09-28). Binding real identities and running the real campaign stay
//! with F50-B/C/D.
//!
//! `m01_a.rs` … `m08_a.rs`, `m10_a.rs`, `m12_a.rs`, `m13_a.rs`, `m16_a.rs`,
//! `m17_a.rs`, `m18_a.rs`, `m19_a.rs`, `m21_a.rs` and `m24_a.rs` are the
//! exception that proves the rule: their `accept_m01_a_*` / … /
//! `accept_m08_a_*` / `accept_m10_a_*` / `accept_m12_a_*` / `accept_m13_a_*` /
//! `accept_m16_a_*` / `accept_m17_a_*` / `accept_m18_a_*` / `accept_m19_a_*` /
//! `accept_m21_a_*` / `accept_m24_a_*`
//! retail tests read `$CS_GAME_DIR`
//! through production code and are marked
//! `#[ignore = "requires CS_GAME_DIR"]`, so CI skips them and the
//! implementing and reviewing agents run them with `--include-ignored`.
//! `evidence.rs` writes those tasks' evidence reports; it is deliberately not
//! named with an acceptance prefix, so a task selection never picks it up as
//! an acceptance test.
//!
//! `f50_e4.rs` is a third kind of member: work order `F50-E4` binds no mission
//! and claims nothing about the campaign, but its `accept_f50_e4_*` tests are
//! retail too — they read `$CS_GAME_DIR` to re-derive the localized table's row
//! geometry and the title exactness of `campaign_bindings` a second time,
//! independently of the code under test, because those two rules were otherwise
//! proved only on authored values. See
//! `docs/findings/2026-10-03-f50-e4-row-geometry-and-title-exactness.md`.
//!
//! `f39_e3.rs` is the same kind of member: work order `F39-E3` binds no mission,
//! but its `accept_f39_e3_*` tests are retail — they read `$CS_GAME_DIR` to
//! measure the installation-scope reader archives (the install-wide and
//! world-group `zrdr.zbd`s) and bound the mission-scoped objective census's
//! denominator. `f39_e3_evidence.rs` is its evidence-report harness, selected by
//! test name like `evidence.rs`'s (`--test campaign evidence_report_f39_e3`), so
//! it keeps out of the `accept_f39_e3_` acceptance selection. Both live here so
//! the task adds no test binary of its own: CI's runner disk cannot afford one
//! more link per task (`docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`).

mod closure;
mod common;
mod coverage;
mod evidence;
mod f39_e3;
mod f39_e3_evidence;
mod f50_e4;
mod identity;
mod inventory;
mod m01_a;
mod m01_lc_player_config;
mod m02_a;
mod m02_t3;
mod m03_a;
mod m04_a;
mod m05_a;
mod m06_a;
mod m07_a;
mod m08_a;
mod m10_a;
mod m12_a;
mod m13_a;
mod m16_a;
mod m16_a_fu1;
mod m17_a;
mod m18_a;
mod m19_a;
mod m21_a;
mod m24_a;
