# F09-A: BM layout and rectangular golden fixtures

Date: 2026-09-28. Task: F09-A "Implement BM layout with rectangular golden
fixtures" (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`,
section `### F09-A`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/bm.rs` (new): `BM_ENTRYPOINT`, `BM_HEADER_BYTES`,
  `BM_BYTES_PER_PIXEL`, `BM_STORED_ROW_ORDER`, `BmPlane`, `BmRawHeader`,
  `BmUnsupportedTail`, `BmFile` (`stored_plane`, `plane_offset`,
  `covered_len`, `tail`, canonical `sample` / `base` / `mask` /
  `overlay`), `BmError` and the entrypoint `read_bm`.
- `crates/cs_formats/src/lib.rs` (wiring only): `pub mod bm;`, a
  `pub use bm::{...}` line and one module-doc sentence.
- `crates/cs_formats/tests/bm.rs` (new): eight `accept_f09_a_*` tests.
- This file.

**Not created in this stage:** `crates/cs_content/src/livery.rs` and
`crates/cs_app/src/livery.rs`. The paint inputs (faction/custom colors,
decals, airframe), the composition output and its cache key belong to
F09-B (composition) and F09-C (wiring into model instances); defining them
now would mean inventing a palette/catalog shape with no consumer. Same
reasoning F07-A and F08-A recorded.

**One observable failure:** a reader that takes width before height, or
that skips or doubles the vertical flip, returns red instead of cyan at
canonical texel (0, 0) of the 2x3 fixture and
`accept_f09_a_rectangular_2x3_canonical_orientation_flips_rows_once` fails.

## Sources

- S09, pinned blob `ec196de05f532cc3c286ccf8fead363bf76e5c63`
  (`extract_bm.py`), read via `gh api .../git/blobs/<sha>`: reads
  `<HH` as `(height, width)`, then `3N`, `N`, `N`, `N`, `4N` bytes; each
  plane goes through `Image.frombytes` (first stored row = top of the
  PIL image) followed by `transpose(FLIP_TOP_BOTTOM)` before saving.
- `docs/research/FORMAT-NOTES.md`, "BM observed subset".
- `fixtures/synthetic/rectangular.bm`, `truncated.bm`, `expected.json` and
  `tools/make_synthetic_fixtures.py` (read only).

## Design decisions

- **Row order is an observed-tool claim.** Because the extractor flips
  every plane once after reading it top-first, the first stored row is the
  **bottom** row of the image the tool produces. `BM_STORED_ROW_ORDER` is
  therefore `RowOrder::BottomUp` (reusing F08's type), documented as claim
  class *observed tool*. Whether the original renderer agrees is not
  established; F09-D compares against the original.
- **One canonical orientation, one flip.** `BmFile::sample` is the only
  place the stored row order is applied; canonical texel `(x, y)` counts
  `y` from the top, as F08's `DecodedImage` does. Stored bytes remain
  available unflipped through `stored_plane` (with `plane_offset` for
  future source spans), so no caller has a reason to flip again.
- **Planes stay separate and borrowed.** Nothing is copied or composited,
  so the allocation budget is not charged (tested). The last plane is
  called `Overlay`, not specular (spec deliverable paragraph).
- **Exact covered length.** A short header or plane is a checked-read
  `unexpected_eof` naming the plane (`bm.overlay` for `truncated.bm`, at
  offset 40). Bytes after `4 + 10N` do not fail the parse and are not
  dropped: they are returned as `BmUnsupportedTail { offset, bytes }`
  (non-negotiable #1, "retain unsupported tails as variant diagnostics").
- **Empty images are rejected** as `BmError::EmptyImage`. The observed
  subset has no zero-sized image; treating a 4-byte file as a valid 0xN
  livery would be a guess.
- **Values untouched.** No color-space, gamma or alpha interpretation;
  overlay alpha is returned as stored.
- **Fixtures.** The committed 2x3 golden fixture is read with
  `include_bytes!`; its stored values in the tests are transcribed from the
  generator and `expected.json`, and the canonical table is written per
  coordinate by hand. The 3x2 counterpart, the tail case, empty headers and
  the maximum header are built by readable test code; no binaries added.

## Test inventory (`accept_f09_a_*`)

All in `crates/cs_formats/tests/bm.rs`; every one calls `read_bm`.

| Test | Covers |
| --- | --- |
| `rectangular_2x3_header_is_height_then_width` | header order, covered length 64, no tail, no allocation |
| `rectangular_2x3_planes_are_separate_and_in_stored_order` | AC01: every plane's offset and bytes; `expected.json` cross-checks |
| `rectangular_2x3_canonical_orientation_flips_rows_once` | AC01: every channel of every plane at all six canonical texels; out-of-range and non-mask lookups are `None` |
| `transposed_3x2_is_a_different_image` | row/column counts are not interchangeable |
| `truncated_fixture_names_the_short_plane` | `truncated.bm`, and every proper prefix names the right field |
| `uncovered_tail_is_kept_as_a_variant_diagnostic` | tail offset and bytes; planes unaffected |
| `empty_dimensions_are_unsupported` | zero width, height or both |
| `maximum_header_needs_every_covered_byte` | 65535x65535 header with no planes fails at `bm.base` without allocating |

## Mutation probes

Each mutation was applied to `bm.rs`, the `bm` test target run and the
file restored with `git checkout`:

| Mutation | Failing tests |
| --- | --- |
| stored row order declared top-down | 3 |
| bottom-up flip ignored in `sample` | 3 |
| width read before height | 5 |
| uncovered tail dropped | 1 |
| pixel stride computed from height | 3 |
| empty image only rejected when both sides are zero | 1 |

## Recorded unknowns

- **Stored row order in the original renderer.** Observed-tool only (see
  above); not verified against retail.
- **Other BM variants.** Whether retail BM files ever carry bytes past
  `4 + 10N`, a different plane count or a zero dimension is not known;
  such files are reported (tail diagnostic / `EmptyImage`), not
  interpreted. The retail corpus survey belongs to F09-D.
- **Meaning of the planes.** Which mask selects which paint color, and
  whether the overlay alpha composites as the tool's helper [S10] does,
  is F09-B/F09-D work; the overlay's color space and alpha convention are
  unknown.
