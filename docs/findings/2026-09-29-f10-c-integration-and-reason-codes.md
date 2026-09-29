# F10-C: the integration seam, and the reason-code contract it pins

**Task:** #43 (`F10-C`), the parent integration stage of
`specs/F10-gamez-mesh-topology-and-material-records.md`, following its slices
F10-C.01 (#364), F10-C.02 (#365) and F10-C.03 (#366).
**Shared contract:** `docs/contracts/IDENTITY-CONTENT.md`.
**Test prefix:** `accept_f10_c_`.
**Capabilities used:** ordinary build/test, and `retail` for the two
`#[ignore = "requires CS_GAME_DIR"]` tests. Implementer: bunny-2.

## What this stage is

F10-C.01 built the render mesh, F10-C.02 the material audit and F10-C.03 the
container/catalog/upload path. This stage is what happens when they meet, and
it found one real defect and one real coverage hole. Both are in the
`cs_content` half, because that is where the three meet.

## Finding 1: a blocking reason carried data, so it was not a code

`unsupported_reasons` is a catalog field: a consumer groups rows by it, asks
"how many render meshes are blocked for `texture_not_found`?", and matches the
strings. That only works if every entry is a **code** — the same cause is the
same string whatever bytes caused it. Three producers in the joined path
embedded data in the code.

| Producer | Was | Bytes on `planes.zbd` |
| --- | --- | --- |
| A name the container stores twice | `container_texture_name_duplicated: <name> table indices [79, 83, …]` | **242** |
| A reader finding | `<code>: <finding text>` | varies |
| A multi-material-group polygon | `multi_material_group_polygons:<count>` | short but still per-mesh |

Measured consequences, all on the real installation:

* **One cause, many strings.** `bldhwk_cowling..tif` is stored at 36 table
  positions, so every row refusing that name carried all 36 indices. Two rows
  with the same cause had different strings and would not have compared equal,
  so grouping by reason would have split one cause across 36 buckets.
* **Unbounded.** The longer a container stores a duplicated name, the longer the
  "code" grows. A container storing it 1 000 times produced a 6 kB reason code.
* **The set property was not reached.** F10-C.03's review had already found that
  the reasons were de-duplicated with `Vec::dedup`, which only collapses
  *neighbours*, and fixed that with an order-preserving insert. That fix was
  correct for the codes as they were and **did not help here**: with the data
  folded in, the same cause through two materials produced two different
  strings, so the set still held both. The F10-C.03 fix and this one are two
  halves of the same defect, which is why this is a parent-stage finding and
  not a F10-C.03 review miss.

**The fix** keeps the code and moves the evidence:

* `unsupported_reasons` is a **closed vocabulary of bare codes**.
  `CONTAINER_DUPLICATE_NAME` and `MULTI_MATERIAL_GROUP_POLYGONS` are now `pub
  const`s so a consumer can match on them by name rather than by a literal.
* `MaterialRow::reason_details` is new: one line per blocking reason, in
  discovery order, **not** de-duplicated. Two findings of one code with
  different stored values are two lines, because the values are the evidence.
* Nothing measured is dropped. The multi-group count was already on the row as
  `faces.multi_material_group_polygons`, so making the reason bare lost
  nothing; the container-duplicate name and positions moved to a detail line.
* A blocked row's `normalize_state` diagnostic now **prefers a detail line**,
  because a bare code says what went wrong but not which stored bytes did it.
  This is why `parse_state`/`normalize_state` diagnostics are unchanged in
  shape and richer in content.
* `MeshDependencyAudit::blocked` is prose for a human, not a code list, so it
  now names the detail lines and falls back to the codes when there are none.

**A decision worth recording:** the finding reasons are *not* wrapped in a
`"code: detail"` composite. F08-C's `ImageRecord` puts `error.code()` in
`unsupported_reasons` and keeps the prose in the diagnostic, and this follows
that precedent rather than inventing a second convention for the same contract
field. F10-C.02's `MaterialState::Display` already emits `"code: detail"` for
the state text, so the composite form is still available to a human reader.

## Finding 2: the airframe producer had no coverage

The sheet's deliverable is "world geometry **and PLANES.ZBD meshes**", and
`ZBD/planes.zbd` is a different container from a world's `gamez.zbd` in two
ways that matter to an integration test:

* it is mounted at the **install root**, not inside a world group, so it
  resolves through a different namespace and mount (`install/default/…`,
  label `install`) than the world path the F10-C.03 fixture used;
* it is the largest GameZ container in the installation.

Nothing exercised it through the joined path. `accept_f10_c_retail_airframe_meshes_reach_the_upload_payload`
now does, and on the real installation it measures:

| | `ZBD/C1/gamez.zbd` (world) | `ZBD/planes.zbd` (airframe) |
| --- | --- | --- |
| stored meshes (rows) | 2 237 | 1 766 |
| resolves and uploads | all | all |
| stored faces | 18 277 | 16 200 |
| triangles | 18 277 | 71 645 |
| degenerate | 0 | 10 497 |
| multi-material-group polygons | 1 006 (across the corpus) | **0** |
| split positions | > 0 | 20 836 |
| **UV seams (AC03)** | > 0 | **4 747** |
| material rows / refs / resolved | 561 / 22 678 / 0 | 935 / 19 810 / 0 |

Three of these are worth naming because they are facts the corpus did not
predict and no fixture asserted:

* **The airframes have no multi-material-group polygons at all.** F10-B measured
  1 006 multi-group polygons and recorded that "all of them [are] in the world
  archives"; this run confirms it on the other half of the corpus, so the
  `multi_material_group_polygons` reason can only ever fire for world geometry.
* **AC03 is not a world-only phenomenon.** 4 747 of the airframe's splits are
  genuine UV seams — two vertices at one stored position index with two
  different authored texture coordinates — so the case the sheet names in its
  minimum scenario is authored in the largest container in the installation,
  not only in a 4-corner fixture.
* **Degenerate triangles are 14.7 % of the airframe corpus** (10 497 of 71 645
  triangles) against 0 in the world container F10-C.03 measured. F10-C.01 keeps
  them and marks them; this shows the mark is load-bearing rather than
  theoretical. Whether the original renderer skipped them is still unknown (see
  below).

The test asserts the *invariants* on real data (every row resolves, every
upload agrees with its row's face counts, seams survive) rather than these
exact numbers: the numbers are this installation's, and pinning them would make
the test a checksum of one owner copy rather than a statement about the format.
The measured figures are recorded here and in the code's comments.

## Fixture work the finding forced

`accept_f10_c_a_render_mesh_row_names_each_reason_once_as_a_bare_code` needs a
container that really stores two material groups per polygon, and the
synthetic writer in `crates/cs_content/src/mesh.rs` could only ever store one
(`mat_count` was hard-coded to `1` and the corner arrays held a single UV set).
So the `multi_material_group_polygons` production branch had **no fixture at
all** and could not be reached by any test.

`StoredPolygon` now carries `extra_groups` and writes `mat_count`, one
material index per group and one UV set per group, in the order the reader
reads them (`read_polygon` reads `mat_count` indices then `mat_count` UV sets
back to back). A writer that interleaved them differently would desynchronise
the next polygon. The reader is unchanged: this is fixture capability, and it
is what let a production branch be tested for the first time.

## Test inventory (`accept_f10_c_*`)

30 tests, all in the `#[cfg(test)]` module of `crates/cs_content/src/mesh.rs`:
the 27 from the three slices, unchanged except where the reason-code contract
now applies, plus these three.

| Test | Covers |
| --- | --- |
| `..._blocking_reasons_are_stable_codes_and_keep_their_evidence` | A code carries no stored name and no position list; the detail line names both. Uses the measured `bldhwk_cowling..tif` duplicate. |
| `..._a_render_mesh_row_names_each_reason_once_as_a_bare_code` | Every reason on a render-mesh row is drawn from a **closed vocabulary**; the same cause through two materials is one entry; the multi-group reason is bare and its count is the row's number, reached through a container that really stores two groups per polygon. |
| `..._retail_airframe_meshes_reach_the_upload_payload` (`#[ignore]`) | The airframe producer, through the install mount, to the upload payload, with AC03 seams on real airframe data. |

### Sensitivity probes actually run

Each is a one-line mutation of the production or fixture code, run against
`cargo test -p cs_content --lib -- accept_f10_c_`, then reverted:

| Mutation | Tests that failed |
| --- | --- |
| Re-embed the name and the index list into the duplicate code | `..._blocking_reasons_are_stable_codes…` |
| Re-embed the count into the multi-group code | `..._a_render_mesh_row_names_each_reason_once_as_a_bare_code` |
| Fold the finding text back into its code | `..._a_render_mesh_row_names_each_reason_once_as_a_bare_code` |
| `&& false` on the multi-group reason branch | `..._a_render_mesh_row_names_each_reason_once_as_a_bare_code` |
| Fixture `group_count()` hard-coded to `1` (reason unreachable) | `..._a_render_mesh_row_names_each_reason_once_as_a_bare_code` |

The first three are the three defects; the last two show the multi-group branch
and the fixture that reaches it are both load-bearing. The retail airframe test
is measured on real data and is not mutation-probed, because a probe that
merely changes the assertion would also "pass" without the fix; the
sensitivity that matters for it is that the whole production path
(`install::discover` → `SessionBuilder` → `ZbdContainer` → both readers →
`RenderMesh` → `MeshCatalog::prepare_upload`) has to succeed for it to pass at
all.

### One existing test updated

`accept_f10_c_02_audit_reports_a_container_duplicate_as_a_reason` matched
`starts_with("container_texture_name_duplicated:")`, i.e. it asserted the old
**defect**. It now asserts the bare code and that the table positions are on
the detail line. This is the only pre-existing F10 acceptance test whose
expectation changed, and it changed because the behaviour it pinned was the
defect.

## Recorded unknowns — unchanged, and not settled by this stage

This stage fixes how a reason is *reported*. It settles none of the three
presentation unknowns, which are still on every row and still gate
`Ready`:

* **front-face winding / handedness** (`front_face_winding_unknown`);
* **the UV convention** (`uv_origin_unknown`) — the 4 747 airframe seams are
  *authored* seams, and this stage still does not know whether the original
  renderer flipped V, wrapped, or clamped them;
* **the corner-colour meaning** (`vertex_color_unknown`).

Newly measured, still unknown:

* **Whether the original renderer skipped the airframes' 10 497 degenerate
  triangles.** They are kept and marked, as F10-C.01 decided; this run shows
  they are common enough that the decision is not academic. F17-B's adapter has
  to read [`MeshUpload`]'s counts and decide; nothing here settles it.
* **Whether the airframe texture names resolve at all.** 0 of 935 airframe
  material rows resolved against the world's `texture.zbd`, and 0 against
  `rimage.zbd` and against two other archives tried by hand. Which archive the
  airframes actually use is **not established** — this is F08-C's open question
  about archive binding, and the audit refuses to answer it by searching, by
  design. Recorded, not guessed.
* **`planes.zbd` has 6 multi-material-group polygons' worth of 0.** That is, it
  has none. Whether the original renderer would have needed the second group is
  unmeasured for the airframes, because they never store one.

## Follow-ups filed, not fixed here

* The airframe texture archive is unestablished (#387) — blocks any claim that
  an airframe material is bound, and is the same open question for world
  geometry that F10-C.02 recorded.
* Multi-material-group polygons still lose their second UV set in the render
  mesh (#382, F10-E), unchanged; F10-C only made the count reportable as a
  number rather than as a reason suffix.

## Checks

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings` and `cargo test --workspace --locked`
pass. `cargo test -p cs_content --lib --locked -- accept_f10_c_ --include-ignored`
runs 30 tests, all passing, with `CS_GAME_DIR` set; without it the three retail
tests fail loudly rather than passing.

**Environment note.** `CARGO_TARGET_DIR` was made per-agent for every command
above, for the reason F10-C.03 recorded: the shared target directory let one
worktree's test run execute another worktree's binary.

## Sources

* `docs/findings/2026-09-29-f10-c-01-render-vertex-splitting.md`,
  `…-f10-c-02-gamez-material-records.md` and
  `…-f10-c-03-mesh-container-catalog-and-upload.md` — the three slices.
* `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` — the multi-group
  polygon measurements and the layout worksheets the fixtures encode.
* `docs/contracts/IDENTITY-CONTENT.md` — the catalog element fields and the
  "collections cannot exclude failed entries" rule.
* `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md` — the
  `unsupported_reasons`-as-codes precedent this stage follows.

No code was copied from any reference. No original game data is committed; the
retail tests read `$CS_GAME_DIR` only.
