//! The mission session's **content-preparation** stage (VS-M01-RT-CONTENT,
//! #1214).
//!
//! A windowed mission composition needs one value that holds every record the
//! composition will consume, read before any window, Bevy app or ECS spawn
//! exists: [`MissionContent`]. [`MissionContent::prepare`] is that read. It
//! takes a satisfied [`crate::mission_launch::MissionLaunchPlan`] (the
//! measured closure [`crate::mission_launch`] already judged) and, for each
//! surface, calls the
//! **production reader that owns the surface** exactly as the launch plan
//! itself does — never a second parser, never a fallback and never an
//! invented default:
//!
//! 1. **World** — [`crate::world::retail::read_world_container`], then
//!    [`cs_content::world::ImportedWorld`] through the same
//!    [`cs_content::coordinates::SourceAdapter`]/[`cs_types::content::Origin`]
//!    pair the launch closure uses, then the container's uploaded meshes and a
//!    [`cs_content::world::WorldInstance`] for this mission's population.
//! 2. **Player start** — [`crate::mission_start::recover_retail_start_configuration`];
//!    the airframe and the initial pose must both arrive
//!    [`cs_types::content::Resolved::Known`].
//! 3. **Player flight law** — the measured campaign airframe's flight record,
//!    imported through [`cs_content::original_airframe`] and the same chain
//!    [`crate::playtest::retail::read_flight`] runs.
//! 4. **Environment** — [`crate::environment::read_mission_weather`] and its
//!    session at the tick rate the composition flies.
//! 5. **World actors** — [`crate::mission_world_actors::bind_mission_world_actors`],
//!    kept whole so the composition spawns from its own session.
//! 6. **Animations** — [`crate::animation::mission::bind_mission_animation`].
//! 7. **Objectives** — the mission's declared objective program is the
//!    measured control/directive lowering of item 8 (owner amendment of
//!    2026-10-10): it must lower **completely** before content exists. The
//!    separate [`crate::objectives`] recovery (→ `DeclaredObjectiveProgram` →
//!    `lower_program`) stays in that module, measured by Rally #1219; it
//!    refuses for every original mission today and is not consulted here.
//! 8. **Script host** — [`crate::mission_control::survey_mission_control_programs`]
//!    → the mission's row → its lowering → the lowered program.
//! 9. **Audio** — the sound-family archives in the mission's scope, classified
//!    through `cs_formats::zbd::dispatch` the way the launch closure classifies
//!    them.
//!
//! # Refusal is the whole contract
//!
//! Every step returns [`MissionSessionError`] and every variant carries the
//! refusing reader's **own** message plus the source path or logical key it
//! refused at (AGENTS.md rules 4 and 5): a value the production reader would
//! not stand behind is never replaced by a default, and a record that reads
//! but cannot be consumed is a refusal rather than a silently thinner
//! `MissionContent`. Nothing here is `verified_original`; the flight law's
//! provenance label travels on [`MissionFlight`] and says what it is.
//!
//! Nothing here spawns, draws, opens a window or runs a tick. The windowed
//! composition (VS-M01-RT-WINDOW, #1215) is the caller: it gates on
//! [`crate::mission_launch::MissionLaunchPlan::launchable`] and then asks this
//! module for the content — through [`compose::stage_for`], which turns the
//! prepared content into a [`compose::MissionStage`], and
//! [`compose::build_windowed`] / [`compose::build_headless`], the two faces
//! of the composition itself.

mod compose;
mod content;

pub use compose::{
    MissionComposition, MissionCompositionError, MissionPlayerBody, MissionPlayerStart,
    MissionStage, StageItem, StageMount, build_headless, build_windowed, run_windowed, stage_for,
    teardown,
};
pub use content::{
    MissionContent, MissionFlight, MissionSessionError, SoundArchive, prepare_mission_content,
    root_seed_from,
};
