//! Acceptance stage F17-A: material classification and the golden
//! synthetic render-test scene
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! section `### F17-A`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! The minimum scenario is AC01 — "golden synthetic scene contains
//! overlapping glass, alpha-cut fence, additive sprite and per-corner
//! colors" — plus the contract behaviors this stage is judged on: a class
//! is declared with an evidence status and checked against established
//! facts, never derived from raw bytes or defaulted to opaque;
//! translucent surfaces order back-to-front against one view; equal-depth
//! ties are reported, not hidden; per-corner colors travel bit-exact.
//!
//! Every fixture in these files is authored here: newly authored synthetic
//! content only, proving the contract rather than the original renderer. The
//! `accept_f17_c_paint_` follow-up adds one `#[ignore]`d retail test and its
//! evidence harness, which read the original installation at `$CS_GAME_DIR`
//! only when run locally with `--include-ignored`. The `accept_t512_`
//! selection in `release_assets.rs` covers the store lifetime of a released
//! batch (Rally #512): what a spawn adds to `Assets<Mesh>` and to a material
//! store, what a release hands back, and what a frame that reuses every batch
//! adds. The `accept_f17_c_reused_` selection in the same file extends that rule
//! (Rally #516) to a reused batch whose material component went missing: it is
//! released and respawned rather than given a replacement entry no owner record
//! names, so neither material store grows.

mod adapters;
mod additive_material;
mod classification;
mod fixture;
mod frame_capture;
mod golden_scene;
mod paint;
mod profiles;
mod release_assets;
mod visibility_consumer;
