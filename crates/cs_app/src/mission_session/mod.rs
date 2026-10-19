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
//!    `lower_program`) stays in that module, whose measurement is Rally
//!    #1355; its `program()` is a computed per-record refusal for every
//!    original mission today and is not consulted here.
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
//! The `evidence` submodule is the capture path (VS-M01-RT-EVIDENCE-A,
//! #1312): `CS_MISSION_CAPTURE=1` makes [`run_windowed`] write a real
//! framebuffer PNG and `trace.jsonl` into `CS_EVIDENCE_DIR`, and refuse by
//! name when `gpu` or the directory is missing.
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
//!
//! The `observation` submodule is the world's own report that host folds in
//! stage 4 (M01-PLAY-TRIGGERS-OBSERVE, #1480): [`observation::mission_world_observation`]
//! reads the live ECS world once per fixed tick and writes
//! [`host::MissionWorldObservation`], installed beside
//! [`host::install_mission_host`] so the report describes the same tick's
//! physics. A row nobody can observe is not written — the player body's pose,
//! each bound world actor's pose under its measured subject name and
//! presence (the world-actor session's lifecycle × the damage authority's
//! destruction ledger, M01-PLAY-TRIGGERS-ACTOR-BIT), and the living count of
//! a group the composition really spawned members for — and
//! every other member chain the record spells stays unobserved, fail-closed.

mod actor_census;
mod actor_visual;
mod cockpit_hud;
mod compose;
mod content;
mod evidence;
mod flight_check;
mod gun_sound;
mod host;
mod landings;
pub(crate) mod launch_camera;
pub(crate) mod launch_cinematic;
mod launch_sound;
mod letterbox;
mod lifecycles;
mod observation;
pub(crate) mod parts;
mod player_visual;
mod radio;
mod scenario;
pub(crate) mod smoke;
mod terminal;
mod weapon_present;
mod weapons;
mod world_draw;

pub use actor_census::{
    CensusRow, MAX_CONTACTS, MissionActorAidPlugin, contact_list_text, nearest_hostiles,
};
pub use actor_visual::{
    ACTOR_MODEL_CLAIM, ActorVisualDrawn, MissionActorVisuals, attach_actor_visuals,
    build_actor_visuals,
};
pub use cockpit_hud::{
    MissionCockpitHudPlugin, clock_hour, compass_strip, damage_fraction, damage_tint, marker_text,
    safe_area,
};
pub use compose::{
    MissionComposition, MissionCompositionError, MissionPlayerBody, MissionPlayerStart,
    MissionStage, StageItem, StageMount, build_headless, build_windowed,
    declared_engine_objectives, run_windowed, stage_for, teardown,
};
pub use content::{
    MissionContent, MissionFlight, MissionSessionError, SoundArchive, prepare_mission_content,
    root_seed_from,
};
pub use evidence::{
    CAPABILITIES_ENV, CAPTURE_ENV, CaptureHandle, CaptureRefusal, CaptureSettings,
    EVIDENCE_DIR_ENV, MIN_CLEAR_COVERAGE_PERMILLE, MIN_WORLD_COVERAGE_PERMILLE,
    MissionCapturePlugin, PLAYBACK_FILE, TRACE_FILE, finish as finish_capture, write_playback,
};
pub use flight_check::{
    ButtonRole, CHANGE_AMMO_UNAVAILABLE, CHANGE_PLANE_UNAVAILABLE, FlightCheckArtwork,
    FlightCheckPhase, FlightCheckPhaseError, FlightCheckScene, FlightCheckSceneError,
    FlightCheckScreenPlugin, PhaseResponse, RETURN_TO_BRIEFING_UNAVAILABLE, SKIP_NOTICE,
    SceneButton, SceneText, SceneUnknown, install_flight_check, install_flight_check_screen,
    mission_is_held, mission_is_released,
};
pub use gun_sound::{
    WeaponSound, WeaponSoundError, measure_weapon_sound, sound_like_keys,
    weapon_sound_from_documents,
};
pub use host::{
    MissionHost, MissionHostLaunchError, MissionHostRefusal, MissionHostReport,
    MissionHostRestartError, MissionHostRestartFailure, MissionHostRestartReport,
    MissionHostRestartRequest, MissionHostSeed, MissionHostStepError, MissionHostTick,
    MissionWorldObservation, host_session_id, install_mission_host, mint_host_generation,
    mission_host_restart, mission_host_tick, no_declared_objectives,
};
pub use landings::{
    LANDING_CALL_CHAIN_CLAIM, LANDING_DETECTION_CLAIM, LANDING_NODE_POSITION_CLAIM,
    LANDING_RADIUS_M, LANDING_ROWS_CLAIM, LANDING_SINK_RATE_MPS, LANDING_WINDOW_CLAIM,
    LandingActivation, LandingCall, LandingLedger, LandingNodeEntry, LandingParseError,
    LandingReadError, LandingRefusal, LandingRow, LandingTick, MissionLandings,
    landing_node_entries, landing_reached, parse_landings, read_landings, step_landings,
    window_satisfied,
};
pub use launch_camera::{
    CAMERA_FRAME_NODE, LaunchCameraPlayback, LaunchCameraStats, camera_in_world,
    install_launch_camera,
};
pub use launch_cinematic::{
    LAUNCH_CAMERA_CARRIER, LaunchCinematic, LaunchCinematicError, LaunchCinematicLength,
    install_launch_cinematic, launch_cinematic_playing, measure_launch_cinematic,
    player_has_control, read_launch_cinematic, ticks_for,
};
pub use launch_sound::{
    LAUNCH_SOUND_CONTAINER, LaunchAudioProbe, LaunchSound, LaunchSoundError, backend_refusal,
    install_launch_audio, measure_launch_sound, member_content_id, stage_container,
};
pub use letterbox::{
    LaunchLetterbox, LetterboxBars, MissionLetterboxPlugin, PICTURE_END, PICTURE_TOP, REF_HEIGHT,
    REF_WIDTH, letterbox_bars,
};
pub use lifecycles::{LifecycleAdvance, ObjectiveLifecycleTick, ObjectiveLifecycles};
pub use observation::{
    PLAYER_MEMBER, mission_world_observation, subject_node_name, world_actor_in_play,
};
pub use parts::{
    BoundPart, DESIGNED_PART_HALF_EXTENTS_M, DESIGNED_PART_INTEGRITY, M01_PART_GEOMETRY_CLAIM,
    M01_PART_GEOMETRY_UNKNOWN_CLAIM, M01_PART_STATE_CLAIM, MissionPartReport, MissionPartState,
    MissionPartSweep, TurretRecord, classed_part_node, designed_part_node, part_entities,
    part_in_play, part_state_chains, part_state_node, part_sweep_geometry, pattern_matches,
    plan_part_states, read_turret_records, spawn_part_state, turret_part_node, turret_record_for,
};
pub use player_visual::{
    MISSION_PLAYER_AIRCRAFT_ROOT_NAME, MISSION_PLAYER_BLUR_NODE_NAME,
    MISSION_PLAYER_BLUR_NODE_SLOT, MISSION_PLAYER_FLARE_LEFT_NODE_SLOT,
    MISSION_PLAYER_FLARE_NODE_NAMES, MISSION_PLAYER_FLARE_RIGHT_NODE_SLOT,
    MISSION_PLAYER_INTACT_NODE_NAME, MISSION_PLAYER_INTACT_NODE_SLOT,
    MISSION_PLAYER_LOD_DISTANCE_M, MISSION_PLAYER_PROP_NODE_NAME, MISSION_PLAYER_PROP_NODE_SLOT,
    MissionPlayerVisual, PlayerAirframeSource, build_player_visual, spawn_player_visual,
};
pub use radio::{
    MissionRadio, MusicTrackSource, PreparedLine, RADIO_SOUND_CONTAINER, RadioError,
    RadioLineOutcome, RadioRefusal, ResolvedGroup, SOUND_GROUPS_ARCHIVE, SOUND_GROUPS_MEMBER,
    VoicePublicationWatch, declared_sound_groups, dialogue_content_id,
    install_radio_voice_publication, offer_cues, publish_prepared_voices,
};
pub use scenario::{
    START_ENV, START_HEADING_ENV, Scenario, StartOverride, TriggerWatch, WATCH_BLOCK_ENV,
    WATCH_MAX_FRAMES_ENV,
};
pub use smoke::{
    ActorMetrics, ApproachMetrics, AudioMetrics, CINEMATIC_SHOT_SECONDS, CameraMetrics, Check,
    CinematicMetrics,
    DEFAULT_MISSION_CAPTURE_DIR, FlightCheckButtonMetrics, FlightCheckMetrics,
    FlightCheckTextMetrics, GunMetrics, HudMetrics, MAX_WINGMAN_DISTANCE_M, MIN_ACTOR_PIXELS,
    MIN_FLIGHT_CHECK_BUTTON_PIXELS, MIN_FLIGHT_CHECK_DRAWN_PERMILLE, MIN_FLIGHT_CHECK_TEXT_PIXELS,
    MIN_GUN_ROUNDS, MIN_MISSION_SMOKE_SECONDS, MIN_OBJECTIVE_TEXT_PIXELS,
    MIN_SEA_COVERAGE_PERMILLE, MIN_TERRAIN_DETAIL_PERMILLE, MIN_TERRAIN_DISTINCT_COLOURS,
    MIN_TERRAIN_LUMINANCE_SPREAD, MISSION_SMOKE_FRAME_HZ, MissionSmokeHandle, MissionSmokeMode,
    MissionSmokePlugin, MissionSmokeReport, RocketMetrics, SMOKE_KIND, SMOKE_REPORT_FILE,
    SeaMetrics, SmokeMeasurements, TerrainMetrics, WINGMAN_SAMPLE_SECONDS, WingmanSample,
    evaluate as evaluate_mission_smoke, finish as finish_mission_smoke, near_terrain_luminance,
    objective_line_of, region_difference,
};
pub use terminal::{MissionExit, MissionTerminal, MissionTerminalSource};
pub use weapon_present::{
    MISSION_WEAPON_VISUAL_CLAIM, MUZZLE_FLASH_LIFETIME_TICKS, MUZZLE_FLASH_VISUAL_CLAIM,
    MissionHudNotice, MissionNoticeHudPlugin, MissionNoticeLine, MissionWeaponPresentationPlugin,
    MuzzleDrawable, MuzzleFlash, NOTICE_HUD_CAPTION, NOTICE_LIFETIME_TICKS, WeaponVisual,
    WeaponVisualKind, WeaponVisualSample, resolve_muzzle_drawable,
};
pub use weapons::{
    MISSION_FACTION_HOSTILE, MISSION_FACTION_NEUTRAL, MISSION_FACTION_PLAYER, MISSION_FIRE_ROCKETS,
    MISSION_GUN_MOUNT_NOSE, MISSION_GUN_MOUNT_WING_LEFT, MISSION_GUN_MOUNT_WING_RIGHT,
    MISSION_PLAYER_SERIAL, MISSION_ROCKET_MOUNT, MISSION_TARGET_NODE, MISSION_WEAPON_CLAIM,
    MISSION_WIND_CLAIM, MissionCockpitActions, MissionGunHold, MissionLoadout,
    MissionOrdnanceOrders, MissionOrdnanceSession, MissionWeaponError, MissionWeaponIds,
    MissionWeaponReport, MissionWeaponSessions, MissionWeapons, MissionWeaponsPlugin, MissionWind,
    admit_actor, attach_gun_rig, attach_player_rig, collect_mission_cockpit_actions,
    compose_mission_mount_poses, designed_ammunition, designed_interaction_rules, designed_loadout,
    designed_rocket, designed_target_rules, drive_mission_ordnance, ensure_mission_weapons,
    gun_hold_due, guns_key_held, is_fire_guns, is_fire_rockets, mission_ammunition,
    mission_ammunition_id, mission_tick_input, mission_wind, open_composed_mission_weapons,
    open_mission_weapons, publish_mission_weapon_input, rocket_content_id,
};
pub use world_draw::{
    HORIZON_SUBTREE_LABEL, HorizonRest, HorizonSubtree, MissionWorldDrawPlugin, MissionWorldParts,
    WORLD_NEUTRAL_COLOR, WorldDrawReport, WorldDrawSource, WorldWaterArt, WorldWaterLevel,
    bind_sea_to_world_water, build_world_draw, water_level_of,
};
