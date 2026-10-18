use std::path::Path;

use cs_xtask::parity::*;
use image::{Rgb, RgbImage};

fn save(dir: &Path, name: &str, w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) {
    std::fs::create_dir_all(dir).unwrap();
    RgbImage::from_fn(w, h, |x, y| Rgb(f(x, y)))
        .save(dir.join(name))
        .unwrap();
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cs_parity_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn accept_parity_m01_compare_names_carry_time() {
    assert_eq!(
        time_from_name("camp1fly_t0005.png", TimeUnit::Seconds),
        Some(5.0)
    );
    assert_eq!(
        time_from_name("shot_t12.5.png", TimeUnit::Seconds),
        Some(12.5)
    );
    assert_eq!(
        time_from_name("frame_00060_world.png", TimeUnit::Frames60),
        Some(1.0)
    );
    assert_eq!(time_from_name("5000.png", TimeUnit::Millis), Some(5.0));
    assert_eq!(time_from_name("nothing.png", TimeUnit::Seconds), None);
}

#[test]
fn accept_parity_m01_compare_letterbox_is_4_3_with_bars() {
    let wide = RgbImage::from_pixel(1920, 800, Rgb([200, 100, 50]));
    let out = letterbox(&wide);
    assert_eq!(out.dimensions(), (WIDTH, HEIGHT));
    assert_eq!(out.get_pixel(512, 2), &Rgb([0, 0, 0]));
    assert_eq!(out.get_pixel(512, 384), &Rgb([200, 100, 50]));
}

#[test]
fn accept_parity_m01_compare_metrics_separate_different_frames() {
    let sky = RgbImage::from_fn(WIDTH, HEIGHT, |_, y| {
        Rgb(if y < 400 {
            [120, 160, 230]
        } else {
            [30, 90, 40]
        })
    });
    let same = measure(&sky);
    assert!(diff(&same, &measure(&sky)).histogram_distance < 1e-6);
    let noisy = RgbImage::from_fn(WIDTH, HEIGHT, |x, y| {
        Rgb(if (x / 2 + y / 2) % 2 == 0 {
            [250, 250, 250]
        } else {
            [5, 5, 5]
        })
    });
    let m = measure(&noisy);
    assert!(m.edge_density > 0.2 && same.edge_density < 0.01);
    let d = diff(&same, &m);
    assert!(d.histogram_distance > 0.5);
    assert!(d.edge_density_delta > 0.2);
    assert!(d.band_delta.iter().all(|b| *b > 10.0));
}

#[test]
fn accept_parity_m01_compare_pairs_by_time_and_writes_outputs() {
    let root = scratch("e2e");
    let (r, o) = (root.join("ref"), root.join("ours"));
    // Original: 1024x768 at t=0,5,10 s. Ours: 800x600 frames at 1/60 s, offset 2 s.
    for (i, t) in [0u32, 5, 10].iter().enumerate() {
        save(&r, &format!("scn_t{t:04}.png"), 1024, 768, |_, _| {
            [100 * i as u8, 80, 120]
        });
    }
    for f in [120u32, 420, 720] {
        save(&o, &format!("frame_{f:05}_x.png"), 800, 600, |_, _| {
            [0, 80, 120]
        });
    }
    let originals = list_frames(&r, TimeUnit::Seconds).unwrap();
    let ours = list_frames(&o, TimeUnit::Frames60).unwrap();
    let c = compare(
        &originals,
        &ours,
        Options {
            offset_seconds: 2.0,
            tolerance_seconds: 0.1,
        },
    )
    .unwrap();
    assert_eq!(c.pairs.len(), 3);
    assert!(c.unpaired.is_empty());
    assert!(c.pairs[0].time_error.abs() < 1e-9);
    assert!(c.pairs[0].diff.band_delta[0] < 1.0);
    assert!(c.pairs[2].diff.band_delta[0] > 50.0);

    // Without the alignment offset nothing lies within tolerance.
    let none = compare(
        &originals,
        &ours,
        Options {
            offset_seconds: 0.0,
            tolerance_seconds: 0.1,
        },
    )
    .unwrap();
    assert!(none.pairs.is_empty());
    assert_eq!(none.unpaired.len(), 3);

    let out = root.join("private").join("sheet");
    write_outputs(&c, &out).unwrap();
    let sheet = image::open(out.join("sheet.png")).unwrap();
    assert!(sheet.height() >= 3 * THUMB_H);
    let json = std::fs::read_to_string(out.join("report.json")).unwrap();
    let md = std::fs::read_to_string(out.join("report.md")).unwrap();
    for text in [&json, &md] {
        assert!(text.contains(VERDICT));
        assert!(text.contains("scn_t0005.png") && text.contains("frame_00420_x.png"));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn accept_parity_m01_compare_refuses_output_outside_private() {
    assert!(check_private_out(Path::new("docs/findings")).is_err());
    assert!(check_private_out(Path::new("private/parity/m01")).is_ok());
}
