//! Acceptance tests for the BM layout and its rectangular golden fixtures
//! (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`, stage
//! `### F09-A`, AC01).
//!
//! The 2x3 golden fixture is `fixtures/synthetic/rectangular.bm`, authored
//! by `tools/make_synthetic_fixtures.py` independently of this crate. Its
//! stored values below are transcribed from that generator (and the
//! `rgb_first` / `rgb_last` / `mask_first` / `rgba_last` entries of
//! `fixtures/synthetic/expected.json`), never from our reader's output. The
//! canonical tables are written per coordinate by hand, applying the
//! observed bottom-up row order once.
//!
//! The 3x2 counterpart and the malformed inputs are built from readable
//! byte-building code here: no new binary blobs are committed.

use cs_formats::texture::RowOrder;
use cs_formats::{
    BM_BYTES_PER_PIXEL, BM_HEADER_BYTES, BM_STORED_ROW_ORDER, BmError, BmPlane, ParseContext,
    ParseErrorKind, read_bm,
};

const RECTANGULAR: &[u8] = include_bytes!("../../../fixtures/synthetic/rectangular.bm");
const TRUNCATED: &[u8] = include_bytes!("../../../fixtures/synthetic/truncated.bm");

const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const YELLOW: [u8; 3] = [255, 255, 0];
const CYAN: [u8; 3] = [0, 255, 255];
const MAGENTA: [u8; 3] = [255, 0, 255];

/// One canonical texel: `(x, y from the top)`, base RGB, the three masks,
/// overlay RGBA.
type CanonicalTexel = ((u32, u32), [u8; 3], [u8; 3], [u8; 4]);

/// Builds a BM with the header fields in on-disk order and the planes given
/// in stored order.
fn build_bm(
    height: u16,
    width: u16,
    base: &[u8],
    masks: [&[u8]; 3],
    overlay: &[u8],
    tail: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(base);
    for mask in masks {
        bytes.extend_from_slice(mask);
    }
    bytes.extend_from_slice(overlay);
    bytes.extend_from_slice(tail);
    bytes
}

/// A 3-wide, 2-high image: the transpose of the golden fixture's shape.
/// Stored rows (bottom first): red green blue / yellow cyan magenta.
fn three_by_two(tail: &[u8]) -> Vec<u8> {
    let base: Vec<u8> = [RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA].concat();
    build_bm(
        2,
        3,
        &base,
        [
            &[1, 2, 3, 4, 5, 6],
            &[11, 12, 13, 14, 15, 16],
            &[21, 22, 23, 24, 25, 26],
        ],
        &(0u8..24).collect::<Vec<_>>(),
        tail,
    )
}

fn parse(name: &str, bytes: &[u8]) -> Result<(u32, u32), BmError> {
    let mut context = ParseContext::with_defaults(name);
    read_bm(&mut context, bytes).map(|file| (file.width(), file.height()))
}

#[test]
fn accept_f09_a_rectangular_2x3_header_is_height_then_width() {
    let mut context = ParseContext::with_defaults("fixtures/synthetic/rectangular.bm");
    let file = read_bm(&mut context, RECTANGULAR).expect("the golden fixture is valid");

    assert_eq!(file.header().height, 3);
    assert_eq!(file.header().width, 2);
    assert_eq!((file.width(), file.height()), (2, 3));
    assert_eq!(RECTANGULAR.len(), 64, "expected.json: bytes");
    assert_eq!(file.covered_len(), 64);
    assert_eq!(BM_HEADER_BYTES + BM_BYTES_PER_PIXEL * 6, 64);
    assert!(file.tail().is_none());
    assert_eq!(context.allocation().used(), 0, "planes are borrowed");
}

#[test]
fn accept_f09_a_rectangular_2x3_planes_are_separate_and_in_stored_order() {
    let mut context = ParseContext::with_defaults("fixtures/synthetic/rectangular.bm");
    let file = read_bm(&mut context, RECTANGULAR).expect("the golden fixture is valid");

    let expected: [(BmPlane, u64, Vec<u8>); 5] = [
        (
            BmPlane::Base,
            4,
            [RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA].concat(),
        ),
        (BmPlane::Mask1, 22, vec![0, 51, 102, 153, 204, 255]),
        (BmPlane::Mask2, 28, vec![255, 204, 153, 102, 51, 0]),
        (BmPlane::Mask3, 34, vec![0, 255, 0, 255, 0, 255]),
        (
            BmPlane::Overlay,
            40,
            vec![
                10, 20, 30, 0, 40, 50, 60, 51, 70, 80, 90, 102, 100, 110, 120, 153, 130, 140, 150,
                204, 160, 170, 180, 255,
            ],
        ),
    ];
    for (plane, offset, bytes) in expected {
        assert_eq!(file.plane_offset(plane), offset, "{plane:?} offset");
        assert_eq!(
            file.stored_plane(plane),
            bytes.as_slice(),
            "{plane:?} bytes"
        );
    }

    // expected.json cross-checks, all in stored order.
    let base = file.stored_plane(BmPlane::Base);
    assert_eq!(base[..3], [255, 0, 0], "rgb_first");
    assert_eq!(base[15..], [255, 0, 255], "rgb_last");
    let mask_first =
        [BmPlane::Mask1, BmPlane::Mask2, BmPlane::Mask3].map(|plane| file.stored_plane(plane)[0]);
    assert_eq!(mask_first, [0, 255, 0], "mask_first");
    assert_eq!(
        file.stored_plane(BmPlane::Overlay)[20..],
        [160, 170, 180, 255],
        "rgba_last"
    );
}

#[test]
fn accept_f09_a_rectangular_2x3_canonical_orientation_flips_rows_once() {
    assert_eq!(BM_STORED_ROW_ORDER, RowOrder::BottomUp);
    let mut context = ParseContext::with_defaults("fixtures/synthetic/rectangular.bm");
    let file = read_bm(&mut context, RECTANGULAR).expect("the golden fixture is valid");

    #[rustfmt::skip]
    let canonical: [CanonicalTexel; 6] = [
        ((0, 0), CYAN,    [204, 51, 0],    [130, 140, 150, 204]),
        ((1, 0), MAGENTA, [255, 0, 255],   [160, 170, 180, 255]),
        ((0, 1), BLUE,    [102, 153, 0],   [70, 80, 90, 102]),
        ((1, 1), YELLOW,  [153, 102, 255], [100, 110, 120, 153]),
        ((0, 2), RED,     [0, 255, 0],     [10, 20, 30, 0]),
        ((1, 2), GREEN,   [51, 204, 255],  [40, 50, 60, 51]),
    ];
    for ((x, y), base, masks, overlay) in canonical {
        assert_eq!(file.base(x, y), Some(base), "base at {x},{y}");
        assert_eq!(
            file.mask(BmPlane::Mask1, x, y),
            Some(masks[0]),
            "mask1 at {x},{y}"
        );
        assert_eq!(
            file.mask(BmPlane::Mask2, x, y),
            Some(masks[1]),
            "mask2 at {x},{y}"
        );
        assert_eq!(
            file.mask(BmPlane::Mask3, x, y),
            Some(masks[2]),
            "mask3 at {x},{y}"
        );
        assert_eq!(file.overlay(x, y), Some(overlay), "overlay at {x},{y}");
        assert_eq!(
            file.sample(BmPlane::Overlay, x, y),
            Some(overlay.as_slice()),
            "sample overlay at {x},{y}"
        );
    }

    // Outside the image, and asking a non-mask plane for a mask value.
    for plane in BmPlane::ALL {
        assert_eq!(file.sample(plane, 2, 0), None);
        assert_eq!(file.sample(plane, 0, 3), None);
    }
    assert_eq!(file.base(0, 3), None);
    assert_eq!(file.overlay(2, 2), None);
    assert_eq!(file.mask(BmPlane::Base, 0, 0), None);
    assert_eq!(file.mask(BmPlane::Overlay, 0, 0), None);
}

#[test]
fn accept_f09_a_transposed_3x2_is_a_different_image() {
    let bytes = three_by_two(&[]);
    let mut context = ParseContext::with_defaults("synthetic/three_by_two.bm");
    let file = read_bm(&mut context, &bytes).expect("the 3x2 image is valid");

    assert_eq!((file.width(), file.height()), (3, 2));
    assert_eq!(file.covered_len(), 64);
    #[rustfmt::skip]
    let canonical: [CanonicalTexel; 6] = [
        ((0, 0), YELLOW,  [4, 14, 24], [12, 13, 14, 15]),
        ((1, 0), CYAN,    [5, 15, 25], [16, 17, 18, 19]),
        ((2, 0), MAGENTA, [6, 16, 26], [20, 21, 22, 23]),
        ((0, 1), RED,     [1, 11, 21], [0, 1, 2, 3]),
        ((1, 1), GREEN,   [2, 12, 22], [4, 5, 6, 7]),
        ((2, 1), BLUE,    [3, 13, 23], [8, 9, 10, 11]),
    ];
    for ((x, y), base, masks, overlay) in canonical {
        assert_eq!(file.base(x, y), Some(base), "base at {x},{y}");
        let found = [BmPlane::Mask1, BmPlane::Mask2, BmPlane::Mask3]
            .map(|plane| file.mask(plane, x, y).expect("inside the image"));
        assert_eq!(found, masks, "masks at {x},{y}");
        assert_eq!(file.overlay(x, y), Some(overlay), "overlay at {x},{y}");
    }
    assert_eq!(file.base(0, 2), None, "only two rows");
}

#[test]
fn accept_f09_a_truncated_fixture_names_the_short_plane() {
    let error = parse("fixtures/synthetic/truncated.bm", TRUNCATED)
        .expect_err("one byte short of the covered length");
    let BmError::Parse(parse_error) = &error else {
        panic!("expected a parse error, got {error:?}");
    };
    assert_eq!(parse_error.kind, ParseErrorKind::UnexpectedEof);
    assert_eq!(parse_error.field, "bm.overlay");
    assert_eq!(parse_error.offset, 40);
    assert_eq!(parse_error.container, "fixtures/synthetic/truncated.bm");
    assert_eq!(error.code(), "unexpected_eof");
    assert_eq!(error.container(), "fixtures/synthetic/truncated.bm");

    // Every shorter prefix fails too, never panics; the header prefixes name
    // the header field.
    for len in 0..RECTANGULAR.len() {
        let error = parse("synthetic/prefix.bm", &RECTANGULAR[..len])
            .expect_err("every proper prefix is short");
        let BmError::Parse(parse_error) = error else {
            panic!("prefix {len}: expected a parse error");
        };
        assert_eq!(
            parse_error.kind,
            ParseErrorKind::UnexpectedEof,
            "prefix {len}"
        );
        let field = match len {
            0..=1 => "bm.header.height",
            2..=3 => "bm.header.width",
            4..=21 => "bm.base",
            22..=27 => "bm.mask1",
            28..=33 => "bm.mask2",
            34..=39 => "bm.mask3",
            _ => "bm.overlay",
        };
        assert_eq!(parse_error.field, field, "prefix {len}");
    }
}

#[test]
fn accept_f09_a_uncovered_tail_is_kept_as_a_variant_diagnostic() {
    let mut bytes = RECTANGULAR.to_vec();
    bytes.extend_from_slice(&[0xAB, 0xCD, 0xEF]);
    let mut context = ParseContext::with_defaults("synthetic/tail.bm");
    let file = read_bm(&mut context, &bytes).expect("the covered subset still reads");

    let tail = file.tail().expect("the extra bytes are reported");
    assert_eq!(tail.offset, 64);
    assert_eq!(tail.bytes, [0xAB, 0xCD, 0xEF]);
    assert_eq!(file.covered_len(), 64);
    // The tail does not shift or leak into any plane.
    assert_eq!(file.overlay(1, 0), Some([160, 170, 180, 255]));
    assert_eq!(
        file.stored_plane(BmPlane::Overlay).len(),
        24,
        "overlay is exactly 4N bytes"
    );

    let transposed = three_by_two(&[7]);
    let mut context = ParseContext::with_defaults("synthetic/three_by_two_tail.bm");
    let file = read_bm(&mut context, &transposed).expect("the covered subset still reads");
    let tail = file.tail().expect("one extra byte");
    assert_eq!((tail.offset, tail.bytes), (64, [7u8].as_slice()));
}

#[test]
fn accept_f09_a_empty_dimensions_are_unsupported() {
    for (height, width) in [(0u16, 2u16), (3, 0), (0, 0)] {
        let bytes = build_bm(height, width, &[], [&[], &[], &[]], &[], &[]);
        let error = parse("synthetic/empty.bm", &bytes).expect_err("no empty image");
        assert_eq!(
            error,
            BmError::EmptyImage {
                container: "synthetic/empty.bm".to_owned(),
                height,
                width,
            }
        );
        assert_eq!(error.code(), "empty_image");
        assert!(error.to_string().contains(&format!("{width}x{height}")));
    }
}

#[test]
fn accept_f09_a_maximum_header_needs_every_covered_byte() {
    // 65535 x 65535 declares ~42.9 GB of planes; with only the header present
    // the parse must refuse at the base plane without allocating anything.
    let bytes = build_bm(u16::MAX, u16::MAX, &[], [&[], &[], &[]], &[], &[]);
    let mut context = ParseContext::with_defaults("synthetic/huge.bm");
    let error = read_bm(&mut context, &bytes).expect_err("planes missing");
    let BmError::Parse(parse_error) = error else {
        panic!("expected a parse error");
    };
    assert_eq!(parse_error.field, "bm.base");
    assert_eq!(parse_error.offset, 4);
    assert_eq!(context.allocation().used(), 0);
}
