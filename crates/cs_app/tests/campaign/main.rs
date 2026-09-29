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

mod closure;
mod common;
mod coverage;
mod identity;
mod inventory;
