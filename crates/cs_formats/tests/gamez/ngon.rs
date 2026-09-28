//! Acceptance stage F10-B: validated n-gon triangulation
//! (`specs/F10-gamez-mesh-topology-and-material-records.md`, section
//! `### F10-B`, AC02 plus the failure cases the scenario implies).
//!
//! Every outline here is newly authored synthetic content, no original game
//! data. Inside/outside and winding are judged by helpers written in this
//! file (shoelace area, even-odd point-in-polygon, Newell normal), not by
//! the triangulator under test.

use cs_formats::gamez::{
    FaceIssue, FaceStatus, MeshTopology, NgonIssue, PrimitiveKind, RawCorner, RawMesh, RawPolygon,
    triangulate_polygon,
};

/// ```text
/// 4 (0,4)           2 (4,4)
///   |  \           /  |
///   |    \       /    |
///   |     3 (2,1)     |
///   |                 |
/// 0 (0,0) ---------- 1 (4,0)
/// ```
///
/// Corner 3 is reflex. A fan from corner 0 draws `(0, 2, 3)`, which lies
/// outside the outline (clockwise, over the notch).
const ARROW: [[f32; 2]; 5] = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [2.0, 1.0], [0.0, 4.0]];

fn corner(position: u32) -> RawCorner {
    RawCorner {
        position,
        normal: None,
        uv: None,
        color: None,
    }
}

/// One mesh with one polygon outline through all `positions` in order.
fn outline_mesh(positions: Vec<[f32; 3]>) -> RawMesh {
    let corners = (0..positions.len() as u32).map(corner).collect();
    RawMesh {
        positions,
        normals: Vec::new(),
        polygons: vec![RawPolygon {
            kind: PrimitiveKind::Polygon,
            raw_flags: 0,
            material: 0,
            corners,
        }],
    }
}

fn flat(points: &[[f32; 2]]) -> Vec<[f32; 3]> {
    points.iter().map(|&[x, y]| [x, y, 0.0]).collect()
}

/// Twice the signed area in the z = 0 plane.
fn signed_area2(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f64 {
    let [a, b, c] = [a, b, c].map(|p| p.map(f64::from));
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Twice the signed shoelace area of an outline in the z = 0 plane.
fn outline_area2(outline: &[[f32; 3]]) -> f64 {
    (0..outline.len())
        .map(|i| {
            let [p, q] = [outline[i], outline[(i + 1) % outline.len()]].map(|p| p.map(f64::from));
            p[0] * q[1] - q[0] * p[1]
        })
        .sum()
}

/// Even-odd test: `p` strictly inside the outline (z = 0 plane).
fn inside(outline: &[[f32; 3]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for i in 0..outline.len() {
        let [a, b] = [outline[i], outline[(i + 1) % outline.len()]].map(|p| p.map(f64::from));
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

fn decoded(topology: &MeshTopology) -> usize {
    match topology.faces[0] {
        FaceStatus::Decoded {
            triangles,
            degenerate,
        } => {
            assert_eq!(degenerate, 0, "triangulation draws no degenerate triangle");
            triangles
        }
        ref other => panic!("outline was not decoded: {other:?}"),
    }
}

/// Every triangle has the outline's winding, lies inside it and together
/// they cover exactly its area, so none can stick out or overlap.
fn assert_covers_exactly(outline: &[[f32; 3]], topology: &MeshTopology) {
    let n = outline.len();
    assert_eq!(decoded(topology), n - 2);
    assert_eq!(topology.triangles.len(), n - 2);
    let winding = outline_area2(outline).signum();
    let mut covered = 0.0;
    for (step, triangle) in topology.triangles.iter().enumerate() {
        assert_eq!(triangle.step, step);
        assert_eq!(
            triangle.positions,
            triangle.corners.map(|c| c as u32),
            "corner c of this fixture is position c"
        );
        let [a, b, c] = triangle.positions.map(|p| outline[p as usize]);
        let area2 = signed_area2(a, b, c);
        assert_eq!(
            area2.signum(),
            winding,
            "{:?} does not keep the outline's winding",
            triangle.positions
        );
        covered += area2;
        let centroid = [0, 1].map(|axis| (f64::from(a[axis] + b[axis] + c[axis])) / 3.0);
        assert!(
            inside(outline, centroid),
            "{:?} lies outside the outline",
            triangle.positions
        );
    }
    assert_eq!(
        covered,
        outline_area2(outline),
        "triangles cover the outline"
    );
}

#[test]
fn accept_f10_b_concave_polygon_triangulates_inside_its_boundary() {
    let outline = flat(&ARROW);
    let topology = outline_mesh(outline.clone()).topology();
    assert!(topology.is_complete());
    assert_covers_exactly(&outline, &topology);

    let fan = [0, 2, 3];
    assert!(
        signed_area2(outline[0], outline[2], outline[3]) < 0.0,
        "the fan triangle really is outside"
    );
    for triangle in &topology.triangles {
        let mut sorted = triangle.positions;
        sorted.sort_unstable();
        assert_ne!(sorted, fan, "fanned from corner 0");
    }

    let direct = triangulate_polygon(&outline).expect("simple outline");
    let from_mesh: Vec<[usize; 3]> = topology.triangles.iter().map(|t| t.corners).collect();
    assert_eq!(direct, from_mesh, "the mesh path uses the triangulator");
}

#[test]
fn accept_f10_b_many_reflex_corners_still_cover_the_outline() {
    // A comb: five teeth pointing up, four reflex notches between them.
    let mut comb = vec![[0.0, 0.0], [9.0, 0.0]];
    for tooth in (0..5).rev() {
        let x = tooth as f32 * 2.0;
        comb.push([x + 1.0, 3.0]);
        comb.push([x, 3.0]);
        if tooth > 0 {
            comb.push([x - 0.5, 1.0]);
        }
    }
    let outline = flat(&comb);
    let topology = outline_mesh(outline.clone()).topology();
    assert_covers_exactly(&outline, &topology);

    // The same comb stored clockwise keeps clockwise triangles.
    let reversed: Vec<[f32; 3]> = outline.iter().rev().copied().collect();
    let topology = outline_mesh(reversed.clone()).topology();
    assert!(outline_area2(&reversed) < 0.0);
    assert_covers_exactly(&reversed, &topology);
}

#[test]
fn accept_f10_b_outline_off_the_xy_plane_keeps_its_stored_winding() {
    // The arrow tilted into the plane z = y, and stood up in the x = 0 plane,
    // each stored in both directions. Winding is judged against the Newell
    // normal of the stored outline.
    let placements: [fn([f32; 2]) -> [f32; 3]; 2] = [|[x, y]| [x, y, y], |[x, y]| [0.0, x, y]];
    for place in placements {
        for reverse in [false, true] {
            let mut outline: Vec<[f32; 3]> = ARROW.iter().copied().map(place).collect();
            if reverse {
                outline.reverse();
            }
            let mut normal = [0.0f64; 3];
            for i in 0..outline.len() {
                let [p, q] =
                    [outline[i], outline[(i + 1) % outline.len()]].map(|p| p.map(f64::from));
                normal[0] += (p[1] - q[1]) * (p[2] + q[2]);
                normal[1] += (p[2] - q[2]) * (p[0] + q[0]);
                normal[2] += (p[0] - q[0]) * (p[1] + q[1]);
            }
            let topology = outline_mesh(outline.clone()).topology();
            assert_eq!(decoded(&topology), 3);
            for triangle in &topology.triangles {
                let [a, b, c] = triangle
                    .positions
                    .map(|p| outline[p as usize].map(f64::from));
                let (u, v) = (
                    [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                    [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
                );
                let face = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                let dot: f64 = (0..3).map(|i| face[i] * normal[i]).sum();
                assert!(
                    dot > 0.0,
                    "{:?} flipped (reverse: {reverse})",
                    triangle.positions
                );
            }
        }
    }
}

#[test]
fn accept_f10_b_convex_quad_decodes_to_two_triangles() {
    let outline = flat(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let topology = outline_mesh(outline.clone()).topology();
    assert_covers_exactly(&outline, &topology);
    assert_eq!(topology.unsupported_faces(), 0);
}

#[test]
fn accept_f10_b_untriangulable_outlines_are_reported_not_fanned() {
    let cases: [(&str, Vec<[f32; 3]>, NgonIssue); 4] = [
        (
            "bow tie",
            flat(&[[0.0, 0.0], [2.0, 0.0], [0.0, 1.0], [3.0, 2.0]]),
            NgonIssue::SelfIntersecting {
                first_edge: 1,
                second_edge: 3,
            },
        ),
        (
            "edge folding back",
            flat(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [4.0, 2.0]]),
            NgonIssue::SelfIntersecting {
                first_edge: 1,
                second_edge: 2,
            },
        ),
        (
            "collinear",
            flat(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]]),
            NgonIssue::ZeroArea,
        ),
        (
            "two corners at one location",
            flat(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0], [0.0, 4.0]]),
            NgonIssue::CoincidentCorners {
                first: 0,
                second: 3,
            },
        ),
    ];
    for (name, outline, reason) in cases {
        let corners = outline.len();
        assert_eq!(triangulate_polygon(&outline), Err(reason), "{name}");
        let topology = outline_mesh(outline).topology();
        let expected = FaceIssue::UnsupportedNgon { corners, reason };
        assert_eq!(
            topology.faces,
            [FaceStatus::Rejected(expected.clone())],
            "{name}"
        );
        assert!(topology.triangles.is_empty(), "{name}: nothing fanned");
        assert!(expected.is_unsupported());
        assert_eq!(topology.unsupported_faces(), 1);
        assert_eq!(topology.invalid_faces(), 0);
        assert!(!topology.is_complete());
        assert!(expected.to_string().starts_with("unsupported_ngon: "));
    }

    // One position index used twice is the same location, and a rejected
    // outline does not stop the rest of the mesh from decoding.
    let mut mesh = outline_mesh(flat(&ARROW));
    let repeated = RawPolygon {
        kind: PrimitiveKind::Polygon,
        raw_flags: 0,
        material: 0,
        corners: [0, 1, 2, 1, 4].map(corner).to_vec(),
    };
    mesh.polygons.insert(0, repeated);
    let topology = mesh.topology();
    assert_eq!(
        topology.faces[0],
        FaceStatus::Rejected(FaceIssue::UnsupportedNgon {
            corners: 5,
            reason: NgonIssue::CoincidentCorners {
                first: 1,
                second: 3
            }
        })
    );
    assert_eq!(topology.decoded_faces(), 1);
    assert!(topology.triangles.iter().all(|t| t.polygon == 1));
    assert_eq!(topology.triangles.len(), 3);
}
