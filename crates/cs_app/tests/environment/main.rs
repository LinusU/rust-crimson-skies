//! F19-A acceptance tests: environment data and time domains.
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`; shared contract `docs/contracts/IDENTITY-CONTENT.md`.
//! Task test prefix: `accept_f19_a_`.
//!
//! These tests drive production code only: `cs_content::environment` owns
//! the authored records and their validation, `cs_sim::visibility` owns the
//! time domain the schedule advances on, and `cs_app::environment` owns the
//! [`SkyFrame`] conversion and the [`EnvironmentClock`] wiring plus the two
//! synthetic fixtures. No test carries its own environment builder, its own
//! clock or its own sky math.
//!
//! No original data and no `CS_GAME_DIR` access: every value here is
//! authored development content (`Origin::SyntheticFixture`), so these
//! tests prove the interface and the time-domain contract, never the
//! original game.
//!
//! The stage's minimum scenario is AC01 — *rebase the world under a fixed
//! horizon; sky and sun direction stay stable* — and it lives in `sky`
//! together with the rest of F19 non-negotiable behavior 3 (centred on
//! camera translation, honouring world orientation, an unknown sun that
//! stays unknown). `records` owns the typed contract in
//! `cs_content::environment`: the missing-sky diagnostic and the
//! synthetic-only generated sky (behavior 5), a designed fog default that
//! never fills in gameplay visibility (behavior 1), the single
//! authoritative wind field plus the separate cosmetic stream (behavior 2),
//! the timeline's validation and the record fingerprint. `clock` owns
//! behavior 4: events fire at their own tick, pause freezes them, a replay
//! of the same frames reaches the same states and no local speed-up may
//! move them.

mod clock;
mod common;
mod records;
mod sky;
