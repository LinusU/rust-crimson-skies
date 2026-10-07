//! F64-B acceptance tests: the verified read-only import subset
//! (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-B`).
//!
//! The stage's minimum scenario is sheet **AC02**: a legacy blueprint that
//! violates the current stock constraints is **rejected with specific
//! fields**. That is pinned here end to end: a synthetic document is read
//! through the production `read_legacy_profile`, its records are extended
//! into real `AircraftBlueprint`s through a declared `BlueprintFieldMap`,
//! and each is judged by the production `ConstructionRules::validate` — the
//! same validator every other blueprint faces — so a mass breach reports
//! `LimitBreach::Mass { limit, total }` with the exact units, not a prose
//! message.
//!
//! Everything is newly authored synthetic fixture content: the document's
//! layout (`synthetic_blueprint_layout`) and the field map
//! (`synthetic_blueprint_map`) are `ClaimStatus::Designed` and are only
//! admitted because the tests name `LayoutAdmission::AllowDesignedFixtures`.
//! No test reads `$CS_GAME_DIR` and nothing here is a claim about an original
//! stored-plane format — F64-B's retail measurement established that no such
//! file ships with the installation.

use cs_content::catalog::Catalog;
use cs_content::construction::{
    BudgetQuantity, BudgetRefusal, ConstructionPolicy, ConstructionRules, ConstructionSchemaError,
    ConstructionSlot, LimitBreach, MoneyMinor, PriceBook, SYNTHETIC_AIRFRAME_KEY,
    SYNTHETIC_ENGINE_KEY, SYNTHETIC_GUN_KEY, SYNTHETIC_MISSILE_KEY, SYNTHETIC_OTHER_AIRFRAME_KEY,
    ValidationRefusal, WeightUnits, declared_synthetic_price_book, synthetic_boundary_rules,
    synthetic_policy, synthetic_unmeasured_limit_rules,
};
use cs_content::damage::DamageNodeKey;
use cs_content::legacy_import::{
    BlueprintFieldMap, BlueprintFieldSlot, BlueprintImportRefusal, BlueprintImportReport,
    BlueprintImportRequest, BlueprintMapError, BlueprintRecordOutcome, BlueprintRecordRefusal,
    BlueprintRole, ImportClass, ImportRequest, LayoutAdmission, LegacyIdBinding, LegacyIdMap,
    TargetProfile, UnresolvedReason, assess_imported_blueprints, plan_import,
    synthetic_blueprint_map,
};
use cs_formats::legacy_profile::{
    ArtifactProposal, LegacyArtifactClass, LegacyIdClass, LegacyLimits, LegacyProfileDocument,
    read_legacy_profile, synthetic_blueprint_layout, synthetic_layout,
};
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind, Known,
    NormalizeState, Origin, Provenance, Readiness, Resolved, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::install::ParseState;
use cs_types::profile::{ProfileId, ProfileKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the fixture id is valid")
}

fn designed(value: &str) -> Provenance {
    Provenance::designed(ClaimId::new(value).expect("the fixture claim id is valid"))
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, designed("f64b.test.value")))
}

fn node(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("the fixture node key is valid")
}

fn ready_element(element_id: &ContentId) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id.clone(),
        display_name: Some(format!("Authored {element_id}")),
        origin: Origin::SyntheticFixture,
        dependencies: vec![Dependency {
            target: element_id.clone(),
            kind: DependencyKind::Static,
            provenance: designed("f64b.test.dependency"),
        }],
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f64b.test.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::<UnsupportedReason>::new(),
        fingerprint: None,
    }
}

/// A catalog holding the F44 synthetic components, all ready.
fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for element_id in [
        cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        cid(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY),
        cid(ContentKind::Engine, SYNTHETIC_ENGINE_KEY),
        cid(ContentKind::Weapon, SYNTHETIC_GUN_KEY),
        cid(ContentKind::Weapon, SYNTHETIC_MISSILE_KEY),
    ] {
        catalog
            .insert(ready_element(&element_id))
            .expect("the fixture element inserts");
    }
    catalog
}

/// The legacy ids the fixture document's records carry: airframe 7, engine 8,
/// gun 1 (a second gun at 2) and missiles at 5.
fn id_map() -> LegacyIdMap {
    let mut ids = LegacyIdMap::new();
    for binding in [
        (
            LegacyIdClass::Airframe,
            7,
            ContentKind::Airframe,
            SYNTHETIC_AIRFRAME_KEY,
        ),
        (
            LegacyIdClass::Engine,
            8,
            ContentKind::Engine,
            SYNTHETIC_ENGINE_KEY,
        ),
        (
            LegacyIdClass::Weapon,
            1,
            ContentKind::Weapon,
            SYNTHETIC_GUN_KEY,
        ),
        (
            LegacyIdClass::Weapon,
            2,
            ContentKind::Weapon,
            SYNTHETIC_GUN_KEY,
        ),
        (
            LegacyIdClass::Ordnance,
            5,
            ContentKind::Weapon,
            SYNTHETIC_MISSILE_KEY,
        ),
    ] {
        ids.insert(
            LegacyIdBinding::new(binding.0, binding.1, cid(binding.2, binding.3))
                .expect("the binding is in its namespace"),
        )
        .expect("the binding inserts");
    }
    ids
}

/// One record of the blueprint fixture document: a name plus the airframe,
/// engine, four gun and two rocket id slots.
struct BlueprintRecord {
    name: &'static str,
    airframe: u32,
    engine: u32,
    guns: [u32; 4],
    rockets: [u32; 2],
}

/// A synthetic document for `synthetic_blueprint_layout`.
fn document(records: &[BlueprintRecord]) -> Vec<u8> {
    let layout = synthetic_blueprint_layout();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&layout.magic());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(
        &u32::try_from(records.len())
            .expect("the fixture record count fits")
            .to_le_bytes(),
    );
    let mut label = [0u8; 8];
    label[..6].copy_from_slice(b"planes");
    bytes.extend_from_slice(&label);
    for record in records {
        let mut name = [0u8; 12];
        let taken = record.name.len().min(12);
        name[..taken].copy_from_slice(&record.name.as_bytes()[..taken]);
        bytes.extend_from_slice(&name);
        bytes.extend_from_slice(&record.airframe.to_le_bytes());
        bytes.extend_from_slice(&record.engine.to_le_bytes());
        for gun in record.guns {
            bytes.extend_from_slice(&gun.to_le_bytes());
        }
        for rocket in record.rockets {
            bytes.extend_from_slice(&rocket.to_le_bytes());
        }
    }
    bytes
}

/// The full record: every declared slot bound to a ready component.
fn full_record() -> BlueprintRecord {
    BlueprintRecord {
        name: "sparrow",
        airframe: 7,
        engine: 8,
        guns: [1, 1, 2, 1],
        rockets: [5, 5],
    }
}

/// The document read through the production reader.
fn read_document(bytes: &[u8]) -> LegacyProfileDocument {
    read_legacy_profile(
        bytes,
        &synthetic_blueprint_layout(),
        &LegacyLimits::designed(),
    )
    .expect("the fixture document reads")
}

/// The fixture admission, with the stock rules and book the tests pass.
#[allow(clippy::too_many_arguments)]
fn request<'a>(
    document: &'a LegacyProfileDocument,
    layout: &'a cs_formats::legacy_profile::LegacyLayout,
    field_map: &'a BlueprintFieldMap,
    ids: &'a LegacyIdMap,
    catalog: &'a Catalog,
    rules: &'a ConstructionRules,
    policy: &'a ConstructionPolicy,
    book: &'a PriceBook,
) -> BlueprintImportRequest<'a> {
    BlueprintImportRequest {
        document,
        layout,
        field_map,
        ids,
        catalog,
        rules,
        policy,
        book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::AllowDesignedFixtures,
    }
}

fn assess<'a>(request: &BlueprintImportRequest<'a>) -> BlueprintImportReport {
    assess_imported_blueprints(request).expect("the fixture document is assessed")
}

/// A rule profile for the fixture airframe with a tighter weight ceiling, so
/// the full record's 5370-unit blueprint is over it.
fn tight_mass_rules(limit: u64) -> ConstructionRules {
    ConstructionRules::try_new(
        cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(4),
        known(8),
        known(WeightUnits::new(limit)),
        known(MoneyMinor::new(42_000)),
        Origin::SyntheticFixture,
        designed("f64b.rules.tight-mass"),
    )
    .expect("the rule profile is valid")
}

/// A rule profile with a tighter rocket-hardpoint count.
fn tight_hardpoint_rules(limit: u32) -> ConstructionRules {
    ConstructionRules::try_new(
        cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY),
        known(4),
        known(limit),
        known(WeightUnits::new(6_040)),
        known(MoneyMinor::new(42_000)),
        Origin::SyntheticFixture,
        designed("f64b.rules.tight-hardpoints"),
    )
    .expect("the rule profile is valid")
}

/// A rule profile for a different airframe.
fn other_airframe_rules() -> ConstructionRules {
    ConstructionRules::try_new(
        cid(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY),
        known(4),
        known(8),
        known(WeightUnits::new(6_040)),
        known(MoneyMinor::new(42_000)),
        Origin::SyntheticFixture,
        designed("f64b.rules.other-airframe"),
    )
    .expect("the rule profile is valid")
}

/// A field map like the fixture's but with every gun occupying `positions`
/// gun positions.
fn map_with_gun_positions(positions: u32) -> BlueprintFieldMap {
    let gun = |field: &str, mount: &str| {
        BlueprintFieldSlot::new(
            field,
            BlueprintRole::Gun {
                mount: node(mount),
                positions: known(positions),
            },
        )
    };
    BlueprintFieldMap::new(
        "synthetic.fixture_blueprint_map/paired",
        ClaimStatus::Designed,
        vec![
            BlueprintFieldSlot::new("airframe_id", BlueprintRole::Airframe),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
            gun("gun_1", "mount_1"),
            gun("gun_2", "mount_2"),
            gun("gun_3", "mount_3"),
            gun("gun_4", "mount_4"),
            BlueprintFieldSlot::new(
                "rocket_1",
                BlueprintRole::Ordnance {
                    hardpoint: node("hardpoint_1"),
                },
            ),
            BlueprintFieldSlot::new(
                "rocket_2",
                BlueprintRole::Ordnance {
                    hardpoint: node("hardpoint_2"),
                },
            ),
        ],
    )
    .expect("the fixture map is valid")
}

/// A record inside every limit of the boundary profile is **conforming**, and
/// the report carries the assembled blueprint with its verdict — four guns on
/// four positions and two missiles on two of eight hardpoints.
#[test]
fn accept_f64_b_a_record_inside_the_stock_limits_is_conforming() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    assert_eq!(report.records().len(), 1);
    assert_eq!(report.conforming_count(), 1);
    assert_eq!(report.rejected_count(), 0);
    assert_eq!(report.refused_count(), 0);
    let BlueprintRecordOutcome::Conforming { blueprint, verdict } = &report.records()[0].outcome()
    else {
        panic!(
            "the full record must be conforming, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert!(verdict.is_valid());
    assert_eq!(
        blueprint.airframe(),
        &cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY)
    );
    assert_eq!(
        blueprint.engine(),
        &cid(ContentKind::Engine, SYNTHETIC_ENGINE_KEY)
    );
    assert_eq!(blueprint.guns().len(), 4);
    assert_eq!(blueprint.ordnance().len(), 2);
    assert_eq!(
        blueprint.id(),
        &cid(
            ContentKind::Blueprint,
            "legacy.synthetic.fixture_blueprint.v1.0"
        ),
        "the blueprint id is derived from the layout label and the record index"
    );
    let totals = verdict.assessment().totals();
    assert_eq!(totals.mass().as_units(), 4_000 + 600 + 4 * 160 + 2 * 65);
    assert_eq!(
        totals.cost().as_minor(),
        12_000 + 3_000 + 4 * 1_500 + 2 * 4_250
    );
    assert_eq!(totals.gun_positions(), 4);
    assert_eq!(totals.rocket_hardpoints(), 2);
}

/// **AC02**: a legacy blueprint over the stock weight ceiling is rejected,
/// and the rejection carries the exact fields — the limit and the measured
/// total in weight units — not a prose message.
#[test]
fn accept_f64_b_a_blueprint_over_the_weight_limit_is_rejected_with_specific_fields() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    // The full record weighs 5370 units; a 5000-unit ceiling rejects it while
    // cost, gun positions and hardpoints all stay inside.
    let rules = tight_mass_rules(5_000);
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    assert_eq!(report.rejected_count(), 1);
    assert_eq!(report.conforming_count(), 0);
    let BlueprintRecordOutcome::Rejected { verdict, .. } = &report.records()[0].outcome() else {
        panic!(
            "the overweight record must be rejected, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert!(!verdict.is_valid());
    assert_eq!(
        verdict.assessment().breaches(),
        &[LimitBreach::Mass {
            limit: WeightUnits::new(5_000),
            total: WeightUnits::new(5_370),
        }],
        "the rejection names the specific limit and the measured total"
    );
}

/// A legacy blueprint using more gun positions than the airframe offers is
/// rejected with the exact `used`/`limit` pair — four paired guns needing
/// eight positions on a four-position rack.
#[test]
fn accept_f64_b_a_blueprint_over_the_gun_rack_is_rejected_with_specific_fields() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = map_with_gun_positions(2);
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    let BlueprintRecordOutcome::Rejected { verdict, .. } = &report.records()[0].outcome() else {
        panic!(
            "the over-racked record must be rejected, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert_eq!(
        verdict.assessment().breaches(),
        &[LimitBreach::GunPositions { limit: 4, used: 8 }],
        "four two-position guns need eight positions on a four-position rack"
    );
}

/// A record with two missiles on a one-hardpoint rack is rejected with the
/// specific hardpoint fields.
#[test]
fn accept_f64_b_a_blueprint_over_the_hardpoints_is_rejected_with_specific_fields() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = tight_hardpoint_rules(1);
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    let BlueprintRecordOutcome::Rejected { verdict, .. } = &report.records()[0].outcome() else {
        panic!(
            "the over-hardpointed record must be rejected, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert_eq!(
        verdict.assessment().breaches(),
        &[LimitBreach::RocketHardpoints { limit: 1, used: 2 }],
        "two missiles on a one-hardpoint rack name the exact usage"
    );
}

/// A component id no binding maps stays an explicit unresolved row — the same
/// row shape the import plan reports — and the record is refused rather than
/// judged with a guessed component.
#[test]
fn accept_f64_b_an_unmapped_component_id_is_a_named_unresolved_row() {
    let mut record = full_record();
    record.guns[1] = 99;
    let bytes = document(&[record]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    assert_eq!(report.refused_count(), 1);
    let BlueprintRecordOutcome::Refused {
        reason: BlueprintRecordRefusal::Unresolved(rows),
    } = &report.records()[0].outcome()
    else {
        panic!(
            "an unmapped component must refuse with unresolved rows, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].record_index, Some(0));
    assert_eq!(rows[0].field.as_deref(), Some("gun_2"));
    assert_eq!(
        rows[0].reason,
        UnresolvedReason::IdNotMapped {
            class: LegacyIdClass::Weapon,
            raw: 99,
        }
    );
}

/// A field map whose role disagrees with the class the layout declared is a
/// declaration error refused before any record is read — the same discipline
/// as a wrong-namespace binding.
#[test]
fn accept_f64_b_a_field_map_mismatch_with_the_layout_is_refused_by_name() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    // The airframe field assigned a gun role: role class `weapon`, declared
    // class `airframe`.
    let wrong_role = BlueprintFieldMap::new(
        "test.mismatched_map",
        ClaimStatus::Designed,
        vec![
            BlueprintFieldSlot::new(
                "airframe_id",
                BlueprintRole::Gun {
                    mount: node("mount_1"),
                    positions: known(1),
                },
            ),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
            BlueprintFieldSlot::new("gun_1", BlueprintRole::Airframe),
        ],
    )
    .expect("the map's own structure is valid");
    let refusal = assess_imported_blueprints(&BlueprintImportRequest {
        document: &doc,
        layout: &layout,
        field_map: &wrong_role,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::AllowDesignedFixtures,
    })
    .expect_err("a role/class mismatch refuses the assessment");
    assert_eq!(
        refusal,
        BlueprintImportRefusal::Map(BlueprintMapError::RoleClassMismatch {
            field: "airframe_id".to_owned(),
            role_class: LegacyIdClass::Weapon,
            declared_class: LegacyIdClass::Airframe,
        })
    );

    // A field the layout does not declare at all is refused by name.
    let unknown_field = BlueprintFieldMap::new(
        "test.unknown_field_map",
        ClaimStatus::Designed,
        vec![
            BlueprintFieldSlot::new("airframe_id", BlueprintRole::Airframe),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
            BlueprintFieldSlot::new(
                "warp_core",
                BlueprintRole::Gun {
                    mount: node("mount_1"),
                    positions: known(1),
                },
            ),
        ],
    )
    .expect("the map's own structure is valid");
    let refusal = assess_imported_blueprints(&BlueprintImportRequest {
        document: &doc,
        layout: &layout,
        field_map: &unknown_field,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::AllowDesignedFixtures,
    })
    .expect_err("an undeclared field refuses the assessment");
    assert_eq!(
        refusal,
        BlueprintImportRefusal::Map(BlueprintMapError::FieldNotDeclared {
            field: "warp_core".to_owned(),
        })
    );

    // A declared record slot that is not an id slot — the text `name` — is
    // refused rather than read as an id.
    let text_as_id = BlueprintFieldMap::new(
        "test.text_id_map",
        ClaimStatus::Designed,
        vec![
            BlueprintFieldSlot::new("airframe_id", BlueprintRole::Airframe),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
            BlueprintFieldSlot::new(
                "name",
                BlueprintRole::Gun {
                    mount: node("mount_1"),
                    positions: known(1),
                },
            ),
        ],
    )
    .expect("the map's own structure is valid");
    let refusal = assess_imported_blueprints(&BlueprintImportRequest {
        document: &doc,
        layout: &layout,
        field_map: &text_as_id,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::AllowDesignedFixtures,
    })
    .expect_err("a text slot assigned an id role refuses the assessment");
    assert_eq!(
        refusal,
        BlueprintImportRefusal::Map(BlueprintMapError::FieldNotAnIdSlot {
            field: "name".to_owned(),
        })
    );
}

/// An unmeasured stock limit is a named refusal, never read as "no limit":
/// the record's blueprint assembles but no verdict is produced.
#[test]
fn accept_f64_b_unmeasured_limits_are_refused_not_read_as_no_limit() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_unmeasured_limit_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    let BlueprintRecordOutcome::Refused {
        reason:
            BlueprintRecordRefusal::Validation(ValidationRefusal::Budget(BudgetRefusal::UnknownLimit {
                quantity,
            })),
    } = &report.records()[0].outcome()
    else {
        panic!(
            "an unmeasured mass limit must refuse as UnknownLimit, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert_eq!(*quantity, BudgetQuantity::Mass);
}

/// A rules profile for a different airframe is a named refusal, not a verdict
/// — an imported plane is judged by its own airframe's rules or not at all.
#[test]
fn accept_f64_b_an_airframe_the_rules_do_not_describe_is_a_refusal_not_a_verdict() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = other_airframe_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));

    let BlueprintRecordOutcome::Refused {
        reason:
            BlueprintRecordRefusal::Validation(ValidationRefusal::Budget(
                BudgetRefusal::AirframeMismatch { blueprint, rules },
            )),
    } = &report.records()[0].outcome()
    else {
        panic!(
            "a mismatched rules profile must refuse as AirframeMismatch, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert_eq!(
        blueprint,
        &cid(ContentKind::Airframe, SYNTHETIC_AIRFRAME_KEY)
    );
    assert_eq!(
        rules,
        &cid(ContentKind::Airframe, SYNTHETIC_OTHER_AIRFRAME_KEY)
    );
}

/// Two guns claiming the same mount is a schema refusal, not a verdict: the
/// record's fitments cannot even assemble into a blueprint.
#[test]
fn accept_f64_b_guns_on_a_repeated_mount_are_refused_not_judged() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let repeated = BlueprintFieldMap::new(
        "test.repeated_mount_map",
        ClaimStatus::Designed,
        vec![
            BlueprintFieldSlot::new("airframe_id", BlueprintRole::Airframe),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
            BlueprintFieldSlot::new(
                "gun_1",
                BlueprintRole::Gun {
                    mount: node("mount_1"),
                    positions: known(1),
                },
            ),
            BlueprintFieldSlot::new(
                "gun_2",
                BlueprintRole::Gun {
                    mount: node("mount_1"),
                    positions: known(1),
                },
            ),
        ],
    )
    .expect("the map's own structure is valid");
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let report = assess(&BlueprintImportRequest {
        document: &doc,
        layout: &layout,
        field_map: &repeated,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::AllowDesignedFixtures,
    });

    let BlueprintRecordOutcome::Refused {
        reason: BlueprintRecordRefusal::Schema(ConstructionSchemaError::DuplicateSlot { slot }),
    } = &report.records()[0].outcome()
    else {
        panic!(
            "a repeated mount must refuse as DuplicateSlot, got {:?}",
            report.records()[0].outcome()
        );
    };
    assert_eq!(*slot, ConstructionSlot::WeaponMount);
}

/// A designed field map is refused under the default admission: only a caller
/// that names the fixture admission can assess one, and the report says so.
#[test]
fn accept_f64_b_a_designed_field_map_is_refused_under_measured_admission() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let refusal = assess_imported_blueprints(&BlueprintImportRequest {
        document: &doc,
        layout: &layout,
        field_map: &map,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::MeasuredOnly,
    })
    .expect_err("a designed map must be refused under the strict admission");
    assert_eq!(
        refusal,
        BlueprintImportRefusal::MapEvidence {
            map: "synthetic.fixture_blueprint_map/v1".to_owned(),
            evidence: ClaimStatus::Designed,
        }
    );

    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));
    assert!(
        report.admitted_designed_map() && report.admitted_designed_layout(),
        "a report made through the fixture admission must say so, for the map \
         and for the document's layout alike"
    );
    assert_eq!(report.layout_id(), "synthetic.fixture_blueprint/v1");
    assert_eq!(report.layout_evidence(), ClaimStatus::Designed);
}

/// A document read through a designed layout is refused under the strict
/// admission even when the field map is measured: the layout evidence the
/// document recorded at read time is gated exactly like the map's, so a
/// guessed layout can never reach a verdict behind a measured map.
#[test]
fn accept_f64_b_a_designed_layout_is_refused_under_measured_admission() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let layout = synthetic_blueprint_layout();
    // A *measured* map, so only the document's layout evidence is on trial.
    let measured_map = BlueprintFieldMap::new(
        "test.measured_map",
        ClaimStatus::VerifiedOriginal,
        vec![
            BlueprintFieldSlot::new("airframe_id", BlueprintRole::Airframe),
            BlueprintFieldSlot::new("engine_id", BlueprintRole::Engine),
        ],
    )
    .expect("the map's own structure is valid");
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let refusal = assess_imported_blueprints(&BlueprintImportRequest {
        document: &doc,
        layout: &layout,
        field_map: &measured_map,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::MeasuredOnly,
    })
    .expect_err("a designed layout must be refused under the strict admission");
    assert_eq!(
        refusal,
        BlueprintImportRefusal::LayoutEvidence {
            layout: "synthetic.fixture_blueprint/v1".to_owned(),
            evidence: ClaimStatus::Designed,
        }
    );
}

/// A layout that is not the one the document was read through is refused by
/// name: the map is validated against the layout that produced the records
/// or the assessment is refused, never against a stranger whose slots or
/// evidence the document does not share.
#[test]
fn accept_f64_b_a_layout_that_is_not_the_documents_is_refused() {
    let bytes = document(&[full_record()]);
    let doc = read_document(&bytes);
    let other = synthetic_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();

    let refusal = assess_imported_blueprints(&BlueprintImportRequest {
        document: &doc,
        layout: &other,
        field_map: &map,
        ids: &ids,
        catalog: &catalog,
        rules: &rules,
        policy: &policy,
        book: &book,
        origin: Origin::SyntheticFixture,
        provenance: designed("f64b.test.blueprint"),
        admission: LayoutAdmission::AllowDesignedFixtures,
    })
    .expect_err("a foreign layout refuses the assessment");
    assert_eq!(
        refusal,
        BlueprintImportRefusal::LayoutMismatch {
            document: "synthetic.fixture_blueprint/v1".to_owned(),
            layout: "synthetic.fixture_profile/v1".to_owned(),
        }
    );
}

/// The subset composes with the plan: the same bytes `plan_import` plans as
/// `Full` assess into conforming blueprints, so the plan's resolved
/// identities and the blueprint stage's verdicts describe one source.
#[test]
fn accept_f64_b_the_blueprint_stage_agrees_with_the_import_plan() {
    let bytes = document(&[full_record()]);
    let source = ArtifactProposal::new(
        "profiles/legacy/custom.pln",
        bytes.len() as u64,
        cs_assets::install::sha256(&bytes),
        Some(LegacyArtifactClass::CustomAircraft),
    )
    .expect("the fixture source is within the cap");
    let layout = synthetic_blueprint_layout();
    let map = synthetic_blueprint_map();
    let ids = id_map();
    let catalog = catalog();
    let rules = synthetic_boundary_rules();
    let policy = synthetic_policy();
    let book = declared_synthetic_price_book();
    let target = TargetProfile::new(
        ProfileId::new(9).expect("the fixture profile id is not zero"),
        ProfileKind::Production,
        "Imported legacy profile",
    )
    .expect("the target profile names itself");

    let plan = plan_import(&ImportRequest {
        source: &source,
        bytes: &bytes,
        layout: &layout,
        limits: LegacyLimits::designed(),
        ids: &ids,
        catalog: &catalog,
        target: &target,
        admission: LayoutAdmission::AllowDesignedFixtures,
        legacy_save_import_enabled: true,
        install_identity: None,
    })
    .expect("a resolvable document plans");
    assert_eq!(plan.class(), LegacyArtifactClass::CustomAircraft);
    assert_eq!(*plan.report().class(), ImportClass::Full);
    assert_eq!(plan.report().records().len(), 1);
    assert_eq!(plan.report().records()[0].resolved_ids.len(), 8);

    let doc = read_document(&bytes);
    let report = assess(&request(
        &doc, &layout, &map, &ids, &catalog, &rules, &policy, &book,
    ));
    assert_eq!(report.conforming_count(), 1);
    let BlueprintRecordOutcome::Conforming { blueprint, .. } = &report.records()[0].outcome()
    else {
        panic!("the planned record must assess as conforming");
    };
    assert_eq!(
        blueprint.airframe(),
        &plan.report().records()[0].resolved_ids[0].1,
        "the blueprint's airframe is the plan's first resolved identity"
    );
}
