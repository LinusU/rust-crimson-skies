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
//! Every value in these files is authored here: newly authored synthetic
//! content only. No original game data, no `CS_GAME_DIR` access, no
//! original-behavior claim — the fixture proves the contract, never the
//! original renderer. `CS_GAME_DIR` is not needed.

mod adapters;
mod classification;
mod fixture;
mod frame_capture;
mod golden_scene;
mod profiles;
