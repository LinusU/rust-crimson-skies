# #2213 GROUNDBLOW-CLASS-TYPE: what writes a node's runtime class type (`node+0x34`), and what M01's colliders get

Date: 2026-10-29. Task #2213 (`GROUNDBLOW-CLASS-TYPE`), the measurement stage in the
ground-blow chain after #2209 (the world segment sweep `0x4c8ec0` / `0x4c8f70`, finding
`2026-10-29-groundblow-world-segment-sweep.md`) and #2212 (the face windings behind the
probe's `sqrt`). It answers section 9 of the #2209 finding — "the water-tile nodes' runtime
class type (`node+0x34`) is unmeasured" — and the same question for every other M01 collider.

Static analysis of the owner-supplied decrypted image `crimson.decrypted.exe` (read-only,
never committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file offset +
`0x400000` below VA `0x643000`), disassembled with Capstone 5.0.7 into `private/`, plus the
original installation's `ZBD/C3/gamez.zbd` and `ZBD/planes.zbd` read through the production
readers (`cs_formats::gamez::{read_gamez_nodes, read_gamez_meshes, read_gamez_materials}`).
**The original executable was not run**: every claim is `static` (claim id
`OWNER-STATIC-2026-10-29`), nothing is `verified_original`. No runtime code changed.

The cited bytes are the machine-readable block at the end; `accept_groundblow_class_*`
re-reads every row from the image and re-derives the per-kind values from retail data, and
the same rows are committed as `[[groundblow_class_fact]]` in
`data/owner-static/engine-facts.toml` (the owner-static table AGENTS.md rule 3 allows for
measured engine facts).

## 0. Answer

1. **Which loader writes `node+0x34`: the GameZ node-array reader in `cls_zbd.c`
   (`D:\zipper\gamez\zclass\cls_zbd.c`, the name pushed at `0x62e354`), and it writes it by
   copying the file.** The 208-byte `NodeCsC` info record already stores the kind at offset
   `0x34` (`cs_formats::gamez::NODE_TYPE_OFFSET` = 52 = the 36-byte name followed by the five
   words `flags 0x24`, `unk040 0x28`, `unk044 0x2c`, `zone_id 0x30`, `node_type 0x34`). The
   reader `fread`s whole 212-byte slots verbatim into the runtime node table
   (`0x4e2890`…`0x4e298b`: the `212·n` arithmetic then `call [0xa201f8]`, `fread`), and its
   per-node post-pass (`0x4e2a54`…`0x4e2b06`) rewrites exactly four fields — `+0x70` (the
   stored-box pointer, `0`/`1`/`2` → `node+0x74`/`+0x8c`/`+0xa4`), `+0x3c` (the mesh index,
   made absolute and resolved through `0x560170` into the mesh's polygon list), `+0x48`
   (zeroed) and the `node_index` word at `+0xd0` — and then calls `0x4e2ba0`, which
   **dispatches on `[node+0x34]` itself** to read the kind's own data record into
   `[node+0x38]`. There is no conversion table: `node+0x34` is the stored `node_type`.
2. **From which retail field or rule: the stored `node_type` word** at offset `0x34` of the
   208-byte info record, whose domain is the CS node kinds. The loader's own table
   (`0x4e32ac`, eleven entries for values `0..10`) names each kind's reader, and each reader's
   record size is the workspace's own constant for that kind:

   | `node+0x34` | reader | record bytes | workspace constant |
   | --- | --- | --- | --- |
   | 0 | `0x4e31e0` (no class data; children only) | — | the reference asserts it never occurs in a CS container |
   | 1 camera | `0x4e2d90` | `0x1e8` = 488 | `CAMERA_DATA_BYTES` |
   | 2 world | `0x4e2f4a` | `0xcc` = 204 | `WORLD_DATA_BYTES` |
   | 3 window | `0x4e2efa` | `0xf8` = 248 | `WINDOW_DATA_BYTES` |
   | 4 display | `0x4e2e9b` | `0x1c` = 28 | `DISPLAY_DATA_BYTES` |
   | 5 object3d | `0x4e2c83` | `0x90` = 144 | `OBJECT3D_DATA_BYTES` |
   | 6 lod | `0x4e2cbb` | `0x5c` = 92 | `LOD_DATA_BYTES` |
   | 7, 8 | `0x4e3283` | — | refused: `gClsBlockReadNode(): Unrecognized node class type:  node = %s class_type = %d` (`0x62e6b0`) and `-1` |
   | 9 light | `0x4e2cfd` | `0x100` = 256 | `LIGHT_DATA_BYTES` |
   | 10 sound | `0x4e2bcc` | `0x94` = 148 | not a CS kind ("Error reading node sound data.", `0x62e7f0`) |

   The loader's error message calls the word **`class_type`**, which is the name this finding
   and #2209 use for it. A CS container stores none of `0`, `7`, `8` or `10`: measured below,
   M01's world container holds only `1, 2, 3, 4, 5, 6, 9`.
3. **Nodes the engine creates at run time get the type from their class constructor**, not
   from a file. `0x4cc4f0` takes a slot out of the same 212-byte node table and zeroes the
   whole 208-byte info record (`mov ecx, 0x34; rep stosd` = 52 dwords), then stamps the
   slot's `node_index` word; the class's constructor writes `+0x34` with a constant —
   object3d `mov dword [esi+0x34], 5` at `0x4d1363`, lod `…, 6` at `0x4e05ec`, camera
   `mov edi, 1; … mov [esi+0x34], edi` at `0x4d21b1`/`0x4d21bc`. The node **copy**
   `0x4d8470` (a slot's `0x4000000` bit blocks it, `0x4d849b`) dispatches on the *source's*
   `node+0x34` through the table at `0x4d8568` and calls that class's copy, which calls the
   class constructor first and then the flag-guarded info copy `0x4d7b90`: the class
   survives a copy. So a runtime node's `+0x34` is its class constant, and a loaded node's
   is the file's `node_type` — the two spaces agree by construction, which is why the
   class-typed consumers below can switch on `+0x34` for nodes of both origins.
4. **Per M01 collider kind, from retail data** (`ZBD/C3/gamez.zbd`, 5408 stored nodes):
   **every collider node is class 5 (object3d)**.
   * 4736 nodes carry both sweep bits `0x04|0x10`; 2343 of them bind no mesh (`mesh_index`
     `-1`) and 0 bind a missing mesh. The **2393 nodes that carry both bits and bind a mesh
     — the exact population the port spawns a collider for (`PlaytestSea` or
     `WorldSceneryCollider`) — are all `node_type = 5`.**
   * **Water / `PlaytestSea`: 347 nodes bind at least one material group whose texture is
     `wtr0*` or `water1` (the population #1850 counted: 251 store `0x0308831c`, 96 store
     `0x0308031c`), and all 347 are class 5.** 344 of them bind only water textures
     (250 × `0x0308831c`, 94 × `0x0308031c` — the "contactable water nodes" #2212
     censused); the other three bind one non-water group as well. The two nodes #2212
     measured — the tile `g28530` (slot 547, mesh 371) and the
     terrain chunk `g28603` (slot 2873, mesh 761) — are both class 5.
   * **The `WorldSceneryCollider` kinds** (mountains, cliffs and land, every other flagged
     mesh node): 2049 nodes, all class 5. Their stored flag words are `0x0308031c` (1757),
     `0x0308033c` (31), `0x0308071c` (124), `0x0308073c` (12), `0x0308831c` (117),
     `0x0308871c` (7) and `0x0388031c` (1).
   * **Aircraft:** the vehicle's node is the plane container's node for that aircraft, looked
     up **by name** and **copied**. `0x477850` takes the plane record's name (`[record+4]`),
     looks it up through `0x4d0280(name, 7)` → `0x4d0320` (a name-keyed walk of the list the
     node loader fills with `0x4d0430(node, 7)`), and copies the result through
     `0x4d85a0` → `0x4d8470`. In `ZBD/planes.zbd` (3317 nodes) all **28 root nodes** are
     class 5, including M01's `bloodhawk` (slot 2363, flags `0x0308051c`, no mesh of its
     own; its five children `markers`/`healthy`/`shadow`/`destroyed`/`dontmove` are class 5,
     and the container's 87 `lod` bands carry the two sweep bits but bind no mesh). So an
     aircraft's node is class 5 and the sweep reaches its mesh-bearing descendants; the
     vehicle-side code reads the same word and compares it, `0x47c27b` against `2` (world)
     and `0x47c337`…`0x47c378` against `5`, `1` and `8`.
5. **What the sweep therefore does with each kind.** The query's dispatch is #2209's
   (`0x4c9ac8`, table `0x4c9de0`): types `2, 3, 4` and anything outside `1..11` log
   `Unrecognized node class type` and return 3 without testing anything. Among M01's nodes
   the skipped kinds are exactly the 435 scene-furniture nodes that carry the bits and no
   mesh: `world1` (class 2, slot 0, flags `0x0108001c`), `display` (4), both windows
   (`window1`, `sgwin`, class 3), both cameras (`camera1`, `spyglass`, class 1) and the 429
   `lod` nodes (class 6). **The class-type filter removes no M01 collider**: every node with
   geometry is class 5, and no class-6 node in C3 binds a mesh (`lod` + mesh = 0 nodes).
6. **Does the sea answer?** **Yes, by class type — and by the type-5 handler's triangle
   path.** The type-5 handler at `0x4c9adf` is: an optional node-bounds pre-test through
   `0x4cc010` (run when the accumulator holds more than one contact **or** the node carries
   `0x20`; a bounds miss returns at once, `0x4c9b09`), a stored-box contact when `0x20` is
   set (`0x4cc1be` → `0x55d270` writes the record, then `0x4c7f50` appends it), and
   otherwise the node's **mesh triangles** through `0x55c9c0(node+0x3c, boxA, boxB, record)`
   (`0x4c9bc9`) gated only by `node+0xc4 > [0x71e8b0]`, followed by a recursion into the
   node's children. No water tile carries `0x20` (0 of 347) and all 2393 colliders store
   `unk196 = node+0xc4 = 160`; `0x71e8b0` is a query threshold whose only setter `0x4c7600`
   stores `-1` for "no limit" (`0x4c7612`) and which `0x5b055d` restores to `-1` after the
   only query that raises it, so the triangle test runs. The sea therefore answers through
   its tiles' own triangles, as #2209 section 7 concluded; **its class type is 5, not one of
   the skipped kinds.** 43 of the 2049 land colliders do carry `0x20` (31 × `0x0308033c`,
   12 × `0x0308073c`) and reach the stored-box contact instead; none of them is water.
7. **The one filter that could still drop the sea is not the class type.** `0x56c430`
   (`node+0x30` = the stored `zone_id`) accepts everything while `[0x62cb50] == 0`, accepts
   `0xff` always, and accepts everything while the layer list at `0xa070a8` is empty — but
   **all 347 water nodes store `zone_id = 1`**, while 1456 of the 2393 colliders store
   `0xff`. If M01 ever installs a layer list without zone 1, the sea is dropped **by the
   layer filter, not by `node+0x34`**; the value of `[0x62cb50]` at ground-blow time is a
   runtime global this task did not measure (it is written `0` at `0x49f3d7` and `0x4d65ea`
   and set to a node's list around specific queries, `0x4d65d8`/`0x4d6602`).

## 1. The loader: the slot is read, then four fields are rewritten

| site | what it does |
| --- | --- |
| `0x4e2890` | `fcn.004e2890(file, first, last)`: the node-array read. `[0x727c10]` rows, `[0x62d410]` slots per row, `[0x727c18]` the row-pointer array, `0x62d414` the free-slot cursor |
| `0x4e28f1`…`0x4e2927` | a new row: `calloc((row+1)·4)` for the pointer array and `calloc(212·per_row)` for the row, then `rep stosd`/`rep stosb` zero it |
| `0x4e2964`…`0x4e2985` | the size arithmetic `lea eax,[ecx+ecx*2]; lea eax,[eax+eax*8]; shl eax,1; sub eax,ecx; shl eax,2` = **212·n**, then `fread(buf, 1, size, file)` through `[0xa201f8]` — the info records land in the runtime table unchanged |
| `0x4e29cb`…`0x4e2a0d` | a slot that straddles a row boundary is zeroed and read in two pieces |
| `0x4e2a54`…`0x4e2b06` | the per-node post-pass: `+0x70` box pointer fixup, `+0x3c` mesh index made absolute and resolved by `0x560170` into the node's polygon list, `+0x48 = 0`, `+0x34` **not written**, then `0x4e2ba0(node, file, base)` and the `+0xd0` index word's `0x3000000` bits set or cleared by its result |
| `0x4e2ba0` | `fcn.004e2ba0(node, file, base)`: `mov eax,[esi+0x34]; cmp eax, 0xa; ja 0x4e3283; jmp [eax*4+0x4e32ac]`, then per kind `malloc`+`fread` the class record into `[node+0x38]` |
| `0x4e32ac` | the eleven-entry table above; entry `5` is `0x4e2c83` (`push 0x90` then `fread`), entry `2` is `0x4e2f4a` (the world record the #727 finding measured) |
| `0x4e2c6b`/`0x4e2c74` | after the record, the loader registers the node with `0x4d0430(node, 7)` and again in its kind's own list at `0x62cfb8` (`0xb` for object3d, `0xa`/`9`/`0x10`/`0xf`/`0xe` for the others), which is what makes it findable by name |

So a container node's class type is the file's; the loader writes the *other* per-node
fields that need addresses in this process (`+0x70`, `+0x3c`, `+0xd0`) and nothing else.

## 2. The runtime side: constructors write it, the copy preserves it

| site | what it does |
| --- | --- |
| `0x4cc4f0` | the node allocator: a free slot from `[0x727c18]`'s 212-byte rows, `[0x62d414]` the next free index; `mov ecx, 0x34; rep stosd` zeroes 208 bytes, `or [slot+0xd0], 0x2000000` marks it live, `0x4d0430(node, 7)` registers it, `[slot+0x4c]=[slot+0x4e]=-1` (the area partition's default) |
| `0x4d1363` | object3d constructor `0x4d1330`: `mov dword [esi+0x34], 5`, `malloc(0x90)` at `[+0x38]` |
| `0x4e05ec` | lod constructor `0x4e05e0`: `mov dword [esi+0x34], 6`, `malloc(0x5c)`, then the LOD defaults (`1`, `0`, `1000.0`, `1000000.0`) |
| `0x4d21bc` | camera constructor `0x4d2180`: `mov edi,1; … mov [esi+0x34], edi`, `malloc(0x1e8)` |
| `0x4d8470` | the node copy: `test dword [eax+0x24], 0x4000000` → refuse; `mov ecx,[eax+0x34]; dec ecx; cmp ecx,0xa; ja` error; `jmp [ecx*4+0x4d8568]` |
| `0x4d8568` | the copy table: `1→0x4d84d6`, `2→0x4d8524`, `3,4→0x4d8544`, `5→0x4d84bc`, `6→0x4d84c9`, `7→0x4d850a`, `8→0x4d84fd`, `9→0x4d84e3`, `10→0x4d84f0`, `11→0x4d8517` |
| `0x4d84bc` | the object3d copy: `0x4d8160(node)` → `0x4d1330()` (constructor, writes 5) then `0x4d7b90(new, source)` |
| `0x4d7b90` | the info copy: every flag-guarded field through its own setter — bit `0x04` → `0x4cca30` (active), `0x08` → `0x4ccf00`, `0x10` → `0x4cd210` (intersection), `0x20` → the bbox setter, and so on; the curve is the same per-bit dispatch #727 section D measured. **No store to `+0x34`** |
| `0x4d850a`/`0x4d8450`, `0x4d8505`… | the kinds the copy also handles (`7`, `10`, `11`) — kind-typed copy routines of the same shape |

`0x4d0430(node, list)` is how a node becomes findable by name: `0x4cf930()` takes an entry,
`[entry] = node`, and the entry is linked into `[0x62cfb8 + list*4]`. `0x4d0320`
(`0x4d0280(name, 7)` → it) walks that list comparing the name byte by byte and returns the
entry's node, moving the found entry to the head. That is the lookup the plane path uses.

## 3. Aircraft: the plane's own node, copied

```
0x477850(record):
  if [record+8] != 0: node = [record+8]                       ; cached
  else:  node = 0x4d0280([record+4], 7)                        ; find by the plane's name
         if node == 0: return 0
         [record+8] = node
  copy = 0x4d85a0(node, 1, 1)                                  ; → 0x4d8470(node)
  if copy == 0: return 0
  if 0x4db0c0([0x71c2ac], copy) != 0: return 0                 ; add to the world
  0x4ccbc0(copy, record)                                       ; attach the record to it
  return copy
```

Its one caller is the vehicle record loader at `0x47c253`, and the record it is handed
(`[ebp+8]`) is the plane definition whose `+4` is its name and whose `+0xc`-carrying
neighbours are the airframe parameters `0x475820` copies into the vehicle. The vehicle then
gets the **copy** of the container node named after the plane; the copy's class comes from
the class constructor (`5` for a `bloodhawk` root, measured below), and the group marker
`0x40000000` that #2209 measured goes on that node's subtree (`0x47d8f5`/`0x48490a`).

## 4. M01's colliders, as the installation stores them

Read through the production readers over `ZBD/C3/gamez.zbd` (M01's world group is `C3`:
`zbd/c3/m01` is the mission directory and `read_world_sources(root, "C3")` is the production
path the port's world draw uses):

* **5408 nodes**, `node_type` histogram `{1: 2, 2: 1, 3: 2, 4: 1, 5: 4972, 6: 429, 9: 1}` —
  only CS's seven kinds, no `0`, `7`, `8` or `10`.
* **4736** nodes carry `flags & 0x14 == 0x14`; **2343** of them store `mesh_index = -1` and
  **none** names a mesh the container does not hold; **2393** bind a mesh. Every one of
  those 2393 is `node_type = 5`. Their flag words are `0x0308031c` (1851), `0x0308033c`
  (31), `0x0308071c` (124), `0x0308073c` (12), `0x0308831c` (367), `0x0308871c` (7) and
  `0x0388031c` (1); all store `unk196 = 160` (`node+0xc4`).
* **Water:** 347 nodes bind at least one `wtr0*`/`water1` group — 251 × `0x0308831c`,
  96 × `0x0308031c`, i.e. #1850's indexed/unindexed split — all class **5**, all
  `zone_id = 1`, none carrying `0x20`. 344 bind only water textures (250 × `0x0308831c`,
  94 × `0x0308031c`); `g28530` (slot 547, mesh 371, `0x0308831c`) and `g28603`
  (slot 2873, mesh 761) are class 5.
* **Land (`WorldSceneryCollider`):** the remaining 2049, all class **5**. 43 of them carry
  `0x20` (31 × `0x0308033c`, 12 × `0x0308073c`); the rest reach the triangle path.
* **Scene furniture that carries the bits and no mesh:** 435 nodes — `world1` (class 2,
  `0x0108001c`), `display` (4, `0x0108001c`), `window1` and `sgwin` (3, `0x0108001c`),
  `camera1` and `spyglass` (1, `0x0108001c`) and 429 `lod` nodes (6, all `0x0308051c`).
  Class 2, 3 and 4 are the kinds the sweep's dispatch refuses; class 1 and 6 are handled
  (camera: its own polygon list then children; lod: its bounds then children) but bind no
  mesh here, so nothing is intersected through them.
* **`ZBD/planes.zbd`:** 3317 nodes, histogram `{5: 3230, 6: 87}`; all 28 roots are class 5;
  `bloodhawk` is slot 2363, class 5, flags `0x0308051c`, `mesh_index = -1`, children
  `markers`, `healthy`, `shadow` (mesh 1443), `destroyed`, `dontmove` — all class 5; the 87
  `lod` nodes carry `0x0308051c` and no mesh.

## 5. What this means for #2214 (the sweep over the port's colliders)

* The class-type filter (types `2`, `3`, `4` and anything outside `1..11`) **never rejects an M01
  collider**, so a port that casts against every `PlaytestSea` / `WorldSceneryCollider`
  collider and against aircraft bodies is not missing a filter the original applies to this
  mission's solid geometry. It would only diverge if the port later spawned colliders for
  scene furniture (`world1`, `window1`, `camera1`, the `lod` grouping nodes) — the original
  refuses class 2/3/4 by name and finds nothing to intersect in 1/6.
* The **sea answers** through the tiles' triangles, i.e. the unnormalised face normal and the
  face area #2212 measured are what the probe's `sqrt(dot)` sees, not an AABB normal.
* The **43 land colliders with `0x20`** are the ones whose contact comes from the stored box
  (`unk112` selects `node+0x74`/`+0x8c`/`+0xa4`), not from the mesh triangles; a port that
  always uses the mesh triangles will differ on those nodes' normals and contact points.
* **Two filters are not the class type** and stay open for #2214: the layer filter
  `0x56c430(node+0x30)` (all water is zone 1, 1456 of the colliders are `0xff`; the enabling
  global `[0x62cb50]` and the list at `0xa070a8` are runtime state), and the grid reachability
  #1850 recorded for the 96 non-indexed tiles.

## 6. Unknown, and who resolves it

| unknown | affects | evidence | resolved by |
| --- | --- | --- | --- |
| `[0x62cb50]` and the layer list `0xa070a8` at ground-blow time | whether the zone-1 water tiles pass the layer filter at all | `0x56c430` is measured; the global is set/restored by several callers (`0x49f3d7`, `0x4d65ea`, `0x4d65d8`/`0x4d6602`, `0x49f25f`) and its value during a mission is runtime state | a runtime capture (#358) or tracing the writers from the mission load |
| whether a *store* to `node+0x34` exists outside the constructors and the file read | the universal phrasing of section 0.3 | the writers found are the constructors (`0x4d1363`, `0x4e05ec`, `0x4d21bc`), the copy dispatch (`0x4d8470`), and the file read (`0x4e2890`); every consumer reads it. A node slot is zeroed on allocation, so a stale value cannot survive | a store scan over the image with a symbol map, or a runtime watchpoint |
| `0x71e8b0`'s value for the ground blow's query | the mesh test's gate `node+0xc4 > [0x71e8b0]` in the type-5 handler | `0x4c7600` sets it to `-1` when asked for "no limit" and `0x5b055d` restores `-1`; all M01 colliders store `node+0xc4 = 160` | read the setter's callers in the mission path |
| which `lod` band of an airframe the original draws per frame | not this task | the 87 `lod` nodes carry the bits and no mesh; their children are class 5 | the LOD selection work (#2054's neighbours) |

## Instruction bytes cited (re-read by the ignored test)

```bytes
4cc4f0: 8b0d14d4620083f9ff7514e860b000008b0d14d4620083f9ff0f84ec00000053558bc199568b3510d46200f7fe578be88bc199f7fea1187c72008b0ca833c08d1c528d1cdbd1e32bdac1e3028b9419d00000008d3419c1e208c1fa08891514d46200b9340000008bfef3aba1187c72008b0ca88bac19d000  # class.node_alloc
4d1330: 56e8bab1ffff8bf085f675206814cc620068350100006818d162006800040000e8db3ff4ff83c41033c05ec368900000006a01c7463405000000ff15b401a20056894638  # class.object3d_ctor
4e05e0: 56e80abffeff8bf06a5c6a01c7463406000000ff15b401a2  # class.lod_ctor
4d21b0: 57bf0100000068e801000057897e34ff15b401a2  # class.camera_ctor
4d849b: f74024000000040f85bb0000008b48344983f90a0f878f000000ff248d68854d0050e89efcffff83c4048bf05ec350e8  # class.copy_dispatch
4d8568: d6844d0024854d0044854d0044854d00bc844d00c9844d000a854d00fd844d00e3844d00f0844d0017854d00  # class.copy_table
4d7bcf: 8b4624c1e80283e0015057e8514effff8bd883c40885db742456566800d9620068750300006858d962006800040000e8  # class.info_copy
4e2964: 2bcf8d04498d04c0d1e02bc1c1e0028b4c24208b15187c7200516a01508b04aa50ff15f801a20083c41083f8  # class.slot_read
4e2ba0: 515355568b7424148b46345733ff83f80a897c24100f87c80600008b5c241c8b2d0002a200ff2485ac324e00  # class.loader_dispatch
4e32ac: e0314e00902d4e004a2f4e00fa2e4e009b2e4e00832c4e00bb2c4e0083324e0083324e00fd2c4e00cc2b4e00  # class.loader_table
4e2a7c: 8b467085c075088d5674895670eb1e83f801750b8d868c000000894670eb0e83f80275098d8ea4000000894e708b463c85c07c0503c389463c8b563c52e8b2d607008b4c24248946  # class.loader_postpass
4d045f: e8ccf4ffff89308b0cbdb8cf62008b0985c9750b8b14bdfc  # class.name_register
4d0320: a100eb710053555633d233ed85c0578bf8745c8b7424148b078a188acb3a1e751c84c974148a5801  # class.name_lookup
477850: 568b7424088b460885c075158b4604506a07e8198a050083c40885c0894608743d6a016a0150e8250d06008b  # class.plane_node_lookup
4d85a0: 8b44240485c0751f6814cc6200683f0600006858d9620068  # class.node_copy_entry
47c275: 56e8b53405008b483483c40483f90274108b15ac  # class.vehicle_type_read
47c32b: 85c0746c8b5624f6c60174648b4e3483f905751df60008754e83c030eb325f5e33c05b8b4df464890d000000008be55dc383f90175070580000000eb1383f9087525f6c2047420f6400404741a83c008  # class.vehicle_type_switch
62e6b0: 67436c73426c6f636b526561644e6f646528293a20556e7265636f676e697a6564206e6f646520636c61737320747970653a0a20206e6f6465203d20257320636c6173735f74797065203d2025640a004572726f722072656164  # class.message_unrecognized
62e354: 443a5c7a69707065725c67616d657a5c7a636c6173735c636c735f7a62642e63  # class.message_file
```

## Tests

Prefix `accept_groundblow_class_`, `crates/cs_content/tests/accept_groundblow_class.rs`:

* `accept_groundblow_class_rows_match_the_finding_and_the_table` (no original data): the
  finding's `bytes` block and the committed `[[groundblow_class_fact]]` rows name the same
  addresses and bytes; the object3d/lod/camera constructors' type constants decode from
  their own bytes; the loader's and the copy's dispatch windows hold
  `mov reg,[reg+0x34]`-shaped reads with the right table addresses; the loader table's
  per-kind record sizes equal the workspace's `*_DATA_BYTES` constants; `NODE_TYPE_OFFSET`
  is `0x34`; the post-pass window contains no store to `0x34`; and the loader's message
  calls the word `class_type`.
* `accept_groundblow_class_bytes_match_the_engine_image` (ignored, needs `CS_ENGINE_IMAGE`):
  loads the image through the production loader (`cs_content::coordinates::load_engine_image`,
  which refuses a drifted digest) and re-reads every row.
* `accept_groundblow_class_retail_m01_colliders_are_object3d` (ignored, needs
  `CS_GAME_DIR`): reads `ZBD/C3/gamez.zbd` and `ZBD/planes.zbd` through the production
  readers and re-derives every number of section 4 — the C3 histogram, the 2393 colliders
  all class 5, the 347 water nodes (all class 5, `zone_id = 1`, none with `0x20`), the 43
  box-path land nodes, the 435 flagged non-object nodes and their kinds, `g28530`/`g28603`,
  the airframe roots and `bloodhawk`'s subtree, and that no class-6 node binds a mesh.
