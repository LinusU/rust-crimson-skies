//! Acceptance stage F52-D: the accessibility flows reviewed on a real GPU
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! section `### F52-D`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! The stage's declared capability is `gpu`, and F52-B's finding left one gap
//! open for it: "nothing here draws". The five geometry tests below run on
//! every push because [`objective_page_boxes`](cs_app::accessibility::gpu_capture::objective_page_boxes)
//! is a pure function of the production page — they pin the horizontal
//! positions against their expected numbers, the clipping, the uniform
//! scale-down when the viewport does not fit the frame, and that a colour
//! filter changes only the fill. The capture test is `#[ignore]`d because it
//! needs a real adapter and no display, and the implementing and reviewing
//! agents run it with `--include-ignored`; it decodes the PNGs it wrote and
//! samples every quad, so the frames are measured, not merely counted.
//!
//! The sixth test is neither: refusing a page with nothing to draw happens
//! before any renderer starts, so CI exercises the refusal on every push.
//!
//! Nothing here is evidence about an original option or the original's
//! appearance: row geometry is designed (F52-B), the quads are the page's own
//! measurements, and no original executable runs in any agent session.

use std::fs;
use std::path::{Path, PathBuf};

use cs_app::accessibility::cues::ColourRole;
use cs_app::accessibility::gpu_capture::{
    PAGE_CAPTURE_HEIGHT, PAGE_CAPTURE_WIDTH, capture_objective_page, objective_page_boxes,
    role_colour,
};
use cs_app::accessibility::objective_page::objective_page;
use cs_app::objectives::DisplayedObjective;
use cs_app::text::{TextBox, TextCaptureError};
use cs_content::settings::{ColourFilter, MAX_UI_SCALE_PERCENT, Presentation};
use cs_script::ir::SymbolId;
use cs_sim::objectives::state::ObjectiveState;
use cs_types::content::{ContentId, ContentKind};

/// The six shown objective states, one row each, all revealed.
const STATES: [ObjectiveState; 6] = [
    ObjectiveState::Pending,
    ObjectiveState::Active,
    ObjectiveState::Succeeded,
    ObjectiveState::Failed,
    ObjectiveState::Optional,
    ObjectiveState::Superseded,
];

fn rows() -> Vec<DisplayedObjective> {
    STATES
        .iter()
        .enumerate()
        .map(|(i, state)| DisplayedObjective {
            symbol: SymbolId(i as u32 + 1),
            content: ContentId::from_source(ContentKind::Objective, &format!("synthetic.row{i}"))
                .expect("a synthetic objective id"),
            state: *state,
            revealed: true,
        })
        .collect()
}

fn presentation(colour_filter: ColourFilter, ui_scale_percent: u16) -> Presentation {
    Presentation {
        colour_filter,
        ui_scale_percent,
        ..Presentation::designed()
    }
}

/// Where captures are written: the evidence directory when the acceptance run
/// supplies one, otherwise this checkout's own private directory (ignored by
/// Git, like every other artifact of this task).
fn capture_dir() -> PathBuf {
    let dir = match std::env::var_os("CS_EVIDENCE_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                dir
            } else {
                workspace_root().join(dir)
            }
        }
        None => workspace_root().join("private/evidence/F52-D-captures"),
    };
    fs::create_dir_all(&dir).expect("the capture directory is created");
    dir
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// The glyph boxes are every second quad: each row contributes its band and
/// then its cue's glyph box, in page order.
fn glyph_boxes(boxes: &[TextBox]) -> Vec<&TextBox> {
    boxes.iter().skip(1).step_by(2).collect()
}

/// The band boxes: every first quad.
fn bands(boxes: &[TextBox]) -> Vec<&TextBox> {
    boxes.iter().step_by(2).collect()
}

/// AC02/behavior 2 at the pixel level: the colour filter changes only the
/// redundant fill. Every box's place and size are the page's measurements and
/// are identical under `monochrome`, while every glyph box loses its role.
#[test]
fn accept_f52_d_the_objectives_page_geometry_is_identical_under_every_colour_filter() {
    let viewport = PAGE_CAPTURE_HEIGHT;
    let off = objective_page(&rows(), &presentation(ColourFilter::Off, 100), viewport);
    let mono = objective_page(
        &rows(),
        &presentation(ColourFilter::Monochrome, 100),
        viewport,
    );
    let plain = objective_page_boxes(&off);
    let filtered = objective_page_boxes(&mono);

    assert_eq!(plain.len(), STATES.len() * 2, "a band and a glyph per row");
    assert_eq!(plain.len(), filtered.len());
    for (before, after) in plain.iter().zip(&filtered) {
        assert_eq!(before.center, after.center, "the filter never moves a box");
        assert_eq!(before.size, after.size, "the filter never resizes a box");
    }
    assert!(
        plain.iter().zip(&filtered).any(|(a, b)| a.color != b.color),
        "the filter changes the redundant fill, so the two frames differ"
    );

    let neutral = role_colour(ColourRole::Neutral);
    for glyph in glyph_boxes(&filtered) {
        assert_eq!(
            glyph.color, neutral,
            "monochrome leaves no colour role: the box is the neutral grey"
        );
    }
    assert!(
        glyph_boxes(&plain)
            .into_iter()
            .any(|glyph| glyph.color != neutral),
        "with the filter off a role is visible in the fill"
    );
}

/// The rows grow with the UI scale and none is dropped or moved out of order
/// (AC02: the page reads at the largest scale).
#[test]
fn accept_f52_d_a_larger_ui_scale_draws_the_same_rows_bigger_and_never_drops_one() {
    let viewport = PAGE_CAPTURE_HEIGHT;
    let small = objective_page_boxes(&objective_page(
        &rows(),
        &presentation(ColourFilter::Off, 100),
        viewport,
    ));
    let large = objective_page_boxes(&objective_page(
        &rows(),
        &presentation(ColourFilter::Off, MAX_UI_SCALE_PERCENT),
        viewport,
    ));

    assert_eq!(small.len(), STATES.len() * 2);
    assert_eq!(large.len(), small.len(), "no row is dropped at 300 %");

    let small_glyph = glyph_boxes(&small);
    let large_glyph = glyph_boxes(&large);
    let small_bands = bands(&small);
    let large_bands = bands(&large);
    assert_eq!(large_glyph[0].size[0], small_glyph[0].size[0] * 3.0);
    assert_eq!(large_glyph[0].size[1], small_glyph[0].size[1] * 3.0);
    assert_eq!(large_bands[0].size[1], small_bands[0].size[1] * 3.0);

    let pitch = |glyphs: &[&TextBox]| glyphs[1].center[1] - glyphs[0].center[1];
    assert_eq!(pitch(&large_glyph), pitch(&small_glyph) * 3.0);

    // The page's own order, top to bottom, at both scales.
    for boxes in [&small, &large] {
        let tops: Vec<f32> = bands(boxes).iter().map(|band| band.center[1]).collect();
        assert!(
            tops.windows(2).all(|pair| pair[0] > pair[1]),
            "rows run downwards in the display's order: {tops:?}"
        );
    }
}

/// What is outside the viewport is not drawn, and what is inside is drawn at
/// its own measured size: a half-visible row shows its own half, never a row
/// shrunk to fit the frame.
#[test]
fn accept_f52_d_only_the_part_of_a_row_inside_the_viewport_is_drawn() {
    // The six rows at 100 % sit at 0, 20, 40, 60, 80 and 100 with a height of
    // 16: the third starts below a 30-pixel viewport, the second is cut
    // through.
    let boxes = objective_page_boxes(&objective_page(
        &rows(),
        &presentation(ColourFilter::Off, 100),
        30,
    ));
    assert_eq!(boxes.len(), 4, "the first row whole, the second clipped");
    assert_eq!(
        boxes[0].size[1], 16.0,
        "the fully visible row keeps its height"
    );
    assert_eq!(
        boxes[0].center[1] + boxes[0].size[1] * 0.5,
        PAGE_CAPTURE_HEIGHT as f32 * 0.5,
        "the page's first row starts at the frame's top"
    );
    assert_eq!(
        boxes[2].size[1], 10.0,
        "the clipped band is clipped, not moved"
    );
    assert_eq!(boxes[3].size[1], 10.0, "and so is its glyph box");

    // A viewport that fits nothing draws nothing at all, which is what the
    // capture below refuses on.
    let none = objective_page_boxes(&objective_page(
        &rows(),
        &presentation(ColourFilter::Off, 100),
        0,
    ));
    assert!(none.is_empty());
}

/// The horizontal geometry, asserted against its **expected numbers** and not
/// only against itself: the cue column sits at the designed padding and glyph,
/// the band starts where the cue ends and reaches the frame's right edge, and
/// the two quads meet without overlapping. A sign or half-width error in the x
/// mapping cannot pass by agreeing with itself.
#[test]
fn accept_f52_d_the_quads_sit_at_their_measured_horizontal_positions() {
    let page = objective_page(
        &rows(),
        &presentation(ColourFilter::Off, 100),
        PAGE_CAPTURE_HEIGHT,
    );
    let boxes = objective_page_boxes(&page);
    assert_eq!(boxes.len(), STATES.len() * 2, "a band and a glyph per row");

    let frame_w = PAGE_CAPTURE_WIDTH as f32;
    let half_w = frame_w * 0.5;
    let glyph = page.metrics.glyph_px as f32;
    // The cue column is half a glyph of designed padding and then the glyph;
    // the band begins where the cue ends and runs to the page's right edge,
    // which at 1:1 is the frame's right edge.
    let cue_centre = glyph - half_w;
    let band_from = glyph * 1.5;
    let band_centre = (band_from + frame_w) * 0.5 - half_w;

    for (index, pair) in boxes.chunks(2).enumerate() {
        let [band, cue] = pair else {
            panic!("row {index}: a band and a glyph box, not {:?}", pair.len());
        };
        assert_eq!(
            cue.center[0], cue_centre,
            "row {index}: the glyph box is centred in the cue column"
        );
        assert_eq!(cue.size[0], glyph, "row {index}: one glyph wide");
        assert_eq!(
            band.center[0], band_centre,
            "row {index}: the band spans from the cue to the page's right edge"
        );
        assert_eq!(
            band.size[0],
            frame_w - band_from,
            "row {index}: and is as wide as that span"
        );
        assert_eq!(
            cue.center[0] + cue.size[0] * 0.5,
            band.center[0] - band.size[0] * 0.5,
            "row {index}: the cue and the band touch without overlapping"
        );
        assert_eq!(
            band.center[0] + band.size[0] * 0.5,
            half_w,
            "row {index}: at 1:1 the band reaches the frame's right edge"
        );
        assert!(
            cue.center[0] - cue.size[0] * 0.5 >= -half_w,
            "row {index}: and the cue column starts inside the frame"
        );
    }
}

/// A viewport taller than the capture frame cannot be shown 1:1, so the whole
/// page is scaled down by one uniform factor: every quad's x and y together,
/// anchored at the frame's top and centred horizontally, none dropped and
/// none outside the frame. The 1:1 tests never take this branch, and this one
/// pins the documented consequence — a scaled-down band ends short of the
/// frame's edge by the same factor as everything else.
#[test]
fn accept_f52_d_a_viewport_that_does_not_fit_the_frame_scales_the_whole_page_down() {
    let fitted_page = PAGE_CAPTURE_HEIGHT * 2;
    let one = objective_page_boxes(&objective_page(
        &rows(),
        &presentation(ColourFilter::Off, 100),
        PAGE_CAPTURE_HEIGHT,
    ));
    let scaled = objective_page_boxes(&objective_page(
        &rows(),
        &presentation(ColourFilter::Off, 100),
        fitted_page,
    ));
    assert_eq!(one.len(), STATES.len() * 2);
    assert_eq!(scaled.len(), one.len(), "scaling never drops a row");

    let half_w = PAGE_CAPTURE_WIDTH as f32 * 0.5;
    let half_h = PAGE_CAPTURE_HEIGHT as f32 * 0.5;
    let scale = 0.5_f32;
    for (index, (before, after)) in one.iter().zip(&scaled).enumerate() {
        assert_eq!(
            after.size[0],
            before.size[0] * scale,
            "row {index}: the band scales with the page"
        );
        assert_eq!(
            after.size[1],
            before.size[1] * scale,
            "row {index}: and so does its height"
        );
        assert_eq!(
            after.center[0],
            before.center[0] * scale,
            "row {index}: a page that does not fit stays centred horizontally"
        );
        assert_eq!(
            after.center[1],
            half_h - (half_h - before.center[1]) * scale,
            "row {index}: and stays anchored at the frame's top"
        );
        assert!(
            after.center[0].abs() + after.size[0] * 0.5 <= half_w
                && after.center[1].abs() + after.size[1] * 0.5 <= half_h,
            "row {index}: every quad stays inside the frame"
        );
    }

    // The documented consequence: at half scale the band ends at half the
    // frame's width, not at its edge.
    assert_eq!(
        scaled[0].center[0] + scaled[0].size[0] * 0.5,
        half_w * scale,
        "a scaled-down band ends short of the frame's edge by the same factor"
    );
    assert!(
        scaled[0].center[0] + scaled[0].size[0] * 0.5 < half_w,
        "…which is strictly inside the frame"
    );
}

/// The refusal half: a page with nothing to draw is reported by name and no
/// PNG is written, so a file in the evidence directory is always a picture
/// that was taken. This runs before any renderer starts, so CI runs it.
#[test]
fn accept_f52_d_a_capture_with_nothing_to_draw_is_refused_without_writing_a_file() {
    let dir = capture_dir();
    let png = dir.join("f52-d-refused.png");
    let _ = fs::remove_file(&png);

    let hidden: Vec<DisplayedObjective> = rows()
        .into_iter()
        .map(|mut row| {
            row.revealed = false;
            row
        })
        .collect();
    let page = objective_page(
        &hidden,
        &presentation(ColourFilter::Off, 100),
        PAGE_CAPTURE_HEIGHT,
    );
    assert!(page.lines.is_empty(), "an unrevealed row is never listed");

    let error = capture_objective_page("hidden", &page, &png)
        .expect_err("a page with nothing to draw is refused");
    assert!(matches!(error, TextCaptureError::NoVisibleLines));
    assert!(!png.exists(), "a refused capture leaves no file");

    let error = capture_objective_page(
        "empty viewport",
        &objective_page(&rows(), &presentation(ColourFilter::Off, 100), 0),
        &png,
    )
    .expect_err("an empty viewport is refused");
    assert!(matches!(error, TextCaptureError::NoVisibleLines));
    assert!(!png.exists());
}

/// The written PNG read back as RGBA8 bytes, row-major from the top: the
/// production decoder, the same one F45-D reads its captures with.
fn read_png(path: &Path) -> Vec<u8> {
    image::ImageReader::open(path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()))
        .decode()
        .unwrap_or_else(|error| panic!("decode {}: {error}", path.display()))
        .into_rgba8()
        .into_raw()
}

/// One pixel of an RGBA8 frame, as `[r, g, b]`.
fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let at = ((y * width + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2]]
}

/// The `gpu` half: three frames of the real page on the real adapter — the
/// designed scale, the largest scale and the colour filter off — each drawn,
/// measured non-uniform, written as a PNG, and each different from the others,
/// so no capture is a static fixture.
///
/// Each frame is then decoded back and **sampled at the centre of every quad
/// [`objective_page_boxes`] asked for**: the pixel has to be that quad's own
/// fill, and what no quad asked for has to be one untouched background shared
/// by all three frames. A frame that is merely non-uniform — a stray
/// rectangle, a misplaced column, a band drawn at half width — does not pass,
/// so what this witnesses is the geometry reaching the image, not just a
/// renderer reporting that it drew something.
#[test]
#[ignore = "requires a GPU adapter; run with --include-ignored"]
fn accept_f52_d_a_real_gpu_capture_draws_the_objectives_page_at_the_ui_scale() {
    let dir = capture_dir();
    let cases = [
        ("scale-100", ColourFilter::Off, 100, "f52-d-scale-100.png"),
        (
            "scale-300",
            ColourFilter::Off,
            MAX_UI_SCALE_PERCENT,
            "f52-d-scale-300.png",
        ),
        (
            "monochrome-100",
            ColourFilter::Monochrome,
            100,
            "f52-d-monochrome-100.png",
        ),
    ];

    let half_w = PAGE_CAPTURE_WIDTH as f32 * 0.5;
    let half_h = PAGE_CAPTURE_HEIGHT as f32 * 0.5;
    let mut digests = std::collections::BTreeSet::new();
    let mut backgrounds = std::collections::BTreeSet::new();
    for (label, filter, scale, file) in cases {
        let page = objective_page(&rows(), &presentation(filter, scale), PAGE_CAPTURE_HEIGHT);
        let png = dir.join(file);
        let capture = capture_objective_page(label, &page, &png)
            .unwrap_or_else(|error| panic!("the {label} capture drew nothing: {error}"));
        assert!(
            capture.drew_lines(),
            "the {label} frame must differ from its clear colour"
        );
        assert_eq!(capture.width, PAGE_CAPTURE_WIDTH);
        assert_eq!(capture.height, PAGE_CAPTURE_HEIGHT);
        assert!(capture.covered_pixels > 0);
        assert!(capture.lines > 0, "the frame carries the boxes it drew");
        assert!(capture.png_bytes > 0);
        assert!(
            png.is_file(),
            "the capture's PNG {} must exist on disk",
            png.display()
        );
        assert!(
            !capture.adapter.is_empty(),
            "the adapter the frame was drawn on is recorded"
        );
        digests.insert(capture.png_sha256.to_hex());

        // The witness: every requested quad really is that quad's fill, at the
        // place the pure function put it.
        let boxes = objective_page_boxes(&page);
        assert_eq!(
            boxes.len(),
            STATES.len() * 2,
            "{label}: a band and a glyph per row"
        );
        let rgba = read_png(&png);
        for (index, quad) in boxes.iter().enumerate() {
            let left = quad.center[0] + half_w - quad.size[0] * 0.5;
            let top = half_h - quad.center[1] - quad.size[1] * 0.5;
            assert!(
                left >= 0.0 && top >= 0.0,
                "{label}: quad {index} starts inside the frame"
            );
            let x = (left + quad.size[0] * 0.5).round() as u32;
            let y = (top + quad.size[1] * 0.5).round() as u32;
            let got = pixel(&rgba, PAGE_CAPTURE_WIDTH, x, y);
            for (channel, byte) in got.iter().enumerate() {
                // The authored fills are f32, so the encoder's byte is the
                // rounded product: one byte of tolerance for the GPU's own
                // rounding, nothing more.
                let want = (quad.color[channel] * 255.0).round().clamp(0.0, 255.0) as i64;
                assert!(
                    (i64::from(*byte) - want).abs() <= 1,
                    "{label}: quad {index} pixel ({x},{y}) channel {channel} is {byte}, the box \
                     asked for {want}"
                );
            }
        }
        // …and what no quad asked for is one untouched background, identical
        // in all three frames: neither the scale nor the filter repaints it.
        let bottom = (0..PAGE_CAPTURE_WIDTH)
            .map(|x| pixel(&rgba, PAGE_CAPTURE_WIDTH, x, PAGE_CAPTURE_HEIGHT - 1))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            bottom.len(),
            1,
            "{label}: the frame below the page is one untouched colour"
        );
        backgrounds.insert(bottom.into_iter().next().expect("one background"));
    }
    assert_eq!(
        backgrounds.len(),
        1,
        "every frame is drawn on the same untouched background: {backgrounds:?}"
    );
    assert_eq!(
        digests.len(),
        cases.len(),
        "every frame differs: the scale and the filter really reached the rendered image"
    );
}
