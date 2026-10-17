//! `cs_app`'s `cs_app_tests_campaign` integration-test binary (task #1543).
//!
//! One of three binaries: the `campaign/` and `world/` trees and the playtest files (the campaign tree borrows `world::import_retail`). The crate keeps three rather than one
//! because a single test binary's `__eh_frame` passes macOS's 16 MiB limit
//! (task #336); `[package.metadata.cs_test_layout]` in `Cargo.toml` declares the
//! count and `cs_xtask test-layout` requires each `.rs` file here to be reached
//! from exactly one target. See `docs/findings/2026-10-17-t1543-cs-app-test-binaries.md`.

mod campaign;
mod playtest_b0004;
mod playtest_fly;
mod playtest_full_aircraft;
mod playtest_retail;
mod playtest_retail_launch;
mod world;
