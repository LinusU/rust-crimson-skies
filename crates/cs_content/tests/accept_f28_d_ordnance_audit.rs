//! F28-D through the declared ordnance catalogue: the audit's rows, every one
//! of its ten named findings, and the closure checks against a measured
//! installation surface.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Task test prefix: `accept_f28_d_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! These tests drive production code: `cs_content::ordnance`'s
//! [`OrdnanceAudit`] over real [`DeclaredOrdnance`] records. The runtime half is
//! `crates/cs_app/tests/accept_f28_d_ordnance_catalogue.rs`; the retail
//! re-measurement of every constant is
//! `crates/cs_content/tests/accept_f28_d_retail_ordnance_catalogue.rs`.
//!
//! The surfaces here are **synthetic**: they are built in the test, not read
//! from an installation. The one place a real measurement appears is
//! `ORIGINAL_ROCKET_ORDNANCE_TYPES` and its neighbours, which the retail test
//! re-derives. Every record value is the fixture's own, never original data.

use cs_content::ordnance::{
    DECLARED_FIELDS_WITHOUT_CONSUMER, DECLARED_SYNTHETIC_AREA_DENIAL_KEY,
    DECLARED_SYNTHETIC_DIRECT_KEY, DECLARED_SYNTHETIC_FLAK_KEY, DECLARED_SYNTHETIC_GUIDED_KEY,
    DECLARED_SYNTHETIC_NITRO_KEY, DECLARED_SYNTHETIC_TORPEDO_KEY, DeclaredArmingRule,
    DeclaredFuseRule, DeclaredGuidanceRule, DeclaredOrdnance, DeclaredOrdnanceDetails,
    DeclaredOrdnanceFamily, DeclaredProjectile, ORIGINAL_NEXT_ROCKET_BLOCK, ORIGINAL_NITRO_CONTROL,
    ORIGINAL_ORDNANCE_HARDPOINT_POINTS, ORIGINAL_ORDNANCE_ROCKET_SLOTS,
    ORIGINAL_ROCKET_NAME_BLOCKS, ORIGINAL_ROCKET_ORDNANCE_TYPES, OrdnanceAudit,
    OrdnanceAuditFinding, OriginalOrdnanceCounts, OriginalOrdnanceSurface,
    OriginalOrdnanceSurfaceError, declared_synthetic_area_denial, declared_synthetic_direct,
    declared_synthetic_flak, declared_synthetic_guided, declared_synthetic_nitro,
    declared_synthetic_provenance, declared_synthetic_torpedo, declared_unknown,
};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

/// The known value of a `Resolved`, cloned out of a shared reference.
fn known_of<T: Clone>(resolved: &Resolved<T>) -> Option<T> {
    match resolved {
        Resolved::Known(known) => Some(known.value.clone()),
        Resolved::Unknown { .. } => None,
    }
}

/// The claim the synthetic surfaces in this file are recorded under.
fn claim(name: &str) -> ClaimId {
    ClaimId::new(name).expect("a valid claim id")
}

/// The whole synthetic declared catalogue: one record per designed family.
fn declared_catalogue() -> Vec<DeclaredOrdnance> {
    vec![
        declared_synthetic_direct(),
        declared_synthetic_flak(),
        declared_synthetic_guided(),
        declared_synthetic_area_denial(),
        declared_synthetic_torpedo(),
        declared_synthetic_nitro(),
    ]
}

/// A **measured** value: `verified_original` provenance, which is what a value
/// read out of an installation's files carries and what the synthetic fixture
/// never can.
fn measured<T: Clone>(id: ClaimId, value: T) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        Provenance::new(
            id,
            ClaimStatus::VerifiedOriginal,
            Some(
                cs_types::asset_id::SourceSpan::new(
                    cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                    "GOSDATA/ASSETS/crimson.rof",
                    Some("ASSETS/SCRIPTS/RESOURCE.H"),
                    0,
                    1,
                    None,
                )
                .expect("a span inside one named member is valid"),
            ),
        )
        .expect("a verified_original provenance names its span"),
    ))
}

/// A declared record whose **every** load-bearing value is `verified_original`,
/// which is the only way a row can be `is_measured`.
fn measured_record(key: &str, family: DeclaredOrdnanceFamily, id: ClaimId) -> DeclaredOrdnance {
    let fixture = declared_synthetic_direct();
    let DeclaredOrdnanceDetails::Projectile(projectile) = fixture.details() else {
        panic!("the direct-explosive fixture is a projectile")
    };
    let mut projectile: DeclaredProjectile = (**projectile).clone();
    projectile.launch.hardpoint = measured(
        id.clone(),
        projectile
            .launch
            .hardpoint
            .known()
            .expect("the fixture's hardpoint kind is known"),
    );
    projectile.launch.launch_speed_mps = measured(id.clone(), 210.0);
    projectile.launch.inheritance = measured(
        id.clone(),
        projectile
            .launch
            .inheritance
            .known()
            .expect("the fixture's inheritance is known"),
    );
    projectile.launch.release_delay_ticks = measured(id.clone(), 0);
    projectile.stack.capacity_units = measured(id.clone(), 6);
    projectile.stack.unit_mass_kg = measured(id.clone(), 2.4);
    projectile.arming = DeclaredArmingRule::AfterTicks(measured(id.clone(), 3));
    projectile.fuse = match projectile.fuse {
        DeclaredFuseRule::Impact => DeclaredFuseRule::Impact,
        other => other,
    };
    projectile.guidance = DeclaredGuidanceRule::Unguided;
    projectile.lifetime_ticks = measured(id.clone(), 240);
    projectile.armor_damage = measured(id.clone(), 22.0);
    projectile.internal_damage = measured(id.clone(), 9.0);
    projectile.media.visual = measured(
        id.clone(),
        projectile
            .media
            .visual
            .known()
            .expect("the fixture's visual is known"),
    );
    projectile.media.sound = measured(
        id.clone(),
        projectile
            .media
            .sound
            .known()
            .expect("the fixture's sound is known"),
    );
    projectile.media.particles = projectile.media.particles.as_ref().map(|particles| {
        measured(
            id.clone(),
            known_of(particles).expect("the fixture's particles are known"),
        )
    });
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, key).expect("a weapon id"),
        Origin::Installation {
            source: cs_types::asset_id::SourceSpan::new(
                cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                "GOSDATA/ASSETS/crimson.rof",
                Some("ASSETS/SCRIPTS/RESOURCE.H"),
                0,
                1,
                None,
            )
            .expect("a valid span"),
        },
        family,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        Provenance::new(
            id,
            ClaimStatus::VerifiedOriginal,
            Some(
                cs_types::asset_id::SourceSpan::new(
                    cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                    "GOSDATA/ASSETS/crimson.rof",
                    Some("ASSETS/SCRIPTS/RESOURCE.H"),
                    0,
                    1,
                    None,
                )
                .expect("a valid span"),
            ),
        )
        .expect("a verified_original provenance names its span"),
    )
    .expect("a fully measured record is valid")
}

/// A measured surface that matches the installation's own declaration.
fn measured_surface(
    rocket_types: u32,
    rocket_slots: u32,
    hardpoint_points: u32,
    nitro_named: bool,
) -> OriginalOrdnanceSurface {
    let id = claim("f28d.test-surface");
    let counts =
        OriginalOrdnanceCounts::try_new(rocket_types, rocket_slots, hardpoint_points, nitro_named)
            .expect("every measured count is nonzero");
    OriginalOrdnanceSurface::new(
        Origin::Installation {
            source: cs_types::asset_id::SourceSpan::new(
                cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                "GOSDATA/ASSETS/crimson.rof",
                Some("ASSETS/SCRIPTS/RESOURCE.H"),
                0,
                1,
                None,
            )
            .expect("a valid span"),
        },
        counts,
        Provenance::new(
            id,
            ClaimStatus::VerifiedOriginal,
            Some(
                cs_types::asset_id::SourceSpan::new(
                    cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                    "GOSDATA/ASSETS/crimson.rof",
                    Some("ASSETS/SCRIPTS/RESOURCE.H"),
                    0,
                    1,
                    None,
                )
                .expect("a valid span"),
            ),
        )
        .expect("a verified_original provenance names its span"),
    )
}

/// The installation as its files declare it: the constants the retail test
/// re-derives.
fn original_surface() -> OriginalOrdnanceSurface {
    measured_surface(
        ORIGINAL_ROCKET_ORDNANCE_TYPES,
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
        true,
    )
}

/// An audit holding the whole synthetic catalogue, laid out for the
/// installation's declared surface.
fn audit_of_fixture() -> OrdnanceAudit {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    for record in declared_catalogue() {
        audit.add(record);
    }
    audit
}

// ------------------------------------------------------- the shipped facts ----

/// The measured constants agree with each other: three rocket blocks, fifteen
/// ids apart, the next block fifteen after the last, and a type count that is
/// **not** the block width.
#[test]
fn accept_f28_d_the_measured_blocks_bound_the_run_without_counting_it() {
    assert_eq!(
        ORIGINAL_ROCKET_NAME_BLOCKS.len(),
        3,
        "the resource header declares three rocket identifier blocks"
    );
    assert_eq!(
        ORIGINAL_ROCKET_NAME_BLOCKS[0].1, "IDS_ROCKETLONGNAME",
        "the first block is the long name"
    );
    assert_eq!(
        ORIGINAL_ROCKET_NAME_BLOCKS[1].1, "IDS_ROCKETSHORTNAME",
        "the second is the short name"
    );
    assert_eq!(
        ORIGINAL_ROCKET_NAME_BLOCKS[2].1, "IDS_ROCKETDESCRIPTION",
        "the third is the description"
    );
    for pair in ORIGINAL_ROCKET_NAME_BLOCKS.windows(2) {
        assert_eq!(
            pair[1].0 - pair[0].0,
            15,
            "the blocks are fifteen ids apart, so the run is bounded at fifteen"
        );
    }
    let last = ORIGINAL_ROCKET_NAME_BLOCKS
        .last()
        .expect("the block list is not empty");
    assert_eq!(
        ORIGINAL_NEXT_ROCKET_BLOCK.0 - last.0,
        15,
        "and the first block after them is fifteen further on"
    );
    assert_eq!(
        ORIGINAL_NEXT_ROCKET_BLOCK.1, "IDS_PAINTLONGNAME",
        "which is the paint block, not another ordnance block"
    );
    assert_eq!(
        ORIGINAL_ROCKET_ORDNANCE_TYPES, 11,
        "the type count comes from the selection screens, not from the fifteen-id \
         block width"
    );
    assert_ne!(
        ORIGINAL_ROCKET_ORDNANCE_TYPES as usize,
        (ORIGINAL_NEXT_ROCKET_BLOCK.0 - ORIGINAL_ROCKET_NAME_BLOCKS[0].0) as usize,
        "a reader who took the block width for the count would say fifteen"
    );
    assert_eq!(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS, 8,
        "eight rocket slots per airframe"
    );
    assert_eq!(
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS, 2,
        "two hardpoint points per airframe"
    );
    assert_eq!(
        ORIGINAL_NITRO_CONTROL,
        ("MPOUT_CHK_NITRO", 10135),
        "the resource header declares the nitro control"
    );
}

/// A measured count of zero cannot be audited against and is refused by name.
#[test]
fn accept_f28_d_an_unauditable_measured_count_is_refused() {
    for (types, slots, points, expected) in [
        (0, 8, 2, OriginalOrdnanceSurfaceError::NoRocketTypes),
        (11, 0, 2, OriginalOrdnanceSurfaceError::NoRocketSlots),
        (11, 8, 0, OriginalOrdnanceSurfaceError::NoHardpointPoints),
    ] {
        assert_eq!(
            OriginalOrdnanceCounts::try_new(types, slots, points, true),
            Err(expected),
            "a zero count is refused"
        );
    }
    let counts = OriginalOrdnanceCounts::try_new(11, 8, 2, false)
        .expect("three nonzero counts and a bool are accepted");
    assert!(
        !counts.nitro_named(),
        "an installation that names no nitro control is a measurement, not an error"
    );
}

// ------------------------------------------------------------ the audit ----

/// The synthetic catalogue is *not* complete, and the report says exactly why.
#[test]
fn accept_f28_d_the_synthetic_catalogue_is_incomplete_by_name() {
    let report = audit_of_fixture().run(&original_surface());
    assert!(
        !report.is_complete(),
        "six designed components cannot be a complete catalogue of eleven \
         unmeasured original types"
    );
    assert_eq!(report.rows().len(), 6, "one row per distinct record");
    assert_eq!(report.declared_rocket_types(), 5, "five launched items");
    assert_eq!(report.declared_boosters(), 1, "one booster");
    assert_eq!(
        report.attributed_rocket_types(),
        0,
        "nothing in the synthetic catalogue is an original measurement"
    );

    let undercounted = report.findings_of("undeclared_rocket_type");
    assert_eq!(
        undercounted.len(),
        1,
        "the count gap is reported once: {:?}",
        report.findings()
    );
    assert_eq!(
        undercounted[0],
        &OrdnanceAuditFinding::UndeclaredRocketType {
            observed: ORIGINAL_ROCKET_ORDNANCE_TYPES,
            declared: 5,
        },
        "the installation offers eleven types and the catalogue enumerates five"
    );

    let unattributed = report.findings_of("unattributed_rocket_type");
    assert_eq!(
        unattributed.len(),
        1,
        "and the attribution gap is reported separately, because a catalogue of \
         eleven invented names would satisfy the count and still name nothing"
    );
    assert_eq!(
        unattributed[0],
        &OrdnanceAuditFinding::UnattributedRocketType {
            observed: ORIGINAL_ROCKET_ORDNANCE_TYPES,
            attributed: 0,
        }
    );

    // Every record is reported as designed content, once each.
    let unmeasured = report.findings_of("unmeasured_record");
    assert_eq!(
        unmeasured.len(),
        6,
        "one finding per record: {:?}",
        unmeasured
    );
    for finding in &unmeasured {
        let OrdnanceAuditFinding::UnmeasuredRecord { class, .. } = finding else {
            panic!("expected an unmeasured_record finding, got {finding:?}");
        };
        assert_eq!(
            *class, "designed",
            "the synthetic fixture is designed content and says so"
        );
    }
    // The two families the sheet's list of leads covers are both used, so no
    // family is reported unused.
    assert!(
        report.findings_of("unused_family").is_empty(),
        "the fixture uses every designed family: {:?}",
        report.findings_of("unused_family")
    );
    // The layout matches the measured surface, so neither layout finding fires.
    assert!(
        report.findings_of("unsupported_rocket_slots").is_empty(),
        "the audit declares the measured eight slots"
    );
    assert!(
        report
            .findings_of("unsupported_hardpoint_points")
            .is_empty(),
        "and the measured two hardpoints"
    );
    // The booster is declared, so its presence is not a gap.
    assert!(
        report.findings_of("missing_nitro_record").is_empty(),
        "the fixture declares a booster"
    );
    // The area-denial record's area is reported as unconsumed, once per field.
    let unconsumed = report.findings_of("unconsumed_field");
    assert_eq!(
        unconsumed.len(),
        DECLARED_FIELDS_WITHOUT_CONSUMER.len(),
        "the one record with an area effect reports both of its unconsumed \
         values: {unconsumed:?}"
    );
    for finding in &unconsumed {
        let OrdnanceAuditFinding::UnconsumedField { ordnance, field } = finding else {
            panic!("expected an unconsumed_field finding, got {finding:?}");
        };
        assert_eq!(
            ordnance.key(),
            DECLARED_SYNTHETIC_AREA_DENIAL_KEY,
            "only the area-denial record declares an area"
        );
        assert!(
            DECLARED_FIELDS_WITHOUT_CONSUMER
                .iter()
                .any(|(declared, _)| *declared == *field),
            "{field} is one of the recorded unconsumed fields"
        );
    }
    // Nothing in the fixture delivers nothing.
    assert!(
        report.findings_of("delivers_no_effect").is_empty(),
        "every fixture component damages or applies a status effect: {:?}",
        report.findings_of("delivers_no_effect")
    );
    // Nothing is an explicit unknown, because the fixture fills every field.
    assert!(
        report.findings_of("unmeasured_field").is_empty(),
        "the fixture declares every field: {:?}",
        report.findings_of("unmeasured_field")
    );
}

/// A catalogue of eleven *measured* records against a surface of eleven is
/// complete — which is what makes the synthetic report's incompleteness
/// meaningful rather than structural.
#[test]
fn accept_f28_d_a_fully_measured_catalogue_is_complete() {
    let id = claim("f28d.test-measured-catalogue");
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    let families = [
        DeclaredOrdnanceFamily::DirectExplosive,
        DeclaredOrdnanceFamily::ProximityFlak,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredOrdnanceFamily::AreaDenialEngine,
        DeclaredOrdnanceFamily::AerialTorpedo,
    ];
    // Eleven measured launched items, spread across the five projectile
    // families, plus the measured booster the installation names.
    for index in 0..ORIGINAL_ROCKET_ORDNANCE_TYPES {
        let family = families[index as usize % families.len()];
        audit.add(measured_record(
            &format!("original.measured_rocket_{index}"),
            family,
            id.clone(),
        ));
    }
    // A fully measured booster, with every declared value measured.
    let booster = declared_synthetic_nitro();
    let DeclaredOrdnanceDetails::Nitro(nitro) = booster.details() else {
        panic!("the nitro fixture is a booster")
    };
    let mut nitro = (**nitro).clone();
    nitro.parameters.capacity_units = measured(id.clone(), 12.0);
    nitro.parameters.consumption_per_s = measured(id.clone(), 3.0);
    nitro.parameters.recovery_per_s = measured(id.clone(), 1.0);
    nitro.parameters.extra_thrust_n = measured(id.clone(), 4200.0);
    nitro.parameters.activation = measured(
        id.clone(),
        cs_content::ordnance::DeclaredNitroActivationRule::WhileHeld,
    );
    nitro.parameters.authority_multiplier = measured(id.clone(), 1.0);
    nitro.media.visual = measured(
        id.clone(),
        nitro
            .media
            .visual
            .known()
            .expect("the fixture's visual is known"),
    );
    nitro.media.sound = measured(
        id.clone(),
        nitro
            .media
            .sound
            .known()
            .expect("the fixture's sound is known"),
    );
    nitro.media.particles = nitro.media.particles.as_ref().map(|particles| {
        measured(
            id.clone(),
            match particles {
                Resolved::Known(known) => known.value.clone(),
                Resolved::Unknown { .. } => panic!("the fixture's particles are known"),
            },
        )
    });
    audit.add(
        DeclaredOrdnance::try_new(
            booster.ordnance().clone(),
            Origin::Installation {
                source: cs_types::asset_id::SourceSpan::new(
                    cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                    "GOSDATA/ASSETS/crimson.rof",
                    Some("ASSETS/SCRIPTS/RESOURCE.H"),
                    0,
                    1,
                    None,
                )
                .expect("a valid span"),
            },
            DeclaredOrdnanceFamily::NitroBooster,
            DeclaredOrdnanceDetails::Nitro(Box::new(nitro)),
            None,
            Provenance::new(
                id.clone(),
                ClaimStatus::VerifiedOriginal,
                Some(
                    cs_types::asset_id::SourceSpan::new(
                        cs_types::evidence::ContentHash::from_bytes([7u8; 32]),
                        "GOSDATA/ASSETS/crimson.rof",
                        Some("ASSETS/SCRIPTS/RESOURCE.H"),
                        0,
                        1,
                        None,
                    )
                    .expect("a valid span"),
                ),
            )
            .expect("a verified_original provenance names its span"),
        )
        .expect("a measured booster is valid"),
    );

    let report = audit.run(&original_surface());
    assert!(
        report.is_complete(),
        "eleven measured launched items, a measured booster and the measured \
         layout are complete: {:?}",
        report.findings()
    );
    assert_eq!(
        report.attributed_rocket_types(),
        ORIGINAL_ROCKET_ORDNANCE_TYPES as usize,
        "all eleven are attributed"
    );
}

/// Dropping the closure check against the measured surface would let a
/// one-component catalogue pass; the audit must not.
#[test]
fn accept_f28_d_a_catalogue_that_under_counts_the_installation_is_incomplete() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(declared_synthetic_direct());
    let report = audit.run(&original_surface());
    assert!(
        !report.is_complete(),
        "one component cannot cover eleven original types: {:?}",
        report.findings()
    );
    assert_eq!(
        report.findings_of("undeclared_rocket_type").len(),
        1,
        "the count gap is named"
    );
    assert_eq!(
        report.findings_of("unattributed_rocket_type").len(),
        1,
        "and so is the attribution gap"
    );
}

/// An empty catalogue is incomplete against every closure check, and reports
/// the family gaps too.
#[test]
fn accept_f28_d_an_empty_catalogue_is_incomplete() {
    let report = OrdnanceAudit::with_layout(0, 0).run(&original_surface());
    assert!(
        report.rows().is_empty(),
        "no rows, because there are no records"
    );
    assert!(
        !report.is_complete(),
        "an empty catalogue covers nothing: {:?}",
        report.findings()
    );
    for label in [
        "undeclared_rocket_type",
        "unattributed_rocket_type",
        "unsupported_rocket_slots",
        "unsupported_hardpoint_points",
        "missing_nitro_record",
    ] {
        assert_eq!(
            report.findings_of(label).len(),
            1,
            "{label} fires against an empty catalogue: {:?}",
            report.findings()
        );
    }
    assert_eq!(
        report.findings_of("unused_family").len(),
        DeclaredOrdnanceFamily::ALL.len(),
        "every designed family is unused"
    );
}

/// A declared layout with fewer slots or hardpoints than the installation's
/// screens build is reported in both directions.
#[test]
fn accept_f28_d_an_unsupported_layout_is_reported() {
    let mut audit = OrdnanceAudit::with_layout(4, 1);
    for record in declared_catalogue() {
        audit.add(record);
    }
    assert_eq!(audit.declared_rocket_slots(), 4, "four slots are declared");
    assert_eq!(
        audit.declared_hardpoint_points(),
        1,
        "one hardpoint is declared"
    );
    let report = audit.run(&original_surface());
    assert_eq!(
        report.findings_of("unsupported_rocket_slots"),
        vec![&OrdnanceAuditFinding::UnsupportedRocketSlots {
            declared: 4,
            observed: ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        }],
        "four declared slots against the measured eight"
    );
    assert_eq!(
        report.findings_of("unsupported_hardpoint_points"),
        vec![&OrdnanceAuditFinding::UnsupportedHardpointPoints {
            declared: 1,
            observed: ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
        }],
        "one declared hardpoint against the measured two"
    );
}

/// A larger declared layout is **not** a gap: the check is a floor, not an
/// equality, so an airframe that offers more than the original's minimum does
/// not fail the audit.
#[test]
fn accept_f28_d_a_larger_declared_layout_is_not_a_gap() {
    let mut audit = OrdnanceAudit::with_layout(64, 16);
    for record in declared_catalogue() {
        audit.add(record);
    }
    let report = audit.run(&original_surface());
    assert!(
        report.findings_of("unsupported_rocket_slots").is_empty(),
        "sixty-four slots covers the measured eight"
    );
    assert!(
        report
            .findings_of("unsupported_hardpoint_points")
            .is_empty(),
        "sixteen hardpoints covers the measured two"
    );
}

/// An installation that names no nitro control makes the booster's presence a
/// choice rather than a gap — and its absence is not reported as one either.
#[test]
fn accept_f28_d_a_missing_nitro_record_is_reported_only_when_nitro_is_named() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(declared_synthetic_direct());
    let with_nitro = audit.run(&original_surface());
    assert_eq!(
        with_nitro.findings_of("missing_nitro_record").len(),
        1,
        "the installation names the nitro control and nothing declares a booster"
    );
    let without_nitro = audit.run(&measured_surface(
        ORIGINAL_ROCKET_ORDNANCE_TYPES,
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
        false,
    ));
    assert!(
        without_nitro.findings_of("missing_nitro_record").is_empty(),
        "an installation with no nitro control has no nitro gap to report"
    );
}

/// An explicit unknown is reported by its declared field name, and the record
/// that carries it would refuse to lower anyway.
#[test]
fn accept_f28_d_an_unmeasured_field_is_reported_by_name() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(flak_with_unknown_trigger_radius());
    let report = audit.run(&original_surface());
    let unknown = report.findings_of("unmeasured_field");
    assert_eq!(
        unknown,
        vec![&OrdnanceAuditFinding::UnmeasuredField {
            ordnance: ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_FLAK_KEY)
                .expect("a weapon id"),
            field: "fuse.trigger_radius_m",
        }],
        "the one unknown is named by its declared field: {unknown:?}"
    );
    let row = report
        .row(
            &ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_FLAK_KEY)
                .expect("a weapon id"),
        )
        .expect("the flak record has a row");
    assert_eq!(row.tally().unknown(), 1, "and the row counts it");
    assert!(
        !row.tally().is_fully_measured(),
        "so the row is not measured"
    );
    assert!(!row.is_measured(), "and the record is not attributed");
}

/// A record that declares neither damage, a status effect nor an area delivers
/// nothing, and the audit says so.
#[test]
fn accept_f28_d_a_record_that_delivers_nothing_is_reported() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(direct_with_zero_damage());
    let report = audit.run(&original_surface());
    let dud = report.findings_of("delivers_no_effect");
    assert_eq!(
        dud,
        vec![&OrdnanceAuditFinding::DeliversNoEffect {
            ordnance: ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_DIRECT_KEY)
                .expect("a weapon id"),
        }],
        "a component with nothing to deliver is named: {dud:?}"
    );
    let row = report
        .row(
            &ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_DIRECT_KEY)
                .expect("a weapon id"),
        )
        .expect("the dud record has a row");
    assert_eq!(row.damage_channels(), 0, "a known zero is a measurement");
    assert_eq!(row.status_effects(), 0, "and it applies no status effect");
    assert!(!row.declares_area(), "and no area");
    assert!(!row.delivers_effect(), "so it delivers nothing");
    assert!(!row.is_booster(), "and it is not a booster either");
}

/// A booster delivers an effect — the boost itself — so it is never reported
/// as delivering nothing.
#[test]
fn accept_f28_d_a_booster_delivers_the_boost() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(declared_synthetic_nitro());
    let report = audit.run(&original_surface());
    assert!(
        report.findings_of("delivers_no_effect").is_empty(),
        "a booster has no damage channel and needs none: {:?}",
        report.findings()
    );
    let row = report
        .row(
            &ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_NITRO_KEY)
                .expect("a weapon id"),
        )
        .expect("the booster has a row");
    assert!(row.is_booster(), "the booster is a booster");
    assert!(row.delivers_effect(), "and it delivers the boost");
    assert_eq!(row.damage_channels(), 0, "it routes no damage channel");
    assert_eq!(row.status_effects(), 0, "and applies no status effect");
}

/// Two records for one id are audited once, and the first in insertion order
/// is the one reported — the disagreement is the importer's business.
#[test]
fn accept_f28_d_a_repeated_record_is_audited_once() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(declared_synthetic_direct());
    audit.add(declared_synthetic_direct());
    audit.add(declared_synthetic_flak());
    assert_eq!(audit.record_count(), 3, "three records were added");
    let report = audit.run(&original_surface());
    assert_eq!(
        report.rows().len(),
        2,
        "two distinct components, so a repeated record cannot inflate the count"
    );
    assert_eq!(
        report.declared_rocket_types(),
        2,
        "and the closure check sees two, not three"
    );
    assert_eq!(
        report.findings_of("unmeasured_record").len(),
        2,
        "each component is reported once"
    );
}

/// A designed family no record uses is named — the only direction in which the
/// sheet's "do not substitute every rocket with one homing missile" is
/// checkable.
#[test]
fn accept_f28_d_an_unused_family_is_named() {
    let mut audit = OrdnanceAudit::with_layout(
        ORIGINAL_ORDNANCE_ROCKET_SLOTS,
        ORIGINAL_ORDNANCE_HARDPOINT_POINTS,
    );
    audit.add(declared_synthetic_direct());
    audit.add(declared_synthetic_guided());
    let report = audit.run(&original_surface());
    let unused: Vec<&str> = report
        .findings_of("unused_family")
        .iter()
        .map(|finding| match finding {
            OrdnanceAuditFinding::UnusedFamily { family } => family.label(),
            other => panic!("expected an unused_family finding, got {other:?}"),
        })
        .collect();
    assert_eq!(
        unused,
        vec![
            DeclaredOrdnanceFamily::ProximityFlak.label(),
            DeclaredOrdnanceFamily::AreaDenialEngine.label(),
            DeclaredOrdnanceFamily::AerialTorpedo.label(),
            DeclaredOrdnanceFamily::NitroBooster.label(),
        ],
        "the four families nothing declares are named, in the designed order: {unused:?}"
    );
}

/// Every finding has a stable label and a message a reader can act on, so the
/// audit's output is usable rather than a pile of variants.
#[test]
fn accept_f28_d_every_finding_names_itself() {
    let report = audit_of_fixture().run(&original_surface());
    for finding in report.findings() {
        let label = finding.label();
        assert!(!label.is_empty(), "{finding:?} has a label");
        assert!(
            !label.contains(' '),
            "{label} is a stable machine-readable token, not a sentence"
        );
        let message = finding.to_string();
        assert!(
            message.len() > label.len(),
            "{label} explains itself: {message}"
        );
    }
    // The distinct label set is pinned: a finding the fixture catalogue is not
    // supposed to produce shows up here, and so does one that stops being
    // produced.
    let mut labels: Vec<&str> = report.findings().iter().map(|f| f.label()).collect();
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(
        labels,
        vec![
            "unattributed_rocket_type",
            "unconsumed_field",
            "undeclared_rocket_type",
            "unmeasured_record",
        ],
        "the fixture catalogue's gaps are exactly these four kinds: {labels:?}"
    );
    // One `unmeasured_record` per record, and one `unconsumed_field` per
    // declared field of the one area record.
    assert_eq!(
        report.findings_of("unmeasured_record").len(),
        report.rows().len(),
        "one per record"
    );
    assert_eq!(
        report.findings_of("unconsumed_field").len(),
        DECLARED_FIELDS_WITHOUT_CONSUMER.len(),
        "one per declared area field"
    );
}

/// The audit walks every declared record and reports its family, its field
/// counts and its provenance — the per-component answer the closure checks sit
/// on.
#[test]
fn accept_f28_d_every_record_gets_a_row_with_its_family_and_tally() {
    let report = audit_of_fixture().run(&original_surface());
    let expected: Vec<(&str, DeclaredOrdnanceFamily)> = vec![
        (
            DECLARED_SYNTHETIC_DIRECT_KEY,
            DeclaredOrdnanceFamily::DirectExplosive,
        ),
        (
            DECLARED_SYNTHETIC_FLAK_KEY,
            DeclaredOrdnanceFamily::ProximityFlak,
        ),
        (
            DECLARED_SYNTHETIC_GUIDED_KEY,
            DeclaredOrdnanceFamily::GuidedRocket,
        ),
        (
            DECLARED_SYNTHETIC_AREA_DENIAL_KEY,
            DeclaredOrdnanceFamily::AreaDenialEngine,
        ),
        (
            DECLARED_SYNTHETIC_TORPEDO_KEY,
            DeclaredOrdnanceFamily::AerialTorpedo,
        ),
        (
            DECLARED_SYNTHETIC_NITRO_KEY,
            DeclaredOrdnanceFamily::NitroBooster,
        ),
    ];
    assert_eq!(report.rows().len(), expected.len(), "one row per record");
    for (key, family) in expected {
        let id = ContentId::from_source(ContentKind::Weapon, key).expect("a weapon id");
        let row = report.row(&id).unwrap_or_else(|| panic!("{key} has a row"));
        assert_eq!(row.family(), family, "{key} keeps its declared family");
        assert!(row.tally().total() > 5, "{key} declares a real record");
        assert_eq!(
            row.tally().verified(),
            0,
            "{key} has no original measurement in it"
        );
        assert!(!row.is_measured(), "{key} is not attributed");
        assert_eq!(
            row.provenance().class,
            ClaimStatus::Designed,
            "{key} carries designed provenance"
        );
        assert_eq!(
            row.origin(),
            &Origin::SyntheticFixture,
            "{key} came from the synthetic fixture"
        );
    }
}

// ------------------------------------------------------------------ helpers ----

/// The proximity-flak fixture with its trigger radius left explicitly unknown.
fn flak_with_unknown_trigger_radius() -> DeclaredOrdnance {
    let fixture = declared_synthetic_flak();
    let DeclaredOrdnanceDetails::Projectile(projectile) = fixture.details() else {
        panic!("the flak fixture is a projectile")
    };
    let mut projectile = (**projectile).clone();
    projectile.fuse = DeclaredFuseRule::Proximity(cs_content::ordnance::DeclaredProximityFuse {
        trigger_radius_m: declared_unknown("the original's fuse radius is unmeasured"),
    });
    DeclaredOrdnance::try_new(
        fixture.ordnance().clone(),
        Origin::SyntheticFixture,
        fixture.family(),
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("an unknown fuse radius is a legal record")
}

/// The direct-explosive fixture with both damage channels declared as a known
/// zero.
fn direct_with_zero_damage() -> DeclaredOrdnance {
    let fixture = declared_synthetic_direct();
    let DeclaredOrdnanceDetails::Projectile(projectile) = fixture.details() else {
        panic!("the direct-explosive fixture is a projectile")
    };
    let mut projectile = (**projectile).clone();
    projectile.armor_damage = cs_content::ordnance::declared_known(0.0);
    projectile.internal_damage = cs_content::ordnance::declared_known(0.0);
    DeclaredOrdnance::try_new(
        fixture.ordnance().clone(),
        Origin::SyntheticFixture,
        fixture.family(),
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("a zero-damage record is valid: zero is a measurement, not a gap")
}
