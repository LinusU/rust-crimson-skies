# The world segment query `0x4c8f70`, read (#2279)

Date: 2026-11-02. Task #2279, follow-up of #2278 and
`2026-11-02-m01-wingman-gains.md` §7.2-7.3. Implementer: `sonnet-2`. All
claims are `static`, read from the owner image (disassembled with capstone,
address convention `VA = file offset + 0x400000`). The original executable
was **not** run: nothing here is `verified_original`. The byte windows are
re-read by `accept_m01_ai_terrain_query_*`
(`crates/cs_app/tests/campaign/m01_ai_terrain_query.rs`, ignored without
`CS_ENGINE_IMAGE`, fails loudly when it is unset). The #2293 follow-up
section (§13) is re-read by `accept_m01_ai_terrain_contact_*`
(`crates/cs_app/tests/campaign/m01_ai_terrain_contact.rs`, same gating);
its retail-file claims are re-read there from `CS_GAME_DIR`.

## 1. Answer in one paragraph

`0x4c8f70` is a **segment-versus-objects query**, not a heightfield lookup. It
walks the segment from a start point to an end point across a 2-D grid of
cells in the (x, z) plane (y is the vertical axis and plays no part in the
walk), calls `0x4c9a00` for every object listed in each visited cell and
`0x4c9740` for the cell's static items, and appends one 0x2c-byte record per
object that the segment touches to a caller buffer whose first byte is the
record count. It returns `0` when the buffer holds at least one record and
`1` when it holds none (`5` for a null argument). The follower's look-ahead
(`0x41f810`) latches the escape climb when the return is `0` and the count
byte is non-zero. No height array is read anywhere in the walk: whether M01's
terrain is such an object, and what builds the grid, is **not** settled here
(§7).

## 2. Signature

`0x4c8f70(holder, x0, y0, z0, x1, y1, z1, buf)`, `cdecl`, eight stack words,
caller pops (`add esp, 0x30` at `0x41f958` after the 0x20 bytes of the
query). Fixed by the caller (`0x41f931`..`0x41f94a`): `holder = [0x71c2ac]`
(a world object; `grid = [holder+0x38]`), start = the vehicle position
(`[esi+0x204..+0x20c]`), end = start + velocity x 4.5 (`0x41f8b5`), `buf` =
the `0x8480`-byte local at `[ebp-0x8480]`.

The prologue (`0x4c8f70`..`0x4c8fce`) bumps the call tally `[0x63b2cc]`,
zeroes `buf[0]` (`mov byte [edi], 0`), and refuses a null holder or a null
`grid` with the assert text at `0x62cc14`/`0x62cbfc` (lines `0x854`/`0x855`)
and returns `5`.

It then stores the segment in globals the callees read:

| global | holds |
| --- | --- |
| `0x71e8b8/bc/c0` | start x, y, z |
| `0x71e8c4/c8/cc` | end x, y, z |
| `0x71e8d0` / `0x71e8dc` | min / max of the two x |
| `0x71e8d4` / `0x71e8e0` | min / max of the two y |
| `0x71e8d8` / `0x71e8e4` | min / max of the two z |
| `0x71e9f0` | `buf` (count byte at `[buf+0]`) |
| `0x71e9f4` | next free record, starts at `buf+4` |

and pushes onto the two small stacks `[0x6379dc]`/`[0x6379e0]` (a zero word
and a pointer to a 0x20-byte local initialised by `0x53a2a0`; popped by
`0x4c1550` at `0x4c9711`). What those stacks are for (a transform stack the
object tests use) is not decoded.

Mode globals set by other code before the call (not by the query itself):

| global | setter | meaning |
| --- | --- | --- |
| `0x71e8a8` | `0x4c75e0(n)` | stop-at-first-record mode. The follower sets it to 1 around the call (`0x41f920`) and back to 0 (`0x41f953`) |
| `0x71e8ac` | `0x4c75f0(n)` | enable the `[obj+0xc4]` filter against `[0x9be6f8]` |
| `0x71e8b4` | cleared at `0x4c9727` | skip-mask tested against `[obj+0x24]` in `0x4c9a00` |

Their defaults outside the follower's call are not measured here (they are
runtime globals; the image holds zeros).

## 3. The grid object `[holder+0x38]`

Offsets the walk reads (all `static`; units from the arithmetic):

| offset | use | evidence |
| --- | --- | --- |
| `+0x34`, `+0x38` | origin x, origin z | `(x0 - [+0x34]) * [+0x84]`, `(z0 - [+0x38]) * [+0x88]` (`0x4c91a1`, `0x4c91c1`) |
| `+0x44`, `+0x48` | far x, far z | the extent reject (below) |
| `+0x74`, `+0x78` | cell size x, z | next boundary `idx * [+0x74] + [+0x34]` (`0x4c94d8`), `idx * [+0x78] + [+0x38]` (`0x4c9511`) |
| `+0x84`, `+0x88` | inverse cell size x, z | as above; the sign trick at `0x4c923d`/`0x4c9288` xors the delta's sign with `[+0x84]`/`[+0x88]`, so `+0x88` may be negative |
| `+0x98`, `+0x9c` | columns, rows | `cmp esi, [+0x98]`, `cmp ecx, [+0x9c]` (`0x4c92da`, `0x4c92ee`) |
| `+0xa0` | row table | `[ [+0xa0] + row*4 ]` is the row's base (`0x4c92fa`..`0x4c9305`) |
| `+0x60` | wrap flag | non-zero: skip the extent reject and remap out-of-grid cells with `0x4da2e0` |
| `+0x64`, `+0x68` | wrap period x, z | `idiv [+0x64]`, `idiv [+0x68]` in `0x4da2e0` |
| `+0x6c`, `+0x70` | wrap tile origin x, z | added to the remainder for cells past the far edge |

Cell stride is `0x58`: `index*11*8` (`lea eax,[esi+esi*4]` / `lea ebx,[esi+eax*2]`
/ `shl ebx,3`, `0x4c92a5`). A cell is `rowbase + column*0x58`.

The extent reject (only when `[+0x60] == 0`, `0x4c9148`..`0x4c9192`):
the segment is dropped (jump to the exit) when its min x is not below `[+0x44]`,
its max x is below `[+0x34]`, its min z is above `[+0x38]`, or its max z is
not above `[+0x48]`. The z test reads `[+0x38]` as the **larger** z and
`[+0x48]` as the smaller: rows run towards decreasing z when `[+0x88]` is
negative. The sign of `[+0x88]` is not measured here.

Cell fields:

| offset | use |
| --- | --- |
| `+0x3a` | `int16` object count |
| `+0x3c` | pointer to the object entries, 12 bytes each, `entry[0]` = object pointer |
| `+0x40` | `int32` static item count |
| `+0x44` | pointer to the static items, 0x60 bytes each |
| `+0x50` | optional tree of 0x40-byte nodes over the items (§5) |

## 4. The walk (`0x4c9141`..`0x4c96e1`)

A standard grid DDA on (x, z):

1. Start cell: `col = ftol(floor-import((x0-ox)*invx))`, `row` likewise
   (`0x4c91b0`: import call `[0xa201dc]`, then the float-to-int helper
   `0x5f6e60`).
2. `dx = x1-x0`, `dz = z1-z0`. Zero delta (`fcomp 0.0`, C3): step `0`;
   otherwise step = `+1`/`-1` from the sign of `dx` xor the sign of
   `[+0x84]` and `1.0/dx` is kept (`[esp+0x30]`, `[esp+0x34]`).
3. Both "time to the next boundary" slots start at `2.0` (`0x40000000`).
4. Loop at `0x4c92d2`: if the cell is inside `[0,cols) x [0,rows)` visit it
   (step 5); if not and the wrap flag is set, remap it (step 7); if not and
   there is no wrap, just step on.
5. Visit: for each of the `[cell+0x3a]` entries take `obj = entry[0]`; skip
   unless `[obj+0x24]` bit `0x04` **and** bit `0x10` are set; in a wrapped
   copy also require bit `0x8000` (`0x4c9334`..`0x4c933e`); with `[0x71e8ac]`
   set skip objects whose `[obj+0xc4]` is above `[0x9be6f8]`; call
   `0x4c9a00(obj, count+1)`; in stop mode leave as soon as `buf[0] > 0`.
   Then, if `[cell+0x40] > 0`, call `0x4c9740(cell)` and apply the same
   stop test.
6. Step: next boundary parameter `t = (idx*size + origin - start) * (1/delta)`
   for each axis (`idx` is `col` or `col+1` by the step sign). The walk ends
   when both `t` are above `1.0` (`0x6032dc`). Otherwise the smaller `t`
   decides the axis; on a tie `col` advances first and the next iteration
   re-evaluates.
7. Wrapped cells (`0x4c9583`..`0x4c96b7`): `0x4da2e0(holder, col, row,
   &col', &row', &inside)` returns the folded cell; the segment's nine stored
   floats are shifted by the whole number of tile lengths that were removed
   (`dcol*[+0x74]`, `drow*[+0x78]`) so the cell's objects are tested in their
   own tile frame, and the shift is undone after the cell (`0x4c93c8`..
   `0x4c945e`, which also shifts the records already written).

At the exit (`0x4c96e6`): when not in stop mode with a hit, the holder's own
children (`word [holder+0x56]` count, `[holder+0x5c]` array) are tested through
`0x4c9980`, which applies the same bit-4, bit-0x10, category and `0xc4`
filters and calls `0x4c9a00`; then the matrix stack is popped, `[0x71e8b4]` is
cleared and the function returns `(buf[0] == 0)`: **`0` = something hit, `1`
= nothing hit** (`sbb`/`inc` at `0x4c972b`..`0x4c9730`).

## 5. `0x4c9740`: the cell's static items

`0x4c9740(cell)`: when `[cell+0x50]` is set it calls `0x4c9880(cell, 0)`, a
walk of a tree of 0x40-byte nodes (`node = [cell+0x50] + index<<6`). A node
with `[node+0x3c] >= 0` is an interior node: it is first tested with
`0x4cbf80(node+0xc)` (a box test against the segment globals) and, if that
passes, its four children `[+0x3c]+0..3` are visited in order (a quad tree
over the items). A leaf (`[node+0x3c] < 0`) holds `[node+0x34]` item indices
in the array at `[node+0x38]`; each index selects `[cell+0x44] + index*0x60`.
Without a tree the items `[cell+0x44] + 0x60*i`, `i < [cell+0x40]`, are
visited linearly (`0x4c9791`: `add edi, 0x60`). Each item goes to
`0x4c97a0(cell, item)`.

`0x4c97a0` is the item filter: `item[0]` is an object; it requires
`[obj+0x24]` bits `0x04` and `0x10`, `[item+0x48]` bit 0 **clear**, and the
same `[0x71e8ac]` filter; it copies `item+0x28..0x30` (a vector) to
`obj+0x60`, for a type-5 object also sets up the object's bounds matrix (the
`0x53cef0`/`0x4d8bd0` calls), links `obj+0x40 = item` and `obj+0x30 = item+0x40`,
calls `0x4c9a00(obj, [cell+0x40]+1)` and clears `obj+0x40` again. A static item is
thus tested as its object at the item's own placement.

## 6. `0x4c9a00(obj, depth)`: the object test

Gates, in order (each failing gate returns `1` = keep walking):
`[obj+0x24]` bit `0x04`, bit `0x10`; `[obj+0x24] & [0x71e8b4]` must be zero;
in stop mode with a record already present it returns `0` (stop); the
category byte `[obj+0x30]` through `0x56c430` must be accepted (the table at
`0x62cb50`; byte `0xff` and an empty table accept everything); the
`[0x71e8ac]` filter. It then tallies (`0x63b2d4`), clears `[obj+0x24]` bit
`0x02000000` (`0x4c9a8f`), asserts `buf[0] < 0x20` (assert text `0x62cbb0`,
line `0xa62`, so **at most 32 records**) and dispatches on `[obj+0x34] - 1`
through the ten-entry table at `0x4c9de0`:

| `[obj+0x34]` | handler | reading |
| --- | --- | --- |
| 1 | `0x4c9d89` -> `0x4c9fe0` | not decoded |
| 2, 3, 4 | `0x4c9daf` | unknown-type assert (`0x62cb54`, line `0xa90`), returns `3` |
| 5 | `0x4c9adf` | bounds test then record (below) |
| 6 | `0x4c9c72` | gated by `[obj38+4] > 5.0`; bounds test; then children |
| 7 | `0x4c9d12` | selects child `[obj38+0x14]` of `[obj38+0x20..]`; bounds test; recurse |
| 8 | `0x4c9d76` -> `0x4c9e10` | reports objects carrying `[obj+0x24]` bit `0x20`; not fully decoded |
| 9 | `0x4c9d9c` -> `0x4ca140` | not decoded |
| 10 | `0x4c9dd7` | returns its depth argument |

The common bounds test is `0x4cc010(obj)`: it returns `1` (reject) unless
`[obj+0x24]` bit `0x100` is set, builds the object's world bounds (`0x4ce1b0`,
or the eight-corner path `0x4cd9d0` when bit `0x20` is set) and compares them
with the segment's min/max globals `0x71e8d0..0x71e8e4`; it returns `0` when
they overlap. **It is an axis-aligned box overlap with the segment's
bounding box, not an exact segment-versus-surface intersection**, at least at
this stage; the exact test for type 5 is `0x55c9c0` (called with the segment
globals, `0x4c9bc9`) only when `[obj+0x3c]` and `[obj+0xc4] > [0x71e8b0]`.
That routine is not decoded here.

When an object is accepted the record is written by `0x4c7f50(obj)`:
`rec = [0x71e9f4]`; `[rec+0x24] = obj`; `[rec+0x28] = [obj+0x40]` (or the
`[obj+0x40]` of the first flagged ancestor when bit `0x200000` is set);
`[0x71e9f4] += 0x2c`; `buf[0] += 1`. Bytes `rec+0x00..0x23` are the contact
data written by the hit routines (not decoded). Layout of the buffer:

```
buf[0]               u8 record count (at most 0x20)
buf+4 + 0x2c * i     record i: +0x24 object pointer, +0x28 owner/item word
```

## 7. `0x4da2e0` and the data question

`0x4da2e0(holder, col, row, &col', &row', &inside)` (`cdecl`, six words)
folds an out-of-range cell into a tile. `*inside` starts at `1` and becomes
`0` when either coordinate was outside:

* `col < 0`: `col' = [+0x64] - ((-1 - col) mod [+0x64]) - 1` (mirrored into
  `[0, [+0x64])`).
* `col >= [+0x98]`: `col' = [+0x6c] + ((col - [+0x98]) mod [+0x64])`.
* otherwise `col' = col`. Rows are the same with `[+0x68]`, `[+0x9c]`,
  `[+0x70]`.

It is a **wrap/tiling** device: outside the declared grid the walk sees the
cells of a repeating tile (period `[+0x64]` x `[+0x68]`), so an object in the
tile is tested at every repetition. It is only reached when `[grid+0x60]`
is non-zero. When it is zero the extent reject returns "nothing hit" for any
segment wholly outside the grid.

**World data.** Nothing in the walk reads a height array: every contact is an
object (types 5-8 above) with a bounds box and flags. The builder of
`[holder+0x38]` (the code that fills `+0x34..+0xa0` and the cell lists) was not
located: a scan of `.text` for stores to `+0x98/+0x9c/+0xa0` finds only
unrelated structures (`0x4032bd`..`0x4032c9`, `0x40170c`, `0x402b53`). So
which original files feed the grid, and whether M01's hillside is a type-5
object, a static item or something else, is **unknown**. In the port,
`cs_content::world` models `WorldObjectInstance`s with an `Aabb` and sectors
but has no cell grid, no row table, no per-object flag word and no segment
query; `cs_assets` has no loader for the grid. Nothing loads this today.

## 8. What this means for the port

* Behaviour a port can reproduce from this decode: the segment (position,
  position + 4.5 x velocity), the (x, z) cell walk and stop-at-first-record
  mode, the object gates (bits 4, 0x10, 0x8000 in a wrapped copy), the cap of
  32 records and the `0`/`1` return. These are enough to write a query over
  *a set of placed object boxes* if the owner paths can supply them.
* Not reproducible yet: the exact contact test (`0x55c9c0`, `0x4cbf80`,
  `0x4c9fe0`, `0x4c9e10`, `0x4ca140`), the contents of the grid for M01, and
  the world floor/ceiling globals `[0x71c3f0]`/`[0x71c3f4]` (still open from
  gains §7.3).
* A port that approximates the probe with the runtime terrain must say so and
  must not call it the original query.

| unknown | affects | resolving step |
| --- | --- | --- |
| builder of `[holder+0x38]` and its source files | whether the hillside is in the grid | find the writers of the cell lists (indirect stores); read the world load |
| exact tests `0x55c9c0`, `0x4cbf80`, `0x4c9fe0`, `0x4c9e10`, `0x4ca140` | what counts as a hit | decode each |
| sign of `[+0x88]`, wrap flag `[+0x60]` for M01 | row direction, outside-grid behaviour | the builder or a capture |
| defaults of `0x71e8ac`/`0x71e8b4`/`0x9be6f8` | filter in non-follower calls | their setters |
| `[0x71c3f0]`, `[0x71c3f4]` | the floor/ceiling latch | world setup |

Nothing here is `verified_original`.

---

# §9-§12: the builder, the data, and M01's hillside (#2290)

Date: 2026-11-02. Task #2290 (`M01-AI-TERRAIN-GRID-BUILDER`), follow-up of
#2279/#2281. Implementer: `deepseek-1`. Same conventions as above: `static`
claims read from the owner image with capstone (`VA = file offset + 0x400000`),
retail claims read from the owner installation with the workspace's own
readers; the original executable was **not** run, so nothing here is
`verified_original`. Sections 9-12 **supersede** the last paragraph of §7, the
statement in §3 that the grid's builder is unknown, and the first, third and
fifth rows of §8's table. The byte windows are re-read by
`accept_m01_ai_terrain_grid_*`
(`crates/cs_app/tests/campaign/m01_ai_terrain_grid.rs`, ignored without
`CS_ENGINE_IMAGE` / `CS_GAME_DIR`).

## 9. The builder: `cls_world.c`'s area partition, not a heightfield

`[holder+0x38]` is not a world-only grid: the same two asserts the query
raises (`0x62cc14` "Null node pointer." for a null holder, `0x62cbfc` "Null
class data pointer" for a null `[holder+0x38]`) are `Class.c`'s
node/class-data pair, so `holder` is a GameZ **node** and `[holder+0x38]` is
*that node's class data*. For the node the mission calls `world1` the class
data is an **area partition** (`cls_world.c`, the source string at
`0x62ddd0`), and the query walks its cells.

The partition is created by `0x4daa20(node, cell_size_x, cell_size_z)`, and
its parameters are set by seven mission-script commands. The command table is
`Script.c`'s native dispatcher (the string at `0x63e27c` and onward); each
line below is the pushed length + name pointer and the handler the branch
calls:

| script command (name VA, length) | handler | stores |
| --- | --- | --- |
| `WorldOrigin` (`0x63e328`, 11) | `0x4da960(node, x, z)` | `+0x34 = x`, `+0x38 = z`; far x/z `+0x44/0x48 = origin + width/depth`; padded bounds `+0x4c..+0x58` |
| `WorldExtents` (`0x63e334`, 12) | `0x4da9c0(node, w, d)` | `+0x3c = w`, `+0x40 = d`; far x/z and padded bounds |
| `WorldPartition` (`0x63e2b8`, 14) | `0x4daa20(node, sx, sz)` | the whole grid (below) |
| `WorldPartitionInclusionTolerance` (`0x63e304`, 32) | `0x4dadf0(node, px, pz)` | `+0x90 = px`, `+0x94 = pz`; recompute padded bounds |
| `WorldPartitionMaxDECFeatureCount` (`0x63e2e0`, 32) | `0x4dae40(node, n)` | byte `+0x5c = min(n, 255)`, message `0x62de24` above 255 |
| `WorldSetVirtualPartition` (`0x63e154`, 24) | `0x4da150(node, flag)` | `+0x60 = flag`; if non-zero, `0x4da070(node)` and `0x4da180(node, [+0x64], [+0x68])` |
| `WorldSetVirtualPartitionMapSize` (`0x63e170`, 31) | `0x4da180(node, x, z)` | `+0x64 = x`, `+0x68 = z`, `+0x6c = cols - x`, `+0x70 = rows - z` |
| `WorldPartitionSetActive` (`0x63e2c8`, 23) | `0x4db790(node, flag, x0, z0, x1, z1)` | no membership change: it clamps the rectangle to cell indices and calls `0x4cca30(node, flag)` for every node listed in those cells (the class "active" bit) |

`0x4daa20` in order (each step is one of the windows the task test re-reads):

1. if `[grid+0xa0]` is set, destroy the old partition with `0x4dacb0(node)`;
2. `+0x74 = cell_size_x`, `+0x78 = cell_size_z`; `+0x90 = cell_size_x *
   [0x60802c]` and `+0x94 = cell_size_z * [0x608dc8]` (the default inclusion
   tolerance; the constants are `0.125` and `-0.125`), then recompute the
   padded bounds `+0x4c..+0x58` from them;
3. `+0x7c = cell_size_x * 0.5`, `+0x80 = cell_size_z * 0.5`;
4. `+0x84 = 1.0 / cell_size_x`, `+0x88 = 1.0 / cell_size_z` (so a negative z
   cell size really is allowed: the C3/M01 data uses `-1024`);
5. `+0x8c` = the fast-inverse-square-root half diagonal, `-0.5 *
   f(cell_size_x^2 + cell_size_z^2)` (`sar eax,1; add eax,0x1fc00000` then
   `* [0x6034f8]` = `-0.5`), i.e. `-768` for a 1024 m cell;
6. `+0x98 = ceil([+0x3c] / [+0x74])` columns, `+0x9c = ceil([+0x40] /
   [+0x78])` rows, each refused above `0x7fff` with the message at
   `0x62ddf4` ("Error: too many world area partitions: %d %d.");
7. `[grid+0xa0] = calloc(rows, 4)`, then `calloc(cols, 0x58)` per row — the
   row table is `+0xa0`, the cell stride `0x58`, and the allocation is zeroed;
8. for every cell: `[cell] |= 0x100`, `[cell+0x04] = -1`, the min corner at
   `+0x08` (x) / `+0x0c` (z) from `origin + col*size`, the same corner at
   `+0x10/+0x24`, the far corner at `+0x18` (z) / `+0x1c` (x), then
   `0x4d8b50(cell+0x10, cell+0x28, cell+0x34)` derives the cell's AABB in the
   fields the hit test `0x4cbf80` reads.

Filling. Two paths exist and they agree in the retail data:

* `0x4db0c0(world, node)` — insert a node. The node's bounds come from its
  class data by node type (`5` -> class data `+0x30`, `1` -> `+0x80`, `8` ->
  `+0x08`, else the world origin/extents pair `[0x75d1b8..c0]`), the cell
  rectangle is computed by `0x4da500(world, &box, &range)` (grow the box by
  `+0x90/+0x94`, `floor((v - origin) * inverse)`, clamp to the padded bounds,
  sort min/max), and `0x4dae80(world, flags_word, node, &range)` inserts it.
  A node whose `[node+0x24]` has bit `0x80` is inserted over the **whole**
  grid (`-1 .. cols-1`, `-1 .. rows-1`, `0x4db1d5`).
* `0x4dae80` — for a single cell it `realloc`s `([cell+0x3a] + 1) * 12` bytes
  at `[cell+0x3c]`, writes `entry[0] = node` and the box's two y words to
  `entry[1]`/`entry[2]` (measured on the retail file: `entry[1]` is the box's
  min y and `entry[2]` its max y), increments the `int16` count at
  `[cell+0x3a]`, and
  caches the cell at `[node+0x4c..+0x58]` (cell x/y and the extra-cell list
  for a node that spans several cells). The 12-byte entry is therefore
  `(node, min y, max y)`: measured on the retail file, a flag node at
  `y = 113.54` stores `103.98 .. 123.10` and a terrain group stores exactly
  the y range of its subtree's mesh.

The reader takes the shipped grid from the file rather than inserting nodes:
`gClsReadNode` (`cls_zbd.c`) at `0x4e2f4a` runs the world case — `malloc(0xcc)`
and read the 204-byte header, read the light/sound arrays at `+0xb0/+0xb4`
and `+0xbc/+0xc0`, `calloc([+0x9c], 4)` the row table, `calloc([+0x98],
0x58)` each row, then per cell read `0x58` bytes and, when `[cell+0x3a] > 0`,
`malloc(count * 12)` at `[cell+0x3c]` and `0x4e3360` reads the entries,
turning each entry's first word from a node index into a node pointer and
keeping words 1 and 2 as stored. `0x4e1be0` is the matching writer. So the
partition contents are **authored/saved data**, not a load-time build; the
builder is the script path that produced the saved numbers in the first place
and that a script-authored world would use.

## 10. Which retail data feeds it (C3/M01)

**Parameters: `ZBD/INTERP.ZBD`, script `support\c3\load.gw`.** The container
is a plain INTERP image (signature `0x08971119`, version 7, 98 scripts); its
script 61 (`support\c3\load.gw`, entry offset 7820, script offset 112696)
holds, token for token and at these absolute offsets:

| line | data offset | size | tokens |
| --- | --- | --- | --- |
| 4 | 112847 | 25 | `WorldOrigin` `-16384.0` `0.0` |
| 5 | 112880 | 30 | `WorldExtents` `16384.0` `-16384.0` |
| 6 | 112918 | 26 | `WorldPartition` `1024` `-1024` |
| 7 | 112952 | 41 | `WorldPartitionInclusionTolerance` `3.0` `3.0` |
| 11 | 113100 | 28 | `WorldSetVirtualPartition` `on` |
| 12 | 113136 | 36 | `WorldSetVirtualPartitionMapSize` `1` `1` |

Script 62 (`support\c3\m01.gw`, line 1 at offset 120825) then calls
`WorldPartitionSetActive off -15360 -7168 -8192 -14336`: M01 turns the class
"active" bit **off** in that rectangle. `support\c3\ia1.gw` (script 59) turns
three other rectangles on; none of these calls change cell membership.

**Contents: `ZBD/C3/gamez.zbd`, the world node's own record.** Node 0 is
`world1`, `node_type` 2, data offset `4808244` (`0x495e34`), 28 060 bytes:
a 204-byte header, one `u32`, the 16 x 16 grid and 14 child slots. Measured
fields and their agreement with the script:

| header field | offset | value | agrees with |
| --- | --- | --- | --- |
| origin x, origin z | `+0x34`, `+0x38` | `-16384.0`, `0.0` | `WorldOrigin` |
| width, depth | `+0x3c`, `+0x40` | `16384.0`, `-16384.0` | `WorldExtents` |
| far x, far z | `+0x44`, `+0x48` | `0.0`, `-16384.0` | origin + extents |
| cell size x, z | `+0x74`, `+0x78` | `1024.0`, `-1024.0` | `WorldPartition 1024 -1024` |
| half cell | `+0x7c`, `+0x80` | `512.0`, `-512.0` | builder step 3 |
| inverse cell | `+0x84`, `+0x88` | `1/1024`, `-1/1024` | builder step 4 |
| half diagonal | `+0x8c` | `-768.0` | builder step 5 |
| tolerance x, z | `+0x90`, `+0x94` | `3.0`, `3.0` | `WorldPartitionInclusionTolerance 3.0 3.0` |
| max DEC features | `+0x5c` (byte) | 16 | not set by `support\c3\load.gw`; default or earlier data |
| virtual partition | `+0x60` | 1 | `WorldSetVirtualPartition on` |
| map size / tile origin | `+0x64/+0x68`, `+0x6c/+0x70` | 1/1, 15/15 | `WorldSetVirtualPartitionMapSize 1 1` (`15 = 16 - 1`) |
| columns, rows | `+0x98`, `+0x9c` | 16, 16 | `ceil(16384 / 1024)` |

The cell walk is exact over the whole block: 256 cells of `0x58` bytes, each
cell's own x/z min corner equals `origin + index * size` for every cell, and
`16*16*0x58 + 439*12 = 27 796` bytes is exactly `partition_bytes`; the block
ends exactly at the record's child slots. There are 439 entries in 256 cells;
no node occupies more than one cell in this container.

Every entry's first word is a node's **info-array index**, and the node's own
208-byte info record stores the same cell in the reference's `AreaPartitionPm`
(`[i16; 4]` = cell x, cell y, virtual x, virtual y): for all 439 entries the
listed cell `(row, col)` equals `AreaPartitionPm` with
`x = col`, `y = row`, `virtual_x = col`, `virtual_y = row`. A node with
`AreaPartitionPm == (-1, -1, 0, 0)` is in no cell.

Which flag bits decide membership: the file's `flags` word of every node that
is listed in a cell has bits `0x04` and `0x10` set (the query's two required
bits, §4) and 430 of the 439 also carry `0x8000` — the bit
`WorldSetVirtualPartition` sets on every cell object (`0x4da070`, the
`0x4dae80`-time marker the wrapped copy requires at `0x4c9334`). **No node
outside the grid carries `0x8000`**, and the nine grid nodes without it are
cell members by the file's own entry list. The runtime insertion gate is
separate: `[node+0x24] & 0x80` (bit 7) sends a node to the whole grid
(`0x4db0cd`, `0x4cea31`), so bit `0x8000` marks "has partition coordinates"
while bit `0x80` means "spans everything".

**The floor/ceiling globals `[0x71c3f0]`/`[0x71c3f4]`: #2277 is confirmed.**
Inside the mission/world setup `0x4735b0`, `0x474151` stores `0x45fa0000`
(`8000.0`) into `[0x71c3f4]` and `0x47415b` stores `0x41a00000` (`20.0`) into
`[0x71c3f0]`, immediately before the setup's config lookups. A scan of the
whole `.text` for writes to those two words finds only this pair and the
save/restore in `0x4216e0` (which sets `[0x71c3f0]` to `0x7f7fffff` and the
vehicle ceiling `+0x314` to `0x7f7fffff` around one call to `0x41b560`, then
restores both). So the world floor is **20 m** and the ceiling **8000 m**, and
nothing in the image overrides them for a normal mission frame.

## 11. M01's hillside is in the grid; the shipwreck ring and the cliffs are not

The hillside the wingman crosses in
`accept_m01_wingman_stick_betty_over_the_hillside` is the terrain of cell
**(row 4, col 12)** — x in `[-4096, -3072]`, z in `[-5120, -4096]`. Betty's
flight (port trace with `CS_STICK_TRACE`, 120 Hz, `original_stick` and the
merged `kind_of` gains) descends from `(-3969, 97, -4614)` at t = 4620 to
`(-4035, 8, -4605)` at t = 5400 and stays inside that cell while the escape
climb latches; the port is the only source for the positions, and the cell is
computed with the retail grid's own origin and cell size, so the *cell index*
is the retail fact.

Cell (4, 12) and its neighbour (5, 12) hold, by the file's own entry list:

| cell | entry | name | mesh | listed y range |
| --- | --- | --- | --- | --- |
| (4, 12) | 829 | `g28596` | 516 | ~0 |
| (4, 12) | 871 | `g28471` | 559 | ~0 |
| (4, 12) | 2874 | `g28604` | group (`-1`), child 2882 `g28982` carries mesh 762 | `-3.57 .. 180.47` |
| (5, 12) | 830 / 881 | `g28598` / `g28474` | 517 / 562 | ~0 |
| (5, 12) | 2873 | `g28603` | 761 | `-16.71 .. 287.94` |

The entry's two floats equal the mesh's own local y range exactly
(`g28982`: `-3.5746138 .. 180.47455`; `g28603`: `-16.713 .. 287.93655`), so the
listed range is the node subtree's y extent, not a cell bound. The hillside
is therefore an ordinary partition object: the original's look-ahead walks
that cell, tests the listed object's AABB against the segment box and can
latch the escape climb there. It needs no heightfield to do so.

The contrast is the shipwreck's mountain ring, node slot 2994 `g28637` (1024 m
tile, mesh 795, y up to 144.6 m) and the east cliffs slot 2965 `g28911`
(mesh 776): both have `AreaPartitionPm = (-1, -1, 0, 0)`, flags word
`0x308031c` without `0x8000`, and **no cell in the file lists them**. They are
drawn (they hang under the world's direct children) but the AI terrain query
can never return them.

## 12. What this settles for the port, and what it does not

* **Settled (static + retail):** the query's `[holder+0x38]` is a world node's
  area partition; its builder is `0x4daa20`; its parameters are the world
  script's `World*` commands (`ZBD/INTERP.ZBD`, `support\c3\load.gw`); its
  contents are the world record's own 16 x 16 grid in `ZBD/C3/gamez.zbd`,
  with `(node index, subtree min y, subtree max y)` per entry and the cell
  mirrored in each node's `AreaPartitionPm`; C3/M01's cell size is
  `1024 x -1024` over `x in [-16384, 0]`, `z in [-16384, 0]`; the hillside is
  in cell (4, 12); the mountain ring and the east cliffs are not in the grid;
  the floor/ceiling writer is `0x474151`/`0x47415b` (8000 / 20).
* **Not settled (here; resolved in §13):** whether `0x4c9a00` records the
  hillside group as a hit depended on the type-5 handler's exact tests
  (`0x4cc010`, `0x55c9c0`) and on the group handlers for node types 6/7.
  §13 decodes them: the hillside members take a triangle-exact mesh test
  behind an AABB pre-reject. The port's collider-face approximation (#2281)
  is in the right direction at the hillside (the original does see that
  geometry) and in the wrong direction at the mountain ring and cliffs (the
  original never sees them); no production behaviour is changed by this
  task and `original_stick` stays off.
* **Not settled:** the nine grid nodes whose flags word lacks `0x8000` may be
  skipped when the query walks a folded (out-of-grid) copy of their cell; the
  file lists them, the wrapped-copy gate at `0x4c9334` would not. None of them
  is in cell (4, 12).
* **Not settled:** whether the runtime `[node+0x24]` equals the file's `flags`
  word bit for bit. Bits `0x04`, `0x10`, `0x80`, `0x8000` and `0x02000000`
  line up between the two readings, which is the extent of what is measured.

| unknown (remaining) | affects | resolving step |
| --- | --- | --- |
| exact tests `0x55c9c0`, `0x4cbf80`, `0x4c9fe0`, `0x4c9e10`, `0x4ca140` | does a listed terrain object count as a hit | **decoded in §13** |
| defaults of `0x71e8ac`/`0x71e8b4`/`0x9be6f8` | filter in non-follower calls | their setters (`0x71e8b0` decoded in §13.9) |
| runtime `[node+0x24]` vs file `flags` for the nine `0x8000`-clear grid nodes | folded-copy visibility of 9 scenery nodes | a capture or the flag fixup in `gClsReadNode` |

Nothing here is `verified_original`.

## 13. The per-type contact tests, decoded (#2293)

Follow-up of #2290/#2292. Implementer: `swe2-max-1`. Same convention as the
rest of this file: `static` reads of the decrypted image, the original was
not run, byte windows re-read by `accept_m01_ai_terrain_contact_*`. Retail
file facts (node types, `mesh_index`) are read from
`$CS_GAME_DIR/ZBD/C3/gamez.zbd` with `cs_formats`' `RawNodeInfo` layout.

### 13.1 The hillside members are all node type 5

Every member the query can meet in cells (4, 12) and (5, 12) — §11's table —
is `node_type = 5` (`NODE_TYPE_OBJECT3D`), including the group `g28604` and
its child `g28982` (retail file, `static`):

| slot | name | type | flags | `mesh_index` |
| --- | --- | --- | --- | --- |
| 829 | `g28596` | 5 | `0x0308831c` | 516 |
| 871 | `g28471` | 5 | `0x0308831c` | 559 |
| 2874 | `g28604` | 5 | `0x0308851c` | `-1` (no own mesh; child 2882 `g28982` has 762) |
| 830 | `g28598` | 5 | `0x0308831c` | 517 |
| 881 | `g28474` | 5 | `0x0308831c` | 562 |
| 2873 | `g28603` | 5 | `0x0308851c` | 761 |

So the only dispatch case the hillside can take is `0x4c9a00`'s case 5
(`0x4c9adf`). The other decoded handlers matter only where the same code
shares the callee: types 1, 8 and 9 run the same exact test `0x55c9c0`
under their own transform pushes.

### 13.2 `0x4c9a00` prologue and the second argument

The dispatch is on `[obj+0x34] - 1` through the table at `0x4c9de0`
(pinned by `accept_m01_ai_terrain_query_`). The shared prologue loads
`bl = 0x20` (`mov bl, 0x20` at `0x4c9a9d`) — the "box test suffices" mask —
before the switch. The call's second argument is **not a depth**: the cell
walk passes the entry's ordinal + 1 (`inc edx; push edx` at `0x4c9357`,
entry stride 12 at `0x4c9384`), and the child loops pass the parent's
child count (`movsx edx, word [esi+0x56]` at `0x4c9c4c` / `0x4c9fb3`). The
AABB-stage handlers share this gate on it — types 5, 8 and 9 open with it,
types 6/7 reach the same shape after their own pre-gates (§13.8), and the
type-1 handler has no `0x4cc010` stage at all:

```asm
004c9ae5  mov eax, dword ptr [esp + 0x18]   ; arg2
004c9aef  cmp eax, ebp                      ; ebp = 1
004c9af7  jg  0x4c9b00                      ; arg2 > 1 -> 0x4cc010 AABB first
004c9af9  mov eax, dword ptr [esi + 0x24]
004c9afc  test bl, al                       ; [obj+0x24] bit 0x20?
004c9afe  je  0x4c9b28                      ; clear -> exact path, no AABB
```

A listed object's **first** cell entry (`arg2 = 1`) therefore skips the
bounds pre-reject for flag-`0x20`-clear objects and goes straight to the
exact test; later entries (`arg2 >= 2`) always run `0x4cc010` first. After
the AABB passes, the same `test bl, al` runs again (`0x4c9b14`): objects
whose `[obj+0x24]` low byte carries `0x20` are recorded on the box alone
(`0x4c7f50` at `0x4c9b18`); `0x20`-clear objects take the exact path.

The hillside members all have low flags byte `0x1c` — `0x20` clear — so each
of them reaches the triangle test.

### 13.3 `0x4cbf80`: strict AABB vs segment box overlap

`0x4cbf80(box)` is a leaf routine (`static`): it compares the segment's
stored min/max globals against the 0x18-byte `box` in the order
`(max_x, min_x, max_y, min_y, max_z, min_z)`, strict `fcomp` inequalities,
returning 0 on the first separated axis:

```asm
004cbf80  mov  eax, [0x63b35c]
004cbf85  fld  [0x71e8dc]        ; seg max x
004cbf8b  mov  ecx, [esp + 4]
004cbf8f  inc  eax
004cbf90  mov  [0x63b35c], eax
004cbfa1  fld  [0x71e8d0]        ; seg min x  vs  [box + 0xc]
004cbfb4  fld  [0x71e8e0]        ; seg max y  vs  [box + 4]
004cbfc7  fld  [0x71e8d4]        ; seg min y  vs  [box + 0x10]
004cbfda  fld  [0x71e8e4]        ; seg max z  vs  [box + 8]
004cbfed  fld  [0x71e8d8]        ; seg min z  vs  [box + 0x14]
```

(`[0x63b35c]` is a call tally like the other `0x63b2xx` counters.) The box
layout is `min(x,y,z)` at `+0..+0xb`, `max(x,y,z)` at `+0xc..+0x17`.
Contact on a face alone does not overlap (strict `>`), so a grazing segment
is not a hit. Its only call site is `0x4c98a9`, inside the quadtree walker
`0x4c9880` (`§5`); the object bounds path inlines the same compares inside
`0x4cc010` instead of calling it (§13.4).

### 13.4 `0x4cc010`: which bounds the object is tested against

`0x4cc010(obj)` (`static`) decides whether the object's bounds test applies
and which form it takes:

```asm
004cc010  mov  edx, [0x63b35c]
004cc02b  test ah, 1             ; [obj+0x24] bit 0x100 required
004cc02e  jne  -> run test
          ...  bit clear -> return 1 ("rejected")
004cc03a  test al, 0x20          ; [obj+0x24] bit 0x20
004cc03c  jne  0x4cc055
004cc044  call 0x4ce1b0          ; -> one world AABB
004cc055  call 0x4cd9d0          ; -> eight transformed corners
```

* `0x4ce1b0` first tries the stored 6-float AABB at `[obj+0x70]` (the file
  calls that region `unk112`/`unk116`; which stored box lands there is a
  runtime detail this reading does not settle) and otherwise evaluates the
  bounds of the node's data record (`edi = [obj+0x38]`).
* `0x4cd9d0` has the same assert skeleton and computes the box's eight
  corners transformed by the current matrix stack — for objects whose
  flag word already carries `0x20` (the "box suffices" bit) it still
  measures the full transformed box.
* The overlap test itself is **inlined** in `0x4cc010` — an image-wide
  `e8`/`e9` scan shows no call edge from `0x4cc010` (or either provider)
  to `0x4cbf80`, whose only caller is the quadtree walker at `0x4c98a9`.
  A seven-count loop (`mov edx, 7` at `0x4cc093`, `add ecx, 0xc; dec edx;
  jnz` at `0x4cc112`) first merges the returned box componentwise against
  a second stack box — the min/max selects look like an intersection or
  clamp; which operand is which is not decoded — and then runs the same
  strict `fcomp` shape `0x4cbf80` uses: six comparisons of the merged box
  against the segment globals `0x71e8d0`..`0x71e8e4`
  (`0x4cc11e`..`0x4cc1b6`), `return 1` on the first separated axis.
* For flag-`0x20` objects whose box overlaps, `0x4cc010` additionally
  calls `0x55d270(&local_box, rec = [0x71e9f4], &seg_start, &seg_end)` at
  `0x4cc1da` — a segment-vs-box contact writer that clears `rec+0x20`
  (the surface word; a box face has no material) and copies the box into
  the same `0xa05e84` scratch the triangle tester uses. Nonzero returns
  `0`; zero rejects. `0x20`-clear objects skip that call (`test byte
  [esi+0x24], 0x20; je` at `0x4cc1be`) and return `0` so the caller can
  run the exact test.

So the broad phase is a world-space **AABB overlap**, optionally of the
transformed corner box — never a sphere and never a heightfield — and a
flag-`0x20` object's hit record comes from an exact segment-vs-box test
inside `0x4cc010`, not from the AABB alone.

### 13.5 Type 5 end to end (`0x4c9adf`..`0x4c9c71`)

Putting the case together (`static`):

1. `arg2 > 1` or flag `0x20` → `0x4cc010`; reject on nonzero.
2. Flag `0x20` set → `0x4c7f50(obj)` completes a record the box test
   inside `0x4cc010` already filled (§13.4) — no triangle test — return 0.
3. Flag `0x20` clear → `edi = [obj+0x38]` (the node's data record). If
   `[edi]` bit 8 is set the transform stage is skipped (`ebp = 0`, no pop
   owed). Otherwise a transform level is pushed, with `[obj+0x24]` bit
   `0x80000` selecting the form: bit clear → `0x539270(edi + 0x60)` +
   `0x53a3c0(edi + 0x30, 3)`; bit set and `[edi]` bit `0x20` set →
   `0x539270(edi + 0x60)` + `0x53a3c0(edi + 0x30, 1)` followed by a
   consume-once clear of `[edi]` bit `0x20` that runs only when `[edi]`
   bit `0x01` is also clear (`test al, 0x01; jnz` skips the
   `and eax, ~0x20; mov [edi], eax` at `0x4c9b55`..`0x4c9b5e`);
   bit set and `[edi]` bit `0x20` clear → the stored matrix is pushed
   directly (flag word `0`, matrix pointer `edi + 0x60`,
   `0x4c9b60`..`0x4c9b85`). `0x539270` pushes a (flag, matrix-pointer)
   pair onto `0x6379dc`/`0x6379e0`; `0x53a3c0` fills it.
4. `eax = [obj+0x3c]` — the node's resolved mesh record, the file's
   `mesh_index` slot patched to a pointer at load (the patch site is not
   pinned; `-1` in the file implies null here). Zero, or
   `[obj+0xc4] <= [0x71e8b0]` (`0x4c9ba2`..`0x4c9bb5`), skips the exact
   test. `[obj+0xc4]` is the file's `unk196`, asserted `160` for
   object/LOD nodes.
5. `0x55c9c0(geo = [obj+0x3c], rec = [0x71e9f4], &0x71e8b8, &0x71e8c4)`
   — the last two args are pointers to the stored segment start/end
   globals (inputs; `0x55ca5d` reads the start through `arg3`).
6. Nonzero return → `0x4c7f50(obj)` completes the record (`+0x24` =
   object, `+0x28` = owner word).
7. Unless stop-mode already produced a record (`0x4c9bde`..`0x4c9bef`),
   every child `[obj+0x5c][i]` passes the flag/`+0xc4` pre-filter and is
   re-dispatched as `0x4c9a00(child, child_count)` (`0x4c9bf1`..`0x4c9c53`).
8. `ebp` nonzero → `0x4c1550` pops the transform level (`0x4c9c55`).
9. Return `1` when the count byte is still zero, `0` when a record was
   added — the query's own convention.

The `0x4cc010` reject label `0x4c9ddb` (shared by every case) returns the
nonzero reject code with no record.

### 13.6 `0x55c9c0`: segment versus triangle mesh

`0x55c9c0(geo, rec, &seg_start, &seg_end)` (`static`) is the exact test;
`geo` is the mesh record `[obj+0x3c]` produced:

```asm
0055c9cc  mov eax, dword ptr [edi + 0x10]   ; group count
0055c9cf  test eax, eax                      ; zero -> return 0, no test
0055c9d7  test byte ptr [edi + 8], 0x10      ; geo flag: scaled vertices
0055c9f0  mov eax, dword ptr [edi + 0x1c]    ; vertex count
0055c9fa  mov edx, dword ptr [edi + 0x44]    ; scale-recip path args
0055ca0b  call 0x53c580                      ; -> scaled vertex array [0xa06fb4]
0055ca1e  mov eax, dword ptr [edi + 0x38]    ; else plain vertex array
0055ca24  mov ecx, dword ptr [0x6379dc]
0055ca2a  cmp  dword ptr [ecx], 0            ; stacked-transform flag
0055ca2d  jne  0x55cb33                      ; nonzero: segment stays world
0055ca41  call 0x53dfc0                      ; inverse affine of stack top
```

* When a transform level is stacked (flag word `0`), `0x53dfc0(matrix,
  &affine, 1)` computes the inverse affine into a 0x30-byte local and the
  segment endpoints are multiplied through it (`0x55ca51`..`0x55cb55`) —
  the mesh is tested in **object space**. The mode argument `1` selects
  the full affine; its other values are not decoded.
* The mesh record's relevant fields: `+0x08` flag word (bit `0x10` = the
  scaled-vertex path), `+0x10` group count, `+0x1c` vertex count, `+0x24`
  a scale scalar (compared against `0.0`, `fcomp [0x603398]`), `+0x34`
  the group table, `+0x38` the vertex array, `+0x44` scale-record
  argument, `+0x58` the last-hit group index (cache, written back at
  `0x55d259`).

Each **group** record is 40 bytes (`esi = [geo+0x34] + last_hit * 40`,
`0x55cb5b`..`0x55cb64`) and is walked first from the cached index, then
the remainder in a second sweep (`0x55cec2`..; cached group skipped at
`0x55ced5`). The group word's bit `0x2000` marks an **indexed strip**;
`& 0x3ff` is the index count and `count - 2` the triangle count
(`0x55cb67`..`0x55cb7a` — degenerate pairs are skipped via the duplicate
checks at `0x55cba7`/`0x55cbb8`). A clear `0x2000` is a **fan/polygon
list** tested as one convex polygon. Vertices are copied into the scratch
triple at `0xa05e84` (three 12-byte copies per triangle,
`0x55cbd2`..`0x55cc35`).

The group field `+0x14` is a pointer to a material/surface record; its
byte `+1` bit `0x02` selects the tester:

```asm
0055cc38  mov  edx, dword ptr [esi + 0x14]
0055cc3b  mov  edx, dword ptr [edx]
0055cc3d  test byte ptr [edx + 1], 2
0055cc41  je   -> 0x55d6c0 (plain)
0055cc9c  call 0x55db90                      ; textured: also fills UV outs
0055ccc4  call 0x55d6c0                      ; plain
```

On a hit (`0x55ccc4` tester → `jne 0x55cdc6`) the routine writes
`rec+0x20 = [[group + 0x14]]` — the surface word — (`0x55cdc6`..`0x55cdce`),
transforms `rec+0` (the normal, through the 3x3 part of the stack matrix,
no translation) and `rec+0xc` (the contact point, full affine) back to
world when a transform is stacked (`0x55cdd1`..`0x55cea9`), normalises
`rec+0` in place via `0x422690` (`0x55ceac`) and returns `1`
(`mov eax, 1` at `0x55ceb8`). The second sweep's hit path does the same
record work and additionally stores the hit group's index into
`[geo+0x58]` (`0x55d259`), so the last hit is where the next query's
walk starts. Every miss path — empty mesh, tester reject, group walk
exhausted — lands on `xor eax, eax; ret` at `0x55d15d` and returns `0`.

### 13.7 `0x55d6c0` / `0x55db90`: the polygon testers

`0x55d6c0(rec, &seg_a, &seg_b, tri, count, double_sided)` (`static`)
tests the segment (two locals the caller built) against the convex
polygon at `tri` (`[0xa05e84]`):

* `0x55d6d2`..`0x55d73a`: face normal from the first three vertices,
  stored at `rec+0..0xb`; `[0x63b364]` is its call tally.
* `0x55d73d`..`0x55d77e`: `d = -(n . v0)`; `denom = n . dir`; when
  `denom` underflows zero (`fcomp [0x6032c8]`, CF clear) the single-sided
  variant (`double_sided == 0`) rejects immediately.
* `0x55d7a9`..`0x55d7c1`: back-face reject via the sign bit of `denom`
  xor `double_sided` (`test edi, 0x80000000`).
* `0x55d7c2`..`0x55d7e6`: `t` and the contact point — `rec+0xc..0x17`.
* The remainder walks the polygon's edges in the two axes of the
  dominant projection (`f7 04 be 00 00 00 80` sign test at `0x55d82e`,
  six-way jump `ff 24 bd 74 db 55 00` at `0x55d84a` through `0x55db74`),
  rejecting when the hit lies outside with the qword epsilon `-0.0001`
  at `0x609180`; inside → `return nonzero`.

`0x55db90` is the textured variant (`sub esp, 0x28`; `0x53e380` computes
the polygon normal into `rec` at `0x55dbbb`, `0x53e42d` computes
barycentric weights at `0x55de2e`): it takes two extra arguments — a
face-UV triple and an interpolated-UV out — which `0x55c9c0` fills from
frame locals (`ebp-0x50`, `ebp-0x20`), so for this query the UV work is
dead output.

`0x422690(rec)` is a 3-component normalise-in-place (`fsqrt`/`fdivrp`
over `rec+0..0xb`), so `rec+0` is left as a **unit** normal.

### 13.8 The other dispatch cases

| type | handler | shape |
| --- | --- | --- |
| 1 (model) | `0x4c9fe0` | no `0x4cc010` stage; flag bit 4 gates `0x539270(esi+0x44)` + `0x53bb20(esi+0x20, esi+0x14, &unit_scale)` then the same `+0x3c`/`+0xc4`/`[0x71e8b0]` gate → `0x55c9c0` (`0x4ca05d`) → inline record |
| 5 (Object3d) | `0x4c9adf` | §13.5 |
| 6 (LOD) | `0x4c9c72` | `[data+4] > 5.0` (`fcomp [0x6033e8]`) → return 1; else optional AABB/record then `0x4c9a00` on **every** child |
| 7 (selector) | `0x4c9d12` | `[data] == 0` → return 1; else optional AABB/record then `0x4c9a00` on **one** child: `[data + [data+0x14]*8 + 0x20]` (`0x4c9d60`) — an 8-byte option table at `+0x20`, index at `+0x14` |
| 8 | `0x4c9e10` | same skeleton as 5; transform `0x539270(edi+0x38)` + `0x53a3c0(edi+8, 1)` gated by flag bit 4; `0x55c9c0` at `0x4c9f02`; the record's `+0x24/+0x28` written inline (same code `0x4c7f50` runs) |
| 9 | `0x4ca140` | same skeleton as 5; transform `0x539270(edi+0x30)` filled by four piecewise helpers — `0x53a970(edi+0xc, edi+0x10, edi+0x14)`, `0x53ac80(edi+4)`, `0x53aae0(edi)`, `0x53ae20(edi+8)` — `0x55c9c0` at `0x4ca244` |
| 10 | `0x4c9dd7` | `mov eax, [esp+0x18]; ret` — returns arg2, tests nothing |
| 2,3,4 | `0x4c9daf` | the unknown-type error already pinned |

The `+0xc4 > [0x71e8b0]` gate, the `+0x3c` null check and the children
recursion are identical in every exact-test handler; the differences are
the transform source and whether `0x4c7f50` or its inline copy writes the
record tail.

### 13.9 The record, what the caller reads, `[0x71e8b0]`

The `0x2c`-byte record layout, consolidated (`static`; every field that
is written is listed — `+0x18..+0x1f` has no measured writer on the
hillside path and stays **unknown**):

| offset | writer | content |
| --- | --- | --- |
| `+0x00..+0x0b` | `0x55d6c0`/`0x55db90`, normalised by `0x422690` | unit contact normal, world space when a transform is stacked |
| `+0x0c..+0x17` | `0x55d6c0`/`0x55db90` | world contact point |
| `+0x18..+0x1f` | — | unknown (no writer measured on this path) |
| `+0x20` | `0x55c9c0` on a triangle hit; `0x55d270` writes `0` on a box-suffices hit | `[[group+0x14]]` — the surface/material word, `0` for a box face |
| `+0x24` | `0x4c7f50` (or its inline copy) | the object node |
| `+0x28` | `0x4c7f50` (or its inline copy) | owner word: `[obj+0x40]`; when the node carries flag `0x200000` the code walks `parent_array[0]` up while the current node keeps the bit, has `[+0x40] == 0` and has a parent, and records the `[+0x40]` of where it stops (`0x4c9e68`..`0x4c9e8a`) |

On the box-suffices path (`0x55d270`, §13.4) the same slots are filled
from the box test instead of the polygon testers.

`0x41f810` reads **only** the count byte `buf[0]` (and the query's return):
the record fields exist for other callers. The escape latch cannot see the
normal, point or material — measured at `0x41f95f`..`0x41f98e`.

`[0x71e8b0]`, the `+0xc4` threshold in every exact-test gate, has a single
setter `0x4c7600(arg1, arg2)`: `arg1 != 0` stores `arg2`, `arg1 == 0`
stores `-1` (`0x4c7600`..`0x4c761c`). Its only call sites — `0x5b053a`
and `0x5b055f` — bracket one `0x4c8ec0` call with `(1, [esi+0x7c])` /
`(0, -1)`, so after any such call the global is left `-1`. `0x41f810`
never sets it: for the follower's query the `+0xc4 > -1` test passes for
every node (the file asserts `unk196 = 160` for object/LOD nodes), i.e.
the threshold is inert on this path.

### 13.10 Answer for the hillside, and consistency with the port

For every listed member of cells (4, 12)/(5, 12) the original test is:

`flags` bits `0x04`+`0x10` → stop-mode/`+0xc4`/`0x2000000` filters →
**AABB pre-reject** (`0x4cc010`, skipped only for a cell's first entry) →
transform push → **`0x55c9c0` triangle-exact test** in object space over
the mesh's strip/fan groups → `0x4c7f50` records `normal | point |
material | node | owner` → children recurse.

So it is **not** a sphere test, **not** a heightfield lookup, and **not**
satisfied by the AABB alone for these nodes (their `0x20` "box suffices"
bit is clear): the segment must cross a collision face. `g28604`, whose
`mesh_index` is `-1`, presumably has `[obj+0x3c] == 0` — its own test is
skipped and the group's reach is exactly its children; `g28982` (mesh
762) carries the geometry the segment is tested against.

Consistency with #2292's port-side probe (`WorldSegmentProbe`, a segment
cast against the runtime colliders): no contradiction — the port already
documents the probe as an approximation, and a trimesh collider over the
same faces approximates this exact test. The decode adds two refinements
the approximation does not have: the AABB pre-reject (performance only,
same answer) and the fact that only the partition-listed nodes are
candidates at all (§11). Nothing here changes `original_stick`.

### 13.11 Unknowns left open

| unknown | affects | resolving step |
| --- | --- | --- |
| `rec+0x18..+0x1f` writers | other callers of the same query | instrument or decode a consumer |
| `0x53dfc0` mode argument / full transform-stack semantics of `0x539270`/`0x53a3c0` | exact space the test runs in for nested nodes | decode the matrix-stack helpers |
| the `0x4cc010` merge pass's second operand; `0x55d270`'s record fields beyond `+0x20 = 0` | box-suffices hit records | decode the helper and the merge operands |
| the runtime patch that turns `mesh_index` into `[obj+0x3c]` | certainty that `g28604`'s pointer is null | `gClsReadNode` / loader fixup |
| `[0x71e8ac]`/`[0x71e8b4]`/`[0x9be6f8]` defaults | non-follower calls | their setters, as before |
| `0x4c8ec0` (the sibling entry point `0x5b0520` wraps) | the other query mode | separate decode |
