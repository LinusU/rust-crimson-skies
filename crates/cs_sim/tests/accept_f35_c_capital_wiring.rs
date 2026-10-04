//! F35-C acceptance: the launch, capture, cargo and staged-destruction
//! wiring the session set drives (synthetic).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`. Ordinary build/test only; nothing
//! here is original data.
//!
//! The minimum scenario is
//! [`accept_f35_c_destroyed_launch_bay_leaves_no_duplicate_aircraft`]: a
//! launch whose ready tick has already arrived is destroyed with its bay
//! before it can run, and no aircraft ever appears for it — while a ship
//! whose bay stays intact releases that same launch exactly once.
//!
//! Every test drives production code — [`CapitalShipSet`]'s tick pass and its
//! commands, the [`CapitalShip`] aggregate, the cargo ledger — so removing the
//! cancellation branch, the release pass, the ownership commit, the capacity
//! bound or the despawn gate fails an assertion or fails to compile.

use cs_script::ir::ActorId;
use cs_sim::capital::{
    Bay, BayKind, CapitalHit, CapitalParts, CapitalRuntimeError, CapitalShip, CapitalShipEvent,
    CapitalShipSet, CaptureRefusal, CaptureStage, CargoRefusal, DespawnPolicy, DestructionState,
    ExposureWindow, HitOutcome, LaunchBayRig, LaunchCancelReason, LaunchCancellation, LaunchId,
    LaunchRefusal, LaunchStatus, SYNTHETIC_LAUNCH_EJECT_M_S, Subsystem, SubsystemEffect,
    SubsystemGraph, SubsystemKey, SubsystemKind, SubsystemState, synthetic_capital_ownership,
    synthetic_capital_ship, synthetic_capital_trajectory,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

const CARRIER: ActorId = ActorId(1);
/// The claimant that captures in the capture tests.
const CLAIMANT: ActorId = ActorId(2);
/// A second carrier, used to prove released ids never collide with a ship id.
const OTHER_CARRIER: ActorId = ActorId(5);

fn key(name: &str) -> SubsystemKey {
    SubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f35c.test").expect("valid")),
    ))
}

fn unknown<T>(value: &str) -> Resolved<T> {
    Resolved::Unknown {
        claim_id: ClaimId::new(value).expect("valid"),
        reason: format!("{value} unmeasured"),
    }
}

fn fighter() -> ContentId {
    ContentId::from_source(ContentKind::Airframe, "synthetic.interceptor").expect("valid")
}

fn navy() -> ContentId {
    ContentId::from_source(ContentKind::Faction, "synthetic.navy").expect("valid")
}

fn registered_set() -> CapitalShipSet {
    let mut set = CapitalShipSet::new(10).expect("valid rate");
    set.register(synthetic_capital_ship(), None)
        .expect("the fixture registers");
    set
}

fn hit(target: ActorId, subsystem: &str, damage: f64, at: u64) -> CapitalHit {
    CapitalHit::try_new(target, key(subsystem), damage, Tick(at)).expect("a valid hit")
}

fn approx(a: [f64; 3], b: [f64; 3], eps: f64) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < eps, "component {i}: {a:?} vs {b:?}");
    }
}

/// The releases of one launch id. A [`LaunchId`] is stable within its
/// carrier's ledger, so events are matched by carrier *and* id.
fn released_of<'a>(
    events: &'a [CapitalShipEvent],
    carrier: ActorId,
    id: &LaunchId,
) -> Vec<&'a CapitalShipEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                CapitalShipEvent::LaunchReleased {
                    carrier: from,
                    id: released,
                    ..
                } if *from == carrier && released == id
            )
        })
        .collect()
}

/// The cancellations of one launch id.
fn cancelled_of<'a>(
    events: &'a [CapitalShipEvent],
    carrier: ActorId,
    id: &LaunchId,
) -> Vec<&'a CapitalShipEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                CapitalShipEvent::LaunchCancelled {
                    carrier: from,
                    id: dead,
                    ..
                } if *from == carrier && dead == id
            )
        })
        .collect()
}

/// A ship carrying only the parts a test needs: one launch bay with the given
/// rig, one lethal keel, the given cargo capacity and course.
fn rigged_ship(
    actor: ActorId,
    rig: Option<LaunchBayRig>,
    cargo: Resolved<f64>,
    trajectory: Option<cs_sim::world_actors::trajectory::Trajectory>,
) -> CapitalShip {
    let mut graph = vec![
        Subsystem::new(key("keel"), SubsystemKind::StructuralSection)
            .with_effect(SubsystemEffect::MissionCondition)
            .with_lethal(true),
    ];
    let mut bays = Vec::new();
    if let Some(rig) = rig {
        graph.push(
            Subsystem::new(key("launch_bay_1"), SubsystemKind::LaunchBay)
                .with_effect(SubsystemEffect::Launching),
        );
        let bay = Bay::new(
            key("launch_bay_1"),
            BayKind::Launch,
            ExposureWindow::try_new(30, 5, 45, 5).expect("valid window"),
        );
        bays.push(bay.with_launch_rig(rig));
    }
    CapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.carrier").expect("valid"),
        actor,
        CapitalParts {
            graph: SubsystemGraph::try_new(graph).expect("valid graph"),
            engines: vec![],
            bays,
            turrets: vec![],
            docking_anchors: vec![],
            sections: vec![],
            cargo,
            ownership: synthetic_capital_ownership(),
            trajectory,
        },
    )
    .expect("the test ship is valid")
}

fn known_rig(offset_m: [f64; 3], capacity: u32) -> LaunchBayRig {
    LaunchBayRig {
        offset_m: Resolved::Known(Known::new(
            offset_m,
            Provenance::designed(ClaimId::new("f35c.test.rig").expect("valid")),
        )),
        capacity: Resolved::Known(Known::new(
            capacity,
            Provenance::designed(ClaimId::new("f35c.test.rig").expect("valid")),
        )),
    }
}

// ------------------------------------------- AC03 minimum scenario ----------

/// The minimum scenario: a launch bay is destroyed while a launch waits on it
/// and no aircraft ever appears for that launch — the id resolves as
/// cancelled, once, and can never be released afterwards. A control ship
/// carrying the identical schedule with an intact bay releases that launch
/// exactly once, so the destruction is the only difference between the two.
#[test]
fn accept_f35_c_destroyed_launch_bay_leaves_no_duplicate_aircraft() {
    let mut set = registered_set();

    // The synthetic launch bay's authored cycle is concealed 0–29, opening
    // 30–34, exposed 35–79, closing 80–84.
    let id = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(40),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("a launch bay with a rig accepts a launch");

    // Not yet ready and the bay is still shut: nothing releases.
    let early = set.advance_to(Tick(34)).expect("advances");
    assert!(released_of(&early, CARRIER, &id).is_empty());
    assert!(matches!(
        set.launch_status(CARRIER, &id).expect("known"),
        Some(LaunchStatus::Pending { .. })
    ));

    // The bay is exposed at tick 35, before the launch is ready at 40, so the
    // bay is destroyed while the launch waits: its own ready tick arrives with
    // nothing left to launch from.
    set.advance_to(Tick(35)).expect("exposed");
    assert_eq!(
        set.ship(CARRIER)
            .expect("registered")
            .bay_state(&key("launch_bay_1"), Tick(35)),
        Some(cs_sim::capital::BayState::Exposed)
    );
    let outcome = set
        .apply_hit(&hit(CARRIER, "launch_bay_1", 10.0, 35))
        .expect("a landed hit resolves");
    assert!(matches!(outcome, HitOutcome::Destroyed { .. }));
    assert_eq!(
        set.ship(CARRIER)
            .expect("registered")
            .subsystem_state(&key("launch_bay_1")),
        Some(SubsystemState::Disabled)
    );

    // The next pass cancels the waiting launch. Its ready tick has not passed
    // yet, and the destruction is the only reason it will not spawn.
    let teardown = set.advance_to(Tick(36)).expect("advances");
    assert_eq!(
        cancelled_of(&teardown, CARRIER, &id).as_slice(),
        &[&CapitalShipEvent::LaunchCancelled {
            carrier: CARRIER,
            id: id.clone(),
            reason: LaunchCancelReason::BayDestroyed,
            at: Tick(36),
        }]
    );
    assert!(released_of(&teardown, CARRIER, &id).is_empty());
    assert_eq!(
        set.launch_status(CARRIER, &id).expect("known"),
        Some(LaunchStatus::Cancelled {
            cancellation: LaunchCancellation {
                launch: cs_sim::capital::PendingLaunch {
                    id: id.clone(),
                    aircraft: fighter(),
                    ready_tick: Tick(40),
                },
                reason: LaunchCancelReason::BayDestroyed,
                at: Tick(36),
            },
        })
    );
    assert!(
        set.pending_launches(CARRIER).expect("known").is_empty(),
        "a cancelled launch leaves the ledger"
    );
    assert!(
        set.launch_status(CARRIER, &id)
            .expect("known")
            .expect("issued")
            .released()
            .is_none(),
        "no aircraft was ever produced for a cancelled launch"
    );

    // Nothing releases it later, however often the bay's cycle comes round —
    // in particular not at tick 40, when its own ready tick arrives.
    let later = set.advance_to(Tick(300)).expect("advances");
    assert!(released_of(&later, CARRIER, &id).is_empty());
    assert!(cancelled_of(&later, CARRIER, &id).is_empty());

    // The retry door refuses it, and the destroyed bay accepts no new launch.
    assert_eq!(
        set.release_launch(CARRIER, &id),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::Cancelled))
    );
    assert_eq!(
        set.schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(400),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        ),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::BayDestroyed))
    );

    // Control: the identical schedule on a ship whose bay stays intact releases
    // exactly once, at its ready tick, carrying the carrier's committed pose
    // and motion.
    let mut control = registered_set();
    let live = control
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(40),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules");
    let events = control.advance_to(Tick(40)).expect("advances");
    let released = released_of(&events, CARRIER, &live);
    assert_eq!(released.len(), 1, "one release per id, got {released:?}");
    let CapitalShipEvent::LaunchReleased { aircraft, at, .. } = released[0] else {
        panic!("a release event carries the spawn state, got {released:?}");
    };
    assert_eq!(*at, Tick(40));
    // Tick 40 of the authored 20 m/s course: 80 m along +X, the socket 5 m
    // below the hull, and the carrier's velocity plus the designed ejection.
    approx(aircraft.position_m, [80.0, -5.0, 0.0], 1e-9);
    approx(aircraft.velocity_m_s, [20.0, 0.0, 2.0], 1e-9);
    assert_eq!(aircraft.tick, Tick(40));
    assert!(
        aircraft.dynamic_authority,
        "authority is granted once, here"
    );
    assert_ne!(aircraft.actor, CARRIER, "an aircraft is never the carrier");
    assert_eq!(
        control.launch_status(CARRIER, &live).expect("known"),
        Some(LaunchStatus::Released {
            aircraft: *aircraft
        })
    );

    // Releasing the same id again — by hand or by any later tick — is refused,
    // so the one aircraft above is the only one that can ever exist.
    assert_eq!(
        control.release_launch(CARRIER, &live),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::AlreadyReleased))
    );
    let rest = control.advance_to(Tick(400)).expect("advances");
    assert!(
        released_of(&rest, CARRIER, &live).is_empty(),
        "a released id never releases again"
    );
}

/// Two launches on one carrier each produce their own fresh aircraft, and the
/// manual release door obeys the same bay-open and ready-tick gate the tick
/// pass does — which is what makes retrying it safe.
#[test]
fn accept_f35_c_launch_releases_once_per_id_with_session_fresh_actor_ids() {
    let mut set = registered_set();

    let first = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules");
    let second = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(36),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules");
    assert_ne!(first, second, "ids are never reused");

    // The retry is not ready yet, and a manual release refuses with exactly
    // the reason the pass would have waited for.
    let events = set.advance_to(Tick(35)).expect("advances");
    let first_release = released_of(&events, CARRIER, &first);
    assert_eq!(first_release.len(), 1);
    assert!(released_of(&events, CARRIER, &second).is_empty());
    assert_eq!(
        set.release_launch(CARRIER, &second),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::NotReady {
            ready_tick: Tick(36)
        }))
    );
    assert!(
        matches!(
            set.launch_status(CARRIER, &second).expect("known"),
            Some(LaunchStatus::Pending { .. })
        ),
        "a refused retry leaves the launch waiting"
    );

    // On its ready tick the pass releases it — one aircraft, one id, and never
    // the same aircraft twice.
    let events = set.advance_to(Tick(36)).expect("advances");
    let second_release = released_of(&events, CARRIER, &second);
    assert_eq!(second_release.len(), 1);
    let CapitalShipEvent::LaunchReleased {
        aircraft: from_pass,
        ..
    } = first_release[0]
    else {
        panic!("a release event carries the spawn state");
    };
    let CapitalShipEvent::LaunchReleased {
        aircraft: also_from_pass,
        ..
    } = second_release[0]
    else {
        panic!("a release event carries the spawn state");
    };
    assert_ne!(
        from_pass.actor, also_from_pass.actor,
        "each release is a distinct aircraft"
    );
    approx(from_pass.position_m, [70.0, -5.0, 0.0], 1e-9);

    // A launch scheduled inside an open window and released by hand beats the
    // next pass by a tick — and the pass then finds nothing left to release.
    let events = set.advance_to(Tick(120)).expect("the bay opens again");
    assert!(released_of(&events, CARRIER, &first).is_empty());
    let manual = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(120),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules into the open bay");
    let released_by_hand = set.release_launch(CARRIER, &manual).expect("releases");
    approx(released_by_hand.position_m, [240.0, -5.0, 0.0], 1e-9);
    let events = set.advance_to(Tick(121)).expect("advances");
    assert!(
        released_of(&events, CARRIER, &manual).is_empty(),
        "a launch the caller already released is not released by the pass"
    );
    assert_eq!(
        set.launch_status(CARRIER, &manual).expect("known"),
        Some(LaunchStatus::Released {
            aircraft: released_by_hand
        })
    );

    // A launch that waits for a shut bay is released when the bay next opens.
    // Tick 170 is the cycle's first concealed tick (170 % 85 == 0).
    set.advance_to(Tick(170))
        .expect("the bay is concealed again");
    let deferred = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(170),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules");
    assert_eq!(
        set.release_launch(CARRIER, &deferred),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::NotOpen))
    );
    let events = set.advance_to(Tick(205)).expect("the bay opens again");
    assert_eq!(
        released_of(&events, CARRIER, &deferred).len(),
        1,
        "the waiting launch runs when its bay next opens"
    );

    // A second carrier registered later cannot collide with the ids already
    // handed out: the allocator only ever climbs.
    let mut spaced = CapitalShipSet::new(10).expect("valid");
    spaced
        .register(
            rigged_ship(
                CARRIER,
                Some(known_rig([0.0, -5.0, 0.0], 4)),
                designed(0.0),
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    spaced
        .register(
            rigged_ship(
                OTHER_CARRIER,
                Some(known_rig([0.0, -5.0, 0.0], 4)),
                designed(0.0),
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    let mut ids = Vec::new();
    for carrier in [CARRIER, OTHER_CARRIER] {
        ids.push(
            spaced
                .schedule_launch(
                    carrier,
                    &key("launch_bay_1"),
                    fighter(),
                    Tick(35),
                    SYNTHETIC_LAUNCH_EJECT_M_S,
                )
                .expect("schedules"),
        );
    }
    let events = spaced.advance_to(Tick(35)).expect("advances");
    let mut actors = Vec::new();
    for (carrier, id) in [(CARRIER, &ids[0]), (OTHER_CARRIER, &ids[1])] {
        let released = released_of(&events, carrier, id);
        assert_eq!(released.len(), 1, "{id:?} released exactly once");
        let CapitalShipEvent::LaunchReleased { aircraft, .. } = released[0] else {
            panic!("a release event carries the spawn state");
        };
        actors.push(aircraft.actor);
    }
    actors.sort_unstable();
    assert_eq!(
        actors,
        vec![ActorId(6), ActorId(7)],
        "released ids start above every registered ship id and never repeat"
    );
}

/// Capacity bounds the launches *waiting* aboard a bay, an unmeasurable socket
/// or capacity refuses by claim, and neither an unknown rig nor a malformed
/// ejection may produce an aircraft.
#[test]
fn accept_f35_c_launch_capacity_and_unknown_rigs_refuse_by_claim() {
    let mut set = registered_set();
    // The synthetic launch bay declares a capacity of four.
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(
            set.schedule_launch(
                CARRIER,
                &key("launch_bay_1"),
                fighter(),
                Tick(35),
                SYNTHETIC_LAUNCH_EJECT_M_S,
            )
            .expect("the bay holds four"),
        );
    }
    assert_eq!(
        set.schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        ),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::BayFull {
            capacity: 4,
            pending: 4,
        }))
    );

    // A released aircraft has left the bay, so its slot is free again: the
    // four waiting launches all ran at tick 35, and the fifth now fits.
    let first_batch = set.advance_to(Tick(35)).expect("advances");
    for id in &ids {
        assert_eq!(
            released_of(&first_batch, CARRIER, id).len(),
            1,
            "{id:?} released once"
        );
    }
    let fifth = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("the released slot is free");
    assert_eq!(fifth.sequence, 4, "a refused schedule spends no id");

    // A weapon bay, a section and a key that names nothing are all not launch
    // bays of this ship.
    for bay in ["weapon_bay_1", "keel", "engine_1"] {
        assert_eq!(
            set.schedule_launch(
                CARRIER,
                &key(bay),
                fighter(),
                Tick(35),
                SYNTHETIC_LAUNCH_EJECT_M_S,
            ),
            Err(CapitalRuntimeError::Launch(LaunchRefusal::UnknownBay)),
            "{bay} is not a launch bay"
        );
    }
    assert_eq!(
        set.schedule_launch(
            ActorId(99),
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        ),
        Err(CapitalRuntimeError::UnknownShip(ActorId(99)))
    );
    assert_eq!(
        set.schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            [f64::NAN, 0.0, 0.0],
        ),
        Err(CapitalRuntimeError::Launch(
            LaunchRefusal::NonFiniteEjection
        ))
    );
    assert!(
        set.pending_launches(CARRIER).expect("known").len() == 1,
        "every refusal left the ledger untouched"
    );

    // An unmeasured socket: nothing is scheduled into a bay with no transform.
    let mut unknown_socket = CapitalShipSet::new(10).expect("valid");
    let socket_ship = ActorId(3);
    unknown_socket
        .register(
            rigged_ship(
                socket_ship,
                Some(LaunchBayRig {
                    offset_m: unknown("launch-socket"),
                    capacity: Resolved::Known(Known::new(
                        2,
                        Provenance::designed(ClaimId::new("f35c.test.rig").expect("valid")),
                    )),
                }),
                designed(0.0),
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    let refused = unknown_socket
        .schedule_launch(
            socket_ship,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect_err("an unmeasured socket refuses");
    let CapitalRuntimeError::Launch(LaunchRefusal::SocketUnknown { claim_id, reason }) = refused
    else {
        panic!("the unknown socket refuses by claim, got {refused:?}");
    };
    assert_eq!(claim_id.as_str(), "launch-socket");
    assert_eq!(reason, "launch-socket unmeasured");

    // An unmeasured capacity: an unbounded hangar is never assumed either.
    let mut unknown_capacity = CapitalShipSet::new(10).expect("valid");
    let capacity_ship = ActorId(4);
    unknown_capacity
        .register(
            rigged_ship(
                capacity_ship,
                Some(LaunchBayRig {
                    offset_m: Resolved::Known(Known::new(
                        [0.0, -5.0, 0.0],
                        Provenance::designed(ClaimId::new("f35c.test.rig").expect("valid")),
                    )),
                    capacity: unknown("launch-capacity"),
                }),
                designed(0.0),
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    let refused = unknown_capacity
        .schedule_launch(
            capacity_ship,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect_err("an unmeasured capacity refuses");
    assert!(
        matches!(
            refused,
            CapitalRuntimeError::Launch(LaunchRefusal::CapacityUnknown { .. })
        ),
        "got {refused:?}"
    );
    assert!(
        unknown_capacity
            .pending_launches(capacity_ship)
            .expect("known")
            .is_empty()
    );
    assert!(
        unknown_capacity
            .advance_to(Tick(200))
            .expect("advances")
            .iter()
            .all(|event| !matches!(event, CapitalShipEvent::LaunchReleased { .. })),
        "no aircraft leaves a bay whose wiring is unknown"
    );
}

// ------------------------------------------------------------- capture ------

/// Capture is staged and its commit is one step: the latch hands the controls
/// to the claimant's owner while the guns stay with the old one, and the
/// completing stage moves ownership, guns, AI and docking together.
#[test]
fn accept_f35_c_capture_switches_ownership_guns_and_control_in_one_step() {
    let mut set = registered_set();
    let raiders = synthetic_capital_ownership().owner;

    // Before any capture the ship acts for its own owner and its docking
    // anchor is intact.
    let control = set.control(CARRIER).expect("known");
    assert_eq!(control.control_owner, raiders);
    assert_eq!(control.guns_owner, raiders);
    assert!(control.docking_open);

    let ticket = set
        .begin_capture(7, CARRIER, CLAIMANT, navy())
        .expect("the mission may attempt a capture");
    assert_eq!(ticket.attempt, 1);
    assert_eq!(ticket.session, 7);
    assert_eq!(
        set.capture_stage(CARRIER).expect("known"),
        CaptureStage::Approaching
    );
    assert_eq!(
        set.begin_capture(7, CARRIER, CLAIMANT, navy()),
        Err(CapitalRuntimeError::Capture(CaptureRefusal::InProgress {
            ship: CARRIER,
            claimant: CLAIMANT,
            stage: CaptureStage::Approaching,
        })),
        "one attempt holds a ship at a time"
    );

    // Approaching and eligible: nothing has moved.
    assert_eq!(
        set.advance_capture(&ticket).expect("advances"),
        CaptureStage::Eligible
    );
    assert_eq!(
        set.advance_capture(&ticket).expect("advances"),
        CaptureStage::Latching
    );
    let latched = set.control(CARRIER).expect("known");
    assert_eq!(
        latched.control_owner,
        navy(),
        "the latch puts a dedicated control owner in"
    );
    assert_eq!(
        latched.guns_owner, raiders,
        "the ship's guns are not turned until the capture commits"
    );
    assert!(
        !latched.is_coherent_for(&navy()),
        "a half-switched ship is never coherent for one owner"
    );
    assert_eq!(
        set.ship(CARRIER).expect("registered").ownership().owner,
        raiders,
        "ownership moves only at completion"
    );
    assert_eq!(
        set.advance_capture(&ticket).expect("advances"),
        CaptureStage::Transferring
    );

    // The completing stage switches everything at once.
    assert_eq!(
        set.advance_capture(&ticket).expect("commits"),
        CaptureStage::Completed
    );
    let ownership = set.ship(CARRIER).expect("registered").ownership();
    assert_eq!(ownership.owner, navy());
    assert!(ownership.captured, "a captured owner is marked as such");
    let control = set.control(CARRIER).expect("known");
    assert!(control.is_coherent_for(&navy()), "guns and control agree");
    assert!(control.docking_open, "the intact anchor still boards");

    // The finished attempt holds the ship no longer.
    assert_eq!(set.capture(CARRIER).expect("known"), None);
    assert_eq!(
        set.capture_stage(CARRIER),
        Err(CapitalRuntimeError::NoCapture { actor: CARRIER })
    );
    assert_eq!(
        set.advance_capture(&ticket),
        Err(CapitalRuntimeError::NoCapture { actor: CARRIER }),
        "a completed ticket can never commit again"
    );

    // The docking gate reads the ship's own anchors: destroying one closes it.
    set.disable(CARRIER, &key("docking_anchor_1"))
        .expect("a known anchor");
    assert!(
        !set.control(CARRIER).expect("known").docking_open,
        "a destroyed anchor closes docking eligibility"
    );
}

/// An aborted attempt frees the ship for a retry and stays stale forever: a
/// delayed callback carrying the old ticket cannot commit ownership, and a
/// ticket from another session is refused outright.
#[test]
fn accept_f35_c_capture_abort_frees_a_retry_and_keeps_the_old_ticket_stale() {
    let mut set = registered_set();
    let raiders = synthetic_capital_ownership().owner;

    let first = set
        .begin_capture(7, CARRIER, CLAIMANT, navy())
        .expect("the first attempt");
    set.advance_capture(&first).expect("eligible");
    assert_eq!(
        set.abort_capture(&first).expect("aborts"),
        CaptureStage::Aborted
    );
    assert_eq!(
        set.ship(CARRIER).expect("registered").ownership().owner,
        raiders,
        "an abort never transfers ownership"
    );
    assert_eq!(set.capture(CARRIER).expect("known"), None);

    // The retry succeeds and gets its own attempt ordinal.
    let retry = set
        .begin_capture(7, CARRIER, CLAIMANT, navy())
        .expect("the abort freed the ship");
    assert_eq!(retry.attempt, 2);
    assert_eq!(
        set.advance_capture(&first),
        Err(CapitalRuntimeError::StaleCaptureTicket {
            actor: CARRIER,
            ticket: 1,
            current: 2,
        }),
        "the aborted ticket is stale forever"
    );
    assert_eq!(
        set.abort_capture(&first),
        Err(CapitalRuntimeError::StaleCaptureTicket {
            actor: CARRIER,
            ticket: 1,
            current: 2,
        })
    );
    assert_eq!(
        set.ship(CARRIER).expect("registered").ownership().owner,
        raiders,
        "the stale call committed nothing"
    );

    // A ticket from another session is refused even when the attempt matches.
    let foreign = cs_sim::capital::CaptureTicket {
        session: first.session + 1,
        ..retry
    };
    assert_eq!(
        set.advance_capture(&foreign),
        Err(CapitalRuntimeError::Capture(
            CaptureRefusal::ForeignSession {
                ship: CARRIER,
                ticket: 8,
                current: 7,
            }
        ))
    );

    // The retry itself still completes normally.
    while set.capture(CARRIER).expect("known").is_some() {
        set.advance_capture(&retry).expect("advances");
    }
    assert_eq!(
        set.ship(CARRIER).expect("registered").ownership().owner,
        navy()
    );
}

// ------------------------------------------------- staged destruction -------

/// Destruction and despawn are separate: a lethal subsystem starts the staged
/// phase, the wreck stays in the world and still takes part damage, and only
/// the end of the phase closes the record.
#[test]
fn accept_f35_c_staged_destruction_keeps_the_wreck_then_despawns_and_closes() {
    let mut set =
        CapitalShipSet::with_despawn_policy(10, DespawnPolicy::after_ticks(3)).expect("valid rate");
    set.register(synthetic_capital_ship(), None)
        .expect("registers");

    // A launch scheduled into the open bay, plus a capture under way, both
    // have to survive nothing: the ship dies first.
    set.advance_to(Tick(35)).expect("the bay is open");
    let pending = set
        .schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules");
    let ticket = set
        .begin_capture(7, CARRIER, CLAIMANT, navy())
        .expect("attempts");
    set.advance_capture(&ticket).expect("eligible");
    assert_eq!(
        set.capture_stage(CARRIER).expect("known"),
        CaptureStage::Eligible
    );

    // The lethal keel empties its pool at tick 35: destruction, not despawn.
    let outcome = set
        .apply_hit(&hit(CARRIER, "keel", 200.0, 35))
        .expect("resolves");
    assert!(matches!(outcome, HitOutcome::Destroyed { .. }));
    assert_eq!(
        set.destruction(CARRIER).expect("known"),
        DestructionState::Sinking {
            since: Tick(35),
            despawn_at: Some(Tick(38)),
        }
    );
    assert!(!set.is_despawned(CARRIER).expect("known"));
    assert_eq!(
        set.capture(CARRIER).expect("known"),
        None,
        "destruction tears the attempt down without committing it"
    );
    assert_eq!(
        set.ship(CARRIER).expect("registered").ownership().owner,
        synthetic_capital_ownership().owner,
        "a destroyed ship is never captured"
    );

    // The next pass cancels the waiting launch: a dying ship launches nothing.
    let events = set.advance_to(Tick(36)).expect("advances");
    assert_eq!(
        cancelled_of(&events, CARRIER, &pending).as_slice(),
        &[&CapitalShipEvent::LaunchCancelled {
            carrier: CARRIER,
            id: pending.clone(),
            reason: LaunchCancelReason::ShipDestroyed,
            at: Tick(36),
        }]
    );
    assert!(released_of(&events, CARRIER, &pending).is_empty());

    // The wreck is still here and still dangerous to touch: parts aboard it
    // keep resolving, though nothing it carries may act.
    assert!(
        set.advance_to(Tick(37))
            .expect("advances")
            .iter()
            .all(|event| !matches!(event, CapitalShipEvent::Despawned { .. }))
    );
    assert!(
        set.disable(CARRIER, &key("turret_1"))
            .expect("a wreck's parts")
            .changed
    );
    assert_eq!(
        set.may_fire(CARRIER, &key("turret_1")),
        Err(CapitalRuntimeError::Turret(
            cs_sim::capital::TurretRefusal::ShipDestroyed {
                key: key("turret_1")
            }
        )),
        "a wreck carries no gun that may fire, whatever its parts"
    );

    // The phase ends at tick 38: the ship leaves the world and its record
    // closes.
    let events = set.advance_to(Tick(38)).expect("advances");
    assert_eq!(
        events.as_slice(),
        &[CapitalShipEvent::Despawned {
            actor: CARRIER,
            at: Tick(38),
        }]
    );
    assert!(set.is_despawned(CARRIER).expect("known"));
    assert_eq!(
        set.destruction(CARRIER).expect("known"),
        DestructionState::Despawned { at: Tick(38) }
    );

    // Its last pose is still readable for diagnostics...
    approx(
        set.pose(CARRIER).expect("posed").position_m,
        [70.0, 0.0, 0.0],
        1e-9,
    );
    // ...but every new order refuses, and the refusal names the tick.
    let closed = CapitalRuntimeError::ShipDespawned {
        actor: CARRIER,
        at: Tick(38),
    };
    assert_eq!(
        set.apply_hit(&hit(CARRIER, "weapon_bay_1", 10.0, 38)),
        Err(closed.clone())
    );
    assert_eq!(
        set.disable(CARRIER, &key("weapon_bay_1")),
        Err(closed.clone())
    );
    assert_eq!(
        set.schedule_launch(
            CARRIER,
            &key("launch_bay_1"),
            fighter(),
            Tick(60),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        ),
        Err(closed.clone())
    );
    assert_eq!(set.load_cargo(CARRIER, 1.0), Err(closed.clone()));
    assert_eq!(set.unload_cargo(CARRIER, 1.0), Err(closed.clone()));
    assert_eq!(
        set.begin_capture(7, CARRIER, CLAIMANT, navy()),
        Err(closed.clone())
    );
    assert_eq!(
        set.despawn_ship(CARRIER),
        Err(closed),
        "a despawn is not repeatable"
    );
}

/// The default policy holds a wreck until the mission despawns it, an intact
/// ship never despawns, and an explicit despawn is the same teardown the timed
/// one performs.
#[test]
fn accept_f35_c_despawn_is_separate_from_destruction_and_holds_by_default() {
    let mut set = registered_set();
    assert_eq!(set.despawn_policy(), DespawnPolicy::HOLD);
    assert_eq!(
        set.despawn_ship(CARRIER),
        Err(CapitalRuntimeError::NotDestroyed { actor: CARRIER }),
        "an intact ship does not leave the world through this door"
    );

    set.load_cargo(CARRIER, 100.0)
        .expect("a live ship holds cargo");
    set.disable(CARRIER, &key("keel"))
        .expect("the scripted path destroys the keel");
    assert_eq!(
        set.destruction(CARRIER).expect("known"),
        DestructionState::Sinking {
            since: Tick(0),
            despawn_at: None,
        }
    );

    // Held: the wreck stays hit for as long as the session runs.
    let events = set.advance_to(Tick(500)).expect("advances");
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, CapitalShipEvent::Despawned { .. })),
        "a held wreck never despawns on its own"
    );
    assert!(matches!(
        set.apply_hit(&hit(CARRIER, "weapon_bay_1", 10.0, 500)),
        Ok(HitOutcome::NotExposed { .. })
    ));

    // The mission's explicit despawn is the same teardown.
    set.despawn_ship(CARRIER).expect("despawns");
    assert_eq!(
        set.destruction(CARRIER).expect("known"),
        DestructionState::Despawned { at: Tick(500) }
    );
    assert_eq!(
        set.cargo_load(CARRIER),
        Err(CapitalRuntimeError::ShipDespawned {
            actor: CARRIER,
            at: Tick(500)
        })
    );

    // A zero-tick policy despawns on the tick the ship dies, and a registered
    // wreck starts already inside its staged phase.
    let mut instant =
        CapitalShipSet::with_despawn_policy(10, DespawnPolicy::after_ticks(0)).expect("valid");
    instant
        .register(synthetic_capital_ship(), None)
        .expect("registers");
    instant.disable(CARRIER, &key("keel")).expect("destroys");
    assert_eq!(
        instant.destruction(CARRIER).expect("known"),
        DestructionState::Despawned { at: Tick(0) }
    );
}

// -------------------------------------------------------------- cargo -------

/// Cargo is bounded by the ship's declared capacity, a refused load or unload
/// changes nothing, an unresolved capacity refuses every load by claim, and a
/// dying ship may still be emptied but never filled.
#[test]
fn accept_f35_c_cargo_is_capacity_bounded_and_refuses_unknowns() {
    let mut set = registered_set();
    let Resolved::Known(capacity) = set.cargo_capacity(CARRIER).expect("known").clone() else {
        panic!("the synthetic ship's cargo capacity is known");
    };
    assert_eq!(
        capacity.value, 5_000.0,
        "the declared capacity lowers verbatim"
    );
    assert_eq!(set.cargo_load(CARRIER).expect("known"), 0.0);

    assert_eq!(set.load_cargo(CARRIER, 3_000.0), Ok(3_000.0));
    assert_eq!(
        set.load_cargo(CARRIER, 2_500.0),
        Err(CapitalRuntimeError::Cargo(
            CargoRefusal::InsufficientCapacity {
                capacity: 5_000.0,
                loaded: 3_000.0,
                requested: 2_500.0,
            }
        )),
        "a load past the declared capacity refuses"
    );
    assert_eq!(set.cargo_load(CARRIER).expect("known"), 3_000.0);
    assert_eq!(
        set.load_cargo(CARRIER, 2_000.0),
        Ok(5_000.0),
        "the bound is exact"
    );

    for (units, expected) in [
        (-1.0, CargoRefusal::NegativeLoad { value: -1.0 }),
        (f64::NAN, CargoRefusal::NonFiniteLoad),
        (f64::INFINITY, CargoRefusal::NonFiniteLoad),
    ] {
        assert_eq!(
            set.load_cargo(CARRIER, units),
            Err(CapitalRuntimeError::Cargo(expected))
        );
    }

    assert_eq!(set.unload_cargo(CARRIER, 1_000.0), Ok(4_000.0));
    assert_eq!(
        set.unload_cargo(CARRIER, 5_000.0),
        Err(CapitalRuntimeError::Cargo(CargoRefusal::InsufficientLoad {
            loaded: 4_000.0,
            requested: 5_000.0,
        }))
    );
    assert_eq!(set.cargo_load(CARRIER).expect("known"), 4_000.0);

    // A dying ship may be emptied — unloading reads what is there — but is
    // never filled with anything new.
    set.disable(CARRIER, &key("keel"))
        .expect("destroys the ship");
    assert_eq!(
        set.load_cargo(CARRIER, 10.0),
        Err(CapitalRuntimeError::Cargo(CargoRefusal::ShipDestroyed {
            actor: CARRIER
        }))
    );
    assert_eq!(set.unload_cargo(CARRIER, 4_000.0), Ok(0.0));

    // An unresolved capacity holds nothing and refuses every load by claim.
    let mut unknown_capacity = CapitalShipSet::new(10).expect("valid");
    let hold = ActorId(6);
    unknown_capacity
        .register(
            rigged_ship(
                hold,
                None,
                unknown("cargo-capacity"),
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    assert_eq!(unknown_capacity.cargo_load(hold).expect("known"), 0.0);
    let refused = unknown_capacity
        .load_cargo(hold, 1.0)
        .expect_err("an unmeasured capacity refuses");
    let CapitalRuntimeError::Cargo(CargoRefusal::CapacityUnknown { claim_id, reason }) = refused
    else {
        panic!("the unknown capacity refuses by claim, got {refused:?}");
    };
    assert_eq!(claim_id.as_str(), "cargo-capacity");
    assert_eq!(reason, "cargo-capacity unmeasured");
    assert_eq!(unknown_capacity.cargo_load(hold).expect("known"), 0.0);
}

// ------------------------------------------------------ error propagation ---

/// Every command refuses an unregistered actor by name, an id the ledger never
/// issued, and a ship that holds no capture — with the set left usable.
#[test]
fn accept_f35_c_commands_refuse_unknown_actors_and_ids_by_name() {
    let mut set = registered_set();
    let ghost = ActorId(42);
    let bay = key("launch_bay_1");

    assert_eq!(
        set.destruction(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.is_despawned(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.control(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.cargo_load(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.cargo_capacity(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.load_cargo(ghost, 1.0),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.unload_cargo(ghost, 1.0),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.pending_launches(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.launch_status(
            ghost,
            &LaunchId {
                bay: bay.clone(),
                sequence: 0
            }
        ),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.capture(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.capture_stage(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.begin_capture(7, ghost, CLAIMANT, navy()),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.despawn_ship(ghost),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    let ghost_ticket = cs_sim::capital::CaptureTicket {
        ship: ghost,
        claimant: CLAIMANT,
        session: 7,
        attempt: 1,
    };
    assert_eq!(
        set.advance_capture(&ghost_ticket),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );
    assert_eq!(
        set.abort_capture(&ghost_ticket),
        Err(CapitalRuntimeError::UnknownShip(ghost))
    );

    // A ship that holds no attempt has nothing to advance or abort.
    let idle = cs_sim::capital::CaptureTicket {
        ship: CARRIER,
        claimant: CLAIMANT,
        session: 7,
        attempt: 1,
    };
    assert_eq!(
        set.advance_capture(&idle),
        Err(CapitalRuntimeError::NoCapture { actor: CARRIER })
    );
    assert_eq!(
        set.abort_capture(&idle),
        Err(CapitalRuntimeError::NoCapture { actor: CARRIER })
    );

    // A launch id the ledger never issued is not releaseable either.
    let never = LaunchId { bay, sequence: 99 };
    assert_eq!(set.launch_status(CARRIER, &never).expect("known"), None);
    assert_eq!(
        set.release_launch(CARRIER, &never),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::UnknownBay))
    );

    // The set is still perfectly usable afterwards.
    set.schedule_launch(
        CARRIER,
        &key("launch_bay_1"),
        fighter(),
        Tick(35),
        SYNTHETIC_LAUNCH_EJECT_M_S,
    )
    .expect("still schedules");
    let events = set.advance_to(Tick(35)).expect("still advances");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, CapitalShipEvent::LaunchReleased { .. }))
            .count(),
        1
    );

    // A carrier at the top of the actor id space leaves the session nothing to
    // hand out. The tick refuses rather than resolving a launch it could not
    // give an aircraft, and nothing is lost: the launch is still waiting.
    let mut exhausted = CapitalShipSet::new(10).expect("valid");
    let top = ActorId(u32::MAX);
    exhausted
        .register(
            rigged_ship(
                top,
                Some(known_rig([0.0, -5.0, 0.0], 4)),
                designed(0.0),
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    let stranded = exhausted
        .schedule_launch(
            top,
            &key("launch_bay_1"),
            fighter(),
            Tick(35),
            SYNTHETIC_LAUNCH_EJECT_M_S,
        )
        .expect("schedules");
    assert_eq!(
        exhausted.advance_to(Tick(35)),
        Err(CapitalRuntimeError::Launch(LaunchRefusal::ActorIdExhausted)),
        "the pass refuses instead of releasing an id it cannot name"
    );
    assert_eq!(
        exhausted.pending_launches(top).expect("known"),
        vec![cs_sim::capital::PendingLaunch {
            id: stranded.clone(),
            aircraft: fighter(),
            ready_tick: Tick(35),
        }],
        "the refused tick resolved nothing: the launch still waits"
    );
    assert!(
        matches!(
            exhausted.launch_status(top, &stranded).expect("known"),
            Some(LaunchStatus::Pending { .. })
        ),
        "a refused pass never produces a half-released launch"
    );
}
