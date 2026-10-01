//! F44-A acceptance tests: blueprint constraints and exact budget arithmetic.
//!
//! Spec `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-A`. The stage's minimum scenario is sheet **AC01**: a
//! boundary loadout exactly at the weight/cost limit is accepted, and one unit
//! over is rejected.
//!
//! Every test here calls the production `cs_content::construction` API — the
//! same entry point F44-B's validator and F44-C's editor will call — so a
//! mutation of the arithmetic or the limit comparison fails these tests rather
//! than passing beside them.

use cs_content::construction::{
    AircraftBlueprint, ArmorFitment, BlueprintAssessment, BudgetBreakdown, BudgetCategory,
    BudgetQuantity, BudgetRefusal, ComponentQuote, ConstructionRules, ConstructionSchemaError,
    ConstructionSlot, DisplayMapping, GunFitment, LimitBreach, MoneyMinor, OrdnanceFitment,
    PaintSelection, PriceBook, SYNTHETIC_ABSENT_PLATE_KEY, SYNTHETIC_AIRFRAME_KEY,
    SYNTHETIC_BLUEPRINT_KEY, SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR, SYNTHETIC_BOUNDARY_COST_MINOR,
    SYNTHETIC_BOUNDARY_GUN_POSITIONS, SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS,
    SYNTHETIC_BOUNDARY_MASS_UNITS, SYNTHETIC_BOUNDARY_ROCKET_HARDPOINTS, SYNTHETIC_DECAL_KEY,
    SYNTHETIC_ENGINE_KEY, SYNTHETIC_GUN_KEY, SYNTHETIC_HEAVY_MISSILE_KEY,
    SYNTHETIC_HEAVY_PLATE_KEY, SYNTHETIC_MISSILE_KEY, SYNTHETIC_OTHER_AIRFRAME_KEY,
    SYNTHETIC_OVERFLOW_ENGINE_KEY, SYNTHETIC_PAINT_MASK_KEY, SYNTHETIC_PLATE_KEY,
    SYNTHETIC_UNMEASURED_ENGINE_KEY, SYNTHETIC_UNPRICED_PLATE_KEY, WeightUnits,
    declared_synthetic_blueprint, declared_synthetic_price_book, synthetic_blueprint_with_engine,
    synthetic_boundary_rules, synthetic_gun_fitments, synthetic_ordnance_fitments,
    synthetic_paint_selection, synthetic_unmeasured_limit_rules, synthetic_wide_rack_rules,
};
use cs_content::damage::DamageNodeKey;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the fixture id is valid")
}

fn designed(value: impl Into<String>) -> Provenance {
    Provenance::designed(ClaimId::new(&value.into()).expect("the fixture claim id is valid"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed("f44a.test.value")))
}

fn unknown<T>() -> Resolved<T> {
    Resolved::unknown(
        ClaimId::new("f44a.test.unknown").expect("the fixture claim id is valid"),
        "no original value was measured by this stage",
    )
    .expect("a reason is present")
}

/// The boundary blueprint measured against the boundary profile: the exact
/// AC01 "at the limit" case.
fn assess_boundary() -> BlueprintAssessment {
    let rules = synthetic_boundary_rules();
    let book = declared_synthetic_price_book();
    rules
        .assess(&declared_synthetic_blueprint(), &book)
        .expect("the boundary blueprint is measurable")
}

/// AC01: a loadout exactly at the weight **and** cost limit is accepted, and
/// the reported totals are the exact integer sums.
///
/// The fixture is built so that all four limits are hit at once: 6040 weight
/// units against a 6040 limit and 42000 minor units against a 42000 limit,
/// with four of four gun positions and four of eight rocket hardpoints used.
/// Being exactly at a limit is inside it, so the assessment is clean.
#[test]
fn accept_f44_a_boundary_loadout_exactly_at_the_limits_is_accepted() {
    let assessment = assess_boundary();

    assert!(
        assessment.is_within_limits(),
        "a loadout exactly at the limit is inside it, not over it"
    );
    assert_eq!(assessment.breaches(), &[] as &[LimitBreach]);
    assert_eq!(assessment.first_breach(), None);

    let totals = assessment.totals();
    assert_eq!(totals.mass().as_units(), SYNTHETIC_BOUNDARY_MASS_UNITS);
    assert_eq!(
        totals.mass().as_units(),
        SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS
    );
    assert_eq!(totals.cost().as_minor(), SYNTHETIC_BOUNDARY_COST_MINOR);
    assert_eq!(
        totals.cost().as_minor(),
        SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR
    );
    assert_eq!(totals.gun_positions(), SYNTHETIC_BOUNDARY_GUN_POSITIONS);
    assert_eq!(totals.gun_positions(), 4);
    assert_eq!(totals.rocket_hardpoints(), 4);
    assert!(
        totals.rocket_hardpoints() < SYNTHETIC_BOUNDARY_ROCKET_HARDPOINTS,
        "the boundary blueprint leaves rocket hardpoints spare but uses every gun position"
    );

    // The totals are the exact fold of the per-category subtotals: a caller can
    // add the breakdown's columns and reach the same integers.
    let breakdown = assessment.breakdown();
    let summed_mass: u64 = breakdown
        .lines()
        .iter()
        .map(|(_, line)| line.mass().as_units())
        .sum();
    let summed_cost: u64 = breakdown
        .lines()
        .iter()
        .map(|(_, line)| line.cost().as_minor())
        .sum();
    assert_eq!(summed_mass, totals.mass().as_units());
    assert_eq!(summed_cost, totals.cost().as_minor());
    assert_eq!(
        breakdown.line(BudgetCategory::Airframe).mass().as_units(),
        4_000
    );
    assert_eq!(
        breakdown.line(BudgetCategory::Guns).mass().as_units(),
        4 * 160
    );
    assert_eq!(
        breakdown.line(BudgetCategory::Ordnance).cost().as_minor(),
        4 * 4_250
    );
    assert_eq!(
        breakdown.line(BudgetCategory::Equipment).cost().as_minor(),
        2_000
    );
}

/// AC01's weight half: swapping in a plate exactly one weight unit heavier
/// pushes the total to 6041 against a 6040 limit and is rejected, while every
/// other quantity stays inside — so the verdict is specifically about weight.
#[test]
fn accept_f44_a_one_weight_unit_over_the_limit_is_rejected() {
    let rules = synthetic_boundary_rules();
    let book = declared_synthetic_price_book();
    let blueprint = declared_synthetic_blueprint()
        .with_armor(vec![
            ArmorFitment::try_new(
                DamageNodeKey::new("armor_zone_fuselage").expect("the fixture node key is valid"),
                id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
            )
            .expect("the armor fitment is valid"),
            ArmorFitment::try_new(
                DamageNodeKey::new("armor_zone_wing").expect("the fixture node key is valid"),
                id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
            )
            .expect("the armor fitment is valid"),
        ])
        .expect("the edited blueprint is valid");

    let assessment = rules
        .assess(&blueprint, &book)
        .expect("the edited blueprint is still measurable");

    assert!(
        !assessment.is_within_limits(),
        "one weight unit over the limit must be rejected"
    );
    assert_eq!(assessment.totals().mass().as_units(), 6_041);
    assert_eq!(
        assessment.breaches(),
        &[LimitBreach::Mass {
            limit: WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS),
            total: WeightUnits::new(6_041),
        }]
    );

    // Exactly one unit over, and the breach says so.
    let breach = assessment
        .first_breach()
        .expect("the weight breach is reported");
    assert_eq!(breach.quantity(), BudgetQuantity::Mass);
    assert_eq!(assessment.totals().excess(*breach), 1);

    // Nothing else moved: the cost is untouched, so a naive "any breach" check
    // would have passed while a per-quantity check must not misattribute it.
    assert_eq!(
        assessment.totals().cost().as_minor(),
        SYNTHETIC_BOUNDARY_COST_MINOR
    );
    assert!(
        !assessment
            .breaches()
            .iter()
            .any(|breach| breach.quantity() == BudgetQuantity::Cost),
        "the cost breach belongs to the price variant, not the weight one"
    );
}

/// AC01's cost half: a missile exactly one minor unit dearer makes the total
/// 42001 against a 42000 limit and is rejected, with the weight still exact.
#[test]
fn accept_f44_a_one_cost_unit_over_the_limit_is_rejected() {
    let rules = synthetic_boundary_rules();
    let book = declared_synthetic_price_book();
    let mut fitments = synthetic_ordnance_fitments();
    fitments[0] = OrdnanceFitment::try_new(
        DamageNodeKey::new("hardpoint_1").expect("the fixture node key is valid"),
        id(ContentKind::Weapon, SYNTHETIC_HEAVY_MISSILE_KEY),
    )
    .expect("the ordnance fitment is valid");
    let blueprint = declared_synthetic_blueprint()
        .with_ordnance(fitments)
        .expect("the edited blueprint is valid");

    let assessment = rules
        .assess(&blueprint, &book)
        .expect("the edited blueprint is still measurable");

    assert!(
        !assessment.is_within_limits(),
        "one minor unit over the price limit must be rejected"
    );
    assert_eq!(assessment.totals().cost().as_minor(), 42_001);
    assert_eq!(
        assessment.breaches(),
        &[LimitBreach::Cost {
            limit: MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR),
            total: MoneyMinor::new(42_001),
        }]
    );
    let breach = assessment
        .first_breach()
        .expect("the cost breach is reported");
    assert_eq!(breach.quantity(), BudgetQuantity::Cost);
    assert_eq!(assessment.totals().excess(*breach), 1);

    // The weight is still exactly at its limit, proving the two limits are
    // independent and neither is derived from the other.
    assert_eq!(
        assessment.totals().mass().as_units(),
        SYNTHETIC_BOUNDARY_MASS_UNITS
    );
    assert!(!assessment.is_within_limits());
}

/// Non-negotiable 2: rounding happens at a *specified* boundary and float UI
/// formatting cannot change purchase eligibility.
///
/// With a display mapping coarse enough to print 42000 and 42001 minor units as
/// the same text, the two blueprints are indistinguishable on screen — yet only
/// one of them is purchasable. The verdict reads the integers, never the
/// string, so the round-trip through the display layer is lossless with respect
/// to eligibility.
#[test]
fn accept_f44_a_display_rounding_cannot_change_purchase_eligibility() {
    // One minor unit per displayed major unit: no fractional part at all.
    let coarse = DisplayMapping::try_new(1).expect("one minor unit per major unit is valid");
    assert_eq!(coarse.minor_per_major(), 1);
    assert_eq!(coarse.decimal_width(), 0);
    assert_eq!(MoneyMinor::new(42_000).format_display(coarse), "42000");

    // A thousand minor units per major unit keeps three decimal digits, and the
    // formatting is integer division, never a float: two totals one unit apart
    // remain two different strings.
    let thousand = DisplayMapping::try_new(1_000).expect("a nonzero divisor is valid");
    assert_eq!(thousand.decimal_width(), 3);
    assert_eq!(MoneyMinor::new(42_000).format_display(thousand), "42.000");
    assert_eq!(MoneyMinor::new(42_001).format_display(thousand), "42.001");

    // Ten thousand per major unit needs four digits.
    let ten_thousand = DisplayMapping::try_new(10_000).expect("a nonzero divisor is valid");
    assert_eq!(ten_thousand.decimal_width(), 4);
    assert_eq!(
        MoneyMinor::new(420_000).format_display(ten_thousand),
        "42.0000"
    );
    assert_eq!(
        MoneyMinor::new(420_001).format_display(ten_thousand),
        "42.0001"
    );

    // Formatting at the divisor's full precision is **lossless**: integer
    // division plus a zero-padded remainder always recovers the exact total, so
    // the declared rounding boundary loses no information at all. This is
    // stronger than "rounding is confined to display" — there is no lossy
    // display step for an eligibility check to be confused by in the first
    // place.
    let exact = DisplayMapping::try_new(1_000_000).expect("a nonzero divisor is valid");
    for minor in [0, 1, 999, 999_999, 1_000_000, 42_000, 42_001] {
        let text = MoneyMinor::new(minor).format_display(exact);
        let (major_text, minor_text) = text
            .split_once('.')
            .expect("a mapping with fractional digits separates major and minor");
        let major: u64 = major_text.parse().expect("the major part is an integer");
        let fraction: u64 = minor_text.parse().expect("the minor part is an integer");
        assert_eq!(
            major * 1_000_000 + fraction,
            minor,
            "{text} round-trips back to the exact total {minor}"
        );
    }

    // Both blueprints are measured by the same profile and the same book, and
    // the display text is not an input: the exact integers decide.
    let rules = synthetic_boundary_rules();
    let book = declared_synthetic_price_book();
    let over = declared_synthetic_blueprint()
        .with_ordnance({
            let mut fitments = synthetic_ordnance_fitments();
            fitments[0] = OrdnanceFitment::try_new(
                DamageNodeKey::new("hardpoint_1").expect("the fixture node key is valid"),
                id(ContentKind::Weapon, SYNTHETIC_HEAVY_MISSILE_KEY),
            )
            .expect("the ordnance fitment is valid");
            fitments
        })
        .expect("the edited blueprint is valid");

    let inside = rules
        .assess(&declared_synthetic_blueprint(), &book)
        .expect("the boundary blueprint is measurable");
    let outside = rules
        .assess(&over, &book)
        .expect("the edited blueprint is measurable");

    // No display mapping is consulted by the verdict, so the two totals are
    // decided by their exact integers under every mapping. Formatting each of
    // them and feeding the text back through the mapping's own arithmetic still
    // recovers a different number, which is what makes the verdict
    // display-independent rather than merely display-consistent.
    let at_limit = inside.totals().cost();
    let over_limit = outside.totals().cost();
    assert_eq!(
        at_limit.format_display(exact),
        MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_MINOR).format_display(exact)
    );
    assert_eq!(
        over_limit.format_display(exact),
        MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_MINOR + 1).format_display(exact)
    );
    assert_ne!(
        at_limit.format_display(exact),
        over_limit.format_display(exact),
        "the one-unit difference survives the display round trip"
    );
    assert_ne!(at_limit, over_limit);
    assert!(inside.is_within_limits());
    assert!(!outside.is_within_limits());

    // A caller that ignores the verdict and re-reads the text still cannot turn
    // the over-limit loadout into a purchasable one: the exact comparison is
    // what decides, and it is stated in integers.
    assert_eq!(
        MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_LIMIT_MINOR)
            .format_display(exact)
            .parse::<String>()
            .map(|text| text == over_limit.format_display(exact)),
        Ok(false),
        "the over-limit total never renders as the limit"
    );

    // A zero divisor is refused rather than producing an undefined fraction.
    assert_eq!(
        DisplayMapping::try_new(0).err(),
        Some(ConstructionSchemaError::ZeroMinorPerMajor)
    );

    // A divisor that is not a power of ten is refused too: its remainder has no
    // exact fixed-width decimal form, so rendering it would print a
    // plausible-looking *wrong* amount — with 2500 minor units per major unit
    // the remainder 2000 is 0.8 major units, not 0.2000. Refusing it also keeps
    // the digit-count arithmetic bounded, so no divisor can overflow it.
    for refused in [2, 25, 250, 2_500, 1_000_001, u32::MAX] {
        assert_eq!(
            DisplayMapping::try_new(refused).err(),
            Some(ConstructionSchemaError::MinorPerMajorNotPowerOfTen {
                minor_per_major: refused
            }),
            "{refused} minor units per major unit is refused rather than misrendered"
        );
    }

    // Every divisor the constructor accepts renders exactly: the fraction field
    // is as wide as that power of ten needs, and the printed text always
    // recovers the exact total.
    let mut scale = 1_u32;
    while let Some(next) = scale.checked_mul(10) {
        let mapping = DisplayMapping::try_new(scale).expect("a power of ten is valid");
        let minor = 42_001;
        let text = MoneyMinor::new(minor).format_display(mapping);
        match text.split_once('.') {
            None => {
                assert_eq!(scale, 1, "only a divisor of one prints no fraction");
                assert_eq!(text, minor.to_string());
            }
            Some((major_text, minor_text)) => {
                assert_eq!(
                    minor_text.len(),
                    mapping.decimal_width(),
                    "the fraction field is exactly as wide as the divisor needs"
                );
                let major: u64 = major_text.parse().expect("the major part is an integer");
                let fraction: u64 = minor_text.parse().expect("the minor part is an integer");
                assert_eq!(
                    major * u64::from(scale) + fraction,
                    minor,
                    "{text} round-trips to the exact total {minor} at a divisor of {scale}"
                );
            }
        }
        scale = next;
    }
    assert_eq!(
        scale, 1_000_000_000,
        "every power of ten that fits was exercised"
    );
}

/// Non-negotiable 1: the manual's four gun positions and eight rocket
/// hardpoints are *profile data*, not compiled-in constants.
///
/// The same blueprint is measured against two profiles of the same airframe. A
/// four-gun loadout fills the boundary profile's four positions exactly, and
/// exceeds the wide profile's limit of two rocket hardpoints; a six-position
/// rack accepts a six-position loadout that the four-position profile rejects.
/// If the limits were constants, at least one of these verdicts would be wrong.
#[test]
fn accept_f44_a_manual_rack_limits_are_profile_data_not_constants() {
    let book = declared_synthetic_price_book();
    let boundary = synthetic_boundary_rules();
    let wide = synthetic_wide_rack_rules();

    // The manual's observed numbers are the boundary profile's declared values,
    // not a module constant: the wide profile declares different ones.
    assert_eq!(
        boundary.gun_positions().clone().known(),
        Some(SYNTHETIC_BOUNDARY_GUN_POSITIONS)
    );
    assert_eq!(
        boundary.rocket_hardpoints().clone().known(),
        Some(SYNTHETIC_BOUNDARY_ROCKET_HARDPOINTS)
    );
    assert_eq!(wide.gun_positions().clone().known(), Some(6));
    assert_eq!(wide.rocket_hardpoints().clone().known(), Some(2));

    // Four guns against four positions is exactly full for the boundary profile.
    let four_guns = boundary
        .assess(&declared_synthetic_blueprint(), &book)
        .expect("the four-gun loadout is measurable");
    assert_eq!(four_guns.totals().gun_positions(), 4);
    assert!(four_guns.is_within_limits());

    // The same loadout exceeds the wide profile's two rocket hardpoints.
    let same_loadout = wide
        .assess(&declared_synthetic_blueprint(), &book)
        .expect("the same loadout is measurable against the wide profile");
    assert!(
        !same_loadout.is_within_limits(),
        "four rockets exceed the wide profile's two hardpoints"
    );
    assert!(
        same_loadout
            .breaches()
            .contains(&LimitBreach::RocketHardpoints { limit: 2, used: 4 })
    );
    assert_eq!(same_loadout.totals().gun_positions(), 4);
    assert!(
        same_loadout.totals().gun_positions() <= 6,
        "four positions still fit the wider rack"
    );

    // A paired selection occupies two gun positions: four guns where two are
    // mated pairs consume six, which the boundary profile rejects and the wide
    // profile accepts exactly.
    let paired_guns: Vec<GunFitment> = synthetic_gun_fitments()
        .into_iter()
        .enumerate()
        .map(|(index, fitment)| {
            let positions = if index < 2 { 2 } else { 1 };
            fitment
                .with_positions(known(positions))
                .expect("a nonzero position count is valid")
        })
        .collect();
    assert_eq!(2 + 2 + 1 + 1, 6);
    let paired = declared_synthetic_blueprint()
        .with_guns(paired_guns)
        .expect("the paired blueprint is valid");

    let paired_on_boundary = boundary
        .assess(&paired, &book)
        .expect("the paired loadout is measurable");
    assert_eq!(paired_on_boundary.totals().gun_positions(), 6);
    assert!(
        !paired_on_boundary.is_within_limits(),
        "six positions exceed the boundary profile's four"
    );
    assert!(
        paired_on_boundary
            .breaches()
            .contains(&LimitBreach::GunPositions {
                limit: SYNTHETIC_BOUNDARY_GUN_POSITIONS,
                used: 6
            })
    );

    let paired_on_wide = wide
        .assess(&paired, &book)
        .expect("the paired loadout is measurable against the wide profile");
    assert_eq!(paired_on_wide.totals().gun_positions(), 6);
    assert!(
        !paired_on_wide
            .breaches()
            .iter()
            .any(|breach| breach.quantity() == BudgetQuantity::GunPositions),
        "six positions fit the wider rack exactly"
    );
}

/// Every limit comparison is inclusive at the boundary, the rocket-hardpoint one
/// included, and the breach list is reported in the canonical quantity order.
///
/// The rack assertions elsewhere only ever compare a loadout that is clearly
/// over or clearly under, so a hardpoint comparison mutated from `>` to `>=`
/// would have failed nothing. Two rockets on the wide profile's two hardpoints
/// is exactly at the limit and must be inside it; three is one over and must be
/// the *only* breach, with weight and price still inside their ceilings.
#[test]
fn accept_f44_a_every_limit_boundary_is_inclusive_and_breaches_are_ordered() {
    let book = declared_synthetic_price_book();
    let wide = synthetic_wide_rack_rules();

    // The boundary blueprint with `count` missiles instead of four.
    let with_rockets = |count: u32| -> AircraftBlueprint {
        let fitments = (0..count)
            .map(|index| {
                OrdnanceFitment::try_new(
                    DamageNodeKey::new(&format!("hardpoint_{index}"))
                        .expect("the fixture node key is valid"),
                    id(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY),
                )
                .expect("the ordnance fitment is valid")
            })
            .collect();
        declared_synthetic_blueprint()
            .with_ordnance(fitments)
            .expect("the edited blueprint is valid")
    };

    // Exactly the profile's two hardpoints is at the limit, so it is inside it,
    // and the lighter loadout is inside the weight and price ceilings too.
    let at_limit = wide
        .assess(&with_rockets(2), &book)
        .expect("the two-rocket loadout is measurable");
    assert_eq!(at_limit.totals().rocket_hardpoints(), 2);
    assert_eq!(at_limit.breaches(), &[] as &[LimitBreach]);
    assert!(
        at_limit.is_within_limits(),
        "two of two rocket hardpoints is at the limit, not over it"
    );

    // One more rocket is a breach of exactly that quantity, and nothing else
    // moved: the weight and price totals are still inside their ceilings.
    let over = wide
        .assess(&with_rockets(3), &book)
        .expect("the three-rocket loadout is measurable");
    assert_eq!(
        over.breaches(),
        &[LimitBreach::RocketHardpoints { limit: 2, used: 3 }]
    );
    assert!(
        !over.is_within_limits(),
        "three of two rocket hardpoints is over the limit"
    );
    let breach = over
        .first_breach()
        .expect("the hardpoint breach is reported");
    assert_eq!(breach.quantity(), BudgetQuantity::RocketHardpoints);
    assert_eq!(over.totals().excess(*breach), 1);

    // A profile every quantity is over reports all four breaches in the canonical
    // quantity order, so a caller may read the first one as *the* reason.
    let tiny = ConstructionRules::try_new(
        id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(0),
        known(0),
        known(WeightUnits::new(1)),
        known(MoneyMinor::new(1)),
        Origin::SyntheticFixture,
        designed("f44a.test.tiny-limits"),
    )
    .expect("a profile with tiny limits is structurally valid");
    let every = tiny
        .assess(&declared_synthetic_blueprint(), &book)
        .expect("a measurable loadout is measurable against tiny limits too");
    assert_eq!(
        every.breaches(),
        &[
            LimitBreach::Mass {
                limit: WeightUnits::new(1),
                total: WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_UNITS),
            },
            LimitBreach::Cost {
                limit: MoneyMinor::new(1),
                total: MoneyMinor::new(SYNTHETIC_BOUNDARY_COST_MINOR),
            },
            LimitBreach::GunPositions { limit: 0, used: 4 },
            LimitBreach::RocketHardpoints { limit: 0, used: 4 },
        ]
    );
    assert_eq!(
        every.first_breach().map(LimitBreach::quantity),
        Some(BudgetQuantity::Mass),
        "the canonical order starts with mass"
    );
    assert_eq!(
        every
            .breaches()
            .iter()
            .map(LimitBreach::quantity)
            .collect::<Vec<_>>(),
        BudgetQuantity::ALL.to_vec(),
        "breaches are reported in the canonical quantity order"
    );
}

/// The rack is part of the loadout, not of the airframe: the profile's limit is
/// compared, and an unmeasured limit refuses the comparison instead of reading
/// as "no limit".
#[test]
fn accept_f44_a_an_unmeasured_limit_refuses_instead_of_reading_as_no_limit() {
    let book = declared_synthetic_price_book();
    let rules = synthetic_unmeasured_limit_rules();

    assert_eq!(
        rules.max_mass().clone().known(),
        None,
        "the fixture's weight ceiling is an explicit unknown"
    );

    let err = rules
        .assess(&declared_synthetic_blueprint(), &book)
        .expect_err("an unmeasured limit must refuse the comparison");
    assert_eq!(
        err,
        BudgetRefusal::UnknownLimit {
            quantity: BudgetQuantity::Mass
        }
    );

    // Compare with the boundary profile, which measures the same blueprint
    // fine: the refusal is the limit's, not the blueprint's.
    assert!(
        synthetic_boundary_rules()
            .assess(&declared_synthetic_blueprint(), &book)
            .is_ok()
    );
}

/// An unmeasured component mass, price, an unpriced component and an unmeasured
/// gun-position count are each refused by name, so nothing can pass as "fits".
#[test]
fn accept_f44_a_unknown_prices_and_footprints_are_refused_by_name() {
    let rules = synthetic_boundary_rules();
    let book = declared_synthetic_price_book();

    // An engine whose mass is unmeasured: no exact total exists.
    let unmeasured_engine =
        synthetic_blueprint_with_engine(id(ContentKind::Engine, SYNTHETIC_UNMEASURED_ENGINE_KEY))
            .expect("the edited blueprint is valid");
    assert_eq!(
        rules.assess(&unmeasured_engine, &book).err(),
        Some(BudgetRefusal::UnknownMass {
            component: id(ContentKind::Engine, SYNTHETIC_UNMEASURED_ENGINE_KEY),
        })
    );

    // A plate whose price is unmeasured.
    let unpriced_plate = declared_synthetic_blueprint()
        .with_armor(vec![
            ArmorFitment::try_new(
                DamageNodeKey::new("armor_zone_fuselage").expect("the fixture node key is valid"),
                id(ContentKind::Armor, SYNTHETIC_UNPRICED_PLATE_KEY),
            )
            .expect("the armor fitment is valid"),
            ArmorFitment::try_new(
                DamageNodeKey::new("armor_zone_wing").expect("the fixture node key is valid"),
                id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
            )
            .expect("the armor fitment is valid"),
        ])
        .expect("the edited blueprint is valid");
    assert_eq!(
        rules.assess(&unpriced_plate, &book).err(),
        Some(BudgetRefusal::UnknownCost {
            component: id(ContentKind::Armor, SYNTHETIC_UNPRICED_PLATE_KEY),
        })
    );

    // A component the book simply does not quote: not free, refused.
    let absent_plate = declared_synthetic_blueprint()
        .with_armor(vec![
            ArmorFitment::try_new(
                DamageNodeKey::new("armor_zone_fuselage").expect("the fixture node key is valid"),
                id(ContentKind::Armor, SYNTHETIC_ABSENT_PLATE_KEY),
            )
            .expect("the armor fitment is valid"),
            ArmorFitment::try_new(
                DamageNodeKey::new("armor_zone_wing").expect("the fixture node key is valid"),
                id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
            )
            .expect("the armor fitment is valid"),
        ])
        .expect("the edited blueprint is valid");
    assert_eq!(
        rules.assess(&absent_plate, &book).err(),
        Some(BudgetRefusal::NotPriced {
            component: id(ContentKind::Armor, SYNTHETIC_ABSENT_PLATE_KEY),
            category: BudgetCategory::Armor,
        })
    );

    // A gun whose position count is unmeasured: the rack usage is unknown.
    let unknown_rack = declared_synthetic_blueprint()
        .with_guns(vec![
            GunFitment::try_new(
                id(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
                DamageNodeKey::new("gun_mount_1").expect("the fixture node key is valid"),
                unknown::<u32>(),
            )
            .expect("the gun fitment is valid"),
        ])
        .expect("the edited blueprint is valid");
    assert_eq!(
        rules.assess(&unknown_rack, &book).err(),
        Some(BudgetRefusal::UnknownGunPositions {
            component: id(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
        })
    );

    // A blueprint on a different airframe is refused rather than measured
    // against the wrong limits.
    let other_airframe = declared_synthetic_blueprint()
        .with_airframe(id(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY))
        .expect("the edited blueprint is valid");
    assert_eq!(
        rules.assess(&other_airframe, &book).err(),
        Some(BudgetRefusal::AirframeMismatch {
            blueprint: id(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY),
            rules: id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        })
    );

    // An arithmetic overflow is refused rather than wrapping to a smaller total
    // that could pass a limit check.
    let enormous =
        synthetic_blueprint_with_engine(id(ContentKind::Engine, SYNTHETIC_OVERFLOW_ENGINE_KEY))
            .expect("the edited blueprint is valid");
    assert_eq!(
        rules.assess(&enormous, &book).err(),
        Some(BudgetRefusal::Overflow {
            quantity: BudgetQuantity::Mass,
            category: Some(BudgetCategory::Engine),
        })
    );
}

/// The blueprint is a validated typed input: kind-checked ids, no repeated
/// damage-graph node, and a paint selection that is references only.
///
/// Non-negotiable 5 requires that an export/import never include copyrighted
/// source textures implicitly, so the paint record carries catalog ids and no
/// pixel data at all.
#[test]
fn accept_f44_a_blueprint_is_a_validated_typed_input() {
    let blueprint = declared_synthetic_blueprint();

    assert_eq!(
        blueprint.id(),
        &id(ContentKind::Blueprint, SYNTHETIC_BLUEPRINT_KEY)
    );
    assert_eq!(
        blueprint.airframe(),
        &id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY)
    );
    assert_eq!(
        blueprint.engine(),
        &id(ContentKind::Engine, SYNTHETIC_ENGINE_KEY)
    );
    assert_eq!(blueprint.armor().len(), 2);
    assert_eq!(blueprint.guns().len(), 4);
    assert_eq!(blueprint.ordnance().len(), 4);
    assert_eq!(blueprint.equipment().len(), 1);
    assert_eq!(blueprint.origin(), &Origin::SyntheticFixture);
    assert!(!blueprint.origin().is_original());

    // Paint is references only: one mask and one decal, both catalog ids.
    let paint = blueprint.paint();
    assert_eq!(
        paint.masks(),
        &[id(ContentKind::PaintMask, SYNTHETIC_PAINT_MASK_KEY)]
    );
    assert_eq!(paint.decals().len(), 1);
    assert_eq!(
        paint.decals()[0].decal(),
        &id(ContentKind::PaintMask, SYNTHETIC_DECAL_KEY)
    );

    // A wrong-namespace id is refused.
    assert_eq!(
        AircraftBlueprint::try_new(
            id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
            id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
            id(ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
            vec![],
            vec![],
            vec![],
            vec![],
            synthetic_paint_selection(),
            Origin::SyntheticFixture,
            designed("f44a.test.kind"),
        )
        .err(),
        Some(ConstructionSchemaError::WrongKind {
            id: id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
            expected: ContentKind::Blueprint,
        })
    );

    // Two fitments claiming the same damage-graph node are refused, and the
    // error names which slot collided.
    let duplicate_armor = declared_synthetic_blueprint().with_armor(vec![
        ArmorFitment::try_new(
            DamageNodeKey::new("armor_zone_fuselage").expect("the fixture node key is valid"),
            id(ContentKind::Armor, SYNTHETIC_PLATE_KEY),
        )
        .expect("the armor fitment is valid"),
        ArmorFitment::try_new(
            DamageNodeKey::new("armor_zone_fuselage").expect("the fixture node key is valid"),
            id(ContentKind::Armor, SYNTHETIC_HEAVY_PLATE_KEY),
        )
        .expect("the armor fitment is valid"),
    ]);
    assert_eq!(
        duplicate_armor.err(),
        Some(ConstructionSchemaError::DuplicateSlot {
            slot: ConstructionSlot::ArmorZone
        })
    );

    // A gun occupying zero positions would free a slot, so it is refused.
    assert_eq!(
        GunFitment::try_new(
            id(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
            DamageNodeKey::new("gun_mount_1").expect("the fixture node key is valid"),
            known(0),
        )
        .err(),
        Some(ConstructionSchemaError::ZeroGunPositions)
    );
    assert_eq!(
        synthetic_gun_fitments()[0]
            .clone()
            .with_positions(known(0))
            .err(),
        Some(ConstructionSchemaError::ZeroGunPositions)
    );

    // A rule profile must name an airframe.
    assert!(matches!(
        ConstructionRules::try_new(
            id(ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
            known(4),
            known(8),
            known(WeightUnits::new(1)),
            known(MoneyMinor::new(1)),
            Origin::SyntheticFixture,
            designed("f44a.test.rules"),
        ),
        Err(ConstructionSchemaError::NotAnAirframe { .. })
    ));

    // A paint selection with a repeated mask is refused.
    let mask = id(ContentKind::PaintMask, SYNTHETIC_PAINT_MASK_KEY);
    assert_eq!(
        PaintSelection::try_new(vec![mask.clone(), mask.clone()], vec![]).err(),
        Some(ConstructionSchemaError::DuplicatePaint { component: mask })
    );

    // The price book refuses a duplicate quote rather than letting the last one
    // silently win.
    let component = id(ContentKind::Armor, SYNTHETIC_PLATE_KEY);
    assert_eq!(
        PriceBook::try_new(vec![
            (
                component.clone(),
                ComponentQuote::new(known(WeightUnits::new(1)), known(MoneyMinor::new(1))),
            ),
            (
                component.clone(),
                ComponentQuote::new(known(WeightUnits::new(2)), known(MoneyMinor::new(2))),
            ),
        ])
        .err(),
        Some(ConstructionSchemaError::DuplicateComponent { component })
    );

    // An empty book prices nothing, and that is a refusal at use time rather
    // than a loadout that is silently free.
    let empty = PriceBook::try_new(vec![]).expect("an empty book is structurally valid");
    assert!(empty.is_empty());
    assert_eq!(
        synthetic_boundary_rules()
            .assess(&declared_synthetic_blueprint(), &empty)
            .err(),
        Some(BudgetRefusal::NotPriced {
            component: id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
            category: BudgetCategory::Airframe,
        })
    );
}

/// A profile's limits are declared, so an unmeasured component in a *different*
/// category surfaces with that category's name; and the labelled vocabulary the
/// reports rely on is closed and consistent.
#[test]
fn accept_f44_a_budget_vocabulary_is_closed_and_consistent() {
    // The priced categories are exactly six, in a canonical order, and the
    // breakdown's line list matches the category list position for position.
    let breakdown = BudgetBreakdown::new();
    let lines = breakdown.lines();
    assert_eq!(lines.len(), BudgetCategory::ALL.len());
    for (index, (category, _)) in lines.iter().enumerate() {
        assert_eq!(
            *category,
            BudgetCategory::ALL[index],
            "the breakdown's fold order is the category list"
        );
    }
    for category in BudgetCategory::ALL {
        assert_eq!(BudgetCategory::from_label(category.label()), Some(category));
    }
    for quantity in BudgetQuantity::ALL {
        assert_eq!(BudgetQuantity::from_label(quantity.label()), Some(quantity));
    }

    // Each breach names exactly one quantity, and a total at the limit is not a
    // breach while a total one over is.
    let assessment = assess_boundary();
    assert_eq!(assessment.breaches().len(), 0);
    let breach = LimitBreach::Mass {
        limit: WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS),
        total: WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS + 1),
    };
    assert_eq!(breach.quantity(), BudgetQuantity::Mass);
    assert_eq!(assessment.totals().excess(breach), 1);
    assert_ne!(breach.to_string(), "");

    // `excess` reports no excess for a breach that is not actually over its
    // limit — a limit borrowed from another profile, say — so the difference
    // can never underflow the exact integers it is computed from.
    assert_eq!(
        assessment.totals().excess(LimitBreach::Mass {
            limit: WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_LIMIT_UNITS + 1),
            total: WeightUnits::new(SYNTHETIC_BOUNDARY_MASS_UNITS),
        }),
        0
    );
    assert_eq!(
        assessment
            .totals()
            .excess(LimitBreach::GunPositions { limit: 6, used: 4 }),
        0
    );
    assert_eq!(
        assessment
            .totals()
            .excess(LimitBreach::RocketHardpoints { limit: 8, used: 4 }),
        0
    );

    // The synthetic profile and fixture are declared data, never original.
    let rules = synthetic_boundary_rules();
    assert_eq!(rules.origin(), &Origin::SyntheticFixture);
    assert!(!rules.origin().is_original());
    assert_eq!(
        rules.airframe(),
        &id(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY)
    );
    assert_eq!(
        declared_synthetic_blueprint().provenance(),
        &designed("f44a.blueprint.synthetic-boundary")
    );
}
