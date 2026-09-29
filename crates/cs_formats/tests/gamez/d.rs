//! Acceptance stage F10-D: the exact missing/invalid face census of every
//! private world and airframe
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, section
//! `### F10-D`, AC04 and the failure cases a report implies).
//!
//! The production code under test is
//! [`cs_formats::gamez::FaceCensus`] — built through
//! [`GameZMeshes::face_census`], which walks the same
//! [`RawMesh::topology`] gate the upload path walks. The synthetic tests fix
//! what each word of the report means (a face the section never held, a broken
//! face, a face this triangulator refuses, a face that decodes to nothing); the
//! retail test then reports those exact counts for **every** world group and
//! for `planes.zbd` of the original installation, with the archive list derived
//! from production discovery rather than assumed.
//!
//! Nothing in this file is original game content: the retail half asserts
//! counts, mesh array indices and polygon indices — never a coordinate, a
//! texture name or a byte of the archives — and it is `#[ignore]`d so CI (which
//! has no installation) skips it. It fails loudly rather than skipping when
//! `CS_GAME_DIR` is absent.

use std::collections::BTreeMap;

use cs_formats::ParseContext;
use cs_formats::gamez::reader::{Fixup, RawMeshInfo};
use cs_formats::gamez::{
    FaceStatus, GameZHeader, GameZMesh, GameZMeshes, MeshIndex, MissingFace, MissingFaceReason,
    PrimitiveKind, RawCorner, RawMesh, RawPolygon, read_gamez_meshes,
};

// ------------------------------------------------------------- synthetic ---

/// A present mesh record: `parent_count` non-zero is what marks it present.
fn info(polygon_count: u32) -> RawMeshInfo {
    RawMeshInfo {
        file_ptr: 1,
        unk04: 0,
        unk08: 0,
        parent_count: 1,
        polygon_count,
        vertex_count: 0,
        normal_count: 0,
        morph_count: 0,
        light_count: 0,
        unk36: 0,
        unk40: 0.0,
        unk44: 0.0,
        unk48: 0,
        polygons_ptr: 0,
        vertices_ptr: 0,
        normals_ptr: 0,
        lights_ptr: 0,
        morphs_ptr: 0,
        unk72: 0.0,
        unk76: 0.0,
        unk80: 0.0,
        unk84: 0.0,
        unk88: 0,
        material_count: 0,
        materials_ptr: 0,
    }
}

fn corner(position: u32) -> RawCorner {
    RawCorner {
        position,
        normal: None,
        uv: None,
        color: None,
    }
}

fn polygon(kind: PrimitiveKind, corners: Vec<RawCorner>) -> RawPolygon {
    RawPolygon {
        kind,
        raw_flags: 0,
        material: 0,
        corners,
    }
}

/// A container of `meshes`, with the remaining slots absent. `declared` is the
/// `polygon_count` stored on each present record, so a record can declare more
/// faces than the section held — the shortfall the report has to name.
fn container(
    meshes: Vec<(u32, u32, Vec<RawPolygon>)>,
    slots: usize,
    declared: &[u32],
) -> GameZMeshes {
    let mut entries: Vec<Option<GameZMesh>> = Vec::new();
    for (position, (index, declared_count, polygons)) in meshes.into_iter().enumerate() {
        let count = declared.get(position).copied().unwrap_or(declared_count);
        entries.push(Some(GameZMesh {
            index,
            info: info(count),
            mesh: RawMesh {
                positions: vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [1.0, 1.0, 0.0],
                ],
                normals: vec![[0.0, 0.0, 1.0]],
                polygons,
            },
            polygon_records: Vec::new(),
            lights: Vec::new(),
            morphs: Vec::new(),
            materials: Vec::new(),
            material_groups: Vec::new(),
            data_offset: 0,
            data_end: 0,
        }));
    }
    entries.resize(slots, None);
    GameZMeshes {
        header: GameZHeader {
            signature: 0,
            version: 42,
            unk08: 0,
            texture_count: 0,
            textures_offset: 40,
            materials_offset: 0,
            meshes_offset: 0,
            node_array_size: 0,
            light_index: 0,
            nodes_offset: 0,
        },
        index: MeshIndex {
            array_size: slots as i32,
            count: entries.iter().flatten().count() as i32,
            last_index: -1,
        },
        fixup: Fixup::None,
        meshes: entries,
        findings: Vec::new(),
        unchecked_material_references: 0,
        data_offset: 0,
        data_end: 0,
    }
}

/// **AC04's three rejection words, on a container that fails all of them.**
/// One mesh holds a stored face whose data is broken, an outline this
/// triangulator refuses, and a face that decodes to triangles which all
/// collapse; a second mesh is fine and one slot is an absent stub. Every count
/// and every listed face is asserted, so an aggregation that drops a face,
/// counts it twice, or files a refusal under "invalid" fails here rather than
/// in the report. (The fourth word — a face the records declared and the
/// section never held — is the next test.)
#[test]
fn accept_f10_d_census_names_every_missing_face_with_its_exact_reason() {
    let square = |positions: &[u32]| {
        polygon(
            PrimitiveKind::TriangleStrip,
            positions.iter().copied().map(corner).collect(),
        )
    };
    let outline = |corners: Vec<u32>| {
        polygon(
            PrimitiveKind::Polygon,
            corners.into_iter().map(corner).collect(),
        )
    };
    // Mesh 0: a decoded strip, a face whose position index is past the four
    // positions (invalid), a bow-tie outline whose two lobes cancel (valid
    // data, refused: `zero_area`), and a strip whose every step repeats an
    // index (decoded, but nothing draws).
    let first = vec![
        square(&[0, 1, 2, 3]),
        square(&[0, 1, 4]),
        outline(vec![0, 1, 2, 3]),
        square(&[2, 2, 2]),
    ];
    // Mesh 1: two faces, both decoded and both drawing.
    let second = vec![square(&[0, 1, 2]), outline(vec![0, 1, 2])];
    let meshes = container(vec![(0, 4, first), (1, 2, second)], 3, &[]);
    let census = meshes.face_census();

    assert_eq!(census.slots, 3, "two meshes and one stub slot");
    assert_eq!(census.present_meshes, 2);
    assert_eq!(census.absent_meshes, 1, "the stub is counted, not skipped");
    assert_eq!(
        census.declared_faces, 6,
        "both records declare exactly what they hold"
    );
    assert_eq!(census.stored_faces, 6, "the section held six");
    assert_eq!(census.shortfall_faces, 0);
    assert_eq!(census.decoded_faces, 4);
    assert_eq!(census.triangles, 5, "two strip triangles, then one each");
    assert_eq!(
        census.degenerate_triangles, 1,
        "only the all-repeating strip collapses"
    );
    assert_eq!(census.drawn_triangles(), 4);
    assert_eq!(census.invalid_faces, 1, "only the out-of-range face");
    assert_eq!(
        census.unsupported_faces, 1,
        "the bow tie is refused, never fanned and never called invalid"
    );
    assert_eq!(
        census.degenerate_only_faces, 1,
        "the collapsed strip decoded and still draws nothing"
    );
    assert_eq!(
        census.missing.len(),
        3,
        "one entry per face that draws nothing"
    );
    assert_eq!(
        census.missing_faces(),
        3,
        "missing is the shortfall plus exactly those three"
    );
    assert!(!census.is_complete());

    assert_eq!(
        census.missing,
        vec![
            MissingFace {
                mesh: 0,
                polygon: 1,
                corners: 3,
                reason: MissingFaceReason::Invalid("position_index_out_of_range"),
            },
            MissingFace {
                mesh: 0,
                polygon: 2,
                corners: 4,
                reason: MissingFaceReason::Unsupported("zero_area"),
            },
            MissingFace {
                mesh: 0,
                polygon: 3,
                corners: 3,
                reason: MissingFaceReason::DegenerateOnly,
            },
        ],
        "mesh, polygon, corner count and reason, in stored order"
    );
    assert!(census.missing[0].reason.is_invalid());
    assert!(!census.missing[1].reason.is_invalid());
    assert_eq!(census.missing[1].reason.code(), "zero_area");
    assert_eq!(census.missing[2].reason.code(), "degenerate_only");
    // Only mesh 0 lost a *rejected* face, so only mesh 0 is refused by the
    // render gate; mesh 1's faces are untouched.
    assert_eq!(census.rejected_meshes(), vec![0]);

    // The counts add up to what was stored, exactly once each.
    assert_eq!(
        census.decoded_faces + census.invalid_faces + census.unsupported_faces,
        census.stored_faces
    );
    assert_eq!(
        census.missing_faces(),
        census.shortfall_faces + census.missing.len() as u64
    );
}

/// **The report's failure case: a face the records declared and the section
/// never held.** A container that reads cleanly cannot produce this shortfall,
/// so the number only means something if it is counted from the record's own
/// `polygon_count`; a census that trusted the walk would report zero missing
/// faces for a container that silently lost two.
#[test]
fn accept_f10_d_census_counts_a_face_the_section_never_held() {
    let face = polygon(
        PrimitiveKind::TriangleStrip,
        vec![corner(0), corner(1), corner(2)],
    );
    // The record declares five, the section holds one: two of the five are
    // never read at all.
    let meshes = container(vec![(0, 1, vec![face.clone()])], 2, &[5]);
    let census = meshes.face_census();

    assert_eq!(census.declared_faces, 5);
    assert_eq!(census.stored_faces, 1);
    assert_eq!(census.shortfall_faces, 4, "five declared, one held");
    assert_eq!(census.decoded_faces, 1, "the face that was held decodes");
    assert_eq!(
        census.missing_faces(),
        4,
        "the faces the section never held are missing, and they are counted"
    );
    assert!(
        census.missing.is_empty(),
        "a shortfall cannot be attributed to a stored polygon index, so it is \
         a count and never an invented entry"
    );
    assert!(!census.is_complete());
    assert!(
        census.rejected_meshes().is_empty(),
        "nothing was rejected: the mesh built from what it held"
    );

    // The complete contrast: the same mesh declaring exactly what it holds
    // reports nothing missing.
    let complete = container(vec![(0, 1, vec![face])], 1, &[1]);
    let census = complete.face_census();
    assert_eq!(census.shortfall_faces, 0);
    assert_eq!(census.missing_faces(), 0);
    assert!(census.is_complete());
    assert!(census.drawn_triangles() > 0);
    assert_eq!(
        census.triangles,
        census.drawn_triangles() + census.degenerate_triangles
    );
}

// ----------------------------------------------------------------- retail ---

/// The archives the report covers: every world group discovery observed, plus
/// the airframe container. Derived from production discovery so a new world
/// group is reported rather than silently missing from the corpus.
fn retail_archives() -> Vec<(String, &'static str)> {
    let game_dir = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect(
        "CS_GAME_DIR must point at the original installation: this test reports the face \
         census of real archives and cannot pass without them",
    ));
    let found = cs_assets::install::discover(&game_dir)
        .expect("production discovery must read the original installation");
    let mut archives: Vec<(String, &'static str)> = found
        .diagnosis
        .world_groups
        .iter()
        .map(|group| (format!("{}/gamez.zbd", group.as_str()), "world"))
        .collect();
    archives.sort();
    let planes = found
        .diagnosis
        .planes_zbd
        .clone()
        .unwrap_or_else(|| panic!("discovery must observe the airframe container"));
    archives.push((planes.as_str().to_owned(), "airframe"));
    assert!(
        !archives.is_empty(),
        "the installation has at least one world and the airframes"
    );
    archives
}

fn read_retail(relative: &str) -> GameZMeshes {
    let game_dir = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let path = game_dir.join(relative);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!("{relative}: every world and airframe must be readable: {error}")
    });
    let mut context = ParseContext::with_defaults(relative);
    read_gamez_meshes(&mut context, relative, &bytes)
        .unwrap_or_else(|error| panic!("{relative}: the retail mesh section must read: {error}"))
}

/// `(missing, invalid)` as measured on this installation and pinned here. The
/// two numbers are AC04's subject: a corpus where a face stops drawing, or
/// starts being called invalid, has to change this table on purpose.
const EXPECTED_MISSING_AND_INVALID: [(&str, u64, u64); 9] = [
    ("ZBD/planes.zbd", 0, 0),
    ("ZBD/C1/gamez.zbd", 0, 0),
    ("ZBD/C1B/gamez.zbd", 0, 0),
    ("ZBD/C1C/gamez.zbd", 0, 0),
    ("ZBD/C2/gamez.zbd", 0, 0),
    ("ZBD/C2B/gamez.zbd", 0, 0),
    ("ZBD/C3/gamez.zbd", 0, 0),
    ("ZBD/C4/gamez.zbd", 3, 0),
    ("ZBD/C5/gamez.zbd", 8, 0),
];

/// Every face of the corpus that draws nothing, as `(archive, mesh, polygon,
/// reason code)`. Eleven entries: three in C4 (one refused outline and two
/// faces whose triangles all collapse) and eight refused outlines in C5.
/// These are stored indices and stable codes — never a coordinate.
const EXPECTED_MISSING_FACES: [(&str, u32, usize, &str); 11] = [
    ("ZBD/C4/gamez.zbd", 191, 20, "coincident_corners"),
    ("ZBD/C4/gamez.zbd", 191, 55, "degenerate_only"),
    ("ZBD/C4/gamez.zbd", 192, 142, "degenerate_only"),
    ("ZBD/C5/gamez.zbd", 370, 1, "coincident_corners"),
    ("ZBD/C5/gamez.zbd", 970, 0, "self_intersecting"),
    ("ZBD/C5/gamez.zbd", 973, 0, "self_intersecting"),
    ("ZBD/C5/gamez.zbd", 974, 0, "self_intersecting"),
    ("ZBD/C5/gamez.zbd", 978, 0, "self_intersecting"),
    ("ZBD/C5/gamez.zbd", 980, 0, "self_intersecting"),
    ("ZBD/C5/gamez.zbd", 981, 0, "self_intersecting"),
    ("ZBD/C5/gamez.zbd", 982, 0, "self_intersecting"),
];

/// **AC04: every private world and airframe, exact missing and invalid face
/// counts.**
///
/// For each archive production discovery names — the eight world groups and
/// `planes.zbd` — the production reader parses it and the production census
/// reports it. The assertions are of three kinds, so the test cannot pass by
/// repeating numbers it read:
///
/// * **invariants recomputed from the parsed container**: every declared face
///   is accounted for exactly once (`decoded + invalid + unsupported ==
///   stored`, and `stored == declared` for a container that read), the report's
///   list matches its own counts, and each listed face is checked back against
///   that mesh's own topology status and corner count;
/// * **the pinned counts** of missing and invalid faces per archive, which is
///   the report AC04 asks for;
/// * **the pinned identities** of every face that draws nothing, so a census
///   that reports the right total from the wrong faces fails.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f10_d_retail_every_world_and_airframe_reports_exact_missing_and_invalid_face_counts() {
    let archives = retail_archives();
    let mut covered: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    let mut reported: Vec<(String, u32, usize, String)> = Vec::new();
    let mut total_missing = 0u64;
    let mut total_invalid = 0u64;

    for (relative, kind) in &archives {
        let parsed = read_retail(relative);
        let census = parsed.face_census();

        // Nothing is skipped and nothing is double counted: the records
        // declare exactly what the section held, and every held face has a
        // status.
        assert_eq!(
            census.declared_faces, census.stored_faces,
            "{relative}: declared vs stored"
        );
        assert_eq!(
            census.decoded_faces + census.invalid_faces + census.unsupported_faces,
            census.stored_faces,
            "{relative}: every stored face is counted exactly once"
        );
        assert_eq!(
            census.missing_faces(),
            census.shortfall_faces + census.missing.len() as u64,
            "{relative}: the report's list and its counts agree"
        );
        assert_eq!(
            census.triangles,
            census.drawn_triangles() + census.degenerate_triangles,
            "{relative}"
        );
        assert_eq!(
            census.present_meshes + census.absent_meshes,
            census.slots,
            "{relative}: one slot is present or absent"
        );
        assert!(
            census.stored_faces > 0,
            "{relative}: the corpus is not empty"
        );

        // Each listed face is checked back against its own mesh's topology,
        // so a census entry that names a face which decodes (or a corner count
        // the polygon does not have) fails.
        for face in &census.missing {
            let mesh = parsed
                .get(face.mesh)
                .unwrap_or_else(|| panic!("{relative}: mesh {} is present", face.mesh));
            let polygon = &mesh.mesh.polygons[face.polygon];
            assert_eq!(
                polygon.corners.len(),
                face.corners,
                "{relative}: mesh {} polygon {}",
                face.mesh,
                face.polygon
            );
            let status = &mesh.topology().faces[face.polygon];
            match (status, &face.reason) {
                (FaceStatus::Rejected(_), MissingFaceReason::Invalid(_))
                | (FaceStatus::Rejected(_), MissingFaceReason::Unsupported(_)) => {}
                (FaceStatus::Decoded { .. }, MissingFaceReason::DegenerateOnly) => {
                    let FaceStatus::Decoded {
                        triangles,
                        degenerate,
                    } = status
                    else {
                        unreachable!()
                    };
                    assert_eq!(
                        triangles, degenerate,
                        "{relative}: mesh {} polygon {} draws nothing",
                        face.mesh, face.polygon
                    );
                }
                (status, reason) => panic!(
                    "{relative}: mesh {} polygon {} is {status:?} but reported as {:?}",
                    face.mesh,
                    face.polygon,
                    reason.code()
                ),
            }
            reported.push((
                relative.clone(),
                face.mesh,
                face.polygon,
                face.reason.code().to_owned(),
            ));
        }

        let expected = EXPECTED_MISSING_AND_INVALID
            .iter()
            .find(|(name, _, _)| name == relative)
            .unwrap_or_else(|| panic!("{relative}: the pinned report has a row for every archive"));
        assert_eq!(
            (census.missing_faces(), census.invalid_faces),
            (expected.1, expected.2),
            "{relative}: exact missing/invalid counts (AC04); unsupported {} \
             degenerate-only {} shortfall {}",
            census.unsupported_faces,
            census.degenerate_only_faces,
            census.shortfall_faces
        );
        covered.insert(
            relative.as_str(),
            (census.missing_faces(), census.invalid_faces),
        );

        println!(
            "F10-D census {relative} ({kind}): slots {} present {} absent {} declared {} \
             stored {} missing {} invalid {} unsupported {} degenerate-only {} triangles {} \
             (degenerate {})",
            census.slots,
            census.present_meshes,
            census.absent_meshes,
            census.declared_faces,
            census.stored_faces,
            census.missing_faces(),
            census.invalid_faces,
            census.unsupported_faces,
            census.degenerate_only_faces,
            census.triangles,
            census.degenerate_triangles,
        );
        total_missing += census.missing_faces();
        total_invalid += census.invalid_faces;
    }

    // Every pinned row was produced by an archive that really exists, and
    // every archive that exists has a pinned row: the report covers exactly
    // the private corpus, no row dropped and no row invented.
    for (name, missing, invalid) in EXPECTED_MISSING_AND_INVALID {
        let covered = covered
            .get(name)
            .unwrap_or_else(|| panic!("{name}: no archive was reported for this row"));
        assert_eq!(
            covered,
            &(missing, invalid),
            "{name}: reported counts differ from the pinned row"
        );
    }
    assert_eq!(
        archives.len(),
        EXPECTED_MISSING_AND_INVALID.len(),
        "the corpus is the eight world groups plus the airframes"
    );

    // Every face that draws nothing is named, and nothing else is.
    reported.sort();
    let mut expected: Vec<(String, u32, usize, String)> = EXPECTED_MISSING_FACES
        .iter()
        .map(|(archive, mesh, polygon, code)| {
            (archive.to_string(), *mesh, *polygon, code.to_string())
        })
        .collect();
    expected.sort();
    assert_eq!(
        reported, expected,
        "exactly these faces of the private corpus draw nothing"
    );
    assert_eq!(
        total_missing,
        EXPECTED_MISSING_FACES.len() as u64,
        "the identities and the totals are the same report"
    );
    assert_eq!(
        total_invalid, 0,
        "the measured corpus stores no invalid face"
    );

    // A world and the airframes are both in the report: AC04 names them
    // separately and the corpus must contain both.
    assert!(
        archives.iter().any(|(_, kind)| *kind == "world"),
        "a world is reported"
    );
    assert!(
        archives.iter().any(|(_, kind)| *kind == "airframe"),
        "the airframes are reported"
    );
}
