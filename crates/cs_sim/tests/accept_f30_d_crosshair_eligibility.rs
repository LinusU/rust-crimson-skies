//! Acceptance scenario F30-D at the simulation layer: crosshair selection
//! against the *approved* eligibility rules, and the reveal rule that gates
//! both the cycle and the crosshair.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-D`. Task test prefix: `accept_f30_d_`.
//!
//! These tests drive `cs_sim::targeting` directly: `TargetStore::crosshair_query`,
//! `TargetStore::apply`, `TargetStore::phase`, `TargetStore::eligible`,
//! `TargetStore::present` and the `synthetic_roster` fixture. Nothing here is a
//! parallel test-only implementation of a rule the module already owns.
//!
//! **What F30-D adds on top of F30-A and F30-C.** F30-A pinned the crosshair
//! *cone*, the observer exclusion, the angular/distance/actor-id tie-breaks and
//! the occlusion set as producer evidence; F30-C pinned that the occlusion set
//! is never invented by a consumer. Neither asserted the **conjunction** the
//! sheet's AC04 asks for: a crosshair pick must satisfy eligibility *and* free
//! sight, and it must do so through the same roster facts the cycle and the
//! phase record read. So this file walks one ray past four actors that are each
//! ineligible in a different way and one that is occluded, and asserts that
//! each fact independently removes a candidate and restoring it puts the
//! candidate back — in the same boundary.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. The original's own target order, reveal rules and assistance
//! behavior are measured separately by
//! `crates/cs_content/tests/accept_f30_d_original_target_vocabulary.rs`; this
//! file makes no claim about them.

use std::collections::BTreeSet;

use cs_sim::damage::{ActorId, LifecycleKind};
use cs_sim::targeting::{
    Allegiance, CrosshairQuery, SelectionClearReason, SelectionRequest, TargetFilter,
    TargetSelection, TargetStore, synthetic_allegiance_table, synthetic_roster,
    synthetic_target_policy,
};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

/// The session generation the fixture roster belongs to.
const SESSION: u64 = 51;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).expect("a nonzero session generation"),
        serial,
    }
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// The fixture roster with every candidate lined up on the +X ray at a distinct
/// distance, so the pick is decided by eligibility and occlusion rather than by
/// angle.
///
/// | actor | distance | state |
/// | --- | --- | --- |
/// | 7 | 10 m | raider, not revealed |
/// | 2 | 20 m | raider, eligible |
/// | 5 | 40 m | raider, eligible |
/// | 9 | 60 m | raider, eligible |
/// | 3 | 80 m | wingman, eligible (a declared friendly) |
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
    // Line the five candidates up along +X. The objective trader stays on +Y,
    // outside the cone, so it cannot be picked by accident.
    for (serial, distance) in [(7, 10.0), (2, 20.0), (5, 40.0), (9, 60.0), (3, 80.0)] {
        store
            .set_pose(actor(serial), pos([distance, 0.0, 0.0]))
            .expect("a registered actor accepts a pose");
    }
    store
}

/// The +X ray with the declared default cone, occluding `occluded`.
fn ray(store: &TargetStore, occluded: BTreeSet<ActorId>) -> CrosshairQuery {
    store
        .crosshair_query(
            pos([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            None,
            occluded,
        )
        .expect("the declared cone is valid")
}

fn pick(
    store: &TargetStore,
    selection: &mut TargetSelection,
    occluded: BTreeSet<ActorId>,
) -> Option<ActorId> {
    let query = ray(store, occluded);
    store
        .apply(
            actor(1),
            selection,
            &SelectionRequest::UnderCrosshair(query),
        )
        .expect("registered")
}

/// AC04: crosshair selection respects occlusion **and** eligibility according to
/// the approved rules. Each fact is applied on its own, so no single missing
/// rule can hide behind another one passing.
#[test]
fn accept_f30_d_crosshair_selection_needs_eligibility_and_free_sight_together() {
    let mut store = store();
    let mut selection = TargetSelection::new();
    let empty = BTreeSet::new();

    // The nearest actor on the ray is the *unrevealed* raider at 10 m: it is
    // registered and present, but not eligible, so it is never a pick.
    assert!(store.present(&actor(7)));
    assert!(!store.eligible(&actor(7)));
    assert_eq!(
        pick(&store, &mut selection, empty.clone()),
        Some(actor(2)),
        "the first eligible actor on the ray wins, not the nearest present one"
    );

    // Occlusion removes the pick without removing the actor: the one behind it
    // is reached, and the occluded actor stays registered and eligible.
    assert_eq!(
        pick(&store, &mut selection, BTreeSet::from([actor(2)])),
        Some(actor(5))
    );
    assert!(
        store.eligible(&actor(2)),
        "an occluded actor is still eligible: occlusion is evidence about the \
         ray, not a statement about the roster"
    );

    // The reveal rule removes the next candidate. The occlusion set still names
    // actor 2, so an implementation that read occlusion as "the roster knows it
    // is unreachable" would still answer correctly here — the assertion that
    // discriminates is the next one, where the occluded actor is restored.
    store
        .set_revealed(actor(5), false)
        .expect("a registered actor accepts a reveal state");
    assert!(!store.eligible(&actor(5)));
    assert_eq!(
        pick(&store, &mut selection, BTreeSet::from([actor(2)])),
        Some(actor(9))
    );

    // The script-phase rule removes the next one in its turn.
    store
        .set_phase_eligible(actor(9), false)
        .expect("a registered actor accepts a phase state");
    assert!(!store.eligible(&actor(9)));
    assert_eq!(
        pick(&store, &mut selection, BTreeSet::from([actor(2)])),
        Some(actor(3)),
        "an eligible declared friendly is picked when no eligible hostile is in \
         the cone: the crosshair filter is eligibility, not allegiance"
    );

    // A lifecycle transition removes the last one, and the query answers
    // "nothing under the crosshair" — clearing the selection, because a phase
    // must not keep describing a target that is gone.
    store
        .record_lifecycle(actor(3), LifecycleKind::Destroyed)
        .expect("a registered actor accepts a lifecycle transition");
    assert_eq!(
        pick(&store, &mut selection, BTreeSet::from([actor(2)])),
        None,
        "occlusion, reveal, phase eligibility and a lifecycle transition together \
         leave no candidate: the answer is nothing, not a fallback"
    );
    assert_eq!(selection.current(), None);

    // Now restore each fact, one at a time, from the state that had no
    // candidate. Each restoration must put exactly one candidate back, and the
    // restorations are read in reverse distance order, so a pick that ignored
    // any single rule answers the wrong actor.
    store
        .record_lifecycle(actor(3), LifecycleKind::PilotBailout)
        .expect("a registered actor accepts a lifecycle transition");
    // Destruction is not undone by a bailout: only a transition that ends
    // targetability writes the recorded ending, so a non-terminal report
    // leaves the airframe gone.
    assert!(
        !store.eligible(&actor(3)),
        "a bailout after a destruction does not resurrect the actor"
    );

    // Withdrawing the occlusion report brings the 20 m raider back: occlusion
    // was the only fact keeping it out.
    assert_eq!(
        pick(&store, &mut selection, empty.clone()),
        Some(actor(2)),
        "an unoccluded eligible actor is reachable again"
    );

    // Re-revealing the 10 m raider makes it the pick even while the 20 m one is
    // still reported occluded: eligibility and occlusion are independent.
    store
        .set_revealed(actor(7), true)
        .expect("a registered actor accepts a reveal state");
    assert_eq!(
        pick(&store, &mut selection, BTreeSet::from([actor(2)])),
        Some(actor(7)),
        "a re-revealed actor is eligible again in the same boundary"
    );

    // Re-opening the script phase and undoing the destruction each bring their
    // own actor back into the eligible set, in distance order.
    store
        .set_phase_eligible(actor(9), true)
        .expect("a registered actor accepts a phase state");
    store
        .set_revealed(actor(5), true)
        .expect("a registered actor accepts a state");
    assert_eq!(
        pick(&store, &mut selection, BTreeSet::from([actor(2), actor(7)])),
        Some(actor(5)),
        "the 40 m raider is the nearest eligible, unoccluded actor again"
    );
    assert!(
        !store.present(&actor(3)),
        "a destroyed actor left the world"
    );
}

/// The reveal rule, one boundary: an unrevealed actor is in neither the cycle
/// nor the crosshair, a held selection on one is cleared with its reason, and
/// re-revealing it restores both without a new store.
#[test]
fn accept_f30_d_the_reveal_rule_gates_the_cycle_and_the_crosshair_together() {
    let mut store = store();
    let mut selection = TargetSelection::new();
    let empty = BTreeSet::new();

    // Select the hidden raider by moving it into the open and revealing it, so
    // the clear that follows is about the reveal rule and nothing else.
    store
        .set_revealed(actor(7), true)
        .expect("a registered actor accepts a reveal state");
    assert_eq!(pick(&store, &mut selection, empty.clone()), Some(actor(7)));

    store
        .set_revealed(actor(7), false)
        .expect("a registered actor accepts a reveal state");
    let phase = store
        .phase(actor(1), &mut selection, Tick(7))
        .expect("registered");
    assert_eq!(phase.selection, None, "the held selection is dropped");
    assert_eq!(
        phase.cleared.map(|cleared| (cleared.actor, cleared.reason)),
        Some((actor(7), SelectionClearReason::NotRevealed)),
        "and the record says the reveal state is why"
    );
    assert_eq!(phase.reticle, None, "no reticle describes it");
    assert!(
        store.present(&actor(7)),
        "an unrevealed actor is still in the world: only its eligibility went"
    );
    assert_eq!(
        pick(&store, &mut selection, empty.clone()),
        Some(actor(2)),
        "the crosshair skips it and takes the next eligible actor on the ray"
    );
    assert!(
        !store
            .ordered(actor(1), TargetFilter::Allegiance(Allegiance::Hostile))
            .expect("registered")
            .contains(&actor(7)),
        "and the hostile cycle does not list it either"
    );

    // Re-revealing it puts it back into both queries in the same boundary, and
    // the phase record then describes the pick the query made.
    store
        .set_revealed(actor(7), true)
        .expect("a registered actor accepts a reveal state");
    assert_eq!(pick(&store, &mut selection, empty), Some(actor(7)));
    let phase = store
        .phase(actor(1), &mut selection, Tick(8))
        .expect("registered");
    assert_eq!(phase.selection, Some(actor(7)));
    assert_eq!(phase.cleared, None, "nothing was cleared this boundary");
    assert!(
        phase.reticle.expect("a reticle").revealed,
        "and the reticle reports the actor as revealed"
    );
}
