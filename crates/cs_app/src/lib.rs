//! Bevy composition, the Avian physics adapter, rendering, input, audio and UI.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. The workspace bootstrap ships
//! the typed, asset-free [`synthetic`] development scene, the `cs` binary
//! entry point, its [`cli`] request parser and the [`run`] entry point that
//! executes a fixed-tick headless synthetic smoke. Rendering, input, audio, UI
//! and retail mission composition arrive with later F00+ tasks.
//!
//! [`origin`] is the world-origin frame of `specs/F16-coordinates-units-
//! origin-management-and-clocks.md`: an f64 `WorldOrigin` per epoch, its f32
//! local frame, the typed distinction between a rebase (world identity and
//! swept continuity survive) and a teleport, and (F16-B) the atomic
//! `OriginShift` transaction that converts a whole set of `SpatialAnchor`s
//! into a new frame. Wiring it into every spatial subsystem is F16-C.

pub mod cli;
pub mod origin;
pub mod run;
pub mod synthetic;
