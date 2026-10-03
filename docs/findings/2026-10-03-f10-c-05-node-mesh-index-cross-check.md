# A scene node's `mesh_index`, range-checked against the container's mesh array

Date: 2026-10-03. Task: #495 (F10-C.05) of
`specs/F10-gamez-mesh-topology-and-material-records.md`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio` were
available and **not used**: nothing is rendered or played and no original run
happened, so nothing here claims `verified_original`.

This closes the gap `docs/findings/2026-10-02-gamez-node-array-layout.md`
recorded under "Unknowns and limitations": the node reader carries each node's
stored `mesh_index` raw and reports the bounds it sees, and nothing checked the
index against the array it names.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/gamez/bindings.rs` (new, the check and its verdict):
  `MeshSlotIssue`, `NodeMeshFinding`, `NodeMeshBindings` and
  `NodeMeshBindings::of`.
- `crates/cs_formats/src/gamez/reader.rs` (extend, an F10 owner path):
  `GameZMeshes::slot_count` and `GameZMeshes::node_bindings`.
- `crates/cs_formats/src/gamez/{mod,nodes}.rs` (wiring and docs only): the
  module declaration, the re-exports, and the two doc paragraphs that said the
  check was a caller's to make.
- `crates/cs_content/src/mesh.rs` (extend, an F10 owner path):
  `MeshContainer::node_bindings`, `MeshContainer::blocked`, a module-doc
  paragraph, and the `accept_f10_c_05_*` tests with the fixture writer
  `gamez_container_with_nodes`.
- `crates/cs_formats/tests/gamez/bindings.rs` (new, an F10 owner path): the
  `accept_f10_c_05_*` tests and the fixture writer `node_section`. The mesh-side
  fixture writer is reused from `crates/cs_formats/tests/gamez/reader.rs`, whose
  fixture items became `pub(crate)`; no existing fixture or test changed.
- `docs/findings/2026-10-03-f10-c-05-node-mesh-index-cross-check.md` (this file).

**One observable failure:** the mesh index is **non-sequential**. An absent mesh
record stores the index the next present one is expected to carry, and that
expectation follows a different order than the array position
(`crates/mech3ax-gamez/src/gamez/cs/fixup.rs`). So the slot a node names is *not*
its position in a compact list of the meshes that are present, and the two
archives that carry a remap table — `ZBD/planes.zbd` and `ZBD/C4/gamez.zbd` —
are exactly where a compact enumeration answers for a different slot than the
node stored. In `planes.zbd` the mesh array holds **2 250** slots of which
**1 766** store a mesh, so 484 positions are all-zero stubs; a check that
enumerated the present meshes and indexed that list would report a completely
different set of verdicts. The check therefore uses `GameZMeshes::get` on the
stored slot, which is the lookup the pinned reference performs, and
`accept_f10_c_05_the_slot_is_looked_up_where_it_is_stored_not_in_a_compact_list`
is the test that fails if that ever changes.

## Design decisions

- **An absent slot and an out-of-range index are two codes.** `MeshSlotIssue::Absent`
  is a position the container really has and stores an all-zero stub in;
  `MeshSlotIssue::OutOfRange` is a position it never had. The task's reason for
  the check is exactly this distinction — an absent mesh is a different fact from
  an uncatalogued one — so collapsing both into one "unknown mesh" would throw
  the finding away. The message names the node, the stored `mesh_index`, the slot
  and (out of range) the array size.
- **The check is a report, not an error.** Both sections have already read, every
  node is still carried with its stored index, and `MeshContainer` still opens.
  This follows the `NodeFinding` and `ParseFinding` precedent: the reference's
  assertion is *measured*, not trusted, and a container whose finding list is
  empty is the strongest claim this module makes about it.
- **A negative index is not this check's business.** `-1` is the reference's "no
  mesh" sentinel and names no position in the array, so it is counted as
  `unnamed`, not range-checked. A value **below** `-1` is a different fact and is
  already a `NodeFinding::MeshIndexSentinel` from the node reader;
  `accept_f10_c_05_a_negative_index_names_no_slot_and_is_not_a_finding_here` pins
  that this check does not double-count it.
- **The node reader keeps the index raw and the bounds stay its honest
  substitute.** The check lives in a function that is handed both sections
  rather than in `read_gamez_nodes`. A node reader that took a mesh section would
  have to be given one at every call site, and a call site with none could not run
  the check at all. The two doc paragraphs that described the gap now name the
  check instead.
- **The node array is an argument to `MeshContainer`, not a field.**
  `MeshContainer::open` reads the mesh and material sections — F10-B's and
  F10-C.02's — and the node section is F11's, which owns the hierarchy import and
  converts `mesh_index` through its own `MeshSlot` catalog. Making the F10 catalog
  read the node array would couple it to a section it does not own and would fail
  a container whose mesh rows are fine. A caller that holds the nodes — F11's
  import, or a survey like `cs_app::world::triggers` — pairs them at the seam
  that owns the mesh array. `MeshContainer::blocked` then joins the audit's
  blocked material rows and the node findings into one list, so node and mesh
  problems are read together rather than fetched from two readers;
  `accept_f10_c_05_the_node_half_is_additive_to_the_material_audit` pins that a
  catalog with no node array is exactly as it was.

## Measurement: all nine GameZ archives

Every number is from the production readers over the read-only installation.
`records` is the node array's record count, `bound` how many nodes store a
non-negative `mesh_index` (the node reader's own `mesh_index_bounds().bound`),
`min`/`max` its own bounds, `slots` the mesh array's size, `present` how many of
those slots stored a mesh, and `resolved` how many nodes named a present one.

| archive | records | bound | min..max | slots | present | resolved | findings |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` | 3 317 | 1 766 | 0..1 778 | 2 250 | 1 766 | 1 766 | 0 |
| `ZBD/C1/gamez.zbd` | 7 064 | 3 966 | 0..2 236 | 2 250 | 2 237 | 3 966 | 0 |
| `ZBD/C1B/gamez.zbd` | 5 603 | 3 485 | 0..1 304 | 1 500 | 1 305 | 3 485 | 0 |
| `ZBD/C1C/gamez.zbd` | 5 644 | 3 354 | 0..1 517 | 2 250 | 1 518 | 3 354 | 0 |
| `ZBD/C2/gamez.zbd` | 4 956 | 2 558 | 0..1 764 | 2 250 | 1 765 | 2 558 | 0 |
| `ZBD/C2B/gamez.zbd` | 4 901 | 3 039 | 0..1 364 | 1 500 | 1 365 | 3 039 | 0 |
| `ZBD/C3/gamez.zbd` | 5 408 | 2 868 | 0..1 900 | 2 250 | 1 901 | 2 868 | 0 |
| `ZBD/C4/gamez.zbd` | 8 289 | 4 929 | 0..2 489 | 3 000 | 2 431 | 4 929 | 0 |
| `ZBD/C5/gamez.zbd` | 11 438 | 6 003 | 0..2 850 | 3 000 | 2 851 | 6 003 | 0 |

**Every stored index in all nine archives names a present mesh**, so the
reference's assertion holds over the whole corpus and this task's finding list is
empty everywhere. That is a measurement, not an assumption, and it is what the
retail test pins for the two remapped archives.

What the measurement says that was not said before:

- The containers hold **absent slots** — 484 of `planes.zbd`'s 2 250, 569 of C4's
  3 000, 732 of C1C's 2 250 — and **no node names one**. The all-zero stubs are
  unused capacity in the arrays, not dangling associations, so "a node resolved to
  an unknown mesh" cannot be explained by an absent slot in this corpus. (Whether
  the original engine ever followed one is a question about the engine, not about
  these files; see the unknowns below.)
- **Every present mesh is named by at least one node**, in every archive
  (`present never named` = 0 across all nine), and the number of *distinct* slots
  the nodes name equals the number of present meshes in every archive. So the
  association covers the mesh array completely; it is not a bijection everywhere,
  because a slot may be named more than once.
- **In `planes.zbd` — the aircraft container — it is a bijection.** Its 1 766
  bound nodes name 1 766 distinct slots, each exactly once (maximum multiplicity
  **1**), and its 1 766 present meshes are all named. The world containers share
  geometry instead: one slot is named by up to 28 nodes in `C1`, 24 in `C3`, 23 in
  `C4` and **79** in `C5`. A stage that needs the multiplicity has to measure it;
  this check reports only which nodes name no mesh, never how many name the same
  slot.
- **The `mesh_index` word is not the array position in a compact list**, which is
  already known from the mesh layout, and this is the second measurement that
  depends on it: `planes.zbd`'s 484 absent slots are exactly the positions a
  compact enumeration would shift.

The distinct-slot count, the "never named" count and the maximum multiplicity were
measured by walking the same two production readers' own outputs over the
installation; they are recorded here as measurements and **not** asserted by a
test, because `NodeMeshBindings` deliberately does not carry a multiplicity.

## Test inventory

| `accept_f10_c_05_*` test | Covers | Fails when |
| --- | --- | --- |
| `a_node_naming_a_slot_outside_the_mesh_array_is_named_with_its_node_and_slot` | an index past the array, its node, its slot, the array size, the code and the message; that the node reader raises no finding and carries the index raw | the slot, the node or the array size is missing from the report, the code changes, or the check is moved into the node reader |
| `a_node_naming_an_all_zero_stub_is_absent_not_out_of_range` | an inside-the-array stub reported as `mesh_slot_absent` and **not** as out of range; both stub slots reported; the two present slots reported as findings for nothing | the two codes collapse into one, or a present slot is reported |
| `the_slot_is_looked_up_where_it_is_stored_not_in_a_compact_list` | a `Fixup::Planes` container with one present mesh at slot **3** of five: the node storing 3 resolves and the three stub nodes are absent findings | the lookup becomes a compact enumeration, which resolves slot 0 and refuses 3 |
| `a_container_whose_every_index_resolves_is_complete` | `bound` equals the node reader's own `mesh_index_bounds().bound`, `slots` and `present` equal the mesh section's own, and the summary line agrees | the check's counts drift from the two readers' independent ones |
| `a_negative_index_names_no_slot_and_is_not_a_finding_here` | `-1` and `-5` are both `unnamed`; the `-5` stays a **node reader** finding and is not counted twice | a negative index is range-checked, or the node reader's own finding is lost |
| `a_broken_node_array_is_refused_before_the_check_runs` | trailing bytes the node walk does not account for are refused as `DataEnd`, while the mesh section of the same bytes still reads | the two readers become interlocked, or the refusal changes |
| `retail_both_remapped_archives_resolve_every_node_mesh_index` (retail, `#[ignore]`d) | the real `ZBD/planes.zbd` and `ZBD/C4/gamez.zbd` through `read_gamez_nodes` **and** `read_gamez_meshes`: each one's selected fixup, record count, `bound`, `min`/`max`, `slots`, `present`, and that `resolved == bound` with an empty finding list | either archive's fixup stops being the remapped one, a count moves, or the check stops resolving on real bytes |
| `the_container_reports_node_and_mesh_problems_in_one_list` (`cs_content`) | a four-slot container whose node array names a present slot, a stub and an index past the array: the codes, the two nodes, and one joined list in stored order — while the container still opens and its meshes are still rows | the joined list loses either half, or the check fails the container |
| `the_node_half_is_additive_to_the_material_audit` (`cs_content`) | a catalog with **no** node array is unchanged, and `blocked` with an empty verdict is exactly the material audit's own list | the node half becomes required of `MeshContainer::open` |

**Sensitivity check.** Five mutations applied and reverted, each killed by a test
CI can run:

| mutation | killed by |
| --- | --- |
| resolve the slot from a compact enumeration of the present meshes instead of `GameZMeshes::get` | `the_slot_is_looked_up_where_it_is_stored_not_in_a_compact_list`, `a_node_naming_an_all_zero_stub_is_absent_not_out_of_range` |
| clamp an out-of-range index to the last slot | `a_node_naming_a_slot_outside_the_mesh_array_is_named_with_its_node_and_slot` |
| report only the first node that names no mesh | `a_node_naming_an_all_zero_stub_is_absent_not_out_of_range`, `the_slot_is_looked_up_where_it_is_stored_not_in_a_compact_list` |
| drop the node half from `MeshContainer::blocked` | `the_container_reports_node_and_mesh_problems_in_one_list` |
| `MeshContainer::node_bindings` ignores the caller's node array | `the_container_reports_node_and_mesh_problems_in_one_list` |

## Unknowns and limitations (all recorded, none guessed)

- **The slot a node names says nothing about how often.** This check reports the
  set of nodes that name no mesh, not the multiplicity of the ones that do, and
  `NodeMeshBindings` carries no count of it. The multiplicity was measured (above)
  but is not part of the type. **Affected content:** a stage that needs it — an
  instance count, or a per-mesh "how many nodes draw this" report — must measure
  it over `GameZNodes` rather than read it here.
- **An absent slot that no node names is unused capacity, and only that.** 484
  positions of `planes.zbd`'s 2 250 hold an all-zero stub and none is named by a
  node, which is a fact about the corpus. Whether the original engine treated an
  absent slot as "draw nothing" or refused the container is **unmeasured**: the
  reference asserts a node's index is inside the array *and* holds a present mesh,
  which is an assertion about what that tool accepts, not a measurement of what the
  game did. **Affected content:** nothing in this task's claims; resolving it needs
  an original run.
- **The header is not cross-checked between the two sections.** Both readers call
  the same `read_container_header` over the same bytes, so a node array and a
  mesh array read from **different** containers would be paired without complaint
  here. `cs_content::mesh::MeshContainer::open` cross-checks the mesh and material
  headers against each other for the same reason, and extending it to the node
  array means `MeshContainer` has to read one, which is F11's decision (see the
  design decisions above). **Resolving it** is a follow-up if F11 pairs arrays
  from a caller that could pass mismatched bytes.
- **Which mesh a node ends up drawing is F11's and F17-B's.** This check
  establishes that every stored index names a present mesh; it does not decide
  which node's transform applies, how a LOD node selects, or what a renderer
  uploads. Those are the stages that own them.
- **`mesh_index` on a node whose record is not an object record is carried and
  checked the same way.** Nothing in this task establishes that a world, window,
  camera, display or light record's `mesh_index` word means the same thing an
  object's does; the field is at the same offset in the shared 208-byte record and
  is read as stored. The measured corpus happens to store `-1` on those records,
  so the corpus does not exercise a non-`-1` value there and this module does not
  claim one would be meaningful.
- **Evidence class.** The mesh index's non-sequentiality, its two remap tables and
  the node reader's `mesh_index` offset are documented in the pinned reference
  (mech3ax v0.6.0, commit `d3521a9721be731d365504568ddcd78e3f9846bb`) and the
  verdicts above were measured from the original bytes across all nine archives,
  which is `ObservedTool`. No original run happened: `retail` is file access, not
  evidence of runtime behaviour, and the reference's assertion is that tool's rule
  rather than a measurement of the game. **A further agent instance with a fresh
  context should review this format work**, and no agent review replaces the
  owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, ranges and verdict totals; no name list, no mesh, no material and no
  screenshot is in the repository.

## Sources used

- `specs/F10-gamez-mesh-topology-and-material-records.md` (F10-C's deliverable and
  non-negotiable behaviors, AC04's exact counts).
- `docs/contracts/IDENTITY-CONTENT.md` (catalog collections keep failed entries,
  unsupported reasons are codes, exact lookup with no filename guessing).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (`mesh_index`, the measured
  bounds, the non-range-check gap this closes, and the recorded unknowns that
  still stand).
- `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` (the non-sequential mesh
  index and the two measured remap tables).
- `docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` and
  `docs/findings/2026-09-29-f10-c-03-mesh-container-catalog-and-upload.md` (the
  catalog this is wired into, and the report shape it follows).
- Pinned reference mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
  `docs/research/SOURCES.md`): `crates/mech3ax-gamez/src/gamez/cs/nodes.rs`
  (`read_nodes`' assertion that a non-negative `mesh_index` is inside the mesh
  array and holds a present mesh) and `crates/mech3ax-gamez/src/gamez/cs/fixup.rs`
  (`Fixup`, the two remap tables). No code was copied; mech3ax is EUPL-1.2 and is
  read as a reference only.
- `crates/cs_formats/src/gamez/{nodes,reader,census}.rs` and
  `crates/cs_content/src/mesh.rs` (the readers this pairs and the catalog this is
  wired into).
