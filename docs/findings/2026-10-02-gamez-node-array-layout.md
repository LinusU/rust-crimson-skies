# The CS GameZ node array: layout, measurement, and what it blocks

Date: 2026-10-02. Task: #392 "Read the GameZ node array into `ParsedNode`
records". Feature sheet: `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-
lod.md` (the F11-A input contract this completes, and the F11-B limitation it
resolves). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities
used: **`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. `gpu` and `audio` were available and **not used**: nothing is
rendered or played, and no original run happened, so nothing here claims
`verified_original`.

This closes the blocker F11-B recorded and that F11-C inherited: the
`crates/cs_formats` GameZ reader stopped at `GameZHeader::nodes_offset`, so
there was no way to produce a `cs_content::scene::ParsedNode` from original
bytes and every original hierarchy was unreachable.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/gamez/nodes.rs` (new, the layout and its reader):
  the size constants (`NODE_INFO_BYTES` 208, `NODE_INDEX_BYTES` 4,
  `NODE_SLOT_BYTES` 212, `NODE_NAME_BYTES` 36, `OBJECT3D_DATA_BYTES` 144,
  `LOD_DATA_BYTES` 92, `WORLD_DATA_BYTES` 204, `WORLD_PARTITION_BYTES` 88,
  `WORLD_PARTITION_VALUE_BYTES` 12, `WINDOW_DATA_BYTES` 248,
  `DISPLAY_DATA_BYTES` 28, `CAMERA_DATA_BYTES` 488, `LIGHT_DATA_BYTES` 256),
  the `NODE_TYPE_*` tags and the `node_index` masks, `MATRIX_AGREEMENT_TOLERANCE`,
  `RawNodeInfo` (the whole 208-byte record), `RawObject3dData`, `RawLodData`,
  `RawWorldData`, `NodeKind`, `RawNode`, `NodeFinding`, `MeshIndexBounds`,
  `GameZNodes`, `GameZNodeError` and `read_gamez_nodes`.
- `crates/cs_formats/src/gamez/mod.rs` (wiring only): `pub mod nodes;` plus the
  re-exports and a module-doc paragraph.
- `crates/cs_content/src/scene.rs` (extend, an F11 owner path): `MeshSlot`,
  `GameZSceneError`, `parsed_nodes_from_gamez`, `scene_graph_from_gamez` and the
  two private helpers `authored_transform` and `parsed_kind`.
- `crates/cs_content/tests/scene.rs` (extend, an F11 owner path): the
  `accept_t392_*` tests and the synthetic container writer.
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (this file).

**One observable failure:** the layout is two passes over two sections, and the
check that makes it falsifiable is that the data section must end **exactly** at
the container's end. A reader that treated the node array as one flat run of
212-byte records, that followed the stored `data_ptr` instead of walking
sequentially, that read a light node's parent word conditionally, or that
mis-sized the object's trailing 48 zero bytes all stop at some other offset. In
the real corpus that lands between 4 and 48 bytes off, and the reader refuses
with `GameZNodeError::DataEnd` or `DataOffset` naming both numbers. The retail
test asserts the same three equalities on `planes.zbd`
(`5 584 432 + 212 × 3 317 == data_offset` and `data_end == 6 083 868`), so a
mis-sized record cannot pass.

## Design decisions

- **The node array is not `nodes_offset .. nodes_offset + node_array_size`.**
  The task description's phrasing is a *count*, not a byte length, and reading it
  as a length is the first thing a reader gets wrong. The array is
  `node_array_size` × (208-byte info record + 4-byte `node_index` word),
  interleaved, immediately followed by a **variable-length** data section holding
  one kind-specific record per node in node order. The task description also says
  the array runs to `nodes_offset + node_array_size`; that is not a byte extent
  and nothing in the corpus supports it, so this file records the correction
  rather than implementing the description's arithmetic.
- **The stored `data_ptr` is a cross-check, not an address.** The reference reads
  the data section sequentially and carries `data_ptr` as an opaque `Ptr`; this
  reader does the same and *checks* that the pointer equals the offset the walk
  reached (`DataOffset`). This keeps one way to address the bytes instead of two
  that can disagree, and it is the check that proves no record was skipped. In
  all nine archives every stored pointer matches the walk, for every node.
- **A light node's parent word is read, but it is not a parent link.** The
  reference reads it unconditionally ("read as a result of `parent_count`, but is
  always 0"). The word is consumed so the walk stays in step, and it is
  range-checked, but `RawNode::parent` follows the record's own `parent_count`
  boolean, which is what the reference's own model uses. A LOD node is different:
  its parent is declared (`lod` asserts `has_parent == true`) and the word is its
  parent.
- **A node's name is the bytes before the first `NUL`, and nothing else.** The
  36-byte field is a write buffer, not a C string: in all nine archives the bytes
  after the terminator are a shifted remainder of the reference's
  `Default_node_name` buffer (`brigturret2 \0name\0…`, `rung6\0t_node_…`). The
  reference's own two hard-coded special cases (`geometry`, `cockpit1`) are
  special cases *because* of this. A reader that validated the padding would
  refuse every real record, so this one requires ASCII in the prefix and nothing
  more. A name the id grammar will later refuse crosses over **unchanged**;
  transliterating it in the reader would invent an identity the store never had.
- **A mesh slot the caller's catalog cannot answer is an explicit unknown, not a
  refusal.** The association and its stored index are kept and the resolution
  becomes `Resolved::Unknown` under `f11-node-array.mesh-slot-unresolved`, with
  the index and the slot count in the reason. Refusing the whole record would
  throw away an exact hierarchy because one catalog row is missing, and
  `MeshBinding` is defined to resolve *or* record unresolved.
- **`SceneGraph::build` stays the only place a hierarchy is judged.** The three
  steps are separate types: the store's `RawNode`, this crate's `ParsedNode`, and
  the canonical `SceneGraph`. A container whose records read but whose hierarchy
  does not convert is reported as exactly that, via
  `GameZSceneError::Build(SceneError)`. That is why the retail test can assert
  both "the records convert" and "the build refuses, here is why".
- **The unmeasured words stay at record level.** `RawNodeInfo` is the whole
  208-byte record, `unk040`/`unk044`, `environment_data`, `action_priority`,
  `action_callback`, `area_partition`, `unk112`, the three bounding boxes,
  `unk196` and the words the reference asserts are zero all included, and
  `RawNode::data_offset` + `data_bytes` address the node's own record so a later
  stage re-derives the world's fog and partitions, the camera's FOV and the
  light's range from the bytes rather than from a guess. F11-A's finding had said
  the F11-B reader "must keep them at record level or re-derive them if a later
  stage needs them"; this is the first half of that, and the addressability is
  what makes the second half possible without reopening this file.
- **The world's record is the only variable-length one, and only its two grid
  counts are read.** Everything else in it is F18's subject. The grid size is
  `partition_x_count × partition_y_count` cells of 88 bytes, each followed by its
  own `count` × 12-byte values; the count sits at offset 58 of the cell, not at
  its start, and the walk ends exactly on the container's end in all nine
  archives — which is what proves the arithmetic.
- **A finding is not an error.** Eleven `NodeFinding` variants report what the
  reference asserts against while the record is still read with everything it
  stores, following the mesh reader's `ParseFinding` precedent. A container whose
  findings list is empty is the strongest claim this reader makes about it.

## Measurement: all nine GameZ archives

Every number below is from the production reader run over the read-only
installation. `data` is the data section's byte range; it starts where the info
array ends and ends at the container's end, in **every** archive. The reader also
verified, per archive, that all `node_array_size` stored `data_ptr` values equal
the offsets the walk reached, and that every `node_index` word's top byte is
`0x02000000`, `environment_data` is 0 and `action_priority` is 1.

| archive | bytes | nodes | kinds | data section | object 32/40 | stored-matrix disagreement | LOD | mesh-bound nodes | mesh index range |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `ZBD/planes.zbd` | 6 083 868 | 3 317 | 3 230 object, 87 LOD | 5 584 432..6 083 868 | 1 522 / 1 708 | 107 | 87 | 1 766 | 0..1 778 |
| `ZBD/C1/gamez.zbd` | 6 875 076 | 7 064 | 6 334 object, 723 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 5 823 864..6 875 076 | 2 659 / 3 675 | 5 | 723 | 3 966 | 0..2 236 |
| `ZBD/C1B/gamez.zbd` | 3 953 304 | 5 603 | 5 134 object, 462 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 3 111 984..3 953 304 | 2 153 / 2 981 | 1 | 462 | 3 485 | 0..1 304 |
| `ZBD/C1C/gamez.zbd` | 4 008 612 | 5 644 | 5 151 object, 486 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 3 161 212..4 008 612 | 1 369 / 3 782 | 1 | 486 | 3 354 | 0..1 517 |
| `ZBD/C2/gamez.zbd` | 4 908 288 | 4 956 | 4 528 object, 421 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 4 162 500..4 908 288 | 1 694 / 2 834 | 1 | 421 | 2 558 | 0..1 764 |
| `ZBD/C2B/gamez.zbd` | 3 435 348 | 4 901 | 4 469 object, 425 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 2 697 712..3 435 348 | 1 087 / 3 382 | 1 | 425 | 3 039 | 0..1 364 |
| `ZBD/C3/gamez.zbd` | 5 633 696 | 5 408 | 4 972 object, 429 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 4 808 244..5 633 696 | 1 818 / 3 154 | 18 | 429 | 2 868 | 0..1 900 |
| `ZBD/C4/gamez.zbd` | 8 102 080 | 8 289 | 7 563 object, 719 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 6 864 412..8 102 080 | 3 657 / 3 906 | 11 | 719 | 4 929 | 0..2 489 |
| `ZBD/C5/gamez.zbd` | 9 385 808 | 11 438 | 10 230 object, 1 201 LOD, 1 world, 2 window, 2 camera, 1 display, 1 light | 7 684 148..9 385 808 | 4 104 / 6 126 | 3 | 1 201 | 6 003 | 0..2 850 |

Totals: **56 620 node records** — 51 611 object, 4 953 LOD, 8 world, 16 window,
16 camera, 8 display, 8 light. (The six singleton kinds occur once per **world**
container, and there are eight of them: `planes.zbd` holds neither a world node
nor a window, camera, display or light one.) Every `flags == 40` object record (31 548 of
them) is exactly the identity transform; every one of the 20 063 `flags == 32`
records has `scale == 1.0` exactly; **148** stored matrices disagree with the
matrix their own euler triple derives, which is the disagreement the reference
documents at ~0.74 % for the archives it names and 0.26 % measured over all nine.

Other measured facts, all inside the reference's asserted profile:

- `unk040` is 0 in `planes.zbd` and takes a small measured set of values in the
  world containers; `unk044` is 1 in almost every record. Neither is interpreted.
- `zone_id` takes 255, 1 and 2 — the three values the reference asserts, plus
  255 as "no zone". Carried raw.
- `unk196` is 160 for every object and LOD record and 0 for the seven
  world/display/window/camera/light records of each world container. The
  reference asserts exactly that, so no finding fires.
- `parent_count` is 0 or 1 everywhere.
- LOD `range_far_sq` equals `range_far²` in all 4 953 records; `range_near_sq` is
  never negative. Near distances run 0..2 050 source units, far 50..10 000 000.
- The `node_index` word's low three bytes are **not** unique: 120 values repeat
  in `planes.zbd` and up to 1 031 in `C5`, and one record per archive carries the
  reference's `0x00FFFFFF` "invalid" value. This word is carried raw and nothing
  addresses by it; the array slot is the identity the hierarchy uses.

## The hierarchy in `planes.zbd`, and the one refusal

`planes.zbd` is the shared aircraft-geometry container, and its node array is a
**strict forest**: 28 roots, 3 289 child slots, and every link agrees in both
directions — each child's stored parent names the node that lists it, and no node
is listed by two parents. No cycle, no dangling link, and every node is reachable
from a root. 1 766 of the 3 317 nodes name a mesh; the other 1 551 store `-1`.
The first records are `wf2test` (a root), `player_bhawk` (a root),
`rung6`, `rt_elev`, `r_aileron1`, `gear`; `geometry` and `cockpit1` appear
repeatedly, which is why the two hard-coded name special cases in the reference
exist.

`parsed_nodes_from_gamez` converts all 3 317 records. **`SceneGraph::build`
refuses the container**, and the reason is a fact about the data meeting F11-A's
id scheme, not about this reader: node 640's stored name is
`b"brigturret2 \0name\0…"` — a trailing space before the terminator — and
`ContentId`'s key grammar allows only `[a-z0-9._-]`. Six nodes are affected (640
and its five descendants, whose name-paths inherit the space), and it is the
first such node the build reaches. This is **not worked around here**: the name
crosses over verbatim and the build's typed refusal is the report. F11-A's rule
is that ambiguity is never resolved by position, and the same argument says a
name is never silently transliterated.

The world containers are further from convertible, and for a different reason:
their child lists do not cover every node that names a parent. In `C1` 346 of
7 064 nodes are absent from their parent's list, so `build` refuses with
`InconsistentParentage` before any name is reached. In `planes.zbd` that count is
**0**. Whether the world containers' child lists are a partial index or the
parent slots are authoritative is unmeasured and is recorded as an unknown
below.

## Test inventory

| `accept_t392_*` test | Covers | Fails when |
| --- | --- | --- |
| `node_array_decodes_every_stored_field_into_its_own_slot` | the two passes tile the container; names, flags, zones, the `node_index` word, the hierarchy in both directions, an object transform, a LOD near/far pair, the mesh bounds, and `data_offset`/`data_bytes` addressing | a field lands in the wrong slot, the stride is wrong, a LOD near bound is not resolved from its stored square, or a record's bytes stop being addressable |
| `typed_records_keep_the_stored_transform_and_resolve_meshes` | stored-matrix precedence where the two disagree and its absence where they agree; the identity for a `flags == 40` record; a mesh slot resolved with the catalog's own provenance; a slot past the catalog as an explicit unknown naming the index and the count | the euler triple overrides a disagreeing stored matrix, a resolved binding loses its provenance, or an unresolved slot is given an id |
| `a_mesh_index_past_the_catalog_stays_an_explicit_unknown` | the association and its index survive an unresolvable slot; a catalog element in the wrong namespace is refused at construction | the record is refused for a missing catalog row, or a non-`mesh` element is accepted |
| `scene_graph_is_built_from_a_decoded_node_array` | a decoded forest becomes a real `SceneGraph` with a name-path-derived id, a preserved mesh association, the shared visual/collision transform, a hand-computed composition under a 90° yaw, a LOD pair whose range converts to metres and drives the selection rule, a detached cycle, a rootless forest and an unspellable name — each as its own typed refusal | the id derivation, the composition order, the unit conversion, the LOD range or any of the four refusals changes |
| `a_broken_node_array_is_refused_with_its_own_reason` | a header that is not a GameZ container, a `nodes_offset` with no room, an undefined kind tag, a `data_ptr` off the walk, an out-of-range parent slot, an out-of-range child slot, a world cell whose value count does not fit, a world grid that does not fit, a grid whose product overflows, a truncated info array, trailing bytes the walk does not account for, a name with no terminator, and an empty declared array | any of the thirteen becomes a silent truncation, a wrong answer or the wrong error variant |
| `every_node_kind_keeps_the_data_walk_in_step` | one container holding all seven kinds at once: the world's 204-byte record, its child-value word and a 2×2 grid of cells that carry their own values; the window, camera, display and light record sizes; and a light node whose parent word is consumed although the record declares no parent | any of those six record lengths is wrong, the world's child-value word is skipped, the per-cell value bytes are not sized from the cell's own count, or the light's parent word stops being unconditional |
| `an_unknown_node_kind_is_refused_by_its_tag` | five undefined tags, each refused by the tag itself with the refusal anchored at the `node_type` word it read | an unknown tag is accepted, or its refusal points somewhere other than the word |
| `records_outside_the_asserted_profile_are_reported_not_dropped` | all eleven `NodeFinding` variants fire with their own codes and node indices while every record is still read; a record flagged as holding no transform that stores a translation keeps its own words; a negative LOD near bound is refused by the conversion | a finding is promoted to an error, a record is dropped, a flagged-but-not-identity record is silently relabelled as the identity, or the conversion turns a negative square into a NaN distance |
| `retail_planes_node_array_decodes_and_its_conversion_verdict_is_typed` (retail) | the real `planes.zbd`: the reference's recorded header words, the two passes tiling the container, the 3 230/87 kind split, the strict forest with 3 289 agreeing links, the mesh bounds, the 107 stored-matrix disagreements and no other finding, 3 317 converted records, and the typed `NodeId` refusal on node 640 with exactly six affected nodes | the layout mis-walks, the forest is not strict, the finding counts move, or the refusal is worked around |
| `retail_every_gamez_archive_walks_to_its_container_end` (retail) | **all nine** archives, not one: each one's stored record count, its info array starting on the header's own `nodes_offset`, its per-archive disagreement count with no other finding, and its data section ending exactly on its container's last byte; plus the corpus totals (56 620 records, 148 disagreements, the per-kind split) | any archive mis-walks, its disagreement count moves, a second deviation from the reference's profile appears, or the per-kind totals stop matching the measurement |

**Sensitivity check.** Mutations applied and reverted. The implementer's own:
reading the object record as 96 bytes instead of 144
(`node_array_decodes_every_stored_field_into_its_own_slot`, on `data_bytes`);
reading the LOD record without its trailing 12 bytes (the same test, on the LOD
pair's offsets); reading the light node's parent word conditionally (the retail
test, on `data_end`); reading the world's grid counts from the wrong header
offset; reading the partition cell's `count` from the cell's first word instead
of offset 58; following `data_ptr` instead of walking (`DataOffset` fires on
node 1 of `planes.zbd`); and clamping an out-of-range mesh index instead of
leaving it unknown
(`a_mesh_index_past_the_catalog_stays_an_explicit_unknown`).

The review added sixteen more, and every one is now killed by a test **CI can
run** (the two retail tests are `#[ignore]`d and are not):

| mutation | killed by |
| --- | --- |
| the light node's parent word read conditionally | `every_node_kind_keeps_the_data_walk_in_step` (before the review only the retail test caught it) |
| the world's grid counts read 8 bytes early | `every_node_kind_keeps_the_data_walk_in_step`, `a_broken_node_array_…` |
| the world's child-value word not consumed | four tests |
| the partition cell sized at 80 bytes | `every_node_kind_keeps_the_data_walk_in_step`, `records_outside_…` |
| the partition cell's `count` read at the cell's first word | `every_node_kind_keeps_the_data_walk_in_step` |
| the camera record at 256 bytes, the window at 128, the display at 64 | `every_node_kind_keeps_the_data_walk_in_step` |
| the world's own children-count check dropped | `records_outside_…` (before the review nothing caught it) |
| each of the object-flags, object-identity, LOD-level, LOD-far-square, LOD-near-square, `unk196`, `parent_count` and `mesh_index`-sentinel findings suppressed | `records_outside_the_asserted_profile_are_reported_not_dropped` |
| the stored matrix dropped even where it disagrees | `typed_records_keep_the_stored_transform_and_resolve_meshes` |
| a `MeshKind` refused at construction reporting a node | `a_mesh_index_past_the_catalog_stays_an_explicit_unknown` |
| the LOD near bound left unresolved from its stored square | `scene_graph_is_built_from_a_decoded_node_array` (before the review only `range_max` was asserted) |
| the LOD level boolean inverted | `scene_graph_is_built_from_a_decoded_node_array` |
| a node's name trimmed instead of crossing over verbatim | the graph test and the retail `planes.zbd` test |
| `zone_id`, `flags`, `parent`, `children` or `index` dropped or substituted | the reader test, the graph test and the retail `planes.zbd` test (before the review `zone_id` and `flags` survived) |

## Unknowns and limitations (all recorded, none guessed)

- **The world containers' child lists do not cover every node that names a
  parent.** 346 of 7 064 nodes in `C1` (and 155–471 across the eight world
  containers) are absent from their parent's list, so `SceneGraph::build` refuses
  all eight with `InconsistentParentage` before any name is reached.
  **Affected content:** every world container's hierarchy — `ZBD/C1` …
  `ZBD/C5` — so F18's world import and any F11-D roster row whose root lives in
  a world container. **Not affected:** `planes.zbd`, the aircraft container
  F11-B/C/D care about, where the count is 0. **Which side is authoritative is
  unmeasured**: the reference reads both and asserts they agree, so it would
  refuse these containers too. Resolving this is a measurement against the
  original engine, not a decision this reader may make; a follow-up task is
  filed.
- **`brigturret2 ` and five descendants cannot form a `scene_node` key.** The
  stored name carries a trailing space. **Affected content:** the `planes.zbd`
  conversion, so F11-B's ECS import and F11-D's roster audit still cannot run
  over the real container end to end, although the records now exist and every
  other stage of the path is proven. The id scheme is F11-A's published contract
  and a fix belongs to the F11 stages, not to this reader; a follow-up task is
  filed.
- **The node flag bits are unmeasured.** `NodeBitFlagsCs` names every bit `UNK`
  in the reference, so `RawNodeInfo::flags` is raw and `SceneNode::visibility`
  stays `Resolved::Unknown`. Resolving it is F11-B/D evidence work.
- **The store's angle unit is radians, and the F16-A registry has no CS source
  yet.** The reference composes each record's stored matrix with `sin`/`cos` of
  the **raw** numbers and asserts every euler component lies in `[-π, π]`; the
  measured corpus tops out at exactly π. So the record is radians, and
  `RawObject3dData::euler_matrix` composes the raw numbers as such.
  `RawObject3dData::matrix_disagrees` is therefore *the reference's own* check,
  useful for reporting what a container holds; a conversion whose declared
  source uses a different angle unit must make its precedence decision in that
  unit rather than reuse that one. The F11-A conversion routes the triple
  through the `SourceAdapter`'s angle unit, so a CS source declared as degrees
  would read a stored π/2 as π/2 *degrees* — that is a property of the adapter
  declaration, not of this reader, and declaring the CS convention is F16's work
  (the unit calibration stage). The node-array tests here therefore convert
  through the F16-A `canonical` adapter (identity axis map, radians, one unit
  per metre) so their composition expectations are about the node array and not
  about an adapter the registry has not been given for CS yet.
- **The euler convention is observed-tool evidence, not measured from the
  game.** `Rz·Ry·Rx` over negated angles is the pinned reference's rule. The
  corpus is consistent with it (only 148 of 20 063 transformed object records
  disagree at all), which is corroboration and not proof: a record that
  disagreed for any other reason would look the same.
- **`zone_id`'s domain is still unknown** beyond "255 means no zone"; 1 and 2 also
  occur. Carried raw.
- **`LodCsC.level`'s meaning is unknown.** The stored boolean crosses over as
  stored. Measured over all nine archives: level 1 in 4 832 of the 4 953 LOD
  records and 0 in 121, so the corpus has both values and says nothing about
  which means what.
- **The LOD `range` fields are source units and the conversion is the caller's.**
  This reader resolves the near bound's stored square and carries the far bound
  as stored; which unit the original engine's distances are in is F16's
  calibration, and the F11-A conversion applies the declared `SourceAdapter`.
- **`scale` is `1.0` in all 51 611 object records**, so the `rotation · scale`
  composition order is unobservable in the corpus. It stays F11-A's designed
  choice; nothing here measures it.
- **The `node_index` word's meaning is unknown** beyond the top-byte constant and
  the `0x00FFFFFF` "invalid" value. It is not unique in the corpus and nothing
  addresses by it, so it is provenance only.
- **The world record's content is not read.** Its area, fog, partitions and
  per-partition node lists are F18's subject. They remain addressable through
  `RawNode::data_offset` / `data_bytes`, so F18 re-derives them from these bytes
  rather than asking this reader to have guessed.
- **The camera, window, display and light records are sized, not interpreted.**
  Only their lengths and the light's parent word are used; no field of theirs is
  decoded. F21 (cameras) and F19 (sky) are the stages that own them, and the
  bytes are addressable.
- **`mesh_index` is not range-checked against the mesh array.** This reader does
  not hold the mesh section, so it reports the bounds it sees
  (`GameZNodes::mesh_index_bounds`) and leaves the check to a caller holding the
  array. The reference asserts a non-negative index is inside the array *and*
  holds a present mesh; that assertion is not made here, and the gap is the
  caller's to close. The F10-C.03 mesh catalog is the natural place.
- **A stored `data_ptr` that disagreed with the walk would be refused, not
  followed.** No measured archive disagrees, so which reading the original engine
  used cannot be decided from the corpus; the strict one is kept and pinned by
  the synthetic test.
- **`read_gamez_nodes` re-reads the 40-byte header** through the same
  `read_container_header` the mesh and material sections use, so all three
  sections are gated by one header parse. This is a consistency property of the
  container, not three independent parses of it, and it is stated the same way
  the F11-D census states it.
- **The task description's byte extent for the node array is not implementable as
  written** and was not implemented: `nodes_offset + node_array_size` is a count
  added to an offset, while the section is `212 × node_array_size` of info
  records plus a variable-length data section that runs to the container's end.
  The layout above is what the bytes show, measured over all nine archives. This
  is recorded here rather than in the task record because the description is not
  a protected path and the correction is a fact about the format.
- **Evidence class.** The layout is documented in a pinned third-party tool
  (mech3ax v0.6.0, commit `d3521a9721be731d365504568ddcd78e3f9846bb`) and
  reproduced from the original bytes across all nine archives, which is
  `ObservedTool`. No original run happened: `retail` is file access, not
  evidence of runtime behaviour, and the euler convention, the node-type tags and
  every field's meaning are that tool's rules, not measurements of the game.
  **A further agent instance with a fresh context should review this format
  work**, and no agent review replaces the owner's approval.
- **Nothing derived from the original bytes is committed.** The numbers above are
  counts, offsets and ranges; no name list, no mesh, no material and no
  screenshot is in the repository.

## Sources used

- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (F11-A input
  contract, deliverable, non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, provenance, explicit
  unknowns, exact lookup, ownership cycles).
- `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` (the `ParsedNode`
  contract, the `scene_node` id scheme, the recorded unknowns, and the instruction
  to keep the unmeasured words at record level).
- `docs/findings/2026-09-29-f11-b-hierarchy-import-and-lod-selection.md` (the
  limitation this task resolves, and the fixture's sibling-LOD shape).
- `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` and
  `crates/cs_formats/src/gamez/reader.rs` (the header this reader reuses, the
  section chain, and the `DataOffset`-style cross-check precedent).
- Pinned reference mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
  `docs/research/SOURCES.md`): `crates/mech3ax-nodes/src/cs/node.rs` (`NodeCsC`,
  `assert_node`, `read_node_info`, `NodeVariantsCs`), `crates/mech3ax-nodes/src/
  cs/{object3d,lod,world,window,camera,light,display}/*` (the per-kind records and
  their assertions), `crates/mech3ax-nodes/src/node_data/display.rs`,
  `crates/mech3ax-nodes/src/types.rs` (`NodeType`, `ZONE_DEFAULT`),
  `crates/mech3ax-nodes/src/flags.rs` (`NodeBitFlagsCs`),
  `crates/mech3ax-nodes/src/math.rs` (`euler_to_matrix`),
  `crates/mech3ax-gamez/src/gamez/cs/nodes.rs` (`read_nodes`, the two-pass order
  and the per-kind parent/children reads) and
  `crates/mech3ax-gamez/src/gamez/common.rs` (the `node_index` masks). No code was
  copied; mech3ax is EUPL-1.2 and is read as a reference only.
- `crates/cs_content/src/{scene,coordinates}.rs` and
  `crates/cs_types/src/{content,space}.rs` (the F11-A and F16-A contracts this
  stage plugs into, and the id grammar the refusal above comes from).

## Review, 2026-10-02

Reviewed by `bunny-alpha-2`, which is **the same agent instance that
implemented this task**: the implementer and the reviewer identities are the
same, so this is not independent review of the format work. AGENTS.md asks for a
different instance or model with a fresh context there, and that remains
outstanding. What the review did do was re-derive the layout from the pinned
reference and re-measure the corpus, rather than take either on trust.

**The layout itself checks out.** Every field offset, record size and parent-word
rule was compared against mech3ax v0.6.0 at the pinned commit: `NodeCsC` 208
with `node_type` at 52 and `mesh_index` at 60, `Object3dCsC` 144 with the
rotation at 24 and the trailing 48 zero bytes, `LodCsC` 92, `WorldCsC` 204
followed by one child-value word and a grid of 88-byte `PartitionCsC` cells whose
`count` is the eighth `u16`, `LightCsC` 256 plus one unconditional word the
reference reads "as a result of `parent_count`, but is always 0", and the
two-pass `NODE_CS_C_SIZE * array_size + 4 * array_size` info array with the
sequential data walk after it. The light judgement call the handover flagged is
correct against the reference.

**Two recorded numbers were wrong and are corrected above**: the per-kind corpus
totals (counted as though all nine containers were world containers) and the
`57 856` in this module's own provenance header. Everything else in the
measurement table re-measured identically: bytes, data-section ranges, the
object 32/40 split, the mesh bounds and index ranges, the per-archive
disagreement counts, `unk196`, `parent_count`, the LOD ranges and levels. The
new retail test pins all of it, so the table is now falsifiable rather than
prose.

**Two production fixes.** The conversion substituted
`AuthoredTransform::IDENTITY` for every record flagged as holding no transform.
The reader reports `ObjectIdentityNotIdentity` for a flagged record that stores
something else — but the conversion then discarded exactly the numbers that
disagreed, so the disagreement was unreportable through the typed record. It now
substitutes the identity only when the record really is the identity.
`GameZSceneError::node()` reported a node array slot for a `MeshKind` refused
when a `MeshSlot` was constructed, at which point no node has used the slot: it
passed the variant's `u32::MAX` sentinel straight through, so a caller logging
the node would have reported a slot no container can contain. It now reports the
absence that case is.

Two smaller reader fixes: an unknown `node_type` refusal was anchored 36 bytes
past the word it had read (`NODE_NAME_BYTES` added twice), and a world cell whose
own value count does not fit what is left of the data section surfaced as a bare
truncation rather than as `PartitionGrid`. The first is now the named
`NODE_TYPE_OFFSET` constant, pinned by a test; the second is refused with the
grid's own reason.

**Two coverage fixes that mattered.** The light node's unconditional parent word
was covered only by a `#[ignore]`d retail test, which CI never runs, so the one
judgement call the handover asked reviewers to check was the one thing a
mutation could remove silently. The world's own children-count check was covered
by nothing at all. Both are now killed by synthetic tests. Sixteen mutations were
added by the review on top of the implementer's seven; every one is killed by a
test CI runs.

**Not changed, and why.** The `brigturret2 ` refusal, the world containers'
partial child lists and the un-range-checked `mesh_index` stand: they are facts
about the data or the F11-A contract, already recorded as limitations above and
filed as follow-ups, and a reviewer resolving them would be inventing.
