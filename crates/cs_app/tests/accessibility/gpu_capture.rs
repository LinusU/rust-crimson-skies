//! Acceptance stage F52-D: the accessibility flows reviewed on a real GPU
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! section `### F52-D`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! The stage's declared capability is `gpu`, and F52-B's finding left one gap
//! open for it: "nothing here draws". The three geometry tests below run on
//! every push because [`objective_page_boxes`](cs_app::accessibility::gpu_capture::objective_page_boxes)
//! is a pure function of the production page; the capture test is `#[ignore]`d
//! because it needs a real adapter and no display, and the implementing and
//! reviewing agents run it with `--include-ignored`.
//!
//! The fourth test is neither: refusing a page with nothing to draw happens
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
    // Three rows at 100 % sit at 0, 20 and 40 with a height of 16: the third
    // starts below a 30-pixel viewport, the second is cut through.
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

/// The `gpu` half: three frames of the real page on the real adapter — the
/// designed scale, the largest scale and the colour filter off — each drawn,
/// measured non-uniform, written as a PNG, and each different from the others,
/// so no capture is a static fixture.
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

    let mut digests = std::collections::BTreeSet::new();
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
    }
    assert_eq!(
        digests.len(),
        cases.len(),
        "every frame differs: the scale and the filter really reached the rendered image"
    );
}
