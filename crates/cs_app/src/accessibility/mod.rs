//! Accessibility boundary (F52-A).
//!
//! Spec: `specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! stage `### F52-A`. Shared contract: `docs/contracts/UI-NETWORK.md`. The
//! settings types are `cs_content::settings`; this module is what consumes
//! them in the application:
//!
//! * [`navigation`] — keyboard-only and controller-only navigation of the
//!   F45-A front end, from boot to launch, and the gaps a binding map leaves.
//! * [`remap`] — a rebinding session that can be cancelled or reset and cannot
//!   commit a map that strands a device.
//! * [`cues`] — objective status cues that never rely on colour, scaled with
//!   the UI scale.
//! * [`objective_page`] — (F52-B) the in-flight objectives page as the player
//!   reads it: each `HudSession` row with its cue at the UI scale, and what
//!   fits a viewport.
//! * [`motion`] — reduced shake/flash that drops cosmetic effects and never a
//!   required damage or target notification.
//! * [`store`] — atomic persistence and the safe-defaults startup.
//!
//! Everything is **designed** and synthetic; no original option is asserted.
//! See `docs/findings/2026-10-01-f52-a-accessibility-settings.md`.

pub mod cues;
pub mod motion;
pub mod navigation;
pub mod objective_page;
pub mod remap;
pub mod store;
