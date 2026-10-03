# F14-D.4: the scene, mesh and material collections of the baseline inventory

Date: 2026-10-03. Task: #487 "Populate the scene, mesh and material collections
of the retail baseline inventory". Feature sheet:
`specs/F14-canonical-content-catalog-and-dependency-closure.md` (stage
`### F14-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Evidence
contract: `docs/contracts/CLI-EVIDENCE.md`. Capabilities used: **`retail`**
(read-only access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio`
were available and **not used**: nothing is rendered or played, and no original
run happened, so nothing here claims `verified_original`.

## The gap this closes

`IDENTITY-CONTENT` requires a catalog collection for "scene roots and nodes",
"render meshes/materials/images" and "collision surfaces". Before this stage
`cs_content::catalog::baseline::retail_baseline` held **no** `SceneNode`, `Mesh`,
`Material`, `Image` or `CollisionSurface` row, so the retail catalog reported
338 rows in six collections and the world's geometry — 52 386 080 bytes of GameZ
container across nine archives — was inventoried only as nine `install_file` rows.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/catalog/baseline.rs` (extend, an F14 owner path): the
  claim ids `CLAIM_GEOMETRY_CONTAINER`, `CLAIM_NODE_PARENTAGE`,
  `CLAIM_NODE_MESH_SLOT`, `CLAIM_AMBIGUOUS_NODE_PATH` and
  `CLAIM_UNSPELLABLE_NODE_PATH`; the constants `GEOMETRY_CONTAINER_FILE`,
  `PLANES_CONTAINER_KEY` and `GAMEZ_CONTAINER_RECORD`; the report struct
  `GeometryContainerReport` and `Baseline::geometry_containers`; the private
  `NodeIdentity`, `gamez_geometry_rows`, `read_geometry_container`,
  `geometry_node_rows`, `geometry_mesh_rows`, `container_span`,
  `node_name_paths`, `scene_node_id`, `scene_node_key`, `mesh_id`,
  `node_key_unknown`, `geometry_sources`, `geometry_container_json`; and
  `BaselineError::UninventoriedContainer`.
- `crates/cs_content/tests/accept_f14_d_4_geometry_collections.rs` (new): the
  nine `accept_f14_d_4_*` tests and the authored GameZ container writer.
- `crates/cs_content/tests/evidence_report_f14_d_4.rs` (new): the evidence
  harness.
- `crates/cs_content/tests/accept_f14_d_baseline.rs` (extend): one field in an
  existing `Baseline { .. }` literal.
- `docs/findings/2026-10-03-f14-d-4-scene-and-mesh-collections.md` (this file).

**One observable failure:** the node array's two passes must tile the container
and the two readers must agree about where the mesh section ends. A reader (or a
baseline) that treated the array as one flat run of 212-byte records, that
followed the stored `data_ptr` instead of walking, or that accepted the two
sections without comparing their boundaries against the container's own header
would land on a different offset: the production readers refuse with
`GameZNodeError::DataOffset`, `GameZError::MeshDataNotSequential` or
`GameZError::MeshDataEnd` naming both numbers. The baseline's own cross-check is
that `meshes.data_end == header.nodes_offset == nodes.info_offset` and
`nodes.data_end == container length`; a container that fails any of them
contributes **no** rows and a named diagnostic, which
`accept_f14_d_4_a_container_the_readers_refuse_is_a_named_diagnostic_and_no_row`
and the retail test both pin.

## Design decisions

- **The containers come from production discovery, not from a name.**
  `cs_assets::install::Diagnosis::world_groups` names every group and
  `Diagnosis::planes_zbd` names the shared aircraft container, so the walk has
  nothing to guess. `GEOMETRY_CONTAINER_FILE` only says *which file inside a
  named group* holds the geometry, which is what F18-D's survey already treats as
  a measured fact. Each container is looked up under the inventory's own **logical
  key** (`RelativePath::logical_key`) — never under a second, privately spelled
  copy of it — and its bytes are read at the original spelling the manifest
  holds, because joining a folded key onto the host root works on a
  case-insensitive filesystem and fails on a case-sensitive one. The shared
  container is read although the task's goal sentence speaks of world groups: it
  is 3 317 of the 56 620 node records and the only aircraft geometry the
  installation has, and leaving it out would make the collection read like an
  installation with none. A group the diagnosis named but that stores no
  `gamez.zbd` contributes no rows and is **named** in the collection's
  diagnostic: a discovered group is not a promise that the file exists, so that
  is a reported gap rather than an error.
- **Both collections are read through the producing stages' readers**, over **one**
  shared `ParseContext`, because the two readers share the 40-byte header parser
  and a parse context labelled from a constant would name the wrong container in
  every diagnostic. That was one of the two defects the F14-D.2 review recorded.
- **A span into a GameZ container has `member_key: None`.** The task description
  asks for "a checked span into the container member (member key, not the file)",
  but a GameZ geometry container **is** a loose installation file: its first word
  is the CS GameZ signature and F06's own container audit classifies all nine as
  `not_listed` — "GameZ containers are not member-listed by F06; their reader is
  F10". `cs_types::asset_id::SourceSpan` documents `member_key: None` as "the
  container is the source itself (a loose file)", which is exactly this case, so
  inventing a member name would add a second, invented address for the same
  bytes. What the description asks for is kept: every span is checked, it names
  the exact byte range of the record, and the static edge points at the
  **inventory row** (`install_file/<install_file_key of the spelling>`) of the
  member that holds the bytes.
- **The node's own record is the span.** `RawNode::data_offset` /
  `RawNode::data_bytes` are the record's address and size, and the reader checks
  that address against the pointer the record itself stores, so the span is a
  checked record and not "somewhere in the file". A mesh row's span is
  `GameZMesh::data_offset .. data_end`, the mesh record's own extent.
- **The semantic key is F11-A's published name path, prefixed by the container's
  inventory key** — `scene_node/<install_file_key>.<authored name path>`, the same
  shape `cs_content::scene::SceneNodeId::for_path` derives, so a row here and a
  scene-graph node can never disagree about what a node is called. The mesh key is
  `<container key>.<stored mesh-array slot>`, the same shape the workspace's
  `MeshSlot` already uses, and the slot is the number a node's `mesh_index` names
  rather than a position in a walk.
- **A node whose semantic key cannot be derived honestly is still a row.** Where
  several nodes spell the same path, where the key is refused (a byte the
  grammar does not accept, or a key over `MAX_CONTENT_KEY_LEN`), or where the
  node's own parent-slot chain does not terminate inside the array, the row is
  keyed by `record<offset>` — its own record's address, which the reader
  cross-checks — and carries one `UnsupportedReason::Unknown` naming exactly
  which of the three happened and quoting the stored path. Merging the rows,
  transliterating a name, inventing a suffix and dropping the node are all
  refused: `IDENTITY-CONTENT` says a collection cannot exclude a failed entry,
  and F11-A's rule is that a name is never silently transliterated. The four-way
  classification lives in one place (`NodeIdentity::of`) because the
  per-container counts and the rows themselves must agree about it — the first
  version of this code had exactly that drift.
- **The parent→child edge follows the parent slots, not the child lists.** A
  node's parent is a field of its own record; a child list is a claim the parent
  makes about its children, and the world containers' two disagree (an unknown
  already recorded in `docs/findings/2026-10-02-gamez-node-array-layout.md`).
  The name path is walked the same way, and a chain that does not terminate (the
  stored parent slots form a cycle, so the node is its own ancestor) yields a
  path that names nothing: those rows are counted as `unterminated` and keyed by
  their record address instead of being published under the part of the loop that
  fitted. On this installation the count is **0** in all nine containers, which
  the retail test pins.
- **A mesh slot the node array names but the array leaves absent has no bytes of
  its own, so it gets no row** and is counted as the `named_slot_without_mesh`
  gap. Every node that names such a slot says so on its own row and carries **no**
  mesh edge: a catalog row may not point at an id nothing holds. On this
  installation that count is 0: all 17 139 named slots hold a present record.
- **Nothing is normalized and nothing is ready.** The stored translation, euler
  angles, LOD bounds and zone id are in source units and nothing in this
  workspace has established the original's world-vertex or angle unit, so every
  row carries `not_normalized` and reads `unavailable`. No row claims a runtime
  consumer.
- **The denominator does not move.** Neither kind is launchable, so no geometry
  row is a declared root; `coverage.roots` is still the campaign walk's plus the
  F14-D.1 scenario directories, and the campaign part is still the frozen F50
  inventory.

## Measurement: all nine GameZ containers

Every number below is from the production readers and the production baseline
builder over the read-only installation.

| container | bytes | nodes | named | ambiguous | unspellable | distinct paths | roots | named mesh slots | mesh rows | absent slots |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` | 6 083 868 | 3 317 | 3 311 | 0 | 6 | 3 317 | 28 | 1 766 | 1 766 | 0 |
| `ZBD/C1/gamez.zbd` | 6 875 076 | 7 064 | 5 747 | 1 003 | 314 | 6 185 | 165 | 2 237 | 2 237 | 0 |
| `ZBD/C1B/gamez.zbd` | 3 953 304 | 5 603 | 3 313 | 1 910 | 380 | 3 773 | 148 | 1 305 | 1 305 | 0 |
| `ZBD/C1C/gamez.zbd` | 4 008 612 | 5 644 | 3 813 | 1 146 | 685 | 4 538 | 149 | 1 518 | 1 518 | 0 |
| `ZBD/C2/gamez.zbd` | 4 908 288 | 4 956 | 4 194 | 468 | 294 | 4 610 | 166 | 1 765 | 1 765 | 0 |
| `ZBD/C2B/gamez.zbd` | 3 435 348 | 4 901 | 3 280 | 1 266 | 355 | 3 674 | 146 | 1 365 | 1 365 | 0 |
| `ZBD/C3/gamez.zbd` | 5 633 696 | 5 408 | 5 044 | 52 | 312 | 5 376 | 157 | 1 901 | 1 901 | 0 |
| `ZBD/C4/gamez.zbd` | 8 102 080 | 8 289 | 5 943 | 1 928 | 418 | 6 467 | 163 | 2 431 | 2 431 | 0 |
| `ZBD/C5/gamez.zbd` | 9 385 808 | 11 438 | 8 252 | 2 462 | 724 | 9 423 | 215 | 2 851 | 2 851 | 0 |

Totals: **56 620 scene node rows** (42 897 keyed by their authored name path,
10 235 keyed by their record address because the store spells that path for
another node too, 3 488 keyed by their record address because the key the path
forms is refused, **0** because their parent-slot chain does not terminate)
and **17 139 mesh rows**, one per named slot, all of which hold a present record.
Nine containers visited, nine read, none refused. The 56 620 node records match
the corpus F11-A measured; the 17 139 present mesh records match the corpus F10-B
measured ("all 17 139 present records across the nine GameZ containers store
data").

The baseline grew from the 338 rows of the six collections the first F14-D
stages published to **74 311** in twelve (228 install files, 24 missions, 53
scripts, 8 instant-action scenarios, 21 multiplayer scenarios, 4 multiplayer
rules, 8 worlds, 11 factions, 184 paint masks, 11 airframes, 56 620 scene nodes,
17 139 meshes). The nine measurements this stage is about are unchanged by the
collections the later stages added; the counts of those collections are named
here because a total that leaves them out is not a total. `coverage` is unchanged
where it matters: 53 roots, 159 reachable, 74 152 unreachable, **0 unresolved
references**, and the geometry rows are counted as unreachable unknowns that still
need a classification. The JSON report is byte-stable for the same installation
and is now ~64 MB, which is the honest size of a complete inventory of this corpus
and is recorded here rather than discovered later.

## Test inventory

| `accept_f14_d_4_*` test | Covers | Fails when |
| --- | --- | --- |
| `a_gamez_container_yields_one_node_row_per_node_and_one_mesh_row_per_named_slot` | one row per stored node and per present named slot across three containers; the F11-A name-path key; the span's container, `member_key`, offset and install fingerprint; the parse/normalize/readiness states and the fingerprint; the static edge onto the inventory row; `ObservedTool` provenance; the parent edge and its absence on a root; the mesh edge and its absence for a stored `-1`; the mesh row's own 36-byte extent; **every** edge of both collections resolving in the catalog | a row is missing or merged, a span names the file rather than the record, a member key is invented, an edge class is upgraded, a root grows a parent edge, `-1` grows a mesh edge, or an edge points at an id nothing holds |
| `a_node_whose_name_path_is_shared_is_a_row_with_an_explicit_unknown` | two nodes spelling one path are two rows keyed by distinct record addresses, each with exactly one `node_path_ambiguous` unknown quoting the stored path; per-container counts (7 nodes, 4 named, 2 ambiguous, 1 unspellable, 6 paths, 1 absent slot); the collection's gap counts | the two rows are merged, one is dropped, either keeps the name-path key, or the counts and the rows disagree |
| `a_node_whose_name_carries_unspellable_bytes_is_a_row_with_an_explicit_unknown` | a stored name ending in a space keeps its display name verbatim, is keyed by its record address with a `node_path_unspellable` unknown quoting `main.wing.brigturret `, appears nowhere as a trimmed identity, and still carries a resolving parent edge | the name is trimmed into an identity, transliterated, dropped, or the row loses its parent edge |
| `a_named_mesh_slot_without_a_present_mesh_is_counted_not_rowed` | a slot named by a node but absent from the mesh array gets no row and is counted as `named_slot_without_mesh`; the node that names it carries `node_mesh_slot_absent` and **no** mesh edge | the absent slot is given a row, the count is not reported, or the naming node grows an edge onto the id nothing holds |
| `a_node_whose_parent_chain_never_ends_is_a_row_with_an_explicit_unknown` | two nodes that are each other's parent: no loop is spelled as a name path, each is keyed by its record address with `node_path_unterminated`, a node whose chain terminates is still a semantic path, and `unterminated` plus the four-way count identity hold per container and in the collection's gaps | a loop is published as a name path, the loop nodes are merged, or the counts and the rows disagree |
| `a_chain_as_long_as_the_node_array_is_not_a_cycle` | a hierarchy that is one chain covering every stored node: the deepest node's authored name path, its resolving parent edge, and `unterminated == 0` in every container of the tree (third review pass) | a terminating chain as long as the array is reported as a loop, so its deepest node loses its name path |
| `a_container_the_readers_refuse_is_a_named_diagnostic_and_no_row` | a container that stops being a GameZ container yields no row, is named in the diagnostic, is counted in `unreadable_container`, and leaves the other two containers' rows and the F14-D inventory intact | a refused container is silently skipped, is guessed at, or takes the rest of the inventory with it |
| `the_geometry_collections_do_not_move_the_coverage_denominator` | neither kind is launchable, no geometry row is a root, `launchable_count == roots.len()`, only the mission is an unsupported launchable, both kinds appear in `unreachable_by_kind`, zero orphan references | a geometry row becomes a root, or the denominator moves |
| `the_report_renders_both_collections_and_the_per_container_counts` | the `collections` map and both `collection_status` records agree with the catalog, `geometry_containers` renders the per-container counts, no synthetic origin appears, and the report is byte-stable for the same installation | a count is missing, the two renderings disagree, or the report is not deterministic |
| `a_world_group_without_a_geometry_container_is_named_not_guessed` | a discovered group that stores no `gamez.zbd` produces no row and is named in the diagnostic; no container is claimed to have been read | a row is minted from a directory's name |
| `retail_every_gamez_container_yields_its_nodes_and_named_meshes` (retail) | the nine containers of `cs_assets::install::Diagnosis`; the 56 620 and 17 139 corpus totals; every container's `named + ambiguous + unspellable + unterminated == nodes` and `mesh_rows + absent_meshes == named_meshes`; `unterminated == 0` and `unterminated_parent_chain == 0`; every row's span re-derived from the production readers as an exact record; no two records sharing an address; every edge of every row resolving in the catalog; the install fingerprint and the container digest on every row; exactly a fallback-keyed row carrying an identity unknown; the frozen F50 denominator | a container is missed, a corpus total moves, a span is not a record, a row loses its digest, an edge points at nothing, or the denominator moves |

**Sensitivity check.** Seven mutations applied and reverted, each killed by a
test CI runs (`cargo test --locked -p cs_content --test
accept_f14_d_4_geometry_collections`, no original data):

| mutation | synthetic tests it fails |
| --- | --- |
| drop the geometry collections from `retail_baseline` | 6 |
| key an ambiguous or unspellable node by its authored path instead of its record address | 7 |
| drop the explicit unknown from a fallback-keyed row | 2 |
| give a node with a stored `mesh_index` of `-1` a mesh edge anyway | 1 |
| read the name path through the child lists instead of the parent slots | 2 |
| compare a different pair of fields in the two-walk cross-check | 7 |
| report the container counts as the number of records pushed rather than the number that read | 1 |
| give a node an edge onto a named mesh slot no present record answers (reviewer pass) | 2 |
| drop the parent-chain termination measurement (reviewer pass) | 1 |
| restore the previous bound (a budget decremented on every iteration, including the one that ends the walk), so a chain holding as many names as the array holds nodes runs the budget to zero and looks like a loop (third review pass) | 1 |

The last three are the reviewer's own mutations; each is recorded in a review
section below with the correction that killed it.

The last two exist because the first pass of this suite let both mutations
through, and that is worth recording rather than hiding: the fixture originally
wrote **no** child lists at all, so the parent-slot choice was unfalsifiable in
CI, and the cross-check had no test pinning which fields it compares. The fixture
now writes every node's child list derived from the parent slots *and* makes them
disagree for one node on purpose (`gun` stores `main` as its parent while `main`'s
child list omits it — the eight world containers' own disagreement), and
`accept_f14_d_4_the_two_walks_meet_at_the_headers_own_node_offset` pins the three
equalities through the production readers.

**The cross-check is redundant with the two readers' own boundary checks.** Neither
`read_gamez_meshes` nor `read_gamez_nodes` hands back a container whose boundary
disagrees with the header, so on today's code the baseline's comparison can never
fire. It is kept because it states the one boundary both readers describe and it
names all four numbers if either reader's own check is ever relaxed — and because
the test above pins which fields it compares, so the check cannot silently become a
comparison of something else.

## Unknowns and limitations (all recorded, none guessed)

- **The authored name path does not identify a node in this corpus.** `world1.g0`
  is stored 49 times in `ZBD/C1B`, `world1.g27816` 34 times in `ZBD/C2B`, and
  `river1.g4.bush1.flt` 6 times in `ZBD/C1`. **Affected content:** 10 235 nodes of
  the nine containers, and therefore every name-based binding that would address
  them — F11-C's socket bindings, F11-D's airframe roster, F11-B's ECS import.
  **Resolving this needs an owner ruling on an identity scheme that tolerates a
  repeated authored name**; the rows are already in place to carry the answer, and
  a follow-up task is filed: **#560 (F14-D.11)**.
- **3 488 name paths form no valid key.** Some carry a byte the grammar refuses
  (`brigturret ` with a trailing space; `z:\crimsonrun\data\common\vessels\` in
  three world containers) and some exceed `MAX_CONTENT_KEY_LEN` once the
  container prefix is added (`world1.cargozep2.…healthy.turret.gun.firepoint` in
  `C1`, `C2` and `C3`). **Affected content:** the same stages as above. The row
  states which of the two happened, so a stage that shortens the key (an owner
  decision, not this baseline's) can find every affected node by claim id.
- **`node_index` is not used as identity.** Its low three bytes are not unique in
  the corpus (up to 1 031 repeats in `C5`) and the reference reserves
  `0x00FFFFFF`, so the reader carries it raw and this collection addresses nodes
  by their record and their name path only.
- **The mesh-array slot is the mesh's only identity.** The store keeps no name for
  a mesh, so `<container>.<slot>` is the whole of what the bytes hold. Which
  airframe part a slot draws is F11-C's binding work, not this collection's.
- **A node's mesh slot is not range-checked by the node reader** (an unknown
  already recorded by F11-A). The baseline closes that gap for itself: it reads
  the mesh section and counts a named slot with no present record as
  `named_slot_without_mesh` rather than minting a row. On this installation the
  count is 0.
- **The world containers' child lists do not cover every node that names a
  parent** (346 of 7 064 in `C1`), already recorded by F11-A. This collection
  walks the parent slots, so it is unaffected — but `SceneGraph::build` still
  refuses all eight world containers, and F11-B/C/D cannot yet consume these rows
  end to end.
- **No normalized field.** The stored transform, LOD bounds and zone id are in
  source units; the CS `SourceAdapter` is F16's work. Affected content: every one
  of the 73 759 geometry rows, all of them `unavailable`.
- **The rendered size of the report is ~64 MB.** `cs-inspect catalog` writes it to
  `--out`, and building the baseline over the retail installation takes about
  7 s in release. Both are recorded here rather than left for a consumer to find.
- **Evidence class.** The node and mesh layouts are documented in a pinned
  third-party tool (mech3ax v0.6.0, commit `d3521a9721be731d365504568ddcd78e3f9846bb`)
  and were reproduced from the original bytes by the readers F10 and F11 across
  all nine archives, which is `ObservedTool`. No original run happened: `retail`
  is file access, not evidence of runtime behaviour, and neither the identity
  scheme nor any field's meaning was measured against the game.
  **A different agent instance or model with a fresh context should review this
  format and identity work**, and no agent review replaces the owner's approval.
- **Materials, images and collision surfaces are not in this slice**, and each is
  filed rather than guessed at. **`material`** (#558, F14-D.9):
  `cs_formats::gamez::materials` reads the section, but `RawMaterial` carries **no
  byte offset**, so a row would have no checked span — re-deriving
  `materials_offset + index * 44` in the baseline would duplicate the reader's own
  layout arithmetic and drift from it silently. **`image`** (#559, F14-D.10): a
  GameZ container stores a texture *name*; the bytes live in `texture.zbd` and the
  `rtexture*.zbd` family, so an image row needs F08's cross-archive resolution and
  naming a texture is not reading an image. **`collision_surface`** (#561,
  F14-D.12): nothing the layout this workspace reads marks a surface as a collision
  surface — `CollisionRole` is designed vocabulary with no measured mapping — so a
  row would assert a mapping nobody measured.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, offsets and ranges; no node name, mesh, material or screenshot from the
  installation is in the repository.

## Evidence

`docs/findings/evidence/F14-D.4.json`, produced by
`crates/cs_content/tests/evidence_report_f14_d_4.rs` and checked with
`python3 tools/validate_evidence.py private/evidence/F14-D.4/acceptance.json --artifact-root private/evidence/F14-D.4 --require-pass`
(`{"structurally_valid": true, "artifact_count": 2}`). It records the eleven
`accept_f14_d_4_` tests that ran (all passing, the retail one included), the
installation and content digests measured by production discovery, and the
consumer report as a hashed artifact.

**A tools-test failure that this stage did not cause, and that main has since
fixed.** On the branch as submitted, `python3 -m unittest discover -s
tools/tests -p 'test_evidence_review_identity.py'` reported 2 failures, and the
reviewer reproduced exactly those two in a clean worktree at `db047e41`
(`origin/main` at the time) with this branch's files absent:
`test_accept_m16_a_fu4_the_reader_covers_the_whole_family` and
`test_accept_m16_a_fu4_a_runtime_identity_harness_is_exempt_and_pinned` pinned a
`runtime`-shaped harness set that committed harnesses had outgrown — filed as
**#562 (TOOLS-EVID)** rather than fixed here. The rebase onto the current
`origin/main` brought #562's fix with it (`ab45cb88`, `eb8438e9`), so on the
reviewed branch the whole selection passes (20 tests, exit 0) and this note no
longer claims a pre-existing failure. The identity cross-check this stage depends
on — that the harness's literal equals the committed report's
`review.identity`, and that the claim stays `implemented` — passes for
`F14-D.4`.


## Review additions to this stage (same task, reviewer pass)

Reviewer: `bunny-alpha-1/bunny-alpha-1` (Rally #487, review claim of
2026-10-03T05:12:04Z) — the **same agent instance** that implemented the stage,
so this is **not independent review** and awards nothing above `checked`. The
session that ran it was fresh (it read only the branch, the specs, this note and
the recorded handover summary; it carries no memory of writing the code), but a
different agent instance or model should still re-examine the format and identity
claims, as `AGENTS.md` asks. Eight corrections were made while reviewing; none
changes a measured count.

1. **A dangling mesh edge, and the claim that hid it.** A node whose stored
   `mesh_index` named a slot with no present mesh record still received a
   `Static` edge onto `mesh/<container>.<slot>` — an id no row ever holds. The
   collection's own fixture builds exactly that case (`ghost` names the absent
   slot 2) and blessed it, and the suite's
   `coverage.unresolved_references == 0` assertions could not see it: the closure
   only walks **from the declared roots**, and no geometry row is reachable yet,
   so "the baseline builds none" was unfalsifiable for these two collections. The
   edge is now emitted only for a slot the container answers, and every node that
   names an absent one carries `f14.d.4.baseline.node_mesh_slot_absent` naming the
   slot instead. Both the synthetic and the retail suites now walk **every** edge
   of both collections directly and require the catalog to hold its target; the
   coverage count is kept as the global accounting, with a comment saying why it
   cannot see this. Mutation check: restoring the old edge fails 2 synthetic
   tests.
2. **A bounded walk is not a terminated one.** `node_name_paths` bounded a parent
   chain with `record count + 1` steps, which is correct for a forest and means
   nothing for a cycle: a node that is its own ancestor got a name path made of
   the part of the loop that fitted, and the row was published under it as
   though the store had named it. A cycle is now **measured**: the walk reports
   the nodes whose chain hit the bound, those rows are keyed by their record
   address with `f14.d.4.baseline.node_path_unterminated`, and the count appears
   per container (`unterminated`) and in the collection record
   (`unterminated_parent_chain`). On this installation the count is **0** in all
   nine containers, which the retail test now pins — so the corpus numbers
   above are unchanged, and the "every name path names a real hierarchy" claim is
   measured instead of assumed. New test:
   `accept_f14_d_4_a_node_whose_parent_chain_never_ends_is_a_row_with_an_explicit_unknown`
   (mutation check: dropping the detection fails exactly that test).
3. **The inventory lookup was case-folded and separator-guessed.** The walk
   looked each container up with `asset.to_ascii_lowercase()` although the
   inventory is keyed by `RelativePath::logical_key()`; an installation whose
   manifest spells a path with `\` would have missed its own file and reported a
   gap. `geometry_sources` now returns logical keys, which is what
   `retail_baseline` keys by.
4. **A guard that could silently skip the shared container.** `geometry_sources`
   compared `planes_zbd.logical_key()` against a second, privately spelled copy
   of `zbd/planes.zbd` and skipped the container when they differed. Since
   `Diagnosis::planes_zbd` is only ever `Some` when discovery found that key, the
   guard can never fire — but if `cs_assets` ever changed the literal, the
   baseline would have stopped reading `ZBD/planes.zbd` with nothing reported.
   The guard and the duplicate constant are gone.
5. **`CollectionStatus::source` named a bare file, not the pattern.** The struct
   documents that a collection with one such file *per row* names the pattern its
   rows follow — which is why the sibling world collection publishes
   `WORLD_READER_PATTERN`. The geometry records published `gamez.zbd`, which is
   not an installation-relative path at all. They now publish
   `GEOMETRY_CONTAINER_PATTERN` (`ZBD/<world group>/gamez.zbd and
   ZBD/planes.zbd`), with the nine real spellings in `geometry_containers`.
6. **An error variant nothing could produce.** `BaselineError::UninventoriedContainer`
   was documented in `# Errors` and matched in two `match` arms, but no code path
   constructed it: a group with no `gamez.zbd` is an expected state that becomes
   a named diagnostic (that is the behaviour the suite tests), so the variant was
   documentation of a path that does not exist. Removed.
7. **Two defects from the rebase.** The module documentation had a duplicated,
   ungrammatical paragraph — a conflict resolution had spliced the F14-D.3
   sentence onto the F14-D.4 one, leaving a fragment starting "use, the file
   inventory is …" — and the bullet list was split by a stray blank `//!` line.
   Both repaired; the paragraphs now read as one derivation list. The same
   mangled fragment was **already on `origin/main`**, introduced by an earlier
   stage's own rebase, so this fixes text two stages touched; a second conflict
   with F14-D.5's faction and paint-mask collections was resolved by keeping
   both sides, and `git diff origin/main...HEAD` shows this branch adds its own
   collections without removing any of F14-D.5's.
8. **A wrong number in this note.** The gap section said "46 MB of GameZ
   container across nine archives"; the nine files measure 52 386 080 bytes
   (≈50 MiB), which is what it now says.

Two smaller corrections in the suites: a leftover `println!("DIAG …")` debug
loop was removed from the first test, and the retail test's "no two records share
an address" check compared `(offset, length)` **pairs**, so two records sharing a
start with different lengths would have passed — it now compares the addresses on
their own as well.

`docs/findings/evidence/F14-D.4.json` was regenerated on the reviewed commit (11
tests, `retail` capability, both artifacts re-hashed) and its `review.identity`
now names the reviewer, as `tools/tests/test_evidence_review_identity.py`
requires. The reviewer ran the harness without `CS_EVIDENCE_REVIEW` and replaced
the harness's literal with the reviewer's own text in the same commit, so the
report and the harness that writes it cannot drift apart.

### The resumed pass: the rebase the lander could not do

The approved commit could not be landed — the lander reported a rebase conflict
with main and handed the task back for a manual rebase — so the review ran a
second time (review claim of 2026-10-03T06:44:27Z, same agent instance, same
caveat: **not independent**). Three of the ten commits conflicted, all with the
F14-D.6 airframe collection, because main had added its rows to the same
`retail_baseline`, the same error enum, the same `use` block and the same module
documentation this stage edits. Each conflict was resolved by keeping both sides,
and the result was checked rather than assumed: the set of items the merged file
declares is exactly main's set plus this stage's, with nothing of main's removed.

That resolution carried four defects of its own, all now fixed:

9. **The inventory's completeness test could no longer account for this stage's
   rows.** F14-D.6 hardened `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
   so that every non-launchable row has to belong to a collection its total
   names — mode rows, world rows, airframe rows, faction rows, paint-mask rows.
   It named none of this stage's two, so the equality read 73 977 against 218 and
   the retail check **failed**. The fix derives the `scene_node` and `mesh` counts
   from the rows like every other collection in that total (no number is
   restated here) and names them in the message; the test passes again. This is
   the check that keeps a later collection honest, and it would have caught this
   stage if it had been written to see it.
10. **The walk's list of derivations was one short.** After the conflict
    resolution `retail_baseline` still said it "reads the installation seven
    ways" while the merged function reads eight — main's airframe roster
    discovery was missing from the list. Corrected, with the airframe container
    named.
11. **The module documentation had two orphaned fragments.** The old duplicated
    paragraph reappeared in a third shape (a fragment beginning "use, the file
    inventory is …" stranded after a paragraph that had already been repaired),
    and the error enum kept a documented variant nothing could construct. Both
    cleaned up; the enum now holds main's two airframe variants and nothing that
    no code path builds.
12. **This note's own totals were stale.** It reported 74 097 rows and did not
    mention the four collections that landed on main after the numbers were
    taken. Re-measured on the reviewed tree and restated above (74 311 in twelve).

Because the merge touched the same file as F14-D.6's completeness check, this
pass ran the **full** four checks plus every F14-D suite with `--include-ignored`,
not the lighter rebase set the 2026-10-01 owner directive allows: the retail
halves of F14-D.2, D.3, D.5 and D.6 all build the whole baseline and all pass
(5, 5, 7 and 8 tests).

### The third pass: the lander could not rebase this one either

The second approved commit failed to land for the same reason (review claim of
2026-10-03T08:54:34Z, same agent instance, same caveat: **not independent**).
Main had meanwhile landed F14-D.7's sound-cue collection, which adds its rows
inside the same `retail_baseline`, the same `use` block, the same module
documentation and the same test file. Two of the fifteen commits conflicted,
both in `baseline.rs`; each was resolved by keeping both sides, and the result
was checked the same way as before (the declared-item set of the merged file is
exactly main's set plus this stage's). Three defects were found and fixed:

13. **A chain as long as the node array was reported as a cycle.** The walk that
    derives a node's authored name path bounded a parent chain by the record
    count **plus one** and reported "unterminated" when that budget ran out. A
    cycle-free chain visits each stored node at most once, so it can hold
    exactly as many names as the array holds nodes: a hierarchy that is one chain
    covering every node spent the whole budget on its way to a root and was
    called a loop. Such a row lost its authored name path, was keyed by its
    record address instead, and carried a `node_path_unterminated` unknown that
    was **false about the container** — the one claim in this stage that
    contradicts the store. The bound now counts the names a chain has consumed
    and stops when a chain would need one more than the array holds, which is
    what a cycle looks like. `accept_f14_d_4_a_chain_as_long_as_the_node_array_is_not_a_cycle`
    writes such a container (three nodes, one chain, the deepest chain three
    names long) and pins the deepest node's path, its parent edge and
    `unterminated == 0`; it fails against the previous bound and passes against
    this one. The corpus is unaffected: the retail test still measures 0
    unterminated chains in all nine containers, because no chain there is as long
    as its array.
14. **The inventory's completeness test could not account for the sound rows.**
    The same merge exposed the identical failure the previous pass fixed for this
    stage's own two collections, one stage later: F14-D.7 inserted 4 951
    `ContentKind::Sound` rows without naming that collection in
    `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`, so
    the equality read 78 928 against 73 977 and the retail check **failed**.
    **This is a pre-existing defect on `main`, not a regression of this stage**:
    on `main` the same equality reads 5 169 against 218 (the sound rows minus
    this stage's rows are the difference). The accounting now names the sound
    collection the same way it names every other non-launchable one — a count
    derived from the rows, no number restated — and the test passes. The root
    cause belongs to F14-D.7 and is filed as its own task; a shared completeness
    total that every new collection must remember to update is what both passes
    tripped over.
15. **The list of derivations lost the sound family.** After the merge
    `retail_baseline` still claimed "reads the installation eight ways" and did
    not mention the sound containers at all, so the function's own account of
    itself was wrong in both the count and the list. Corrected to ten ways with
    the sound family named.

Because the merge touched the same file as F14-D.7's collection and its
completeness check, this pass also ran the **full** four checks plus every F14-D
suite with `--include-ignored`, including F14-D.7's own retail suite. The corpus
measurements are unchanged: 56 620 scene node rows, 17 139 mesh rows, nine
containers read, 0 refused, 0 unterminated chains.


## Sources used

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (stage
  `### F14-D`, non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (the required collections, the catalog
  element, "collections cannot exclude failed entries", stable ids from a semantic
  source key, ownership cycles).
- `docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json` (the
  evidence record, the task-prefix discovery rule, `checked` as the agent ceiling).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (the two-pass layout, the
  per-archive corpus, the partial child lists, the unspellable `brigturret ` name).
- `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` and
  `crates/cs_formats/src/gamez/reader.rs` (the 17 139 present mesh records, the
  mesh index's own assertions, the `not_listed` family classification F06 reports).
- `crates/cs_app/src/world/audit.rs` (`GEOMETRY_CONTAINER_FILE` as a measured
  fact, the one-parse-context rule, the header cross-check this baseline repeats).
- `crates/cs_content/src/{scene,mesh}.rs` (`SceneNodeId::for_path` and
  `MeshSlot`'s `<container>.<slot>` key, the `ParsedNode` conversion).
- `docs/findings/2026-10-02-f14-d-2-multiplayer-rules-collection.md` (the
  `CollectionStatus` shape this stage follows, and the two defects its review
  recorded).