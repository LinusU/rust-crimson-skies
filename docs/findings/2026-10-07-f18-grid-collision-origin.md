# F18-GRID-COLLISION-ORIGIN: how the original engine built world collision, and the six grid-named fog volumes

Date: 2026-10-07. Task #727 `F18-GRID-COLLISION-ORIGIN`, the follow-up #716
(`M01-LC-FVOL-ROLES`) filed against its own two disagreeing statements. Spec:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md` (`### F18-B`,
`### F18-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.

## Provenance and method

Static analysis of the owner-supplied decrypted image
`$CS_GAME_DIR/crimson.decrypted.exe` (sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`), same image,
same provenance and same address convention as
`docs/findings/2026-10-05-f04-d-original-lookup-order.md`: addresses are virtual
addresses, image base `0x400000`. Tools: radare2 (analysis, string cross
references), `llvm-objdump` and capstone (disassembly), plus a scratch byte
parser used only to cross-check numbers the production readers already publish.
No image bytes, disassembly listing or decompiled code is committed — only
addresses, constants, instruction shapes and counts.

**This is code-derived evidence, not a runtime capture.** Nothing here is
`verified_original`, and no claim below is a behavior landmark before #358
supplies an original run. The evidence class of everything touched by this task
stays `observed_tool`.

Two independent things were measured, then bound together:

1. what the 2000 engine does with the world's partition grid and with a node's
   per-record state when it decides what a spatial query may hit (sections A–D);
2. what the original installation actually stores in those six records (section
   E), through the production readers.

## A. The loader reads the whole partition grid — and rewrites its values into node pointers

The GameZ reader is `D:\zipper\gamez\zclass\cls_zbd.c` (its name is pushed with
every error in the region). The container is opened by checking the signature and
version the workspace's own reader checks: `cmp dword [edi], 0x2971222` at
`0x4e341f` and `cmp dword [edi+4], 0x2a` at `0x4e3433`, failing with
"Error reading GameZ header data; incompatible file type" (`0x62e8c4`, line
`0x6c8`) and "…incompatible file version" (`0x62e888`, line `0x6f1`).

The world node's branch (`fcn.004e2ba0`) then does, in order — every address is
an instruction, not an inference:

| step | instruction(s) | address |
| --- | --- | --- |
| read the 204-byte world record | `push 0xcc` … `fread` into `[node+0x38]` | `0x4e2f4a`–`0x4e2f6b` (failure "Error reading node world data." `0x62e72c`, line `0x53c`) |
| `partition_y_count` = `[world+0x9c]`, `partition_x_count` = `[world+0x98]` | `mov ecx,[eax+0x9c]` … `mov eax,[eax+0x98]` | `0x4e304d`, `0x4e3081` |
| one pointer per row: `calloc(y, 4)` stored at `[world+0xa0]` | `push 4; push ecx; call [0xa201b4]` → `mov [edx+0xa0],eax` | `0x4e3053`–`0x4e305f` |
| one buffer per row: `calloc(x, 0x58)` → `array[row]` | `push 0x58; push eax; call [0xa201b4]` → `mov [edi],eax` | `0x4e3081`–`0x4e3094` |
| one 88-byte cell: `fread(buf, 0x58, 1, f)` | `push ebx; push 1; push 0x58; push edi` | `0x4e30de`–`0x4e30ea` |
| the cell's value count is the `u16` at **offset 0x3a** | `movsx eax, word [edi+0x3a]` | `0x4e30f6` |
| its value buffer is `count × 12` bytes at **cell+0x3c** | `lea eax,[eax+eax*2]; shl eax,2` → `mov [edi+0x3c],eax` | `0x4e3101`–`0x4e3116` |
| that buffer is read from the file by `fcn.004e3360` | `call 0x4e3360` | `0x4e3119` |
| next cell | `add edi, 0x58` | `0x4e3137` |

Failure of a cell read is "Error reading world area partition data." (`0x62e700`,
line `0x58d`).

So the engine's own reader uses **exactly** the layout this workspace's
`WorldPartitionGrid::read` implements: 88-byte cells, the `u16` count at cell
offset 58, 12-byte values. That is a cross-check of the format constants from
inside the original executable, not a restatement of the pinned reference.

`fcn.004e3360` does one more thing that matters for what the grid *is* at run
time. After `fread(count × 12)` it walks the buffer at a 12-byte stride
(`add esi, 0xc`, `0x4e33cd`) and, for each value, replaces the stored first word:

```
mov eax,[esi]; test eax,eax; jl 0x4e33c0      ; a negative stored index stays as it is
add eax,ebx                                    ; plus the caller's base
mov [esi],eax
mov edx,[esi]; push edx; call 0x4e18f0; mov [esi],eax
```

`fcn.004e18f0` (`0x4e18f0`–`0x4e1927`) is that replacement: negative → `0`,
otherwise `row = index / [0x62d410]`, `col = index % [0x62d410]`,
`base = [0x727c18][row]`, and it returns

```
lea eax,[edx+edx*2] ; lea eax,[eax+eax*8] ; shl eax,1 ; sub eax,edx ; lea eax,[edx+eax*4]
```

= `base + 4·(54·col − col)` = **`base + 212·col`** — a pointer into the in-memory
node table, whose stride is the 212-byte node slot
(`NODE_SLOT_BYTES` in `crates/cs_formats/src/gamez/nodes.rs`). The same
212-byte arithmetic sizes the table itself in `fcn.004e1930` (`54n − n` words
→ `212·n` bytes, `0x4e1945`–`0x4e1958`).

**Measured:** the partition grid is loaded completely, and each value's stored
node slot is rewritten at load into a pointer to that node's own record. The
grid is therefore a live in-memory structure, not bytes the engine skips.

## B. `cls_di.c` walks that grid to build its "intersections" database

`D:\zipper\gamez\zclass\cls_di.c` (name pushed at `0x62cb90` with every error in
the region) holds the only consumer found that reads those cells back.
`fcn.004cb420` (its own asserts: "Null node pointer." `0x62cc14` line `0x111b`,
"Null class data pointer" `0x62cbfc` line `0x111c`) takes a node plus a query
point and radius, converts the padded box into a cell range with two calls to
`0x4da430` (`0x4cb4e4`, `0x4cb509`), and then walks:

| what | instruction(s) | address |
| --- | --- | --- |
| the row-pointer array of the record at `[node+0x38]` | `mov eax,[eax+0xa0]` | `0x4cb581` |
| one row | `mov eax,[eax+edx*4]` | `0x4cb587` |
| the cell: `lea esi,[ecx+ecx*4]` → `lea esi,[ecx+esi*2]` → `lea eax,[eax+esi*8]` = `col × 88` | `0x4cb58a`–`0x4cb590` |
| its count | `lea ebp,[eax+0x3a]` … `cmp word [ebp],bx` | `0x4cb5a1`, `0x4cb5a4` |
| its value buffer | `mov ecx,[ebp+2]` (= cell+0x3c) | `0x4cb5b6` |
| the value → the node it now points at | `mov esi,[edx+ecx]` | `0x4cb5c2` |
| database capacity (32 entries) else "Database intersections array is full" `0x62cbb0` (line `0x1184`) | `cmp byte [eax],0x20` | `0x4cb5c5`, `0x4cb5ca` |

Each candidate is then **filtered**, not accepted because the grid named it:

* `test byte [esi+0x24], 4` — the record's own flags word (offset 36 of the
  208-byte record), `0x4cb5eb`, else the value is skipped;
* a whitelist over the record's byte at offset `0x30`:
  `mov cl,[esi+0x30]; call 0x56c430` (`0x4cb5f7`), the list living at
  `0xa070a8`/`0xa070a9` with `0xff` as its own sentinel (`0x56c445`) — an empty
  list allows everything (`0x56c43a`);
* an optional name filter: `strncmp` through IAT slot `0xa20348` against the
  string at `0x71e890` (`0x4cb615`–`0x4cb62f`);
* then either recursion into the record's own children (`[esi+0x56]` count,
  `[esi+0x5c]` array, `0x4cb63c`–`0x4cb660`, into `fcn.004cb950`) or — when
  `flags & 0x40` is set (`0x4cb638`) and `flags & 0x100` is set (`0x4cb665`) —
  the narrow phase: `fcn.004cd960` copies **24 bytes from the address held at
  `[node+0x70]`** (`mov esi,[eax+0x70]`; `test ch,1`; `mov ecx,6`; `rep movsd`,
  `0x4cd9a4`–`0x4cd9b5`) and hands them to `fcn.004cb8b0`.

Its single caller is `fcn.005ac150` (`call 0x4cb420` at `0x5aca7e`), which is
called once from game code at `0x4ab284` and consumes the returned distance
against a threshold before calling `0x4b7e70`. **Which gameplay query that is
was not established here** — see section F.

**Measured:** the partition grid is the broad phase of this query, and what a
grid-named record must still pass is node state — flags, a whitelist, an
optional name, and a 24-byte box reached through `node+0x70`.

## C. The world record itself is never an intersectable object

The per-node Intersect dispatcher in the same file reads the record's
`node_type` at offset `0x34` and jumps through a table
(`mov eax,[esi+0x34]` `0x4c9ac8`; `lea ecx,[eax-1]; cmp ecx,9` `0x4c9acb`;
`jmp dword [ecx*4+0x4c9de0]` `0x4c9ad8`):

| node type | target | what happens |
| --- | --- | --- |
| 1 camera | `0x4c9d89` | handled |
| **2 world**, 3 window, 4 display | `0x4c9daf` | "Unrecognized node class type: node = %s class_type = %d" (`0x62cb54`, cls_di.c line `0xa90`) |
| 5 object3d | `0x4c9adf` | handled (counter `0x63b2f4`) |
| 6, 7, 8, 9, 10 | `0x4c9c72`, `0x4c9d12`, `0x4c9d76`, `0x4c9d9c`, `0x4c9dd7` | handled |

The `Intersect:%d C:%d A:%d Li:%d LO:%d O:%d S:%d B:%d P:%d` statistics string
(`0x625280`) reads the globals these handlers increment (`0x63b2cc` total,
incremented in `fcn.004c8f70`; per-type in `0x4c9a8b`, `0x4c9ae1`, `0x4c9c74`,
`0x4c9e12`, `0x4ca142`, …), and its twin `Altitude:%d …` (`0x625210`) reports a
second query over the same node classes.

**Measured:** the world record is refused by name in this dispatcher, so the
world node is not itself an intersectable object; the per-node machinery is
about the records it owns.

## D. Whether a node intersects at all is a separate, script-set structure

`cls_util.c`'s node copy (`D:\zipper\gamez\zclass\cls_util.c`, `0x62d958`) moves
one field per flag bit, each guarded by its own flag and each with its own
error string — which is where the flag bits get their names:

| bit | setter | error string | address |
| --- | --- | --- | --- |
| `0x10` | `fcn.004cd210` | "ERROR copying node while setting intersection field" `0x62d83c` (line `0x38b`) | `0x4d7c49`–`0x4d7c85` |
| `0x20` | `fcn.004cd2a0` | "…setting intersect bbox field" `0x62d7dc` (line `0x396`) | `0x4d7c86`–`0x4d7cc2` |
| `0x40` | `fcn.004cd310` | "…setting proximity flag" `0x62d784` (line `0x3a1`) | `0x4d7cc3`–`0x4d7cfe` |
| `0x80` | `fcn.004cd360` | "…setting landmark flag" `0x62d72c` (line `0x3ac`) | `0x4d7d00`–`0x4d7cff` |
| `0x20000` | `fcn.004ccb70` | "…setting clip_to flag" `0x62d678` (line `0x3c2`) | `0x4d7d7a` |
| `0x1000000` | `fcn.004cd460` | "…setting DI zone check flag" `0x62d61c` (line `0x3cd`) | `0x4d7db7` |

and the two fields are settable from scripts: `SetIntersectSurface` and
`SetIntersectBBOX` are INTERP commands compared in the command dispatcher at
`0x5bbccc` and `0x5bbc90` (names at `0x63e5a8`, `0x63e5bc`), beside
`SetAltitudeSurface` (`0x63e654`).

**Measured:** "does this node intersect" is per-node state that lives outside
the container's stored record and is partly written at run time by scripts.

## E. The six grid-named fog volumes, as the installation stores them

Read through the production readers (the committed test
`accept_f18_grid_collision_origin_retail_the_six_grid_named_fog_volumes_are_named`
does exactly this, one discovery over all eight world containers), and
cross-checked by an independent byte parser whose walk ends on the container's
last byte for all eight:

* the eight world containers hold **65 `fvol*` records; exactly six of them are
  named by a world's partition grid** — `c1c`: `fvol1`…`fvol4`, node slots
  **944, 945, 946, 947** (4 of 293 grid records); `c5`: `fvol1` and `fvol3`,
  node slots **2304** and **2306** (2 of 471); **zero** in `c1`, `c1b`, `c2`,
  `c2b`, `c3`, `c4`. The other 59 `fvol*` records are world-owned and
  unindexed, which is where they already resolve role-unknown (`#677`'s
  split).
* all six bind a stored mesh, all have `parent_count = 1`, `children_count = 0`,
  stored flags `0x0308831c` and zone id 2 (`c1c`) / 1 (`c5`). Their stored
  `unk140` boxes are 1024×120×1024 unit boxes at the four corners of `c1c`'s
  `[-12288, 0]²` plane and two 1024×646×1024 unit boxes in `c5`.
* recorded without interpretation: every grid-named record of `c1c` has
  `children_count = 0` and none of the 293 carries flag `0x40`; `c5` has 372 of
  471 with `children_count = 0` and none of the 471 carries `0x40`. The flag
  bits' meanings stay as section D names them and nothing more.

The name itself is the second measured statement: the decrypted image's only
name-keyed consumer of the four bytes `fvol` is the `strncmp` at **VA
`0x44e087`** inside the routine at **VA `0x44d9d0`**, which is also the routine
that reads `fogvol.zrd`'s fog keys (measured by #716 and quoted in #727's
description). So a stored `fvol*` record is taken by the original as a fog
volume.

## F. What fed world collision, and what is still unknown

The four options in #727's description, answered against the measurements:

1. **It consumes the partition grid** — completely, at load (section A), and
   back as a broad phase in `cls_di.c`'s intersection-database builder (section
   B). It is not "nothing outside the spatial index".
2. **It does not read the stored `unk140` boxes as a collision field in that
   path.** The box the narrow phase copies comes from the address held at
   `node+0x70` (offset 112 of the record, `unk112`, a word the pinned reference
   gives no meaning). **Whether that address points at one of the record's three
   stored boxes (offsets 116, 140 or 164) is not measured** and is not claimed.
3. **A separate structure exists** for whether a node may be intersected at all:
   the `intersection` and `intersect bbox` fields behind flags `0x10`/`0x20`,
   settable at run time by `SetIntersectSurface`/`SetIntersectBBOX` (section D).
4. Grid membership is therefore **candidate membership**: a record still has to
   pass node flags, a whitelist, an optional name and a box test before it can
   be reported by a query.

**Not established, recorded rather than guessed:**

* which gameplay query `fcn.005ac150`/`fcn.004cb420` serves — the only caller
  found, `0x4ab284`, is in game code this task did not identify;
* what `node+0x70` points at, and what each flag bit means beyond the names
  `cls_util.c`'s error strings give them;
* whether the original collides with a fog volume at run time at all. No
  original run happened (#358), so `retail` here is file access and nothing is
  `verified_original`.

## G. The binding

`crates/cs_content/src/world.rs`:

* new claim id
  **`f18-world.grid-named-fog-volume-role-unmeasured`**
  (`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`), documented with the addresses
  above. A record the grid names whose name starts with the four bytes `fvol`
  resolves its collision role **and** its shape to `Resolved::Unknown` under
  this claim instead of `INDEXED_RECORD_IS_STATIC`'s `Solid`+`FromMesh`. The
  match is the image's own: four bytes, in order, no case folding
  (`is_fog_volume_name`).
* `WorldImportReport` gains **`partition_records_fog_volume()`**, so the two
  statements about one record are reconcilable with three numbers:
  `partition_records == objects_solid + partition_records_fog_volume`.
* `INDEXED_RECORD_IS_STATIC` keeps its place for the rest of the index and
  gains the measured exception in its own documentation; `objects_solid`'s
  documentation no longer claims equality with `partition_records`.

Nothing else changed: sector membership, the mesh reference, the stored
transform, the unindexed split (`#677`) and every other claim id are untouched,
and the six records are still *in* the grid and still get a sector.

**Affected content, stated as #727 requires:**

* collision over the six grid-named fog volumes — `c1c` `node-944`…`node-947`
  and `c5` `node-2304`, `node-2306`: they no longer produce a collider, they
  report `unknown_collision_role` instead, and their shape is unknown rather
  than `FromMesh`;
* **every claim that a grid-named record is static collision geometry**: it now
  carries the named exception above. `INDEXED_RECORD_IS_STATIC` stays a designed
  rule about this conversion for the other 289 (`c1c`) and 469 (`c5`) records,
  and stays *not* a statement about how the 2000 engine collided.
* count changes that follow, all re-pinned through the production path and
  stated against the tree task #716 landed on (which had already resolved the
  17 unindexed `fvol*` records of `c1c` and the 15 of `c5` as fog volumes, and
  had left these six `Solid` as this task's description quotes it): `c1c`
  `objects_solid` 293 → **289**, colliders 292 → **288**, `unresolved_collision`
  0 → **4** — the four grid-named records are now the only `c1c` record with no
  measured role; `c5` `objects_solid` 471 → **469**, colliders 367 → **365**,
  `unresolved_collision` 70 → **72**. The other six containers do not move.
* the committed evidence censuses `docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json`
  and `docs/findings/evidence/M01-LC-FVOL-ROLES.json` were each produced at
  their own recorded tree and predate this change; their `objects_solid` rows
  for `c1c` and `c5` describe those trees. Regenerating them is those tasks'
  own harnesses, and no number in either was edited here.

## Test inventory

| test | covers | fails when |
| --- | --- | --- |
| `accept_f18_grid_collision_origin_a_grid_named_fog_volume_is_never_solid_geometry` (synthetic) | a grid-named `fvol*` record resolving the new claim for role **and** shape, the report's fog counter, the other three grid records still `Solid`, the unindexed split untouched, and `spawn_world` giving it no collider while reporting both gaps | the carve-out is removed (role becomes `Solid`, `objects_solid` 4, fog counter 0, colliders 4), it spills to other records, or the claim id/reason is dropped |
| `accept_f18_grid_collision_origin_retail_the_six_grid_named_fog_volumes_are_named` (`#[ignore]`, retail) | one production discovery over all eight containers: exactly 4 + 2 + 0×6 fog volumes, their node slots **and stored names**, the partition identity `objects_solid == partition_records − fog`, and every other grid-named record in every container still `Solid` | any count moves, a name or slot changes, the corpus stops being six, or the exception widens to another record |

Both run and pass; the retail one is `#[ignore] = "requires CS_GAME_DIR"` and
runs locally with `--include-ignored` (measured on this host: 291 s, dominated
by one production discovery).

Supporting assertions were re-pinned where the change is visible:
`crates/cs_app/tests/world/import_retail.rs` (c1c counts, spawn report, the
claim-id census now nine), `crates/cs_app/tests/world/world_units.rs` (a
`grid_fog` column per container, the solid/unresolved/collider identities),
`crates/cs_app/tests/accept_m01_lc_world_unit_roles.rs` (the c1c spawn split)
and — because #716 landed this branch's other half while it was in review, and
its own test pinned the opposite answer for exactly these six records —
`crates/cs_app/tests/world/fvol_roles.rs`: the grid-named `fvol*` record of its
fixture now resolves #727's claim instead of `Solid`, and its two
`objects_solid == partition_records` identities became
`objects_solid == partition_records − partition_records_fog_volume`. No test
was weakened: each assertion that moved states a *smaller* or *differently
partitioned* truth and names why, and each keeps the property it existed for
(the index still decides the rest of the role; the spawn still reports exactly
its gaps).

## Evidence

`docs/findings/evidence/F18-GRID-COLLISION-ORIGIN.json` — schema
`schemas/evidence.schema.json`, written by
`crates/cs_app/tests/evidence_report_f18_grid_collision_origin.rs` and
validated with `tools/validate_evidence.py … --require-pass` (exit 0,
`structurally_valid: true`). Capabilities `retail` + `synthetic`; the candidate tree recorded in the JSON
is the tree of the commit the acceptance run tested; 2 tests discovered,
executed and passed; claim `implemented`. Its
second artifact, `grid-collision-origin-census.json`, is a **second production
observation** — one discovery over the installation and one import per
container — recording each group's grid size, fog counter, solid count and the
node slots of the six fog volumes (C1C 293/4/289, C5 471/2/469, zero
elsewhere), plus the measured source's own calibration record. No original byte
is in either file: ids, digests, counts and claim labels only.

## Sources used

- `$CS_GAME_DIR/crimson.decrypted.exe` (sha256 above), addresses as in
  `docs/findings/2026-10-05-f04-d-original-lookup-order.md`.
- `docs/findings/2026-10-04-m01-lc-world-import.md` (what the grid and the
  `unk140` boxes are, and the two open questions this task answers),
  `docs/findings/2026-10-02-gamez-node-array-layout.md` (the 208-byte record's
  field offsets, the 212-byte slot, the grid framing),
  `docs/findings/2026-10-05-f18-world-units-containers.md` (the per-container
  counts this task re-pins).
- `crates/cs_formats/src/gamez/nodes.rs` (`NODE_SLOT_BYTES`, `WORLD_PARTITION_BYTES`,
  `WORLD_PARTITION_VALUE_BYTES`, `RawNodeInfo` offsets) and
  `crates/cs_formats/src/zbd/header.rs` (`GAMEZ_SIGNATURE`, `GAMEZ_VERSION`) —
  the constants the disassembly reproduces.
- `crates/cs_content/src/world.rs` (`import_world_container`,
  `WorldImportReport`) and `crates/cs_app/src/world/{retail,spawn}.rs`, the
  production path the tests drive.
- Pinned reference mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` ([S02], [S17] in
  `docs/research/SOURCES.md`), read as a reference only; nothing was copied.

## Commands run

```sh
cargo fmt --all -- --check                                            # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --workspace --locked   # exit 0, 409 test binaries + 10 doc-test suites (419 green results)
cargo test --workspace --locked -- accept_f18_grid_collision_origin_ --include-ignored
#   exit 0: 2 tests discovered, executed and passed (1 synthetic, 1 retail)
# the re-pinned sibling suites, each with CS_GAME_DIR:
cargo test -p cs_app --test world -- accept_m01_lc_world_import_ --include-ignored        # exit 0, 9 passed
cargo test -p cs_app --test world -- accept_f18_world_units_containers_ --include-ignored # exit 0, 5 passed
cargo test -p cs_app --test accept_m01_lc_world_unit_roles -- --include-ignored           # exit 0, 5 passed
cargo test -p cs_app --test world -- accept_m01_lc_fvol_roles_ --include-ignored          # exit 0, 4 passed
# evidence (docs/contracts/CLI-EVIDENCE.md):
cargo test --locked -p cs_app --test evidence_report_f18_grid_collision_origin -- --ignored  # exit 0
python3 tools/validate_evidence.py docs/findings/evidence/F18-GRID-COLLISION-ORIGIN.json \
  --artifact-root private/evidence/F18-GRID-COLLISION-ORIGIN --require-pass  # exit 0
# every check above was re-run on the rebased branch head after main moved with
# Cargo.toml/Cargo.lock changes; the report was regenerated on that tree.
```

## Review (bunny-2, 2026-10-07)

For the record (Rally does not enforce reviewer assignment): the implementer and
the reviewer are both the agent `bunny-2`, in **separate sessions with fresh
contexts** — the reviewing session started from the task history, not from the
implementing one — and no agent review replaces the owner's human approval.

**The review found task #716 (`M01-LC-FVOL-ROLES`) merged into `main` while this
branch was under review**, in the same files. It had measured what the image
does with an `fvol*` record's *name* (`FOG_VOLUME_RECORD_NEVER_BLOCKS`: an
unindexed one resolves `None`), added this report's
`partition_records_fog_volume()` accessor with the opposite reading — count the
six, keep them `Solid` — and pinned that answer in its own suite as exactly the
limitation #727 was filed to settle. The branch was rebased onto it and the two
measurements were bound together:

* an unindexed `fvol*` record keeps #716's `None` + `FOG_VOLUME_RECORD_NEVER_BLOCKS`;
* a **grid-named** `fvol*` record resolves this task's explicit unknown instead
  of `Solid`, which is the half #716 recorded as its open limitation;
* `partition_records_fog_volume()` keeps one meaning — how many grid-named
  records the fog consumer keys — and
  `objects_solid == partition_records − partition_records_fog_volume` holds for
  every container;
* `crates/cs_app/tests/world/fvol_roles.rs` (#716's suite) was re-pinned to the
  settled answer: the grid-named record of its fixture carries this claim for
  role **and** shape, and its two `objects_solid == partition_records`
  identities were re-partitioned.

Re-run on this branch head by the reviewer:

* `cargo fmt --all -- --check` — exit 0.
* `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — exit 0.
* `cargo test --workspace --locked` — exit 0 (409 test binaries + 10 doc-test
  suites, 419 green results, 0 failures).
* `cargo test --workspace --locked -- accept_f18_grid_collision_origin_
  --include-ignored` — exit 0 (2 discovered, 2 passed; the log above is this
  run's).
* The four re-pinned sibling suites, each with `CS_GAME_DIR`: `accept_m01_lc_world_import_`
  9 passed (53 s), `accept_f18_world_units_containers_` 5 passed (392 s),
  `accept_m01_lc_world_unit_roles` 5 passed (302 s), `accept_m01_lc_fvol_roles_`
  4 passed (286 s).
* **Sensitivity re-checked, not assumed:** `is_fog_volume` was forced to return
  `false` — the carve-out removed — and the synthetic task test failed on its
  own report assertion (fog counter `0`, expected `1`); the tree was restored
  (`git status --short` clean) before the checks above.
* The evidence report was regenerated on the tested tree with
  `CS_EVIDENCE_REVIEWER` naming both identities, and re-validated with
  `tools/validate_evidence.py … --require-pass` (exit 0).

Documentation corrections made during review: the claim-id census row of
`docs/findings/2026-10-04-m01-lc-world-import.md` (this task moved that
assertion from eight distinct ids to nine, while the row still said seven), the
workspace count in the command block above, the counts this rebase moved in that
file's retail rows and in the #716 write-up (supersession notes, no number
edited in either task's committed evidence).
