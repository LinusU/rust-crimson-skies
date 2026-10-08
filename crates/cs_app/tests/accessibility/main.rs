//! Acceptance stage F52-A: accessibility settings and fidelity boundaries
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! section `### F52-A`).
//!
//! The minimum scenario is AC01's shape: boot to launch with the keyboard only
//! and with the controller only. The rest is the data half of AC02 (cues
//! without colour, at large scale), AC03 (presentation settings leave the
//! gameplay inputs and required notifications unchanged) and AC04 (an assist
//! is named in the fidelity metadata), plus remap recovery and atomic
//! persistence. Everything is authored synthetic data; this proves the
//! boundary only, never an original option (F52-B/C/D).
//!
//! F52-B adds `objective_page`: the F46-C objectives page read with the colour
//! filter off at the largest UI scale.

mod cues;
mod fidelity;
mod navigation;
mod objective_page;
mod remap;
mod store;
