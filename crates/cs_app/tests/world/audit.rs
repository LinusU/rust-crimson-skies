//! F18-D acceptance tests: the world-group audit, its refusals, and a real
//! offscreen GPU capture of each group's stored geometry.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-D`, acceptance scenario **AC04** ("visit every discovered
//! world group and compare representative geometry and traversal routes").
//! Task test prefix: `accept_f18_d_`.
//!
//! The instrument under test is production code, split the way the sheet's owner
//! paths are: `cs_content::world` owns the audit records and the judgement, and
//! `cs_app::world::{audit, gpu_capture}` reach the installation and the
//! renderer. No test here carries its own world-group walk, its own census or its
//! own render path.
//!
//! Four of the eight tests read no original data and are not ignored, so CI runs
//! them: they pin the audit's *contract* — what it maps, what it reports as
//! missing, what it refuses to construct, and which material group a refused
//! upload is attributed to — on synthetic fixtures. The remaining four need
//! capabilities CI does not have and are marked accordingly: two need a GPU,
//! two need `CS_GAME_DIR`, and one of the two needs both.

mod evidence;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cs_app::world::audit::{
    GEOMETRY_CONTAINER_FILE, OPENING_CLASS_ABSENT_FROM_THE_CORPUS, PRESENTABLE_PROBE_MESHES,
    REPRESENTATIVE_MESHES, ROUTE_ABSENT_FROM_THE_CORPUS, TEXTURE_ARCHIVE_FILE, audit_survey,
    class_absent, locate_stunt_passages, route_absent, survey_world_groups, upload_verdict,
};
use cs_app::world::gpu_capture::{CaptureRequest, capture_world_mesh};
use cs_content::mesh::RenderMesh;
use cs_content::stunts::{
    RetailStuntEncodingSurvey, RetailStuntGate, ScenarioFlyThroughTarget, StuntEncodingSpan,
};
use cs_content::world::{
    GroupFacts, OPENING_SEARCH_UNSUPPLIED, OpeningClass, PlacementSource, RepresentativeGeometry,
    RetailTriggerVolume, RouteSearch, StoredVolume, StuntOpening, StuntOpeningAudit,
    TraversalBlocker, TraversalRoute, TriggerVolumeSpan, UnlocatedOpening, UploadVerdict,
    WorldAuditError, WorldAuditGap, WorldGroupAudit, WorldGroupCensus, WorldGroupRef, WorldId,
};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_types::evidence::ContentHash;

// -------------------------------------------------------------- fixtures ---

/// The synthetic content every unignored test in this file is built from.
///
/// All **authored** values: the group keys, the counts, the extents. The
/// `PlacementSource::Undecoded` arm is not the retail installation's state —
/// F18-E decodes the node array, so retail groups report `Decoded` — but it
/// remains the arm that exercises the audit's blockers, which is what the
/// negative tests below pin; the decoded arm is covered separately by
/// [`decoded_census`].
fn synthetic_group(key: &str) -> WorldGroupRef {
    WorldGroupRef::new(
        WorldId::from_key(key).expect("a valid world key"),
        format!("ZBD/{key}"),
        format!("zbd/{key}/{GEOMETRY_CONTAINER_FILE}"),
        format!("zbd/{key}/{TEXTURE_ARCHIVE_FILE}"),
        vec!["M01".to_owned(), "M02".to_owned()],
    )
    .expect("two distinct mission labels")
}

/// One measured census for a synthetic group, with the numbers named in the
/// argument so a test never asserts on a number it did not write down.
#[allow(clippy::too_many_arguments)]
fn synthetic_census(
    key: &str,
    present: usize,
    placement: PlacementSource,
    vertex_scale_to_m: Option<f64>,
    routes: Vec<TraversalRoute>,
    openings: Vec<StuntOpening>,
    unlocated_openings: Vec<UnlocatedOpening>,
    route_search: RouteSearch,
) -> WorldGroupCensus {
    let representative: Vec<RepresentativeGeometry> = (0..2)
        .map(|step| RepresentativeGeometry {
            mesh_index: step * 10,
            triangles: 40 - step as usize * 10,
            vertices: 12,
            material_groups: 1,
            stored_min: [0.0, -1.0 - f64::from(step), 2.0],
            stored_max: [4.0, 1.0 + f64::from(step), 6.0],
            fingerprint: hash(&format!("{key}-mesh-{step}")),
            upload: UploadVerdict::Uploaded {
                groups: 1,
                vertices: 12,
                triangles: 40 - step as usize * 10,
            },
        })
        .collect();
    let facts = GroupFacts {
        container_key: format!("zbd/{key}/{GEOMETRY_CONTAINER_FILE}"),
        container_sha256: hash(key).to_hex(),
        mesh_slots: present + 3,
        present_meshes: present,
        declared_faces: 900,
        drawn_triangles: 1_200,
        missing_faces: 0,
        texture_names: 12,
        bound_texture_names: 10,
        multi_material_group_polygons: 4,
    };
    WorldGroupCensus::new(
        WorldId::from_key(key).expect("a valid world key"),
        facts,
        placement,
        vertex_scale_to_m,
        representative,
        routes,
        openings,
        unlocated_openings,
        route_search,
    )
    .expect("authored census values are finite")
}

/// The one census whose placement and unit scale **are** established, so the
/// audit's measured arm is reachable from a test.
fn decoded_census(key: &str) -> WorldGroupCensus {
    synthetic_census(
        key,
        40,
        PlacementSource::Decoded { placed_objects: 38 },
        Some(0.01),
        vec![TraversalRoute {
            route: "yard.through".to_owned(),
            from_m: [0.0, 12.0, 0.0],
            to_m: [400.0, 18.0, 0.0],
            clearance_m: Some(6.5),
            openings: vec![(OpeningClass::Hangar, 10)],
        }],
        OpeningClass::ALL
            .iter()
            .enumerate()
            .map(|(step, class)| StuntOpening {
                class: *class,
                mesh_index: 10 + step as u32 * 10,
                clearance_m: Some(6.5 - step as f64),
            })
            .collect(),
        // This fixture authors openings for every class and one route, so no
        // class is unlocated and the route search is never consulted.
        Vec::new(),
        RouteSearch::default(),
    )
}

/// A SHA-256 over a label, so a fixture's fingerprints are distinct values with
/// no meaning beyond "these two are not the same bytes".
fn hash(label: &str) -> ContentHash {
    cs_assets::install::sha256(label.as_bytes())
}

/// A stored mesh whose **second** material group is the one the upload adapter
/// refuses, because that group stores a normal on only some of its vertices.
///
/// The refusal has to be attributed to group 1, not to group 0. The retail
/// corpus is full of exactly this case, and an implementation that read the
/// group out of the adapter's message by string matching (`split_whitespace`
/// on `"material group 1 carries a normal on …"` yields `"material"`,
/// `"group"`, `"1"` as separate words, so a two-word prefix never matches)
/// reported every refusal as group 0 while the message beside it said
/// otherwise. Only comparing the **typed** field against the message catches
/// that, which is why this fixture and [`cs_app::world::upload_verdict`] — the
/// production function the census calls — are the two halves of the test.
///
/// Built through the production [`RenderMesh::from_stored_groups`], so the
/// fixture is the same value the census would measure.
fn second_group_refusal() -> RenderMesh {
    use cs_formats::gamez::RawMaterialGroup;

    // Two polygons, one per material group, so the mesh really has two groups.
    // Only the second carries a partial normal: one of its three corners names
    // the mesh's single stored normal and two name none, which is the
    // `IncompleteAttribute` refusal. `RawCorner::normal` is an **index** into
    // the mesh's normal array, not a vector.
    let positions = vec![
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [0.0, 2.0, 0.0],
        [2.0, 2.0, 0.0],
    ];
    let corner = |position: u32, normal: Option<u32>| RawCorner {
        position,
        normal,
        uv: Some([0.0, 0.0]),
        color: None,
    };
    let polygons = vec![
        RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: vec![corner(0, None), corner(1, None), corner(2, None)],
        },
        RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 1,
            corners: vec![corner(0, Some(0)), corner(3, None), corner(2, None)],
        },
    ];
    // One group entry per polygon; the second names material 1.
    let groups = vec![
        vec![RawMaterialGroup {
            material: 0,
            uvs: vec![[0.0, 0.0]; 3],
        }],
        vec![RawMaterialGroup {
            material: 1,
            uvs: vec![[0.0, 0.0]; 3],
        }],
    ];
    let render = RenderMesh::from_stored_groups(
        &RawMesh {
            positions,
            normals: vec![[0.0, 0.0, 1.0]],
            polygons,
        },
        &groups,
    )
    .expect("two polygons with one stored group each is valid stored data");
    assert_eq!(
        render.groups().len(),
        2,
        "the fixture must really have two material groups, or it cannot attribute a refusal to \
         the second one"
    );
    render
}

/// **The refusal names the group it refused.** The head of this stage's finding
/// is that 23 of the 24 retail representative meshes are refused by the
/// production upload adapter, and the *group* the adapter named is part of that
/// measured fact. Reading the group out of the adapter's `Display` instead of
/// its typed payload attributed every one of those refusals to material group 0,
/// including the ones the adapter refused on **group 1** — a census that
/// looked measured and was not.
///
/// This test does not need the installation: the refusal is produced by the
/// production adapter from an authored stored mesh, through the **production**
/// [`cs_app::world::upload_verdict`] the census itself calls, so CI runs it and
/// removing or breaking that function fails here.
#[test]
fn accept_f18_d_a_refused_upload_names_the_material_group_the_adapter_refused() {
    let render = second_group_refusal();
    let verdict = upload_verdict(&render);
    let UploadVerdict::Refused {
        material_group,
        reason,
    } = &verdict
    else {
        panic!(
            "the fixture must be refused by the production adapter, or it proves nothing; got \
             {verdict:?}"
        );
    };
    assert_eq!(
        *material_group, 1,
        "the adapter refused the second material group, so the verdict must say 1 and not the \
         first group: {reason}"
    );
    assert!(
        reason.contains("material group 1 "),
        "the adapter's own message is carried verbatim and must name the same group: {reason:?}"
    );
    assert_ne!(
        *material_group, 0,
        "a scraper that split the message on whitespace would produce exactly this 0"
    );

    // The census carries that same verdict, so a reader of a census sees the
    // number the adapter reported rather than a re-derived one.
    let carried = WorldGroupCensus::new(
        WorldId::from_key("c1").expect("a valid world key"),
        GroupFacts {
            container_key: "k".to_owned(),
            container_sha256: "d".to_owned(),
            mesh_slots: 12,
            present_meshes: 12,
            declared_faces: 3,
            drawn_triangles: 3,
            missing_faces: 0,
            texture_names: 0,
            bound_texture_names: 0,
            multi_material_group_polygons: 0,
        },
        PlacementSource::Undecoded {
            stored_node_records: 10,
            nodes_offset: 20,
        },
        None,
        vec![RepresentativeGeometry {
            mesh_index: 4,
            triangles: render.triangles().len(),
            vertices: render.vertices().len(),
            material_groups: render.groups().len(),
            stored_min: [0.0; 3],
            stored_max: [2.0; 3],
            fingerprint: hash("refused-representative"),
            upload: verdict.clone(),
        }],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        RouteSearch::default(),
    )
    .expect("finite authored values");
    assert_eq!(carried.refused_representatives(), 1);
    let refused = carried.refused().next().expect("the one refusal");
    assert_eq!(refused.mesh_index, 4);
    assert_eq!(refused.material_groups, 2);
    assert_eq!(
        refused.upload, verdict,
        "the census carries the adapter's own verdict, group included"
    );
}

// ------------------------------------------------------- the mapping arm ---

/// **AC04's mapping arm.** Every declared group is visited, its representative
/// geometry is compared group against group, and a group whose facts *are*
/// established gets measured traversal routes and located openings.
///
/// This is the half a reader checks by mutating the audit: if a group were
/// skipped, a census filed under the wrong row, a route recorded while its facts
/// are missing, or an opening class reported as located that nothing located,
/// this test fails.
#[test]
fn accept_f18_d_the_audit_visits_every_group_and_compares_its_representative_geometry() {
    let groups = vec![
        synthetic_group("c1c"),
        synthetic_group("c1"),
        synthetic_group("c2b"),
    ];
    let declared = WorldGroupAudit::new(groups.clone()).expect("three distinct groups");
    assert_eq!(declared.groups().len(), 3);

    let mut asked: Vec<String> = Vec::new();
    let report = declared.audit(|row| {
        asked.push(row.world().key().to_owned());
        match row.world().key() {
            "c1c" => Ok(decoded_census("c1c")),
            "c1" => Ok(synthetic_census(
                "c1",
                25,
                PlacementSource::Undecoded {
                    stored_node_records: 4_328,
                    nodes_offset: 2_104_512,
                },
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                RouteSearch::default(),
            )),
            _ => Err(cs_content::world::WorldGroupBlocker::NoGeometry {
                world: row.world().clone(),
                slots: 7,
            }),
        }
    });

    // The survey seam was asked once per group, for the row it names.
    assert_eq!(asked, vec!["c1c", "c1", "c2b"], "every group visited once");

    assert_eq!(report.groups().len(), 3);
    assert_eq!(
        report.visited().count(),
        2,
        "one group had no geometry at all"
    );
    assert_eq!(report.blocked().count(), 1);
    assert!(!report.is_empty(), "three groups were declared");
    assert!(
        !report.is_complete(),
        "a group with no geometry and a group with no placement keep the report from a pass"
    );
    assert_eq!(
        report.present_mesh_count(),
        65,
        "40 + 25 stored meshes read"
    );
    assert_eq!(
        report.drawn_triangle_count(),
        2_400,
        "1 200 per visited group"
    );

    // The group with its facts established is the only routed one, and its
    // routes and openings are the ones its own census stated.
    let routed: Vec<&str> = report
        .routed()
        .map(|audit| audit.group().world().key())
        .collect();
    assert_eq!(routed, vec!["c1c"]);
    let c1c = &report.groups()[0];
    assert_eq!(c1c.routes().len(), 1);
    assert_eq!(c1c.routes()[0].route, "yard.through");
    assert_eq!(c1c.routes()[0].openings, vec![(OpeningClass::Hangar, 10)]);
    assert!(c1c.routes_measured());
    assert_eq!(c1c.gaps(), &[] as &[cs_content::world::WorldAuditGap]);

    // The opening audit visits **every** class the sheet names, whether or not
    // anything located it, so "was a hangar looked for?" always has an answer.
    let classified: &[cs_content::world::StuntOpeningAudit] = c1c.openings();
    assert_eq!(classified.len(), OpeningClass::ALL.len());
    let located_classes: Vec<OpeningClass> = classified
        .iter()
        .flat_map(|audit| audit.located().iter().map(|opening| opening.class))
        .collect();
    assert_eq!(
        located_classes,
        OpeningClass::ALL.to_vec(),
        "every class the sheet names was visited, and the one group with its facts established \
         located all five"
    );
    assert!(
        classified.iter().all(StuntOpeningAudit::is_complete),
        "a class audit that located something has nothing unlocated"
    );
    assert_eq!(
        classified
            .iter()
            .flat_map(|audit| audit.unlocated())
            .count(),
        0,
        "nothing is left unlocated in a measured group"
    );
    for opening in classified.iter().flat_map(|audit| audit.located()) {
        assert!(opening.clearance_m.is_some());
        assert!(opening.clearance_m.expect("a measured clearance") > 0.0);
    }

    // The group whose placement is undecoded names the blocker with the
    // container's own numbers, and the scale blocker with the largest stored
    // extent the census measured.
    let c1 = &report.groups()[1];
    assert!(!c1.routes_measured());
    assert!(c1.routes().is_empty());
    assert_eq!(
        c1.traversal_blockers().len(),
        2,
        "both missing facts are named"
    );
    match &c1.traversal_blockers()[0] {
        TraversalBlocker::PlacementUndecoded {
            stored_node_records,
            nodes_offset,
            ..
        } => {
            assert_eq!(*stored_node_records, 4_328);
            assert_eq!(*nodes_offset, 2_104_512);
            let text = c1.traversal_blockers()[0].to_string();
            assert!(
                text.contains("4,328 stored node records")
                    || text.contains("4328 stored node records"),
                "the blocker must quote the measured count, got: {text}"
            );
        }
        other => panic!("the missing placement is named first, got {other:?}"),
    }
    match &c1.traversal_blockers()[1] {
        TraversalBlocker::VertexScaleUnmeasured {
            largest_stored_extent,
            ..
        } => assert!(
            *largest_stored_extent >= 4.0,
            "the scale blocker quotes the largest stored extent the census measured, got \
             {largest_stored_extent}"
        ),
        other => panic!("the unmeasured scale is named second, got {other:?}"),
    }

    // The empty group is a blocker with its own measured slot count, not a
    // census of zeroes and not a dropped row.
    let c2b = &report.groups()[2];
    assert_eq!(c2b.census(), None);
    let blocker = c2b.blocker().expect("an empty group is blocked");
    assert_eq!(blocker.world().key(), "c2b");
    assert!(blocker.to_string().contains('7'), "{}", blocker);
    assert_eq!(
        c2b.openings().len(),
        0,
        "a group with no census has no opening audit"
    );
}

// --------------------------------------------------- the negative arms ---

/// **AC04's negative arm.** Every way the audit could report a pass it has not
/// earned: a route or an opening stated while the facts they need are missing, a
/// clean group that still measured no route, and a census filed under the wrong
/// row.
///
/// The "wrong row" case is the one that matters most: a survey that returned
/// another group's numbers would otherwise be counted under this row without a
/// word, and every count in the report would be right and attributed wrongly.
#[test]
fn accept_f18_d_an_unlocated_opening_or_route_is_reported_instead_of_assumed() {
    let undecoded = PlacementSource::Undecoded {
        stored_node_records: 100,
        nodes_offset: 4_096,
    };

    // A census that claims a route and an opening while the placement and the
    // unit scale are both missing.
    let claims = WorldGroupAudit::new(vec![synthetic_group("c1")]).expect("one group");
    let report = claims.audit(|_| {
        Ok(synthetic_census(
            "c1",
            5,
            undecoded,
            None,
            vec![TraversalRoute {
                route: "invented".to_owned(),
                from_m: [0.0; 3],
                to_m: [1.0; 3],
                clearance_m: Some(2.0),
                openings: vec![(OpeningClass::Tunnel, 0)],
            }],
            vec![StuntOpening {
                class: OpeningClass::Tunnel,
                mesh_index: 0,
                clearance_m: Some(2.0),
            }],
            Vec::new(),
            RouteSearch::default(),
        ))
    });
    let gaps = report.groups()[0].gaps();
    assert_eq!(
        gaps,
        &[cs_content::world::WorldAuditGap::RouteWithoutFacts {
            world: WorldId::from_key("c1").expect("a valid world key"),
            routes: 1,
            openings: 1,
        }],
        "a route without a placement or a unit scale is a named gap, not a result"
    );
    assert_eq!(report.groups()[0].traversal_blockers().len(), 2);
    assert!(
        !report.is_complete(),
        "a report that caught the contradiction is still not a pass: the group has no route"
    );

    // The same contradiction with only one of the two halves claimed: a route
    // with no located opening is just as unsupported as a route with one, so the
    // gap check must fire on either.
    let route_only = WorldGroupAudit::new(vec![synthetic_group("c5")]).expect("one group");
    let report = route_only.audit(|_| {
        Ok(synthetic_census(
            "c5",
            5,
            undecoded,
            None,
            vec![TraversalRoute {
                route: "half".to_owned(),
                from_m: [0.0; 3],
                to_m: [1.0; 3],
                clearance_m: None,
                openings: Vec::new(),
            }],
            Vec::new(),
            Vec::new(),
            RouteSearch::default(),
        ))
    });
    assert_eq!(
        report.groups()[0].gaps(),
        &[cs_content::world::WorldAuditGap::RouteWithoutFacts {
            world: WorldId::from_key("c5").expect("a valid world key"),
            routes: 1,
            openings: 0,
        }],
        "a route with no placement is unsupported whether or not an opening was claimed too"
    );

    // A census whose facts are all established and which still states no route.
    let clean = WorldGroupAudit::new(vec![synthetic_group("c2")]).expect("one group");
    let report = clean.audit(|_| {
        Ok(synthetic_census(
            "c2",
            9,
            PlacementSource::Decoded { placed_objects: 7 },
            Some(0.05),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RouteSearch::default(),
        ))
    });
    assert_eq!(
        report.groups()[0].gaps(),
        &[cs_content::world::WorldAuditGap::NoRouteMeasured {
            world: WorldId::from_key("c2").expect("a valid world key"),
            placed_objects: 7,
        }],
        "a group that placed objects and measured its scale and still found no route is a \
         shortfall, not a clean bill of health"
    );
    assert_eq!(report.routed().count(), 0);

    // A census about a group the row does not declare.
    let mixed = WorldGroupAudit::new(vec![synthetic_group("c3")]).expect("one group");
    let report = mixed.audit(|_| Ok(decoded_census("c4")));
    assert_eq!(
        report.groups()[0].gaps(),
        &[cs_content::world::WorldAuditGap::CensusGroupMismatch {
            declared: WorldId::from_key("c3").expect("a valid world key"),
            measured: WorldId::from_key("c4").expect("a valid world key"),
        }],
        "the measured numbers belong to c4 and must not be counted under c3"
    );

    // Completeness needs *every* group visited, on its own: one group whose
    // geometry could not be read keeps the report from a pass even when every
    // other clause would hold, so an audit that only compared its own groups to
    // each other and never to the declared set cannot pass.
    let partly_read = WorldGroupAudit::new(vec![synthetic_group("c1"), synthetic_group("c2")])
        .expect("two groups");
    let report = partly_read.audit(|row| {
        if row.world().key() == "c1" {
            Ok(decoded_census("c1"))
        } else {
            Err(cs_content::world::WorldGroupBlocker::GeometryUnreadable {
                world: row.world().clone(),
                container: row.geometry_container().to_owned(),
                reason: "authored".to_owned(),
            })
        }
    });
    assert_eq!(report.visited().count(), 1, "one of the two groups read");
    assert!(
        !report.is_complete(),
        "a report that could not read one of its two declared groups is not a pass, however \
         complete the group it did read is: {:?}",
        report.groups()[0]
    );

    // An empty audit of nothing is never a pass.
    let empty = WorldGroupAudit::new(Vec::new()).expect("no groups is a valid set");
    let report = empty.audit(|_| unreachable!("an audit of no group asks for no census"));
    assert!(report.is_empty());
    assert!(
        !report.is_complete(),
        "an audit that looked at nothing must not read as a pass"
    );
}

/// The construction-time refusals, and the census values a reader would
/// otherwise compare and find silently wrong.
#[test]
fn accept_f18_d_world_group_records_refuse_contradictions_and_impossible_values() {
    // Two rows for one group would count one group's census twice.
    assert_eq!(
        WorldGroupAudit::new(vec![synthetic_group("c1"), synthetic_group("c1")]),
        Err(WorldAuditError::DuplicateGroup {
            world: "c1".to_owned()
        }),
        "a repeated group must be refused, not merged"
    );
    // A blank mission label names nothing.
    assert_eq!(
        WorldGroupRef::new(
            WorldId::from_key("c1").expect("a valid world key"),
            "ZBD/c1",
            "a",
            "b",
            vec![String::new()],
        ),
        Err(WorldAuditError::BlankMissionLabel {
            world: "c1".to_owned(),
            index: 0
        }),
    );
    assert_eq!(
        WorldGroupRef::new(
            WorldId::from_key("c1").expect("a valid world key"),
            "ZBD/c1",
            "a",
            "b",
            vec!["M01".to_owned(), "M01".to_owned()],
        ),
        Err(WorldAuditError::DuplicateMission {
            world: "c1".to_owned(),
            mission: "M01".to_owned()
        }),
    );
    // A group with **no** campaign mission is still a discovered group.
    let no_mission = WorldGroupRef::new(
        WorldId::from_key("c1").expect("a valid world key"),
        "ZBD/c1",
        "a",
        "b",
        Vec::new(),
    )
    .expect("an empty mission list is not an error");
    assert!(!no_mission.has_missions());
    assert!(no_mission.missions().is_empty());

    // A unit scale that is not a length.
    let world = WorldId::from_key("c1").expect("a valid world key");
    let facts = GroupFacts {
        container_key: "k".to_owned(),
        container_sha256: "d".to_owned(),
        mesh_slots: 2,
        present_meshes: 1,
        declared_faces: 3,
        drawn_triangles: 3,
        missing_faces: 0,
        texture_names: 0,
        bound_texture_names: 0,
        multi_material_group_polygons: 0,
    };
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let refused = WorldGroupCensus::new(
            world.clone(),
            facts.clone(),
            PlacementSource::Undecoded {
                stored_node_records: 1,
                nodes_offset: 2,
            },
            Some(bad),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RouteSearch::default(),
        )
        .expect_err("a non-finite scale must be refused, not stored");
        match refused {
            WorldAuditError::NonFiniteVertexScale {
                world: named,
                scale,
            } => {
                assert_eq!(named, "c1");
                // Compared by bits, because `NaN != NaN` and a test that could
                // not tell one non-finite value from another would pass on any.
                assert_eq!(
                    scale.to_bits(),
                    bad.to_bits(),
                    "the refusal quotes the offending value"
                );
            }
            other => panic!("a non-finite scale is refused by name, got {other:?}"),
        }
    }
    // A stored corner no render vertex can hold.
    let bad_corner = RepresentativeGeometry {
        mesh_index: 4,
        triangles: 1,
        vertices: 3,
        material_groups: 1,
        stored_min: [0.0, f64::NAN, 0.0],
        stored_max: [1.0; 3],
        fingerprint: hash("corner"),
        upload: UploadVerdict::Refused {
            material_group: 0,
            reason: "authored".to_owned(),
        },
    };
    assert_eq!(
        WorldGroupCensus::new(
            world.clone(),
            facts.clone(),
            PlacementSource::Undecoded {
                stored_node_records: 1,
                nodes_offset: 2
            },
            None,
            vec![bad_corner],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            RouteSearch::default(),
        )
        .err(),
        Some(WorldAuditError::NonFiniteStoredCorner {
            world: "c1".to_owned(),
            mesh_index: 4,
            axis: 1
        })
    );
    // And the measured arm really is reachable: with a decoded placement, a
    // measured unit scale, a route and all five opening classes the audit does
    // report itself complete — which is what makes the retail run's
    // `!is_complete()` a verdict rather than a tautology.
    let measured = WorldGroupAudit::new(vec![synthetic_group("c1")]).expect("one group");
    let report = measured.audit(|_| Ok(decoded_census("c1")));
    assert!(
        report.is_complete(),
        "a group whose facts are established and which measured routes and all five opening \
         classes is a complete audit: {report:?}"
    );
    assert_eq!(report.blocker_count(), 0);
    assert_eq!(report.gap_count(), 0);
    assert_eq!(report.routed().count(), 1);
    assert_eq!(
        report.groups()[0].routes()[0].openings.len(),
        1,
        "the route names the one opening it threads"
    );
}

// ------------------------------------------------------- the GPU capture ---

/// A real offscreen render of a real world mesh, on the real adapter.
///
/// The evidence F18-D's `gpu` capability needs, and the only way to tell a drawn
/// frame from a file that merely exists. **Ignored** because CI has no GPU
/// adapter: `cargo test --workspace` skips it, and the implementer and reviewer
/// run it with `--include-ignored`.
///
/// What it pins: the capture drew *something* (more than one luminance level
/// and a non-zero covered area), the PNG on disk is the one the capture hashed,
/// and the counts it reports are the counts the production upload produced.
/// A frame that came back blank is [`cs_app::world::GpuCaptureError::UniformFrame`]
/// and this test can never see a `GpuCapture` at all, so the assertion is
/// reachable only by a real draw.
#[test]
#[ignore = "requires a GPU: CI selects no adapter, so run it with --include-ignored"]
fn accept_f18_d_a_gpu_capture_proves_the_stored_geometry_was_drawn() {
    let render = synthetic_arch();
    let png = evidence_dir().join("gpu-capture-arch.png");
    let capture = capture_world_mesh(&CaptureRequest {
        group: "fixture",
        mesh_index: 0,
        render: &render,
        unknowns: &[],
        png: &png,
    })
    .expect("a real adapter draws a real mesh and returns a measured capture");

    assert_eq!(capture.width, cs_app::world::CAPTURE_WIDTH);
    assert_eq!(capture.height, cs_app::world::CAPTURE_HEIGHT);
    assert!(
        capture.distinct_luminance > 1,
        "a frame with one luminance level is the background alone, not a drawn mesh: {capture:?}"
    );
    assert!(
        capture.covered_pixels > 0,
        "nothing reached the frame: {capture:?}"
    );
    assert!(capture.drew_geometry());
    assert!(
        !capture.adapter.contains("no adapter reported"),
        "the capture must name the adapter the driver selected, got {:?}",
        capture.adapter
    );
    // The geometry that was submitted is the geometry the production upload
    // built, and the report says so: four stored quad faces, two triangles each.
    assert_eq!(capture.groups, 1, "one material group, one draw");
    assert_eq!(capture.triangles, 8, "four quad faces of the authored arch");
    assert!(capture.vertices >= 3, "{}", capture.vertices);
    // Back-face culling is expected and is *not* a defect here: the capture
    // presents the stored winding, and which winding the original treated as
    // front-facing is F17's open `FrontFaceWinding` question. The capture
    // therefore reports the triangles it submitted and the coverage the
    // surviving ones reached, and makes no claim about how many faces the
    // original would have drawn.
    assert!(
        capture.covered_permille > 0 && capture.covered_permille < 1_000,
        "the arch fills part of the frame and not all of it: {} per mille",
        capture.covered_permille
    );
    // The digest is of the file on disk, so it can be re-checked by a reader.
    let bytes = std::fs::read(&png).expect("the PNG is on disk");
    assert_eq!(cs_assets::install::sha256(&bytes), capture.png_sha256);
    assert_eq!(bytes.len() as u64, capture.png_bytes);
}

/// The refusals a capture makes before and after the render, each reached with an
/// argument that produces it rather than by mocking the driver.
#[test]
#[ignore = "requires a GPU: CI selects no adapter, so run it with --include-ignored"]
fn accept_f18_d_a_capture_that_drew_nothing_is_refused_rather_than_written() {
    // A mesh with no drawable triangle has nothing to frame, and the refusal
    // names the group and the index rather than writing an empty PNG.
    let empty = empty_mesh();
    let png = evidence_dir().join("gpu-capture-empty.png");
    let _ = std::fs::remove_file(&png);
    let refused = capture_world_mesh(&CaptureRequest {
        group: "fixture",
        mesh_index: 7,
        render: &empty,
        unknowns: &[],
        png: &png,
    });
    assert!(
        matches!(
            refused,
            Err(cs_app::world::GpuCaptureError::EmptyMesh { groups: 0, .. })
        ),
        "an empty mesh is refused by name, got {refused:?}"
    );
    assert!(
        !png.exists(),
        "a refused capture must leave no file that could be read as evidence"
    );

    // A mesh whose every stored corner is one point has no bounds, so there is
    // no frame to put it in. Checked before the app is built, so the refusal
    // costs nothing and writes nothing.
    let point = one_point_mesh();
    let png = evidence_dir().join("gpu-capture-point.png");
    let _ = std::fs::remove_file(&png);
    let refused = capture_world_mesh(&CaptureRequest {
        group: "fixture",
        mesh_index: 9,
        render: &point,
        unknowns: &[],
        png: &png,
    });
    assert!(
        matches!(
            refused,
            Err(cs_app::world::GpuCaptureError::DegenerateBounds { mesh_index: 9, .. })
        ),
        "a mesh with no extent is refused by name, got {refused:?}"
    );
    assert!(!png.exists(), "a refused capture must leave no file");

    // A mesh that uploads and yet draws nothing: three stored corners on the
    // same position index. The upload adapter skips a degenerate stored step —
    // correctly, it has no area — so the frame comes back as the background, and
    // that is exactly what `UniformFrame` exists to report. Without this case
    // the uniform-frame gate is unreachable from a test and would be untested
    // production code.
    let degenerate = degenerate_only_mesh();
    let png = evidence_dir().join("gpu-capture-degenerate.png");
    let _ = std::fs::remove_file(&png);
    let refused = capture_world_mesh(&CaptureRequest {
        group: "fixture",
        mesh_index: 11,
        render: &degenerate,
        unknowns: &[],
        png: &png,
    });
    match refused {
        Err(cs_app::world::GpuCaptureError::UniformFrame {
            distinct_luminance,
            covered_pixels,
            total_pixels,
            ..
        }) => {
            assert_eq!(distinct_luminance, 1, "the frame is the background alone");
            assert_eq!(covered_pixels, 0, "nothing reached the frame");
            assert_eq!(
                total_pixels,
                (cs_app::world::CAPTURE_WIDTH * cs_app::world::CAPTURE_HEIGHT) as usize,
                "the frame is the declared capture size"
            );
        }
        other => panic!("a frame that drew nothing is refused by name, got {other:?}"),
    }
    assert!(
        !png.exists(),
        "a refused capture must leave no file that could be read as evidence"
    );
}

/// A stored mesh with **one** stored position and a triangle whose three corners
/// all name it: the mesh uploads (it holds a drawable triangle) and every
/// rendered vertex is the same point, so it has no extent on any axis.
fn one_point_mesh() -> RenderMesh {
    RenderMesh::build(&RawMesh {
        positions: vec![[4.0, 4.0, 4.0]],
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: (0..3)
                .map(|_| RawCorner {
                    position: 0,
                    normal: None,
                    uv: Some([0.0, 0.0]),
                    color: None,
                })
                .collect(),
        }],
    })
    .expect("a three-corner outline on one position is valid stored data")
}

/// A stored mesh with **two** positions and a triangle whose corners name them
/// as `0, 0, 1`: the stored step is degenerate (two equal position indices), so
/// the render mesh keeps it, the upload adapter skips it, and the buffer that
/// reaches the GPU is empty — a frame that can only come back as the background.
///
/// Two positions, not one, so the mesh *has* an extent: this case has to get past
/// the bounds check to reach the uniform-frame one, or the two refusals would be
/// indistinguishable.
fn degenerate_only_mesh() -> RenderMesh {
    RenderMesh::build(&RawMesh {
        positions: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 6.0]],
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: [0, 0, 1]
                .into_iter()
                .map(|position| RawCorner {
                    position,
                    normal: None,
                    uv: Some([0.0, 0.0]),
                    color: None,
                })
                .collect(),
        }],
    })
    .expect("a triangle with two equal stored positions is valid stored data")
}

/// A synthetic mesh with a real opening in it: two legs and a lintel, as the
/// production content layer's render mesh holds it.
///
/// Authored fixture content (`Origin::SyntheticFixture`), the same arch shape
/// the F18-A/B fixtures use, built through [`RenderMesh::build`] — the one
/// production constructor — so the capture draws exactly the geometry a real
/// world object would draw. Twelve triangles: four quad faces, two triangles
/// each, in one material group.
fn synthetic_arch() -> RenderMesh {
    let positions: Vec<[f32; 3]> = vec![
        // left leg
        [-2.0, 0.0, -0.5],
        [-2.0, 0.0, 0.5],
        [-2.0, 3.0, 0.5],
        [-2.0, 3.0, -0.5],
        // right leg
        [2.0, 0.0, -0.5],
        [2.0, 0.0, 0.5],
        [2.0, 3.0, 0.5],
        [2.0, 3.0, -0.5],
        // lintel, front and back faces
        [-2.0, 3.0, -0.5],
        [2.0, 3.0, -0.5],
        [2.0, 4.0, -0.5],
        [-2.0, 4.0, -0.5],
        [-2.0, 3.0, 0.5],
        [2.0, 3.0, 0.5],
        [2.0, 4.0, 0.5],
        [-2.0, 4.0, 0.5],
    ];
    let quads: [[u32; 4]; 4] = [[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11], [12, 13, 14, 15]];
    let polygons = quads
        .iter()
        .map(|quad| RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners: quad
                .iter()
                .map(|index| RawCorner {
                    position: *index,
                    normal: None,
                    uv: Some([0.0, 0.0]),
                    color: None,
                })
                .collect(),
        })
        .collect();
    RenderMesh::build(&RawMesh {
        positions,
        normals: Vec::new(),
        polygons,
    })
    .expect("authored arch geometry has a decodable outline")
}

/// A stored mesh with positions and no polygon at all: nothing to draw, so a
/// frame of it would be empty however well it was framed.
fn empty_mesh() -> RenderMesh {
    RenderMesh::build(&RawMesh {
        positions: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
        normals: Vec::new(),
        polygons: Vec::new(),
    })
    .expect("a mesh with no polygon has nothing to validate")
}

// ------------------------------------------------------------- the retail ---

/// **AC04 over the real installation.** Every world group production discovery
/// finds is visited, each one's own `gamez.zbd` is read through the production
/// VFS mount, the production ZBD dispatch and both production GameZ readers, and
/// the representative geometry of every group is compared.
///
/// **F18-D over the real installation, after task #732's opening rule.** Every
/// group's placement is decoded and the measured metre is carried, so both
/// traversal blockers are gone. What the traversal half now says is a
/// measurement rather than an absence: a **stunt passage** is located wherever
/// the group's own instant-action scenario declares a fly-through danger-zone
/// target (54 across six of the eight groups, `c1c` and `c2b` author none),
/// the other four opening classes are reported unlocated *with the measurement
/// that says the corpus holds none of them*, and no route is stated because the
/// container stores no path and the corpus's only route carrier (`aiv.zrd`) has
/// an unmeasured encoding — one `NoRouteInMeasuredCorpus` gap per group, naming
/// the missions it affects. A route claimed without its two facts is still
/// `RouteWithoutFacts`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_d_retail_every_discovered_world_group_is_visited_and_compared() {
    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"));
    let survey =
        survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");

    // Every discovered group is a row, and the discovered set is the reference
    // set of eight leads on this installation family.
    let keys: Vec<String> = survey
        .groups
        .iter()
        .map(|group| group.world().key().to_owned())
        .collect();
    let unique: BTreeSet<&String> = keys.iter().collect();
    assert_eq!(unique.len(), keys.len(), "one row per group, no repeats");
    assert_eq!(
        keys,
        ["c1", "c1b", "c1c", "c2", "c2b", "c3", "c4", "c5"],
        "the installation's world groups, in discovered order"
    );
    assert!(
        survey.absent_reference_groups.is_empty(),
        "every reference lead of spec F02 is present, absent: {:?}",
        survey.absent_reference_groups
    );
    // The campaign walk puts missions in all eight, which is why no group needed
    // the empty-mission path — and it is a real fact about this installation
    // rather than an assumption, so it is asserted rather than tolerated.
    for group in &survey.groups {
        assert!(
            group.group.has_missions() && !group.missions.is_empty(),
            "{}: every discovered group carries at least one campaign mission here",
            group.world()
        );
    }

    // The report comes from the survey this test already has, so the
    // installation is walked once rather than twice.
    let report = audit_survey(&survey).expect("the audit runs over the survey");
    assert_eq!(report.groups().len(), 8, "one verdict per discovered group");
    assert!(
        !report.is_empty(),
        "eight groups were declared, so the report is not empty"
    );
    assert_eq!(
        report.visited().count(),
        8,
        "every group's own geometry container reads through the production path"
    );
    assert_eq!(report.blocked().count(), 0);
    assert!(
        !report.is_complete(),
        "the four opening classes the corpus states nothing about stay unlocated and no route is \
         stated, so every group carries a NoRouteInMeasuredCorpus gap and the audit is \
         explicitly not a pass"
    );
    assert_eq!(report.routed().count(), 0);
    assert!(
        report.present_mesh_count() > 0,
        "the corpus holds stored meshes"
    );
    assert!(report.drawn_triangle_count() > 0);

    // Per group: measured geometry that agrees with the census, a traversal
    // verdict whose two facts both exist, and one opening row per class with
    // the measurement behind it.
    //
    // The stunt-passage column is F42-D's own measurement of the eight
    // instant-action scenarios (task #463): the fly-through danger-zone targets
    // each group's `targets.zrd` declares. `c1c` and `c2b` author none, which is
    // the two rows where the class must come back unlocated **with that
    // measurement** rather than as a bare zero.
    let stunt_passages: BTreeMap<&str, usize> = [
        ("c1", 5_usize),
        ("c1b", 5),
        ("c1c", 0),
        ("c2", 9),
        ("c2b", 0),
        ("c3", 4),
        ("c4", 14),
        ("c5", 17),
    ]
    .into_iter()
    .collect();
    let mut total_stored_nodes = 0_u32;
    let mut total_placed = 0_usize;
    for audit in report.visited() {
        let census = audit.census().expect("a visited group has a census");
        let group = audit.group();
        assert_eq!(census.world(), group.world());
        assert!(
            census.present_meshes() > 0,
            "{}: a container with no present mesh holds no geometry",
            group.world()
        );
        assert!(
            census.mesh_slots() >= census.present_meshes(),
            "{}: present slots cannot exceed the slots there are",
            group.world()
        );
        assert!(
            census.declared_faces() > 0,
            "{}: a container that declares no face holds no scene",
            group.world()
        );
        assert!(
            census.drawn_triangles() > 0,
            "{}: a container that draws no triangle holds no presentable geometry",
            group.world()
        );
        assert!(
            census.missing_faces() <= census.declared_faces(),
            "{}: a missing face is one of the declared ones",
            group.world()
        );
        assert_eq!(
            census.representative().len(),
            REPRESENTATIVE_MESHES,
            "{}: the declared representative budget",
            group.world()
        );
        // The refused count is the audit's own, and it is counted from the
        // verdicts the representatives carry rather than from a second stored
        // number, so the two cannot disagree. This was a tautology once
        // (`refused_representatives()` compared with `refused().count()`, and
        // the first is *defined* as the second); it now checks the verdicts
        // themselves, so a census that reported every representative as
        // presentable, or attributed a refusal to the wrong material group,
        // fails here.
        let refused_here = census
            .representative()
            .iter()
            .filter(|mesh| matches!(mesh.upload, UploadVerdict::Refused { .. }))
            .count();
        assert_eq!(
            census.refused_representatives(),
            refused_here,
            "{}: the refused count is counted from the representatives' own verdicts",
            group.world()
        );
        for mesh in census.refused() {
            let UploadVerdict::Refused {
                material_group,
                reason,
            } = &mesh.upload
            else {
                panic!("{}: refused() yielded an uploaded mesh", group.world());
            };
            // The group the adapter refused is carried **in the typed field**,
            // and the message quotes the same group. An implementation that
            // scraped the number out of the message instead would report
            // `material_group: 0` for a refusal the adapter attributed to
            // group 1, and the two would disagree.
            assert!(
                reason.contains(&format!("material group {material_group} ")),
                "{} mesh {}: the verdict's group {material_group} is not the group the adapter's \
                 own message names: {reason:?}",
                group.world(),
                mesh.mesh_index
            );
            assert!(
                *material_group < mesh.material_groups,
                "{} mesh {}: refused material group {material_group} is outside the mesh's own \
                 {} material group(s)",
                group.world(),
                mesh.mesh_index,
                mesh.material_groups
            );
        }
        for mesh in census.representative() {
            assert!(
                mesh.triangles > 0,
                "{}: a representative must draw",
                group.world()
            );
            for axis in 0..3 {
                assert!(
                    mesh.stored_max[axis] >= mesh.stored_min[axis],
                    "{}: stored bounds are ordered on axis {axis}",
                    group.world()
                );
            }
            assert!(
                mesh.stored_radius() > 0.0,
                "{}: a representative with no extent cannot be framed",
                group.world()
            );
        }
        assert!(
            !census.container_sha256().is_empty(),
            "{}: production discovery measures the container's digest",
            group.world()
        );

        // Placement: decoded by the production node reader, never beyond the
        // records the container's own header declares.
        let stored_node_records = survey
            .group(group.world())
            .and_then(|surveyed| surveyed.container().ok())
            .map(|container| container.meshes.header.node_array_size)
            .expect("the surveyed container is readable");
        match census.placement() {
            PlacementSource::Decoded { placed_objects } => {
                assert!(
                    placed_objects > 0,
                    "{}: a decoded world places at least one mesh",
                    group.world()
                );
                assert!(
                    u32::try_from(placed_objects).expect("fits") <= stored_node_records,
                    "{}: a node places at most one mesh, so placed objects cannot exceed the {} \
                     stored node records",
                    group.world(),
                    stored_node_records
                );
                total_stored_nodes += stored_node_records;
                total_placed += placed_objects;
            }
            PlacementSource::Undecoded { .. } => panic!(
                "{}: the node array is decoded by `read_gamez_nodes`, so the placement must be \
                 reported as decoded",
                group.world()
            ),
        }
        assert_eq!(
            census.vertex_scale_to_m(),
            Some(1.0),
            "{}: the stored vertex unit is the measured metre (task #677, code-derived)",
            group.world()
        );

        // Traversal: both facts established, so the blockers are gone; the audit
        // states no route, and says so through the **measured** gap — what was
        // searched, why the corpus holds none, and which missions the absence
        // affects — rather than the old "measured for the wrong thing" shortfall.
        assert!(audit.routes_measured());
        assert!(
            audit.traversal_blockers().is_empty(),
            "{}: placement and scale are established, so no blocker remains",
            group.world()
        );
        assert_eq!(
            audit.routes().len(),
            0,
            "{}: the container stores no path and aiv.zrd's encoding is unmeasured (task #455), \
             so no route can be stated",
            group.world()
        );
        let located_here = audit
            .openings()
            .iter()
            .map(|class_audit| class_audit.located().len())
            .sum::<usize>();
        match audit.gaps() {
            [
                WorldAuditGap::NoRouteInMeasuredCorpus {
                    world: named,
                    located_openings,
                    affected,
                    measured,
                },
            ] => {
                assert_eq!(
                    named,
                    group.world(),
                    "the gap is filed against the group it measured"
                );
                assert_eq!(
                    *located_openings, located_here,
                    "the gap quotes how many openings the same rule did locate"
                );
                assert!(
                    !affected.is_empty(),
                    "{}: the gap names the content the absent route affects, got nothing",
                    group.world()
                );
                assert!(
                    affected.iter().any(|entry| entry == group.world().key())
                        || affected.iter().all(|entry| entry.starts_with('M')),
                    "{}: the affected content is this group's missions or the group itself, got \
                     {affected:?}",
                    group.world()
                );
                assert!(
                    measured.contains("aiv.zrd") && measured.contains("455"),
                    "{}: the gap cites the route carrier and the task that owns its unmeasured \
                     encoding, got {measured:?}",
                    group.world()
                );
            }
            other => panic!(
                "{}: with both facts established and no route stated, the one gap is \
                 NoRouteInMeasuredCorpus, got {other:?}",
                group.world()
            ),
        }

        // Openings: all five classes visited. Stunt passages are located from
        // the group's own instant-action declarations; the other four classes
        // are unlocated **with the measurement that says the corpus holds none
        // of them** — never a bare class, never a silent zero.
        assert_eq!(audit.openings().len(), OpeningClass::ALL.len());
        let mut unlocated_here = 0_usize;
        for class_audit in audit.openings() {
            if class_audit.located().is_empty() {
                assert_eq!(
                    class_audit.unlocated().len(),
                    1,
                    "{}: an unlocated class carries exactly one reason",
                    group.world()
                );
                let row = &class_audit.unlocated()[0];
                let class = row.class();
                assert!(
                    OpeningClass::ALL.contains(&class),
                    "{}: the unlocated class is one the sheet names",
                    group.world()
                );
                assert!(
                    !row.measured().trim().is_empty(),
                    "{}: {class} is unlocated with no measurement: the silent zero the audit \
                     exists to prevent",
                    group.world()
                );
                assert_ne!(
                    row.measured(),
                    OPENING_SEARCH_UNSUPPLIED,
                    "{}: the retail survey searched every class, so no class may report that the \
                     search never happened",
                    group.world()
                );
                if class == OpeningClass::StuntPassage {
                    assert!(
                        row.measured().contains("fly-through danger-zone target"),
                        "{}: a group that authors no stunt passage says so from its own scenario \
                         declarations, got {:?}",
                        group.world(),
                        row.measured()
                    );
                    assert_eq!(
                        *stunt_passages
                            .get(group.world().key())
                            .expect("every discovered group is pinned here"),
                        0,
                        "{}: only a group that authors no fly-through target may leave this class \
                         unlocated",
                        group.world()
                    );
                } else {
                    assert!(
                        row.measured().contains("the container stores no field"),
                        "{}: {class} is reported absent from the corpus with that measurement, \
                         got {:?}",
                        group.world(),
                        row.measured()
                    );
                }
                assert!(!class_audit.is_complete());
                unlocated_here += 1;
                continue;
            }
            assert_eq!(
                class_audit.located()[0].class,
                OpeningClass::StuntPassage,
                "{}: the only class the measured rule locates is the stunt passage",
                group.world()
            );
            assert!(
                class_audit.unlocated().is_empty(),
                "{}: a located class has nothing unlocated",
                group.world()
            );
            for opening in class_audit.located() {
                let clearance = opening.clearance_m.unwrap_or_else(|| {
                    panic!(
                        "{}: a located opening carries a measured clearance",
                        group.world()
                    )
                });
                assert!(
                    clearance.is_finite() && clearance > 0.0,
                    "{}: the clearance is the zone box's own narrowest extent times the measured \
                     metre, got {clearance}",
                    group.world()
                );
            }
        }
        let stunt_here = *stunt_passages
            .get(group.world().key())
            .expect("every discovered group is pinned here");
        assert_eq!(
            located_here,
            stunt_here,
            "{}: the located stunt passages are exactly the fly-through targets this group's own \
             scenario declares",
            group.world()
        );
        let expected_unlocated = if stunt_here > 0 {
            OpeningClass::ALL.len() - 1
        } else {
            OpeningClass::ALL.len()
        };
        assert_eq!(
            unlocated_here,
            expected_unlocated,
            "{}: exactly the four classes the corpus states nothing about are unlocated when \
             {stunt_here} stunt passage(s) were located, and all five when none was",
            group.world()
        );
    }
    assert!(
        total_placed > 0 && total_placed <= total_stored_nodes as usize,
        "placed objects {total_placed} over {total_stored_nodes} stored nodes"
    );
    assert!(
        total_stored_nodes > 10_000,
        "the eight world archives declare far more stored node records than one group, got \
         {total_stored_nodes}"
    );
}

/// **The `gpu` half of AC04 over the real installation**: one representative mesh
/// of every discovered world group, drawn on the real adapter, each capture a
/// measured non-empty frame.
///
/// This is the discriminating half of the F18-D evidence: the census says how
/// much geometry a group stores, and this says the geometry is *drawable as
/// stored*. A group whose mesh count is right but whose frame is blank fails
/// here, which a census-only test cannot see.
///
/// **Ignored** because CI has no GPU adapter; run it with `--include-ignored`
/// beside the other retail tests.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU: CI has neither, so run it with --include-ignored"]
fn accept_f18_d_retail_every_world_group_draws_a_measured_frame_on_the_gpu() {
    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"));
    let survey =
        survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");
    assert_eq!(
        survey.groups.len(),
        8,
        "every discovered world group is visited"
    );

    let directory = evidence_dir();
    std::fs::create_dir_all(&directory).expect("the private evidence directory is writable");
    let mut captured = 0_usize;
    let mut refused = 0_usize;
    // `(group, mesh index, uploadable, geometry digest)` for every frame drawn.
    let mut drawn: BTreeMap<String, (u32, bool, String)> = BTreeMap::new();
    for group in &survey.groups {
        let container = group.container().expect("every group's container read");
        // The group's own largest **uploadable** stored mesh, by the same
        // declared rule the census uses, so the capture and the census name the
        // same geometry. "Uploadable" is the adapter's own verdict and the
        // refused representatives are counted below rather than skipped in
        // silence: several of the retail world's largest meshes store a normal
        // on only some of their vertices, and the upload adapter will not fill a
        // buffer from that.
        let (mesh_index, render) = container.largest_presentable().unwrap_or_else(|| {
            panic!(
                "{}: none of the first {PRESENTABLE_PROBE_MESHES} stored meshes went through \
                     the upload adapter, so there is nothing to draw",
                group.world()
            )
        });
        assert!(
            !render.triangles().is_empty(),
            "{}: a representative mesh must draw",
            group.world()
        );
        refused += container.refused_before_presentable();

        let png = directory.join(format!("render-{}.png", group.world().key()));
        let unknown = capture_group_mesh(group.world().key(), render, mesh_index, &png);
        assert!(
            unknown.drew_geometry(),
            "{}: the capture came back with {} luminance levels and {} covered pixels, so the \
             stored geometry was not drawn: {unknown:?}",
            group.world(),
            unknown.distinct_luminance,
            unknown.covered_pixels
        );
        assert!(!unknown.adapter.contains("no adapter reported"));
        assert_eq!(unknown.mesh_index, mesh_index);
        // Two groups can legitimately draw the **same** stored geometry: the
        // retail archives share meshes, and `c1c`, `c2b` and `c3` all hold
        // array index 33 with identical vertex bytes, so their frames are
        // byte-identical. That is a fact about the corpus, and the census must
        // be able to *show* it rather than leave a reader comparing three
        // identical PNG digests and wondering whether the capture reused a
        // file. So the geometry that was drawn is fingerprinted here, and the
        // distinct-geometry count is asserted against the frame count below.
        drawn.insert(
            group.world().key().to_owned(),
            (
                mesh_index,
                cs_app::world::upload_verdict(render).is_uploaded(),
                geometry_digest(render).to_hex(),
            ),
        );
        captured += 1;
    }
    assert_eq!(captured, 8, "one measured frame per discovered world group");
    let distinct: BTreeSet<&String> = drawn.values().map(|(_, _, digest)| digest).collect();
    assert!(
        distinct.len() <= drawn.len(),
        "each frame's geometry digest is one of the frames' own, so the distinct count cannot \
         exceed the frame count"
    );
    eprintln!(
        "F18-D GPU: {captured} measured frames over {} distinct stored geometries; the archives \
         share meshes, so equal digests are shared content and not a reused capture",
        distinct.len()
    );
    // F17-G: the partial-normal refusal this stage first measured (23 of 24
    // representatives) is lifted by `PARTIAL_NORMAL_POLICY`, which splits a
    // group by stored normal presence. Anything still refused would be a
    // different cause and must fail here rather than be skipped in silence.
    assert_eq!(
        refused, 0,
        "no retail representative is expected to be refused under the declared partial-normal \
         policy"
    );
    eprintln!(
        "F18-D GPU: {captured} measured frames; {refused} probed stored meshes were refused by \
         the upload adapter before the first accepted one"
    );
}

/// A digest of exactly the vertex bytes a capture would draw, so two frames
/// with the same digest are known to be the same **geometry** rather than two
/// draws that merely happened to look alike.
///
/// Deliberately over the render mesh's own positions, which are the production
/// upload's input bit for bit; nothing about the camera, the colour or the
/// adapter enters the digest, so it identifies the stored geometry alone.
fn geometry_digest(render: &RenderMesh) -> cs_types::evidence::ContentHash {
    let mut bytes: Vec<u8> = Vec::new();
    for vertex in render.vertices() {
        for component in vertex.position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for group in 0..render.groups().len() {
        bytes.extend_from_slice(&(group as u32).to_le_bytes());
    }
    cs_assets::install::sha256(&bytes)
}

/// Renders one group's mesh through the production upload and captures it.
///
/// The presentation unknowns handed to the upload adapter are **empty** on
/// purpose, and the reason is stated here rather than left implicit: the
/// capture presents a flat colour with no stored texture, so F17's open
/// questions (`FrontFaceWinding`, `UvOrigin`, `VertexColor`,
/// `MultiMaterialGroup`) change nothing about the frame this measures. Passing
/// them would make the artifact depend on a subsystem this stage makes no claim
/// about. The geometry — the part the capture *is* evidence for — is the
/// production upload's, bit for bit.
fn capture_group_mesh(
    group: &str,
    render: &RenderMesh,
    mesh_index: u32,
    png: &Path,
) -> cs_app::world::GpuCapture {
    capture_world_mesh(&CaptureRequest {
        group,
        mesh_index,
        render,
        unknowns: &[],
        png,
    })
    .unwrap_or_else(|error| panic!("{group} mesh {mesh_index} could not be captured: {error}"))
}

/// The private evidence directory, which the harness and the captures share.
fn evidence_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../private/evidence/F18-D")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../private/evidence/F18-D")
                .to_path_buf()
        })
}

/// F17-G on the retail corpus: every representative mesh the partial-normal
/// refusal used to stop uploads under the declared split policy, the split
/// loses no triangle (each group's parts add up to the group's own stored
/// triangles) and at least one group really splits, so the policy — not an
/// unrelated change — is what lifted the refusal.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f17_g_retail_representatives_upload_under_the_partial_normal_policy() {
    use cs_app::render::bevy_mesh::{GroupPart, upload_group, upload_group_parts};

    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"));
    let survey =
        survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");
    let (mut meshes, mut split_groups, mut strict_refusals) = (0_usize, 0_usize, 0_usize);
    for group in &survey.groups {
        let container = group.container().expect("every group's container read");
        for (index, render, _) in &container.representatives {
            meshes += 1;
            for (group_index, source) in render.groups().iter().enumerate() {
                if upload_group(render, group_index, &[]).is_err() {
                    strict_refusals += 1;
                }
                let parts = upload_group_parts(render, group_index, &[])
                    .unwrap_or_else(|error| panic!("mesh {index} group {group_index}: {error}"));
                let triangles: usize = parts.iter().map(|part| part.report().triangles).sum();
                assert_eq!(
                    triangles,
                    source.triangles.len(),
                    "mesh {index} group {group_index}: the split must keep every stored triangle"
                );
                if parts.iter().any(|part| part.part() != GroupPart::Whole) {
                    split_groups += 1;
                }
            }
        }
    }
    assert_eq!(meshes, 24, "three representatives in each of eight groups");
    assert!(
        split_groups > 0 && split_groups == strict_refusals,
        "every group the strict adapter refused splits, and only those: {split_groups} split, \
         {strict_refusals} refused strictly"
    );
    eprintln!("F17-G: {meshes} representatives, {split_groups} material groups split");
}

/// **F18-E over the real installation**: every world group reports a decoded
/// placement and the measured metre, and the two traversal blockers F18-D named
/// are gone because the facts exist, not because they were dropped.
///
/// What stays open is stated rather than hidden: four of the five opening
/// classes and every traversal route are measured to be **absent** from the
/// corpus, so each group reports one `NoRouteInMeasuredCorpus` gap naming the
/// missions the absence affects, every unlocated class carries the measurement
/// behind it, and a route claimed without its two facts is still
/// `RouteWithoutFacts`. The `accept_f18_e1_` suite below pins that opening rule
/// itself.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_e_retail_placement_is_decoded_and_the_scale_is_the_measured_metre() {
    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"));
    let survey =
        survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");
    let report = audit_survey(&survey).expect("the audit runs over the survey");
    assert_eq!(report.groups().len(), 8);
    let placed: BTreeMap<String, usize> = report
        .visited()
        .map(|audit| {
            let census = audit.census().expect("visited");
            let PlacementSource::Decoded { placed_objects } = census.placement() else {
                panic!("{}: placement must be decoded", audit.group().world());
            };
            assert_eq!(census.vertex_scale_to_m(), Some(1.0));
            assert!(audit.traversal_blockers().is_empty());
            (audit.group().world().key().to_owned(), placed_objects)
        })
        .collect();
    assert_eq!(placed.len(), 8, "{placed:?}");
    let expected: BTreeMap<String, usize> = [
        ("c1", 3966),
        ("c1b", 3485),
        ("c1c", 3354),
        ("c2", 2558),
        ("c2b", 3039),
        ("c3", 2868),
        ("c4", 4929),
        ("c5", 6003),
    ]
    .into_iter()
    .map(|(group, count)| (group.to_owned(), count))
    .collect();
    assert_eq!(placed, expected, "mesh-naming node records per group");
    assert_eq!(
        report.gap_count(),
        8,
        "one NoRouteInMeasuredCorpus gap per group"
    );
    assert!(
        report
            .groups()
            .iter()
            .all(|audit| audit.gaps().iter().all(|gap| matches!(
                gap,
                cs_content::world::WorldAuditGap::NoRouteInMeasuredCorpus { .. }
            ))),
        "the shortfall gap is replaced by the measured one for every group the rule covered"
    );
    assert!(!report.is_complete());
}

// ------------------------------------- F18-E.1: the measured opening rule ---

/// One authored instant-action fly-through target resolved to one authored
/// world box — the shape F42-D's survey produces for each of its 54 rows, so a
/// synthetic test can drive the **production** classification
/// ([`locate_stunt_passages`]) without the installation.
///
/// `world_zone` is `None` for a target whose own scenario never bound its
/// label, which F42-D reports as a gap; such a row must contribute no opening.
fn synthetic_fly_through_gate(
    world: &str,
    label: &str,
    world_zone: Option<&str>,
    mesh_index: Option<i32>,
    corners: ([f64; 3], [f64; 3]),
) -> RetailStuntGate {
    let id = WorldId::from_key(world).expect("a valid world key");
    let geometry = mesh_index.map(|index| {
        let volume = StoredVolume::new(corners.0, corners.1).expect("an authored box");
        RetailTriggerVolume::new(
            TriggerVolumeSpan::new(
                id.clone(),
                format!("zbd/{world}/gamez.zbd"),
                hash(&format!("{world}-container")).to_hex(),
                12,
                4_096,
                144,
            ),
            world_zone.unwrap_or("dzpath?").to_owned(),
            Some(index),
            volume,
        )
    });
    let target = ScenarioFlyThroughTarget {
        zone_label: label.to_owned(),
        description: "MSG_OBJ_DESCRIPTION_1".to_owned(),
        category_label: "MSG_OBJ_DZ".to_owned(),
        help_label: "MSG_OBJ_FLYTHROUGH".to_owned(),
    };
    RetailStuntGate::new(
        id,
        "stunt_flying",
        label,
        world_zone.map(str::to_owned),
        &target,
        StuntEncodingSpan::new(
            format!("zbd/{world}/ia1/zrdr.zbd"),
            hash("scenario").to_hex(),
            "targets.zrd",
            64,
            128,
        ),
        geometry,
    )
}

/// **Task #732's opening rule, on evidence this test writes down.**
///
/// Three things a reviewer has to be able to see failing:
///
/// 1. the **classification** itself — [`locate_stunt_passages`] turns an
///    authored fly-through target resolved to an authored world box into a
///    `StuntOpening` whose mesh index is the box's and whose clearance is the
///    box's *narrowest stored extent times the caller's measured unit*. Remove
///    the box→clearance conversion, or classify by something other than the
///    objective's own declaration, and the numbers here stop matching;
/// 2. the **gap the measured route search produces** —
///    `NoRouteInMeasuredCorpus`, naming the missions it affects and citing the
///    route carrier and the task that owns its unmeasured encoding. Drop the
///    `RouteSearch` consultation and this group falls back to the old shortfall
///    gap, which is asserted *not* to be there;
/// 3. the **never-silent** property — a class the survey searched and found
///    nothing of carries the survey's own measurement, and a census that
///    searched for nothing still carries `OPENING_SEARCH_UNSUPPLIED` rather
///    than an empty string; a blank measurement is refused at construction.
#[test]
fn accept_f18_e1_a_located_stunt_passage_and_an_absent_route_carry_their_measurement() {
    // --- 1. the classification, over authored evidence -----------------------
    let world = WorldId::from_key("c1").expect("a valid world key");
    // Stored corners: extents 8 x 5.5 x 19, so the narrowest is 5.5 units.
    let resolved = synthetic_fly_through_gate(
        "c1",
        "dz1",
        Some("dzpath1"),
        Some(949),
        ([-10.0, 4.0, -20.0], [-2.0, 9.5, -1.0]),
    );
    // A target whose scenario never bound its label: F42-D reports it as a gap,
    // and the classification must produce no opening from it.
    let unbound = synthetic_fly_through_gate("c1", "sghangar", None, None, ([0.0; 3], [1.0; 3]));
    // Another group's target: the classification is per world.
    let other = synthetic_fly_through_gate(
        "c5",
        "dz2",
        Some("dzpath2"),
        Some(7),
        ([0.0, 0.0, 0.0], [4.0, 4.0, 4.0]),
    );
    let stunts =
        RetailStuntEncodingSurvey::new(hash("install").to_hex(), vec![resolved, unbound, other]);

    let located = locate_stunt_passages(&world, &stunts, 2.0);
    assert_eq!(
        located,
        vec![StuntOpening {
            class: OpeningClass::StuntPassage,
            mesh_index: 949,
            clearance_m: Some(11.0),
        }],
        "the narrowest stored extent (5.5) times the caller's measured unit (2.0) is the \
         clearance, and the box's own mesh binding is the opening's mesh"
    );
    assert!(
        locate_stunt_passages(
            &WorldId::from_key("c5").expect("a valid world key"),
            &stunts,
            2.0
        )
        .len()
            == 1,
        "the other group locates its own target and not this one's"
    );

    // --- 2. the measured verdicts the audit derives from them ---------------
    let unlocated: Vec<UnlocatedOpening> = OpeningClass::ALL
        .iter()
        .filter(|class| **class != OpeningClass::StuntPassage)
        .map(|class| {
            UnlocatedOpening::new(*class, class_absent(*class)).expect("a non-empty measurement")
        })
        .collect();
    assert_eq!(
        unlocated.len(),
        4,
        "four classes the corpus states nothing about"
    );
    assert!(
        unlocated.iter().all(|row| {
            row.measured()
                .contains(OPENING_CLASS_ABSENT_FROM_THE_CORPUS)
                && row.measured().contains(row.class().code())
        }),
        "every absence quotes the shared measurement and names its own class"
    );

    let census = synthetic_census(
        "c1",
        40,
        PlacementSource::Decoded { placed_objects: 38 },
        Some(2.0),
        Vec::new(),
        located,
        unlocated,
        RouteSearch::Unstated {
            measured: route_absent(&world),
        },
    );
    let declared = WorldGroupAudit::new(vec![synthetic_group("c1")]).expect("one group");
    let report = declared.audit(|_| Ok(census.clone()));
    let verdict = &report.groups()[0];

    assert_eq!(
        verdict.gaps(),
        &[WorldAuditGap::NoRouteInMeasuredCorpus {
            world: world.clone(),
            located_openings: 1,
            affected: vec!["M01".to_owned(), "M02".to_owned()],
            measured: route_absent(&world),
        }],
        "the measured route search names what was searched, what it located and which missions \
         the absent route affects"
    );
    assert!(
        !report.groups().iter().any(|audit| audit
            .gaps()
            .iter()
            .any(|gap| matches!(gap, WorldAuditGap::NoRouteMeasured { .. }))),
        "a group the rule covered no longer reports the shortfall gap"
    );
    assert_eq!(
        report.routed().count(),
        0,
        "no route is stated, so nothing counts as routed"
    );
    assert!(!report.is_complete(), "four classes stay unlocated");

    let opening_rows = verdict.openings();
    assert_eq!(opening_rows.len(), OpeningClass::ALL.len());
    for row in opening_rows {
        if row.located().is_empty() {
            assert_eq!(row.unlocated().len(), 1);
            let reason = row.unlocated()[0].measured();
            assert!(
                !reason.trim().is_empty() && reason != OPENING_SEARCH_UNSUPPLIED,
                "a class this survey searched carries the survey's measurement, got {reason:?}"
            );
        } else {
            assert_eq!(row.located()[0].class, OpeningClass::StuntPassage);
            assert!(row.unlocated().is_empty());
        }
    }

    // --- 3. a census that searched for nothing is still never silent --------
    let unsought = synthetic_census(
        "c2",
        9,
        PlacementSource::Decoded { placed_objects: 7 },
        Some(1.0),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        RouteSearch::default(),
    );
    let declared = WorldGroupAudit::new(vec![synthetic_group("c2")]).expect("one group");
    let report = declared.audit(|_| Ok(unsought.clone()));
    assert_eq!(
        report.groups()[0].gaps(),
        &[WorldAuditGap::NoRouteMeasured {
            world: WorldId::from_key("c2").expect("a valid world key"),
            placed_objects: 7,
        }],
        "with no measured route search the shortfall gap is still the honest verdict"
    );
    for row in report.groups()[0].openings() {
        assert_eq!(row.unlocated().len(), 1, "every class is visited");
        assert_eq!(
            row.unlocated()[0].measured(),
            OPENING_SEARCH_UNSUPPLIED,
            "an unlocated class whose search never ran says exactly that"
        );
    }

    // A blank measurement is refused, so "unlocated and silent" is not a state
    // a census can be built in.
    assert_eq!(
        UnlocatedOpening::new(OpeningClass::Hangar, "   ").err(),
        Some(WorldAuditError::BlankOpeningReason {
            class: "hangar".to_owned(),
        })
    );
    assert!(
        UnlocatedOpening::new(OpeningClass::Hangar, class_absent(OpeningClass::Hangar)).is_ok(),
        "a real measurement is accepted"
    );

    // --- the contradiction arm still wins -----------------------------------
    let contradicted = synthetic_census(
        "c3",
        5,
        PlacementSource::Undecoded {
            stored_node_records: 100,
            nodes_offset: 4_096,
        },
        None,
        vec![TraversalRoute {
            route: "invented".to_owned(),
            from_m: [0.0; 3],
            to_m: [1.0; 3],
            clearance_m: None,
            openings: Vec::new(),
        }],
        Vec::new(),
        Vec::new(),
        RouteSearch::Unstated {
            measured: ROUTE_ABSENT_FROM_THE_CORPUS.to_owned(),
        },
    );
    let declared = WorldGroupAudit::new(vec![synthetic_group("c3")]).expect("one group");
    let report = declared.audit(|_| Ok(contradicted.clone()));
    assert_eq!(
        report.groups()[0].gaps(),
        &[WorldAuditGap::RouteWithoutFacts {
            world: WorldId::from_key("c3").expect("a valid world key"),
            routes: 1,
            openings: 0,
        }],
        "a route claimed while the facts it needs are missing is still RouteWithoutFacts, \
         whatever the route search measured"
    );
}

/// **F18-E.1 over the real installation.** The opening rule applied to all
/// eight groups: stunt passages located from each group's own instant-action
/// declarations (54 across six groups, `c1c` and `c2b` author none), every
/// other class unlocated **with the measurement that says the corpus holds none
/// of it**, one measured route gap per group naming the missions it affects,
/// and no shortfall gap anywhere.
///
/// Ignored because CI has no original data; run it with `--include-ignored`
/// beside the other retail tests.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_e1_retail_stunt_passages_are_located_and_every_absence_is_measured() {
    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"));
    let survey =
        survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");
    let report = audit_survey(&survey).expect("the audit runs over the survey");

    // F42-D's own measurement of the eight instant-action scenarios (task #463):
    // the fly-through danger-zone targets each group's `targets.zrd` declares.
    let expected: BTreeMap<&str, usize> = [
        ("c1", 5_usize),
        ("c1b", 5),
        ("c1c", 0),
        ("c2", 9),
        ("c2b", 0),
        ("c3", 4),
        ("c4", 14),
        ("c5", 17),
    ]
    .into_iter()
    .collect();
    let stunts = survey.stunts().expect("the stunt encoding was measured");

    let mut located_total = 0_usize;
    for verdict in report.visited() {
        let world = verdict.group().world();
        let group = survey.group(world).expect("the surveyed row");
        let container = group.container().expect("every group's container reads");
        let expected_here = *expected
            .get(world.key())
            .expect("the installation's eight groups are pinned");
        assert_eq!(
            locate_stunt_passages(world, stunts, container.vertex_scale_to_m).len(),
            expected_here,
            "{world}: the classification over the measured survey finds this group's own \
             fly-through targets"
        );

        let mut located_here = 0_usize;
        for row in verdict.openings() {
            if row.located().is_empty() {
                assert_eq!(row.unlocated().len(), 1, "{world}: one reason per class");
                let reason = row.unlocated()[0].measured();
                assert!(
                    !reason.trim().is_empty(),
                    "{world}: {} is unlocated with no measurement — the silent zero the audit \
                     exists to prevent",
                    row.unlocated()[0].class()
                );
                assert_ne!(
                    reason, OPENING_SEARCH_UNSUPPLIED,
                    "{world}: the retail survey searched every class"
                );
                match row.unlocated()[0].class() {
                    OpeningClass::StuntPassage => {
                        assert!(
                            expected_here == 0 && reason.contains("declares 0 fly-through"),
                            "{world}: a group that authors no fly-through target says so from its \
                             own scenario, got {reason:?}"
                        );
                        assert!(
                            reason.contains(&format!("zbd/{}/ia1/zrdr.zbd", world.key())),
                            "{world}: the reason names this group's own scenario container, got \
                             {reason:?}"
                        );
                    }
                    class => assert_eq!(
                        reason,
                        class_absent(class),
                        "{world}: {class} carries the shared corpus-absence measurement"
                    ),
                }
                continue;
            }
            assert_eq!(
                row.located()[0].class,
                OpeningClass::StuntPassage,
                "{world}: the only class the measured rule locates"
            );
            assert_eq!(
                row.located().len(),
                expected_here,
                "{world}: counted twice?"
            );
            located_here += row.located().len();
            for opening in row.located() {
                let clearance = opening
                    .clearance_m
                    .unwrap_or_else(|| panic!("{world}: a measured clearance"));
                assert!(
                    clearance.is_finite() && clearance > 0.0,
                    "{world}: the clearance is the zone box's narrowest stored extent times the \
                     measured metre, got {clearance}"
                );
                assert!(
                    (opening.mesh_index as usize) < container.meshes.meshes.len(),
                    "{world}: mesh {} the container stores (it holds {} slots)",
                    opening.mesh_index,
                    container.meshes.meshes.len()
                );
                assert!(
                    container.meshes.meshes[opening.mesh_index as usize].is_some(),
                    "{world}: mesh {} the located opening names is a stored mesh, not an absent \
                     slot",
                    opening.mesh_index
                );
            }
        }
        assert_eq!(located_here, expected_here, "{world}: located count");
        located_total += located_here;

        assert_eq!(
            verdict.gaps(),
            &[WorldAuditGap::NoRouteInMeasuredCorpus {
                world: world.clone(),
                located_openings: expected_here,
                affected: verdict.group().missions().to_vec(),
                measured: route_absent(world),
            }],
            "{world}: one measured route gap naming this group's missions and the evidence"
        );
        assert!(
            verdict
                .gaps()
                .iter()
                .all(|gap| !matches!(gap, WorldAuditGap::NoRouteMeasured { .. })),
            "{world}: the shortfall gap is gone for every group the rule covered"
        );
    }
    assert_eq!(located_total, 54, "F42-D's measured corpus total");
    assert_eq!(
        report.gap_count(),
        8,
        "one measured route gap per group, no shortfall gap"
    );
    assert!(
        !report.is_complete(),
        "four classes and every route stay open"
    );
}
