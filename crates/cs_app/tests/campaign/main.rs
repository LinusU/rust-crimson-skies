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
//! `m01_a.rs` … `m06_a.rs`, `m08_a.rs`, `m12_a.rs`, `m13_a.rs` and `m16_a.rs` are the exception that proves the rule: their
//! `accept_m01_a_*` / … / `accept_m06_a_*` / `accept_m08_a_*` / `accept_m12_a_*` / `accept_m13_a_*` / `accept_m16_a_*` retail tests read `$CS_GAME_DIR`
//! through production code and are marked
//! `#[ignore = "requires CS_GAME_DIR"]`, so CI skips them and the
//! implementing and reviewing agents run them with `--include-ignored`.
//! `evidence.rs` writes those tasks' evidence reports; it is deliberately not
//! named with an acceptance prefix, so a task selection never picks it up as
//! an acceptance test.

mod closure;
mod common;
mod coverage;
mod evidence;
mod identity;
mod inventory;
mod m01_a;
mod m02_a;
mod m03_a;
mod m04_a;
mod m05_a;
mod m06_a;
mod m08_a;
mod m12_a;
mod m13_a;
mod m16_a;
