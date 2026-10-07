# F17-D: the original-data comparison matrix and its material coverage

Date: 2026-10-07. Task: F17-D "Review original-data screenshot matrix and
material coverage"
(`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`, section
`### F17-D`), acceptance scenario **AC04** — *"Original comparison set includes
cockpit, skyline, vegetation, night effects and close-up aircraft"*. Shared
contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`**
(read access to the owner's installation at `$CS_GAME_DIR`) and **`gpu`** (five
frames drawn on the real adapter, Apple M3 Pro / Metal). No `audio`,
`human_play` or `human_review` was used or available: this stage plays nothing
and no human looked at the frames, so nothing here is `verified_original`.

## Files and the one observable failure

- `crates/cs_app/src/render/matrix.rs` (new; ~1 250 lines): the stage's
  production surface — `ComparisonSubject` (+ `ALL`, `code`, `index`, `side`),
  `SubjectSide`, `SubjectNode`, `CandidateNode`, `SubjectSelection`,
  `MatrixError`, `CandidateSkip`, `ResolvedSubject`, `MaterialCoverage`,
  `MatrixRow`, `ComparisonMatrix::build`, `MatrixContainer`, `select`, `load`,
  `resolve_all`, and the pins `MATRIX_WORLD_GROUP`, `HORIZON_ANCHOR`,
  `AIRFRAME_ANCHOR`, `INTACT_ANCHOR`, `COCKPIT_ANCHOR`, `NIGHT_ANCHORS`.
- `crates/cs_app/src/render/mod.rs` (wiring only): `pub mod matrix;` plus one
  doc paragraph. No `Cargo.toml` change: the module uses the crate's existing
  dependencies.
- `crates/cs_app/tests/render/matrix.rs` (new): the nine `accept_f17_d_*`
  tests — six fast (CI) and three `#[ignore]`d (retail resolution, retail
  coverage, GPU capture).
- `crates/cs_app/tests/render/evidence.rs` (new): the evidence-report harness,
  deliberately **not** named with the task prefix.
- `crates/cs_app/tests/render/main.rs` (wiring only): `mod matrix;`,
  `mod evidence;` and a doc paragraph.
- This file and `docs/findings/evidence/F17-D.json`.

**One observable failure.** Before this stage there was no production path
from the installation to AC04's comparison set: nothing named the five
subjects, nothing resolved them against the original bytes and no coverage of
their materials existed, so "the set includes cockpit, skyline, vegetation,
night effects and close-up aircraft" had no implementation to test. The
narrowest observable failure is the set's own validation — with
`ComparisonMatrix::build`'s subject check removed,
`accept_f17_d_a_set_missing_or_duplicating_a_required_subject_is_refused` fails
on both the missing row and the duplicate row; with a selection rule that
accepts whatever it finds,
`accept_f17_d_a_selection_rule_that_finds_nothing_is_refused_by_name` fails
because the refusals stop being named.

## What the stage builds

A subject is resolved in two steps and neither is optional:

1. **`select`** finds the subject's **anchor** — a stored name measured on
   this installation — and collects the anchor's mesh-bound descendants. A
   missing anchor is `MatrixError::AnchorAbsent`, never a subject left out of
   the set.
2. **`load`** builds every candidate through
   `cs_content::mesh::RenderMesh::from_stored_groups`, ranks them by the one
   declared rule (below), counts the coverage of the stored material records
   the winner references, and records every candidate that could not be drawn
   in `ResolvedSubject::skipped` with the reason.

The set itself is `ComparisonMatrix::build`, which refuses a set that is
missing a required subject or carries one twice; `resolve_all` turns both
containers into five rows, keeping a refusal as a row
(`MatrixRow::Unresolved`) instead of dropping it. That is the structural half
of AC04: a gap is *reported*, not turned into an absence.

### The anchors, and what each one is

| subject | side | anchor (stored) | measured on this installation |
| --- | --- | --- | --- |
| `cockpit` | aircraft | `cockpit1` under the first parentless airframe root that carries one | `player_bhawk` (root slot 1) → `cockpit1` slot 243: 100 nodes, **67 mesh-bound**; 10 parentless roots carry a `cockpit1` child |
| `skyline` | world | the world record's child `horizon` | `ZBD/C1C` slot 789: 8 nodes, **5 mesh-bound** (`moon`, `g1155`, `stars`, `h_zone2scroll`, `g1164`) |
| `vegetation` | world | the world record's child family whose stored name repeats | one repeated name (**30 instances**), first instance slot 969: 38 nodes, **35 mesh-bound**, every one a two-triangle billboard |
| `night_effects` | world | the world's night-sky nodes, `moon` and `stars` | slots 791 and 793; `stars` (mesh 154) stores **no position and no polygon** |
| `close_up_aircraft` | aircraft | `healthy` under the root `bloodhawk` | `bloodhawk` (root slot 2363) → `healthy` slot 2296: 46 nodes, **29 mesh-bound** |

The world record is found by its `NodeKind::World`, not by a name; the
vegetation family is found by *being instanced* (exactly one child name
repeats), so the rule never names a generated `g…` identifier itself; zero
repeated names and two repeated names are both refusals.

### The ranking rule

**Most drawn triangles, ties to the lowest stored slot**, applied identically
to every subject. It is a drawn-geometry rule rather than an extent one, and
that difference was measured: the first version of this stage ranked by stored
bounding-box **volume** and it picked `g371` for `cockpit` — a needle-shaped
mesh (393.7 × 11.8 × 106.0 stored units, 80 triangles, two material groups)
that won by being long, while the mesh that draws the cockpit's most geometry
(`g366`, 231 triangles, six material groups) is an order of magnitude smaller
in volume. The
rule was changed to triangles and every subject re-measured; the retail and
GPU tests were re-run on the new rule (the branch's second commit).

## What the retail run measured

`private/evidence/F17-D/comparison-matrix.json` (derived counts and names
only, never a stored byte), from the production matrix resolved again over the
installation during the evidence run:

| subject | container | anchor | chosen mesh | triangles | extent (stored units) | materials | skipped |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `cockpit` | `zbd/planes.zbd` | `cockpit1` (243) | `g366` (slot 247, mesh 146) | 231 | 31.63 × 28.43 × 64.55 | 6 | — |
| `skyline` | `zbd/c1c/gamez.zbd` | `horizon` (789) | `h_zone2scroll` (794, mesh 155) | 52 | 17 487.9 × 1 646.4 × 17 487.9 | 1 | `stars`: no position, no polygon |
| `vegetation` | `zbd/c1c/gamez.zbd` | `g27816` (969) | `g27817` (972, mesh 157) | 2 | 188.05 × 188.05 × 0 | 1 | — |
| `night_effects` | `zbd/c1c/gamez.zbd` | `moon` (791) | `moon` (791, mesh 152) | 2 | 422.78 × 422.76 × 0 | 1 | `stars`: no position, no polygon |
| `close_up_aircraft` | `zbd/planes.zbd` | `healthy` (2296) | `g442` (2497, mesh 1365) | 251 | 1.72 × 1.54 × 5.04 | 7 | `g59`: no position, no polygon |

Three cross-checks fall out of this table:

* the **skyline** extent (17 488 stored units) is the 17 km horizon dome
  recorded by `docs/findings/2026-10-05-t648-playtest-retail-scene.md`, whose
  subtree count of five mesh-bound nodes matches exactly;
* the **vegetation** billboards are flat (one extent axis is exactly `0`),
  which is what an instanced two-triangle quad looks like, and all 30
  instances share the meshes of their family;
* **`stars` is empty in both rows that reach it** — the star field stores no
  geometry at all, so it is reported as a refusal with that reason rather than
  drawn as a blank frame. `skyline` still resolves because its sibling meshes
  are drawable; `night_effects` resolves on `moon`.

### The five captures

One PNG per subject under `private/evidence/F17-D/` (Git-ignored, referenced by
SHA-256 from the report): `subject-cockpit.png` 13 612 B,
`subject-skyline.png` 12 001 B, `subject-vegetation.png` 18 343 B,
`subject-night_effects.png` 17 932 B, `subject-close_up_aircraft.png`
12 911 B. Each came back with `drew_geometry()` true — distinct luminance
levels above one and pixels off the clear colour — and each digest was
re-read from the file on disk. The adapter reported Apple M3 Pro / Metal.

## The material-coverage finding

**16 stored material records are referenced by the five chosen meshes (6, 1,
1, 1, 7). Zero of them establishes a render class. All 16 are reported
`unclassified` with the single refusal reason `undeclared`.**

That is this stage's review result, and it is the honest one: a stored GameZ
record alone asserts no class — `MaterialFacts::for_raw_record` deliberately
leaves `declared` as `None` and coverage `Unknown` — so until an evidence
source states a class per material, `classify` refuses. The finding is pinned
by
`accept_f17_d_retail_the_comparison_s_material_coverage_is_reported_not_assumed`,
which fails if a class appears without evidence *and* fails if a material is
counted `Opaque` by default (spec F17 non-negotiable 1).

So the coverage question this stage was asked to review answers itself in two
halves:

1. **Coverage accounting works and is complete** for original content:
   every referenced record is counted exactly once, refusals are grouped by
   their stable code, and `is_complete()` holds for all five rows.
2. **Class coverage is 0 of 16.** No opaque, masked, blended, additive or
   emissive class is established for any of them, which means no fidelity
   claim about original materials can be made yet, and F17's `Emissive` /
   `Additive` classes remain exercised only by synthetic fixtures.

The follow-ups this creates are filed below (render-class evidence table; the
F17-C additive-phase question).

## What a row of this matrix is not

* **Not a comparison against the original.** There has been no original run.
  The other side of the comparison needs `#358 REF-OWNER-FIRST-CAPTURE`.
  `retail` here means read access to the owner's files only.
* **Not a claim about textures, lighting or colours.** The capture is
  `cs_app::world::gpu_capture::capture_world_mesh`, which uses its own
  declared flat material and key light (F18-D's geometry-witness pattern). A
  row is evidence that *this original mesh* is presentable through the
  production adapter. The textured path exists separately
  (`playtest_textures`, task #666) and is not wired into the matrix.
* **Not a material-class claim.** The unknowns handed to the adapter are
  `stored_presentation_unknowns`'s — front-face winding, UV origin, vertex
  colour, plus multi-material-group when the mesh stores one — so the report
  carries the same open questions F17-B recorded, not a quieter set.
* **Not an answer to F17-C's additive-phase question.** That question ("did
  the original draw additive surfaces after all translucency, or interleaved?")
  needs an original observation of an additive surface. 0 of 16 materials
  classify, so this stage produces no evidence about it either way; it is
  recorded as unknown, not decided.

## Recorded unknowns and limitations

* **The original's render classes are unestablished** (16 of 16 `undeclared`).
  Affected content: every original surface. Resolving task: the render-class
  evidence table filed below, gated on `#358`.
* **The F17-C additive-phase ordering question stays open** (assumption row 10
  of `docs/findings/2026-09-30-f17-c-followup-additive-material.md`). Affected
  content: every additive-class surface (muzzle flash, engine glow, tracer
  light). Resolving task filed below, gated on `#358`.
* **One world group, five subjects.** `MATRIX_WORLD_GROUP` pins `C1C` (the
  group the retail playtest pins too) and the aircraft side is the shared
  `ZBD/planes.zbd`. The other seven world groups are not in the set; a
  follow-up task covers widening it.
* **The captures are geometry witnesses**, so "screenshot matrix" here means
  one real frame per subject from the real adapter, not a textured, lit
  beauty shot and not an on-screen comparison.
* **Anchor names are installation-specific.** Every one is re-checked at run
  time and refused by name when absent, so a renamed stored node fails the
  retail test rather than silently selecting something else.

## Tests

`cargo test --workspace --locked -- accept_f17_d_ --include-ignored` discovers
**9** tests, all passing:

| test | kind | what it pins |
| --- | --- | --- |
| `accept_f17_d_the_comparison_set_is_exactly_the_five_required_subjects` | fast | AC04's five codes, their order, their distinct coverage buckets, their side |
| `accept_f17_d_a_set_missing_or_duplicating_a_required_subject_is_refused` | fast | `build` refuses 4 rows (`MissingSubject::NightEffects`) and 6 rows (`DuplicateSubject::Skyline`) |
| `accept_f17_d_an_unresolved_subject_stays_in_the_set_with_its_reason` | fast | a refusal is a row with a readable reason, never a dropped subject |
| `accept_f17_d_a_selection_rule_that_finds_nothing_is_refused_by_name` | fast | eight failure cases: no world record, two world records, absent `horizon`, anchor with no mesh, absent `cockpit1`, absent night anchors, uninstanced vegetation, ambiguous vegetation |
| `accept_f17_d_a_selection_rule_finds_the_anchor_and_its_candidates` | fast | the happy paths return the anchor and its mesh-bound candidates in stored order |
| `accept_f17_d_material_coverage_counts_every_material_and_never_invents_a_class` | fast | counts add up, refusals group by code, no unclassified material is counted opaque, empty coverage is not "fully unclassified" |
| `accept_f17_d_retail_every_subject_resolves_to_named_original_content` | retail | all five resolve against `$CS_GAME_DIR`; the measured anchors, extents and the `stars` refusal |
| `accept_f17_d_retail_the_comparison_s_material_coverage_is_reported_not_assumed` | retail | ≥1 material per subject, complete accounting, every one `undeclared`, zero counted `Opaque` |
| `accept_f17_d_gpu_every_subject_draws_a_measured_frame` | gpu | five frames on the real adapter, each `drew_geometry()`, each digest re-read from disk |

## Commands run

All from the workspace root on branch
`rally/72-review-original-data-screenshot-matrix-a`, Rust 1.98.1.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (3 893 passed, 0 failed, 418 suites) |
| `cargo test --workspace --locked -- accept_f17_d_ --include-ignored` | 0 (9 tests) |
| each of the 9 tests alone with `--exact --include-ignored` | 0 (9 × 1 passed) |
| `python3 tools/validate_evidence.py private/evidence/F17-D/acceptance.json --artifact-root private/evidence/F17-D --require-pass` | 0 (`structurally_valid: true`, 7 artifacts) |

## Evidence

* `private/evidence/F17-D/acceptance.json` — schema
  `schemas/evidence.schema.json`, capabilities `["retail", "gpu", "synthetic"]`,
  9 discovered / 9 executed / 9 passed, `unknowns: []`, claim `implemented`.
  The committed copy `docs/findings/evidence/F17-D.json` carries the
  **reviewer's** regeneration on the rebased head `0dd7885b` (tree
  `59b13b51f9dfe1ff1677027796981cf207d8d10f`); it reproduces the implementer's
  report for tree `49335d6f80ce46d541140d71e65b23b9e8e565f9` (commit
  `c583fc13`) field by field — see *Reviewer verification* below.
* The branch was rebased onto `origin/main` (19 commits, no conflicts; none of
  them touched this branch's files or any `Cargo.toml`/`Cargo.lock`), all four
  checks were re-run green on the rebased head, and the report was
  **regenerated there** — an earlier report for the pre-rebase tree was
  discarded rather than reused. The only delta after that regeneration is this
  finding's own text and the committed evidence copy, whose bytes are that
  file.
* Installation `c14a876f4457d8710dee7986333ab636122c9549cf72b646fd69cbe7e72c5352`,
  content
  `148a24b7b0506812e8f1ee13d8d3137a05926abebbe10161994e8c4cd300c35e`,
  engine rustc 1.98.1 / bevy 0.19.1 / avian 0.7.0.
* Artifacts referenced by SHA-256: `cargo-test.log`,
  `comparison-matrix.json`, and the five `subject-*.png`. The PNGs, the log
  and the matrix stay in `private/`; the committed copy
  `docs/findings/evidence/F17-D.json` carries hashes and paths only.
* `unknowns: []` is this task's own blocker list and is empty because the
  acceptance run passed. The **product incompleteness** above — 0 of 16
  materials classified, no original run, one world group — is the matrix's
  asserted verdict, pinned by its tests and written out here. It is not
  dropped to satisfy the validator.

## Reviewer verification (2026-10-07, `bunny-alpha-1`)

Independent review of this branch (Rally #72). Implementer: `bunny-alpha-2`
(opencode). Reviewer: `bunny-alpha-1` (opencode), a different agent instance
with a fresh context, so this is an independent re-run rather than a
re-reading of the implementer's numbers. No agent review replaces the owner's
human approval, and nothing here is `verified_original`.

* Rebased onto `origin/main` (3 incoming commits, no conflicts; none touched
  this branch's files and none touched a `Cargo.toml`/`Cargo.lock`).
* `cargo fmt --all -- --check` → 0; `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings` → 0; `cargo test --workspace --locked`
  → 0 (422 suites, 3 912 passed, 0 failed, 515 ignored);
  `cargo test --workspace --locked -- accept_f17_d_ --include-ignored` → 0
  (9 of 9, including the two `#[ignore]`d retail tests and the GPU capture
  test, run with `CS_GAME_DIR` set and a real adapter).
* Evidence **regenerated on the rebased head** `0dd7885b` (tree `59b13b51…`)
  and validated with
  `python3 tools/validate_evidence.py private/evidence/F17-D/acceptance.json
  --artifact-root private/evidence/F17-D --require-pass` → 0
  (`structurally_valid: true`, 7 artifacts). `comparison-matrix.json` and all
  five `subject-*.png` digests are **byte-identical** to the implementer's
  report, as are `install_sha256`, `content_sha256`, the engine versions and
  all nine assertions; only `cargo-test.log` (a different invocation) and the
  run metadata (`candidate_tree`, `cwd`, `created_at`) differ. The committed
  copy now holds this reviewer run.
* **Mutation check:** deleting `ComparisonMatrix::build`'s subject validation
  makes
  `accept_f17_d_a_set_missing_or_duplicating_a_required_subject_is_refused`
  fail (exit 101, four rows accepted). The source was restored byte-for-byte
  and all four checks re-run green afterwards, so the branch as pushed carries
  no mutation.

## Wiring edits (outside owner paths, logic-free)

* `crates/cs_app/src/render/mod.rs`: `pub mod matrix;` and one doc paragraph.
* `crates/cs_app/tests/render/main.rs`: `mod matrix;`, `mod evidence;` and one
  doc paragraph.

No protected path, no original datum and no binary file is committed; every
artifact of the original installation stays under `private/`.

## Follow-up tasks filed

Created with `create_tasks` from this stage's unknowns:

| task | subject | gate |
| --- | --- | --- |
| **#734** `F17-E-CLASS-TABLE` | the evidence-backed render-class table the 0-of-16 coverage needs | `#358 REF-OWNER-FIRST-CAPTURE` |
| **#735** `F17-E-ADDITIVE-PHASE` | F17-C's assumption row 10 (additive after all translucency, or interleaved) | `#358 REF-OWNER-FIRST-CAPTURE` |
| **#736** `F17-E-MATRIX-COVERAGE` | widening the matrix to all eight world groups and to the textured capture path | none |

None of them is fixed here: this stage records what it measured and does not
guess a class, an order or a group it did not read.

## Sources

`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
(deliverable, non-negotiable 1 and 3, AC04, `### F17-D`);
`docs/contracts/IDENTITY-CONTENT.md`; `docs/contracts/CLI-EVIDENCE.md`;
`schemas/evidence.schema.json`;
`docs/findings/2026-09-30-f17-a-material-classification-and-golden-scene.md`
(what F17-D was declared to answer);
`docs/findings/2026-09-30-f17-b-canonical-mesh-and-image-to-bevy.md`;
`docs/findings/2026-09-30-f17-c-followup-additive-material.md` (assumption row
10);
`docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md` (the
capture and evidence-harness pattern this stage follows);
`docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`
(the cockpit-binding capture precedent);
`docs/findings/2026-10-05-t648-playtest-retail-scene.md` (the horizon dome,
the vegetation instances and the pinned airframe this stage re-derives);
`docs/findings/2026-10-02-gamez-node-array-layout.md` (the node kinds and the
world record).
