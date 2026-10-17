//! Acceptance suite M01-PLAY-AI-ARM-ORDER (#1518): the arming pass never
//! registers a fresh generation's actor into the dead generation's weapon
//! session.
//!
//! `mission_host_restart` runs in `PostUpdate` — the stage after the fixed
//! tick — so before #1518 the first fixed tick after a composed restart ran
//! [`arm_mission_ai`] against the session the restart had just replaced:
//! `ensure_mission_weapons` re-opens it only later in the same tick, in
//! `FixedPostUpdate`. Every respawned record took one stale
//! "the weapon session refuses the guns" entry, and
//! [`MissionAiReport::refusals`] — replaced only at spawn — kept the dead
//! entry for the rest of the run.
//!
//! The fix runs the same production staleness check ahead of registering:
//! `arm_mission_ai` calls `ensure_mission_weapons` before it reads the
//! session, so the session it arms into is always the one the host serves.
//! That is the description's option (a) — re-open before the pass — chosen
//! over (b) suppressing the refusal: a session mismatch is a real signal
//! elsewhere, and the same check `FixedPostUpdate` already runs is the
//! single definition of it.
//!
//! This suite drives a composed restart on authored synthetic content — the
//! same entry the playtest reset and a terminal's retry reach — then two
//! fixed ticks, and asserts no weapon-session refusal is latched and every
//! flown record is bound to the fresh session's generation. It runs in CI:
//! every value is authored or designed, nothing is read from the
//! installation and nothing is `verified_original`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use bevy::prelude::{App, Entity, Time};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_ai::{
    MissionAiAirframe, MissionAiAirframes, MissionAiReport, MissionAircraftAi,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHostRestartRequest, MissionStage, MissionWeapons, build_headless, stage_for, teardown,
};
use cs_app::mission_start::{ScriptedAircraft, StoredStartPose, spawn_dispatch};
use cs_app::weapons::WeaponActorBinding;
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;

use crate::common::label;
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// Two fixed ticks after the restart: the first proves the pass armed into
/// the fresh session, the second proves the binding stuck.
const DRIVE_TICKS: u64 = 2;

/// The synthetic record's name; the `_` makes its measured name-prefix
/// dispatch `synthetic`, which is the airframe key the stage carries.
const SYNTHETIC_RECORD: &str = "synthetic_1";

/// The synthetic stage's `player` record's measured team; the flyer's
/// differs, so the designed reading selects the pursuit behaviour on both
/// sides of the restart.
const SYNTHETIC_PLAYER_TEAM: u32 = 1;

/// The synthetic flyer's measured team.
const SYNTHETIC_FLYER_TEAM: u32 = 2;

/// One fixed-tick frame per update, exactly as the AI suites drive the
/// composed loop.
fn drive(app: &mut App, ticks: u64) {
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    for _ in 0..ticks {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }
}

/// The live entity of a scripted aircraft, by measured record name.
fn entity_named(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &cs_app::mission_actors::ScriptedAircraftActor)>();
    query
        .iter(world)
        .find(|(_, actor)| actor.record.name == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{name} spawns with the mission's roster"))
}

/// A synthetic airframe in the measured field shapes, imported through the
/// production `from_values` boundaries — the same tables
/// `m01_play_ai_restart.rs` declares. It proves the ordering, nothing about
/// retail parameters.
fn synthetic_ai_airframe() -> MissionAiAirframe {
    let airframe = OriginalAirframe::from_values(&[
        ("roll_torque", 7.5),
        ("pitch_torque", 3.3),
        ("rudder_torque", 2.0),
        ("level_off_rate", 1.0),
        ("return_rate", 3.0),
        ("ang_momentum_damp", 5.0),
        ("rec_moments_inertia_x", 1.18),
        ("rec_moments_inertia_y", 1.0),
        ("rec_moments_inertia_z", 1.1),
        ("fd_speed", 135.0),
        ("engine_factor", 0.62),
        ("drag_factor", 0.37),
        ("veh_weight", 1900.0),
        ("ref_area", 330.0),
        ("gravity", 20.0),
    ])
    .expect("the declared airframe fields are complete");
    let globals = OriginalGlobals::from_values(&[
        ("nom_gravity", 20.0),
        ("lift_aoa_0_deg", 5.0),
        ("lift_aoa_1_deg", 9.0),
        ("max_aoa_deg", 46.0),
        ("high_g_0", 9.0),
        ("high_g_1", 15.0),
        ("low_g_0", -6.0),
        ("low_g_1", -9.0),
        ("lift_accel_rate", 0.75),
        ("stall_mag", 1.25),
        ("turn_fade_in_mph", 10.0),
        ("turn_fade_out_mph", 50.0),
        ("yaw_low_speed", 0.0625),
        ("yaw_high_speed", 0.17),
        ("yaw_fade_in_mph", 10.0),
        ("yaw_max_mph", 50.0),
        ("yaw_fade_out_mph", 400.0),
    ])
    .expect("the declared globals are complete");
    MissionAiAirframe {
        model: OriginalFlightModel::full(airframe, globals),
        record: "synthetic".to_owned(),
        inheritance_chain: Vec::new(),
        provenance: PROVENANCE_LABEL,
    }
}

/// One synthetic scripted-aircraft record: enabled, awake, of the other
/// team, its stored target the player. Authored in the measured `aiv.zrd`
/// shapes the readers and the spawn consume.
fn synthetic_flyer() -> ScriptedAircraft {
    ScriptedAircraft {
        name: SYNTHETIC_RECORD.to_owned(),
        index: 1,
        field_count: 81,
        netids: None,
        stored_pose: Some(StoredStartPose {
            position: [40.0, 20.0, -60.0],
            heading: 0.0,
        }),
        team: Some(SYNTHETIC_FLYER_TEAM),
        group: None,
        enabled: Some(true),
        primary_target: Some("player".to_owned()),
        title: None,
        starts_asleep: Some(false),
        dispatch: spawn_dispatch(SYNTHETIC_RECORD),
        provenance: Provenance::designed(
            ClaimId::new("test.m01-play-ai-arm-order.flyer").expect("the claim id is valid"),
        ),
    }
}

/// The stage's `player` record, so `player_team_of` has a side to read on
/// both sides of the restart.
fn synthetic_player_record() -> ScriptedAircraft {
    ScriptedAircraft {
        name: "player".to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: Some(StoredStartPose {
            position: [0.0, 20.0, 0.0],
            heading: 0.0,
        }),
        team: Some(SYNTHETIC_PLAYER_TEAM),
        group: None,
        enabled: Some(true),
        primary_target: None,
        title: None,
        starts_asleep: Some(false),
        dispatch: spawn_dispatch("player"),
        provenance: Provenance::designed(
            ClaimId::new("test.m01-play-ai-arm-order.player").expect("the claim id is valid"),
        ),
    }
}

/// A synthetic stage whose roster holds one awake scripted record with an
/// imported airframe and a measured team, beside the `player` record whose
/// measured team splits the roster.
fn arm_order_stage(scratch: &Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.aircraft = vec![synthetic_player_record(), synthetic_flyer()];
    let mut models = BTreeMap::new();
    models.insert("synthetic".to_owned(), synthetic_ai_airframe());
    stage.ai_airframes = MissionAiAirframes {
        models,
        refusals: Vec::new(),
    };
    stage
}

/// **After a composed restart, the first fixed tick already arms into the
/// fresh session: no weapon-session refusal is latched and the record's
/// binding carries the fresh generation.**
///
/// Before the fix the first tick's `arm_mission_ai` saw the session the
/// `PostUpdate` restart had just replaced, recorded one stale
/// "the weapon session refuses the guns" entry for the record, and
/// [`MissionAiReport::refusals`] kept it for the rest of the run — so the
/// `refusals` assertion fails on the unfixed tree.
#[test]
fn accept_m01_play_ai_arm_order_composed_restart_arms_into_the_fresh_session() {
    let scratch = Scratch::new("m01-play-ai-arm-order");
    let stage = arm_order_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    // The spawn-only frame: zero fixed ticks run, so Startup spawns the
    // roster and nothing has stepped it yet.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    // A few committed ticks arm the record through the first generation's
    // session, exactly as the first spawn's arming pass does.
    drive(&mut app, 5);
    assert!(
        app.world()
            .resource::<MissionAiReport>()
            .refusals
            .is_empty(),
        "the first generation arms clean: {:?}",
        app.world().resource::<MissionAiReport>().refusals
    );

    // The composed restart, exactly as the playtest reset and a terminal's
    // retry reach it.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();

    // At least two fixed ticks: the first is the one that previously armed
    // into the dead session; the second proves the fresh binding stuck.
    drive(&mut app, DRIVE_TICKS);

    let report = app.world().resource::<MissionAiReport>();
    assert!(
        report.refusals.is_empty(),
        "no weapon-session refusal survives the first post-restart tick: {:?}",
        report.refusals
    );

    // Every flown record is bound to the session the host now serves.
    let fresh_generation = app.world().resource::<MissionWeapons>().generation();
    let entity = entity_named(&mut app, SYNTHETIC_RECORD);
    assert!(
        app.world().get::<MissionAircraftAi>(entity).is_some(),
        "the respawned record flies, so the arming pass owns its guns"
    );
    let binding = app
        .world()
        .get::<WeaponActorBinding>(entity)
        .expect("an armed record carries its weapon binding");
    assert_eq!(
        binding.generation, fresh_generation,
        "the record's binding is stamped with the fresh session's generation"
    );
}

// ---------------------------------------------------------------------------
// M01's own roster, through the installation (retail)
// ---------------------------------------------------------------------------

/// **On M01's real roster, the post-restart arming pass latches no
/// weapon-session refusal and binds every flown record to the fresh
/// session's generation.**
///
/// `retail` is read access to the owner's installation: the records are
/// M01's own, the restart the composed production entry, the assertions the
/// same as the synthetic leg's — nothing here is `verified_original` and no
/// original executable ran.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_ai_arm_order_retail_restart_binds_every_flown_record_fresh() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    // The spawn-only frame, then a few committed ticks to arm the first
    // generation.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();
    drive(&mut app, 5);

    let flown: Vec<String> = app.world().resource::<MissionAiReport>().flown.clone();
    assert!(
        !flown.is_empty(),
        "M01's awake records fly at the first spawn"
    );

    // The composed restart, then the two fixed ticks the ordering defect
    // lived in.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    drive(&mut app, DRIVE_TICKS);

    let report = app.world().resource::<MissionAiReport>();
    assert!(
        report.refusals.is_empty(),
        "no weapon-session refusal survives the first post-restart tick: {:?}",
        report.refusals
    );
    let fresh_generation = app.world().resource::<MissionWeapons>().generation();
    for name in &flown {
        let entity = entity_named(&mut app, name);
        let binding = app
            .world()
            .get::<WeaponActorBinding>(entity)
            .unwrap_or_else(|| panic!("{name} flies, so the pass armed it"));
        assert_eq!(
            binding.generation, fresh_generation,
            "{name}'s binding is stamped with the fresh session's generation"
        );
    }

    teardown(&mut app);
}
