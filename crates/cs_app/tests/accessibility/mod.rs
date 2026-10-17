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
//!
//! F52-C adds `session`: the live settings session — recovery from an
//! unusable file, apply/retry/teardown with the error propagated, the
//! labelled control profile, and one projection that hands a frame's
//! gameplay, presented effects and fidelity metadata to their consumers
//! together (AC03: shake and flash off, gameplay telemetry unchanged).
//!
//! F52-D adds `gpu_capture` and `record`: the objectives page drawn on a real
//! adapter (AC02/behavior 2 at the pixel level, the `gpu` capability) and AC04
//! end to end — a gameplay assist enabled in the live session and named by the
//! comparison/replay metadata, with the inert, refused and unsaved failure
//! cases. `evidence` is this task's evidence-report harness: deliberately not
//! named `accept_f52_d_*`, and `#[ignore]`d so the task selection never picks
//! it up as an acceptance test.

mod cues;
mod evidence;
mod fidelity;
mod gpu_capture;
mod navigation;
mod objective_page;
mod record;
mod remap;
mod session;
mod store;
