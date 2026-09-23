//! Bevy composition, the Avian physics adapter, rendering, input, audio and UI.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. The workspace bootstrap ships
//! the typed, asset-free [`synthetic`] development scene, the `cs` binary
//! entry point and its [`cli`] request parser. Rendering, input, audio, UI and
//! retail mission composition arrive with later F00+ tasks.

pub mod cli;
pub mod synthetic;
