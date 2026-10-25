//! Does the loader build the measured muzzle-burst art from retail? (#2105)
//! Task test prefix: `accept_f27_c_fx_orig_draw_b_`.
//!
//! Measured in
//! `docs/findings/2026-10-24-f27-c-fx-orig-muzzle-effect-binding.md`. Every
//! test reads `$CS_GAME_DIR` (never writes it) and fails loudly without it.

use std::path::PathBuf;

use cs_app::mission_session::muzzle_burst_art::{
    MuzzleBurstArt, MuzzleBurstArtError, load_muzzle_burst_art,
};

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: #2105 needs the retail capability")
        }),
    )
}

fn load(group: &str, weapon: &str) -> Result<MuzzleBurstArt, MuzzleBurstArtError> {
    load_muzzle_burst_art(&game_dir(), group, weapon)
}

fn width_and_z(art: &cs_app::mission_session::muzzle_burst_art::BurstQuad) -> (f32, f32, f32) {
    let xs = art.corners.iter().map(|c| c[0]);
    let zs = art.corners.iter().map(|c| c[2]);
    (
        xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min),
        zs.clone().fold(f32::MAX, f32::min),
        zs.fold(f32::MIN, f32::max),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_draw_b_wep_50_loads_the_measured_slug_art() {
    let art = load("C1", "wep_50").expect("wep_50 binds muzzle_burst_slug");
    assert_eq!(art.burst, "muzzle_burst_slug");
    // mb1: one quad 0.34 wide, z from -0.195 to 0.4.
    assert_eq!(art.flash.quads.len(), 1);
    let quad = &art.flash.quads[0];
    let (width, low, high) = width_and_z(quad);
    assert!((width - 0.34).abs() < 1e-6, "{width}");
    assert!((low + 0.195).abs() < 1e-6, "{low}");
    assert_eq!(high, 0.4);
    assert_eq!(quad.texture, "slug_muzzle1.tif");
    // mb_spinflame: three quads, 60 degrees apart about z.
    assert_eq!(art.spin.quads.len(), 3);
    let angle = |q: &cs_app::mission_session::muzzle_burst_art::BurstQuad| {
        // The quad's centre-line direction in the XY plane, folded to [0, 180).
        let (x, y) = (
            q.corners[0][0] - q.corners[1][0],
            q.corners[0][1] - q.corners[1][1],
        );
        let (x, y) = if x.abs() + y.abs() < 1e-6 {
            (
                q.corners[0][0] - q.corners[3][0],
                q.corners[0][1] - q.corners[3][1],
            )
        } else {
            (x, y)
        };
        y.atan2(x).to_degrees().rem_euclid(180.0)
    };
    let mut angles: Vec<f32> = art.spin.quads.iter().map(angle).collect();
    angles.sort_by(f32::total_cmp);
    for pair in angles.windows(2) {
        assert!((pair[1] - pair[0] - 60.0).abs() < 0.01, "{angles:?}");
    }
    assert!(
        art.spin
            .quads
            .iter()
            .all(|q| q.texture == "slug_muzzle2.tif")
    );
    assert_eq!(art.spin_rolls_deg, vec![30.0, 80.0, 140.0]);
    // Non-empty decoded textures for both names.
    let names: Vec<&str> = art.textures.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["slug_muzzle1.tif", "slug_muzzle2.tif"]);
    for texture in &art.textures {
        let extent = texture.image.extent();
        assert!(extent.width > 0 && extent.height > 0, "{}", texture.name);
        assert!(!texture.image.texels().is_empty(), "{}", texture.name);
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_draw_b_every_gun_kind_loads_its_own_textures() {
    for (weapon, burst, kind) in [
        ("wep_51", "muzzle_burst_dum", "dum"),
        ("wep_52", "muzzle_burst_ap", "ap"),
        ("wep_53", "muzzle_burst_mag", "mag"),
    ] {
        let art = load("C1", weapon).unwrap_or_else(|e| panic!("{weapon}: {e}"));
        assert_eq!(art.burst, burst);
        assert_eq!(art.flash.quads[0].texture, format!("{kind}_muzzle1.tif"));
        assert_eq!(art.textures.len(), 2, "{weapon}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_draw_b_refuses_by_name() {
    // A rocket row names no muzzle burst.
    let error = load("C1", "wep_06").expect_err("wep_06 has no muzzle_burst binding");
    assert_eq!(
        error,
        MuzzleBurstArtError::NoBinding {
            weapon: "wep_06".to_owned()
        }
    );
    assert!(error.to_string().contains("wep_06"));
    // A row that does not exist.
    assert!(matches!(
        load("C1", "wep_9999"),
        Err(MuzzleBurstArtError::NoBinding { .. })
    ));
    // muzzle_burst2 is bound but its art is not a measured gun burst.
    let error = load("C1", "wep_23").expect_err("muzzle_burst2 is not a gun burst");
    assert!(matches!(
        &error,
        MuzzleBurstArtError::NotAGunBurst { animation, .. } if animation == "muzzle_burst2"
    ));
    // A world group the installation lacks refuses by its gamez path.
    let error = load("C9", "wep_50").expect_err("no such world group");
    assert!(error.to_string().contains("zbd/c9/gamez.zbd"), "{error}");
}
