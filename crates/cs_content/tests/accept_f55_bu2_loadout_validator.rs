//! F55-BU2 acceptance: the production lobby loadout judgement over the shared
//! `cs_content::construction` validator.
//!
//! Every test builds an authored [`AircraftBlueprint`] catalog and calls the
//! production [`LobbyConstructionCatalog::judge`] — the same function
//! `cs_app::ui::lobby::ConstructionLoadoutValidator` delegates the
//! `LoadoutValidator` hook to. No test double stands in for the validator.
//!
//! The mapping the module documents is exercised directly: a loadout whose
//! component list is not the authored blueprint's is refused before judging
//! (`loadout_mismatch`), and an authored blueprint is refused by the shared
//! rules with a stable code per budget breach and per constraint violation.

use std::collections::BTreeSet;

use cs_content::construction::{
    AircraftBlueprint, ArmorFitment, ConstructionPolicy, ConstructionRules, GunFitment, MoneyMinor,
    OrdnanceFitment, SYNTHETIC_AIRFRAME_KEY, SYNTHETIC_BLUEPRINT_KEY, SYNTHETIC_ENGINE_KEY,
    SYNTHETIC_HEAVY_MISSILE_KEY, SYNTHETIC_HEAVY_PLATE_KEY, SYNTHETIC_MISSILE_KEY,
    SYNTHETIC_RADIO_KEY, SYNTHETIC_UNPRICED_PLATE_KEY, WeightUnits, declared_synthetic_blueprint,
    declared_synthetic_price_book, synthetic_armor_fitments, synthetic_boundary_rules,
    synthetic_gun_fitments, synthetic_ordnance_fitments, synthetic_paint_selection,
    synthetic_policy, synthetic_unmeasured_limit_rules,
};
use cs_content::damage::DamageNodeKey;
use cs_content::lobby_loadout::{
    BANNED_COMPONENT, GUN_NOT_PAIRABLE, LOADOUT_MISMATCH, LobbyConstructionCatalog, OVER_COST,
    OVER_GUN_POSITIONS, OVER_HARDPOINTS, OVER_MASS, UNAVAILABLE_COMPONENT, UNKNOWN_BLUEPRINT,
    UNKNOWN_PAIRING_RULE, UNKNOWN_RULES, UNMEASURED_BUDGET, UNSUPPORTED_GUN_SELECTION,
};
use cs_types::content::{
    ContentId, ContentKind, Known, Origin, Provenance, Resolved as ResolvedValue,
};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("the test claim id is valid")
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the test content id is valid")
}

fn node(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("the test damage node key is valid")
}

fn known<T>(value: T) -> ResolvedValue<T> {
    ResolvedValue::Known(Known::new(
        value,
        Provenance::designed(claim("f55bu2.test.value")),
    ))
}

fn airframe() -> ContentId {
    id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY)
}

fn engine() -> ContentId {
    id(ContentKind::Engine, SYNTHETIC_ENGINE_KEY)
}

/// A rule profile for the fixture airframe with the given rack and the fixture
/// boundary's mass and cost ceilings.
fn rack_rules(gun_positions: u32, rocket_hardpoints: u32) -> ConstructionRules {
    ConstructionRules::try_new(
        airframe(),
        known(gun_positions),
        known(rocket_hardpoints),
        known(WeightUnits::new(
            cs_content::construction::SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS,
        )),
        known(MoneyMinor::new(
            cs_content::construction::SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR,
        )),
        Origin::SyntheticFixture,
        Provenance::designed(claim("f55bu2.test.rules")),
    )
    .expect("the test rule profile is valid")
}

/// The pieces every test blueprint is assembled from.
fn assemble(
    blueprint_key: &str,
    armor: Vec<ArmorFitment>,
    guns: Vec<GunFitment>,
    ordnance: Vec<OrdnanceFitment>,
) -> AircraftBlueprint {
    AircraftBlueprint::try_new(
        id(ContentKind::Blueprint, blueprint_key),
        airframe(),
        engine(),
        armor,
        guns,
        ordnance,
        vec![id(ContentKind::HardpointEquipment, SYNTHETIC_RADIO_KEY)],
        synthetic_paint_selection(),
        Origin::SyntheticFixture,
        Provenance::designed(claim("f55bu2.test.blueprint")),
    )
    .expect("the test blueprint is valid")
}

/// The fixture boundary blueprint with both armor fitments swapped to `plate`.
fn plate_blueprint(plate_key: &str) -> AircraftBlueprint {
    let plate = id(ContentKind::Armor, plate_key);
    let armor = vec![
        ArmorFitment::try_new(node("armor_zone_fuselage"), plate.clone()).expect("armor fitment"),
        ArmorFitment::try_new(node("armor_zone_wing"), plate).expect("armor fitment"),
    ];
    assemble(
        "fixture.f55bu2_plate_blueprint",
        armor,
        synthetic_gun_fitments(),
        synthetic_ordnance_fitments(),
    )
}

/// The fixture boundary blueprint with every hardpoint's ordnance set to
/// `missile_key`.
fn missile_blueprint(missile_key: &str) -> AircraftBlueprint {
    let missile = id(ContentKind::Weapon, missile_key);
    let ordnance = (1..=4)
        .map(|index| {
            OrdnanceFitment::try_new(node(&format!("hardpoint_{index}")), missile.clone())
                .expect("ordnance fitment")
        })
        .collect();
    assemble(
        "fixture.f55bu2_missile_blueprint",
        synthetic_armor_fitments(),
        synthetic_gun_fitments(),
        ordnance,
    )
}

/// The fixture boundary blueprint with each of its four gun fitments occupying
/// `positions[i]` gun positions.
fn gun_position_blueprint(positions: [u32; 4]) -> AircraftBlueprint {
    let guns = synthetic_gun_fitments()
        .into_iter()
        .zip(positions)
        .map(|(gun, positions)| {
            gun.with_positions(known(positions))
                .expect("a nonzero gun position count")
        })
        .collect();
    assemble(
        "fixture.f55bu2_gun_blueprint",
        synthetic_armor_fitments(),
        guns,
        synthetic_ordnance_fitments(),
    )
}

/// A catalog over the fixture's authored blueprint and price book, with the
/// declared pairing rule and the given rule profiles.
fn catalog(
    blueprints: Vec<AircraftBlueprint>,
    rules: Vec<ConstructionRules>,
    policy: ConstructionPolicy,
) -> LobbyConstructionCatalog {
    LobbyConstructionCatalog::new(blueprints, rules, policy, declared_synthetic_price_book())
}

/// The distinct component ids a lobby loadout would carry for `blueprint`.
///
/// A lobby loadout is a set (its own shape check refuses a repeated component),
/// so an authored blueprint that fits the same component twice still yields one
/// entry here — exactly the list a client would send.
fn components(blueprint: &AircraftBlueprint) -> Vec<ContentId> {
    let distinct: BTreeSet<ContentId> = blueprint
        .components()
        .into_iter()
        .map(|(_, component)| component.clone())
        .collect();
    distinct.into_iter().collect()
}

fn fixture_catalog(rules: ConstructionRules) -> (LobbyConstructionCatalog, AircraftBlueprint) {
    let blueprint = declared_synthetic_blueprint();
    (
        catalog(vec![blueprint.clone()], vec![rules], synthetic_policy()),
        blueprint,
    )
}

/// The minimum scenario: a loadout whose blueprint is authored and inside every
/// shared limit is accepted, and this is the one test whose expectation is that
/// nothing is refused.
#[test]
fn accept_f55_bu2_a_boundary_loadout_is_accepted() {
    let (catalog, blueprint) = fixture_catalog(synthetic_boundary_rules());
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Ok(()),
        "the boundary blueprint sits exactly on every limit and is inside them"
    );
}

/// A loadout that names a blueprint the catalog does not hold is refused rather
/// than passed unjudged.
#[test]
fn accept_f55_bu2_an_unknown_blueprint_is_refused() {
    let (catalog, _blueprint) = fixture_catalog(synthetic_boundary_rules());
    let foreign = id(ContentKind::Blueprint, "fixture.f55bu2_not_authored");
    assert_eq!(catalog.judge(&foreign, &[]), Err(UNKNOWN_BLUEPRINT));
}

/// An authored blueprint whose airframe has no rule profile is refused with its
/// own code; a partial catalog is not a permissive one.
#[test]
fn accept_f55_bu2_a_blueprint_with_no_rule_profile_is_refused() {
    let blueprint = declared_synthetic_blueprint();
    let catalog = catalog(vec![blueprint.clone()], vec![], synthetic_policy());
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(UNKNOWN_RULES)
    );
}

/// The loadout's component list must be the authored blueprint's own
/// components: a swapped, added or dropped component is refused before the
/// shared rules are consulted, and a reordered list is the same multiset and is
/// accepted.
#[test]
fn accept_f55_bu2_a_loadout_that_does_not_match_its_blueprint_is_refused() {
    let (catalog, blueprint) = fixture_catalog(synthetic_boundary_rules());
    let fitted = components(&blueprint);

    // A heavier plate smuggled in place of the authored plate: the shared rules
    // would call this over mass, but the loadout is not the authored
    // blueprint's at all, so it is a mismatch.
    let heavy = id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY);
    let mut swapped = fitted.clone();
    let at = swapped
        .iter()
        .position(|component| component.kind() == ContentKind::Armor)
        .expect("the boundary blueprint carries armor");
    swapped[at] = heavy;
    assert_eq!(
        catalog.judge(blueprint.id(), &swapped),
        Err(LOADOUT_MISMATCH)
    );

    // One component fewer, and one component the blueprint does not carry
    // added.
    assert_eq!(
        catalog.judge(blueprint.id(), &fitted[..fitted.len() - 1]),
        Err(LOADOUT_MISMATCH)
    );
    let mut added = fitted.clone();
    added.push(id(ContentKind::Gun, "fixture.f55bu2_other_gun"));
    assert_eq!(catalog.judge(blueprint.id(), &added), Err(LOADOUT_MISMATCH));

    // The same multiset in another order is the same loadout.
    let mut reordered = fitted.clone();
    reordered.reverse();
    assert_eq!(catalog.judge(blueprint.id(), &reordered), Ok(()));
}

/// One weight unit over the ceiling is refused as a mass breach, exactly as the
/// shared validator refuses it.
#[test]
fn accept_f55_bu2_a_blueprint_over_its_mass_ceiling_is_refused() {
    let blueprint = plate_blueprint(SYNTHETIC_HEAVY_PLATE_KEY);
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![synthetic_boundary_rules()],
        synthetic_policy(),
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(OVER_MASS)
    );
}

/// One minor unit over the price ceiling is refused as a cost breach.
#[test]
fn accept_f55_bu2_a_blueprint_over_its_cost_ceiling_is_refused() {
    let blueprint = missile_blueprint(SYNTHETIC_HEAVY_MISSILE_KEY);
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![synthetic_boundary_rules()],
        synthetic_policy(),
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(OVER_COST)
    );
}

/// More ordnance than the rack has hardpoints is refused as a hardpoint breach.
#[test]
fn accept_f55_bu2_a_blueprint_over_its_hardpoints_is_refused() {
    let (catalog, blueprint) = fixture_catalog(rack_rules(8, 2));
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(OVER_HARDPOINTS)
    );
}

/// More gun positions than the rack has is refused as a gun-position breach.
#[test]
fn accept_f55_bu2_a_blueprint_over_its_gun_positions_is_refused() {
    let blueprint = gun_position_blueprint([2, 1, 1, 1]);
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![rack_rules(4, 8)],
        synthetic_policy(),
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(OVER_GUN_POSITIONS)
    );
}

/// A component the host bans is a constraint violation, with its own code
/// distinct from the lobby's own ban check.
#[test]
fn accept_f55_bu2_a_banned_component_is_refused() {
    let blueprint = declared_synthetic_blueprint();
    let declared = synthetic_policy();
    let policy = ConstructionPolicy::new(
        declared.pairable_guns().clone(),
        BTreeSet::from([engine()]),
        declared.available().clone(),
    );
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![synthetic_boundary_rules()],
        policy,
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(BANNED_COMPONENT)
    );
}

/// A component the caller may not use is a constraint violation.
#[test]
fn accept_f55_bu2_an_unavailable_component_is_refused() {
    let blueprint = declared_synthetic_blueprint();
    let declared = synthetic_policy();
    let policy = ConstructionPolicy::new(
        declared.pairable_guns().clone(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![synthetic_boundary_rules()],
        policy,
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(UNAVAILABLE_COMPONENT)
    );
}

/// A gun selection that is neither single nor pair is a constraint violation.
#[test]
fn accept_f55_bu2_an_unsupported_gun_selection_is_refused() {
    let blueprint = gun_position_blueprint([3, 1, 1, 1]);
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![rack_rules(6, 8)],
        synthetic_policy(),
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(UNSUPPORTED_GUN_SELECTION)
    );
}

/// A pair the pairing rule does not let be mated is a constraint violation.
#[test]
fn accept_f55_bu2_a_non_pairable_pair_is_refused() {
    let blueprint = gun_position_blueprint([2, 1, 1, 1]);
    let declared = synthetic_policy();
    let policy = ConstructionPolicy::new(
        known(BTreeSet::new()),
        declared.banned().clone(),
        declared.available().clone(),
    );
    let catalog = catalog(vec![blueprint.clone()], vec![rack_rules(6, 8)], policy);
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(GUN_NOT_PAIRABLE)
    );
}

/// A pair against an unmeasured pairing rule is refused, not allowed or
/// forbidden by guess.
#[test]
fn accept_f55_bu2_an_unmeasured_pairing_rule_is_refused() {
    let blueprint = gun_position_blueprint([2, 1, 1, 1]);
    let declared = synthetic_policy();
    let policy = ConstructionPolicy::new(
        ResolvedValue::unknown(
            claim("f55bu2.test.unmeasured-pairing"),
            "no original pairing rule was measured",
        )
        .expect("a reason is present"),
        declared.banned().clone(),
        declared.available().clone(),
    );
    let catalog = catalog(vec![blueprint.clone()], vec![rack_rules(6, 8)], policy);
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(UNKNOWN_PAIRING_RULE)
    );
}

/// An unmeasured limit refuses the comparison instead of reading as "no limit".
#[test]
fn accept_f55_bu2_an_unmeasured_limit_is_refused() {
    let (catalog, blueprint) = fixture_catalog(synthetic_unmeasured_limit_rules());
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(UNMEASURED_BUDGET)
    );
}

/// A component the price book does not price refuses the budget instead of
/// defaulting to free.
#[test]
fn accept_f55_bu2_an_unpriced_component_is_refused() {
    let blueprint = plate_blueprint(SYNTHETIC_UNPRICED_PLATE_KEY);
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![synthetic_boundary_rules()],
        synthetic_policy(),
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Err(UNMEASURED_BUDGET)
    );
}

/// The fixture missile ordnance is the same weapon namespace the boundary
/// blueprint uses, so the fixture is coherent; this guards the helper that the
/// other tests build their variants from.
#[test]
fn accept_f55_bu2_the_fixture_missile_blueprint_matches_its_declared_components() {
    let blueprint = missile_blueprint(SYNTHETIC_MISSILE_KEY);
    let catalog = catalog(
        vec![blueprint.clone()],
        vec![synthetic_boundary_rules()],
        synthetic_policy(),
    );
    assert_eq!(
        catalog.judge(blueprint.id(), &components(&blueprint)),
        Ok(()),
        "the fixture missile is the boundary blueprint's own ordnance"
    );
    assert_eq!(
        components(&blueprint).len(),
        components(&declared_synthetic_blueprint()).len()
    );
    assert_ne!(
        blueprint.id(),
        &id(ContentKind::Blueprint, SYNTHETIC_BLUEPRINT_KEY)
    );
}
