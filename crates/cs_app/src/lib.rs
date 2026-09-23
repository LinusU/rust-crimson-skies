//! Bevy composition, the Avian physics adapter, rendering, input, audio and UI.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. F00-A ships only the workspace
//! bootstrap: the typed, asset-free [`synthetic`] development scene and the
//! `cs` binary entry point. Rendering, input, audio, UI and retail mission
//! composition arrive with later F00+ tasks.

pub mod synthetic;
