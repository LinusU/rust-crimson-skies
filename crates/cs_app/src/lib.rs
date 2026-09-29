//! Bevy composition, the Avian physics adapter, rendering, input, audio and UI.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. The workspace bootstrap ships
//! the typed, asset-free [`synthetic`] development scene, the `cs` binary
//! entry point, its [`cli`] request parser and the [`run`] entry point that
//! executes a fixed-tick headless synthetic smoke. Rendering, input, audio, UI
//! and retail mission composition arrive with later F00+ tasks.
//!
//! [`origin`] is the world-origin frame of `specs/F16-coordinates-units-
//! origin-management-and-clocks.md` (stage F16-A): an f64 `WorldOrigin` per
//! epoch, its f32 local frame, and the typed distinction between a rebase
//! (world identity and swept continuity survive) and a teleport. Applying a
//! shift to every spatial subsystem atomically is F16-B.

pub mod cli;
pub mod origin;
pub mod run;
pub mod synthetic;
