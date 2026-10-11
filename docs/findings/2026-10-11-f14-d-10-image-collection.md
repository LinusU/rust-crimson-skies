# F14-D.10: the image collection of the baseline inventory

Date: 2026-10-11. Task: #559 "Populate the image collection from the F08 texture
archives a GameZ container names" (follow-up of #487 / F14-D.4). Feature sheet
`specs/F08-texture-archives-and-conventional-image-decoding.md`; contract
`docs/contracts/IDENTITY-CONTENT.md`. Capability used: **`retail`** (read-only
`$CS_GAME_DIR`). `gpu` and `audio` were not used and the original game was not
run, so nothing here claims `verified_original`.

The Rally key of this task, `F14-D.10`, was already the evidence id of the
engine and gun collections (#563), so this task's evidence is
`docs/findings/evidence/F14-D.10.image.json`.

## The change

- `cs_formats::gamez::GameZTextureName` gained `data_offset` and `data_end`: the
  absolute container offsets of the entry's 44-byte record, taken from the
  reader's own position (the same wiring D.9 did for materials). Struct literals
  in `cs_content/src/mesh.rs` and `cs_app/tests/render/matrix_wide.rs` fixtures
  gained the two fields, set to 0.
- `cs_content::textures::TextureCatalog::stored_range` reports where a resolved
  texture's stored level lies inside its archive, so the baseline repeats no
  package layout.
- `retail_baseline` holds one `image` row per texture-name table entry of each of
  the nine GameZ containers. Identity `<container key>.<table index>`; display
  name the entry's decoded name exactly as stored.
- Location goes through the content session's `world` mount, as F18-D's survey
  and F10-C.02's audit do: a session selecting the container's world group,
  `world/default/texture.zbd`, `TextureCatalog::resolve` (folded request,
  exact comparison with stored spellings, no search across archives). A located
  row's span is the entry's stored level in the archive; it depends statically on
  the container's `install_file` row and the archive's.
- A name not stored exactly once, an archive that does not open, and every name
  of `planes.zbd` (no world group, so no established archive) keep their row with
  an explicit unknown (`f14.d.10.baseline.image_name_unresolved`,
  `f14.d.10.baseline.image_archive_unestablished`). Their span is the name
  entry's own record, the only original bytes the row has.
- Nothing is normalized, no row claims a consumer, rows are unavailable, and the
  denominator does not move. The collection status counts
  `name_without_archive_entry` and `name_in_container_without_established_archive`;
  the geometry report gained `images` and `images_unresolved` per container.

## Measured on this installation

9 containers, **3 985** name-table entries: planes 298, C1 565, C1B 359, C1C 279,
C2 498, C2B 285, C3 465, C4 654, C5 582.

- **Located: 10** (C2's five and C3's five extension-less names).
- **Unresolved: 3 975**, of which 298 are `planes.zbd`'s.

This is exactly what F10-C.02 measured (`docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`):
containers spell a name with a stored extension (`bldhwk_cowling..tif`), the
archives store a bare stem, and F08's lookup compares exactly. The task rules
forbid stripping or guessing, so these are explicit unknowns, not misses.

## Limits (also in the evidence report)

- The extension question is the dominant gap; resolving it is an F08/F09
  decision, not this stage's.
- `planes.zbd`'s archive is not established.
- The `rimage.zbd` and loose-file fallthrough is not walked.
- No material row has an edge onto an image row yet.

## Evidence

`docs/findings/evidence/F14-D.10.image.json`, produced by
`crates/cs_content/tests/evidence_report_f14_d_10_image.rs` and validated with
`tools/validate_evidence.py --require-pass`. Implementer: sonnet-1; no
independent review yet.
