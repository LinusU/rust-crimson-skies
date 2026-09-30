//! F19-A and F19-B acceptance tests: environment data, time domains and
//! environment effects.
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stages
//! `### F19-A` and `### F19-B`; shared contract
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefixes: `accept_f19_a_`
//! (stage A) and `accept_f19_b_` (stage B).
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
//! The stage-A minimum scenario is AC01 — *rebase the world under a fixed
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
//!
//! The stage-B modules are the *effects* of those records, in
//! `cs_app::environment`:
//!
//! * `air` owns F19 non-negotiable behavior 2 as an effect: the one
//!   authoritative wind field, `v_air = v_world - wind_world` for aircraft
//!   and projectiles alike, and the refusal of an unknown wind. Its
//!   minimum scenario is **AC02** — *wind changes affect aircraft airspeed
//!   and projectile-relative velocity consistently* — measured against the
//!   real `cs_sim::flight::FlightModel`.
//! * `effects` owns what a frame may be drawn from: the sky decision
//!   (behavior 5), the fog fade that is never a sight range (behavior 1),
//!   the sun/ambient rig that is never invented, the cloud layers and the
//!   per-frame `EnvironmentEffects` that follows the weather timeline.
//! * `cosmetic` owns the decorative side of behavior 2: particles drawn only
//!   from the run's cosmetic weather stream and advected only by the
//!   authoritative wind, with only authored precipitation drawn at all.
//!
//! Stage B's remaining sheet cases are not silently dropped: AC03 (weather
//! seeds do not change mission AI RNG sequences) is F19-C's minimum scenario
//! and needs the AI consumer that does not exist yet, and AC04 (the original
//! environment states actually present in each world) needs `retail` + `gpu`
//! in F19-D. Neither is claimed here.

mod air;
mod clock;
mod common;
mod cosmetic;
mod effects;
mod records;
mod sky;
