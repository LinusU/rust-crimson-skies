//! Which pose does weapon fire give the FIRE ANIMATION root node? (#2104)
//! Task test prefix: `accept_f27_c_fx_orig_draw_a_`.
//!
//! Rows are `[[muzzle_burst_pose_fact]]` of
//! `data/owner-static/engine-facts.toml`, described in
//! `docs/findings/2026-10-25-f27-c-fx-orig-muzzle-burst-pose.md`.

const TABLE: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `[[muzzle_burst_pose_fact]]` row.
fn rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a muzzle burst pose row lacks {name}"))
    };
    TABLE
        .split("[[muzzle_burst_pose_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            assert_eq!(field(chunk, "provenance"), "static");
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-25");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

#[test]
fn accept_f27_c_fx_orig_draw_a_rows_pin_the_pose_arguments() {
    let rows = rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "pose.yaw",
            "pose.pitch",
            "pose.asin_import",
            "pose.roll",
            "pose.start_call",
            "pose.starter_position",
            "pose.starter_rotation",
            "frame.matrix_rows_xy",
            "frame.matrix_row_z",
            "frame.point_transform",
            "frame.at_node_offset",
            "anim.call_executor_node_branch",
            "time.event_offset_seconds",
        ]
    );
    let get = |name: &str| &rows.iter().find(|(n, _, _)| n == name).expect("a row").2;
    // fld [ebx]; ...; fchs; ...; fpatan region: both direction components negated.
    assert_eq!(get("pose.yaw")[..2], [0xd9, 0x03]);
    assert_eq!(get("pose.yaw")[5..7], [0xd9, 0xe0]);
    // fld [ebx+4]; call 0x5f704a.
    assert_eq!(get("pose.pitch")[..3], [0xd9, 0x43, 0x04]);
    // jmp [0xa20198] (_CIasin).
    assert_eq!(
        get("pose.asin_import"),
        &[0xff, 0x25, 0x98, 0x01, 0xa2, 0x00]
    );
    // test dl,1 gates the random roll; call [0xa20350] (rand).
    assert_eq!(get("pose.roll")[..5], [0xf6, 0xc2, 0x01, 0x74, 0x23]);
    assert_eq!(
        get("pose.roll")[5..11],
        [0xff, 0x15, 0x50, 0x03, 0xa2, 0x00]
    );
    // push 0 (parent node) then push handle, call 0x4edc10's opcode.
    assert_eq!(get("pose.start_call")[21..], [0x6a, 0x00, 0x52, 0xe8]);
    // m7 = -sin(pitch): fld [ebp-4]; fchs.
    assert_eq!(
        get("frame.matrix_row_z")[9..14],
        [0xd9, 0x45, 0xfc, 0xd9, 0xe0]
    );
}

/// The derived claim of the finding: with the rows' yaw, pitch and matrix, the
/// node's local +Z maps to minus the fire direction and the (0, -0.2, -1) node
/// offset lands one unit ahead of the muzzle.
#[test]
fn accept_f27_c_fx_orig_draw_a_local_z_is_opposite_the_fire_direction() {
    let rotate = |pitch: f32, yaw: f32, roll: f32, v: [f32; 3]| {
        let (sa, ca) = pitch.sin_cos();
        let (sb, cb) = yaw.sin_cos();
        let (sc, cc) = roll.sin_cos();
        let m = [
            [cc * cb + sa * sb * sc, sc * ca, sa * cb * sc - cc * sb],
            [sa * sb * cc - cb * sc, cc * ca, sb * sc + sa * cb * cc],
            [sb * ca, -sa, cb * ca],
        ];
        [0, 1, 2].map(|j| v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j])
    };
    for dir in [
        [0.0f32, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.6, 0.0, -0.8],
        [0.0, 1.0, 0.0],
    ] {
        let yaw = (-dir[0]).atan2(-dir[2]);
        let pitch = f32::asin(dir[1]);
        let z = rotate(pitch, yaw, 0.0, [0.0, 0.0, 1.0]);
        for k in 0..3 {
            assert!((z[k] + dir[k]).abs() < 1e-5, "{dir:?}: local +Z {z:?}");
        }
        // The AT_NODE offset (0, -0.2, -1) puts the burst one unit ahead.
        let ahead = rotate(pitch, yaw, 0.0, [0.0, -0.2, -1.0]);
        let along: f32 = (0..3).map(|k| ahead[k] * dir[k]).sum();
        assert!((along - 1.0).abs() < 1e-5, "{dir:?}: ahead by {along}");
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f27_c_fx_orig_draw_a_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = std::fs::read(path).expect("the engine image reads");
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    for (name, va, bytes) in rows() {
        let offset = facts
            .image_offset(va)
            .unwrap_or_else(|error| panic!("{name}: {error}")) as usize;
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at {va:#x}"
        );
    }
}
