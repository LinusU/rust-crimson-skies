//! Acceptance suite F29-C.3c (VS-M01-SCORE-ADMIT, #1426): every damage actor
//! the mission host spawns is admitted to the session's score ledger.
//!
//! `docs/findings/2026-10-18-vs-m01-score-admit.md` records the design. The
//! spawn host (`mission_actors`) registers each actor with the session's
//! damage resolver and admits it, from the spawn record it holds, to the
//! `SessionScoreLedger` of the session it serves; a restart admits the rebuilt
//! actors to the fresh ledger it opens.
//!
//! * **Synthetic (CI).** A bound ground target spawns, is admitted at its
//!   spawn pose, and a kill resolved for it scores; a kill for an actor that
//!   was never admitted is refused by name and pays nothing; a restart
//!   admits the rebuilt target to the fresh ledger.
//! * **Retail.** Every damage actor the composed M01 spawns is in the ledger.
//!
//! No original executable ran and nothing here is `verified_original`.

use bevy::prelude::{App, With};
use cs_app::damage::drive_damage_schedule;
use cs_app::damage::{DamageActorBinding, DamageScheduleReport, PendingDamageEvents};
use cs_app::mission_actors::MissionGroundTargetReport;
use cs_app::mission_session::{
    MissionHost, MissionHostRestartReport, MissionHostRestartRequest, MissionStage, build_headless,
    teardown,
};
use cs_app::mission_targets::{BoundTarget, DamageCoupling, MissionTargetBinding};
use cs_app::mission_world_actors::{AllegianceBinding, AllegianceSource};
use cs_app::score::SessionScoreLedger;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{
    AttributionRule, DamageChannel, DamageNodeKey, DamagePolicy, DamageResolver, HitEvent,
    HitEventId, synthetic_airframe_graph,
};
use cs_sim::net_state::{NetActorState, NetLifecycle, NetStateError};
use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

const RESOLVER_PRODUCER: u32 = 1;
const RESOLVED_TICK: u64 = 5;
const TARGET_POSITION_M: [f64; 3] = [120.0, 45.0, -300.0];

/// The synthetic stage with one bound ground target in its host seed.
fn stage_with_target(scratch: &Scratch) -> MissionStage {
    let mut stage = synthetic_stage(scratch.path());
    stage.host.targets = MissionTargetBinding {
        targets: vec![BoundTarget {
            declared: Some(0),
            chain: vec!["t_truck01".to_owned()],
            node: "t_truck01".to_owned(),
            index: 1,
            path: "world1.t_truck01".to_owned(),
            position_m: TARGET_POSITION_M,
            target_point_m: TARGET_POSITION_M,
            target_half_extents_m: [0.0; 3],
            healthy: None,
            destroyed: None,
            intact_slots: Vec::new(),
            destroyed_slots: Vec::new(),
            coupling: DamageCoupling::default(),
            objective: false,
            other_target: false,
            description: None,
            help_label: None,
            allegiance: AllegianceBinding::Measured {
                team: 2,
                source: AllegianceSource::NoBinding,
            },
        }],
        declared: vec![0],
        refusals: Vec::new(),
    };
    stage
}

/// The spawned target's damage actor.
fn target_actor(app: &App) -> ActorId {
    app.world()
        .resource::<MissionGroundTargetReport>()
        .target("t_truck01")
        .expect("the bound target spawned")
        .actor
}

/// Two fresh ids past every serial the spawn host allocated: an attacker the
/// test admits itself, and a victim nobody admits.
fn spare_actors(session: SessionId) -> (ActorId, ActorId) {
    let mut allocator = ActorAllocator::new(session);
    for _ in 0..30 {
        allocator.allocate().expect("the serial space exists");
    }
    (
        allocator.allocate().expect("the serial space exists"),
        allocator.allocate().expect("the serial space exists"),
    )
}

fn hit(session: SessionId, producer: u32, attacker: ActorId, victim: ActorId) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session,
            tick: Tick(RESOLVED_TICK),
            producer,
            sequence: 0,
        },
        Some(attacker),
        victim,
        DamageNodeKey::new("hull").expect("valid node key"),
        DamageChannel::Internal,
        50.0,
    )
    .expect("the fixture hit is valid")
}

/// Resolves the AC01 pair against `victim` and publishes the batch where the
/// session's producers publish theirs, then runs the installed consumer.
fn kill(app: &mut App, session: SessionId, victim: ActorId, attacker: ActorId) {
    let mut resolver = DamageResolver::new(session, RESOLVER_PRODUCER);
    resolver
        .register_actor(
            victim,
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the synthetic victim registers");
    let hits = vec![
        hit(session, RESOLVER_PRODUCER, attacker, victim),
        hit(session, RESOLVER_PRODUCER + 4, attacker, victim),
    ];
    let events = resolver
        .resolve(Tick(RESOLVED_TICK), &hits)
        .expect("valid batch")
        .events;
    app.world_mut()
        .insert_resource(SessionDamageResolver::new(resolver));
    app.world_mut().init_resource::<PendingDamageEvents>();
    app.world_mut()
        .resource_mut::<PendingDamageEvents>()
        .push(hits, events);
    drive_damage_schedule(app.world_mut());
}

fn admit_attacker(app: &mut App, attacker: ActorId) {
    let state = NetActorState::spawn(
        attacker,
        WorldPosition::try_new([0.0, 100.0, 0.0]).expect("finite"),
        Quaternion::IDENTITY,
        400,
    );
    app.world_mut()
        .resource_mut::<SessionScoreLedger>()
        .admit(state)
        .expect("the attacker is admitted once");
}

/// **A ground target the mission host spawns is admitted to its session's
/// ledger at the pose it spawned at, and a kill resolved for it scores.**
#[test]
fn accept_f29_c_3c_a_spawned_actor_is_admitted_and_its_kill_scores() {
    let scratch = Scratch::new("f29-c-3c-admit");
    let stage = stage_with_target(&scratch);
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    let served = app.world().resource::<MissionHost>().served();
    let victim = target_actor(&app);

    let ledger = app.world().resource::<SessionScoreLedger>();
    assert_eq!(ledger.session(), served);
    let state = ledger
        .ledger()
        .state(victim)
        .expect("the spawn host admitted the actor it spawned");
    assert_eq!(state.lifecycle, NetLifecycle::Alive);
    assert_eq!(
        state.pose.position.to_array(),
        TARGET_POSITION_M,
        "the admitted pose is the spawn record's, not a default"
    );
    let points = ledger.ledger().kill_score().points();

    let (attacker, _) = spare_actors(served);
    admit_attacker(&mut app, attacker);
    kill(&mut app, served, victim, attacker);

    let report = app.world().resource::<DamageScheduleReport>();
    let scored = report.score.as_ref().expect("the score consumer ran");
    assert_eq!(scored.awarded(), 1, "{scored:?}");
    assert_eq!(scored.refusals(), &[]);
    assert_eq!(app.world().resource::<SessionScoreLedger>().score(), points);
    teardown(&mut app);
}

/// **A kill for an actor nobody admitted is refused by name and pays
/// nothing.**
#[test]
fn accept_f29_c_3c_an_unadmitted_actor_is_refused_by_name() {
    let scratch = Scratch::new("f29-c-3c-refuse");
    let stage = stage_with_target(&scratch);
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    let served = app.world().resource::<MissionHost>().served();
    let (attacker, stranger) = spare_actors(served);
    admit_attacker(&mut app, attacker);
    kill(&mut app, served, stranger, attacker);

    let scored = app
        .world()
        .resource::<DamageScheduleReport>()
        .score
        .clone()
        .expect("the score consumer ran");
    assert_eq!(scored.awarded(), 0);
    assert_eq!(
        scored.refusals(),
        &[(stranger, NetStateError::UnknownActor { actor: stranger })],
        "the refusal names the actor"
    );
    assert_eq!(app.world().resource::<SessionScoreLedger>().score(), 0);
    teardown(&mut app);
}

/// **A restart admits the rebuilt actors to the fresh ledger it opens.**
#[test]
fn accept_f29_c_3c_a_restart_admits_the_rebuilt_actors_to_the_fresh_ledger() {
    let scratch = Scratch::new("f29-c-3c-restart");
    let stage = stage_with_target(&scratch);
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    let served = app.world().resource::<MissionHost>().served();
    let old = target_actor(&app);

    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    let report = app
        .world_mut()
        .remove_resource::<MissionHostRestartReport>()
        .expect("the composed restart ran");
    assert_ne!(report.served, served);
    let fresh = target_actor(&app);
    assert_eq!(fresh.session, report.served, "the target was rebuilt");
    let ledger = app.world().resource::<SessionScoreLedger>();
    assert_eq!(ledger.session(), report.served);
    assert!(
        ledger.ledger().state(fresh).is_some(),
        "the rebuilt actor is admitted to the fresh ledger"
    );
    assert!(
        ledger.ledger().state(old).is_none(),
        "the torn-down generation's actor is not carried over"
    );
    teardown(&mut app);
}

/// **Retail M01: every damage actor the composition spawns is in the
/// ledger.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f29_c_3c_retail_every_damage_actor_is_admitted() {
    use crate::campaign::common::label;
    use crate::campaign::vs_m01_rt_window::game_dir;
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::stage_for;

    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    app.update();
    app.update();
    let actors: Vec<ActorId> = {
        let world = app.world_mut();
        world
            .query_filtered::<&DamageActorBinding, With<DamageActorBinding>>()
            .iter(world)
            .map(|binding| binding.actor)
            .collect()
    };
    assert!(!actors.is_empty(), "M01 spawns damage actors");
    let ledger = app.world().resource::<SessionScoreLedger>();
    for actor in actors {
        assert!(
            ledger.ledger().state(actor).is_some(),
            "{actor:?} was spawned with a damage binding and never admitted"
        );
    }
    teardown(&mut app);
}
