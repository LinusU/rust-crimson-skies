//! Acceptance scenario F35-C at the boundary: the declared launch bay's
//! socket and capacity — the two fields F35-A deliberately parked — lower into
//! the runtime launch rig the session set actually consumes, and a lowered
//! ship schedules, releases and cancels an aircraft end to end.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-C`. Task test prefix: `accept_f35_c_`.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_app::capital::lower_capital_ship;
use cs_content::capital::{
    CapitalSubsystemEffect, CapitalSubsystemKey, CapitalSubsystemKind, DeclaredCapitalParts,
    DeclaredCapitalShip, DeclaredLaunchBay, DeclaredSubsystem, declared_synthetic_capital_ship,
};
use cs_script::ir::ActorId;
use cs_sim::capital::{
    CapitalHit, CapitalRuntimeError, CapitalShipEvent, CapitalShipSet, HitOutcome, LaunchRefusal,
    SubsystemKey,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const CARRIER: ActorId = ActorId(1);

fn key(name: &str) -> SubsystemKey {
    SubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn declared_key(name: &str) -> CapitalSubsystemKey {
    CapitalSubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f35c.test.boundary").expect("valid")),
    ))
}

fn designed_owner(value: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Faction, value).expect("valid"),
        Provenance::designed(ClaimId::new("f35c.test.boundary").expect("valid")),
    ))
}

fn hit(target: ActorId, subsystem: &str, damage: f64, at: u64) -> CapitalHit {
    CapitalHit::try_new(target, key(subsystem), damage, Tick(at)).expect("a valid hit")
}

fn approx(a: [f64; 3], b: [f64; 3]) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-9, "component {i}: {a:?} vs {b:?}");
    }
}

fn fighter() -> ContentId {
    ContentId::from_source(ContentKind::Airframe, "synthetic.interceptor").expect("valid")
}

/// The declared fixture lowers with the two parked launch fields intact: the
/// launch bay carries its socket and capacity verbatim, a weapon bay carries
/// no launch rig, and the runtime reads both.
#[test]
fn accept_f35_c_declared_socket_and_capacity_lower_into_the_runtime_rig() {
    let declared = declared_synthetic_capital_ship();
    let ship = lower_capital_ship(CARRIER, &declared).expect("the fixture lowers");

    let DeclaredLaunchBay {
        socket_offset_m,
        capacity,
        ..
    } = &declared.launch_bays()[0];
    let rig = ship
        .launch_rig(&key("launch_bay_1"))
        .expect("the rig lowers");
    assert_eq!(
        rig.offset_m, *socket_offset_m,
        "the socket offset lowers field-wise, provenance included"
    );
    assert_eq!(
        rig.capacity, *capacity,
        "the declared capacity lowers field-wise"
    );
    let Resolved::Known(capacity) = &rig.capacity else {
        panic!("the fixture's launch capacity is known");
    };
    assert_eq!(capacity.value, 4);

    // A weapon bay is not a hangar: it carries no rig at all.
    assert_eq!(
        ship.launch_rig(&key("weapon_bay_1")),
        None,
        "a weapon bay never spawns aircraft"
    );
    assert_eq!(
        ship.launch_rigs(),
        1,
        "exactly one declared launch bay lowered a rig"
    );
}

/// A lowered ship drives the F35-C release path end to end: it schedules
/// against the declared capacity, releases once from the declared socket with
/// the carrier's committed motion, and cancels — without a duplicate — when
/// the bay is destroyed underneath the pending launch.
#[test]
fn accept_f35_c_lowered_ship_releases_once_and_cancels_on_bay_destruction() {
    let declared = declared_synthetic_capital_ship();
    let mut set = CapitalShipSet::new(10).expect("the declared rate");
    set.register(
        lower_capital_ship(CARRIER, &declared).expect("lowers"),
        None,
    )
    .expect("registers");

    // The declared cargo capacity reached the runtime hold.
    let Resolved::Known(cargo) = set.cargo_capacity(CARRIER).expect("known") else {
        panic!("the fixture's cargo capacity is known");
    };
    assert_eq!(cargo.value, 5_000.0);

    // The declared launch bay cycle is exposed 35–79.
    let doomed = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(40),
            [0.0, 0.0, 2.0],
        )
        .expect("the lowered rig accepts a launch");
    set.advance_to(Tick(35)).expect("the bay is exposed");
    assert!(matches!(
        set.launch_status(CARRIER, &doomed).expect("known"),
        Some(cs_sim::capital::LaunchStatus::Pending { .. })
    ));

    // Destroy the bay on the exposed tick, before the launch's ready tick.
    assert!(matches!(
        set.apply_hit(&hit(CARRIER, "launch_bay_1", 10.0, 35))
            .expect("resolves"),
        HitOutcome::Destroyed { .. }
    ));
    let events = set.advance_to(Tick(36)).expect("advances");
    assert!(
        events.iter().any(|event| matches!(
            event,
            CapitalShipEvent::LaunchCancelled {
                carrier,
                id,
                reason: cs_sim::capital::LaunchCancelReason::BayDestroyed,
                at: Tick(36),
            } if *carrier == CARRIER && *id == doomed
        )),
        "the waiting launch is cancelled, got {events:?}"
    );

    // The rest of the session never spawns it — not even at its ready tick.
    let rest = set.advance_to(Tick(200)).expect("advances");
    assert!(
        rest.iter()
            .all(|event| !matches!(event, CapitalShipEvent::LaunchReleased { .. })),
        "no aircraft appears for a launch whose bay was destroyed: {rest:?}"
    );
    assert!(
        set.launch_status(CARRIER, &doomed)
            .expect("known")
            .expect("issued")
            .released()
            .is_none()
    );

    // The control: a second carrier carrying the same declared launch bay
    // releases the same schedule exactly once, at the declared socket.
    let mut control = CapitalShipSet::new(10).expect("valid");
    control
        .register(
            lower_capital_ship(CARRIER, &declared).expect("lowers"),
            None,
        )
        .expect("registers");
    let live = control
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(40),
            [0.0, 0.0, 2.0],
        )
        .expect("schedules");
    let events = control.advance_to(Tick(40)).expect("advances");
    let released = events
        .iter()
        .filter_map(|event| match event {
            CapitalShipEvent::LaunchReleased { id, aircraft, .. } if *id == live => Some(*aircraft),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(released.len(), 1, "one release per id, got {events:?}");
    // The declared socket is [0, -5, 0] in the body frame and the declared
    // course is 20 m/s, so tick 40 puts the aircraft 80 m along +X, 5 m below
    // the hull, carrying the carrier's velocity plus the ejection.
    approx(released[0].position_m, [80.0, -5.0, 0.0]);
    approx(released[0].velocity_m_s, [20.0, 0.0, 2.0]);
    assert!(released[0].dynamic_authority);
    assert_eq!(released[0].tick, Tick(40));
    assert_ne!(released[0].actor, CARRIER);
    assert_eq!(
        control.release_launch(CARRIER, &live),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::AlreadyReleased)),
        "the one release is the only one that can ever exist"
    );
}

/// An unmeasured declared socket or capacity lowers verbatim and is refused by
/// claim at runtime: the boundary never invents a transform or an unbounded
/// hangar to make a launch work.
#[test]
fn accept_f35_c_unknown_declared_launch_wiring_refuses_by_claim() {
    let declared = DeclaredCapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.leviathan").expect("valid"),
        Origin::SyntheticFixture,
        Provenance::designed(ClaimId::new("f35c.test.boundary").expect("valid")),
        None,
        DeclaredCapitalParts {
            subsystems: vec![DeclaredSubsystem {
                key: declared_key("launch_bay_1"),
                kind: CapitalSubsystemKind::LaunchBay,
                effect: Some(CapitalSubsystemEffect::Launching),
                lethal: false,
            }],
            engines: vec![],
            weapon_bays: vec![],
            launch_bays: vec![DeclaredLaunchBay {
                key: declared_key("launch_bay_1"),
                exposure: cs_content::capital::DeclaredExposure {
                    concealed_ticks: 30,
                    opening_ticks: 5,
                    exposed_ticks: 45,
                    closing_ticks: 5,
                },
                socket_offset_m: Resolved::Unknown {
                    claim_id: ClaimId::new("f35c.test.boundary.socket").expect("valid"),
                    reason: "launch socket unmeasured".to_owned(),
                },
                capacity: Resolved::Unknown {
                    claim_id: ClaimId::new("f35c.test.boundary.capacity").expect("valid"),
                    reason: "launch capacity unmeasured".to_owned(),
                },
            }],
            turrets: vec![],
            docking_anchors: vec![],
            sections: vec![],
        },
        designed(0.0),
        designed_owner("synthetic.raiders"),
    )
    .expect("unknown launch wiring is a valid declared record");

    let ship = lower_capital_ship(ActorId(7), &declared).expect("lowers");
    let rig = ship
        .launch_rig(&key("launch_bay_1"))
        .expect("the rig lowers");
    assert!(
        matches!(rig.offset_m, Resolved::Unknown { .. }),
        "the unknown socket lowers verbatim"
    );
    assert!(
        matches!(rig.capacity, Resolved::Unknown { .. }),
        "the unknown capacity lowers verbatim"
    );

    // A course-less ship registers on a pose, so the refusal is the only thing
    // under test.
    let mut set = CapitalShipSet::new(10).expect("valid");
    set.register(
        ship,
        Some(cs_sim::world_actors::trajectory::Pose {
            position_m: [0.0; 3],
            orientation: cs_sim::world_actors::Quat::IDENTITY,
            velocity_m_s: [0.0; 3],
            angular_velocity_rad_s: [0.0; 3],
        }),
    )
    .expect("a course-less ship registers on a pose");

    let refused = set
        .schedule_launch(
            ActorId(7),
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            [0.0; 3],
        )
        .expect_err("an unmeasured socket refuses");
    let CapitalRuntimeError::Launch(LaunchRefusal::SocketUnknown { claim_id, reason }) = refused
    else {
        panic!("the unknown socket refuses by claim, got {refused:?}");
    };
    assert_eq!(claim_id.as_str(), "f35c.test.boundary.socket");
    assert_eq!(reason, "launch socket unmeasured");
    assert!(
        set.pending_launches(ActorId(7)).expect("known").is_empty(),
        "a refused schedule leaves the ledger empty"
    );
    assert!(
        set.advance_to(Tick(200))
            .expect("advances")
            .iter()
            .all(|event| !matches!(event, CapitalShipEvent::LaunchReleased { .. })),
        "nothing spawns from an unmeasured socket"
    );
}
