# #540: each declared cockpit binding's subtree, resolved to drawable geometry

Date: 2026-10-11. Task: #540 "Resolve each declared cockpit binding's subtree
to geometry (F21-D follow-up)". Feature sheet:
`specs/F21-cameras-cockpit-views-and-spyglass.md`, stage `### F21-D`. Shared
contract: `docs/contracts/UI-NETWORK.md`; evidence contract:
`docs/contracts/CLI-EVIDENCE.md`. Predecessor:
`docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`,
whose coverage audit left 44 of 220 declared `(airframe, binding)` pairs
without directly resolved drawable geometry and filed this task.

Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`) and
ordinary build/test. `gpu` and `audio` were available and **not used**: nothing
is rendered and nothing is played; the measurement is over node and mesh
arrays, not frames.

**Nothing here is `verified_original`.** No original run happened. `retail`
here is read access to files. Whether the original's engine *draws* these
descendants when it runs is behaviour no file records — this task names the
geometry a declared binding's own subtree holds, not what the original does
with it. A different agent instance with a fresh context should review this
measurement, and no agent review replaces the owner's approval.

Installation fingerprint:
`b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` (the F02-B
`install::fingerprint` over the whole manifest — the same installation F21-D
measured).

## What F21-D left open

`audit_cockpit_coverage` resolves each declared `(airframe, binding)` pair to a
single node in the airframe's children closure. Over the retail corpus that is
176 `Bound` pairs (16 per airframe) and 44 pairs in a named unresolved state:

- `gungauge` and `missilegauge` resolve to one node each, but that node stores
  `mesh_index = -1` → `NoMesh`, 22 pairs;
- `4char_ammo` and `6char_type` name two nodes in the same airframe →
  `Ambiguous`, 22 pairs.

The audit answered *which node*, correctly, and stopped there: a `NoMesh`
container's drawable children and an `Ambiguous` name's per-candidate geometry
were never reported. This task adds the second half of the question — *what
the resolved node's own subtree draws* — as the production pass
`cs_app::camera::coverage::resolve_cockpit_content`.

## What was added

`resolve_cockpit_content(report, archive, archive_id)` takes an audited
`CockpitCoverageReport` and the `GameZNodes` array it was measured over, and
for every declared binding of every audited airframe walks each resolved
candidate's own children closure (the same `subtree_indices` walk the audit
applies to the airframe root — children edges only, never parent edges, so a
candidate's content can never leave the airframe it was found in). It returns
a `CockpitContentReport` of `AirframeCockpitContent` rows, each carrying
`CockpitBindingContent` rows in declared order. Per binding:

- `coverage()` is the audit's `CockpitNodeCoverage` **carried verbatim** — a
  container whose descendants draw is still `NoMesh`, a name two nodes share
  is still `Ambiguous`;
- `subtrees()` is one `CockpitSubtreeContent` per resolved candidate: the
  candidate's index, its stored name, its own signed `mesh_index`, the size of
  its subtree (`subtree_nodes`), and the subtree's drawable members as
  `(node_index, stored name, mesh_index)` triples;
- `geometry()` is the binding-level union of those members, deduplicated by
  node index; `AirframeCockpitContent::drawable_nodes()` is the same union per
  airframe — a node inside two bindings' subtrees (the corpus has these: a
  `4char_ammo` leaf is both its own binding's candidate and a member of a
  gauge container's subtree) is still one instrument.

Refusals: `ContainerMismatch` when a coverage row was audited over a different
archive than the one handed to the pass, and the new
`CockpitCoverageError::ResolvedNodeAbsent` when an index the report resolved
is not in the array — both mean the array is not the one the audit measured,
and a subtree reported against it would be a different container's content.

## The measured corpus

Over `ZBD/planes.zbd` (`read_gamez_nodes`, 3 317 nodes), all eleven declared
airframes show the identical structure (the acceptance suite pins each row;
the `player_pfighter` row, node indices omitted):

- `gungauge` — `NoMesh` container, subtree of **10** nodes: itself, a
  mesh-less `markers` group, and **8 drawable** members: `4char_ammo`,
  `ggindicator0`–`ggindicator3`, `ggarrow`, `6char_type`, `g815`.
- `missilegauge` — `NoMesh` container, subtree of **14** nodes: itself, a
  mesh-less `marker` leaf, and **12 drawable** members: `4char_ammo`,
  `mgindicator0`–`mgindicator7`, `mgarrow`, `6char_type`, `g819`.
- `4char_ammo` — `Ambiguous`, two candidates: one leaf inside `missilegauge`,
  one inside `gungauge`; each draws its own (different) mesh.
- `6char_type` — `Ambiguous`, same shape: one leaf per gauge, each with its
  own mesh.
- The other sixteen declared names (`ggindicator0–3`, `mgindicator0–7`,
  `rightwingdamage`, `leftwingdamage`, `taildamage`, `nosedamage`) — `Bound`
  leaves whose subtree is themselves: one drawable node, carrying the mesh
  index the audit resolved. The twelve indicators' nodes are the same nodes
  the two gauge subtrees already hold.

So per airframe the bindings' subtrees hold **24 distinct drawable nodes** in
union (8 + 12 + 4 damage nodes; the indicator and character leaves are shared
subtrees' members), and **every one of the 220 declared pairs resolves to
drawable subtree content** — including all 44 pairs whose direct state stays
`NoMesh` or `Ambiguous`. Every drawable member's `mesh_index` was checked
against the archive's own mesh section (`read_gamez_meshes`): in range, and a
stored mesh, not an empty stub slot. The per-airframe drawable sets are
pairwise disjoint (264 nodes across the corpus), so no candidate's subtree
ever crossed an airframe boundary.

The full per-airframe, per-binding, per-node measurement is committed in the
`binding-subtree-census.json` artifact hashed by
`docs/findings/evidence/T540.json`.

## What this deliberately does not answer

- **Where the pilot's eye sits.** Still `Undeclared`: no readable file names
  it (task #542). A drawable gauge subtree is content, not a viewpoint.
- **What the original *does* with this geometry.** `mesh_index >= 0` is an
  association into the archive's mesh array, checked against the mesh section;
  whether the original engine draws it, and how the `markers`/`marker`
  mesh-less members behave, is a runtime question no file answers.
- **`CameraSetHorizon`'s effect** on these instruments (task #541) and the
  meaning of the `g815`/`g819` nodes, which carry meshes but no declaring
  script line names them individually.
- **Why the character nodes are duplicated.** Two `4char_ammo`/`6char_type`
  leaves exist per airframe, one per gauge; which one the original's script
  binding drives at runtime is not decidable from the files — the report keeps
  `Ambiguous` and names both.

## Files changed

- `crates/cs_app/src/camera/coverage.rs` — the content pass, its result types
  (`CockpitContentNode`, `CockpitSubtreeContent`, `CockpitBindingContent`,
  `AirframeCockpitContent`, `CockpitContentReport`), the `ResolvedNodeAbsent`
  refusal and module documentation.
- `crates/cs_app/src/camera/mod.rs` — re-exports and the module-doc bullet
  (wiring only).
- `crates/cs_app/tests/camera/content.rs` — the `accept_t540_` suite; shares
  the F21-D fixtures via `pub(crate)` items in
  `crates/cs_app/tests/camera/coverage.rs` (visibility-only edits).
- `crates/cs_app/tests/camera/main.rs` — `mod content` and the module docs
  (wiring only).
- `crates/cs_app/tests/evidence_report_t540.rs` — the evidence harness (not an
  acceptance test).
- `docs/findings/evidence/T540.json` — the validated acceptance report.

## Test sensitivity

Nine `accept_t540_` tests: eight over authored archives (CI runs them) and one
retail test (`#[ignore = "requires CS_GAME_DIR"]`). Each mutation below was
applied to `resolve_cockpit_content`, run against the task selection, and
reverted; the failures listed are the ones actually observed:

| Removed behaviour | Tests that failed |
| --- | --- |
| drawable list emptied (`geometry` → `vec![]`), run with `--include-ignored` | 8 of 9 — every test that asserts content; the refusal test does not and passed |
| subtree walk replaced by the candidate node alone | 5 of the 8 synthetic tests — the container, contentless-subtree, isolation, union and verbatim tests; the leaf, ambiguous and refusal tests correctly survive, since their subtrees are singletons |
| `NoMesh` verdict rewritten to `Bound` on the content row | 4 of the 8 synthetic tests — every test that compares the carried verdict |
| `ContainerMismatch` check removed | `accept_t540_a_mismatched_or_truncated_archive_is_refused_not_walked_past` |

## Checks run

* `cargo fmt --all -- --check` = 0;
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  = 0; `cargo test --workspace --locked` = 0; and
  `cargo test --workspace --locked -- accept_t540_ --include-ignored` = 0
  (9 discovered, 9 passed: 8 unignored authored-corpus tests for CI and the
  retail test run over `$CS_GAME_DIR`). Each discovered test was also run
  individually with `--exact` and passed.
* Evidence: `private/evidence/T540/acceptance.json` produced by
  `evidence_report_t540` from the recorded acceptance log, production
  discovery of `$CS_GAME_DIR` and a second full production re-run of the
  measured chain inside the harness; validated with
  `tools/validate_evidence.py --artifact-root private/evidence/T540
  --require-pass`; copy committed as `docs/findings/evidence/T540.json`.

## Sources used

- `specs/F21-cameras-cockpit-views-and-spyglass.md` (### F21-D),
  `docs/contracts/UI-NETWORK.md`, `docs/contracts/CLI-EVIDENCE.md`,
  `docs/findings/2026-10-03-f21-d-original-view-controls-and-cockpit-coverage.md`
  and `docs/findings/evidence/F21-D.json`.
- The owner's installation, read-only, over `$CS_GAME_DIR`: `ZBD/interp.zbd`
  (`support\cockpit.gw`, `support\planes.gw`) and `ZBD/planes.zbd` (node array
  and mesh section). Installation fingerprint above.
- `crates/cs_app/src/camera/coverage.rs` (the F21-D audit this builds on) and
  `crates/cs_formats/src/gamez/` (the node/mesh readers).

**No original data is committed.** The census carries counts, stored node
names, node and mesh indices and digests; no extracted script, no mesh, no
screenshot is in the repository, and every private output went to `private/`,
outside it.
