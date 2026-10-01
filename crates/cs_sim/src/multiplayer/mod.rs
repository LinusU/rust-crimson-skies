//! Multiplayer match rules (F56-A).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-A`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`result`] is the one place a match's final result is decided: lethal
//! events and the two limits are folded into a single sealed
//! [`result::FinalResult`], whatever order the events arrived in and however
//! often a reliable event was retransmitted. The rule values it consumes (the
//! score table and the limits) are inputs; the original values are unknown
//! (see `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`), so nothing
//! here supplies a default.

pub mod result;
