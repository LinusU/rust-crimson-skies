//! Acceptance scenario F14-A through the catalog: stable ids, the declared
//! launchable baseline, the unsupported-mission count (AC01), deterministic
//! order (AC02) and the synthetic-versus-retail distinction (AC04).
//!
//! These tests exercise production code only: `cs_content::catalog::Catalog`
//! over the typed schema in `cs_types::content`. Removing or neutering the
//! catalog's duplicate refusal, its baseline accounting or the element
//! validation makes them fail.

use cs_content::catalog::{Catalog, CatalogError};
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind, ElementError,
    NormalizeState, Origin, Provenance, Readiness, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::ClaimId;
use cs_types::install::ParseState;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

/// A ready element of `element_id`'s kind, depending on `deps`.
fn ready(element_id: &ContentId, deps: &[&ContentId]) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id.clone(),
        display_name: Some(format!("Authored {element_id}")),
        origin: Origin::SyntheticFixture,
        dependencies: deps
            .iter()
            .map(|target| Dependency {
                target: (*target).clone(),
                kind: DependencyKind::Static,
                provenance: designed("f14.test.dependency"),
            })
            .collect(),
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f14.test.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    }
}

/// A launchable mission whose readiness is `is_ready`; an unsupported one
/// carries the one required reason.
fn mission(key: &str, is_ready: bool) -> CatalogElement {
    let mut element = ready_element(&cid(ContentKind::Mission, key), is_ready);
    element.display_name = Some(format!("Authored Mission {key}"));
    element
}

fn ready_element(element_id: &ContentId, is_ready: bool) -> CatalogElement {
    let element = ready(element_id, &[]);
    if is_ready {
        element
    } else {
        CatalogElement {
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::MissingParser],
            ..element
        }
    }
}

/// AC01 — the minimum scenario: adding an unsupported mission increases the
/// unsupported count and prevents full readiness, and the unsupported
/// mission is never filtered out of the denominator.
#[test]
fn accept_f14_a_adding_unsupported_mission_increases_unsupported_count() {
    let m01 = cid(ContentKind::Mission, "m01");
    let m02 = cid(ContentKind::Mission, "m02");

    let mut catalog = Catalog::new();
    catalog
        .insert(ready(&m01, &[]))
        .expect("the mission inserts");
    catalog
        .declare_launchable(&m01)
        .expect("the mission is launchable");
    assert_eq!(catalog.unsupported_count(), 0);
    assert!(catalog.is_fully_ready());

    catalog
        .insert(mission("m02", false))
        .expect("the unsupported mission still inserts");
    catalog
        .declare_launchable(&m02)
        .expect("the unsupported mission is declared as well");
    assert_eq!(
        catalog.unsupported_count(),
        1,
        "the unsupported mission joins the denominator instead of being filtered out"
    );
    assert!(
        !catalog.is_fully_ready(),
        "an unsupported launchable mission prevents full readiness"
    );

    let unsupported = catalog.unsupported_launchables();
    assert_eq!(unsupported.len(), 1);
    assert_eq!(unsupported[0].id, m02);
    assert_eq!(unsupported[0].unsupported_codes(), vec!["missing_parser"]);
    assert_eq!(
        catalog.launchable_count(),
        2,
        "the denominator is the declared baseline, not the supported subset"
    );
}

/// AC04 — a synthetic launchable row is counted separately and is never a
/// retail catalog entry, however ready it looks.
#[test]
fn accept_f14_a_synthetic_launchable_row_is_not_a_retail_entry() {
    let mission_id = cid(ContentKind::Mission, "m01");

    let mut catalog = Catalog::new();
    catalog
        .insert(ready(&mission_id, &[]))
        .expect("the synthetic mission inserts");
    catalog
        .declare_launchable(&mission_id)
        .expect("the synthetic mission is launchable");

    assert!(catalog.is_fully_ready(), "the synthetic mission is ready");
    assert_eq!(catalog.launchable_count(), 1);
    assert_eq!(catalog.original_launchable_count(), 0);
    assert_eq!(catalog.synthetic_launchable_count(), 1);
    assert!(
        !catalog.is_retail_ready(),
        "a fully ready synthetic catalog is still not retail-ready"
    );
    assert!(
        !catalog
            .get(&mission_id)
            .expect("present")
            .origin
            .is_original()
    );
}

/// AC02 (catalog half) — ids and enumeration order derive from identity, not
/// from the order elements happen to be inserted in.
#[test]
fn accept_f14_a_ids_and_order_are_stable_under_reordering() {
    // The same rows, in three different input enumerations.
    let forward = vec![
        ready(&cid(ContentKind::World, "c1"), &[]),
        ready(&cid(ContentKind::Airframe, "scout"), &[]),
        ready(&cid(ContentKind::Image, "body"), &[]),
        mission("m01", true),
        mission("m02", false),
    ];
    let reversed: Vec<CatalogElement> = forward.iter().rev().cloned().collect();
    let shuffled = vec![
        forward[3].clone(),
        forward[0].clone(),
        forward[4].clone(),
        forward[2].clone(),
        forward[1].clone(),
    ];

    let build = |rows: &[CatalogElement]| {
        let mut catalog = Catalog::new();
        for row in rows {
            catalog.insert(row.clone()).expect("row inserts");
        }
        for key in ["m01", "m02"] {
            catalog
                .declare_launchable(&cid(ContentKind::Mission, key))
                .expect("mission is launchable");
        }
        catalog
    };

    let a = build(&forward);
    let b = build(&reversed);
    let c = build(&shuffled);

    let expected: Vec<&str> = {
        let mut ids: Vec<&str> = a.sorted_ids().iter().map(|id| id.as_str()).collect();
        ids.sort_unstable();
        ids
    };
    for catalog in [&b, &c] {
        let ids: Vec<&str> = catalog.sorted_ids().iter().map(|id| id.as_str()).collect();
        assert_eq!(
            ids, expected,
            "the canonical id order is the same for every input enumeration"
        );
        assert_eq!(catalog.unsupported_count(), a.unsupported_count());
        assert_eq!(catalog.is_fully_ready(), a.is_fully_ready());
    }
    assert_eq!(a, b, "insertion order does not change the catalog");
    assert_eq!(b, c, "insertion order does not change the catalog");
}

/// Non-negotiable behavior 5: a duplicate identity is refused, never merged
/// or silently overwritten.
#[test]
fn accept_f14_a_duplicate_identities_are_refused() {
    let mission_id = cid(ContentKind::Mission, "m01");
    let mut catalog = Catalog::new();
    catalog
        .insert(ready(&mission_id, &[]))
        .expect("inserts once");
    assert_eq!(
        catalog.insert(mission("m01", false)),
        Err(CatalogError::DuplicateId {
            id: mission_id.clone()
        })
    );
    assert_eq!(catalog.len(), 1, "the refused insert changed nothing");
    assert!(
        catalog.get(&mission_id).expect("present").is_ready(),
        "the first row is not overwritten by the duplicate"
    );
}

/// A launchable declaration must name an inserted, launchable row.
#[test]
fn accept_f14_a_launchable_baseline_requires_a_launchable_element() {
    let mut catalog = Catalog::new();
    assert_eq!(
        catalog.declare_launchable(&cid(ContentKind::Mission, "m01")),
        Err(CatalogError::UnknownElement {
            id: cid(ContentKind::Mission, "m01")
        })
    );

    let world = cid(ContentKind::World, "c1");
    catalog.insert(ready(&world, &[])).expect("world inserts");
    assert_eq!(
        catalog.declare_launchable(&world),
        Err(CatalogError::NotLaunchable {
            id: world.clone(),
            kind: ContentKind::World
        })
    );
    assert_eq!(catalog.launchable_count(), 0);
}

/// The catalog validates what it is given: an unavailable element with no
/// reason is refused at insertion, so a silent "unavailable" can never enter
/// the collection.
#[test]
fn accept_f14_a_catalog_refuses_an_invalid_element() {
    let mut catalog = Catalog::new();
    let mission_id = cid(ContentKind::Mission, "m01");
    let invalid = CatalogElement {
        readiness: Readiness::Unavailable,
        unsupported_reasons: Vec::new(),
        ..ready(&mission_id, &[])
    };
    assert_eq!(
        catalog.insert(invalid),
        Err(CatalogError::Element(
            ElementError::UnavailableWithoutReason
        ))
    );
    assert!(catalog.is_empty());
}
