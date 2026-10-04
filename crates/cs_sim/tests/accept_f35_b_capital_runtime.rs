//! F35-B acceptance: capital-ship movement under propulsion, the
//! exposure-gated weakpoint resolver, section integrity pools and turret
//! aim/fire behavior (synthetic).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-B`. Ordinary build/test only; nothing here is original
//! data.
//!
//! The minimum scenario is
//! [`accept_f35_b_weakpoint_hit_resolves_before_during_and_after_exposure`]:
//! a bay weakpoint refuses hits before and after its exposed phase and is
//! destroyed by one during it. Every test drives production code —
//! [`CapitalShipSet`], [`CapitalShip::apply_hit`], the turret commands —
//! so removing the exposure gate, the propulsion fraction, the pools or
//! the traverse clamp fails an assertion or fails to compile.

use cs_script::ir::ActorId;
use cs_sim::capital::{
    BayState, CapitalHit, CapitalParts, CapitalRuntimeError, CapitalShip, CapitalShipEvent,
    CapitalShipSet, HitError, HitOutcome, IntegrityPool, Subsystem, SubsystemEffect,
    SubsystemGraph, SubsystemKey, SubsystemKind, SubsystemState, TurretMount, TurretRefusal,
    synthetic_capital_bays, synthetic_capital_docking_anchors, synthetic_capital_engines,
    synthetic_capital_graph, synthetic_capital_ownership, synthetic_capital_sections,
    synthetic_capital_ship, synthetic_capital_trajectory, synthetic_capital_turrets,
};
use cs_sim::world_actors::Quat;
use cs_sim::world_actors::trajectory::{Keyframe, Pose, Trajectory};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn key(name: &str) -> SubsystemKey {
    SubsystemKey::new(name).expect("test subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f35b.test").expect("valid")),
    ))
}

fn designed_id(name: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, name).expect("valid"),
        Provenance::designed(ClaimId::new("f35b.test").expect("valid")),
    ))
}

/// The synthetic ship registered in a 10 ticks/s set.
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

/// A lean ship used when the test needs parts the fixture does not carry:
/// one lethal keel, an optional trajectory, whatever part lists are given.
fn lean_ship(
    actor: ActorId,
    turrets: Vec<TurretMount>,
    sections: Vec<IntegrityPool>,
    trajectory: Option<Trajectory>,
) -> CapitalShip {
    CapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.barge").expect("valid"),
        actor,
        CapitalParts {
            graph: SubsystemGraph::try_new(vec![
                Subsystem::new(key("turret_1"), SubsystemKind::Turret)
                    .with_effect(SubsystemEffect::WeaponAccess),
                Subsystem::new(key("keel"), SubsystemKind::StructuralSection)
                    .with_effect(SubsystemEffect::MissionCondition)
                    .with_lethal(true),
            ])
            .expect("valid graph"),
            engines: vec![],
            bays: vec![],
            turrets,
            docking_anchors: vec![],
            sections,
            cargo: designed(0.0),
            ownership: synthetic_capital_ownership(),
            trajectory,
        },
    )
    .expect("the lean ship is valid")
}

// -------------------------------------------------- AC02 minimum scenario ---

/// The minimum scenario: hits on a bay weakpoint resolve differently
/// before, during and after its exposure window — and a destroyed bay is
/// never a weakpoint again.
///
/// The synthetic weapon bay's authored cycle is concealed 0–39, opening
/// 40–49, exposed 50–109, closing 110–119, then repeats.
#[test]
fn accept_f35_b_weakpoint_hit_resolves_before_during_and_after_exposure() {
    let mut set = registered_set();
    let ship = ActorId(1);

    // Before: concealed (tick 20) and opening (tick 45) hits land nothing.
    set.advance_to(Tick(20)).expect("advances");
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 50.0, 20)),
        Ok(HitOutcome::NotExposed {
            subsystem: key("weapon_bay_1"),
            state: BayState::Concealed,
        })
    );
    set.advance_to(Tick(45)).expect("advances");
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 50.0, 45)),
        Ok(HitOutcome::NotExposed {
            subsystem: key("weapon_bay_1"),
            state: BayState::Opening,
        })
    );
    assert_eq!(
        set.ship(ship)
            .expect("registered")
            .subsystem_state(&key("weapon_bay_1")),
        Some(SubsystemState::Intact),
        "a non-exposed hit leaves the subsystem intact"
    );

    // During: the exposed phase is the weakpoint — one landed hit destroys
    // the bay and applies its weapon-access effect.
    set.advance_to(Tick(60)).expect("advances");
    let outcome = set
        .apply_hit(&hit(ship, "weapon_bay_1", 50.0, 60))
        .expect("a landed hit resolves");
    let HitOutcome::Destroyed { outcome, .. } = outcome else {
        panic!("an exposed hit destroys the bay, got {outcome:?}");
    };
    assert!(outcome.changed);
    assert!(!outcome.actor_destroyed, "a bay is not lethal");
    assert_eq!(outcome.effect, Some(SubsystemEffect::WeaponAccess));
    assert_eq!(
        set.ship(ship)
            .expect("registered")
            .subsystem_state(&key("weapon_bay_1")),
        Some(SubsystemState::Disabled)
    );

    // A destroyed bay never exposes again: a hit at a later exposed-phase
    // tick reports the part is already gone.
    set.advance_to(Tick(170))
        .expect("next cycle's exposed phase");
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 50.0, 170)),
        Ok(HitOutcome::AlreadyDisabled {
            subsystem: key("weapon_bay_1"),
        })
    );
    assert_eq!(
        set.ship(ship)
            .expect("registered")
            .bay_state(&key("weapon_bay_1"), Tick(170)),
        Some(BayState::Destroyed)
    );

    // After: a second ship never hit during its window refuses closing and
    // next-cycle concealed hits — the weakpoint is only open while open.
    let mut other = registered_set();
    other.advance_to(Tick(115)).expect("closing phase");
    assert_eq!(
        other.apply_hit(&hit(ship, "weapon_bay_1", 50.0, 115)),
        Ok(HitOutcome::NotExposed {
            subsystem: key("weapon_bay_1"),
            state: BayState::Closing,
        })
    );
    other.advance_to(Tick(130)).expect("concealed again");
    assert_eq!(
        other.apply_hit(&hit(ship, "weapon_bay_1", 50.0, 130)),
        Ok(HitOutcome::NotExposed {
            subsystem: key("weapon_bay_1"),
            state: BayState::Concealed,
        })
    );
    assert_eq!(
        other
            .ship(ship)
            .expect("registered")
            .subsystem_state(&key("weapon_bay_1")),
        Some(SubsystemState::Intact)
    );
}

/// A launch bay is a weakpoint on the same terms: its exposed phase opens
/// at 35 and the hit policy reads the authored window, not the bay kind.
#[test]
fn accept_f35_b_launch_bay_weakpoint_uses_its_own_window() {
    let mut set = registered_set();
    let ship = ActorId(1);

    // Launch bay cycle: concealed 0–29, opening 30–34, exposed 35–79.
    set.advance_to(Tick(20)).expect("advances");
    assert_eq!(
        set.apply_hit(&hit(ship, "launch_bay_1", 10.0, 20)),
        Ok(HitOutcome::NotExposed {
            subsystem: key("launch_bay_1"),
            state: BayState::Concealed,
        })
    );
    set.advance_to(Tick(35)).expect("exposed");
    let outcome = set
        .apply_hit(&hit(ship, "launch_bay_1", 10.0, 35))
        .expect("resolves");
    assert!(matches!(outcome, HitOutcome::Destroyed { .. }));
}

// --------------------------------------------------------- movement --------

/// AC01 through the runtime: with both engines intact the ship keeps its
/// authored schedule; killing an engine halves its progress rate; killing
/// both stops it. The hull is never destroyed by engine loss.
#[test]
fn accept_f35_b_losing_engines_slowing_then_stopping_the_course() {
    // Intact: 100 ticks at full rate moves the drive 100 ticks → 200 m at
    // the authored 20 m/s.
    let mut set = registered_set();
    let ship = ActorId(1);
    set.advance_to(Tick(100)).expect("advances");
    let pose = set.pose(ship).expect("posed");
    approx(pose.position_m, [200.0, 0.0, 0.0], 1e-9);
    approx(pose.velocity_m_s, [20.0, 0.0, 0.0], 1e-9);
    assert_eq!(set.drive_ticks(ship), Ok(100.0));

    // A landed hit destroys engine_1 (an engine has no pool: one damaging
    // hit kills it); the rate halves and the measured velocity halves.
    let mut crippled = registered_set();
    let outcome = crippled
        .apply_hit(&hit(ship, "engine_1", 10.0, 0))
        .expect("resolves");
    let HitOutcome::Destroyed { outcome, .. } = outcome else {
        panic!("an engine hit destroys it, got {outcome:?}");
    };
    assert_eq!(outcome.effect, Some(SubsystemEffect::Propulsion));
    crippled.advance_to(Tick(100)).expect("advances");
    let pose = crippled.pose(ship).expect("posed");
    approx(pose.position_m, [100.0, 0.0, 0.0], 1e-9);
    approx(pose.velocity_m_s, [10.0, 0.0, 0.0], 1e-9);
    assert!(
        !crippled.is_destroyed(ship).expect("known"),
        "engine loss cannot destroy the hull"
    );

    // The scripted disable path is equivalent: both engines gone → the
    // drive stops advancing and the reported velocity is zero.
    crippled
        .disable(ship, &key("engine_2"))
        .expect("known engine");
    crippled.advance_to(Tick(200)).expect("advances");
    let pose = crippled.pose(ship).expect("posed");
    approx(pose.position_m, [100.0, 0.0, 0.0], 1e-9);
    approx(pose.velocity_m_s, [0.0, 0.0, 0.0], 1e-12);
    assert_eq!(crippled.drive_ticks(ship), Ok(50.0));
    assert!(!crippled.is_destroyed(ship).expect("known"));
    assert_eq!(
        crippled
            .ship(ship)
            .expect("registered")
            .subsystem_state(&key("keel")),
        Some(SubsystemState::Intact)
    );
}

/// The drive clamps at the course's end: the ship holds its final pose,
/// reports zero velocity and emits `CourseCompleted` exactly once.
#[test]
fn accept_f35_b_course_completion_clamps_and_reports_once() {
    let mut set = registered_set();
    let ship = ActorId(1);
    let events = set.advance_to(Tick(500)).expect("advances to course end");
    assert!(
        events.contains(&CapitalShipEvent::CourseCompleted {
            actor: ship,
            at: Tick(500),
        }),
        "the course end reports once, got {events:?}"
    );
    let pose = set.pose(ship).expect("posed");
    approx(pose.position_m, [1000.0, 0.0, 0.0], 1e-9);

    let events = set.advance_to(Tick(510)).expect("beyond the end");
    assert!(events.is_empty(), "completion is an edge, not a state");
    let pose = set.pose(ship).expect("posed");
    approx(pose.position_m, [1000.0, 0.0, 0.0], 1e-9);
    approx(pose.velocity_m_s, [0.0, 0.0, 0.0], 1e-12);
    assert_eq!(set.drive_ticks(ship), Ok(500.0));
}

/// A ship with no engine subsystem has no propulsion to lose: its authored
/// course is unpowered, it keeps schedule, and only a lethal part destroys
/// it.
#[test]
fn accept_f35_b_engineless_ship_keeps_its_unpowered_course() {
    let mut set = CapitalShipSet::new(10).expect("valid");
    let barge = ActorId(7);
    set.register(
        lean_ship(barge, vec![], vec![], Some(synthetic_capital_trajectory())),
        None,
    )
    .expect("registers");
    set.advance_to(Tick(100)).expect("advances");
    let pose = set.pose(barge).expect("posed");
    approx(pose.position_m, [200.0, 0.0, 0.0], 1e-9);

    // Its lethal keel has no pool, so a landed hit destroys the ship and
    // freezes the wreck — the course never resumes.
    set.apply_hit(&hit(barge, "keel", 1.0, 100))
        .expect("resolves");
    assert!(set.is_destroyed(barge).expect("known"));
    set.advance_to(Tick(150)).expect("advances");
    let wreck = set.pose(barge).expect("posed");
    approx(wreck.position_m, [200.0, 0.0, 0.0], 1e-9);
    approx(wreck.velocity_m_s, [0.0, 0.0, 0.0], 1e-12);
}

/// A ship with no trajectory is moored: it holds the registered pose and
/// never moves, whatever its engines would produce.
#[test]
fn accept_f35_b_course_less_ship_holds_its_start_pose() {
    let mut set = CapitalShipSet::new(10).expect("valid");
    let moored = ActorId(9);
    let start = Pose {
        position_m: [5.0, 100.0, -3.0],
        orientation: Quat::IDENTITY,
        velocity_m_s: [0.0; 3],
        angular_velocity_rad_s: [0.0; 3],
    };
    set.register(lean_ship(moored, vec![], vec![], None), Some(start))
        .expect("registers");
    set.advance_to(Tick(50)).expect("advances");
    let pose = set.pose(moored).expect("posed");
    approx(pose.position_m, [5.0, 100.0, -3.0], 1e-12);
    approx(pose.velocity_m_s, [0.0, 0.0, 0.0], 1e-12);
}

// ------------------------------------------------------- section pools ----

/// A section's declared pool absorbs hits until depleted; depleting a
/// lethal section destroys the ship — and freezes the wreck mid-course.
#[test]
fn accept_f35_b_section_pools_absorb_until_depleted() {
    let mut set = registered_set();
    let ship = ActorId(1);
    set.advance_to(Tick(50)).expect("mid-course");

    // Gas cell: 120 integrity. The first hit drains, the second destroys.
    assert_eq!(
        set.apply_hit(&hit(ship, "gas_cell_1", 50.0, 50)),
        Ok(HitOutcome::Damaged {
            subsystem: key("gas_cell_1"),
            remaining_integrity: 70.0,
        })
    );
    assert_eq!(
        set.ship(ship)
            .expect("registered")
            .remaining_integrity(&key("gas_cell_1")),
        Some(70.0)
    );
    assert!(!set.is_destroyed(ship).expect("known"));
    assert_eq!(set.pose(ship).expect("posed").position_m, [100.0, 0.0, 0.0]);

    // Depleting the lethal cell destroys the ship and freezes its pose.
    let outcome = set
        .apply_hit(&hit(ship, "gas_cell_1", 70.0, 50))
        .expect("resolves");
    let HitOutcome::Destroyed { outcome, .. } = outcome else {
        panic!("depleting destroys the cell, got {outcome:?}");
    };
    assert!(outcome.actor_destroyed);
    assert_eq!(outcome.effect, Some(SubsystemEffect::Vulnerability));
    assert!(set.is_destroyed(ship).expect("known"));

    // The wreck is frozen at the pose it died on: later steps cannot move
    // it, but part hits on the wreck still resolve.
    let wreck = set.pose(ship).expect("posed");
    approx(wreck.position_m, [100.0, 0.0, 0.0], 1e-9);
    approx(wreck.velocity_m_s, [0.0, 0.0, 0.0], 1e-12);
    set.advance_to(Tick(60)).expect("the wreck cannot move");
    assert_eq!(set.pose(ship).expect("posed").position_m, [100.0, 0.0, 0.0]);
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 5.0, 60)),
        Ok(HitOutcome::Destroyed {
            subsystem: key("weapon_bay_1"),
            outcome: cs_sim::capital::DisableOutcome {
                state: SubsystemState::Disabled,
                changed: true,
                actor_destroyed: false,
                effect: Some(SubsystemEffect::WeaponAccess),
            },
        })
    );
}

/// A section whose declared pool is unresolved blocks the hit by claim:
/// nothing is absorbed, nothing destroyed, and the block names its claim.
#[test]
fn accept_f35_b_unknown_integrity_blocks_the_hit() {
    let mut set = CapitalShipSet::new(10).expect("valid");
    let barge = ActorId(7);
    set.register(
        lean_ship(
            barge,
            vec![],
            vec![IntegrityPool {
                key: key("keel"),
                integrity: Resolved::Unknown {
                    claim_id: ClaimId::new("f35b.test.unknown-integrity").expect("valid"),
                    reason: "keel integrity unmeasured".to_owned(),
                },
            }],
            Some(synthetic_capital_trajectory()),
        ),
        None,
    )
    .expect("registers");
    set.advance_to(Tick(10)).expect("advances");

    let outcome = set
        .apply_hit(&hit(barge, "keel", 50.0, 10))
        .expect("resolves");
    let HitOutcome::Blocked {
        claim_id, reason, ..
    } = outcome
    else {
        panic!("an unknown pool blocks, got {outcome:?}");
    };
    assert_eq!(claim_id.as_str(), "f35b.test.unknown-integrity");
    assert_eq!(reason, "keel integrity unmeasured");
    // The part is intact and the ship lives: nothing was invented.
    assert!(!set.is_destroyed(barge).expect("known"));
    assert_eq!(
        set.ship(barge)
            .expect("registered")
            .subsystem_state(&key("keel")),
        Some(SubsystemState::Intact)
    );
    assert_eq!(
        set.ship(barge)
            .expect("registered")
            .remaining_integrity(&key("keel")),
        None
    );
}

// ------------------------------------------------------------- turrets ----

/// The synthetic dorsal turret bears up (+Y) with a 180° traverse cone:
/// commands inside the cone land verbatim, commands outside clamp to the
/// rim, and the verdict reports the off-boresight angle.
#[test]
fn accept_f35_b_turret_aim_is_clamped_to_the_traverse_cone() {
    let mut set = registered_set();
    let ship = ActorId(1);
    let turret = key("turret_1");

    // Aims start on the boresight.
    assert_eq!(
        set.ship(ship).expect("registered").turret_aim(&turret),
        Some([0.0, 1.0, 0.0])
    );

    // Inside the cone (the boresight itself): verbatim, in-arc.
    let aim = set
        .aim_turret(ship, &turret, [0.0, 1.0, 0.0])
        .expect("aims");
    assert!(aim.within_arc);
    assert_eq!(aim.off_boresight_deg, 0.0);
    approx(aim.aim, [0.0, 1.0, 0.0], 1e-9);

    // On the rim (90° off a 180° cone): still in-arc, verbatim.
    let aim = set
        .aim_turret(ship, &turret, [1.0, 0.0, 0.0])
        .expect("aims");
    assert!(aim.within_arc);
    assert!((aim.off_boresight_deg - 90.0).abs() < 1e-9);
    approx(aim.aim, [1.0, 0.0, 0.0], 1e-9);

    // Beyond the rim ([1,-1,0] is 135° off): clamps to the cone rim on the
    // great circle toward the command — +X here — and reports outside.
    let aim = set
        .aim_turret(ship, &turret, [1.0, -1.0, 0.0])
        .expect("aims");
    assert!(!aim.within_arc);
    assert!((aim.off_boresight_deg - 135.0).abs() < 1e-9);
    approx(aim.aim, [1.0, 0.0, 0.0], 1e-9);
    assert_eq!(
        set.ship(ship).expect("registered").turret_aim(&turret),
        Some(aim.aim),
        "the clamped bearing is kept"
    );

    // Degenerate commands are refused, not clamped into NaN.
    assert_eq!(
        set.aim_turret(ship, &turret, [0.0, 0.0, 0.0]),
        Err(CapitalRuntimeError::Turret(TurretRefusal::ZeroDirection {
            key: turret.clone(),
        }))
    );
    assert_eq!(
        set.aim_turret(ship, &turret, [f64::NAN, 0.0, 0.0]),
        Err(CapitalRuntimeError::Turret(
            TurretRefusal::NonFiniteDirection {
                key: turret.clone(),
            }
        ))
    );
    assert_eq!(
        set.aim_turret(ship, &key("keel"), [0.0, 1.0, 0.0]),
        Err(CapitalRuntimeError::Turret(TurretRefusal::UnknownTurret {
            key: key("keel"),
        }))
    );
}

/// Firing requires a live ship, a live turret and a known weapon binding:
/// the synthetic turret's unmeasured weapon refuses by claim, a destroyed
/// turret refuses to aim or fire, and a wreck's turrets stay silent.
#[test]
fn accept_f35_b_turret_fire_gate_respects_state_and_unknowns() {
    let mut set = registered_set();
    let ship = ActorId(1);
    let turret = key("turret_1");

    // The fixture turret's weapon is an explicit unknown: it can aim but
    // never fire.
    set.aim_turret(ship, &turret, [0.5, 1.0, 0.0])
        .expect("aims");
    let refused = set.may_fire(ship, &turret).expect_err("no unknown gun");
    let CapitalRuntimeError::Turret(TurretRefusal::WeaponUnknown {
        claim_id, reason, ..
    }) = refused
    else {
        panic!("the unmeasured weapon refuses by claim, got {refused:?}");
    };
    assert_eq!(claim_id.as_str(), "f35a.turret-weapon-unmeasured");
    assert_eq!(reason, "turret weapon binding unmeasured");

    // Destroying the turret ends aim and fire together: the WeaponAccess
    // effect is real.
    set.apply_hit(&hit(ship, "turret_1", 1.0, 0))
        .expect("a turret hit resolves");
    assert_eq!(
        set.aim_turret(ship, &turret, [0.0, 1.0, 0.0]),
        Err(CapitalRuntimeError::Turret(TurretRefusal::Destroyed {
            key: turret.clone(),
        }))
    );
    assert_eq!(
        set.may_fire(ship, &turret),
        Err(CapitalRuntimeError::Turret(TurretRefusal::Destroyed {
            key: turret.clone(),
        }))
    );

    // A ship whose turret carries a known weapon fires along its aim.
    let mut armed = CapitalShipSet::new(10).expect("valid");
    let barge = ActorId(7);
    armed
        .register(
            lean_ship(
                barge,
                vec![TurretMount {
                    key: key("turret_1"),
                    weapon: designed_id("synthetic.gun"),
                    traverse_deg: designed(180.0),
                    boresight: [0.0, 1.0, 0.0],
                }],
                vec![],
                Some(synthetic_capital_trajectory()),
            ),
            None,
        )
        .expect("registers");
    assert_eq!(
        armed.may_fire(barge, &turret),
        Ok([0.0, 1.0, 0.0]),
        "a known weapon fires along the boresight"
    );
    armed
        .aim_turret(barge, &turret, [1.0, 0.0, 0.0])
        .expect("aims");
    assert_eq!(armed.may_fire(barge, &turret), Ok([1.0, 0.0, 0.0]));

    // And a destroyed ship's turrets stay silent even when intact.
    armed
        .apply_hit(&hit(barge, "keel", 1.0, 0))
        .expect("resolves");
    assert!(armed.is_destroyed(barge).expect("known"));
    assert_eq!(
        armed.may_fire(barge, &turret),
        Err(CapitalRuntimeError::Turret(TurretRefusal::ShipDestroyed {
            key: turret.clone(),
        }))
    );
    assert_eq!(
        armed.aim_turret(barge, &turret, [0.0, 1.0, 0.0]),
        Err(CapitalRuntimeError::Turret(TurretRefusal::ShipDestroyed {
            key: turret.clone(),
        }))
    );
}

/// A turret whose traverse arc is unresolved cannot judge an aim: the
/// refusal names the unknown, never a guessed arc.
#[test]
fn accept_f35_b_unknown_traverse_refuses_aim() {
    let mut set = CapitalShipSet::new(10).expect("valid");
    let barge = ActorId(7);
    set.register(
        lean_ship(
            barge,
            vec![TurretMount {
                key: key("turret_1"),
                weapon: designed_id("synthetic.gun"),
                traverse_deg: Resolved::Unknown {
                    claim_id: ClaimId::new("f35b.test.unknown-traverse").expect("valid"),
                    reason: "traverse arc unmeasured".to_owned(),
                },
                boresight: [0.0, 1.0, 0.0],
            }],
            vec![],
            None,
        ),
        Some(Pose {
            position_m: [0.0; 3],
            orientation: Quat::IDENTITY,
            velocity_m_s: [0.0; 3],
            angular_velocity_rad_s: [0.0; 3],
        }),
    )
    .expect("registers");
    let refused = set
        .aim_turret(barge, &key("turret_1"), [1.0, 0.0, 0.0])
        .expect_err("no arc to judge");
    assert!(matches!(
        refused,
        CapitalRuntimeError::Turret(TurretRefusal::TraverseUnknown { .. })
    ));
    // The weapon is known though: the turret may still fire along its
    // boresight — the unresolved arc limits aiming, not the mount's gun.
    assert_eq!(set.may_fire(barge, &key("turret_1")), Ok([0.0, 1.0, 0.0]));
}

// -------------------------------------------------------- set discipline --

/// Hits are stamped for the set's committed tick and name registered
/// actors and real subsystems; malformed damage and misuse refuse.
#[test]
fn accept_f35_b_hits_obey_tick_registry_and_damage_discipline() {
    let mut set = registered_set();
    let ship = ActorId(1);
    set.advance_to(Tick(10)).expect("advances");

    // A hit stamped for another tick never resolves against the wrong
    // weakpoint phase.
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 10.0, 9)),
        Err(CapitalRuntimeError::ForeignTick {
            expected: Tick(10),
            found: Tick(9),
        })
    );
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 10.0, 11)),
        Err(CapitalRuntimeError::ForeignTick {
            expected: Tick(10),
            found: Tick(11),
        })
    );

    // Unregistered actors and unknown subsystems refuse by name.
    assert_eq!(
        set.apply_hit(&hit(ActorId(99), "keel", 10.0, 10)),
        Err(CapitalRuntimeError::UnknownShip(ActorId(99)))
    );
    assert_eq!(
        set.apply_hit(&hit(ship, "rudder", 10.0, 10)),
        Err(CapitalRuntimeError::Hit(HitError::UnknownSubsystem(key(
            "rudder"
        ))))
    );

    // Malformed damage refuses at hit construction and at the ship.
    assert_eq!(
        CapitalHit::try_new(ship, key("keel"), -1.0, Tick(10)),
        Err(HitError::NegativeDamage {
            subsystem: key("keel"),
            value: -1.0,
        })
    );
    assert_eq!(
        CapitalHit::try_new(ship, key("keel"), f64::NAN, Tick(10)),
        Err(HitError::NonFiniteDamage {
            subsystem: key("keel"),
        })
    );

    // A zero-damage hit lands nothing — even on an exposed weakpoint.
    set.advance_to(Tick(60)).expect("exposed");
    assert_eq!(
        set.apply_hit(&hit(ship, "weapon_bay_1", 0.0, 60)),
        Ok(HitOutcome::Ineffective {
            subsystem: key("weapon_bay_1"),
        })
    );
    assert_eq!(
        set.ship(ship)
            .expect("registered")
            .subsystem_state(&key("weapon_bay_1")),
        Some(SubsystemState::Intact)
    );

    // And a hit on a moored ship resolves the same — pose is irrelevant to
    // hit policy.
}

/// Registration and stepping enforce the session's structural rules.
#[test]
fn accept_f35_b_registration_and_stepping_enforce_session_rules() {
    assert_eq!(
        CapitalShipSet::new(0),
        Err(CapitalRuntimeError::ZeroTickRate)
    );

    let mut set = CapitalShipSet::new(10).expect("valid");
    set.register(synthetic_capital_ship(), None)
        .expect("registers");
    assert_eq!(
        set.register(synthetic_capital_ship(), None),
        Err(CapitalRuntimeError::DuplicateActor(ActorId(1)))
    );
    set.advance_to(Tick(5)).expect("advances");
    assert_eq!(
        set.advance_to(Tick(4)),
        Err(CapitalRuntimeError::NonMonotonicTick {
            current: Tick(5),
            requested: Tick(4),
        })
    );

    // A trajectory at another tick rate cannot pretend its sampled
    // velocities are the set's.
    let fast_course = Trajectory::new(
        vec![
            Keyframe {
                tick: Tick(0),
                position_m: [0.0; 3],
                orientation: Quat::IDENTITY,
            },
            Keyframe {
                tick: Tick(1000),
                position_m: [1000.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
        ],
        20,
    )
    .expect("valid trajectory");
    assert_eq!(
        set.register(
            lean_ship(ActorId(7), vec![], vec![], Some(fast_course)),
            None
        ),
        Err(CapitalRuntimeError::TickRateMismatch {
            actor: ActorId(7),
            set_ticks_per_second: 10,
            trajectory_ticks_per_second: 20,
        })
    );

    // A course-less ship needs a start pose, and a coursed ship refuses
    // one it could never use.
    assert_eq!(
        set.register(lean_ship(ActorId(8), vec![], vec![], None), None),
        Err(CapitalRuntimeError::NoCourse { actor: ActorId(8) })
    );
    assert_eq!(
        set.register(
            synthetic_capital_ship_at(ActorId(9)),
            Some(Pose {
                position_m: [0.0; 3],
                orientation: Quat::IDENTITY,
                velocity_m_s: [0.0; 3],
                angular_velocity_rad_s: [0.0; 3],
            }),
        ),
        Err(CapitalRuntimeError::CourseAndPose { actor: ActorId(9) })
    );

    // The wreck of a destroyed ship holds its pose; `pose` on an unknown
    // actor refuses.
    set.apply_hit(&hit(ActorId(1), "keel", 1_000.0, 5))
        .expect("lethal hit resolves");
    assert!(set.is_destroyed(ActorId(1)).expect("known"));
    let wreck = set.pose(ActorId(1)).expect("posed");
    set.advance_to(Tick(20)).expect("advances");
    assert_eq!(
        set.pose(ActorId(1)).expect("posed"),
        wreck,
        "a wreck cannot move"
    );
    assert_eq!(
        set.pose(ActorId(42)),
        Err(CapitalRuntimeError::UnknownShip(ActorId(42)))
    );
}

/// The same fixture under another actor id.
fn synthetic_capital_ship_at(actor: ActorId) -> CapitalShip {
    let ship = synthetic_capital_ship();
    CapitalShip::try_new(
        ship.subject().clone(),
        actor,
        CapitalParts {
            graph: synthetic_capital_graph(),
            engines: synthetic_capital_engines(),
            bays: synthetic_capital_bays(),
            turrets: synthetic_capital_turrets(),
            docking_anchors: synthetic_capital_docking_anchors(),
            sections: synthetic_capital_sections(),
            cargo: designed(5_000.0),
            ownership: synthetic_capital_ownership(),
            trajectory: Some(synthetic_capital_trajectory()),
        },
    )
    .expect("valid")
}
