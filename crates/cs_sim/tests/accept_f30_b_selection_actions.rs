//! Acceptance scenario F30-B (the minimum scenario — a faction change
//! updates the reticle and the AI hostility gate in the same phase
//! boundary — and its failure cases): the bound selection actions, the
//! threat state's authoritative feed and the phase record's clearing.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-B`. Task test prefix: `accept_f30_b_`.
//!
//! These tests drive production code only: [`cs_sim::targeting`]'s
//! [`TargetStore`], its [`SelectionBinding`]/[`SelectionAction`] command
//! table, [`AttackEvent::from_hit`], [`TargetStore::record_hits`] and
//! [`TargetStore::phase`], plus the synthetic fixtures. The AI half of the
//! minimum scenario is the real [`cs_sim::ai::combat`] planner: the
//! candidate view it decides on is built from the phase record's reticle,
//! so the hostility gate under test is the production gate, not a copy of
//! the assertion.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use std::collections::BTreeSet;

use cs_sim::ai::combat::{
    CandidateVerdict, CombatPlanner, CombatRequest, CombatRole, RejectReason, RoleAssignment,
    synthetic_arsenal, synthetic_combat_planner,
};
use cs_sim::damage::{ActorId, DamageNodeKey, HitEvent, HitEventId, LifecycleKind};
use cs_sim::targeting::{
    Allegiance, SelectionAction, SelectionBinding, SelectionFrame, TargetClass, TargetError,
    TargetFilter, TargetSelection, TargetStore, synthetic_allegiance_table,
    synthetic_player_faction, synthetic_raider_faction, synthetic_roster,
    synthetic_selection_binding, synthetic_target_policy, synthetic_trader_faction,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::input::FlightCommand;
use cs_types::space::{Radians, UnitVec3, WorldPosition};

/// The session generation the fixture roster and the combat planner share.
const SESSION: u64 = 7;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn fixture_store() -> TargetStore {
    let mut store = TargetStore::new(
        SESSION,
        synthetic_target_policy(),
        synthetic_allegiance_table(),
    );
    for record in synthetic_roster(SESSION) {
        store.register(record).expect("fixture records register");
    }
    store
}

fn world(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// One attributable hit from producer 5, in the shape the damage system
/// submits: its own id carries the tick, so the ledger never needs a
/// caller's clock.
fn fixture_hit(
    attacker: Option<ActorId>,
    victim: ActorId,
    tick: Tick,
    producer: u32,
    sequence: u32,
    damage: f64,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: SESSION,
            tick,
            producer,
            sequence,
        },
        attacker,
        victim,
        DamageNodeKey::new("synthetic.cannon").expect("a valid node key"),
        cs_sim::damage::DamageChannel::Internal,
        damage,
    )
    .expect("the fixture hit is well-formed")
}

/// The combat planner's own view of the actor a reticle describes: the
/// candidate view carries the allegiance targeting resolved, which is the
/// seam the AI's hostility gate reads.
fn candidate(reticle: &cs_sim::targeting::Reticle) -> cs_sim::ai::combat::CandidateView {
    cs_sim::ai::combat::CandidateView::new(
        reticle.target,
        reticle.position,
        reticle.allegiance,
        reticle.objective,
    )
}

/// Runs the real combat planner over one candidate and reports the gate's
/// verdict for it.
fn ai_verdict(reticle: &cs_sim::targeting::Reticle) -> CandidateVerdict {
    let planner: CombatPlanner = synthetic_combat_planner();
    let assignment = RoleAssignment::new(actor(1), CombatRole::FighterAttack);
    let arsenal = synthetic_arsenal();
    let decision = planner
        .decide(&CombatRequest {
            observer: actor(1),
            now: Tick(10),
            observer_position: world([0.0, 0.0, 0.0]),
            assignment: &assignment,
            formation: None,
            protected_alive: None,
            candidates: &[candidate(reticle)],
            arsenal: Some(&arsenal),
            profile: None,
        })
        .expect("the request is well-formed");
    decision
        .trace
        .candidate(reticle.target)
        .expect("the candidate is in the trace")
        .verdict
}

/// The bound hostile-cycle action for one command edge, as the session runs
/// it.
fn bound_action(binding: &SelectionBinding, command: FlightCommand) -> SelectionAction {
    binding
        .action(command)
        .cloned()
        .expect("the fixture binds this command")
}

/// AC02 minimum scenario: a faction change updates the reticle **and** the
/// AI hostility gate in the same phase boundary. A selected raider reads as
/// a declared hostile on the reticle and passes the production AI gate; an
/// ownership change — the same actor captured into the player's faction —
/// flips both in the next phase record, and the planner refuses the very
/// candidate the reticle just called a friend. Nothing is cached at
/// selection time, so the two can never disagree.
#[test]
fn accept_f30_b_faction_change_updates_reticle_and_ai_hostility_in_one_phase() {
    let mut store = fixture_store();
    let binding = synthetic_selection_binding();
    let mut selection = TargetSelection::new();

    // The bound target-next edge selects the nearest declared hostile.
    let picked = store
        .act(
            actor(1),
            &mut selection,
            &bound_action(&binding, FlightCommand::TargetNext),
            SelectionFrame::at(Tick(10)),
        )
        .expect("the observer is registered");
    assert_eq!(picked, Some(actor(2)));

    let before = store
        .phase(actor(1), &mut selection, Tick(10))
        .expect("the observer is registered");
    let reticle = before.reticle.expect("a selected target has a reticle");
    assert_eq!(reticle.allegiance, Some(Allegiance::Hostile));
    assert!(reticle.hostile, "a declared hostile is engageable");
    assert!(!reticle.threatening, "no attack has been recorded yet");
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Eligible,
        "the production AI gate accepts a declared hostile"
    );

    // An ownership change: raider 2 is captured into the player's faction.
    store
        .set_faction(actor(2), synthetic_player_faction())
        .expect("registered");
    let after = store
        .phase(actor(1), &mut selection, Tick(11))
        .expect("the observer is registered");
    let reticle = after.reticle.expect("the selection survived the capture");
    assert_eq!(
        reticle.target,
        actor(2),
        "the capture does not drop the selection"
    );
    assert_eq!(reticle.allegiance, Some(Allegiance::Friendly));
    assert!(
        !reticle.hostile,
        "the reticle and the AI gate cannot still call a captured actor hostile"
    );
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Rejected(RejectReason::NotHostile {
            allegiance: Allegiance::Friendly,
        }),
        "the AI gate refuses the same candidate the reticle just reclassified"
    );

    // A scripted relation change reaches the same phase the same way: the
    // factions make peace and the reticle follows the directed pair.
    store
        .set_faction(actor(2), synthetic_raider_faction())
        .expect("registered");
    store.set_allegiance(
        synthetic_player_faction(),
        synthetic_raider_faction(),
        Allegiance::Neutral,
    );
    let after = store
        .phase(actor(1), &mut selection, Tick(12))
        .expect("the observer is registered");
    let reticle = after.reticle.expect("the selection is still held");
    assert_eq!(reticle.allegiance, Some(Allegiance::Neutral));
    assert!(!reticle.hostile);
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Rejected(RejectReason::NotHostile {
            allegiance: Allegiance::Neutral,
        })
    );

    // An undeclared relation is neither hostile nor friendly, and the AI
    // gate says so in its own words.
    store.set_allegiance(
        synthetic_player_faction(),
        synthetic_raider_faction(),
        Allegiance::Hostile,
    );
    let pirates = ContentId::from_source(ContentKind::Faction, "synthetic.pirates")
        .expect("a valid faction id");
    store.set_faction(actor(2), pirates).expect("registered");
    let after = store
        .phase(actor(1), &mut selection, Tick(13))
        .expect("the observer is registered");
    let reticle = after.reticle.expect("the selection is still held");
    assert_eq!(reticle.allegiance, None, "undeclared means unknown");
    assert!(!reticle.hostile);
    assert_eq!(
        ai_verdict(&reticle),
        CandidateVerdict::Rejected(RejectReason::UndeclaredAllegiance)
    );
}

/// The command edges are data, and the session runs them: the two target
/// edges walk the declared hostile cycle in arrival order, a repeated edge
/// advances it, a non-target command changes nothing, and an unbound
/// command is simply not a targeting command.
#[test]
fn accept_f30_b_command_edges_run_the_bound_actions() {
    let store = fixture_store();
    let binding = synthetic_selection_binding();
    let mut selection = TargetSelection::new();

    let cycle = |store: &TargetStore, selection: &mut TargetSelection, command| {
        store
            .act(
                actor(1),
                selection,
                &bound_action(&binding, command),
                SelectionFrame::at(Tick(10)),
            )
            .expect("the observer is registered")
    };
    assert_eq!(
        cycle(&store, &mut selection, FlightCommand::TargetNext),
        Some(actor(2))
    );
    assert_eq!(
        cycle(&store, &mut selection, FlightCommand::TargetNext),
        Some(actor(5)),
        "a second press walks the cycle rather than repeating the pick"
    );
    assert_eq!(
        cycle(&store, &mut selection, FlightCommand::TargetPrev),
        Some(actor(2)),
        "the previous edge walks the same total order backwards"
    );

    // A command the table does not bind is not a targeting command: it is
    // reported as unbound, never guessed into an action.
    assert!(binding.action(FlightCommand::FirePrimary).is_none());
    assert_eq!(binding.len(), 2, "only the two target edges are bound");
    assert_eq!(
        binding
            .bindings()
            .map(|(command, _)| command)
            .collect::<Vec<_>>(),
        vec![FlightCommand::TargetNext, FlightCommand::TargetPrev],
        "the table iterates in stable command order"
    );
    let before = selection.current();
    assert_eq!(
        store
            .act(
                actor(1),
                &mut selection,
                &SelectionAction::Clear,
                SelectionFrame::at(Tick(10)),
            )
            .expect("the observer is registered"),
        None
    );
    assert_eq!(selection.current(), None);
    assert_ne!(before, selection.current(), "clear really dropped it");

    // The under-crosshair action needs the frame's ray: without one it is
    // refused rather than answered with "nothing under the crosshair".
    let mut selection = TargetSelection::new();
    assert_eq!(
        store.act(
            actor(1),
            &mut selection,
            &SelectionAction::UnderCrosshair,
            SelectionFrame::at(Tick(10)),
        ),
        Err(TargetError::MissingCrosshair)
    );
    assert_eq!(
        selection.current(),
        None,
        "a refused action selects nothing"
    );

    // With the producer's ray — built here from the *declared* default cone,
    // because the producer supplied no cone of its own — the action picks
    // the nearer of the two eligible actors on the ray.
    let ray = store
        .crosshair_query(
            world([0.0, 0.0, 0.0]),
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
            None,
            BTreeSet::new(),
        )
        .expect("the declared cone is valid");
    assert_eq!(ray.cone, synthetic_target_policy().crosshair_cone);
    assert_eq!(
        store
            .act(
                actor(1),
                &mut selection,
                &SelectionAction::UnderCrosshair,
                SelectionFrame::at(Tick(10)).with_crosshair(&ray),
            )
            .expect("the observer is registered"),
        Some(actor(3)),
        "the wingman at 50 m wins over the raider at 100 m on the same ray"
    );

    // A corrupt cone is refused at the boundary rather than inside a sort.
    assert!(
        store
            .crosshair_query(
                world([0.0, 0.0, 0.0]),
                UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction"),
                Some(Radians(-1.0)),
                BTreeSet::new(),
            )
            .is_err()
    );
}

/// The threat state's real feed is the damage system's own hits: an
/// attributable hit on a tracked actor mints one cue evidenced by the hit's
/// id, while a hazard hit, a self-hit, a redelivered batch and a hit on an
/// untracked part each do exactly one thing and say so.
#[test]
fn accept_f30_b_threat_state_feeds_from_authoritative_hits() {
    let mut store = fixture_store();
    let hit = |attacker: Option<ActorId>, victim: ActorId, sequence: u32| {
        fixture_hit(attacker, victim, Tick(40), 5, sequence, 12.0)
    };

    // Proximity alone mints nothing.
    assert!(store.threats(actor(1), Tick(40)).is_empty());

    let landed = hit(Some(actor(5)), actor(1), 0);
    let feed = store
        .record_hits(std::slice::from_ref(&landed))
        .expect("the batch is in session");
    assert_eq!(feed.recorded, 1);
    assert_eq!(feed.unattributed, 0);
    assert_eq!(feed.untracked, 0);
    let cues = store.threats(actor(1), Tick(40));
    assert_eq!(
        cues,
        vec![cs_sim::targeting::ThreatCue {
            attacker: actor(5),
            last_attack: Tick(40)
        }],
        "the cue is evidenced by the hit and stamped with the hit's own tick"
    );

    // A redelivered batch is idempotent; a hazard hit and a self-hit credit
    // nobody; a hit on an actor the roster never listed is counted, not
    // refused — damage records exist for parts targeting never listed.
    let hazard = hit(None, actor(1), 1);
    let self_hit = hit(Some(actor(1)), actor(1), 2);
    let untracked = hit(Some(actor(5)), actor(42), 3);
    let feed = store
        .record_hits(&[landed, hazard, self_hit, untracked])
        .expect("the batch is in session");
    assert_eq!(feed.recorded, 0, "the redelivered hit mints nothing new");
    assert_eq!(feed.repeated, 1);
    assert_eq!(
        feed.unattributed, 2,
        "a hazard and a self-hit credit nobody"
    );
    assert_eq!(feed.untracked, 1);
    assert_eq!(store.threats(actor(1), Tick(40)).len(), 1);

    // The whole batch is refused when any part of it belongs to another
    // generation, rather than half-recording the rest.
    let mut foreign = hit(Some(actor(5)), actor(1), 4);
    foreign.id.session = SESSION + 1;
    assert_eq!(
        store.record_hits(&[foreign]),
        Err(TargetError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );

    // A cue is a *live* threat: it expires on the declared window, and the
    // phase record reports the attacker as threatening exactly while it is
    // inside it.
    let mut store = fixture_store();
    store
        .record_hits(&[hit(Some(actor(5)), actor(1), 0)])
        .expect("in session");
    let mut selection = TargetSelection::new();
    let phase = store
        .phase(actor(1), &mut selection, Tick(40))
        .expect("registered");
    assert_eq!(phase.threats.len(), 1);
    assert!(
        store
            .act(
                actor(1),
                &mut selection,
                &SelectionAction::NearestAttacker,
                SelectionFrame::at(Tick(40)),
            )
            .expect("registered")
            .is_some(),
        "the nearest-attacker action finds the attacker the ledger evidenced"
    );
    let reticle = store
        .phase(actor(1), &mut selection, Tick(41))
        .expect("registered")
        .reticle
        .expect("a selection is held");
    assert!(
        reticle.threatening,
        "the reticle warns about the actor that actually attacked"
    );
    let phase = store
        .phase(actor(1), &mut selection, Tick(41 + 120))
        .expect("registered");
    assert!(
        phase.threats.is_empty(),
        "the cue expired on the declared window"
    );
    assert!(
        !store
            .phase(actor(1), &mut selection, Tick(41 + 120))
            .expect("registered")
            .reticle
            .expect("a selection is held")
            .threatening
    );
}

/// AC03's contract half at the F30-B boundary: a destroyed selected target
/// is cleared by the phase record itself, and the attack that destroyed it
/// stays in the ledger as evidence.
#[test]
fn accept_f30_b_destroyed_selection_clears_in_the_phase_record() {
    let mut store = fixture_store();
    let mut selection = TargetSelection::new();
    store
        .act(
            actor(1),
            &mut selection,
            &SelectionAction::Cycle {
                direction: cs_sim::targeting::CycleDirection::Next,
                filter: TargetFilter::Allegiance(Allegiance::Hostile),
            },
            SelectionFrame::at(Tick(10)),
        )
        .expect("registered");
    assert_eq!(selection.current(), Some(actor(2)));

    let blow = fixture_hit(Some(actor(9)), actor(2), Tick(20), 5, 0, 40.0);
    store
        .record_hits(std::slice::from_ref(&blow))
        .expect("in session");
    store
        .record_lifecycle(actor(2), LifecycleKind::Destroyed)
        .expect("registered");

    let phase = store
        .phase(actor(1), &mut selection, Tick(20))
        .expect("registered");
    assert_eq!(phase.selection, None, "a destroyed target clears");
    assert_eq!(phase.reticle, None, "and no reticle describes it");
    assert!(
        phase.threats.is_empty(),
        "and the observer has no threat of its own to report"
    );
    assert_eq!(
        store.threats(actor(2), Tick(20)),
        vec![cs_sim::targeting::ThreatCue {
            attacker: actor(9),
            last_attack: Tick(20)
        }],
        "destruction does not erase the attack that caused it: the record of \\
         what killed an actor is evidence, even though the actor can no longer \\
         be selected"
    );

    // A bailout is not a destruction: the airframe is still there.
    let mut store = fixture_store();
    store
        .record_lifecycle(actor(2), LifecycleKind::PilotBailout)
        .expect("registered");
    assert!(store.eligible(&actor(2)));
}

/// The roster's own transaction: unregistering an actor takes it out of the
/// roster *and* out of every ledger, so an actor that left the world can
/// never be a live cue, while a destroyed one keeps its evidence.
#[test]
fn accept_f30_b_unregistered_actor_leaves_roster_and_ledger() {
    let mut store = fixture_store();
    let blow = fixture_hit(Some(actor(5)), actor(1), Tick(5), 5, 0, 3.0);
    store
        .record_hits(std::slice::from_ref(&blow))
        .expect("in session");
    assert_eq!(store.threats(actor(1), Tick(5)).len(), 1);

    store.unregister(actor(5));
    assert!(!store.is_registered(&actor(5)));
    assert!(
        store.threats(actor(1), Tick(5)).is_empty(),
        "an actor that left the world is not a live threat"
    );
    assert!(!store.registered().contains(&actor(5)));

    // Unregistering again, or with a foreign identity, is a no-op: two
    // systems noticing the same departure cannot fail each other.
    store.unregister(actor(5));
    store.unregister(actor(99));
    store.unregister(ActorId {
        session: SESSION + 1,
        serial: 5,
    });
    assert_eq!(store.registered().len(), 6);

    // The victim's own ledger entry goes with the actor.
    store.unregister(actor(1));
    assert!(!store.is_registered(&actor(1)));
    assert_eq!(store.registered().len(), 5, "the rest of the roster stands");
    assert!(
        store.threats(actor(1), Tick(5)).is_empty(),
        "an unregistered victim has no ledger entry at all"
    );
}

/// Non-negotiable 5: the queries read canonical f64 world positions, so a
/// uniform rebase — every actor moved by the same huge offset, as a world
/// rebase expresses itself — leaves the order, the selection and the
/// reported distance relationships untouched. A local f32 pose would lose
/// the metre at this magnitude and reorder the cycle.
#[test]
fn accept_f30_b_a_rebase_leaves_targets_and_order_intact() {
    let store = fixture_store();
    let mut selection = TargetSelection::new();
    let mut rebased = store.clone();
    let offset = 5.0e7_f64;
    for actor_id in store.registered() {
        let position = store.record(&actor_id).expect("registered").position;
        let [x, y, z] = position.to_array();
        rebased
            .set_pose(actor_id, world([x + offset, y - offset, z + 0.5 * offset]))
            .expect("registered");
    }

    let filter = TargetFilter::Allegiance(Allegiance::Hostile);
    assert_eq!(
        rebased.ordered(actor(1), filter).expect("registered"),
        store.ordered(actor(1), filter).expect("registered"),
        "a uniform rebase does not reorder the cycle"
    );

    let walk = |store: &TargetStore| {
        let mut selection = TargetSelection::new();
        (0..4)
            .map(|_| {
                store
                    .act(
                        actor(1),
                        &mut selection,
                        &SelectionAction::Cycle {
                            direction: cs_sim::targeting::CycleDirection::Next,
                            filter,
                        },
                        SelectionFrame::at(Tick(10)),
                    )
                    .expect("registered")
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        walk(&rebased),
        walk(&store),
        "and does not change the cycle sequence"
    );

    // The reticle's distance is the difference between canonical positions,
    // so it survives the rebase too.
    let before = store
        .phase(actor(1), &mut selection, Tick(10))
        .expect("registered")
        .reticle;
    let after = rebased
        .phase(actor(1), &mut selection, Tick(10))
        .expect("registered")
        .reticle;
    assert_eq!(
        before.map(|reticle| reticle.distance),
        after.map(|reticle| reticle.distance)
    );
}

/// The declared filter vocabulary reaches the actions, so a bound
/// "next objective" edge selects the objective and a "next non-aircraft"
/// edge the same, and neither silently means the hostile cycle.
#[test]
fn accept_f30_b_declared_filters_reach_the_bound_actions() {
    let store = fixture_store();
    let mut selection = TargetSelection::new();
    let pick = |store: &TargetStore, selection: &mut TargetSelection, action| {
        store
            .act(actor(1), selection, &action, SelectionFrame::at(Tick(10)))
            .expect("registered")
    };

    assert_eq!(
        pick(
            &store,
            &mut selection,
            SelectionAction::Nearest {
                filter: TargetFilter::Objective
            }
        ),
        Some(actor(4)),
        "the objective action selects the declared objective"
    );
    assert_eq!(
        pick(
            &store,
            &mut selection,
            SelectionAction::Nearest {
                filter: TargetFilter::NotClass(TargetClass::Aircraft)
            }
        ),
        Some(actor(4)),
        "the non-aircraft action selects the world object"
    );
    assert_eq!(
        pick(
            &store,
            &mut selection,
            SelectionAction::Nearest {
                filter: TargetFilter::Allegiance(Allegiance::Friendly)
            }
        ),
        Some(actor(3)),
        "the ally action selects the wingman"
    );
    assert_eq!(
        pick(
            &store,
            &mut selection,
            SelectionAction::Nearest {
                filter: TargetFilter::Allegiance(Allegiance::Neutral)
            }
        ),
        Some(actor(4)),
        "the neutral action selects the trader, not an undeclared pair"
    );
    assert_eq!(
        selection.current(),
        Some(actor(4)),
        "and the reticle reads the selected world object as an objective"
    );
    let reticle = store
        .phase(actor(1), &mut selection, Tick(10))
        .expect("registered")
        .reticle
        .expect("a selection is held");
    assert_eq!(reticle.class, TargetClass::WorldObject);
    assert_eq!(reticle.faction, synthetic_trader_faction());
    assert!(reticle.objective);
    assert!(
        (reticle.distance.0 - 60.0).abs() < 1e-9,
        "the reticle reports the canonical distance"
    );
    assert!(!reticle.hostile, "a neutral objective is not a hostility");
}
