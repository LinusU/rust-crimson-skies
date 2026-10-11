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
//! Preparation itself spawns nothing, draws nothing, opens no window and runs
//! no tick. The windowed composition (VS-M01-RT-WINDOW, #1215) is the caller:
//! it gates on
//! [`crate::mission_launch::MissionLaunchPlan::launchable`] and then asks this
//! module for the content — through [`compose::stage_for`], which turns the
//! prepared content into a [`compose::MissionStage`], and
//! [`compose::build_windowed`] / [`compose::build_headless`], the two faces
//! of the composition itself.
//!
//! The `host` submodule is the stage those records were prepared for
//! (VS-M01-RT-MISSION-HOST, #1217): [`host::MissionHostSeed`] is what [`compose::stage_for`] leaves on
//! the stage, [`host::MissionHost`] launches it as one session, and
//! [`host::mission_host_tick`] is the one composed per-tick entry the
//! composition installs in the fixed-tick schedule. That module *does* run
//! ticks — one per committed fixed tick, in the documented order — and every
//! record it cannot drive is named as a [`host::MissionHostRefusal`] rather
//! than replaced. A settled run ends through [`terminal::MissionTerminal`]:
//! one terminal from either source, one exit, and the one report line the run
//! writes. [`host::MissionHost::restart`] rebuilds the authored initial
//! state — every session, the world and the player body — under a fresh
//! session generation, and [`host::mission_host_restart`] is the composed
//! entry that observes the playtest's meta reset and a
//! [`host::MissionHostRestartRequest`] and performs it.

mod compose;
mod content;
mod host;
mod player_visual;
mod terminal;

pub use compose::{
    MissionComposition, MissionCompositionError, MissionPlayerBody, MissionPlayerStart,
    MissionStage, StageItem, StageMount, build_headless, build_windowed, run_windowed, stage_for,
    teardown,
};
pub use content::{
    MissionContent, MissionFlight, MissionSessionError, SoundArchive, prepare_mission_content,
    root_seed_from,
};
pub use host::{
    MissionHost, MissionHostLaunchError, MissionHostRefusal, MissionHostReport,
    MissionHostRestartError, MissionHostRestartFailure, MissionHostRestartReport,
    MissionHostRestartRequest, MissionHostSeed, MissionHostStepError, MissionHostTick,
    host_session_id, install_mission_host, mint_host_generation, mission_host_restart,
    mission_host_tick, no_declared_objectives,
};
pub use player_visual::{
    MISSION_PLAYER_AIRCRAFT_ROOT_NAME, MISSION_PLAYER_INTACT_NODE_NAME,
    MISSION_PLAYER_INTACT_NODE_SLOT, MISSION_PLAYER_LOD_DISTANCE_M, MISSION_PLAYER_PROP_NODE_NAME,
    MISSION_PLAYER_PROP_NODE_SLOT, MissionPlayerVisual, PlayerAirframeSource, build_player_visual,
    spawn_player_visual,
};
pub use terminal::{MissionExit, MissionTerminal, MissionTerminalSource};
