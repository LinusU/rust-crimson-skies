//! Multiplayer match rules (F56-A, F56-B).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stages
//! `### F56-A`/`### F56-B`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`result`] is the one place a match's final result is decided: lethal
//! events and the two limits are folded into a single sealed
//! [`result::FinalResult`], whatever order the events arrived in and however
//! often a reliable event was retransmitted. Its victory condition is a
//! declared input ([`result::VictoryRule`]), not an assumption. The rule
//! values it consumes (the score table and the limits) are inputs; the
//! original values are unknown (see
//! `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`), so nothing here
//! supplies a default.
//!
//! [`objective`] is the server-authoritative possession state machine
//! (F56-B): every claim, drop, return and score is an [`EventId`]-identified
//! event queued on submission and adjudicated in id order at tick close, so
//! two clients claiming one objective can never both hold it — only the
//! server-accepted claim applies — and a retransmitted event is a duplicate,
//! never a second transition.

pub mod objective;
pub mod result;
