//! Task #2039: where the engine draws a pane with `scale`, `rotate` and
//! `origin` applied, so the cabin's `PC_MEMENTO` picture can be placed.
//! Task test prefix: `accept_parity5_cabin_memento_pane_transform_`.
//!
//! The synthetic tests run the production [`PaneTransform`] model; the image
//! test re-reads the cited bytes of the decrypted engine image and is ignored
//! in CI.

use cs_app::ui::front_end::{
    CABIN_MEMENTO, CABIN_MEMENTO_POSITION, PaneTransform, SCALE_UNIT, pane_rotate_radians,
};

const PICTURE: (u32, u32) = (362, 460);

fn render() -> (f32, f32) {
    (
        CABIN_MEMENTO_POSITION.0 as f32,
        CABIN_MEMENTO_POSITION.1 as f32,
    )
}

fn near(a: (f32, f32), b: (f32, f32)) {
    assert!(
        (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01,
        "{a:?} != {b:?}"
    );
}

/// **Scale is percent, rotate is degrees with the engine's own constants.**
#[test]
fn accept_parity5_cabin_memento_pane_transform_units() {
    let (sx, sy) = CABIN_MEMENTO.scale_factors();
    assert!((sx - 0.2).abs() < 1e-6 && (sy - 0.2).abs() < 1e-6);
    // 3 * (1/360) * 6.28, not 3 * pi / 180.
    let radians = pane_rotate_radians(3);
    assert!((radians - 0.052_333_3).abs() < 1e-6, "{radians}");
    assert!((radians - 3.0_f32.to_radians()).abs() > 1e-5);
    assert_eq!(SCALE_UNIT, 0.01);
}

/// **The cabin picture's four corners.** 362x460 at 20 % is 72.4x92, turned 3
/// degrees counter-clockwise about its top left, which lands on the render
/// position.
#[test]
fn accept_parity5_cabin_memento_pane_transform_cabin_picture_corners() {
    let corners = CABIN_MEMENTO
        .corners(PICTURE, render())
        .expect("only z is rotated");
    near(corners[0], (169.0, 325.0));
    near(corners[1], (241.30, 321.21));
    near(corners[2], (173.81, 416.87));
    near(corners[3], (246.11, 413.09));
    let (left, top, right, bottom) = CABIN_MEMENTO.bounds(PICTURE, render()).expect("bounds");
    near((left, top), (169.0, 321.21));
    near((right, bottom), (246.11, 416.87));
}

/// **Origin is the pivot and the anchor.** Moving it to the picture's centre
/// puts that point on the render position and keeps it fixed under rotation.
#[test]
fn accept_parity5_cabin_memento_pane_transform_origin_is_pivot_and_anchor() {
    let centred = PaneTransform {
        origin: (181, 230, 0),
        ..CABIN_MEMENTO
    };
    let flat = PaneTransform {
        rotate: (0, 0, 0),
        ..centred
    };
    // Unrotated, the centre of the 72.4x92 box is on the render position.
    let c = flat.corners(PICTURE, render()).unwrap();
    near(
        ((c[0].0 + c[3].0) / 2.0, (c[0].1 + c[3].1) / 2.0),
        (169.0, 325.0),
    );
    // Rotated, it still is.
    let r = centred.corners(PICTURE, render()).unwrap();
    near(
        ((r[0].0 + r[3].0) / 2.0, (r[0].1 + r[3].1) / 2.0),
        (169.0, 325.0),
    );
    // A positive angle turns counter-clockwise on a y-down screen: the top
    // right corner rises.
    let neutral = PaneTransform::NEUTRAL
        .corners((100, 100), (0.0, 0.0))
        .unwrap();
    near(neutral[1], (100.0, 0.0));
    let turned = PaneTransform {
        rotate: (0, 0, 90),
        ..PaneTransform::NEUTRAL
    }
    .corners((100, 100), (0.0, 0.0))
    .unwrap();
    // 90 degrees is 1.5700001 radians with the engine's constants, a
    // fraction of a degree short of a quarter turn.
    near(turned[1], (0.0796, -100.0));
}

/// **Unknown stays unknown.** An x or y rotation has no decoded composition
/// order, so the model refuses rather than guesses.
#[test]
fn accept_parity5_cabin_memento_pane_transform_x_and_y_rotation_are_refused() {
    for rotate in [(1, 0, 0), (0, 1, 0), (5, 5, 5)] {
        let transform = PaneTransform {
            rotate,
            ..CABIN_MEMENTO
        };
        assert!(transform.corners(PICTURE, render()).is_none(), "{rotate:?}");
    }
}

/// **The facts are the engine's.** Re-reads the cited bytes of the decrypted
/// image: the three constants, the keyword pointers, the dispatch table
/// entries, and the setters' instructions.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity5_cabin_memento_pane_transform_facts_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE")
        .expect("CS_ENGINE_IMAGE must name the decrypted engine image for this test");
    let image = std::fs::read(path).expect("the image reads");
    let base = 0x40_0000_u32;
    let bytes = |va: u32, n: usize| {
        let at = (va - base) as usize;
        &image[at..at + n]
    };
    let word = |va: u32| u32::from_le_bytes(bytes(va, 4).try_into().unwrap());
    let float = |va: u32| f32::from_bits(word(va));
    let text = |va: u32| {
        let s = bytes(va, 16);
        String::from_utf8(s[..s.iter().position(|b| *b == 0).unwrap()].to_vec()).unwrap()
    };

    assert_eq!(float(0x60_32d8), cs_app::ui::front_end::SCALE_UNIT);
    assert_eq!(float(0x60_b894), 1.0 / 360.0);
    assert_eq!(float(0x60_b890), cs_app::ui::front_end::TURN_RADIANS);
    assert_eq!(
        float(0x60_ee6c),
        cs_app::ui::front_end::PERSPECTIVE_DISTANCE
    );
    assert_eq!(float(0x60_32c8), 0.0);

    // The statement keyword table at 0x6402a8 holds `scale` at index 43; the
    // dispatch table at 0x5d9ad0 is indexed by keyword index minus two.
    for (index, name, handler) in [
        (43_u32, "scale", 0x5d_94fd_u32),
        (44, "rotate", 0x5d_96b1),
        (45, "origin", 0x5d_9741),
    ] {
        assert_eq!(text(word(0x64_02a8 + 4 * index)), name);
        assert_eq!(word(0x5d_9ad0 + 4 * (index - 2)), handler, "{name}");
    }

    // setscale: stores the first two values at +0x10 and +0x14.
    assert_eq!(
        bytes(0x5e_4e40, 17),
        [
            0x8b, 0x44, 0x24, 0x04, 0x8b, 0x54, 0x24, 0x08, 0x89, 0x41, 0x10, 0x89, 0x51, 0x14,
            0xc2, 0x0c, 0x00
        ]
    );
    // setrotate: three values at +0x30, +0x34 and +0x38.
    assert_eq!(
        bytes(0x5e_4e20, 24),
        [
            0x8b, 0x44, 0x24, 0x04, 0x8b, 0x54, 0x24, 0x08, 0x89, 0x41, 0x30, 0x8b, 0x44, 0x24,
            0x0c, 0x89, 0x51, 0x34, 0x89, 0x41, 0x38, 0xc2, 0x0c, 0x00
        ]
    );
}
