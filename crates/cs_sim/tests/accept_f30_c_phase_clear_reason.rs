//! Acceptance scenario F30-C at the simulation layer: what a phase record says
//! when a held selection stops being eligible, and what "still in the world"
//! means for a threat cue's attacker.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-C`. Task test prefix: `accept_f30_c_`.
//!
//! These tests drive `cs_sim::targeting` directly: `TargetStore::phase`,
//! `TargetStore::clear_reason`, `TargetStore::present`,
//! `TargetStore::guidance` and the `synthetic_roster` fixture. The F30-A/B
//! layers above them own the ECS wiring and the consumer views; this file owns
//! the rules those views read.
//!
//! The minimum scenario is AC03's contract half: a destroyed selected target is
//! cleared by the phase record, with the reason, before any consumer sees it.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use std::collections::BTreeSet;

use cs_sim::damage::{
    ActorId, DamageChannel, DamageNodeKey, HitEvent, HitEventId, LifecycleKind,
    SYNTHETIC_MOUNT_NODE,
};
use cs_sim::targeting::{
    Allegiance, CrosshairError, SelectionClearReason, SelectionRequest, TargetClass, TargetError,
    TargetFilter, TargetSelection, TargetStore, WeaponGuidance, synthetic_allegiance_table,
    synthetic_raider_faction, synthetic_roster, synthetic_target_policy,
};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

/// The session generation the fixture roster belongs to.
const SESSION: u64 = 41;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).expect("a nonzero session generation"),
        serial,
    }
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// A store over the synthetic roster.
fn store() -> TargetStore {
    let mut store = TargetStore::new(
        SESSION,
        synthetic_target_policy(),
        synthetic_allegiance_table(),
    );
    for record in synthetic_roster(SESSION) {
        store
            .register(record)
            .expect("the fixture roster registers");
    }
    store
}

fn hit(sequence: u32, node: &str, attacker: ActorId, victim: ActorId, at: u64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: SessionId::new(SESSION).expect("a nonzero session"),
            tick: Tick(at),
            producer: 9,
            sequence,
        },
        Some(attacker),
        victim,
        DamageNodeKey::new(node).expect("a valid node key"),
        DamageChannel::Internal,
        4.0,
    )
    .expect("the fixture hit is well-formed")
}

/// AC03 at the simulation layer: a destroyed selected target is cleared by the
/// phase record, with the reason, and the reticle never describes it.
#[test]
fn accept_f30_c_phase_reports_the_cleared_selection_and_its_reason() {
    let mut store = store();
    let mut selection = TargetSelection::new();
    assert_eq!(
        store
            .apply(
                actor(1),
                &mut selection,
                &SelectionRequest::Nearest {
                    filter: TargetFilter::Allegiance(Allegiance::Hostile),
                },
            )
            .expect("registered"),
        Some(actor(2)),
        "the store's own evaluation selects a raider"
    );

    let phase = store
        .phase(actor(1), &mut selection, Tick(20))
        .expect("registered");
    assert_eq!(phase.selection, Some(actor(2)));
    assert_eq!(phase.cleared, None, "nothing was cleared yet");
    assert!(phase.reticle.expect("a reticle").hostile);

    // A lifecycle transition ends the actor's targetability.
    store
        .record_lifecycle(actor(2), LifecycleKind::Destroyed)
        .expect("registered");

    let phase = store
        .phase(actor(1), &mut selection, Tick(21))
        .expect("registered");
    assert_eq!(phase.selection, None, "the destroyed selection is cleared");
    assert_eq!(
        phase.cleared.map(|cleared| (cleared.actor, cleared.reason)),
        Some((
            actor(2),
            SelectionClearReason::Ended(LifecycleKind::Destroyed)
        )),
        "and the record says which actor went and why"
    );
    assert_eq!(phase.reticle, None, "no reticle describes it");
    assert_eq!(
        selection.current(),
        None,
        "the held selection really was dropped, not just hidden"
    );

    // Calling the phase again is safe and reports no clear: the first one
    // already applied it.
    let phase = store
        .phase(actor(1), &mut selection, Tick(22))
        .expect("registered");
    assert_eq!(
        phase.cleared, None,
        "a repeated boundary has nothing to clear"
    );
}

/// A re-selection is not a clear: the record describes the new selection and
/// reports no clear, because the consumer's previous target simply changed.
#[test]
fn accept_f30_c_a_reselection_is_not_reported_as_a_clear() {
    let store = store();
    let mut selection = TargetSelection::new();
    store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::Cycle {
                direction: cs_sim::targeting::CycleDirection::Next,
                filter: TargetFilter::Allegiance(Allegiance::Hostile),
            },
        )
        .expect("registered");
    let phase = store
        .phase(actor(1), &mut selection, Tick(20))
        .expect("registered");
    assert_eq!(phase.selection, Some(actor(2)));
    assert_eq!(phase.cleared, None);

    // Cycle to the next raider.
    store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::Cycle {
                direction: cs_sim::targeting::CycleDirection::Next,
                filter: TargetFilter::Allegiance(Allegiance::Hostile),
            },
        )
        .expect("registered");
    let phase = store
        .phase(actor(1), &mut selection, Tick(21))
        .expect("registered");
    assert_eq!(phase.selection, Some(actor(5)));
    assert_eq!(
        phase.cleared, None,
        "moving the selection to another eligible target is not a clear"
    );
    assert_eq!(phase.reticle.expect("a reticle").target, actor(5));
}

/// Every way a selection can stop being eligible reports its own reason, and
/// the reasons are tested in one fixed order so one store gives one answer.
#[test]
fn accept_f30_c_each_ineligibility_has_its_own_reason() {
    let mut store = store();

    // Eligible: no reason at all.
    assert_eq!(store.clear_reason(&actor(2)), None);
    assert!(store.present(&actor(2)));
    assert!(store.eligible(&actor(2)));

    // Reveal lost.
    store.set_revealed(actor(2), false).expect("registered");
    assert_eq!(
        store.clear_reason(&actor(2)),
        Some(SelectionClearReason::NotRevealed)
    );
    assert!(
        store.present(&actor(2)),
        "an actor that merely lost contact is still in the world"
    );
    assert!(!store.eligible(&actor(2)));
    store.set_revealed(actor(2), true).expect("registered");

    // Script phase closed.
    store
        .set_phase_eligible(actor(2), false)
        .expect("registered");
    assert_eq!(
        store.clear_reason(&actor(2)),
        Some(SelectionClearReason::PhaseIneligible)
    );
    assert!(store.present(&actor(2)));
    store
        .set_phase_eligible(actor(2), true)
        .expect("registered");

    // A lifecycle transition wins over both: the reason order is fixed.
    store
        .record_lifecycle(actor(2), LifecycleKind::Despawned)
        .expect("registered");
    store.set_revealed(actor(2), false).expect("registered");
    assert_eq!(
        store.clear_reason(&actor(2)),
        Some(SelectionClearReason::Ended(LifecycleKind::Despawned)),
        "a lifecycle transition is reported before reveal or phase, whatever else is true"
    );
    assert!(
        !store.present(&actor(2)),
        "but it is no longer in the world"
    );

    // A bailout and a capture do not end targetability (the F30-A contract),
    // so neither is a clear reason.
    store.set_revealed(actor(5), true).expect("registered");
    store
        .record_lifecycle(actor(5), LifecycleKind::PilotBailout)
        .expect("registered");
    assert_eq!(store.clear_reason(&actor(5)), None);
    assert!(
        store.eligible(&actor(5)),
        "a bailed-out airframe is still a thing"
    );
    store
        .record_lifecycle(actor(5), LifecycleKind::OwnershipCaptured)
        .expect("registered");
    assert_eq!(
        store.clear_reason(&actor(5)),
        None,
        "a capture changes who owns the actor, not whether it exists"
    );

    // An actor the roster does not hold left the world: that is the only way a
    // selection can name an unregistered actor.
    assert_eq!(
        store.clear_reason(&actor(77)),
        Some(SelectionClearReason::LeftWorld)
    );
    assert!(!store.present(&actor(77)));
}

/// `present` and `eligible` answer different questions, and a threat cue's
/// attacker is judged by `present`: an attack that happened is evidence even
/// after the attacker is gone, but a destroyed attacker is not on the warning
/// list.
#[test]
fn accept_f30_c_present_separates_in_the_world_from_selectable() {
    let mut store = store();
    let hits = [
        hit(0, SYNTHETIC_MOUNT_NODE, actor(5), actor(1), 60),
        hit(1, SYNTHETIC_MOUNT_NODE, actor(9), actor(1), 60),
    ];
    let feed = store.record_hits(&hits).expect("in session");
    assert_eq!(feed.recorded, 2, "both landed hits are attacks");

    let phase = store
        .phase(actor(1), &mut TargetSelection::new(), Tick(60))
        .expect("registered");
    assert_eq!(phase.threats.len(), 2, "both attackers are live cues");

    // Raider 5 lost contact: still a cue, no longer selectable.
    store.set_revealed(actor(5), false).expect("registered");
    assert!(!store.eligible(&actor(5)));
    assert!(store.present(&actor(5)));
    let phase = store
        .phase(actor(1), &mut TargetSelection::new(), Tick(61))
        .expect("registered");
    assert_eq!(
        phase.threats.len(),
        2,
        "an attacker that lost sensor contact is still a live threat"
    );

    // Raider 5 is destroyed: the cue survives as evidence, the attacker is not
    // in the world.
    store
        .record_lifecycle(actor(5), LifecycleKind::Destroyed)
        .expect("registered");
    assert!(!store.present(&actor(5)));
    assert!(!store.eligible(&actor(5)));
    let phase = store
        .phase(actor(1), &mut TargetSelection::new(), Tick(62))
        .expect("registered");
    assert_eq!(
        phase.threats.len(),
        2,
        "the ledger keeps the cue: the attack did happen"
    );
    assert!(
        phase
            .threats
            .iter()
            .any(|cue| cue.attacker == actor(5) && !store.present(&cue.attacker)),
        "and the consumer can tell the two apart with `present`"
    );

    // Unregistering is a different statement: it purges the ledger too.
    store.unregister(actor(5));
    let phase = store
        .phase(actor(1), &mut TargetSelection::new(), Tick(63))
        .expect("registered");
    assert_eq!(
        phase.threats.len(),
        1,
        "an actor that left the world cannot be a live threat cue"
    );
}

/// The guidance query offers a declared hostile and refuses everything else by
/// name: no selection, an unregistered observer, an orphaned selection, an
/// ally, an undeclared pair and a target with no bearing.
#[test]
fn accept_f30_c_guidance_is_offered_to_a_declared_hostile_and_refused_otherwise() {
    let mut store = store();
    let mut selection = TargetSelection::new();

    // Nothing selected.
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(1)),
        Err(TargetError::NoSelection),
        "an absent selection is refused, not answered with a default aid"
    );

    // An unregistered observer.
    assert_eq!(
        store.guidance(actor(42), &mut selection, Tick(1)),
        Err(TargetError::UnknownActor { actor: actor(42) })
    );

    // A declared hostile: the record names the target and a bearing toward it,
    // and nothing else.
    store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::Nearest {
                filter: TargetFilter::Allegiance(Allegiance::Hostile),
            },
        )
        .expect("registered");
    let aid = store
        .guidance(actor(1), &mut selection, Tick(1))
        .expect("offered");
    assert_eq!(aid.target, actor(2));
    assert!(aid.hostile);
    assert_eq!(aid.allegiance, Some(Allegiance::Hostile));
    assert_eq!(
        aid.bearing,
        UnitVec3::try_new([0.0, 1.0, 0.0]).expect("a unit direction"),
        "the bearing points at the target from the observer"
    );
    assert_eq!(aid.position, pos([0.0, 100.0, 0.0]));
    assert_eq!(aid.distance, cs_types::space::Meters(100.0));
    assert!(!aid.threatening);
    assert_eq!(aid.at, Tick(1));

    // A capture turns the selected actor into a declared friendly, and the aid
    // is refused by name with the allegiance that produced the refusal.
    store
        .set_faction(actor(2), cs_sim::targeting::synthetic_player_faction())
        .expect("registered");
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(2)),
        Err(TargetError::NotHostile {
            actor: actor(2),
            allegiance: Some(Allegiance::Friendly),
        }),
        "an aid toward a declared ally is refused, not offered with a flag"
    );

    // An *undeclared* pair is not treated as hostile for the purpose of an aid.
    store
        .set_faction(
            actor(2),
            cs_types::content::ContentId::from_source(
                cs_types::content::ContentKind::Faction,
                "synthetic.undeclared",
            )
            .expect("a valid faction id"),
        )
        .expect("registered");
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(3)),
        Err(TargetError::NotHostile {
            actor: actor(2),
            allegiance: None,
        }),
        "an undeclared relation is an explicit unknown, never a hostility"
    );

    // A target sitting on the observer has no bearing.
    store
        .set_faction(actor(2), synthetic_raider_faction())
        .expect("registered");
    store
        .set_pose(actor(2), pos([0.0, 0.0, 0.0]))
        .expect("registered");
    assert_eq!(
        store.guidance(actor(1), &mut selection, Tick(4)),
        Err(TargetError::DegenerateBearing { actor: actor(2) }),
        "a target on the observer has no direction to point an aid at"
    );
}

/// The guidance record is a copy with no handle into the store: rewriting it
/// changes nothing, and it carries no field a shot could be corrected by.
#[test]
fn accept_f30_c_guidance_record_confers_no_combat_authority() {
    let store = store();
    let mut selection = TargetSelection::new();
    store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::Nearest {
                filter: TargetFilter::Allegiance(Allegiance::Hostile),
            },
        )
        .expect("registered");
    let mut aid = store
        .guidance(actor(1), &mut selection, Tick(1))
        .expect("offered");
    aid.target = actor(9);
    aid.hostile = false;

    // The store is untouched: the aid was a value, not a handle.
    assert_eq!(
        store
            .guidance(actor(1), &mut selection, Tick(2))
            .expect("offered")
            .target,
        actor(2)
    );
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::Allegiance(Allegiance::Hostile))
            .expect("registered"),
        vec![actor(2), actor(5), actor(9)],
        "and the declared hostile cycle is unchanged"
    );
    // The record is exactly the seven fields the contract declares: a target,
    // its position, a bearing toward it, its distance and two live verdicts.
    let expected = WeaponGuidance {
        at: Tick(0),
        target: actor(2),
        position: pos([0.0, 100.0, 0.0]),
        bearing: UnitVec3::try_new([0.0, 1.0, 0.0]).expect("a unit direction"),
        distance: cs_types::space::Meters(100.0),
        allegiance: Some(Allegiance::Hostile),
        hostile: true,
        threatening: false,
    };
    assert_eq!(
        format!("{aid:?}").lines().count(),
        format!("{expected:?}").lines().count(),
        "the record carries no field beyond the declared ones"
    );
}

/// The clear reason is reported with a stable label, and the phase record's
/// field order is fixed, so a consumer's report is reproducible.
#[test]
fn accept_f30_c_clear_reasons_have_stable_labels() {
    assert_eq!(
        SelectionClearReason::Ended(LifecycleKind::Destroyed).label(),
        "destroyed",
        "a lifecycle reason keeps the transition's own label"
    );
    assert_eq!(SelectionClearReason::NotRevealed.label(), "not_revealed");
    assert_eq!(
        SelectionClearReason::PhaseIneligible.label(),
        "phase_ineligible"
    );
    assert_eq!(SelectionClearReason::LeftWorld.label(), "left_world");
    assert_eq!(
        SelectionClearReason::NotRevealed.to_string(),
        "not_revealed",
        "the Display form matches the label"
    );
}

/// The occlusion set stays producer evidence: a target the producer reports as
/// occluded is not selectable through the crosshair, and the consumer views
/// never invent one.
#[test]
fn accept_f30_c_crosshair_occlusion_is_producer_evidence_only() {
    let store = store();
    let mut selection = TargetSelection::new();
    let ray = store
        .crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            None,
            BTreeSet::new(),
        )
        .expect("the declared cone is valid");
    assert_eq!(
        store
            .apply(
                actor(1),
                &mut selection,
                &SelectionRequest::UnderCrosshair(ray.clone())
            )
            .expect("registered"),
        Some(actor(3)),
        "the nearer of the two eligible actors on the ray wins"
    );

    // The producer's occlusion set is honoured, and reaches the views.
    let occluded = store
        .crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            None,
            BTreeSet::from([actor(3)]),
        )
        .expect("the declared cone is valid");
    assert_eq!(
        store
            .apply(
                actor(1),
                &mut selection,
                &SelectionRequest::UnderCrosshair(occluded)
            )
            .expect("registered"),
        Some(actor(9)),
        "an occluded actor is skipped and the one behind it is reached"
    );

    // A corrupt cone is refused at the boundary rather than inside a sort.
    assert!(matches!(
        store.crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            Some(cs_types::space::Radians(f64::NAN)),
            BTreeSet::new(),
        ),
        Err(CrosshairError::NonFiniteCone)
    ));
    assert!(matches!(
        store.crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            Some(cs_types::space::Radians(4.0)),
            BTreeSet::new(),
        ),
        Err(CrosshairError::ConeOutOfRange { .. })
    ));
}

/// A class change and an objective flag are still reached through their own
/// transactions, so the consumer views' classification is never stale.
#[test]
fn accept_f30_c_classification_reaches_the_phase_record_through_its_transaction() {
    let mut store = store();
    let mut selection = TargetSelection::new();
    // The wingman becomes a capital ship and a raider becomes the objective.
    store
        .set_class(actor(3), TargetClass::CapitalShip)
        .expect("registered");
    store.set_objective(actor(9), true).expect("registered");
    store
        .apply(
            actor(1),
            &mut selection,
            &SelectionRequest::Nearest {
                filter: TargetFilter::Objective,
            },
        )
        .expect("registered");
    let phase = store
        .phase(actor(1), &mut selection, Tick(20))
        .expect("registered");
    assert_eq!(
        phase.selection,
        Some(actor(4)),
        "the trader at 60 m is nearer than the newly objective raider at 100 m"
    );
    assert!(phase.reticle.expect("a reticle").objective);
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::Objective)
            .expect("registered"),
        vec![actor(4), actor(9)],
        "and both flagged actors are in the objective cycle"
    );
    assert_eq!(
        store
            .ordered(actor(1), TargetFilter::NotClass(TargetClass::Aircraft))
            .expect("registered"),
        vec![actor(3), actor(4)],
        "the reclassified wingman is no longer an aircraft, so the non-aircraft \\
         cycle now includes it ahead of the trader at 50 m"
    );
}
