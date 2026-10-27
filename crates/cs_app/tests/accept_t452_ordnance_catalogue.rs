//! Task T452 integration: the original PC ordnance catalogue, measured into
//! [`cs_sim::weapons::OrdnanceRegistry`].
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. The
//! measured inputs are T452.01's record census, T452.02's fuse shape and
//! sampling, T452.03's nitro numbers and T452.04's per-component table.
//!
//! Every test here calls production code: `cs_app::ordnance::original_catalogue`
//! and `cs_app::ordnance::read_original_catalogue` build the catalogue,
//! `cs_sim::weapons::OrdnanceRegistry` answers coverage, and
//! `cs_sim::weapons::OrdnanceState::fuse_decision` is the fuse path the measured
//! sampling model is implemented in. Nothing is reimplemented in a test.
//!
//! `accept_t452_retail_*` need `CS_GAME_DIR` and are `#[ignore]`; the others run
//! in CI with no original data. No test awards `verified_original`: *retail*
//! here is read access to original files, and no agent ran the original.

use cs_app::ordnance::{
    ORIGINAL_NITRO_COMPONENT_RECORDS, OriginalCatalogue, T452_FUSE_CLAIM, T452_LOST_TARGET_CLAIM,
    T452_NITRO_ACTIVATION_CLAIM, T452_NITRO_TRADEOFF_CLAIM, T452_RUNTIME_LOWERING_CLAIM,
    catalogue_measured_ordnance, original_catalogue, original_catalogue_coverage,
    read_original_catalogue,
};
use cs_content::ordnance::{
    DeclaredOrdnanceFamily, ORIGINAL_NITRO, ORIGINAL_NITRO_CONTROL, OriginalAreaDeclaration,
    OriginalClusterShape, OriginalComponentLaunch, OriginalComponentTable,
    OriginalComponentUnknown, OriginalDamageDeclaration, OriginalLifetime,
    OriginalMountDeclaration, OriginalMountLayout, OriginalNitro, OriginalRocketBlast,
    OriginalSlotArray, read_original_rocket_blasts,
};
use cs_sim::weapons::guns::ProjectileSegment;
use cs_sim::weapons::{
    CatalogueError, CataloguedFuse, CataloguedGuidance, CataloguedMount, CataloguedOrdnance,
    FuseDecision, FuseRule, FuseTrigger, LostTargetBehavior, OrdnanceFamily, OrdnanceId,
    OrdnanceRegistry, OrdnanceRegistryError, OrdnanceState, ProximityFuse, ProximitySampling,
    ProximityShape, TargetPath,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{ActorId, SessionId};
use cs_types::space::WorldPosition;

// ------------------------------------------------------------------ fixtures --

fn record_id(record: &str) -> OrdnanceId {
    OrdnanceId::try_new(ContentId::from_source(ContentKind::Weapon, record).expect("valid id"))
        .expect("the weapon namespace is accepted")
}

fn slots(slots: u32) -> OriginalSlotArray {
    OriginalSlotArray {
        member: "fixture.script".to_owned(),
        array: "RKA".to_owned(),
        slots,
        loop_bound: slots,
        deactivates_empty_slots: true,
    }
}

fn layout(rocket_slots: u32) -> OriginalMountLayout {
    OriginalMountLayout {
        rocket_slots: slots(rocket_slots),
        ammunition_slots: slots(4),
        hardpoint_points: slots(2),
        findings: Vec::new(),
    }
}

/// One measured row, with every value the record does not state left empty —
/// as the production reader returns it.
fn row(record: &str, family: DeclaredOrdnanceFamily) -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: record.to_owned(),
        family,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: Some(1200.0),
        fire_rate: Some(1.0),
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration::default(),
        damage: OriginalDamageDeclaration::default(),
        statuses: Vec::new(),
        cluster: OriginalClusterShape::Undeclared,
        unknowns: vec![OriginalComponentUnknown {
            claim: cs_content::ordnance::T452_04_VELOCITY_UNIT.to_owned(),
            reason: "no file states that VELOCITY is metres per second".to_owned(),
        }],
    }
}

fn table(rows: Vec<OriginalComponentLaunch>) -> OriginalComponentTable {
    OriginalComponentTable {
        layout: layout(8),
        rows,
        excluded: vec!["wep_00".to_owned()],
        unknowns: Vec::new(),
    }
}

/// One measured `ROCKET` blast record, as the rocket reader returns it.
fn measured_blast(record: &str, proximity: f64, time: Option<f64>) -> OriginalRocketBlast {
    OriginalRocketBlast {
        record: record.to_owned(),
        impact_proximity: Some(proximity),
        detonation_distance: Some(proximity),
        detonation_time_s: time,
        armor_damage: Some(10.0),
        health_damage: Some(10.0),
        lock_on: Some(1.0),
        range: Some(900.0),
        range_minimum: None,
        range_minimum_blast_scale: None,
        keys: ["LOCK_ON", "DETONATION_DISTANCE", "RANGE"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

fn catalogue_of(
    rows: Vec<OriginalComponentLaunch>,
    blasts: &[OriginalRocketBlast],
) -> OriginalCatalogue {
    original_catalogue(&table(rows), blasts, &ORIGINAL_NITRO).expect("the fixture catalogues")
}

fn entry<'a>(catalogue: &'a OriginalCatalogue, record: &str) -> &'a CataloguedOrdnance {
    &catalogue
        .entry(record)
        .expect("the fixture entry exists")
        .entry
}

// ------------------------------------------------------- catalogue entries --

#[test]
fn accept_t452_a_catalogued_entry_refuses_a_family_contradiction() {
    // A guided rocket that does not track contradicts the family by
    // definition, and an area-denial component that does track contradicts its
    // own measured rule (T452.01 classifies only non-`LOCK_ON` records as area
    // denial). Both are refused at construction rather than catalogued.
    let rocket = CataloguedOrdnance::try_new(
        record_id("wep_04"),
        OrdnanceFamily::GuidedRocket,
        CataloguedMount::RocketSlot { slots: 8 },
        CataloguedGuidance::Unguided,
        CataloguedFuse::Proximity {
            fuse: ProximityFuse::try_new(30.0).expect("valid radius"),
            shape: ProximityShape::Sphere,
        },
        cs_sim::weapons::CataloguedArming::OnRelease,
        Vec::new(),
    );
    assert_eq!(
        rocket,
        Err(CatalogueError::GuidanceContradictsFamily {
            family: OrdnanceFamily::GuidedRocket
        })
    );

    let denial = CataloguedOrdnance::try_new(
        record_id("wep_12"),
        OrdnanceFamily::AreaDenialEngine,
        CataloguedMount::RocketSlot { slots: 8 },
        CataloguedGuidance::Targeted { lost_target: None },
        CataloguedFuse::Undeclared {
            claim: T452_FUSE_CLAIM.to_owned(),
        },
        cs_sim::weapons::CataloguedArming::OnRelease,
        Vec::new(),
    );
    assert_eq!(
        denial,
        Err(CatalogueError::GuidanceContradictsFamily {
            family: OrdnanceFamily::AreaDenialEngine
        })
    );

    // A booster family is not a catalogued projectile at all.
    let booster = CataloguedOrdnance::try_new(
        record_id("wep_14"),
        OrdnanceFamily::NitroBooster,
        CataloguedMount::RocketSlot { slots: 8 },
        CataloguedGuidance::Unguided,
        CataloguedFuse::Impact,
        cs_sim::weapons::CataloguedArming::OnRelease,
        Vec::new(),
    );
    assert_eq!(
        booster,
        Err(CatalogueError::FamilyWithoutBehaviour {
            family: OrdnanceFamily::NitroBooster
        })
    );
}

#[test]
fn accept_t452_a_proximity_fuse_keeps_the_measured_shape_and_sampling() {
    let mut guided = row("wep_04", DeclaredOrdnanceFamily::GuidedRocket);
    guided.proximity_trigger_m = Some(30.0);
    let catalogue = catalogue_of(vec![guided], &[measured_blast("wep_04", 30.0, None)]);
    let entry = entry(&catalogue, "wep_04");
    let CataloguedFuse::Proximity { fuse, shape } = entry.fuse() else {
        panic!("wep_04's DETONATION_DISTANCE must be its proximity fuse");
    };
    // T452.02: a sphere of the record's radius, sampled once per step at the
    // item's start-of-step position — not the swept relative-path model.
    assert_eq!(*shape, ProximityShape::Sphere);
    assert_eq!(fuse.trigger_radius_m(), 30.0);
    assert_eq!(fuse.sampling(), ProximitySampling::StartOfStepSample);
    assert!(fuse.sampling().is_measured_original());
    assert_ne!(fuse.sampling(), ProximitySampling::SweptRelativePath);
    assert_eq!(entry.arming(), cs_sim::weapons::CataloguedArming::OnRelease);
}

#[test]
fn accept_t452_the_start_of_step_fuse_misses_a_crossing_inside_a_step() {
    // The measured sampling model is production code: `fuse_decision` takes the
    // sampling from the fuse. One crossing that happens entirely inside a step
    // is caught by the swept model and missed by the original's single sample,
    // which is exactly the difference T452.02 measured.
    let item_start = WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite");
    let item_end = WorldPosition::try_new([10.0, 0.0, 0.0]).expect("finite");
    let segment = ProjectileSegment {
        projectile: cs_sim::weapons::ProjectileId {
            session: 1,
            serial: 1,
        },
        previous: item_start,
        current: item_end,
    };
    let path = TargetPath::try_new(
        ActorId {
            session: SessionId::new(1).expect("session"),
            serial: 2,
        },
        [5.0, 5.0, 0.0],
        [5.0, -5.0, 0.0],
    )
    .expect("finite target path");

    let mut swept = OrdnanceState::launch(
        cs_sim::weapons::ProjectileId {
            session: 1,
            serial: 1,
        },
        record_id("wep_04"),
        OrdnanceFamily::GuidedRocket,
        cs_sim::weapons::ArmingRule::AfterTicks(0),
        FuseRule::Proximity(ProximityFuse::try_new(2.0).expect("valid radius")),
        600,
        Tick(0),
    );
    let swept_decision = swept.fuse_decision(&segment, std::slice::from_ref(&path), &[]);
    let FuseDecision::Triggered(FuseTrigger::Proximity { distance_m, .. }) = swept_decision else {
        panic!("the swept model must catch the crossing, got {swept_decision:?}");
    };
    assert!(distance_m <= 2.0);

    let mut sampled = OrdnanceState::launch(
        cs_sim::weapons::ProjectileId {
            session: 1,
            serial: 2,
        },
        record_id("wep_05"),
        OrdnanceFamily::GuidedRocket,
        cs_sim::weapons::ArmingRule::AfterTicks(0),
        FuseRule::Proximity(
            ProximityFuse::try_new(2.0)
                .expect("valid radius")
                .with_sampling(ProximitySampling::StartOfStepSample),
        ),
        600,
        Tick(0),
    );
    let sampled_decision = sampled.fuse_decision(&segment, std::slice::from_ref(&path), &[]);
    assert!(
        matches!(sampled_decision, FuseDecision::Inert(_)),
        "the original's single sample cannot see a pass inside one step, got {sampled_decision:?}"
    );
}

#[test]
fn accept_t452_a_guided_entry_names_its_lost_target_rule_or_its_claim() {
    let measured = catalogue_of(
        vec![row("wep_04", DeclaredOrdnanceFamily::GuidedRocket)],
        &[measured_blast("wep_04", 30.0, None)],
    );
    assert_eq!(
        entry(&measured, "wep_04").lost_target(),
        Some(LostTargetBehavior::CoastThenDetonate),
        "task #454 measured a lost target as a cleared pointer: the item flies on and \
         detonates at the end of RANGE"
    );
    assert!(!entry(&measured, "wep_04").names_claim(T452_LOST_TARGET_CLAIM));

    // No measured blast record (the glidebomb `wep_25` is `LOCK_ON` but not
    // `ROCKET`-flagged): the entry is still a seeker, and the rule it does not
    // know is a claim rather than an assumed `Coast`.
    let unmeasured = catalogue_of(
        vec![row("wep_25", DeclaredOrdnanceFamily::GuidedRocket)],
        &[],
    );
    let guided = entry(&unmeasured, "wep_25");
    assert!(guided.guidance().is_targeted());
    assert_eq!(guided.lost_target(), None);
    assert!(guided.names_claim(T452_LOST_TARGET_CLAIM));
}

#[test]
fn accept_t452_an_unmeasured_range_end_is_a_claim_not_a_coast_detonate() {
    // Task #454 measured coast-then-detonate-at-end-of-RANGE for the twelve
    // `ROCKET` records: locked on, no `EXPIRES`, so the end of range detonates.
    // A record that declares `EXPIRES` without `DETONATE_AT_RANGE` is outside
    // that measurement — what removes it at the end of range is unmeasured
    // (`t454.limit.range_end_removal`) — so the catalogue names the claim rather
    // than folding it into the measured rule.
    let mut expires = measured_blast("wep_04", 30.0, None);
    expires.keys.insert("EXPIRES".to_owned());
    let catalogue = catalogue_of(
        vec![row("wep_04", DeclaredOrdnanceFamily::GuidedRocket)],
        &[expires],
    );
    let guided = entry(&catalogue, "wep_04");
    assert!(guided.guidance().is_targeted());
    assert_eq!(guided.lost_target(), None);
    assert!(guided.names_claim(T452_LOST_TARGET_CLAIM));
}

#[test]
fn accept_t452_an_unmeasured_mount_is_a_claim_not_a_default_slot() {
    let mut glidebomb = row("wep_25", DeclaredOrdnanceFamily::GuidedRocket);
    glidebomb.mount = OriginalMountDeclaration::Unknown {
        claim: cs_content::ordnance::T452_04_MOUNT.to_owned(),
        reason: "the record reaches an ordnance family but declares no ROCKET flag".to_owned(),
    };
    glidebomb.unknowns.push(OriginalComponentUnknown {
        claim: cs_content::ordnance::T452_04_MOUNT.to_owned(),
        reason: "no screen states which slot holds a record without a ROCKET flag".to_owned(),
    });
    let catalogue = catalogue_of(vec![glidebomb], &[]);
    let entry = entry(&catalogue, "wep_25");
    assert_eq!(entry.mount().slots(), None);
    assert!(matches!(entry.mount(), CataloguedMount::Unknown { .. }));
    assert!(entry.names_claim(cs_content::ordnance::T452_04_MOUNT));
}

#[test]
fn accept_t452_every_declared_id_is_queryable_in_the_registry() {
    let catalogue = catalogue_of(
        vec![
            row("wep_04", DeclaredOrdnanceFamily::GuidedRocket),
            row("wep_12", DeclaredOrdnanceFamily::AreaDenialEngine),
            row("wep_26", DeclaredOrdnanceFamily::ProximityFlak),
        ],
        &[measured_blast("wep_04", 30.0, None)],
    );
    let mut registry = OrdnanceRegistry::new();
    let added = catalogue_measured_ordnance(&catalogue, &mut registry).expect("no duplicates");
    assert_eq!(added, 3);
    for record in ["wep_04", "wep_12", "wep_26"] {
        let id = record_id(record);
        let resolved = registry
            .catalogued(&id)
            .unwrap_or_else(|| panic!("{record} must resolve in the registry"));
        assert_eq!(resolved.ordnance(), &id);
        assert_eq!(resolved.family(), entry(&catalogue, record).family());
        assert_eq!(resolved.guidance(), entry(&catalogue, record).guidance());
        assert_eq!(resolved.fuse(), entry(&catalogue, record).fuse());
        assert_eq!(resolved.mount(), entry(&catalogue, record).mount());
        assert!(!resolved.unmeasured().is_empty());
    }
    let coverage = original_catalogue_coverage(&catalogue, &registry);
    assert!(coverage.is_complete());
    assert!(coverage.missing.is_empty());
    assert_eq!(coverage.catalogued, 3);

    // A second cataloguing of the same id is a contradiction, not an overwrite.
    assert!(matches!(
        catalogue_measured_ordnance(&catalogue, &mut registry),
        Err(OrdnanceRegistryError::DuplicateOrdnance { .. })
    ));
}

#[test]
fn accept_t452_an_uncatalogued_id_is_reported_missing() {
    let catalogue = catalogue_of(
        vec![row("wep_04", DeclaredOrdnanceFamily::GuidedRocket)],
        &[measured_blast("wep_04", 30.0, None)],
    );
    let registry = OrdnanceRegistry::new();
    let coverage = original_catalogue_coverage(&catalogue, &registry);
    assert!(!coverage.is_complete());
    assert_eq!(coverage.missing, vec!["wep_04".to_owned()]);
    assert_eq!(coverage.declared, 1);
    assert_eq!(coverage.catalogued, 0);
}

#[test]
fn accept_t452_the_nitro_record_carries_the_measured_rule_and_its_vocabulary_gap() {
    let catalogue = catalogue_of(Vec::new(), &[]);
    let nitro = &catalogue.nitro;
    assert_eq!(nitro.control, ORIGINAL_NITRO_CONTROL);
    assert_eq!(nitro.control.0, "MPOUT_CHK_NITRO");
    assert_eq!(nitro.component_records, ORIGINAL_NITRO_COMPONENT_RECORDS);
    assert_eq!(nitro.capacity, 30.0);
    assert_eq!(nitro.consumption_per_dt, 4.0);
    assert_eq!(nitro.recovery_per_dt, 1.0);
    assert_eq!(nitro.start_min_fraction, 0.99);
    assert_eq!(nitro.stop_below_fraction, 0.05);
    assert_eq!(nitro.running_engine_value, 1.8);
    assert_eq!(nitro.running_result_factor, 0.8);
    // Neither "while held" nor a fixed burn: the measured rule is charge
    // bounded, and `NitroActivationRule` has no such variant, so the gap is a
    // claim rather than a variant the rule is not.
    assert!(!nitro.activation_is_while_held());
    assert_eq!(nitro.extra_thrust_n(), None);
    assert!(nitro.names_claim(T452_NITRO_ACTIVATION_CLAIM));
    assert!(nitro.names_claim(T452_NITRO_TRADEOFF_CLAIM));
}

#[test]
fn accept_t452_a_timed_fuse_keeps_the_records_seconds_and_no_tick_count() {
    // A record whose only stated end is its duration keeps the record's seconds:
    // turning it into a tick count needs a rate no file states.
    let mut timed = row("wep_18", DeclaredOrdnanceFamily::GuidedRocket);
    timed.proximity_trigger_m = None;
    timed.lifetime = OriginalLifetime::Declared {
        seconds: 2.0,
        key: "DETONATION_TIME",
    };
    let stated = catalogue_of(vec![timed], &[]);
    assert_eq!(
        entry(&stated, "wep_18").fuse(),
        &CataloguedFuse::TimedSeconds { seconds: 2.0 }
    );

    // A measured DETONATION_DISTANCE is the proximity fuse even when the record
    // also states a timer.
    let mut both = row("wep_16", DeclaredOrdnanceFamily::GuidedRocket);
    both.proximity_trigger_m = Some(1.0);
    both.lifetime = OriginalLifetime::Declared {
        seconds: 2.0,
        key: "DETONATION_TIME",
    };
    let fused = catalogue_of(vec![both], &[measured_blast("wep_16", 1.0, Some(2.0))]);
    let CataloguedFuse::Proximity { .. } = entry(&fused, "wep_16").fuse() else {
        panic!("a measured DETONATION_DISTANCE is the proximity fuse");
    };

    // A record with no detonation key cannot default to impact.
    let theatre = catalogue_of(
        vec![row("wep_17", DeclaredOrdnanceFamily::GuidedRocket)],
        &[],
    );
    let CataloguedFuse::Undeclared { claim } = entry(&theatre, "wep_17").fuse() else {
        panic!("a record with no detonation key cannot default to impact");
    };
    assert_eq!(claim, T452_FUSE_CLAIM);
}

// --------------------------------------------------------------------- retail --

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!(
            "CS_GAME_DIR is not set: accept_t452_retail_* need the original installation; \
             run them with --include-ignored and CS_GAME_DIR set"
        )
    });
    std::path::PathBuf::from(dir)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_retail_every_declared_component_id_resolves_in_the_registry() {
    let catalogue = read_original_catalogue(&game_dir()).expect("the installation catalogues");
    // T452.01/T452.04: sixteen classified records — twelve guided, two
    // area-denial, one aerial torpedo, one proximity flak — and thirty-two
    // records no family rule reaches, named rather than folded.
    assert_eq!(catalogue.entries.len(), 16);
    assert_eq!(catalogue.excluded.len(), 32);
    assert_eq!(catalogue.of_family(OrdnanceFamily::GuidedRocket).len(), 12);
    assert_eq!(
        catalogue.of_family(OrdnanceFamily::AreaDenialEngine).len(),
        2
    );
    assert_eq!(catalogue.of_family(OrdnanceFamily::AerialTorpedo).len(), 1);
    assert_eq!(catalogue.of_family(OrdnanceFamily::ProximityFlak).len(), 1);

    let mut registry = OrdnanceRegistry::new();
    let added = catalogue_measured_ordnance(&catalogue, &mut registry).expect("no duplicates");
    assert_eq!(added, catalogue.entries.len());

    let coverage = original_catalogue_coverage(&catalogue, &registry);
    assert!(
        coverage.is_complete(),
        "every declared id must resolve: missing {:?}",
        coverage.missing
    );

    // Every id resolves with a family and a declared behavior, not merely a row.
    for declared in &catalogue.entries {
        let resolved = registry
            .catalogued(declared.entry.ordnance())
            .unwrap_or_else(|| panic!("{} must be catalogued", declared.record));
        assert_eq!(resolved.family(), declared.entry.family());
        assert_eq!(resolved.fuse(), declared.entry.fuse());
        assert_eq!(resolved.arming(), declared.entry.arming());
        assert_eq!(resolved.mount(), declared.entry.mount());
        assert_eq!(resolved.guidance(), declared.entry.guidance());
        assert!(
            resolved.fuse().label() != "undeclared" || resolved.names_claim(T452_FUSE_CLAIM),
            "{} has no end-of-flight rule and no claim",
            declared.record
        );
    }

    // The measured screen layout is the one the catalogue reports.
    assert_eq!(catalogue.rocket_slots, 8);
    assert_eq!(catalogue.ammunition_slots, 4);
    assert_eq!(catalogue.hardpoint_points, 2);

    // The excluded records are named, and no catalogued component is also
    // excluded: the thirty-one `CANNON` machine and turret guns are F27's, and
    // the cannonball `wep_28` reaches no stated family (T452.01's
    // `t452_01.limit.wep_28`), so it is named as a limitation rather than
    // catalogued with an invented one.
    assert!(
        catalogue.excluded.iter().any(|record| record == "wep_28"),
        "the cannonball is a named exclusion: {:?}",
        catalogue.excluded
    );
    for declared in &catalogue.entries {
        assert!(
            !catalogue.excluded.contains(&declared.record),
            "{} is both catalogued and excluded",
            declared.record
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_retail_each_fuse_is_the_records_own_trigger() {
    let dir = game_dir();
    let catalogue = read_original_catalogue(&dir).expect("the installation catalogues");
    let blasts = read_original_rocket_blasts(&dir).expect("the rocket records read");
    for blast in &blasts {
        let Some(fuse) = blast.proximity_fuse() else {
            continue;
        };
        let CataloguedFuse::Proximity {
            fuse: catalogued,
            shape,
        } = entry(&catalogue, &blast.record).fuse()
        else {
            panic!(
                "{} declares DETONATION_DISTANCE but is not catalogued as a fuse",
                blast.record
            );
        };
        assert_eq!(*shape, ProximityShape::Sphere);
        assert_eq!(catalogued.trigger_radius_m(), fuse.trigger_radius_m);
        assert_eq!(
            catalogued.sampling(),
            ProximitySampling::StartOfStepSample,
            "{} must keep the measured sampling",
            blast.record
        );
    }
    // The flare is the one record whose only stated end is its timer.
    let flare = entry(&catalogue, "wep_15");
    assert!(
        matches!(flare.fuse(), CataloguedFuse::TimedSeconds { seconds } if *seconds == 2.0),
        "wep_15 states DETONATION_TIME 2 and no DETONATION_DISTANCE"
    );
    // `wep_12` declares its own `DETONATION_DISTANCE` but is unguided, and the
    // measured proximity burst requires `LOCK_ON`, so that value never reaches
    // the fuse; the smoke record and the flak record state no trigger of their
    // own either. All three keep an undeclared fuse and say so.
    for record in ["wep_12", "wep_13", "wep_26"] {
        assert!(
            matches!(
                entry(&catalogue, record).fuse(),
                CataloguedFuse::Undeclared { .. }
            ),
            "{record} states no trigger the measured engine path reaches"
        );
        assert!(entry(&catalogue, record).names_claim(T452_FUSE_CLAIM));
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_retail_the_nitro_record_is_the_measured_one() {
    let catalogue = read_original_catalogue(&game_dir()).expect("the installation catalogues");
    let nitro: &OriginalNitro = &ORIGINAL_NITRO;
    assert_eq!(catalogue.nitro.capacity, nitro.capacity);
    assert_eq!(catalogue.nitro.consumption_per_dt, nitro.consumption_per_dt);
    assert_eq!(catalogue.nitro.recovery_per_dt, nitro.recovery_per_dt);
    assert_eq!(catalogue.nitro.start_min_fraction, 0.99);
    assert_eq!(catalogue.nitro.stop_below_fraction, 0.05);
    assert_eq!(
        catalogue.nitro.component_records,
        ORIGINAL_NITRO_COMPONENT_RECORDS
    );
    assert_eq!(catalogue.nitro.control.1, 10135);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_retail_every_entry_names_the_values_it_does_not_measure() {
    let catalogue = read_original_catalogue(&game_dir()).expect("the installation catalogues");
    let mut claims: Vec<&str> = Vec::new();
    for declared in &catalogue.entries {
        let entry = &declared.entry;
        assert!(
            !entry.unmeasured().is_empty(),
            "{} states no unmeasured value at all, which cannot be true of a record with \
             no stack, media or equipment keys",
            declared.record
        );
        assert!(
            entry.names_claim(T452_RUNTIME_LOWERING_CLAIM),
            "{} must say why it is catalogued and not registered",
            declared.record
        );
        for unknown in entry.unmeasured() {
            assert!(!unknown.field.trim().is_empty());
            assert!(!unknown.claim.trim().is_empty());
            assert!(!unknown.reason.trim().is_empty());
            claims.push(&unknown.claim);
        }
    }
    for claim in [
        cs_content::ordnance::T452_04_VELOCITY_UNIT,
        cs_content::ordnance::T452_04_RELEASE_DELAY,
        cs_content::ordnance::T452_04_NO_EXPIRES,
        T452_RUNTIME_LOWERING_CLAIM,
    ] {
        assert!(
            claims.contains(&claim),
            "the catalogue must record {claim} against the components it affects"
        );
    }
    // The glidebomb is the one record with no stated slot.
    assert!(matches!(
        entry(&catalogue, "wep_25").mount(),
        CataloguedMount::Unknown { .. }
    ));
    // Every claim the catalogue records maps to a catalogue field.
    for claim in catalogue.claims() {
        let field = cs_app::ordnance::catalogue_field(&claim);
        assert!(!field.is_empty(), "{claim} maps to no field");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_retail_the_projectile_component_type_is_still_the_runtime_one() {
    // The catalogue entry is not a runtime component: nothing above may have
    // smuggled a fabricated stack, media or equipment record into one. This
    // asserts the type boundary directly, so a future "just register it" change
    // has to come with the measured values instead of a default.
    let catalogue = read_original_catalogue(&game_dir()).expect("the installation catalogues");
    let mut registry = OrdnanceRegistry::new();
    catalogue_measured_ordnance(&catalogue, &mut registry).expect("no duplicates");
    for declared in &catalogue.entries {
        let id = declared.entry.ordnance();
        assert!(
            registry.catalogued(id).is_some(),
            "{} is catalogued",
            declared.record
        );
        assert!(
            registry.get(id).is_none(),
            "{} must not be registered as a flyable component while its stack, media and \
             equipment rules are unmeasured",
            declared.record
        );
    }
}
