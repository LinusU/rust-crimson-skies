//! Acceptance scenario F30-A: the declared targeting schema validates
//! its identity discipline and keeps unknowns explicit.
//!
//! Spec: `specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`. Task test prefix: `accept_f30_a_`.
//!
//! These tests drive production code only:
//! [`cs_content::target_rules`]'s [`DeclaredTargetRules::try_new`]
//! validation and the `declared_synthetic_target_rules` fixture. Removing
//! the validation or defaulting an unknown fails a test or fails to
//! compile.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_content::target_rules::{
    DeclaredAllegiance, DeclaredRelation, DeclaredTargetRules, TargetRuleSet, TargetRulesError,
    declared_synthetic_target_rules,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

fn faction(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Faction, key).expect("test faction ids are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f30a.test-rules").expect("valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn relation(from: &ContentId, to: &ContentId, allegiance: DeclaredAllegiance) -> DeclaredRelation {
    DeclaredRelation {
        from: from.clone(),
        to: to.clone(),
        allegiance: known(allegiance),
    }
}

fn rule_set() -> TargetRuleSet {
    TargetRuleSet {
        threat_window: known(120),
        crosshair_cone: known(Radians(std::f64::consts::PI / 18.0)),
        lead_indicator: known(true),
        aim_assistance: known(false),
    }
}

fn build(
    factions: Vec<ContentId>,
    relations: Vec<DeclaredRelation>,
    rules: TargetRuleSet,
) -> Result<DeclaredTargetRules, TargetRulesError> {
    DeclaredTargetRules::try_new(
        ContentId::from_source(ContentKind::IaScenario, "synthetic.test").expect("valid"),
        Origin::SyntheticFixture,
        factions,
        relations,
        rules,
        Provenance::designed(claim()),
    )
}

/// The declared synthetic fixture assembles: three factions, six directed
/// relations and a fully known rule set — and its provenance says what it
/// is (synthetic fixture, designed claims, never original data).
#[test]
fn accept_f30_a_synthetic_fixture_validates_and_carries_provenance() {
    let rules = declared_synthetic_target_rules();

    assert_eq!(
        rules.subject().as_str(),
        "ia_scenario/synthetic.target-range"
    );
    assert_eq!(rules.origin(), &Origin::SyntheticFixture);
    assert_eq!(rules.factions().len(), 3);
    assert_eq!(rules.relations().len(), 6);
    for faction in rules.factions() {
        assert_eq!(faction.kind(), ContentKind::Faction);
    }
    let set = rules.rules();
    assert_eq!(set.threat_window.clone().known(), Some(120));
    assert_eq!(set.lead_indicator.clone().known(), Some(true));
    assert_eq!(set.aim_assistance.clone().known(), Some(false));
    for relation in rules.relations() {
        assert!(relation.allegiance.is_known());
    }
}

/// The identity discipline: a non-faction id is not a faction, duplicate
/// factions and duplicated directed pairs are refused, and a relation
/// names only declared factions — an undeclared endpoint cannot smuggle
/// an implicit faction into the table.
#[test]
fn accept_f30_a_schema_refuses_bad_identities_and_relations() {
    let (player, raiders, traders) = (
        faction("synthetic.nathan"),
        faction("synthetic.raiders"),
        faction("synthetic.traders"),
    );
    let factions = || vec![player.clone(), raiders.clone(), traders.clone()];

    // A mission id is not a faction.
    let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("valid");
    assert_eq!(
        build(vec![mission.clone()], vec![], rule_set()),
        Err(TargetRulesError::FactionKindMismatch { faction: mission })
    );

    // Duplicate faction ids collapse identity; refuse.
    assert_eq!(
        build(vec![player.clone(), player.clone()], vec![], rule_set()),
        Err(TargetRulesError::DuplicateFaction {
            faction: player.clone()
        })
    );

    // A relation endpoint must be a declared faction.
    let pirates = faction("synthetic.pirates");
    assert_eq!(
        build(
            factions(),
            vec![relation(&pirates, &raiders, DeclaredAllegiance::Hostile)],
            rule_set()
        ),
        Err(TargetRulesError::UndeclaredFaction {
            relation: 0,
            missing: pirates.clone(),
        })
    );

    // A relation cannot relate a faction to itself: self-allegiance is
    // the contract's own invariant, not data.
    assert_eq!(
        build(
            factions(),
            vec![relation(&raiders, &raiders, DeclaredAllegiance::Friendly)],
            rule_set()
        ),
        Err(TargetRulesError::SelfRelation {
            faction: raiders.clone()
        })
    );

    // The same directed pair twice is refused; the *reverse* pair is a
    // separate declaration and is accepted.
    assert_eq!(
        build(
            factions(),
            vec![
                relation(&player, &raiders, DeclaredAllegiance::Hostile),
                relation(&player, &raiders, DeclaredAllegiance::Neutral),
            ],
            rule_set()
        ),
        Err(TargetRulesError::DuplicateRelation {
            from: player.clone(),
            to: raiders.clone(),
        })
    );
    assert!(
        build(
            factions(),
            vec![
                relation(&player, &raiders, DeclaredAllegiance::Hostile),
                relation(&raiders, &player, DeclaredAllegiance::Neutral),
            ],
            rule_set()
        )
        .is_ok(),
        "relations are directed: the reverse pair is its own record"
    );

    // A non-faction relation endpoint is refused before membership.
    let mission = ContentId::from_source(ContentKind::Mission, "m02").expect("valid");
    assert_eq!(
        build(
            factions(),
            vec![relation(&mission, &raiders, DeclaredAllegiance::Hostile)],
            rule_set()
        ),
        Err(TargetRulesError::RelationEndpointKind { endpoint: mission })
    );
}

/// A malformed known cone is refused; an `Unknown` rule value is
/// preserved as an explicit unknown — the schema records it instead of
/// defaulting it, and the lowering boundary (cs_app) is what refuses it.
#[test]
fn accept_f30_a_unknowns_survive_and_bad_cones_fail() {
    let (player, raiders) = (faction("synthetic.nathan"), faction("synthetic.raiders"));
    let factions = || vec![player.clone(), raiders.clone()];

    let mut rules = rule_set();
    rules.crosshair_cone = known(Radians(f64::NAN));
    assert_eq!(
        build(factions(), vec![], rules),
        Err(TargetRulesError::NonFiniteCone)
    );

    let mut rules = rule_set();
    rules.crosshair_cone = known(Radians(4.0));
    assert_eq!(
        build(factions(), vec![], rules),
        Err(TargetRulesError::ConeOutOfRange { radians: 4.0 })
    );

    // Unknowns are representable records, not defaults.
    let mut rules = rule_set();
    rules.threat_window = Resolved::unknown(claim(), "the original threat window is unmeasured")
        .expect("nonempty reason");
    rules.aim_assistance =
        Resolved::unknown(claim(), "the original assistance option is unverified")
            .expect("nonempty reason");
    let declared = build(
        factions(),
        vec![relation(&player, &raiders, DeclaredAllegiance::Hostile)],
        rules,
    )
    .expect("unknowns are valid declared records");
    assert!(!declared.rules().threat_window.is_known());
    assert!(!declared.rules().aim_assistance.is_known());
}
