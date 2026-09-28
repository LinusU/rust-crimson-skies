# F08-D: private texture contact sheet and full decode audit

Date: 2026-09-28. Task: F08-D "Produce a private texture contact sheet and
full decode audit" (`specs/F08-texture-archives-and-conventional-image-decoding.md`,
section `### F08-D`, AC04 and non-negotiable #5). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f08_d_`.
Capabilities used: `retail` (read-only `$CS_GAME_DIR`) plus ordinary
build/test. Evidence: `docs/findings/evidence/F08-D.json`.

## Files and the one observable failure (listed before editing)

- `tools/cs_inspect/src/textures.rs` (new): the `texture-audit` command,
  the comparison (`compare_texture`, `TextureAudit`, `Orientation`,
  `Difference`), the bounded reference reader (`read_reference`: ZIP with
  CRC-32 check, 8-bit RGB/RGBA PNG with all five filters), the contact
  sheet (`SheetBuilder`, uncompressed TGA), the JSON report, the inline
  `accept_f08_d_*` tests and the evidence harness.
- `crates/cs_content/src/textures.rs`: `TextureCatalog::resolve_entry`,
  `TextureAttempt::Entry`, `TextureResolveError::EntryNotFound`
  (`texture_entry_not_found`) and one `accept_f08_d_` test.
- Wiring: `tools/cs_inspect/src/lib.rs` (`pub mod textures;`, doc),
  `tools/cs_inspect/src/main.rs` (dispatch `texture-audit`, help text, doc),
  `tools/cs_inspect/Cargo.toml` (`miniz_oxide = "0.9"`, the package
  `cs_formats` already depends on; `Cargo.lock` gains only that edge).

**One observable failure:** before this stage nothing compared a decoded
texture with anything but its own synthetic fixture. A decoder that flipped
ZBD rows (F08-B.02 recorded the row order as *observed tool*, "not matched")
would pass every existing test. With this stage it is a `color` difference
with orientation `vertical_flip` and exit 3
(`accept_f08_d_flipped_or_transposed_reference_is_an_unexplained_difference`).
Also, `TextureCatalog::resolve` refuses repeated names by design, so no
consumer could reach both `twin` entries of an archive. `resolve_entry`
reaches them by table position.

## The pinned reference

- **Source:** mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` (`docs/research/SOURCES.md`
  S02), the same clone F08-B.02 read (`private/src/mech3ax`, tag `v0.6.0`,
  clean). Nothing is copied from it (EUPL-1.2). The reference is the *tool's*
  decode, not the original renderer.
- **Build** (the repository's `rust-toolchain` pins Rust 1.76.0, which
  rustup installed on first use):
  `cargo build --release --locked -p unzbd` →
  `target/release/unzbd`, SHA-256
  `a9c426dde913703142e025b03125d2329658bff384395884bbd80e905485e0f0` on this
  machine (aarch64-apple-darwin).
- **Extraction** (from the workspace root; writes only under `private/`):

  ```sh
  REF=$PWD/private/f08d-reference UNZBD=$PWD/private/src/mech3ax/target/release/unzbd
  cd "$CS_GAME_DIR" && for f in ZBD/rimage.zbd ZBD/*/texture.zbd ZBD/*/rtexture*.zbd; do
    mkdir -p "$REF/$(dirname "$f")" && "$UNZBD" cs textures "$f" "$REF/$f.zip"; done
  ```

  49 archives, 49 exits 0. Each ZIP holds one PNG per texture in table order
  (the tool renames repeated names `name-1`, `name-2`, …) and
  `manifest.json`. The ZIPs are original content: private only. Their
  SHA-256 values are in the audit report (`texture-audit.json`).
- **What the tool does to a texel** (read in
  `crates/mech3ax-image/src/textures.rs` and
  `crates/mech3ax-pixel-ops/src/pixel_ops/mod.rs`): each 565 word becomes 8-bit
  RGB through a lookup table; full alpha is the stored plane; direct-color
  "simple alpha" is 0 where the word is `0x0000` and 255 elsewhere; palette
  "simple alpha" is skipped (RGB output, see "Explained difference" below).
  Rows are written top row first, unflipped.

## What the command compares

```sh
cs-inspect texture-audit --cs-path "$CS_GAME_DIR" --reference private/f08d-reference \
  --sheet-dir private/f08d-sheets --out private/f08d-audit.json
```

Every inventoried `.zbd` is offered to the F08-C `TextureCatalog` by its
`install:` key. The F06-A dispatch decides which ones are texture packages:
`wrong_family` skips the file, and any other failure is a failed row. Every
entry goes through `resolve_entry` → `prepare_upload`, so the audited values
are exactly what the renderer adapter receives. Independently per texture
(non-negotiable #5):

| Check | Difference code |
| --- | --- |
| a reference image exists at the same table position | `reference_missing`; surplus images: `extra_reference_entries` |
| same name (or the tool's `name-N` for a name stored more than once) | `name` |
| same width and height | `dimensions` |
| reference has coverage iff the descriptor establishes it (plane or stored-value key) | `alpha_channel` |
| every texel's reference RGB truncates (`r>>3, g>>2, b>>3`) to the decoded 565 word | `color`, with count, first coordinate and the mirroring (if any) that would match |
| every texel's reference alpha equals the plane / the key rule | `alpha`, with count and first coordinate |
| one reference color per 565 word across the corpus | `reference expansion is inconsistent` |
| decode through the upload boundary succeeds | `decode_failed` |

Truncation is used because the 565→888 expansion is a presentation choice
(F08-B.02, non-negotiable #3). The corpus-wide consistency check stops a
reference that is merely "close" from passing. The report also counts which
expansion the reference used, but does not adopt either one.

**Explained difference (the only one):** `alpha_source_unknown`. For a palette
texture with the "simple alpha" flag, the descriptor says `Unknown` and the
tool writes an opaque RGB image. Both are consistent with "not established";
it is listed on the row, not hidden.

## Retail result (fingerprint `b4e780ab…1978`, content `a0223506…c12d`)

| | |
| --- | --- |
| texture archives | 49 (every other `.zbd` routed elsewhere by the dispatch) |
| textures | 37,004, each row count equal to the header's count read directly |
| match | 36,982 |
| explained (`alpha_source_unknown`) | 22, exactly the palette simple-alpha textures |
| unexplained | **0**; exit 0 |
| orientation | `as_stored` for all 37,004 |
| distinct 565 words shown | 31,679; the reference expanded **all** by rounding (`round(c·255/max)`), 20,580 of them also equal bit replication; no word expanded two ways |

So the production decoder and the pinned tool agree on dimensions, texel
order (top row first, left to right), alpha plane, the direct-color key rule
and every 565 value of the whole retail corpus. This is agreement between two
readers. It does **not** verify the original renderer (claim stays
`implemented`; nothing is `verified_original`).

## Contact sheet

With `--sheet-dir`, each archive gets `<spelling with / as _>.tga`:
uncompressed 32-bit TGA, top-left origin, textures at their own size on
1024-texel shelves (wider textures get their own shelf) with a 2-texel
gutter. The report gives each texture's cell and the sheet's SHA-256. The
sheet is a **review rendering**: 565 is expanded by bit replication, and
coverage comes from the plane or the stored-value key (opaque otherwise). Neither is a
runtime decision. For retail it is about 1.2 GB for the 49 archives, all
in `private/`. I looked at one sheet converted to PNG (`ZBD/C1/rtexture2.zbd`:
the mission map text reads upright, left to right). That is an agent's
glance, not `human_review`.

## Tests

| Test | Covers |
| --- | --- |
| `accept_f08_d_matching_reference_passes_with_every_texel_compared` | AC04 on two archives: 3x2 asymmetric, 2x3 plane with 0/255 edges, direct simple alpha, indexed opaque and plane, palette simple alpha (explained), a twice-stored name; exit 0, report totals, rounding expansion, contact-sheet header and texels at the reported cells |
| `accept_f08_d_flipped_or_transposed_reference_is_an_unexplained_difference` | vertical and horizontal mirroring named as such, transposition as `dimensions`; siblings still compared; first-texel detail |
| `accept_f08_d_alpha_edge_and_channel_mismatches_fail` | alpha 255→254 at an edge, plane dropped by the reference, coverage invented by the reference, one 6-bit green field off |
| `accept_f08_d_missing_extra_renamed_or_inconsistent_references_fail` | missing / surplus / renamed image (renaming accepted only for repeated names), missing and corrupt reference files as failed rows beside each other, one word expanded two ways |
| `accept_f08_d_reference_reader_undoes_every_filter_and_refuses_the_rest` | CRC-32 check value, all five PNG filters across two IDAT chunks, indexed PNG refused, CRC mismatch, missing file |
| `accept_f08_d_cli_refuses_bad_input_and_a_missing_installation` | exit 4 without an installation, 2 for bad flags, missing `--reference` or reference dir, and `--out`/`--sheet-dir` inside the installation (nothing written) |
| `accept_f08_d_resolve_entry_reaches_duplicate_names_by_position` (`cs_content`) | both `twin` entries, own texels, table-length error, foreign session |
| `accept_f08_d_retail_every_texture_matches_the_pinned_reference` (ignored without `CS_GAME_DIR`) | the retail result above; panics without the installation or the reference extraction (`CS_F08_D_REFERENCE`, default `private/f08d-reference`) |

The synthetic reference colors are computed in the test from the authored
565 words (rounding expansion). The PNGs are written with stored zlib blocks
and every filter type, and the ZIPs by hand. None of this uses the code
under test.

Mutation probes (applied, the `accept_f08_d_` tests of the crate run,
restored):

| Mutation | Failing tests |
| --- | --- |
| reference compared vertically flipped | 5 |
| alpha comparison skipped | `alpha_edge_…` |
| expansion consistency not checked | `missing_extra_…` |
| name not checked | `missing_extra_…` |
| surplus reference images not counted | `missing_extra_…` |
| PNG Paeth filter undone as Up | `reference_reader_…` |
| ZIP CRC not checked | `reference_reader_…` |
| stored-value key coverage not expected | 5 |
| dimensions compared by area only | `flipped_or_transposed_…` |
| `resolve_entry` always returns entry 0 | `resolve_entry_…` |

## Recorded unknowns (not guessed, unchanged by this audit)

- Everything the F08-B findings list as presentation unknowns stays open:
  color space, alpha test, how the **original renderer** expands 565 (the
  tool rounds; that says nothing about the game), stretch, flag bit 0, bits
  5–7, and whether palette "simple alpha" has a transparent index.
- Row order and the key rule are now confirmed against the pinned tool on
  the whole corpus, not against the original renderer. Only a renderer
  observation (GPU capture or reference-executable research, owner-run) can
  move them past *observed tool*.
- The BMP (`00000409.*`) and TGA (`GOSDATA/ASSETS/GRAPHICS/*.tga`) images are
  outside this audit. The pinned tool has no reader for them, and F08-C gave
  them no catalog role. Their decode stays covered only by the F08-B
  differential and census tests.
