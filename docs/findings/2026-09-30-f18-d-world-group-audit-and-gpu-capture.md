# F18-D: the world-group audit, the GPU capture, and what they found

Date: 2026-09-30. Task: F18-D "Audit all original world variants and stunt-critical
openings" (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
section `### F18-D`), acceptance scenario **AC04**. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`** and
**`gpu`** — this stage read the owner's original installation (`$CS_GAME_DIR`)
and rendered real stored geometry on the real adapter (Metal, Apple M3 Pro). No
`audio`, `human_play` or `human_review` was used or available: this stage plays
nothing, and it was not reviewed by a human.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/world.rs` (edited): the audit contract —
  `OpeningClass` (the five nouns the sheet's deliverable names), `WorldGroupRef`,
  `PlacementSource`, `UploadVerdict`, `RepresentativeGeometry`, `GroupFacts`,
  `WorldGroupCensus`, `TraversalRoute`, `StuntOpening`, `TraversalBlocker`,
  `StuntOpeningAudit`, `WorldGroupVerdict`, `WorldGroupBlocker`,
  `WorldAuditGap`, `WorldAuditError`, `WorldGroupAudit`, `WorldGroupAuditReport`.
- `crates/cs_app/src/world/audit.rs` (new): the measured half —
  `survey_world_groups`, `audit_world_groups`, `audit_survey`,
  `SurveyedWorldGroup`, `SurveyedContainer`, and the three declared budgets
  `GEOMETRY_CONTAINER_FILE`, `TEXTURE_ARCHIVE_FILE`, `REPRESENTATIVE_MESHES`,
  `PRESENTABLE_PROBE_MESHES`.
- `crates/cs_app/src/world/gpu_capture.rs` (new): the `gpu` half —
  `capture_world_mesh`, `CaptureRequest`, `GpuCapture`, `GpuCaptureError`, and
  the declared framing constants `CAPTURE_WIDTH`/`CAPTURE_HEIGHT`,
  `FRAMING_DISTANCE_FACTOR`, `CAPTURE_VIEW_DIRECTION`, `NEAR_PLANE_FRACTION`,
  `FAR_PLANE_FACTOR`.
- `crates/cs_app/src/world/mod.rs` (wiring and module docs only).
- `crates/cs_app/tests/world/audit.rs` (new): the eight `accept_f18_d_*`
  tests. `crates/cs_app/tests/world/audit/evidence.rs` (new): the evidence
  harness, deliberately **not** named with the task prefix.
- This file and `docs/findings/evidence/F18-D.json`.

**One observable failure:** a world group is visited, its stored geometry is
counted, and the report says nothing about the fact that **23 of the 24 largest
stored meshes across the eight world groups cannot be presented at all** —
`accept_f18_d_retail_every_world_group_draws_a_measured_frame_on_the_gpu` fails
with `c1 mesh 236 material group 0 would not upload: material group 0 carries a
normal on 65 of 111 vertices; the buffer cannot be filled from stored values
alone` (measured, not predicted), and the census that reports triangles and
vertices without that verdict reads as "presentable" for a mesh the world load
would refuse.

## The two production derivations of "every discovered world group"

`cs_assets::install::Diagnosis::world_groups` gives the **discovered** set —
every directory under the installation's `zbd` root, original spellings
preserved, sorted by logical key — and
`cs_content::campaign_bindings::campaign_layout` gives the **mission** set. The
survey uses both: a discovered group with no campaign mission is still visited
(it has real geometry) and reports `has_missions() == false`, and a reference
lead of spec F02 the installation lacks is reported as absent rather than
audited as an empty world.

The two derivations spell a group differently — the campaign walk reports `c1c`,
discovery reports `zbd/c1c` — and the join is on the last component of the
logical key. That is written down in `mission_labels_by_group` because getting it
wrong silently drops every mission, and it did exactly that on the first run of
this stage's own test (the test failed on "every discovered group carries at
least one campaign mission here", which is how the join key was found).

Measured over the real installation: **8 world groups** (`c1`, `c1b`, `c1c`,
`c2`, `c2b`, `c3`, `c4`, `c5`), **24 campaign missions**, **no absent
reference lead**, and every group carries at least one mission. The group
membership (`M02, M04, M05` in `c1`; `M03` in `c1b`; `M01` in `c1c`;
`M01–M03, M05` in `c2`; `M04` in `c2b`; `M01–M05` in `c3` and `c4`;
`M01–M04` in `c5`) is the same derivation `cs-inspect campaign` already
reports, so the two cannot disagree.

## What the retail run measured

Every group's own `gamez.zbd`, read through **both** production GameZ readers
over one shared parse context, with the mesh walk required to end exactly on the
`nodes_offset` the header declares:

| world | mesh slots | present | stored faces | drawn triangles | missing faces | stored texture names | multi-group polygons | representative triangles | upload refusals |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `c1` | 2 250 | 2 237 | 18 277 | 48 054 | 0 | 565 | 390 | 700 / 420 / 628 | 3 of 3 |
| `c1b` | 1 500 | 1 305 | 8 323 | 19 763 | 0 | 359 | 8 | 153 / 262 / 262 | 2 of 3 |
| `c1c` | 2 250 | 1 518 | 8 040 | 19 492 | 0 | 279 | 10 | 262 / 262 / 240 | 3 of 3 |
| `c2` | 2 250 | 1 765 | 12 645 | 33 802 | 0 | 498 | 206 | 360 / 262 / 262 | 3 of 3 |
| `c2b` | 1 500 | 1 365 | 7 008 | 16 744 | 0 | 285 | 8 | 262 / 262 / 262 | 3 of 3 |
| `c3` | 2 250 | 1 901 | 16 087 | 39 151 | 0 | 465 | 16 | 310 / 310 / 310 | 3 of 3 |
| `c4` | 3 000 | 2 431 | 19 661 | 56 809 | 3 | 654 | 16 | 455 / 527 / 360 | 3 of 3 |
| `c5` | 3 000 | 2 851 | 22 493 | 58 421 | 8 | 582 | 352 | 349 / 361 / 240 | 3 of 3 |
| **total** | | **15 373** | **112 534** | **292 236** | **11** | **3 687** | **1 006** | | **23 of 24** |

Three of these numbers are independent cross-checks of earlier stages, which is
why they are worth stating: the `present` column equals F11-D's per-archive
"present meshes" census (`c1` 2 237, `c1b` 1 305, `c1c` 1 518, `c2` 1 765,
`c2b` 1 365, `c3` 1 901, `c4` 2 431, `c5` 2 851), the `multi-group polygons`
column equals F10-E's measured **1 006** stored polygons with more than one
material group across the eight world archives, and the stored node-record
totals (7 064 + 5 603 + 5 644 + 4 956 + 4 901 + 5 408 + 8 289 + 11 438 =
**53 303**) plus each group's `nodes_offset` match F11-D's per-archive table
exactly. Three derivations from three stages agreeing on the same corpus is
evidence; one of them agreeing with itself is not.

### The finding: the retail world corpus is largely **not presentable**

**23 of the 24 representative meshes — the three largest stored meshes of each
of the eight world groups — are refused by the F17-B upload adapter**, every one
of them for the same reason: a stored normal on *some* of a material group's
vertices and not others. Measured, not inferred; the distinct refusals were

| refusal | count |
| --- | --- |
| `material group 1 carries a normal on 24 of 492 vertices; the buffer cannot be filled from stored values alone` | 8 |
| `material group 1 carries a normal on 25 of 32 vertices; …` | 3 |
| `material group 0 carries a normal on 65 of 111 vertices; …` | 1 |
| `material group 0 carries a normal on 76 of 112 vertices; …` | 2 |
| the other eleven refusals, between `8 of 180` and `132 of 172` vertices | 9 |

**Which material group was refused, re-measured after the review fix.** Over the
23 refusals the refused group is **0 nine times, 1 twelve times and 9 twice** —
so the group matters and it is not usually the first one. The submitted branch
recorded `0` for all 23 (see the review section: the group was scraped out of
the adapter's message with a two-word prefix that never matched), which means
the census as submitted was wrong for the twelve group-1 refusals and the two
group-9 refusals. The refusal *count* and the verbatim messages were correct
throughout; only the structured field was wrong. `world-group-census.json` now
carries the adapter's own group per refusal.

The adapter is **right** to refuse: a buffer filled with an invented normal for
the corners that store none is exactly the "developer placeholder" the sheet
forbids, and F17-B's contract is to refuse rather than default. So this is not a
defect to repair inside this stage; it is a content gap in the presentation
policy, and it is the reason F18-D's own `gpu` evidence had to be re-shaped.

What the stage did about it is state it in a field of its own rather than around
it:

* `RepresentativeGeometry::upload` carries the adapter's own verdict — the
  census no longer reads as "presentable" for a mesh the world load would refuse.
* `WorldGroupCensus::refused_representatives()` counts them **from the
  representatives' own verdicts**, and `WorldGroupCensus::refused()` iterates
  them. (This branch originally also carried a `GroupFacts` field of the same
  name; it was removed in review because `WorldGroupCensus::new` never read it,
  so two fields held the same number and one was dead.)
* The capture draws a **second**, separately declared selection:
  `SurveyedContainer::presentable`, the largest stored mesh inside
  `PRESENTABLE_PROBE_MESHES = 64` candidates (by stored `polygon_count`) that
  the adapter accepted. Measured on this installation, **63 probed stored meshes
  were refused before the first accepted one** — so the largest presentable mesh
  is not the largest stored mesh anywhere, and a rule of "draw the biggest mesh"
  would have produced eight refusals and no evidence.

A third finding follows from the same data: **retail world geometry sits
thousands of stored units from the origin** (`c1b` mesh 277 spans
`-8157.86 .. -7230.46` in `x` and `-6143.36 .. -5126.45` in `z`), while Bevy's
default perspective projection has a far plane of `1000`. Any capture that left
the default clipped the entire corpus away. The capture's clip planes are
therefore derived from the framing distance (`near = 0.01 d`, `far = 4 d`), and
that derivation is a named constant pair rather than a private choice.

## The GPU capture, and what the frames are

`cs_app::world::gpu_capture::capture_world_mesh` takes the **production render
mesh** (`RenderMesh::from_stored_groups`, every stored material group kept),
runs it through the **production upload adapter**, spawns one draw per material
group into a real Bevy app with the real render stack (`WinitPlugin` disabled —
an offscreen image target needs an adapter, not a display), and reads the frame
back through Bevy's own screenshot path.

Declared, and visible in the module docs, because each was chosen for a reason a
reader can check:

* **No back-face culling.** Which stored winding the original treated as
  front-facing is F17's open `FrontFaceWinding` question. Culling on it would
  make the artifact depend on a question this stage does not answer, and
  `c1b`'s largest presentable mesh is a **flat sheet** (927 × 1.3e-13 × 1017
  stored units, lying in a plane) whose faces all point the same way.
* **A three-quarter view direction** `[0.5, 0.6, 0.62]`. A camera on a single
  axis sees a flat sheet edge-on, where its projected area is zero: the first
  version looked along `+z` and `c1b` came back as 0 of 76 800 pixels off the
  background. This was measured, not guessed.
* **A bounding-sphere framing** at `2.75 ×` the AABB's half-diagonal, which is
  the distance at which a 45° vertical field of view fits a sphere with 14 %
  margin.
* **An explicit clear colour**, so `covered_pixels` measures the geometry rather
  than a difference from the renderer's own default clear colour (with the
  default, every background pixel counts as covered and the measurement is
  1000 per mille of noise).
* **Clip planes derived from the framing distance** (`near = 0.01 d`,
  `far = 4 d`), because Bevy 0.19.1's default far plane is `1000.0` and this
  corpus sits at `~8000` stored units. Reverting them to the defaults was
  measured and did **not** make any capture fail on this pinned pair — see the
  sensitivity table.

The refusals are the point of the module: `NoAdapter`, `EmptyMesh`,
`DegenerateBounds`, `NoScreenshotCaptured`, `UniformFrame`, `GroupRefused` and
`Io`. A refused capture leaves **no PNG behind** — pinned by
`accept_f18_d_a_capture_that_drew_nothing_is_refused_rather_than_written`, which
asserts both the refusal and the absence of the file, because a file on disk that
nobody could tell from a good capture is worse than no file.

Measured over the installation: **8 frames, one per world group, every one
non-uniform**, on the adapter the driver named (`Apple M3 Pro (Metal)`). The
artifacts are `private/evidence/F18-D/render-{c1,c1b,c1c,c2,c2b,c3,c4,c5}.png`,
hashed by `docs/findings/evidence/F18-D.json`. One of them, `ZBD/C2`'s largest
presentable mesh, is a curved shell — the geometry is recognisably a *world prop*,
not a heightfield substitute, which is the sheet's deliverable stated as an
observation.

**Three of the eight frames are byte-identical, and that is the corpus, not a
reused capture.** `ZBD/C1C`, `ZBD/C2B` and `ZBD/C3` all select array index 33 as
their largest presentable mesh, and its vertex bytes hash identically in all
three (`23f98175e782…`): the world archives share meshes, so the same stored
geometry is drawn three times and the PNGs come out equal. A reader comparing
the artifact digests in `docs/findings/evidence/F18-D.json` will see
`render-c1c.png`, `render-c2b.png` and `render-c3.png` carry one SHA-256, and
should read that as shared content rather than as a capture that reused a file.
The retail GPU test now records a geometry digest per frame and reports the
distinct count (6 of 8), so the fact is measured rather than left for a reader
to infer. Whether the original shares these meshes across chapter archives, or
loads one copy per chapter from a shared source, is **not established here** and
is not claimed.

**What the frames are not.** They are not a placement (no mesh has a world
position), not a metric (the stored vertex unit is unmeasured, so the camera's
distances are in stored units), and not the original's appearance (a flat
declared colour, a declared key light, and F17's presentation unknowns open).
They are evidence that each group's stored geometry is **drawable as stored**,
through the real pipeline, on a real GPU.

## The traversal half of AC04, and why it is blocked

**No world group can state a traversal route or locate a stunt opening, and the
audit says so per group with the container's own numbers.** Two independent
facts are missing and both are named:

1. **Placement.** No production path decodes a GameZ node array (#392), so no
   stored mesh has a position, orientation or scale in world space. Measured per
   group: `PlacementSource::Undecoded` carrying `node_array_size` 7 064 / 5 603 /
   5 644 / 4 956 / 4 901 / 5 408 / 8 289 / 11 438 and the matching `nodes_offset`.
   `TraversalBlocker::PlacementUndecoded`'s `Display` quotes them.
2. **The stored vertex unit.** `cs_content::mesh` applies no scale to stored
   positions and nothing in this workspace has established the original's
   world-vertex unit, so `vertex_scale_to_m` is `None` and
   `TraversalBlocker::VertexScaleUnmeasured` quotes the largest stored extent
   the census did measure (40.6 in `c2b` to 1 652.1 in `c5`).

Without both, a route has no referent: an opening's position and its clearance
are both unavailable, so "traversal route" is not a smaller claim than "placement",
it is the same claim with the units left out. The audit therefore reports zero
routes and refuses to derive one.

The five opening classes the sheet's deliverable names — `tunnel`, `arch`,
`building_opening`, `hangar`, `stunt_passage` — are a **declared list to look
for**, and the audit visits **every one of them for every group**, reporting
`Unlocated` with the blocker that stopped it. This is deliberate: the classes are
not inferred from geometry (a tunnel is a property of *placed* geometry between
two spaces), and a report that could answer "was a hangar looked for?" in every
group is worth more than one that silently omitted the class.

`WorldGroupAuditReport::is_complete()` is therefore `false` over the real
installation, and that is the verdict, not a failure of the audit. The audit
*can* report itself complete — the synthetic test proves it with a decoded
placement, a measured scale, a route and all five classes located — so the
retail `false` is a statement about the data, not about the instrument. The
evidence harness asserts `!report.is_complete()` for the same reason: a complete
report there would mean the blockers had been dropped rather than named.

## Why the survey does not open a content session

`MeshContainer::open` is the production seam for a GameZ container and this
survey's counts are the counts it produces. It is not used, for one **measured**
reason recorded here rather than left to be rediscovered: a world group is only
addressable through a content session whose `ResolveContext` names that group, so
eight groups means eight `SessionBuilder::mount_installation` calls, and on this
installation **each one measured 33 s** (it walks and mounts the whole 470 MB
tree). Eight groups came to **460 s** of mounting to re-derive in 0.3 s per
group what the production GameZ readers produce from the same bytes. The survey
reads the inventoried file's bytes and hands them to `read_gamez_meshes` +
`read_gamez_materials`, which is the same reader pair F11-D's retail census uses
and the same pair `MeshContainer::open` itself calls; the retail AC04 test now
runs in **29 s**.

What that choice gives up is stated in the module docs and in the census: the
material-index-to-texture **binding** is not audited here, so
`bound_texture_names` is `0` for every group rather than implying a reconciled
corpus. That is F10-C.02's and F09's subject, measured through
`cs_content::textures` on a session; a world-group geometry census has no claim
about it. What the survey does read is the group's own stored texture **names**
and its stored material-group table, both from the container.

## Test inventory

| `accept_f18_d_*` test | Covers | Fails when |
| --- | --- | --- |
| `the_audit_visits_every_group_and_compares_its_representative_geometry` (unignored) | **AC04's mapping arm**: three declared groups, the survey seam asked once per group and in declared order, 2 visited / 1 blocked, the group with established facts routed with its own route and all five classes located, the group with undecoded placement carrying both blockers with the container's own numbers quoted in its `Display`, the empty group blocked with its slot count, the representative budget and the per-group triangle totals | a group is skipped, a census is filed under the wrong row, a blocker loses its measured numbers, an unlocated class is reported as located, the routed group is not the one whose facts are established, or the empty group reads as a census of zeroes |
| `an_unlocated_opening_or_route_is_reported_instead_of_assumed` (unignored) | **AC04's negative arm**: a census claiming a route and an opening while both facts are missing (`RouteWithoutFacts`), a clean group that measured no route (`NoRouteMeasured`), a census about a group the row does not declare (`CensusGroupMismatch`), and an audit of nothing that must not read as a pass | a contradiction is reported as a result, a shortfall is rounded up to a pass, a mismatched census is counted under the wrong row, or an empty report is complete |
| `world_group_records_refuse_contradictions_and_impossible_values` (unignored) | construction refusals (duplicate group, blank mission label, duplicate mission), a group with **no** campaign mission being valid, a non-finite unit scale and a non-finite stored corner each refused with the offending value quoted by bits, and the measured arm being reachable — a complete single-group report with zero blockers and zero gaps | a row contradiction is accepted, an empty mission list is treated as an error, an impossible value is stored, or completeness is unreachable |
| `a_gpu_capture_proves_the_stored_geometry_was_drawn` (`#[ignore]`, needs a GPU) | the `gpu` half of the mapping arm: a real offscreen render of a real `RenderMesh` built through `RenderMesh::build`, more than one luminance level, non-zero coverage, coverage above zero and below a thousand per mille, the adapter named, one material group and eight submitted triangles for the authored arch, and the PNG's digest equal to the capture's | the frame is blank, the capture hides the adapter, the counts stop matching the upload, or the digest is of a buffer rather than the file |
| `a_capture_that_drew_nothing_is_refused_rather_than_written` (`#[ignore]`, needs a GPU) | the refusal half: a mesh with no polygon is refused by name with its group and index, **and no PNG is left behind** | an empty mesh is captured into a file, or a refusal still writes an artifact |
| `retail_every_discovered_world_group_is_visited_and_compared` (`#[ignore = "requires CS_GAME_DIR"]`) | **AC04 over the real installation**: eight groups in discovered order with no repeats and no absent reference lead, every group carrying a campaign mission, eight visited and none blocked, 15 373 present meshes and 292 236 drawn triangles in total, per group the measured bounds and the representative budget and fingerprint-free ordering, `Undecoded` placement with a non-zero node-record count, `vertex_scale_to_m == None`, zero routes, both traversal blockers in order with the scale one quoting a measured extent, all five opening classes unlocated and self-naming, no gaps, and more than 10 000 stored node records over the eight groups | a group is missed or duplicated, a census of zeroes passes for a group, a route appears, a blocker is dropped, a class is reported located, or the traversal verdict silently changes |
| `retail_every_world_group_draws_a_measured_frame_on_the_gpu` (`#[ignore]`, needs `CS_GAME_DIR` and a GPU) | the `gpu` half over the real installation: one measured non-uniform frame per discovered world group from the group's largest **uploadable** mesh, the adapter named, a geometry digest recorded per frame and the distinct count reported, and the count of refused candidates non-zero so a run that asked the adapter nothing cannot pass | a group's frame is blank, the adapter is unnamed, or the upload verdicts were skipped |
| `a_refused_upload_names_the_material_group_the_adapter_refused` (unignored; **added in review**) | the refusal **attribution**, through the production `cs_app::world::upload_verdict` the census itself calls: an authored stored mesh with two material groups whose second stores a normal on one of three corners, so the adapter refuses **group 1**; the verdict must name group 1, the carried verbatim message must name the same group, and the census must carry that verdict unchanged | the group is scraped out of the adapter's message instead of read from its typed error — the submitted branch's form, which reported group 0 for all 23 retail refusals. Restoring it fails with `left: 0, right: 1` |

The evidence harness (`evidence_report_f18_d_writes_the_acceptance_report`,
deliberately **not** named with the task prefix) derives
`private/evidence/F18-D/acceptance.json` and `world-group-census.json` from the
recorded acceptance log, production discovery of `$CS_GAME_DIR` and its own
`audit_world_groups` run; it refuses a stale `CS_CANDIDATE_TREE`, a missing log,
a missing required test and a census that claims completeness, and it writes a
failing report when the acceptance run failed.

## Sensitivity check (mutations applied and reverted while implementing)

Every row below was applied to the source, the whole `accept_f18_d_` selection
was run with `--include-ignored`, and the source was restored. All eight tests
pass unmutated. **Two rows did not fail and are reported as such** rather than
claimed as coverage.

| Mutation | Failing `accept_f18_d_*` tests |
| --- | --- |
| the census stops refusing a non-finite stored corner | 1: `world_group_records_refuse_contradictions_and_impossible_values` |
| the audit skips `RouteWithoutFacts` when only the *routes* are claimed and no opening | 1: `an_unlocated_opening_or_route_is_reported_instead_of_assumed`. **This row initially did not fail** — the negative arm's first case claimed a route *and* an opening, so a check written as "both are non-empty" passed. The case was split (a route with no opening is just as unsupported) and the row then fired |
| `is_complete` drops its "every group visited" clause | **0 — this row did not fail, and the clause is redundant.** A group that is not visited cannot be routed, so the `routed().count() == groups.len()` clause already fails first. The clause is defensive, not load-bearing, and a test cannot distinguish it from the other. Recorded rather than presented as coverage |
| the audit drops `VertexScaleUnmeasured` when the placement is also undecoded | 3: the mapping arm, the negative arm and `retail_every_discovered_world_group_...` |
| the opening audit reports no `unlocated` class when nothing located it | 1: `retail_every_discovered_world_group_...` (the retail case); the synthetic mapping arm is unaffected because it locates all five |
| the capture clears with the renderer's default clear colour again | 1: `a_gpu_capture_proves_the_stored_geometry_was_drawn`, on `covered_permille == 1000` — every background pixel then differs from the value `measure` compares against |
| the capture's view direction returns to `+z` | 1: `retail_every_world_group_draws_...`, with `c1b` at **0 of 76 800** pixels off the background — the measured flat-sheet case |
| the capture's clip planes return to `PerspectiveProjection::default()` | **0 — this row did not fail.** Bevy 0.19.1's default far plane is `1000.0` and `c1b`'s geometry sits at `~8000`, so the derivation looks necessary, but reverting it made no capture fail on this pinned pair. The uniform frames this stage found were the **view direction**, not the clip planes. The derived planes are kept as the defensible choice and the constant's doc says this, because the reason the default did not bite is not established |
| the `UniformFrame` refusal is dropped | 1: `a_capture_that_drew_nothing_...`. **This row initially did not fail**, because `UniformFrame` had no reachable case: with the view direction fixed no group produced a blank frame, and the empty-mesh test exercises a different error. The variant was documented but untested production code. A fixture whose stored triangle is degenerate (`0, 0, 1`) now reaches it |
| `discard_capture` is made a no-op | 1: `a_capture_that_drew_nothing_...`, on "a refused capture must leave no file". Found by that same test: the renderer writes the PNG the moment the frame arrives, *before* the code has looked at it, so a uniform frame was leaving a file that read exactly like a good capture |

### Reviewer rows (bunny-alpha-2)

| Mutation | Failing `accept_f18_d_*` tests |
| --- | --- |
| `upload_verdict` reads the refused group out of the adapter's `Display` with the original two-word `strip_prefix` (the submitted form) | 1: `a_refused_upload_names_the_material_group_the_adapter_refused`, on `left: 0, right: 1`. **This row is the review's main finding** — the mutation restores the exact code the branch shipped, and the retail run then reports group 0 for all 23 refusals where 12 are group 1 and 2 are group 9 |
| `read_file` is given the lowercased logical key again | 0 on this machine — the retail installation sits on a case-insensitive filesystem, so `zbd/c1c/gamez.zbd` and `ZBD/C1C/gamez.zbd` are the same file. **Recorded as not discriminating here**, and fixed anyway: the defect is real on a case-sensitive host, which is where a reviewer or the owner may well run this, and no test on this machine can prove it |
| `census_verdict`'s `UnknownOpeningClass` check is restored | 0 — and it cannot be made to fail, which is why the variant was removed rather than tested. The openings it inspects are already filtered by a class from `OpeningClass::ALL`, and `StuntOpening::class` is a closed enum, so the branch is unreachable by construction |
| the retail test's `refused_representatives()` assertion is restored to `== census.refused().count()` | 0 — that is the point: the submitted form compares a method with its own definition, so it could not fail. It is replaced with a count taken from the representatives' own verdicts plus a typed-group-against-message check |

The two rows that initially did not fail are the reason this table is worth
having. In both cases the code was correct-looking and **unreachable** — a
documented refusal with nothing that could reach it, and a check that only fired
when two conditions held together. Neither was visible from reading the code.
The reviewer's rows make the same point from the other side: of the four defects
found here, two (the message-scraping and the tautological assertion) are
invisible to the eye precisely because the code reads correctly, and a third
(the case-folded read path) cannot be made to fail on this machine at all.

## Designed vocabulary, not original data

Designed here: `OpeningClass` and its five codes, `WorldGroupRef`,
`PlacementSource`, `TraversalRoute`, `StuntOpening`, `WorldAuditError`'s
refusals, `REPRESENTATIVE_MESHES`, `PRESENTABLE_PROBE_MESHES`, the capture's
frame size, framing factor, view direction and clip-plane fractions, the capture's
clear colour and mesh colour, the key light's illuminance, the four-frame warm-up
and the 24-update bound, and the *selection rules* (largest stored
`polygon_count`, ties by lower array index). The synthetic arch and every census
fixture are authored development content (`Origin::SyntheticFixture`).

Measured here, on the real installation with the real GPU: every number in the
census table, the 23 upload refusals and their reasons, the 63 refused
candidates before the first accepted one, the stored coordinate magnitude, and
the eight non-uniform frames.

Unknown, and **not** guessed:

- **How the original stores world placement** and how it identifies a sector or
  an object instance. The container header declares 53 303 stored node records
  across the eight groups and none of them is decoded (resolving task **#392**).
- **The original's world-vertex unit.** Every stored extent in a census is in
  stored units and no factor to metres exists.
- **Which gameplay surface classes the original distinguishes** and what rule
  each carries. `SurfaceRole` is still the two the spec names.
- **The original's floor, ceiling and world-boundary rules.** `WorldBoundary`
  still documents "no rule" rather than a wall.
- **Which stored mesh is a tunnel, an arch, a hangar or a stunt passage.**
  `OpeningClass` is a list to look for; nothing here classifies geometry.
- **Whether a normal stored on some corners and not others means the mesh is
  unlit, means the other corners are flat, or means the renderer computed them.**
  The upload adapter refuses, which is right; what the original *did* is unknown.
- **Which stored winding is front-facing** (F17's `FrontFaceWinding`). The
  capture presents both sides so that it depends on the stored triangles only.

## Known limitations that gate later stages (not silently dropped)

1. **No traversal route and no located opening in any world group.** The world
   placement is undecoded and the stored vertex unit is unmeasured, so the
   traversal half of AC04 is a blocker per group with the container's own
   numbers. **Affected content:** every traversal route, every stunt-critical
   opening, every sector boundary and every world-collision role in all eight
   world groups — i.e. F18's deliverable "preserve tunnels, arches, building
   openings, hangars and stunt passages" is **not** met for original data, and
   the heightfield-substitute prohibition cannot be checked either. **Resolving
   task:** **#392** (read the GameZ node array into `ParsedNode` records; needs
   the owner to grant `crates/cs_formats/` owner paths) and the follow-up filed
   with this stage, **#436** (F18-E, "Measure the world placement and the stored
   vertex unit, then audit traversal routes"), which owns the measurement of the
   unit and the re-run of the audit against a decoded placement. This limitation **survives this task being
   marked done** and gates every F18 world-geometry fidelity claim.
2. **The retail world corpus is largely not presentable: 23 of 24 representative
   meshes are refused by the F17-B upload adapter for partial stored normals.**
   **Affected content:** the largest stored meshes of all eight world groups,
   i.e. most of what a player would see in a chapter's best-known locations. A
   mission built on these archives today could not load its own world geometry
   for those meshes. **Resolving task:** the follow-up filed with this stage
   (**#435**, F17-G, "Decide and implement the presentation of a partly stored
   attribute") — it needs an *evidence* decision about what the original did with
   a corner that stores no normal, which is not a question a pipeline can
   answer by picking a default. Until it is decided, no claim that the retail
   world's geometry is loadable may be made.
3. **The material-index-to-texture binding is not audited by this stage**, so
   every group's `bound_texture_names` is `0`. That is a *reported* zero, not a
   reconciled corpus, and it is deliberate (the session cost is recorded above).
   **Affected content:** every world material's texture dependency. **Resolving
   task:** F10-C.02's and F09's own stages; nothing here needs them.
4. **The GPU captures are geometry witnesses, not appearances.** A flat declared
   colour, a declared key light, no back-face culling, both clip planes derived
   from the framing distance, and F17's presentation unknowns open.
   **Affected content:** every visual-fidelity claim about world geometry.
   **Resolving tasks:** F17's `FrontFaceWinding`/`UvOrigin` stages, and the
   owner-supplied `REF-OWNER-FIRST-CAPTURE` for the original's actual look.
5. **The census reads inventoried bytes rather than a mounted session**, so a
   group's geometry is read under its installation-relative spelling rather than
   through the `world` mount's `MissionWorld` precedence. The bytes are the same
   and the digest comes from production discovery, but the *resolution* of the
   same name is F04-D's question and this stage makes no claim about it.
   **Affected content:** nothing in this stage's counts; a consumer that needs
   the mount's precedence must ask a session. **Resolving task:** F04-D's own
   measurements.
6. **No original run and no human judgement.** `retail` means the installation's
   files were read, not that `crimson.exe` was run; `gpu` means frames came back
   from a real adapter, not that the original's rendering was compared. Nothing
   here is `verified_original` or `release_approved`; this stage can award at
   most **checked**.

## Review (bunny-alpha-2, fresh context, 2026-09-30)

The review found **four defects** in the submitted branch. All four are fixed
here; the first is the one that mattered.

1. **Every refused upload was attributed to material group 0, whatever the
   adapter said.** `census_of` read the refused group out of the adapter's
   `Display` by `split_whitespace().find_map(|w| w.strip_prefix("material
   group "))`. `"material group 1 carries a normal on 24 of 492 vertices; …"`
   splits into `"material"`, `"group"`, `"1"`, so the two-word prefix never
   matched, the `find_map` returned `None`, and the `unwrap_or(0)` fallback
   made **every** refusal report group 0. The refusal table above is right
   about the *messages* (they are carried verbatim) and the head count is
   right, but the census's structured `material_group` field was wrong for
   every refusal that was not on group 0 — which, per that same table, is most
   of them. Both `MeshAdapterError` variants carry the group in their payload,
   so the message never had to be parsed. Fixed by reading the typed error in
   the new [`upload_verdict`], which is now the single place a verdict is taken
   (the census and the presentable search both call it, so they cannot drift).
   Regression test:
   `accept_f18_d_a_refused_upload_names_the_material_group_the_adapter_refused`,
   which fails with `left: 0, right: 1` when the parsing form is restored.
2. **The container bytes were read through a lowercased path.** `read_file`
   joined the *logical key* (`zbd/c1c/gamez.zbd`) onto the host root, but the
   installation stores `ZBD/C1C/gamez.zbd`. That works on a case-insensitive
   filesystem and fails on a case-sensitive one — the exact difference the
   manifest's preserved original spelling exists to survive, and the row
   carrying it was right there. Now reads `record.relative_spelling`.
3. **A gap check that could not fire.** `census_verdict` pushed
   `WorldAuditGap::UnknownOpeningClass` from inside a loop over openings that
   had already been filtered by `opening.class == *class` for `class ∈
   OpeningClass::ALL`, and `StuntOpening::class` is a closed enum. The branch
   was unreachable and no test could reach it. This is the same class of
   defect the implementer's own sensitivity table found twice
   (`UniformFrame`, `DegenerateBounds`); the variant and the check are removed
   rather than left as readable dead code.
4. **A tautological assertion.** The retail test asserted
   `census.refused_representatives() == census.refused().count()`, and the
   first is *defined* as the second, so it could not fail. It now counts the
   verdicts on the representatives themselves and additionally checks that each
   refusal's typed group agrees with the message the adapter produced and lies
   inside the mesh's own group count.

`GroupFacts::refused_representatives` was also removed: it was written by every
caller and **read by nobody** — `WorldGroupCensus::new` never copied it into the
census, and the census derives the count from the representatives instead. Two
fields claiming to hold the same number, one of them dead, is how a reader ends
up comparing two values that quietly disagree.

Not changed, and why: the `is_complete()` "every group visited" clause is
redundant with the routed clause, as the implementer's table already recorded.
It is harmless, the redundancy is documented, and removing it would delete a
defensive check for no behavioural gain. The clip-plane derivation is likewise
kept as the defensible choice with its "not established" note intact.

## Sources

- `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
  (the deliverable, non-negotiable behaviors 1–5, AC01–AC04, `### F18-D`),
  `docs/contracts/IDENTITY-CONTENT.md` (stable ids, provenance, explicit
  unknowns, one pose owner), `docs/contracts/CLI-EVIDENCE.md` and
  `schemas/evidence.schema.json` (the evidence record and its validator).
- `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`,
  `.../2026-09-30-f18-b-world-import-and-static-collision.md` and
  `.../2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md` — the
  records this stage audits and the unknowns they left, including "the original's
  world-vertex unit and coordinate handedness are unmeasured (F18-B/D)" and
  "no retail world group was visited: AC04 needs `gpu` + `retail` (F18-D)".
- `docs/findings/2026-09-30-f11-d-private-roster-audit.md` — the audit
  precedent this stage's instrument follows (declared rows plus a measured seam,
  typed blockers that quote numbers, `is_complete()` that an empty report cannot
  satisfy, and the `#[ignore = "requires CS_GAME_DIR"]` convention), and the
  node-array blocker whose numbers this run re-measured.
- `docs/findings/2026-09-30-f23-d-stability-high-speed-contact-and-convergence-evidence.md`
  — the refusal-and-measurement style of the GPU probes, and the record of what
  a stage's own numbers are allowed to claim.
- `cs_formats::gamez::{read_gamez_meshes, read_gamez_materials, FaceCensus}`
  (the two production readers and the face accounting this census is built on),
  `cs_content::mesh::RenderMesh::from_stored_groups` (the production render
  mesh), `cs_app::render::bevy_mesh::upload_groups` (the production upload
  adapter whose verdict the census carries), and Bevy 0.19.1's
  `bevy_render::view::screenshot` (the readback path). Adapter behaviour
  (`SweptCcdBodyQuery`, cull modes, clip planes) was read from the pinned sources
  in the local cargo registry and then **measured** by running the capture.
