//! Acceptance for PARITY4-LENS-FLARE-B (#1859): the sun's lens flare — the
//! placement and fade rule of the engine's flare draw, the flagged mesh light
//! that makes the horizon's `sun` node a source, and the four `lflare`
//! textures. Task test prefix: `accept_parity4_lens_flare_b_`.
//!
//! Facts: `docs/findings/2026-10-21-parity4-lens-flare-engine-rule.md` §2–3
//! and `docs/findings/2026-10-22-parity4-lens-flare-a-sun-source-and-blend.md`.
//! The pure half needs nothing; the retail half reads M01's world group
//! (`ZBD/C3`) through the production readers and needs `$CS_GAME_DIR`.

use cs_app::mission_session::{HorizonSubtree, LensFlareSources, read_flare_images};
use cs_app::playtest_retail::read_world_sources;
use cs_app::render::lens_flare::{
    BIG_RING_SCALE, FlareQuad, MIN_HALF_EXTENT_PX, flare_fade, flare_quads,
};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn accept_parity4_lens_flare_b_quads_follow_the_recovered_layout() {
    // 1024 x 768, light at the upper left.
    let quads = flare_quads([1024.0, 768.0], [200.0, 100.0], 1.0);
    assert_eq!(quads.len(), 4);
    let centre = [512.0, 384.0];
    let half = 1024.0 * 0.0625;
    let at = |position: f32| {
        [
            centre[0] + position * (200.0 - centre[0]),
            centre[1] + position * (100.0 - centre[1]),
        ]
    };
    let expect = [
        (0, [200.0, 100.0], half),
        (1, at(0.5), half),
        (2, at(0.1), half * 0.5),
        (3, at(-1.0), half * BIG_RING_SCALE),
    ];
    for (quad, (texture, position, extent)) in quads.iter().zip(expect) {
        assert_eq!(quad.texture, texture);
        assert!(near(quad.centre[0], position[0]) && near(quad.centre[1], position[1]));
        assert!(near(quad.half_extent, extent), "{quad:?}");
    }
    // The big ring is the mirror of the glow through the viewport centre.
    assert!(near(quads[3].centre[0], 824.0) && near(quads[3].centre[1], 668.0));
}

#[test]
fn accept_parity4_lens_flare_b_fade_scales_the_quads_and_drops_the_tiny_ones() {
    let half = flare_quads([1000.0, 800.0], [500.0, 400.0], 0.5);
    assert!(near(half[0].half_extent, 1000.0 * 0.5 * 0.0625));
    // A fade so small every quad is under a pixel leaves nothing.
    assert!(flare_quads([1000.0, 800.0], [10.0, 10.0], 0.01).is_empty());
    // Only the big ring survives a threshold between 0.5x and 1.5x.
    let f = 1.2 * MIN_HALF_EXTENT_PX / (1000.0 * 0.0625);
    let kept: Vec<usize> = flare_quads([1000.0, 800.0], [10.0, 10.0], f)
        .iter()
        .map(|quad: &FlareQuad| quad.texture)
        .collect();
    assert_eq!(kept, [0, 1, 3]);
}

#[test]
fn accept_parity4_lens_flare_b_fade_follows_the_stored_pair() {
    // The sun's pair (5, 10000): t = 1/depth never reaches 5, so always full.
    for depth in [6.0, 100.0, 4800.0, 1.0e6] {
        assert_eq!(flare_fade(depth, 5.0, 10000.0), Some(1.0));
    }
    // A pair in range fades linearly: (max - t) / (max - min).
    let f = flare_fade(0.5, 1.0, 3.0).expect("t = 2 is inside the pair");
    assert!(near(f, 0.5));
    assert_eq!(flare_fade(2.0, 1.0, 3.0), Some(1.0));
    assert_eq!(
        flare_fade(0.25, 1.0, 3.0),
        None,
        "t = 4 >= max draws nothing"
    );
    assert_eq!(flare_fade(-1.0, 1.0, 3.0), None, "behind the camera");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_lens_flare_b_retail_m01_sun_is_the_drawn_source_and_art_decodes() {
    let root = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: this retail test needs the original installation")
    });
    let sources = read_world_sources(std::path::Path::new(&root), "C3")
        .expect("M01's world group reads through the production readers");
    let horizon = HorizonSubtree::read(sources.world().nodes()).expect("C3 stores a horizon");
    let flare = LensFlareSources::from_container(sources.world(), Some(&horizon));
    assert_eq!(flare.drawn.len(), 1, "{flare:?}");
    let sun = &flare.drawn[0];
    assert_eq!(sun.name, "sun");
    assert!(near(sun.bulb.x, 6.18489) && near(sun.bulb.y, 18.294_209));
    assert_eq!((sun.fade_min, sun.fade_max), (5.0, 10000.0));
    let held: Vec<&str> = flare.held_back.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(held, ["lens_flash", "lens_flash"], "held back, not drawn");

    let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
    let handles =
        read_flare_images(sources.textures(), &mut images).expect("lflare1..4 resolve in C3");
    for handle in &handles {
        let image = images.get(handle).expect("stored");
        assert!(image.width() > 1 && image.height() > 1);
    }
}
