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
//! only when run locally with `--include-ignored`.

mod adapters;
mod additive_material;
mod classification;
mod fixture;
mod frame_capture;
mod golden_scene;
mod paint;
mod profiles;
mod visibility_consumer;
