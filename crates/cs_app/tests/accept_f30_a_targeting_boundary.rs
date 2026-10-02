//! Acceptance scenario F30-A: the declared → runtime conversion boundary
//! and the ECS binding record.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`. Task test prefix: `accept_f30_a_`.
//!
//! These tests drive production code only: [`cs_app::targeting`]'s
//! [`lower_rules`] and [`TargetableBinding`], plus the
//! `cs_sim::targeting::TargetStore` the lowered records feed — the AC01
//! equal-distance cycle runs end-to-end through the lowered rules, so
//! removing the conversion or silently defaulting an unknown fails them.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_app::scene::SceneGeneration;
use cs_app::targeting::{TargetLowerError, TargetableBinding, lower_rules};
use cs_content::target_rules::{
    DeclaredAllegiance, DeclaredRelation, DeclaredTargetRules, TargetRuleSet,
    declared_synthetic_target_rules,
};
use cs_sim::damage::ActorId;
use cs_sim::targeting::{
    Allegiance, CycleDirection, SelectionRequest, TargetFilter, TargetSelection, TargetStore,
    synthetic_player_faction, synthetic_raider_faction, synthetic_roster, synthetic_trader_faction,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::Radians;

const SESSION: u64 = 5;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn faction(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Faction, key).expect("test faction ids are valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn claim() -> ClaimId {
    ClaimId::new("f30a.boundary-test").expect("valid claim id")
}

/// `lower_rules` preserves the whole declared structure: the policy knobs
/// map field-wise, the two assistance options lower separately, and every
/// directed relation lands in the runtime table — nothing is dropped,
/// reordered or symmetrized.
#[test]
fn accept_f30_a_lower_rules_preserves_the_declared_structure() {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture lowers");

    assert_eq!(lowered.policy.threat_window_ticks, 120);
    assert_eq!(lowered.policy.crosshair_cone.0, std::f64::consts::PI / 18.0);
    // The assistance options are separate options (non-negotiable 3):
    // the fixture declares the indicator on and assistance off.
    assert!(lowered.lead_indicator);
    assert!(!lowered.aim_assistance);

    let (player, raiders, traders) = (
        synthetic_player_faction(),
        synthetic_raider_faction(),
        synthetic_trader_faction(),
    );
    assert_eq!(lowered.allegiance.len(), 6, "directed pairs stay directed");
    assert_eq!(
        lowered.allegiance.get(&player, &raiders),
        Some(Allegiance::Hostile)
    );
    assert_eq!(
        lowered.allegiance.get(&raiders, &player),
        Some(Allegiance::Hostile)
    );
    assert_eq!(
        lowered.allegiance.get(&player, &traders),
        Some(Allegiance::Neutral)
    );
    assert_eq!(
        lowered.allegiance.get(&player, &player),
        Some(Allegiance::Friendly),
        "self-allegiance is the contract invariant"
    );
    let outsiders = faction("synthetic.outsiders");
    assert_eq!(
        lowered.allegiance.get(&player, &outsiders),
        None,
        "an undeclared pair lowers to no relation, not a guessed one"
    );
}

/// An unresolved rule value refuses to lower — a session never runs
/// targeting under a guessed window, cone or assistance flag — and an
/// unresolved *relation* refuses too, because "unknown" is not "no
/// relation".
#[test]
fn accept_f30_a_unknowns_refuse_to_lower() {
    let (player, raiders) = (faction("synthetic.nathan"), faction("synthetic.raiders"));
    let subject =
        || ContentId::from_source(ContentKind::IaScenario, "synthetic.test").expect("valid");

    let mut rules = TargetRuleSet {
        threat_window: known(120),
        crosshair_cone: known(Radians(std::f64::consts::PI / 18.0)),
        lead_indicator: known(true),
        aim_assistance: known(false),
    };
    rules.threat_window = Resolved::unknown(claim(), "unmeasured").expect("reason");
    let declared = DeclaredTargetRules::try_new(
        subject(),
        Origin::SyntheticFixture,
        vec![player.clone(), raiders.clone()],
        vec![DeclaredRelation {
            from: player.clone(),
            to: raiders.clone(),
            allegiance: known(DeclaredAllegiance::Hostile),
        }],
        rules,
        Provenance::designed(claim()),
    )
    .expect("declared record with an unknown rule is valid");
    assert_eq!(
        lower_rules(&declared),
        Err(TargetLowerError::UnknownRule {
            field: "threat_window",
            claim_id: claim(),
            reason: "unmeasured".to_owned(),
        })
    );

    let declared = DeclaredTargetRules::try_new(
        subject(),
        Origin::SyntheticFixture,
        vec![player.clone(), raiders.clone()],
        vec![DeclaredRelation {
            from: player.clone(),
            to: raiders.clone(),
            allegiance: Resolved::unknown(claim(), "the original relation is unmeasured")
                .expect("reason"),
        }],
        TargetRuleSet {
            threat_window: known(120),
            crosshair_cone: known(Radians(std::f64::consts::PI / 18.0)),
            lead_indicator: known(true),
            aim_assistance: known(false),
        },
        Provenance::designed(claim()),
    )
    .expect("declared record with an unknown relation is valid");
    assert_eq!(
        lower_rules(&declared),
        Err(TargetLowerError::UnknownRelation {
            relation: 0,
            claim_id: claim(),
            reason: "the original relation is unmeasured".to_owned(),
        })
    );
}

/// The lowered rules drive the runtime store end to end: AC01's
/// equal-distance cycle through the boundary produces the declared
/// deterministic order.
#[test]
fn accept_f30_a_lowered_rules_drive_the_equal_distance_cycle() {
    let lowered = lower_rules(&declared_synthetic_target_rules()).expect("the fixture lowers");
    let mut store = TargetStore::new(SESSION, lowered.policy, lowered.allegiance);
    for record in synthetic_roster(SESSION) {
        store.register(record).expect("fixture records register");
    }

    let mut selection = TargetSelection::new();
    let cycle: Vec<ActorId> = (0..4)
        .map(|_| {
            store
                .apply(
                    actor(1),
                    &mut selection,
                    &SelectionRequest::Cycle {
                        direction: CycleDirection::Next,
                        filter: TargetFilter::Allegiance(Allegiance::Hostile),
                    },
                )
                .expect("registered")
                .expect("hostiles exist")
        })
        .collect();
    assert_eq!(
        cycle,
        vec![actor(2), actor(5), actor(9), actor(2)],
        "the equal-distance cycle follows the stable actor-id order"
    );
}

/// The binding record ties an entity to its session-qualified actor and
/// the rules subject under the scene generation that spawned it.
#[test]
fn accept_f30_a_targetable_binding_is_generation_stamped() {
    let declared = declared_synthetic_target_rules();
    let binding = TargetableBinding {
        actor: actor(9),
        rules: declared.subject().clone(),
        generation: SceneGeneration(3),
    };
    assert_eq!(binding.actor.session, session(SESSION));
    assert_eq!(binding.rules.as_str(), "ia_scenario/synthetic.target-range");
    assert_eq!(binding.generation, SceneGeneration(3));

    // A different generation is a different binding — a reload can never
    // alias a stale entity.
    let stale = TargetableBinding {
        generation: SceneGeneration(2),
        ..binding.clone()
    };
    assert_ne!(binding, stale);
}
