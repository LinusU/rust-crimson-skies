//! The synthetic content-catalog fixture (F14-A).
//!
//! [`synthetic_catalog_fixture`] builds a small authored catalog through the
//! canonical [`Catalog`] constructor: ready and unsupported elements, a
//! ready launchable mission and an unsupported one, and a non-launchable
//! unsupported resource. Every row is `Origin::SyntheticFixture`, so the
//! fixture proves nothing about a retail installation and can never be
//! mistaken for a retail catalog entry (spec F14 AC04). It exists so tests
//! and the later `catalog` command (F14-C) have a real, validated input
//! without touching the owner's installation at `$CS_GAME_DIR`.
//!
//! Naming, ids, digests and reasons here are newly authored fixture data,
//! not original content.

use cs_content::catalog::Catalog;
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::ClaimId;
use cs_types::install::ParseState;

/// A validated fixture claim id.
fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("synthetic fixture claim id is valid")
}

/// Designed provenance for one fixture claim.
fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

/// A validated synthetic element id.
fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("synthetic fixture id is valid")
}

/// A ready synthetic element of `id`'s kind, depending on `deps`.
fn ready_element(element_id: ContentId, display: &str, deps: &[&ContentId]) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id,
        display_name: Some(display.to_owned()),
        origin: Origin::SyntheticFixture,
        dependencies: deps
            .iter()
            .map(|target| Dependency {
                target: (*target).clone(),
                kind: DependencyKind::Static,
                provenance: designed("f14.synthetic.dependency"),
            })
            .collect(),
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f14.synthetic.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    }
}

/// Builds the minimal synthetic content catalog.
///
/// The catalog holds a ready launchable mission and an unsupported
/// launchable mission, so its `unsupported_count` is one and it is not
/// fully ready; the unsupported non-launchable resource stays visible
/// beside them (collections cannot exclude failed entries). Every row is a
/// synthetic fixture.
pub fn synthetic_catalog_fixture() -> Catalog {
    let mut catalog = Catalog::new();

    let world = id(ContentKind::World, "c1");
    let airframe = id(ContentKind::Airframe, "scout");
    let image = id(ContentKind::Image, "scout_body");
    let sound = id(ContentKind::Sound, "engine_loop");
    let ready_mission = id(ContentKind::Mission, "m01");
    let unsupported_mission = id(ContentKind::Mission, "m02");
    let missing_texture = id(ContentKind::Image, "missing_texture");

    catalog
        .insert(ready_element(world.clone(), "Synthetic World C1", &[]))
        .expect("synthetic world inserts");
    catalog
        .insert(ready_element(
            airframe.clone(),
            "Synthetic Scout",
            &[&image],
        ))
        .expect("synthetic airframe inserts");
    catalog
        .insert(ready_element(image.clone(), "Synthetic Scout Body", &[]))
        .expect("synthetic image inserts");
    catalog
        .insert(ready_element(sound.clone(), "Synthetic Engine Loop", &[]))
        .expect("synthetic sound inserts");
    catalog
        .insert(ready_element(
            ready_mission.clone(),
            "Synthetic Mission M01",
            &[&world, &airframe],
        ))
        .expect("synthetic ready mission inserts");

    let mut unsupported = ready_element(
        unsupported_mission.clone(),
        "Synthetic Mission M02",
        &[&world],
    );
    unsupported.readiness = Readiness::Unavailable;
    unsupported.parse_state = ParseState::Failed {
        diagnostic: "synthetic mission has no parser in this workspace stage".to_owned(),
    };
    unsupported.unsupported_reasons = vec![UnsupportedReason::ParseFailed {
        diagnostic: "synthetic mission has no parser in this workspace stage".to_owned(),
    }];
    catalog
        .insert(unsupported)
        .expect("synthetic unsupported mission inserts");

    let mut missing = ready_element(missing_texture, "Synthetic Missing Texture", &[]);
    missing.readiness = Readiness::Unavailable;
    missing.unsupported_reasons = vec![UnsupportedReason::MissingParser];
    catalog
        .insert(missing)
        .expect("synthetic unsupported image inserts");

    catalog
        .declare_launchable(&ready_mission)
        .expect("the ready mission is launchable");
    catalog
        .declare_launchable(&unsupported_mission)
        .expect("the unsupported mission is still launchable and counted");

    catalog
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC01 through the fixture: the unsupported launchable mission raises
    /// the unsupported count and prevents full readiness, and every row —
    /// including the failed ones — stays in the collection.
    #[test]
    fn accept_f14_a_fixture_keeps_unsupported_rows_and_counts_them() {
        let catalog = synthetic_catalog_fixture();

        assert_eq!(catalog.len(), 7, "every fixture row stays in the catalog");
        assert_eq!(catalog.launchable_count(), 2);
        assert_eq!(
            catalog.unsupported_count(),
            1,
            "the unsupported launchable mission is counted, never dropped"
        );
        assert!(!catalog.is_fully_ready());

        let unsupported = catalog.unsupported_launchables();
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].id.as_str(), "mission/m02");
        assert_eq!(unsupported[0].unsupported_codes(), vec!["parse_failed"]);

        let failed = catalog
            .elements()
            .filter(|element| !element.is_ready())
            .collect::<Vec<_>>();
        assert_eq!(
            failed.len(),
            2,
            "the unsupported non-launchable resource stays visible too"
        );
    }

    /// AC04 through the fixture: every launchable row is a synthetic
    /// fixture, so none can be mistaken for a retail catalog entry.
    #[test]
    fn accept_f14_a_fixture_is_synthetic_and_never_retail() {
        let catalog = synthetic_catalog_fixture();

        assert_eq!(catalog.original_launchable_count(), 0);
        assert_eq!(catalog.synthetic_launchable_count(), 2);
        assert!(
            !catalog.is_retail_ready(),
            "a synthetic catalog is never retail-ready"
        );
        assert!(
            catalog
                .elements()
                .all(|element| !element.origin.is_original()),
            "no fixture row claims an installation origin"
        );
    }
}
