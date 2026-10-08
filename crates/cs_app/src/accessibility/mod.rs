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
//! * [`gpu_capture`] — (F52-D) that page drawn on a real adapter: the row
//!   geometry and the surviving colour roles as measured quads, refusing a
//!   frame that drew nothing. The `gpu` half of the stage's evidence.
//! * [`motion`] — reduced shake/flash that drops cosmetic effects and never a
//!   required damage or target notification.
//! * [`store`] — atomic persistence and the safe-defaults startup.
//! * [`session`] — (F52-C) the live [`session::SettingsSession`] that ties
//!   them together: the recovery report of a boot, apply/retry/teardown with
//!   the error propagated instead of a change dropped, the labelled
//!   [`session::ControlProfile`], and [`session::SettingsSession::project`],
//!   which hands one frame's gameplay, presented effects and fidelity
//!   metadata to their consumers together.
//!
//! Everything is **designed** and synthetic; no original option is asserted.
//! See `docs/findings/2026-10-01-f52-a-accessibility-settings.md`,
//! `docs/findings/2026-10-08-f52-c-settings-session-integration.md` and
//! `docs/findings/2026-10-08-f52-d-accessibility-flows-gpu-review.md`.

pub mod cues;
pub mod gpu_capture;
pub mod motion;
pub mod navigation;
pub mod objective_page;
pub mod remap;
pub mod session;
pub mod store;
