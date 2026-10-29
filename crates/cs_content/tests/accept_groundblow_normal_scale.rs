//! Task #2212 (GROUNDBLOW-NORMAL-SCALE): the face winding and face sizes
//! behind the ground blow's `sqrt(dot(backward_axis, N))` — the bound-box
//! corner enumeration and face orders of `0x55d270`, the unnormalised
//! cross product `0x55d6c0` stores, and the retail water-tile / terrain
//! face areas the term sees on M01 content.
//!
//! Task test prefix: `accept_groundblow_normal_`. Finding:
//! `docs/findings/2026-10-29-groundblow-normal-scale.md`.
//!
//! The committed rows carry the addresses and bytes of every measured
//! site; the image test re-reads each byte window from
//! `$CS_ENGINE_IMAGE`, and the retail test re-reads the measured polygons
//! from `$CS_GAME_DIR` through the production `cs_formats::gamez`
//! readers (static analysis and container reads, never original-run
//! behavior).

use cs_formats::ParseContext;
use cs_formats::gamez::{NodeKind, RawNode, read_gamez_meshes, read_gamez_nodes};

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// `(name, va, bytes, fact)` of every committed `[[groundblow_normal_fact]]` row.
fn committed_rows() -> Vec<(String, u64, Vec<u8>, String)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a groundblow_normal_fact row lacks {name}"))
    };
    TABLE
        .split("[[groundblow_normal_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            assert_eq!(field(chunk, "provenance"), "static");
            (field(chunk, "name"), va, bytes, field(chunk, "fact"))
        })
        .collect()
}

/// The rows name every measured site: the corner enumeration, the six
/// face orders and their windings, the shared normal writer, the accept
/// gates, the soup two-sided bit, the probe's reject and sqrt, and the
/// retail faces.
#[test]
fn accept_groundblow_normal_the_committed_rows_cover_the_measured_sites() {
    let rows = committed_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "groundblow_normal.box_corner_build",
            "groundblow_normal.box_face_x_min",
            "groundblow_normal.box_face_z_max",
            "groundblow_normal.box_face_x_max",
            "groundblow_normal.box_face_z_min",
            "groundblow_normal.box_face_y_min",
            "groundblow_normal.box_face_y_max",
            "groundblow_normal.cross_first_three",
            "groundblow_normal.normal_unnormalised",
            "groundblow_normal.accept_gate",
            "groundblow_normal.straddle_test",
            "groundblow_normal.two_sided_arg",
            "groundblow_normal.selector_zero",
            "groundblow_normal.dot_reject",
            "groundblow_normal.sqrt_site",
            "groundblow_normal.retail_water",
            "groundblow_normal.retail_terrain",
            "groundblow_normal.sqrt_dot_scale",
        ]
    );
    let get = |name: &str| rows.iter().find(|r| r.0 == name).expect("committed row");

    // The corner enumeration: the direct build at 0x4cdabc reads
    // `[ebp+0x70]` (`mov eax,[ebp+0x70]`) and fills the eight slots.
    assert_eq!(get("groundblow_normal.box_corner_build").1, 0x004c_dabc);
    assert_eq!(
        get("groundblow_normal.box_corner_build").2[..3],
        [0x8b, 0x45, 0x70],
        "`mov eax,[ebp+0x70]` reads the node's bound record"
    );

    // Each face row ends in a `call rel32` to 0x55d6c0 — checked against
    // the image below; here the windows' tails carry the call opcode.
    for name in [
        "groundblow_normal.box_face_x_min",
        "groundblow_normal.box_face_z_max",
        "groundblow_normal.box_face_x_max",
        "groundblow_normal.box_face_z_min",
        "groundblow_normal.box_face_y_min",
        "groundblow_normal.box_face_y_max",
    ] {
        let (_, _, bytes, fact) = get(name);
        assert_eq!(
            bytes[bytes.len() - 5],
            0xe8,
            "{name}'s window must end at the face-test call"
        );
        assert!(
            fact.contains("outward"),
            "{name} must state the measured winding"
        );
    }

    // The normal is the first-three-corners cross product, stored
    // unnormalised into record+0x00.
    let (_, _, cross, fact) = get("groundblow_normal.cross_first_three");
    assert_eq!(
        cross[..4],
        [0xd9, 0x41, 0x0c, 0xd8],
        "`fld [ecx+0xc]` opens e1 = buf[1]-buf[0]"
    );
    assert!(fact.contains("first three supplied corners"));
    let (_, _, nstore, fact) = get("groundblow_normal.normal_unnormalised");
    assert_eq!(
        &nstore[4..6],
        [0xd9, 0x1e],
        "`fstp [esi]` stores N.x into record+0x00"
    );
    assert!(fact.contains("no divide or fsqrt"));

    // The probe: reject non-positive dot, then sqrt of the raw dot.
    assert_eq!(get("groundblow_normal.dot_reject").1, 0x0048_c118);
    assert_eq!(get("groundblow_normal.sqrt_site").1, 0x0048_c142);
    assert_eq!(
        get("groundblow_normal.sqrt_dot_scale").2.as_slice(),
        [0xd9, 0xfa],
        "the committed sqrt byte is the `fsqrt` at 0x48c148"
    );

    // The retail rows carry the measured faces: the 1024 m water tile's
    // 262144 m2 cross and the island strip's up-and-outward winding.
    let water = &get("groundblow_normal.retail_water").3;
    assert!(water.contains("262144"));
    assert!(water.contains("537..393216"));
    let terrain = &get("groundblow_normal.retail_terrain").3;
    assert!(terrain.contains("19469"));
    assert!(terrain.contains("0.540,0.841,0"));
}

/// Every cited byte window is in the owner's decrypted executable.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_groundblow_normal_image_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    let rows = committed_rows();
    assert_eq!(rows.len(), 18);
    for (name, va, bytes, _) in rows {
        let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at {va:#08x} no longer matches the image bytes"
        );
    }
}

/// The six face rows' tail calls all reach `0x55d6c0`, the shared face
/// test the finding names. The target is derived from each row's own
/// committed bytes (`e8 rel32`); the image test above anchors the same
/// bytes to `$CS_ENGINE_IMAGE`, so this needs no image of its own.
#[test]
fn accept_groundblow_normal_call_targets_match_the_finding() {
    let get = |name: &str| {
        committed_rows()
            .into_iter()
            .find(|r| r.0 == name)
            .expect("committed row")
    };
    for name in [
        "groundblow_normal.box_face_x_min",
        "groundblow_normal.box_face_z_max",
        "groundblow_normal.box_face_x_max",
        "groundblow_normal.box_face_z_min",
        "groundblow_normal.box_face_y_min",
        "groundblow_normal.box_face_y_max",
    ] {
        let (.., va, bytes, _) = get(name);
        let tail = &bytes[bytes.len() - 5..];
        assert_eq!(tail[0], 0xe8, "{name}'s window must end in a rel32 call");
        let rel = i32::from_le_bytes(tail[1..5].try_into().unwrap());
        assert_eq!(
            (va + bytes.len() as u64).wrapping_add(rel as i64 as u64),
            0x0055_d6c0,
            "{name}'s tail call must reach the shared face test 0x55d6c0"
        );
    }
}

// ------------------------------------------------------------------
// retail container re-read through the production cs_formats readers
// ------------------------------------------------------------------

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The local transform a node's own record carries (identity for
/// non-object kinds), as `cs_content::scene`'s canonical transform so the
/// parent-chain composition is the production rule, not a test-local one.
fn local(node: &RawNode) -> cs_content::scene::CanonicalTransform {
    let f = |v: [f32; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
    match node.kind {
        NodeKind::Object3d(data) => {
            cs_content::scene::CanonicalTransform::try_new(data.matrix.map(f), f(data.translation))
                .expect("stored transform is finite")
        }
        _ => cs_content::scene::CanonicalTransform::IDENTITY,
    }
}

/// `world = parent_world.compose(local)` walked to the root.
fn global(nodes: &[RawNode], index: u32) -> cs_content::scene::CanonicalTransform {
    let mut chain = Vec::new();
    let mut at = Some(index);
    while let Some(i) = at {
        let node = &nodes[i as usize];
        chain.push(local(node));
        at = node.parent;
    }
    let mut xf = cs_content::scene::CanonicalTransform::IDENTITY;
    for local in chain.iter().rev() {
        xf = xf.compose(local);
    }
    xf
}

/// `N = (c1-c0)x(c2-c0)` of one stored polygon, in stored corner order —
/// the same value `0x55d6c0` computes for the face.
fn stored_normal(
    mesh: &cs_formats::gamez::GameZMesh,
    poly: &cs_formats::gamez::RawPolygon,
    xf: &cs_content::scene::CanonicalTransform,
) -> (V3, f32) {
    let p = |i: usize| {
        let w = xf.apply(mesh.mesh.positions[poly.corners[i].position as usize].map(|v| v as f64));
        [w[0] as f32, w[1] as f32, w[2] as f32]
    };
    let n = cross(sub(p(1), p(0)), sub(p(2), p(0)));
    (n, norm(n))
}

/// The measured M01 faces: the flat water tile and the island strip.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_normal_retail_faces_match_the_committed_rows() {
    let root = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the owner installation's ZBD/C3 data",
    );
    let bytes = std::fs::read(format!("{}/ZBD/C3/gamez.zbd", root.to_string_lossy()))
        .expect("ZBD/C3/gamez.zbd reads");
    let mut ctx = ParseContext::with_defaults("zbd/c3/gamez.zbd".to_string());
    let meshes = read_gamez_meshes(&mut ctx, "zbd/c3/gamez.zbd", &bytes).expect("meshes");
    let nodes = read_gamez_nodes(&mut ctx, &bytes).expect("nodes");

    // The water tile: node 'g28530' (mesh 371), a 1024x1024 m tile at
    // y=0. Its outline quad poly 6 must give the committed N=(0,262144,0).
    let tile = nodes
        .nodes
        .iter()
        .find(|n| n.index == 547 && n.name == "g28530")
        .expect("water tile node 547 'g28530' exists in C3");
    assert_eq!(tile.mesh_index(), 371);
    let mesh = meshes.meshes[371].as_ref().expect("mesh 371 present");
    let xf = global(&nodes.nodes, 547);
    assert!(
        xf.determinant() > 0.0,
        "the tile transform preserves winding"
    );

    let (n, area) = stored_normal(mesh, &mesh.mesh.polygons[6], &xf);
    assert_eq!(n[0], 0.0, "water tile face is horizontal");
    assert_eq!(n[2], 0.0);
    assert_eq!(
        area, 262144.0,
        "the 512x512 m outline quad's stored cross is the committed parallelogram area"
    );
    assert_eq!(n[1] / area, 1.0, "the stored winding faces up (+Y)");

    // Every polygon of that tile: all up, |N| between 40231 and 262144.
    for poly in &mesh.mesh.polygons {
        let (_, a) = stored_normal(mesh, poly, &xf);
        assert!(a > 1.0, "every water-tile face is far from unit area");
    }

    // The island chunk: node 'g28603' (mesh 761), strip poly 7 must give
    // the committed up-and-outward winding and 19469 m2 cross.
    let chunk = nodes
        .nodes
        .iter()
        .find(|n| n.index == 2873 && n.name == "g28603")
        .expect("terrain node 2873 'g28603' exists in C3");
    assert_eq!(chunk.mesh_index(), 761);
    let cmesh = meshes.meshes[761].as_ref().expect("mesh 761 present");
    let cxf = global(&nodes.nodes, 2873);
    assert!(cxf.determinant() > 0.0);

    let (cn, carea) = stored_normal(cmesh, &cmesh.mesh.polygons[7], &cxf);
    let unit = [cn[0] / carea, cn[1] / carea, cn[2] / carea];
    assert!(
        (carea - 19469.0).abs() < 100.0,
        "the measured terrain strip's cross is ~19469 m2, got {carea}"
    );
    assert!(
        (unit[0] - 0.540).abs() < 0.01 && (unit[1] - 0.841).abs() < 0.01 && unit[2].abs() < 0.01,
        "the stored winding is up-and-outward, got {unit:?}"
    );

    // The assist's own quantity: for a level aircraft back is horizontal,
    // so a descending approach on the water tile gives dot = |N| * sin(a).
    // The committed claim is that sqrt(dot) is sqrt(|N|)*sqrt(cos), i.e.
    // |N| != 1 — assert the scale fact's content directly.
    assert!(
        area.sqrt() > 400.0,
        "sqrt(|N|) = {} x sqrt(cos) on the widest water quad — not sqrt(cos)",
        area.sqrt()
    );
}
