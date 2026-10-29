# GROUNDBLOW-NORMAL-SCALE: the face winding and face sizes behind the ground blow's `sqrt(Z·N)`

Task #2212. Follows `2026-10-29-groundblow-world-segment-sweep.md`
(#2209, the probe and its one-segment world query) and
`2026-10-29-hit-record-contact-point.md` (#2208, the record layout). The
probe at `0x48bf60` scales its assist by `sqrt(dot(backward_axis, N))`
where `N` is the hit record's *unnormalised* face normal; this finding
measures the winding and the magnitude that term actually sees — on the
engine's six bound-box faces (static) and on M01's own water tile and
terrain chunk (retail container).

Static analysis (Capstone 5 over `$CS_ENGINE_IMAGE`, image base `0x400000`)
plus a read of `ZBD/C3/gamez.zbd` through the production
`cs_formats::gamez` readers (`read_gamez_meshes`, `read_gamez_materials`,
`read_gamez_nodes`; the same entry points `cs_app::world::audit` uses).
No original executable ran; nothing here is `verified_original`. The
committed byte citations are the `[[groundblow_normal_fact]]` rows of
`data/owner-static/engine-facts.toml`, re-read by the
`accept_groundblow_normal_` image test.

## The eight bound corners (`0x4cc010` → `0x4cd9d0`)

`0x4cc010` is the only caller of the box face tester (`call 0x55d270` at
`0x4cc1da`). For a node with `[node+0x24] & 0x20` it first calls
`0x4cd9d0(node, &corners)` (`0x4cc05b`), which fills an 8×`vec3` corner
array on the stack; the slab min/max of those corners is then walked and
rejects misses before `0x55d270` runs. Flag `0x20` clear takes a different
bound path (`0x4ce1b0`), not this tester.

Inside `0x4cd9d0`, the common build (`esi != 0`, transform slot `edx == 0`)
copies the node's bound record `[node+0x70]` — six floats, two `vec3`s,
`lo = (f0, f1, f2)` and `hi = (f3, f4, f5)` — into the corners at
`0x4cdabc..0x4cdb4d`:

| corner | offset | value |
| --- | --- | --- |
| c0 | `+0x00` | `(f0, f1, f5)` = `(lo.x, lo.y, hi.z)` |
| c1 | `+0x0c` | `(f3, f1, f5)` = `(hi.x, lo.y, hi.z)` |
| c2 | `+0x18` | `(f3, f1, f2)` = `(hi.x, lo.y, lo.z)` |
| c3 | `+0x24` | `(f0, f1, f2)` = `(lo.x, lo.y, lo.z)` |
| c4 | `+0x30` | `(f0, f4, f5)` = `(lo.x, hi.y, hi.z)` |
| c5 | `+0x3c` | `(f3, f4, f5)` = `(hi.x, hi.y, hi.z)` |
| c6 | `+0x48` | `(f3, f4, f2)` = `(hi.x, hi.y, lo.z)` |
| c7 | `+0x54` | `(f0, f4, f2)` = `(lo.x, hi.y, lo.z)` |

c0..c3 walk the `y = lo.y` ring in order, c4..c7 the `y = hi.y` ring in
the same order. The transformed variant `0x53cef0` (used when the corner
slot needs a matrix) writes the same eight slots under that transform.

## The six box faces (`0x55d270`)

`0x55d270` zeroes `record+0x20` (`0x55d27f`), copies each face's four
corners from the array into the scratch point buffer `0xa05e84`, and calls
`0x55d6c0(record, seg_start, seg_end, buf, count=4, flag=0)` per face —
returning success on the **first** accepted face (`0x55d31e` and the same
`test eax,eax / je next` shape before every later call). Between calls the
buffer is only partially rewritten; the unwritten slot keeps the previous
face's corner, which is why the orders below interleave.

| call site | face | buf order | plane | N = `(c1−c0)×(c2−c0)` | direction |
| --- | --- | --- | --- | --- | --- |
| `0x55d314` | 0 | c0, c4, c7, c3 | `x = lo.x` | `(0,+Y,0)×(0,+Y,−Z)` | **−X — outward** |
| `0x55d397` | 1 | c0, c1, c5, c4 | `z = hi.z` | `(+X,0,0)×(+X,+Y,0)` | **+Z — outward** |
| `0x55d413` | 2 | c5, c1, c2, c6 | `x = hi.x` | `(0,−Y,0)×(0,−Y,−Z)` | **+X — outward** |
| `0x55d48c` | 3 | c7, c6, c2, c3 | `z = lo.z` | `(+X,0,0)×(+X,−Y,0)` | **−Z — outward** |
| `0x55d503` | 4 | c0, c3, c2, c1 | `y = lo.y` | `(0,0,−Z)×(+X,0,−Z)` | **−Y — outward** |
| `0x55d59b` | 5 | c4, c5, c6, c7 | `y = hi.y` | `(+X,0,0)×(+X,0,−Z)` | **+Y — outward** |

All six stored normals point **outward** from the box. (The face orders
read straight off the `lea`/`mov` copy chains at `0x55d286`, `0x55d32d`,
`0x55d3b0`, `0x55d42c`, `0x55d4a5`, `0x55d51c`; the signs follow from the
corner table, `dx = hi.x−lo.x > 0` etc.) Because the accept gate (below)
needs the segment end on the −N side, the outward convention means the
face N always looks back toward the incoming segment — the same side the
aircraft approaches on.

## The shared face test (`0x55d6c0`) and its normal

`0x55d6c0` computes the face normal from the **first three** supplied
corners — `e1 = buf[1]−buf[0]`, `e2 = buf[2]−buf[0]`, `N = e1×e2`
(`0x55d6d2..0x55d738`) — and stores it, unnormalised, at `record+0x00`
(`fstp [esi]`, `[esi+4]`, `[esi+8]` at `0x55d717`, `0x55d727`,
`0x55d73a`). The task's working hypothesis of `corner3` was wrong: the
third edge is never touched by the normal; only the inside-edge walk uses
the later corners.

The accept sequence: `d2 = dot(arg2_end − buf[0], N)` (`0x55d73d..0x55d75b`)
must be strictly negative or the face is rejected, unless the `flag`
argument is non-zero (`0x55d75b..0x55d772`, comparing against the `0.0`
constant `0x6032c8`). Then `d1 = dot(arg1_end − buf[0], N)` must have the
opposite sign (`0x55d7a5..0x55d7bb`, an `xor` on the raw sign bits) — the
segment must straddle the plane. The hit point is the crossing
`arg1 + t·(arg2 − arg1)`, `t = d1/(d1−d2)` (`0x55d7c2` on; the point store
opens at `0x55d7d8`), and the inside test walks the face's edges.

So `flag != 0` is a per-face two-sided override; the one-sided rule is
"the `arg2` endpoint lies strictly on the −N side" — N faces the
`arg1`/start side. For the box calls `flag = 0` literally; for the soup
calls `0x55c9c0` pushes `(vertex_info >> 10) & 1` (`0x55cc73..0x55cc7b`),
i.e. the stored polygon's two-sided bit, `RawPolygonInfo::flags() & 0x04`,
`RawPolygon::is_two_sided`.

The polygon paths resolve corners in **stored order**: outlines take the
corners as stored, and strips decompose to `(i, i+1, i+2)` on even
triangles and `(i, i+2, i+1)` on odd (`0x55cb90..0x55cce6`) — the standard
consistent-winding fan — so a strip's stored order already carries a
uniform front face. In all cases `N`'s magnitude is the cross product of
the stored edges: `2·A_triangle` for a three-corner face (and per strip
triangle), the first-three-corners parallelogram for a larger outline,
`|e1||e2|sinθ` generally.

## The probe's `sqrt(dot)` (`0x48bf60`)

The ground-blow probe runs the one-segment query `0x4c8ec0` (`0x48c01f`)
and, on a hit, dots the record normal `record+0x00..0x0b` with the
aircraft's backward axis (`0x48c0ff..0x48c115`), rejecting `dot <= 0`
(`fcomp [0x6032c8]` at `0x48c118`, `jne` on `ah & 0x41`). Then
(`0x48c13a..0x48c155`):

    amount = (elev − |hit − position|) / elev · sqrt(dot(back, N))

with `fsqrt` at `0x48c148` applied to the raw dot and `fdiv [ebp+8]` —
the `elev` divisor — at `0x48c152`; `elev` is `[0x71c420]` = 400 m
(finding of #2209). No instruction between the face writer and the dot
normalises `N`: the record keeps the unnormalised cross product, so

    sqrt(dot(back, N)) = sqrt(|N|) · sqrt(cos θ)

— a per-face `sqrt(parallelogram_area)` factor the task's `sqrt(cos)`
reading does not have. Whether retail faces are near unit area is the
retail question below.

## Retail M01: the water tile

`ZBD/C3/gamez.zbd` (the C3 world group M01's world mounts, per
`2026-10-17-m01-archive-decision.md`): 5 408 nodes, 1 901 meshes. The
contactable mesh nodes (`flags & 0x04 && & 0x10`, `mesh_index ≥ 0`)
split into 344 water-material nodes (every material group's texture name
`wtr0*` or `water1`) and 2 036 non-water nodes.

Measured tile: node 547 `g28530`, mesh 371 — a 1 024 × 1 024 m tile flat
at `y = 0`, eight stored **outline** polygons (`two_sided` clear, flag
word `0x0`), all wound so `N` points exactly `+Y`:

| poly | corners | `N` (stored order) | `|N|` |
| --- | --- | --- | --- |
| 6 | quad `(−14848,0,−9216) (−14848,0,−9728) (−15360,0,−9728) (−15360,0,−9216)` | `(0, 262144, 0)` | **262 144 m²** |
| 5 | quad `…(−14336,0,−9856) (−14336,0,−10240) (−14848,0,−10240) (−14848,0,−9728)` | `(0, 196608, 0)` | **196 608 m²** |
| 1 | tri `(−14602.7,0,−9348.7) (−14848,0,−9728) (−14848,0,−9216)` | `(0, 125611.5, 0)` | **125 612 m²** |

Census over all 1 556 stored polygons of the 344 water nodes: **1 556 up,
0 down, 0 edge-on**, 0 two-sided, 5 strips; `|N|` spans
**537 m² … 393 216 m²** — a `sqrt(|N|)` span of ≈ 23× … 627× the unit-face
value. (The prior sea-contact finding's 347-node count included
non-mesh-bearing or non-uniform-material variants; the contactable,
uniformly-water-material set measured here is 344.)

For a **level** aircraft the backward axis is horizontal, so
`dot(back, N) = 0` exactly over flat water — the probe's `dot <= 0`
reject means the assist does nothing until the nose drops; descending
(`back.y > 0`), `dot > 0` and the term is `sqrt(|N|·sin α)`, up to
`627·sqrt(sin α)` on the widest quad.

## Retail M01: a terrain chunk

Island chunks carry `cliff1*`/`forest1*`/`sand*`/`shore*` textures; the
backdrop dome (`g1155`/`h_zone2scroll`, `Sky1`/`cloud*`/`cumulus*`) is a
separate contactable node — excluded here. Measured chunk: node 2873
`g28603`, mesh 761, 22 stored **triangle strips** (flag `0x30` = strip +
normals, two-sided clear), `cliff1_watertrans2`/`forest1` textures,
≈ 1 027 m across, composed node transform `det = +1` (pure rotation —
stored winding is world winding):

| poly | kind | `N` (stored order, world) | `|N|` | `N/|N|` |
| --- | --- | --- | --- | --- |
| 19 | strip, 30 corners | `(0, 20559.4, 0)` | **20 559 m²** | `(0, +1.000, 0)` |
| 7 | strip, 4 corners | `(10517.2, 16384.0, 0)` | **19 469 m²** | `(+0.540, +0.841, 0)` |
| 2 | strip, 8 corners | `(30817.0, 65536.0, −22929.4)` | **75 963 m²** | `(+0.406, +0.863, −0.302)` |

Every measured strip's stored winding faces **up and outward** (`N.y > 0`
on all three shown; the chunk's walkable faces share the convention) —
the aircraft descending toward the slope reads `dot(back, N) > 0` when it
approaches the face's out-of-hill side; from the opposite heading the
stored face is the hill's back side and `dot < 0`, which the probe
rejects — the two-sided bit is clear, so the original reports no such
hit either (the strip path never applies the flag, but a segment can only
straddle the plane one way).

Census over the 2 036 contactable non-water nodes' polygons: 8 395 up /
3 248 down / 5 432 edge-on — the down faces live on props and vehicles
(`gun` 137, `turret` 80, `hull` 47) and the backdrop dome, i.e. on
undersides a downward segment legitimately meets from behind.

## Answer

`sqrt(dot(backward_axis, N))` is **not** `sqrt(cos θ)` on retail geometry.
The record `N` is the stored face's unnormalised edge cross product, so
the term is `sqrt(|N|) · sqrt(cos θ)` where `|N|` is the face's
parallelogram area in m² (twice the triangle area for triangles/strip
triangles). On M01 content:

* flat-water polygons: `|N|` = 537 … 393 216 m² → the sqrt term is
  ≈ **23× … 627×** the `sqrt(cos)` value;
* the measured island-chunk strips: `|N|` = 19 469 … 75 963 m² →
  ≈ **140× … 276×**;
* bound-box faces (the flag-`0x20` path, not measured on C3's water or
  island chunks, which store `0x1c` low and take the soup path):
  `|N|` = the box face's width×height — equally non-unit.

A faithful implementation must feed the **unnormalised** cross product of
the actual retail face (or bound face) into `sqrt(dot)`; a unit normal
understates the term by two to three orders of magnitude on these
surfaces.

## Not measured

* Whether `0x53cef0` (the transformed-corner build used when the corner
  slot carries a matrix) produces the identical c0..c7 enumeration under
  every flag combination; its stores land in the same eight slots and the
  direct path above is the measured common case.
* The per-triangle `|N|` distribution inside each terrain strip (the
  probe takes whichever strip triangle the segment hits); the measured
  first-triangle values bound the strip's own first face only.
* The `0x4ce1b0` bound path for `flag 0x20`-clear bound tests — the
  slab rejects misses, but its face tester is a sibling the ground blow's
  C3 targets do not reach through the soup path.
* The exact `arg1`/`arg2` endpoint order the ground-blow caller passes
  into `0x4cd9d0`'s dispatcher (the gate text above names them arg1/arg2;
  whichever is the far end must land on the −N side).
