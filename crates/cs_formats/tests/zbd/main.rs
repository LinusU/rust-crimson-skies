//! Acceptance stage F06-A: ZBD family inventory and two-key dispatch
//! (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
//! section `### F06-A`, AC01 plus the failure cases the scenario implies).
//!
//! Every byte in this file is authored here: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access.
//!
//! The fixture is deliberately minimal — two headers and a handful of
//! installation paths — because this stage decides *which reader a
//! container is routed to*, not what a reader parses (F06-B does that).
//! The second header is important: it is plain authored content that
//! matches no documented signature, so where a role routes it to a family
//! without a header rule dispatch has to say so (`HeaderStatus::Unvalidated`)
//! instead of pretending it checked them. Task #340 documented the GameZ and
//! animation signatures, so the AC01 pair is now two documented headers.

mod readers;
mod t340;
mod t343;
mod t344;

use cs_formats::zbd::{
    CONTENT_ROOT, DispatchBasis, GAMEZ_SIGNATURE, GAMEZ_VERSION, HeaderStatus, INTERP_SIGNATURE,
    INTERP_VERSION, INTERP_VERSION_OFFSET, OUTSIDE_CONTENT_ROOT, RoleStatus, UNOBSERVED_NAME,
    ZBD_FAMILY_INVENTORY, ZbdDispatch, ZbdDispatchError, ZbdFamily, ZbdProbe, ZbdReaderId, ZbdRole,
    dispatch, family_record, role_for_path,
};
use cs_types::evidence::ClaimStatus;
use cs_types::install::RelativePath;

/// Provenance label carried by every result and error these tests assert on.
const CONTAINER: &str = "synthetic/f06_a_dispatch.zbd";

/// The documented INTERP header: signature, version 7, script count — the
/// 12 bytes `docs/research/FORMAT-NOTES.md` ("INTERP observed subset") and
/// `specs/F07-interp-loading-script-container.md` record.
fn interp_header(script_count: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&INTERP_SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&INTERP_VERSION.to_le_bytes());
    bytes.extend_from_slice(&script_count.to_le_bytes());
    bytes
}

/// The documented INTERP header with a version word the pack does not
/// document: bytes 4..8 are little-endian `version`.
fn interp_header_with_version(version: u32) -> Vec<u8> {
    let mut bytes = interp_header(0);
    bytes[4..8].copy_from_slice(&version.to_le_bytes());
    bytes
}

/// The GameZ header words task #340 read from the pinned mech3ax v0.6.0
/// source: signature `0x02971222`, Crimson Skies version 42, then an
/// authored field.
fn gamez_header() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&GAMEZ_SIGNATURE.to_le_bytes());
    bytes.extend_from_slice(&GAMEZ_VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

/// Authored bytes that begin with no documented signature.
///
/// Nothing here claims these bytes are any real family's layout — three of
/// the six families have no header signature (task #340 findings), so
/// dispatch must route such a container on its observed role and record the
/// header as unvalidated.
fn other_header() -> Vec<u8> {
    vec![
        0x5A, 0x01, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x7A, 0x02, 0x00, 0x00, 0x00,
        0x00,
    ]
}

/// A fixture installation path, validated by the production type.
fn path(spelling: &str) -> RelativePath {
    RelativePath::new(spelling).expect("fixture spellings are valid relative paths")
}

/// Dispatches `header` at the installation path `path`, with the fixture
/// container label.
fn dispatch_at<'probe>(
    path: &'probe RelativePath,
    header: &'probe [u8],
) -> Result<ZbdDispatch<'probe>, ZbdDispatchError> {
    dispatch(ZbdProbe::new(CONTAINER, path, header))
}

// --- AC01: two distinct headers, two different readers ----------------------

#[test]
fn accept_f06_a_two_distinct_headers_route_to_different_readers() {
    // Header one: the documented INTERP header at its observed location.
    let interp_path = path("zbd/interp.zbd");
    let interp = interp_header(0);
    let first = dispatch_at(&interp_path, &interp)
        .expect("a documented signature at an observed role name dispatches");

    // Header two: the documented GameZ header at the observed location of a
    // different family.
    let planes_path = path("zbd/planes.zbd");
    let other = gamez_header();
    let second = dispatch_at(&planes_path, &other)
        .expect("the documented GameZ header at its observed role name dispatches");

    // Two distinct synthetic ZBD-family headers…
    assert_ne!(interp, other, "the two fixtures must be distinct headers");
    assert_ne!(
        first.header_bytes(),
        second.header_bytes(),
        "the dispatched probes are the two authored headers"
    );

    // …routed to different readers.
    assert_ne!(
        first.reader(),
        second.reader(),
        "AC01: two distinct headers must reach different readers"
    );
    assert_ne!(first.family(), second.family());

    assert_eq!(first.family(), ZbdFamily::Interp);
    assert_eq!(first.reader(), ZbdReaderId::Interp);
    assert_eq!(first.basis(), DispatchBasis::HeaderAndRole);
    assert!(matches!(
        first.header_status(),
        HeaderStatus::Validated {
            signature: INTERP_SIGNATURE,
            version: INTERP_VERSION,
        }
    ));

    assert_eq!(second.family(), ZbdFamily::GameZ);
    assert_eq!(second.reader(), ZbdReaderId::GameZ);
    assert_eq!(second.basis(), DispatchBasis::HeaderAndRole);
    assert!(matches!(
        second.header_status(),
        HeaderStatus::Validated {
            signature: GAMEZ_SIGNATURE,
            version: GAMEZ_VERSION,
        }
    ));

    // Both decisions round-trip through the inventory rows they name.
    assert_eq!(first.record(), family_record(ZbdFamily::Interp));
    assert_eq!(second.record(), family_record(ZbdFamily::GameZ));
}

#[test]
fn accept_f06_a_role_only_dispatch_records_an_unvalidated_header() {
    // The observed world-group spelling, uppercased: role rules compare the
    // logical key, and the original spelling stays with the caller.
    let texture_path = path("ZBD/C1/TEXTURE.ZBD");
    let header = other_header();
    let decided = dispatch_at(&texture_path, &header)
        .expect("an observed archive name dispatches without a documented header");

    assert_eq!(decided.family(), ZbdFamily::Texture);
    assert_eq!(decided.reader(), ZbdReaderId::Texture);
    assert_eq!(decided.basis(), DispatchBasis::RoleOnly);
    assert_eq!(decided.path(), &texture_path);
    assert_eq!(decided.container(), CONTAINER);
    assert_eq!(decided.header_bytes(), header.as_slice());
    assert_eq!(decided.file_family().as_str(), "zbd.texture");

    // The bytes were not validated, and the result says so explicitly.
    let expected_reason = family_record(ZbdFamily::Texture)
        .header_rule()
        .undocumented_reason()
        .expect("the texture family has no header signature");
    match decided.header_status() {
        HeaderStatus::Unvalidated { reason } => {
            assert!(!reason.is_empty(), "an unvalidated header carries why");
            assert_eq!(reason, expected_reason);
        }
        HeaderStatus::Validated { .. } => {
            panic!("the texture header has no signature; dispatch must not claim validation")
        }
    }

    // The role half is evidence-labelled, not asserted as original fact.
    match decided.role_status() {
        RoleStatus::Observed { rule } => {
            assert_eq!(rule.evidence(), ClaimStatus::Documented);
            assert_eq!(rule.pattern().to_string(), r#"basename == "texture.zbd""#);
            assert!(!rule.source().is_empty());
        }
        RoleStatus::Unrecognized { reason } => {
            panic!("`texture.zbd` is an observed archive name, got {reason}")
        }
    }
}

#[test]
fn accept_f06_a_documented_header_without_a_known_role_routes_to_the_interp_reader() {
    let header = interp_header(0);

    // Outside the observed content root: the header alone decides.
    let outside_path = path("docs/notes.zbd");
    let decided = dispatch_at(&outside_path, &header)
        .expect("a documented signature dispatches even where no role rule applies");
    assert_eq!(decided.family(), ZbdFamily::Interp);
    assert_eq!(decided.reader(), ZbdReaderId::Interp);
    assert_eq!(decided.basis(), DispatchBasis::HeaderOnly);
    assert!(matches!(
        decided.header_status(),
        HeaderStatus::Validated { .. }
    ));
    assert_eq!(
        decided.role_status(),
        RoleStatus::Unrecognized {
            reason: OUTSIDE_CONTENT_ROOT
        }
    );

    // Inside the content root, under a basename no rule observes: same
    // decision, different recorded reason.
    let unknown_name_path = path("zbd/mystery.zbd");
    let decided = dispatch_at(&unknown_name_path, &header)
        .expect("a documented signature dispatches where the basename is unknown");
    assert_eq!(decided.basis(), DispatchBasis::HeaderOnly);
    assert_eq!(
        decided.role_status(),
        RoleStatus::Unrecognized {
            reason: UNOBSERVED_NAME
        }
    );
}

// --- Failure cases: explicit refusals, never a silent fallback -------------

#[test]
fn accept_f06_a_header_and_role_disagree_fails_instead_of_falling_back() {
    // An INTERP header where the observed role names the GameZ family.
    let planes_path = path("zbd/planes.zbd");
    let interp = interp_header(0);
    let error = dispatch_at(&planes_path, &interp)
        .expect_err("a documented header contradicting its role must not dispatch");

    assert_eq!(error.code(), "header_role_conflict");
    assert_eq!(error.container(), CONTAINER);
    match &error {
        ZbdDispatchError::HeaderRoleConflict {
            header_family,
            role_family,
            signature,
            role_rule,
            ..
        } => {
            assert_eq!(*header_family, ZbdFamily::Interp);
            assert_eq!(*role_family, ZbdFamily::GameZ);
            assert_eq!(*signature, INTERP_SIGNATURE);
            assert_eq!(
                role_rule.pattern().to_string(),
                r#"basename == "planes.zbd""#
            );
        }
        other => panic!("expected `header_role_conflict`, got {other:?}"),
    }
    // The failure names both families instead of quietly picking one.
    let message = error.to_string();
    assert!(message.contains("interp"), "message: {message}");
    assert!(message.contains("gamez"), "message: {message}");

    // The mirror: an observed `interp.zbd` whose bytes carry no documented
    // signature fails on its own rule rather than parsing as something else.
    let interp_path = path("zbd/interp.zbd");
    let error = dispatch_at(&interp_path, &other_header())
        .expect_err("an observed interp archive must carry the interp signature");
    assert_eq!(error.code(), "header_mismatch");
}

#[test]
fn accept_f06_a_role_that_names_a_documented_family_requires_its_signature() {
    let interp_path = path("zbd/interp.zbd");

    // (a) Enough bytes, wrong signature.
    let error = dispatch_at(&interp_path, &other_header())
        .expect_err("the observed role demands the documented signature");
    assert_eq!(error.code(), "header_mismatch");
    assert_eq!(error.container(), CONTAINER);
    match &error {
        ZbdDispatchError::HeaderMismatch {
            family,
            expected_signature,
            available,
            role_rule,
            ..
        } => {
            assert_eq!(*family, ZbdFamily::Interp);
            assert_eq!(*expected_signature, INTERP_SIGNATURE);
            assert_eq!(*available, other_header().len());
            assert_eq!(role_rule.evidence(), ClaimStatus::Documented);
        }
        other => panic!("expected `header_mismatch`, got {other:?}"),
    }

    // (b) Too few bytes to evaluate the documented rule at all: dispatch
    //     refuses instead of treating "unreadable" as "validated".
    let signature_bytes = INTERP_SIGNATURE.to_le_bytes();
    let truncated = &signature_bytes[..3];
    let error = dispatch_at(&interp_path, truncated)
        .expect_err("a probe too short for the documented rule must not dispatch");
    assert_eq!(error.code(), "header_too_short");
    match &error {
        ZbdDispatchError::HeaderTooShort {
            family,
            needed,
            available,
            ..
        } => {
            assert_eq!(*family, ZbdFamily::Interp);
            assert_eq!(*needed, INTERP_VERSION_OFFSET + 4);
            assert_eq!(*available, 3);
        }
        other => panic!("expected `header_too_short`, got {other:?}"),
    }
}

#[test]
fn accept_f06_a_unsupported_header_version_fails_explicitly() {
    let header = interp_header_with_version(INTERP_VERSION + 1);

    // With a matching observed role: the family is right, the version is not.
    let interp_path = path("zbd/interp.zbd");
    let error = dispatch_at(&interp_path, &header)
        .expect_err("an undocumented version must not dispatch as a supported one");
    assert_eq!(error.code(), "unsupported_header_version");
    match &error {
        ZbdDispatchError::UnsupportedHeaderVersion {
            family,
            observed,
            supported,
            source,
            ..
        } => {
            assert_eq!(*family, ZbdFamily::Interp);
            assert_eq!(*observed, INTERP_VERSION + 1);
            assert_eq!(*supported, INTERP_VERSION);
            assert!(
                !source.is_empty(),
                "the version cites where it was documented"
            );
        }
        other => panic!("expected `unsupported_header_version`, got {other:?}"),
    }

    // Without any role: the documented version still gates the decision.
    let unknown_path = path("zbd/mystery.zbd");
    let error = dispatch_at(&unknown_path, &header)
        .expect_err("an undocumented version must not dispatch from the header alone");
    assert_eq!(error.code(), "unsupported_header_version");
}

#[test]
fn accept_f06_a_unknown_header_and_unknown_role_are_rejected() {
    let header = other_header();

    // Inside `zbd/`, but no observed archive name and no documented
    // signature: there is nothing to route this container by.
    let unknown_path = path("zbd/mystery.zbd");
    let error =
        dispatch_at(&unknown_path, &header).expect_err("two unknown keys must dispatch nothing");
    assert_eq!(error.code(), "unknown_family");
    assert_eq!(error.container(), CONTAINER);
    match &error {
        ZbdDispatchError::UnknownFamily { reason, .. } => assert_eq!(*reason, UNOBSERVED_NAME),
        other => panic!("expected `unknown_family`, got {other:?}"),
    }

    // Outside the observed content root: same refusal, other reason.
    let foreign_path = path("gosdata/assets/crimson.rof");
    let error = dispatch_at(&foreign_path, &header)
        .expect_err("a path outside the content root dispatches nothing without a signature");
    assert_eq!(error.code(), "unknown_family");
    match &error {
        ZbdDispatchError::UnknownFamily { reason, .. } => assert_eq!(*reason, OUTSIDE_CONTENT_ROOT),
        other => panic!("expected `unknown_family`, got {other:?}"),
    }
}

#[test]
fn accept_f06_a_dispatch_failures_carry_a_container_and_a_stable_code() {
    let interp_path = path("zbd/interp.zbd");
    let planes_path = path("zbd/planes.zbd");
    let mystery_path = path("zbd/mystery.zbd");

    let signature_bytes = INTERP_SIGNATURE.to_le_bytes();
    let truncated = &signature_bytes[..3];
    let wrong_version = interp_header_with_version(9);
    let unknown = other_header();
    let contradicting = interp_header(0);

    let failures = vec![
        (dispatch_at(&interp_path, truncated), "header_too_short"),
        (dispatch_at(&interp_path, &unknown), "header_mismatch"),
        (
            dispatch_at(&interp_path, &wrong_version),
            "unsupported_header_version",
        ),
        (
            dispatch_at(&planes_path, &contradicting),
            "header_role_conflict",
        ),
        (dispatch_at(&mystery_path, &unknown), "unknown_family"),
    ];

    let mut codes = Vec::new();
    for (result, expected) in failures {
        let error =
            result.expect_err("every fixture in this table is a dispatch failure by design");
        assert_eq!(error.code(), expected);
        assert_eq!(error.container(), CONTAINER, "errors name their container");
        let message = error.to_string();
        assert!(
            message.contains(CONTAINER),
            "display names it too: {message}"
        );
        assert!(
            !message.contains("0x19 0x11"),
            "no probe bytes are echoed: {message}"
        );
        codes.push(error.code());
    }
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes.len(),
        failures_len(),
        "every failure kind has its own machine code"
    );
}

/// Number of rows in the failure table above (kept separate so the table
/// and the uniqueness assertion cannot drift apart silently).
fn failures_len() -> usize {
    5
}

// --- The inventory itself ---------------------------------------------------

#[test]
fn accept_f06_a_role_inventory_covers_every_observed_archive_name() {
    // Every archive name committed evidence records, with the family it
    // routes to and how honestly that assignment is labelled.
    let observed: &[(&str, ZbdFamily, ClaimStatus)] = &[
        ("zbd/interp.zbd", ZbdFamily::Interp, ClaimStatus::Documented),
        ("ZBD/PLANES.ZBD", ZbdFamily::GameZ, ClaimStatus::Documented),
        (
            "zbd/c1/gamez.zbd",
            ZbdFamily::GameZ,
            ClaimStatus::Documented,
        ),
        (
            "zbd/c1/texture.zbd",
            ZbdFamily::Texture,
            ClaimStatus::Documented,
        ),
        (
            "zbd/c1/rtexture15.zbd",
            ZbdFamily::Texture,
            ClaimStatus::Documented,
        ),
        (
            "zbd/c2b/rtexture9.zbd",
            ZbdFamily::Texture,
            ClaimStatus::Documented,
        ),
        (
            "zbd/rimage.zbd",
            ZbdFamily::Texture,
            ClaimStatus::Documented,
        ),
        ("zbd/zrdr.zbd", ZbdFamily::Reader, ClaimStatus::Documented),
        (
            "zbd/c1/zrdr.zbd",
            ZbdFamily::Reader,
            ClaimStatus::Documented,
        ),
        (
            "zbd/c1/cam_anim.zbd",
            ZbdFamily::Animation,
            ClaimStatus::Documented,
        ),
        (
            "zbd/c1/m02/mis_anim.zbd",
            ZbdFamily::Animation,
            ClaimStatus::Documented,
        ),
        (
            "zbd/c1/ia1/zrdr.zbd",
            ZbdFamily::Reader,
            ClaimStatus::Documented,
        ),
        ("ZBD/soundsl.zbd", ZbdFamily::Sound, ClaimStatus::Documented),
        ("zbd/soundsh.zbd", ZbdFamily::Sound, ClaimStatus::Documented),
    ];

    for (spelling, expected_family, expected_evidence) in observed {
        match role_for_path(&path(spelling)) {
            ZbdRole::Observed { family, rule } => {
                assert_eq!(
                    family, *expected_family,
                    "{spelling} routes to the wrong family"
                );
                assert_eq!(
                    rule.evidence(),
                    *expected_evidence,
                    "{spelling} has the wrong evidence class"
                );
                assert!(
                    !rule.source().is_empty(),
                    "{spelling} cites its observation"
                );
            }
            ZbdRole::Unrecognized { reason } => {
                panic!("{spelling} matched no observed role rule ({reason})")
            }
        }
    }

    // Negatives: an unobserved basename and a path outside the content root
    // both stay unrecognized, with their distinct reasons.
    assert_eq!(
        role_for_path(&path("zbd/notes.txt")),
        ZbdRole::Unrecognized {
            reason: UNOBSERVED_NAME
        }
    );
    assert_eq!(
        role_for_path(&path("gosdata/assets/crimson.rof")),
        ZbdRole::Unrecognized {
            reason: OUTSIDE_CONTENT_ROOT
        }
    );
    assert_eq!(CONTENT_ROOT, "zbd/");
}

#[test]
fn accept_f06_a_family_inventory_declares_one_reader_per_family() {
    assert_eq!(ZBD_FAMILY_INVENTORY.len(), ZbdFamily::ALL.len());

    let mut readers = Vec::new();
    for family in ZbdFamily::ALL {
        let record = family_record(family);
        assert_eq!(record.family(), family);
        assert_eq!(record.reader(), family.reader());
        assert_eq!(record.reader().family(), family);
        assert!(!record.source().is_empty(), "{family:?} cites its evidence");

        // The F02 open family vocabulary accepts every label we hand out.
        assert_eq!(
            record.family().file_family().as_str(),
            record.family().file_family_label()
        );

        // Evidence classes are honest: a signature rule cites where it was
        // documented (or, for a version no source states, where it was
        // observed), an undocumented layout says so and is `unknown`.
        match record.header_rule().signature() {
            Some(rule) => assert!(
                matches!(
                    record.header_rule().evidence(),
                    ClaimStatus::Documented | ClaimStatus::ObservedTool
                ),
                "{family:?} cites {source}",
                source = rule.source()
            ),
            None => {
                assert_eq!(record.header_rule().evidence(), ClaimStatus::Unknown);
                assert!(
                    record
                        .header_rule()
                        .undocumented_reason()
                        .is_some_and(|reason| !reason.is_empty()),
                    "{family:?} records why its header is unknown"
                );
            }
        }

        readers.push(record.reader());
    }

    // Two families never share a reader slot: the routing target is the
    // family's own reader, so "different readers" is a real distinction.
    for (index, reader) in readers.iter().enumerate() {
        for other in &readers[index + 1..] {
            assert_ne!(reader, other, "reader slots must be unique per family");
        }
    }

    // Every row of the static inventory is a row for a known family, and
    // every role rule it carries claims only evidence a source can support
    // (`verified_original` would be a lie: an agent never awards it).
    let mut rows = 0;
    for record in ZBD_FAMILY_INVENTORY.iter() {
        assert!(ZbdFamily::ALL.contains(&record.family()));
        rows += 1;
        for rule in record.role_rules() {
            assert!(matches!(
                rule.evidence(),
                ClaimStatus::Documented | ClaimStatus::Inferred
            ));
            assert!(!rule.source().is_empty());
        }
    }
    assert_eq!(rows, ZBD_FAMILY_INVENTORY.len());

    // Since task #340 every family owns at least one observed role rule —
    // the sound family's is `ZBD/sounds*.zbd` — so dispatch can reach every
    // reader slot.
    for record in ZBD_FAMILY_INVENTORY.iter() {
        assert!(
            !record.role_rules().is_empty(),
            "{:?} has no observed archive name",
            record.family()
        );
    }
}

#[test]
fn accept_f06_a_role_rules_apply_only_at_their_observed_level() {
    // Each name was observed at one directory level (F02-C/F02-D findings);
    // the same basename anywhere else is not evidence of the same family.
    let misplaced = [
        "zbd/c1/planes.zbd",
        "zbd/c1/m02/interp.zbd",
        "zbd/gamez.zbd",
        "zbd/c1/m02/texture.zbd",
        "zbd/rtexture2.zbd",
        "zbd/c1/mis_anim.zbd",
        "zbd/c1/m02/cam_anim.zbd",
        "zbd/c1/soundsl.zbd",
        "zbd/c1/rimage.zbd",
        "zbd/c1/m02/extra/zrdr.zbd",
    ];
    for spelling in misplaced {
        assert_eq!(
            role_for_path(&path(spelling)),
            ZbdRole::Unrecognized {
                reason: UNOBSERVED_NAME
            },
            "{spelling} is not at a level its name was observed at"
        );
    }

    // So a misplaced `planes.zbd` is not routed to GameZ on its name alone:
    // without a documented signature it dispatches nothing.
    let error = dispatch_at(&path("zbd/c1/planes.zbd"), &other_header())
        .expect_err("a name outside its observed level is no role evidence");
    assert_eq!(error.code(), "unknown_family");

    // Every rule lists at least one level, and each observed spelling in
    // `role_inventory_covers_every_observed_archive_name` sits at one of them.
    for record in ZBD_FAMILY_INVENTORY.iter() {
        for rule in record.role_rules() {
            assert!(!rule.levels().is_empty(), "{} has no level", rule.pattern());
        }
    }
}
