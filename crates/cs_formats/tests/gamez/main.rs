//! Acceptance stage F10-A: the lossless mesh IR and topology fixtures
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, section
//! `### F10-A`, AC01 plus the failure cases the scenario implies).
//!
//! Every value in this file is authored here: newly authored synthetic
//! content, no original game data, no `CS_GAME_DIR` access.
//!
//! The fixture is a unit square. Orientation is checked with a signed area
//! computed here from the positions, independently of the strip decoder, so
//! a decoder that forgets the odd-step swap or restarts parity after a
//! degenerate step produces a triangle with the opposite sign.
//!
//! Stage F10-B's n-gon triangulation tests are in [`ngon`], the established
//! CS GameZ layout and its reader are in [`reader`], and task F10-C.02's
//! texture-name table, material records and material-to-texture binding are in
//! [`materials`].

mod d;
mod materials;
mod ngon;
mod reader;

use cs_formats::gamez::{
    FaceIssue, FaceStatus, MIN_STRIP_INDICES, MeshTriangle, NgonIssue, PrimitiveKind, RawCorner,
    RawMesh, RawPolygon, StripError, decode_strip,
};

/// ```text
/// 2 (0,1) ---- 3 (1,1)
///   |        / |
///   |      /   |
/// 0 (0,0) ---- 1 (1,0)
/// ```
///
/// Strip `[0, 1, 2, 3]` covers the square with two counter-clockwise
/// triangles: `(0, 1, 2)` and `(2, 1, 3)`.
const SQUARE: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
];

/// Twice the signed area of a triangle in the z = 0 plane: positive for
/// counter-clockwise, negative for clockwise, zero for degenerate.
fn signed_area(positions: [u32; 3]) -> f32 {
    let [a, b, c] = positions.map(|index| SQUARE[index as usize]);
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
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

fn strip(positions: &[u32]) -> RawPolygon {
    polygon(
        PrimitiveKind::TriangleStrip,
        positions.iter().copied().map(corner).collect(),
    )
}

fn square_mesh(polygons: Vec<RawPolygon>) -> RawMesh {
    RawMesh {
        positions: SQUARE.to_vec(),
        normals: vec![[0.0, 0.0, 1.0]],
        polygons,
    }
}

fn drawn_positions(triangles: &[MeshTriangle]) -> Vec<[u32; 3]> {
    triangles
        .iter()
        .filter(|t| !t.is_degenerate())
        .map(|t| t.positions)
        .collect()
}

#[test]
fn accept_f10_a_strip_0123_keeps_one_orientation() {
    let steps = decode_strip(&[0, 1, 2, 3]).expect("four indices form a strip");
    let indices: Vec<[u32; 3]> = steps.iter().map(|t| t.indices).collect();
    assert_eq!(indices, [[0, 1, 2], [2, 1, 3]]);
    assert_eq!(steps[1].corners, [2, 1, 3], "odd step swaps its first two");
    for triangle in &steps {
        assert!(!triangle.is_degenerate());
        assert!(
            signed_area(triangle.indices) > 0.0,
            "step {} {:?} is clockwise",
            triangle.step,
            triangle.indices
        );
    }

    let topology = square_mesh(vec![strip(&[0, 1, 2, 3])]).topology();
    assert_eq!(drawn_positions(&topology.triangles), indices);
    assert_eq!(
        topology.faces,
        [FaceStatus::Decoded {
            triangles: 2,
            degenerate: 0
        }]
    );
    assert!(topology.is_complete());
}

#[test]
fn accept_f10_a_degenerate_insertion_still_advances_parity() {
    // A repeated index inserted mid-strip adds one degenerate step. The
    // step after it is even again and draws the same triangle as step 1 of
    // the plain strip; a decoder that skips the degenerate step without
    // counting it would treat that triangle as odd and flip it.
    let steps = decode_strip(&[0, 1, 2, 1, 3]).expect("five indices form a strip");
    assert_eq!(steps.len(), 3, "nothing is dropped");
    assert_eq!(steps[1].indices, [2, 1, 1]);
    assert!(steps[1].is_degenerate());
    assert_eq!(steps[2].step, 2);
    assert_eq!(steps[2].corners, [2, 3, 4], "source-corner map of step 2");
    assert_eq!(steps[2].indices, [2, 1, 3]);

    let topology = square_mesh(vec![strip(&[0, 1, 2, 1, 3])]).topology();
    let drawn = drawn_positions(&topology.triangles);
    assert_eq!(drawn, [[0, 1, 2], [2, 1, 3]], "same square as [0,1,2,3]");
    for positions in drawn {
        assert!(signed_area(positions) > 0.0, "{positions:?} is clockwise");
    }
    assert_eq!(
        topology.faces,
        [FaceStatus::Decoded {
            triangles: 3,
            degenerate: 1
        }]
    );
}

#[test]
fn accept_f10_a_parity_counts_from_the_strip_start_not_the_first_drawn_step() {
    // A leading repeated index makes step 0 degenerate and shifts every
    // later step by one, so the whole strip is drawn with the opposite
    // winding, consistently: (1, 0, 2) and (1, 2, 3). Compacting the
    // repeated index away first would instead draw the counter-clockwise
    // (0, 1, 2) and (2, 1, 3).
    let steps = decode_strip(&[0, 0, 1, 2, 3]).expect("five indices form a strip");
    let degenerate: Vec<bool> = steps.iter().map(|t| t.is_degenerate()).collect();
    assert_eq!(degenerate, [true, false, false]);

    let topology = square_mesh(vec![strip(&[0, 0, 1, 2, 3])]).topology();
    let drawn = drawn_positions(&topology.triangles);
    assert_eq!(drawn, [[1, 0, 2], [1, 2, 3]]);
    for positions in drawn {
        assert!(
            signed_area(positions) < 0.0,
            "{positions:?} is counter-clockwise"
        );
    }
}

#[test]
fn accept_f10_a_short_strip_is_rejected_not_dropped() {
    for indices in [&[][..], &[0][..], &[0, 1][..]] {
        assert_eq!(
            decode_strip(indices),
            Err(StripError::TooShort {
                indices: indices.len()
            })
        );
    }
    assert_eq!(decode_strip(&[0, 1]).unwrap_err().code(), "strip_too_short");
    assert_eq!(MIN_STRIP_INDICES, 3);

    let topology = square_mesh(vec![strip(&[0, 1]), strip(&[0, 1, 2])]).topology();
    assert_eq!(
        topology.faces[0],
        FaceStatus::Rejected(FaceIssue::TooFewCorners {
            kind: PrimitiveKind::TriangleStrip,
            corners: 2
        })
    );
    assert_eq!(topology.invalid_faces(), 1);
    assert_eq!(topology.decoded_faces(), 1);
    assert!(!topology.is_complete());
    assert!(topology.triangles.iter().all(|t| t.polygon == 1));
}

#[test]
fn accept_f10_a_every_broken_or_unsupported_face_is_counted() {
    let with = |mut c: RawCorner, f: fn(&mut RawCorner)| {
        f(&mut c);
        c
    };
    let mesh = square_mesh(vec![
        // 0: decoded strip
        strip(&[0, 1, 2, 3]),
        // 1: decoded three-corner polygon, corner order kept
        polygon(
            PrimitiveKind::Polygon,
            vec![corner(2), corner(1), corner(3)],
        ),
        // 2: position index past the four positions
        strip(&[0, 1, 4]),
        // 3: normal index past the one normal
        polygon(
            PrimitiveKind::Polygon,
            vec![
                corner(0),
                with(corner(1), |c| c.normal = Some(1)),
                corner(2),
            ],
        ),
        // 4: NaN texture coordinate
        polygon(
            PrimitiveKind::Polygon,
            vec![
                corner(0),
                corner(1),
                with(corner(2), |c| c.uv = Some([f32::NAN, 0.0])),
            ],
        ),
        // 5: bow-tie outline: valid indices, but its two lobes cancel out
        // and it cannot be triangulated (a convex quad would be, F10-B)
        polygon(
            PrimitiveKind::Polygon,
            vec![corner(0), corner(1), corner(2), corner(3)],
        ),
        // 6: two-corner polygon
        polygon(PrimitiveKind::Polygon, vec![corner(0), corner(1)]),
    ]);
    let topology = mesh.topology();

    let issue = |index: usize| match &topology.faces[index] {
        FaceStatus::Rejected(issue) => issue.clone(),
        other => panic!("face {index} was not rejected: {other:?}"),
    };
    assert_eq!(
        issue(2),
        FaceIssue::PositionIndexOutOfRange {
            corner: 2,
            index: 4,
            positions: 4
        }
    );
    assert_eq!(
        issue(3),
        FaceIssue::NormalIndexOutOfRange {
            corner: 1,
            index: 1,
            normals: 1
        }
    );
    assert_eq!(
        issue(4),
        FaceIssue::NonFinite {
            corner: 2,
            attribute: "uv"
        }
    );
    assert_eq!(
        issue(5),
        FaceIssue::UnsupportedNgon {
            corners: 4,
            reason: NgonIssue::ZeroArea
        }
    );
    assert!(issue(5).is_unsupported());
    assert_eq!(issue(5).code(), "unsupported_ngon");
    assert_eq!(issue(6).code(), "too_few_corners");

    assert_eq!(topology.faces.len(), 7);
    assert_eq!(topology.decoded_faces(), 2);
    assert_eq!(topology.invalid_faces(), 4);
    assert_eq!(topology.unsupported_faces(), 1);
    assert!(!topology.is_complete());

    let polygons: Vec<usize> = topology.triangles.iter().map(|t| t.polygon).collect();
    assert_eq!(polygons, [0, 0, 1]);
    assert_eq!(topology.triangles[2].positions, [2, 1, 3]);
    assert_eq!(topology.triangles[2].corners, [0, 1, 2]);
}

#[test]
fn accept_f10_a_shared_position_keeps_per_corner_attributes() {
    // Corners 0 and 3 share position 1 but carry different UVs and colors;
    // raw flags and material stay as stored. The source-corner map leads
    // back to each corner so the seam survives into later stages.
    let uv_corner = |position, uv: [f32; 2], color: [f32; 3]| RawCorner {
        position,
        normal: Some(0),
        uv: Some(uv),
        color: Some(color),
    };
    let mesh = square_mesh(vec![RawPolygon {
        kind: PrimitiveKind::TriangleStrip,
        raw_flags: 0xDEAD_BEEF,
        material: 7,
        corners: vec![
            uv_corner(1, [0.25, 0.0], [1.0, 0.0, 0.0]),
            uv_corner(3, [0.25, 1.0], [0.0, 1.0, 0.0]),
            uv_corner(0, [0.0, 0.0], [0.0, 0.0, 1.0]),
            uv_corner(1, [0.75, 0.0], [1.0, 1.0, 0.0]),
        ],
    }]);
    let topology = mesh.topology();
    assert!(topology.is_complete());
    let face = &mesh.polygons[0];
    assert_eq!((face.raw_flags, face.material), (0xDEAD_BEEF, 7));

    let uvs_of_position_1: Vec<[f32; 2]> = topology
        .triangles
        .iter()
        .flat_map(|t| t.corners.iter().zip(t.positions))
        .filter(|&(_, position)| position == 1)
        .map(|(&c, _)| face.corners[c].uv.expect("authored uv"))
        .collect();
    assert!(uvs_of_position_1.contains(&[0.25, 0.0]));
    assert!(uvs_of_position_1.contains(&[0.75, 0.0]));
}
