//! Acceptance for task #1673 (`PARITY-M01-LC-MEASURE-CAM-FRAME`): how the
//! engine turns a rotation part into the node's rotation, and the frame of the
//! launch cinematic's camera positions
//! (`docs/findings/2026-10-18-parity-m01-lc-camera-frame-and-rotation.md`).
//!
//! The synthetic tests author every byte they read. The retail tests read
//! `$CS_GAME_DIR` and `$CS_ENGINE_IMAGE` (never write them) and fail loudly
//! without them. The frame half (the camera is re-parented to the Pandora)
//! needs the event decoder and lives in `cs_app`'s
//! `parity_m01_lc_measure_cam_frame` test module.

use cs_formats::zbd::anim_tracks::{TrackBlock, read_animation_tracks};

use super::t1662::{Container, GENERIC_INTRO, put_f32, put_u32, region, tracks_of};

/// A rotation part: start quaternion, one word of padding, the linear rate
/// (words 4..7) and three cubics (words 7..19).
fn rotation_part(start: [f32; 4], rate: [f32; 3], cubics: [[f32; 4]; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    for word in start {
        put_f32(&mut out, word);
    }
    for word in rate {
        put_f32(&mut out, word);
    }
    for cubic in cubics {
        for word in cubic {
            put_f32(&mut out, word);
        }
    }
    out
}

fn rotation_segment(part: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, 2);
    put_f32(&mut out, 0.0);
    put_f32(&mut out, 1.0);
    out.extend(part);
    out
}

fn authored(mode: u32, start: [f32; 4], rate: [f32; 3], cubics: [[f32; 4]; 3]) -> Vec<u8> {
    region(&[(
        "..\\data\\common\\zrdr\\x\\a.zan",
        "camera1",
        mode,
        vec![rotation_segment(rotation_part(start, rate, cubics))],
    )])
}

const IDENTITY: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const NO_CUBIC: [[f32; 4]; 3] = [[0.0; 4]; 3];

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn accept_parity_m01_lc_measure_cam_frame_without_a_rotation_part_there_is_no_orientation() {
    let bytes = region(&[(
        "..\\data\\common\\zrdr\\x\\a.zan",
        "camera1",
        1,
        vec![{
            let mut out = Vec::new();
            put_u32(&mut out, 0);
            put_f32(&mut out, 0.0);
            put_f32(&mut out, 1.0);
            out
        }],
    )]);
    let tracks = read_animation_tracks(&bytes, 1).expect("tiles");
    let segment = &tracks.block(0).expect("block").segments()[0];
    assert!(segment.orientation_at(true, 0.5).is_none());
}

#[test]
fn accept_parity_m01_lc_measure_cam_frame_the_start_quaternion_alone_is_the_orientation() {
    // 90 degrees about X: (cos 45, sin 45, 0, 0).
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let bytes = authored(1, [h, h, 0.0, 0.0], [0.0; 3], NO_CUBIC);
    let tracks = read_animation_tracks(&bytes, 1).expect("tiles");
    let o = tracks.block(0).expect("block").segments()[0]
        .orientation_at(true, 0.25)
        .expect("a rotation part");
    assert!(close(o.quaternion[0], f64::from(h)) && close(o.quaternion[1], f64::from(h)));
    // x is the first angle, and it is +90 degrees for a +90 degree turn about X
    // under the engine's row-vector matrix: m[2][1] = 2yz - 2xw = -1 gives
    // the gimbal branch, x = -copysign(pi/2, -1) = +pi/2.
    assert!(
        close(o.euler[0], std::f64::consts::FRAC_PI_2),
        "{:?}",
        o.euler
    );
    assert!(
        close(o.euler[1], 0.0) && close(o.euler[2], 0.0),
        "{:?}",
        o.euler
    );
}

#[test]
fn accept_parity_m01_lc_measure_cam_frame_turns_about_each_axis_land_in_the_matching_angle() {
    // The vector is exponentiated without halving: v = (a, 0, 0) turns by 2a.
    for (axis, angle) in [(0usize, 0.3f32), (1, 0.4), (2, 0.2)] {
        let mut v = [0.0f32; 3];
        v[axis] = angle;
        let cubics = [
            [v[0], 0.0, 0.0, 0.0],
            [v[1], 0.0, 0.0, 0.0],
            [v[2], 0.0, 0.0, 0.0],
        ];
        let bytes = authored(1, IDENTITY, [0.0; 3], cubics);
        let tracks = read_animation_tracks(&bytes, 1).expect("tiles");
        let o = tracks.block(0).expect("block").segments()[0]
            .orientation_at(true, 0.7)
            .expect("rotation");
        for (k, value) in o.euler.iter().enumerate() {
            let expected = if k == axis {
                2.0 * f64::from(angle)
            } else {
                0.0
            };
            assert!(close(*value, expected), "axis {axis}: {:?}", o.euler);
        }
    }
}

#[test]
fn accept_parity_m01_lc_measure_cam_frame_the_polynomial_quaternion_is_on_the_left() {
    // start: 60 degrees about Y (half-angle 30); polynomial: 0.5 about X.
    // exp(v) * start and start * exp(v) differ; the engine multiplies
    // 0x53f920(exp, start).
    let start = [0.866_025_4f32, 0.0, 0.5, 0.0];
    let cubics = [[0.5, 0.0, 0.0, 0.0], [0.0; 4], [0.0; 4]];
    let bytes = authored(1, start, [0.0; 3], cubics);
    let tracks = read_animation_tracks(&bytes, 1).expect("tiles");
    let o = tracks.block(0).expect("block").segments()[0]
        .orientation_at(true, 0.0)
        .expect("rotation");
    let (c, s) = (0.5f64.cos(), 0.5f64.sin());
    let (sw, sy) = (f64::from(start[0]), f64::from(start[2]));
    // exp = (c, s, 0, 0); exp * start has x = s*sw, y = c*sy, z = +s*sy;
    // start * exp has z = -s*sy.
    let left = [c * sw, s * sw, c * sy, s * sy];
    let right = [c * sw, s * sw, c * sy, -s * sy];
    for (k, expected) in left.iter().enumerate() {
        assert!(close(o.quaternion[k], *expected), "{k}: {:?}", o.quaternion);
    }
    assert!(!close(o.quaternion[3], right[3]), "the other order differs");
}

#[test]
fn accept_parity_m01_lc_measure_cam_frame_linear_mode_has_no_start_term_in_the_vector() {
    // Mode 0: the vector is u * words 4..7; the start words 0..4 are only the
    // quaternion. u = 0.5, rate (0, 0.8, 0): a turn of 2 * 0.4 about Y.
    let bytes = authored(0, IDENTITY, [0.0, 0.8, 0.0], NO_CUBIC);
    let tracks = read_animation_tracks(&bytes, 1).expect("tiles");
    let part = tracks.block(0).expect("block").segments()[0]
        .rotation()
        .expect("rotation");
    assert_eq!(part.rotation_vector_at(false, 0.5), [0.0, 0.4, 0.0]);
    let o = tracks.block(0).expect("block").segments()[0]
        .orientation_at(false, 0.5)
        .expect("rotation");
    assert!(close(o.euler[1], 0.8), "{:?}", o.euler);
}

#[test]
fn accept_parity_m01_lc_measure_cam_frame_matrix_is_the_transpose_of_the_column_matrix() {
    // Yaw only: v = (0, a, 0) turns by 2a about Y. The column-vector matrix of
    // a +b turn about Y has m02 = +sin b; the engine's layout has it at m[2][0].
    let a = 0.35f32;
    let bytes = authored(
        1,
        IDENTITY,
        [0.0; 3],
        [[0.0; 4], [a, 0.0, 0.0, 0.0], [0.0; 4]],
    );
    let tracks = read_animation_tracks(&bytes, 1).expect("tiles");
    let o = tracks.block(0).expect("block").segments()[0]
        .orientation_at(true, 0.0)
        .expect("rotation");
    let b = 2.0 * f64::from(a);
    assert!(close(o.matrix[2][0], b.sin()) && close(o.matrix[0][2], -b.sin()));
    assert!(close(o.matrix[0][0], b.cos()) && close(o.matrix[2][2], b.cos()));
}

/// Row `r` of `Ry(y) * Rx(x) * Rz(z)` on column vectors, transposed: the
/// engine's layout.
fn engine_matrix(euler: [f64; 3]) -> [[f64; 3]; 3] {
    let (sx, cx) = euler[0].sin_cos();
    let (sy, cy) = euler[1].sin_cos();
    let (sz, cz) = euler[2].sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        out
    };
    let column = mul(mul(ry, rx), rz);
    let mut transposed = [[0.0; 3]; 3];
    for (i, row) in transposed.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = column[j][i];
        }
    }
    transposed
}

fn intro_tracks<R>(f: impl FnOnce(&[TrackBlock<'_>]) -> R) -> R {
    let spelling = "ZBD/C3/cam_anim.zbd";
    Container::open(spelling).with(spelling, |index, records| {
        let tracks = tracks_of(spelling, index, records).expect("C3 has a tail");
        f(&tracks.blocks()[..GENERIC_INTRO.len()])
    })
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_frame_retail_composition_is_continuous_at_every_boundary() {
    intro_tracks(|blocks| {
        let (mut engine_worst, mut other_worst) = (0.0f64, 0.0f64);
        let mut checked = 0;
        for block in blocks {
            let cubic = block.mode() != 0;
            for pair in block.segments().windows(2) {
                let (Some(here), Some(next)) = (pair[0].rotation(), pair[1].rotation()) else {
                    continue;
                };
                let length = pair[0].end() - pair[0].start();
                let next = next.start_quaternion().map(f64::from);
                let deviation =
                    |q: [f64; 4]| 1.0 - q.iter().zip(&next).map(|(a, b)| a * b).sum::<f64>().abs();
                let composed = pair[0].orientation_at(cubic, length).expect("rotation");
                engine_worst = engine_worst.max(deviation(composed.quaternion));
                // The other order: start * exp(v).
                let v = here.rotation_vector_at(cubic, length).map(f64::from);
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                let (c, s) = (l.cos(), if l > 0.0 { l.sin() / l } else { 0.0 });
                let e = [c, v[0] * s, v[1] * s, v[2] * s];
                let q = here.start_quaternion().map(f64::from);
                let swapped = [
                    q[0] * e[0] - q[1] * e[1] - q[2] * e[2] - q[3] * e[3],
                    q[1] * e[0] + e[1] * q[0] + e[3] * q[2] - e[2] * q[3],
                    q[2] * e[0] + e[2] * q[0] + e[1] * q[3] - e[3] * q[1],
                    q[3] * e[0] + e[3] * q[0] + e[2] * q[1] - e[1] * q[2],
                ];
                other_worst = other_worst.max(deviation(swapped));
                checked += 1;
            }
        }
        assert!(checked > 300, "{checked} boundaries");
        assert!(engine_worst < 1e-6, "engine order: {engine_worst:e}");
        assert!(
            other_worst > 1e-4 && other_worst > 100.0 * engine_worst,
            "the other order must fit clearly worse: {other_worst:e} vs {engine_worst:e}"
        );
    });
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_measure_cam_frame_retail_euler_rebuilds_the_matrix() {
    intro_tracks(|blocks| {
        let mut gimbal = 0;
        let mut rebuilt = 0;
        for block in blocks {
            let cubic = block.mode() != 0;
            for segment in block.segments() {
                for fraction in [0.0f32, 0.5, 1.0] {
                    let u = fraction * (segment.end() - segment.start());
                    let Some(o) = segment.orientation_at(cubic, u) else {
                        continue;
                    };
                    if o.matrix[2][1].abs() >= 1.0 - 1e-9 {
                        gimbal += 1;
                        continue;
                    }
                    let again = engine_matrix(o.euler);
                    for i in 0..3 {
                        for j in 0..3 {
                            assert!(
                                (again[i][j] - o.matrix[i][j]).abs() < 1e-5,
                                "{:?} -> {again:?} vs {:?}",
                                o.euler,
                                o.matrix
                            );
                        }
                    }
                    rebuilt += 1;
                }
            }
        }
        assert!(rebuilt > 500, "{rebuilt} rebuilt, {gimbal} at the gimbal");
    });
}

/// `(call site, target)` of every call the finding reads in the segment
/// evaluator, plus the camera setters' own entry bytes.
const CALLS: [(u32, u32, &str); 8] = [
    (0x4ea57f, 0x4d1d50, "Object3d position setter"),
    (0x4ea59c, 0x4d2710, "camera position setter"),
    (0x4ea631, 0x53fbf0, "vector to quaternion"),
    (0x4ea641, 0x53f920, "quaternion product"),
    (0x4ea650, 0x53f850, "quaternion normalise"),
    (0x4ea65f, 0x53fa40, "quaternion to matrix"),
    (0x4ea66e, 0x53df30, "matrix to euler"),
    (0x4ea6d0, 0x4d2490, "camera rotation setter"),
];

fn image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE")
        .expect("this retail test needs CS_ENGINE_IMAGE (the decrypted executable)");
    std::fs::read(path).expect("the engine image is readable")
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_lc_measure_cam_frame_image_calls_and_constants_are_the_cited_ones() {
    const BASE: u32 = 0x40_0000;
    let image = image();
    let at = |va: u32| (va - BASE) as usize;
    for (site, target, what) in CALLS {
        assert_eq!(image[at(site)], 0xe8, "{what}: not a call at {site:#x}");
        let rel = i32::from_le_bytes(image[at(site) + 1..at(site) + 5].try_into().expect("4"));
        assert_eq!(
            (site as i64 + 5 + i64::from(rel)) as u32,
            target,
            "{what} at {site:#x}"
        );
    }
    // Node-type dispatch after the camera setters' call sites: `dec eax; je` to
    // the camera branch, `sub eax, 4; jne` to leave anything but type 5.
    assert_eq!(
        &image[at(0x4ea564)..at(0x4ea56d)],
        &[0x8b, 0x45, 0x34, 0x48, 0x74, 0x22, 0x83, 0xe8, 0x04]
    );
    assert_eq!(
        &image[at(0x4ea69e)..at(0x4ea6a7)],
        &[0x8b, 0x45, 0x34, 0x48, 0x74, 0x1c, 0x83, 0xe8, 0x04]
    );
    // Constants: 0 (identity tolerance), 1.0, 1.0 (gimbal bound).
    let f32_at = |va: u32| f32::from_le_bytes(image[at(va)..at(va) + 4].try_into().expect("4"));
    let f64_at = |va: u32| f64::from_le_bytes(image[at(va)..at(va) + 8].try_into().expect("8"));
    assert_eq!(f32_at(0x6032c8), 0.0);
    assert_eq!(f32_at(0x6032dc), 1.0);
    assert_eq!(f64_at(0x6032e8), 1.0);
    // The product's argument order is the one the evaluator pushes: out, start
    // words (edi), exp quaternion.
    assert_eq!(
        &image[at(0x4ea636)..at(0x4ea641)],
        &[
            0x8d, 0x54, 0x24, 0x34, 0x52, 0x8d, 0x44, 0x24, 0x48, 0x57, 0x50
        ]
    );
    // Rotation setter writes its three arguments to class + 0x20, 0x24, 0x28
    // and clears bit 1 of the flag word at class + 0x10; the position setter
    // writes class + 0x14 unless that bit is set.
    assert_eq!(
        &image[at(0x4d2505)..at(0x4d2505) + 6],
        &[0x89, 0x50, 0x24, 0x8b, 0x50, 0x10]
    );
    assert_eq!(&image[at(0x4d2512)..at(0x4d2512) + 3], &[0x83, 0xe2, 0xfd]);
    assert_eq!(
        &image[at(0x4d277d)..at(0x4d277d) + 4],
        &[0xf6, 0x41, 0x10, 0x02]
    );
}
