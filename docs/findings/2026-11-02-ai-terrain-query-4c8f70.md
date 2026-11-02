# The world segment query `0x4c8f70`, read (#2279)

Date: 2026-11-02. Task #2279, follow-up of #2278 and
`2026-11-02-m01-wingman-gains.md` §7.2-7.3. Implementer: `sonnet-2`. All
claims are `static`, read from the owner image (disassembled with capstone,
address convention `VA = file offset + 0x400000`). The original executable
was **not** run: nothing here is `verified_original`. The byte windows are
re-read by `accept_m01_ai_terrain_query_*`
(`crates/cs_app/tests/campaign/m01_ai_terrain_query.rs`, ignored without
`CS_ENGINE_IMAGE`, fails loudly when it is unset).

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
