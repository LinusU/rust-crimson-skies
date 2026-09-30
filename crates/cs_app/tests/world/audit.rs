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
//! Three of the six tests read no original data and are not ignored, so CI runs
//! them: they pin the audit's *contract* — what it maps, what it reports as
//! missing, and what it refuses to construct — on synthetic fixtures. The
//! remaining three need capabilities CI does not have and are marked accordingly:
//! two need `CS_GAME_DIR` and one needs a GPU.

mod evidence;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cs_app::world::audit::{
    GEOMETRY_CONTAINER_FILE, REPRESENTATIVE_MESHES, TEXTURE_ARCHIVE_FILE, audit_survey,
    survey_world_groups,
};
use cs_app::world::gpu_capture::{CaptureRequest, capture_world_mesh};
use cs_content::mesh::RenderMesh;
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_content::world::{
    GroupFacts, OpeningClass, PlacementSource, RepresentativeGeometry, StuntOpening,
    StuntOpeningAudit, TraversalBlocker, TraversalRoute, WorldAuditError, WorldGroupAudit,
    WorldGroupCensus, WorldGroupRef, WorldId,
};
use cs_types::evidence::ContentHash;

// -------------------------------------------------------------- fixtures ---

/// The synthetic content every unignored test in this file is built from.
///
/// All **authored** values: the group keys, the counts, the extents. The
/// `PlacementSource::Undecoded` arm is the one the retail installation is
/// actually in, so a fixture that claimed a decoded placement would test an
/// arm no real installation reaches yet; the decoded arm is covered separately
/// by [`decoded_census`].
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
fn synthetic_census(
    key: &str,
    present: usize,
    placement: PlacementSource,
    vertex_scale_to_m: Option<f64>,
    routes: Vec<TraversalRoute>,
    openings: Vec<StuntOpening>,
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
    )
}

/// A SHA-256 over a label, so a fixture's fingerprints are distinct values with
/// no meaning beyond "these two are not the same bytes".
fn hash(label: &str) -> ContentHash {
    cs_assets::install::sha256(label.as_bytes())
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
    assert_eq!(report.visited().count(), 2, "one group had no geometry at all");
    assert_eq!(report.blocked().count(), 1);
    assert!(!report.is_empty(), "three groups were declared");
    assert!(
        !report.is_complete(),
        "a group with no geometry and a group with no placement keep the report from a pass"
    );
    assert_eq!(report.present_mesh_count(), 65, "40 + 25 stored meshes read");
    assert_eq!(report.drawn_triangle_count(), 2_400, "1 200 per visited group");

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
    assert_eq!(c1c.routes_measured(), true);
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
        classified.iter().flat_map(|audit| audit.unlocated()).count(),
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
    assert_eq!(c1.routes_measured(), false);
    assert!(c1.routes().is_empty());
    assert_eq!(c1.traversal_blockers().len(), 2, "both missing facts are named");
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
                text.contains("4,328 stored node records") || text.contains("4328 stored node records"),
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
    assert_eq!(c2b.openings().len(), 0, "a group with no census has no opening audit");
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

    // A census whose facts are all established and which still states no route.
    let clean = WorldGroupAudit::new(vec![synthetic_group("c2")]).expect("one group");
    let report = clean.audit(|_| {
        Ok(synthetic_census(
            "c2",
            9,
            PlacementSource::Decoded {
                placed_objects: 7,
            },
            Some(0.05),
            Vec::new(),
            Vec::new(),
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
                nodes_offset: 2
            },
            Some(bad),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .err()
        .expect("a non-finite scale must be refused, not stored");
        match refused {
            WorldAuditError::NonFiniteVertexScale { world: named, scale } => {
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
    // built, and the report says so.
    assert_eq!(capture.groups, 1, "one material group, one draw");
    assert!(capture.triangles >= 12, "{}", capture.triangles);
    assert!(capture.vertices >= 3, "{}", capture.vertices);
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
        [-2.0, 0.0, -0.5], [-2.0, 0.0, 0.5], [-2.0, 3.0, 0.5], [-2.0, 3.0, -0.5],
        // right leg
        [2.0, 0.0, -0.5], [2.0, 0.0, 0.5], [2.0, 3.0, 0.5], [2.0, 3.0, -0.5],
        // lintel, front and back faces
        [-2.0, 3.0, -0.5], [2.0, 3.0, -0.5], [2.0, 4.0, -0.5], [-2.0, 4.0, -0.5],
        [-2.0, 3.0, 0.5], [2.0, 3.0, 0.5], [2.0, 4.0, 0.5], [-2.0, 4.0, 0.5],
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
/// The traversal half of the acceptance scenario is where the honest verdict
/// lives: no production path decodes a GameZ node array, so **no** group can
/// locate an opening or measure a route, and the report says so per group with
/// the container's own `node_array_size` and `nodes_offset` quoted. This test
/// pins that verdict, so a future stage that decodes a node array has to change
/// the test — which is the point.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_d_retail_every_discovered_world_group_is_visited_and_compared() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"),
    );
    let survey = survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");

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
        "no group can state a traversal route while the node array is undecoded, so the audit is \
         explicitly not a pass — and never will be until a production path decodes it"
    );
    assert_eq!(report.routed().count(), 0);
    assert!(
        report.present_mesh_count() > 0,
        "the corpus holds stored meshes"
    );
    assert!(report.drawn_triangle_count() > 0);

    // Per group: measured geometry that agrees with the census, a traversal
    // verdict blocked by both missing facts, and all five opening classes
    // reported unlocated with the blockers that stopped them.
    let mut total_stored_nodes = 0_u32;
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
        for mesh in census.representative() {
            assert!(mesh.triangles > 0, "{}: a representative must draw", group.world());
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

        // Placement: undecoded, with the container header's own numbers.
        match census.placement() {
            PlacementSource::Undecoded {
                stored_node_records,
                nodes_offset,
            } => {
                assert!(
                    stored_node_records > 0,
                    "{}: a container with no stored node record holds no placed scene",
                    group.world()
                );
                assert!(nodes_offset > 0, "{}: the array starts somewhere", group.world());
                total_stored_nodes += stored_node_records;
            }
            PlacementSource::Decoded { .. } => panic!(
                "{}: this stage expects no decoded placement; a stage that decodes the node array \
                 must change this test and re-measure",
                group.world()
            ),
        }
        assert_eq!(
            census.vertex_scale_to_m(),
            None,
            "{}: the stored vertex unit is unmeasured and is reported as such",
            group.world()
        );

        // Traversal: both missing facts, named, and no route.
        assert_eq!(audit.routes().len(), 0);
        assert_eq!(audit.routes_measured(), false);
        let blockers = audit.traversal_blockers();
        assert_eq!(blockers.len(), 2, "{}: both facts are named", group.world());
        match &blockers[0] {
            TraversalBlocker::PlacementUndecoded { .. } => {}
            other => panic!("{}: the missing placement is named first, got {other:?}", group.world()),
        }
        match &blockers[1] {
            TraversalBlocker::VertexScaleUnmeasured {
                largest_stored_extent, ..
            } => assert!(
                *largest_stored_extent > 0.0,
                "{}: the scale blocker quotes a measured extent",
                group.world()
            ),
            other => panic!("{}: the unmeasured scale is named second, got {other:?}", group.world()),
        }

        // Openings: all five classes visited, none located, every one naming the
        // class it stands for.
        assert_eq!(audit.openings().len(), OpeningClass::ALL.len());
        for opening in audit.openings() {
            assert!(
                opening.located().is_empty(),
                "{}: no opening class may be located without a placement",
                group.world()
            );
            assert_eq!(opening.unlocated().len(), 1);
            assert!(
                OpeningClass::ALL.contains(&opening.unlocated()[0]),
                "{}: the unlocated class is one the sheet names",
                group.world()
            );
            assert!(!opening.is_complete());
        }
        assert_eq!(audit.gaps(), &[] as &[cs_content::world::WorldAuditGap]);
    }
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
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"),
    );
    let survey = survey_world_groups(&game_dir).expect("the installation is discovered and surveyed");
    assert_eq!(survey.groups.len(), 8, "every discovered world group is visited");

    let directory = evidence_dir();
    std::fs::create_dir_all(&directory).expect("the private evidence directory is writable");
    let mut captured = 0_usize;
    for group in &survey.groups {
        let container = group.container().expect("every group's container read");
        // The group's own largest stored mesh, by the same declared rule the
        // census uses, so the capture and the census name the same geometry.
        let (mesh_index, render) = container
            .representatives
            .iter()
            .max_by_key(|(_, render)| render.triangles().len())
            .map(|(index, render)| (*index, render))
            .unwrap_or_else(|| {
                panic!(
                    "{}: the survey chose no representative mesh to capture",
                    group.world()
                )
            });
        assert!(
            !render.triangles().is_empty(),
            "{}: a representative mesh must draw",
            group.world()
        );

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
        captured += 1;
    }
    assert_eq!(captured, 8, "one measured frame per discovered world group");
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
        render: &render,
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
