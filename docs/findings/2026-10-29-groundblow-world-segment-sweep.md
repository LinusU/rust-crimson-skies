# #2209 GROUNDBLOW-SWEEP-DECODE: the world segment sweep `0x4c8ec0`, its hit record, and the probe `0x48bf60`

Date: 2026-10-29. Task #2209 (`GROUNDBLOW-SWEEP-DECODE`), which unblocks #2107 (the
ground-blow assist) and #2210 (its runtime seam). Static analysis only: the
owner-supplied decrypted image `crimson.decrypted.exe` (read-only, never
committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file
offset + `0x400000` below VA `0x643000`) was disassembled with
`llvm-objdump -d -M intel` and Capstone 5.0.7 into `private/`. **The original
executable was not run**: every claim is `static` (claim id
`OWNER-STATIC-2026-10-29`), nothing is `verified_original`. Implementer:
`deepseek-1`; no independent review yet.

Read first, as the task asked: `2026-10-25-force-block-reread.md` section 4.1,
`2026-10-21-m01-contact-response-static-locate.md` (e), #2107's block reason and
`2026-10-22-m01-sea-contact.md`. #2208 (the hit record's position field) has
**not** landed, so the record layout below is measured here from its writers and
its two consumers rather than taken from #2208.

The cited bytes are the machine-readable block at the end;
`accept_groundblow_sweep_*` re-reads every row from the image, and the same rows
are committed as `[[groundblow_sweep_fact]]` in
`data/owner-static/engine-facts.toml`.

## 0. Answer

1. **What the sweep tests.** `0x4c8ec0` is a thin wrapper over the one-segment
   world query `0x4c8f70`, which tests the segment against the **world
   database's node list** (`world+0x56` count, `world+0x5c` array) and its
   **partition grid** (cells indexed from `world+0x84/+0x88`, dimensions
   `+0x98/+0x9c`, rows `+0xa0`), then against each node's geometry with the
   node's **current** transform. A node is tested only when its flag word
   `node+0x24` carries **both** `0x04` and `0x10` (plus the layer filter
   `0x56c430` and the caller's node-flag mask `0x71e8b4`, which the ground blow
   leaves 0). Terrain, water tiles, buildings and **aircraft** are all nodes in
   that one database, so all of them answer; nothing in the query is specific to
   a surface class. Class types `1, 5, 6, 7, 8, 9, 10` are intersected, types
   `2, 3, 4` log `Unrecognized node class type` and are skipped.
   **There is no sea plane.** The sea answers only through its water-tile nodes'
   own triangles (their flag words were measured in
   `2026-10-22-m01-sea-contact.md`: `0x0308831c` / `0x0308031c`, both carrying
   `0x04|0x10`), at the tiles' own altitude (y = 0 for the flat sea).
2. **The hit record.** `0x4c8ec0(world, p0, p1, out)` fills a caller-owned
   buffer: `out[0]` = the **index** of the nearest contact, `out[1]` = the
   **count** (0..32), records from `out+4` at stride `0x2c`, at most 32
   (`0x584` bytes). A record is `+0x00` contact normal (three `f32`, **not**
   normalised — the raw cross product of the face's edges, so its magnitude is
   the face's parallelogram area), `+0x0c` the contact point (three `f32`,
   metres), `+0x18` unwritten by every writer read here, `+0x1c` the **squared**
   distance `|p0 - point|^2` (written by `0x4c8ec0` alone, and only when the
   count is above one), `+0x20` a face selector the box test zeroes, `+0x24` the
   hit **node** (the owner), `+0x28` the node's `+0x40` sub-object (or the first
   descendant that has one). There is no distance along the ray and no
   normalised distance; the probe's own length `groundblow_elev` = 400 is
   **400 metres** (`[position]` in the committed facts: world units are metres).
   The ground blow does not read `+0x1c`: it computes `|point - position|` itself
   with `0x5388d0`, in metres.
3. **The player's skip conditions (`0x48c036..0x48c0e9`) and the three vehicle
   bytes.** For the **player only**, a hit whose owner node has bit
   `0x40000000` in `node+0x28` (the marker `0x4848f0` puts on an aircraft's node
   subtree, set for class 0/4 at `0x47d8f5`) is resolved to its vehicle through
   the registry `0x71dab8` (`0x4afee0`, walking the first-child chain when the
   node itself is not registered). A resolved vehicle with **`V+0xcc == 0`
   discards the hit** (`amount = 0`); `V+0xcc != 0` keeps it, and a node with no
   resolved vehicle keeps it too. **This corrects the polarity stated in
   `2026-10-21-m01-contact-response-static-locate.md` (e)**, which has the
   comparison inverted. `V+0xac` is a **collision-grace deadline in seconds**
   (`now + 1.0`, set on both aircraft by the contact response `0x48d383` /
   `0x48d395` when class 0 or 4 hits class 0 or 4); the ground blow of a
   **non-player** is skipped entirely while `now < V+0xac`. `V+0xb4` is a second
   deadline of the same shape that scales a non-player's ground blow (and its
   velocity alignment) by `0.15` while `now < V+0xb4`; **no writer other than the
   reset (0) exists in this image**, so in a retail session the field reads 0 and
   that damping is unreachable — recorded, not guessed. `V+0xcc` is a **byte flag
   the record loader sets to 1 when the record carries a path**
   (`0x47c568`): the same branch stores the path at `V+0xc8`, and the flag makes
   the vehicle skip the placement restore on wake (`0x4b1025`) and take the
   far/simplified update (`0x4899ea`). The reading "an aircraft that is on its
   taxi path counts as ground for the player's probe" follows the mechanism; it
   is a reading, not a measurement of intent.
4. **Which existing Rust query answers this.** **None answers it today.** The
   closest transport is Avian's ray cast already used for the turret line of
   sight (`crates/cs_app/src/mission_session/turret_fire.rs`,
   `SpatialQuery::cast_ray_predicate` → `RayHitData { entity, distance, normal }`),
   which is exactly "nearest surface along a segment, with a normal and an
   owner". What it does **not** give: (a) the rest of M01's scenery — only the
   water tiles are colliders (#1850); (b) hits on **other aircraft**, because the
   existing call filters to static bodies, while the original's probe hits them
   and then discards them by the `V+0xcc` rule; (c) the original's **unnormalised**
   face normal, which changes `sqrt(Z·N)` by `sqrt(area)`; (d) the original's
   chord distance `|point - position|` (Avian gives the along-ray distance — the
   two agree only when the contact is perpendicular to the probe).
   `cs_sim::weapons::WeaponState::sweep` is a swept-OBB query over **actor boxes
   only** and cannot see the world; the contact reporting
   (`crates/cs_app/src/physics/contacts.rs`, `world/crossings.rs`) reports pairs,
   not a segment→surface query. Section 8 has the details.

## 1. The probe `0x48bf60`

`float 0x48bf60(V, vec3 axis, vec3 *out)`. `V` is the vehicle, `axis` the probe
direction the caller passes, `out` a vector the caller has zeroed; the return
value is the probe's "amount".

| site | what it does |
| --- | --- |
| `0x48bf84`..`0x48bf98` | `bit = (V[+0xc][+0x24] >> 2) & 1` — whether the vehicle's own node is currently "active" |
| `0x48bf92`, `0x48bf9b` | `0x4cca30(node, 0)` — deactivate it, so the probe cannot hit the probing vehicle's own geometry |
| `0x48bfa3` | `elev = [0x71c420]` (`groundblow_elev`, retail 400) |
| `0x48bfb0`..`0x48bfd6` | `delta = axis * elev`, then `p1 = position + delta` |
| `0x48bfd9`, `0x48c00b` | the vehicle's virtual 0 returns its position pointer: `p0 = position` |
| `0x48c00d`..`0x48c024` | `0x4c8ec0([0x71c2ac], p0, p1, out)` — the world is the global `0x71c2ac`; `out` is a `0x584`-byte stack buffer at `ebp-0x5c0` |
| `0x48c027` | a non-zero return (no contact) returns `0.0` |
| `0x48c036`..`0x48c054` | `index = out[0] & 0xff`; `record = out + 4 + index * 0x2c` |
| `0x48c047` | `V == [0x71c298]` (the player) selects the owner gate of section 5 |
| `0x48c0ed`..`0x48c126` | `d = dot(V[+0x198..0x1a0], record normal)`; `d <= 0` (or NaN) returns `0.0` |
| `0x48c13a`..`0x48c155` | `amount = (elev - 0x5388d0(position, record point)) / elev * sqrt(d)` |
| `0x48c161`..`0x48c19d` | `B = normalize(record normal x V[+0x198..0x1a0])` (`0x422690` normalises in place) |
| `0x48c1a4`..`0x48c1eb` | `*out += B * amount`; returns `amount` |
| `0x48c1f1`..`0x48c204` | `0x4cca30(node, bit)` — restore the vehicle's node if it was active |

So the probe is a segment from the body's position along the caller's axis, of
length `groundblow_elev` metres, and its amount is
`(elev - |point - position|) / elev * sqrt(dot(back_axis, N))`: zero when
nothing is hit, when the hit's normal does not point back along the body, and
zero again when the player's owner gate discards the hit.

Two properties worth naming because a port must reproduce them:

* **The normal is used unnormalised.** `0x55d6c0` writes
  `cross(edge, edge)` into the record's first three floats and never normalises
  it (`0x55d6d2`..`0x55d73a`), and `0x4c8f70` does not normalise records on the
  way out. Both other consumers normalise before use (the contact response at
  `0x48dc24`); the ground blow does not, so its `sqrt(dot)` is scaled by
  `sqrt(|N|)` = `sqrt(area of the hit face)`. Over a mesh tessellated into
  unit-sized quads this is `sqrt(cos)`; over a large flat water tile it is not.
  Whether retail's quads are unit-sized is **not settled here** (section 9).
* **A head-on hit gives no torque.** `B` is `normalize(N x Z)`, so when the
  normal is exactly the body's backward axis (a wall square to the nose) the
  cross product vanishes and only the amount survives; the torque term is zero
  while the velocity alignment still runs.

## 2. The query `0x4c8ec0`

`int 0x4c8ec0(world, vec3 *p0, vec3 *p1, out)`.

```
0x4c8ec0: unpack (world, p0, p1, out); call 0x4c8f70(world, p0.x, p0.y, p0.z, p1.x, p1.y, p1.z, out)
0x4c8ef3: if that returned non-zero  -> return 1                  (no contact)
0x4c8eff: out[1] = out[0]            ; the count, saved
0x4c8f01: if count <= 1              -> out[0] = 0; return 0      (one contact: index 0, no distance)
0x4c8f0a: record0[+0x1c] = 0x538880(p0, record0+0xc)              ; |p0 - point|^2
0x4c8f2d: for every further record: record[+0x1c] = |p0 - point|^2, keep the minimum's index
0x4c8f61: out[0] = that index; return 0
```

* `0x538880(a, b)` is `|a - b|^2` (squared; `0x5388d0` is its square root, the
  distance).
* The nearest selection is by **squared distance from the segment's start to
  the contact point** — a chord, not the parameter along the segment. It is the
  only thing `0x4c8ec0` adds to `0x4c8f70`.
* The `+0x1c` field is **not written when the count is 1**; the buffer is the
  caller's, so `+0x1c` is then whatever the caller's stack held. The ground blow
  never reads it; a future consumer must not either unless `out[1] > 1`.
* Return: `0` with `out[0]` = the nearest index and `out[1]` = the count, or `1`
  for no contact (in which case `out[1]` is not written).

## 3. The sweep `0x4c8f70`

`int 0x4c8f70(world, p0x, p0y, p0z, p1x, p1y, p1z, out)` — the one-segment form
of the same query family as `0x4ca320` (multi-segment) and `0x4cad00` (one node).
Returns `0` when `out[0]` (the count) is non-zero, `1` otherwise.

| site | what it does |
| --- | --- |
| `0x4c8f7a` | `world` is `arg1`; a null world or a null `world+0x38` (its database) logs `Null class data pointer` / `Null node pointer.` and returns `5` |
| `0x4c8fdc`..`0x4c90bb` | box A = `(p0, p1)` as given (`0x71e8b8..0x71e8cc`), box B = componentwise min/max (`0x71e8d0..0x71e8e4`) |
| `0x4c9100`, `0x4c911d` | `out` and `out+4` published in `0x71e9f0` / `0x71e9f4`, the record pointer the node tests advance |
| `0x4c9123`..`0x4c913a` | the previous values are saved on a stack (`0x6379dc`, `0x6379e0`), so a nested query from a node's own code restores them |
| `0x4c9141`..`0x4c9192` | reject the segment against the world's horizontal extent (`world+0x34/+0x44` x, `+0x38/+0x48` z) when `world+0x60` is 0 |
| `0x4c9198`..`0x4c96e1` | the grid walk: `world+0x74/+0x78` scales, `+0x34/+0x38` origin, `+0x98/+0x9c` dimensions, `+0xa0` cell rows |
| `0x4c9311`..`0x4c9393` | per cell, the first list (`cell+0x3a` count, `cell+0x3c` array): the same `0x04|0x10` flag test, then `0x4c9a00(node, count)` |
| `0x4c9393`..`0x4c93a0` | the cell's second list (`cell+0x40` count) through `0x4c9740` → `0x4c97a0` per `0x60`-byte entry, same flag test |
| `0x4c96fd`..`0x4c970e` | the world's own node list (`world+0x56`, `world+0x5c`) through `0x4c9980`, same flag test, then `0x4c9a00` |
| `0x4c9711` | `0x4c1550` releases whatever the node tests acquired |
| `0x4c9716`..`0x4c9735` | return `out[0] > 0 ? 0 : 1`; **`0x71e8b4` (the node-flag mask) is cleared to 0** |

Filters, in the order a node meets them (`0x4c998f`, `0x4c9311`, `0x4c97a0`,
then `0x4c9a00`):

1. `node+0x24 & 0x04` and `node+0x24 & 0x10` — both required. `0x04` is the
   node's own "active" bit (`0x4cca30(node, 0)` clears exactly it, and that is how
   the ground blow excludes the probing vehicle);
2. `0x56c430(node+0x30)` — the layer filter, which accepts everything while
   `[0x62cb50] == 0` or the layer list at `0xa070a8` is empty, and accepts layer
   `0xff` always;
3. `0x71e8ac` — when non-zero, a node whose `node+0xc4 > [0x9be6f8]` is skipped
   (the setter is `0x4c75f0`; the ground-blow path does not set it);
4. `test [0x71e8b4], node_flags` — **rejects** a node whose flag word intersects
   the caller's mask. `0x4c7620` sets it; `0x5acf60` (the gun round) sets
   `0x40000`. The ground blow sets no mask and every query clears it on return,
   so it runs with mask 0.

Then `0x4c9a00(node, count)` dispatches on `node+0x34`, the node's class type
(table at `0x4c9de0`):

| type | handler | what it tests |
| --- | --- | --- |
| 1 | `0x4c9fe0` | the node's own polygon list `node+0x3c` through `0x55c9c0`, then recurses into `node+0x56/+0x5c` |
| 2, 3, 4 | `0x4c9daf` | nothing: logs `Unrecognized node class type:  node = %s class_type = %d` (`0x62cb54`, `cls_di.c`) and returns 3 |
| 5 | `0x4c9adf` | the node box through `0x4cc010` → `0x55d270`, appends a contact |
| 6 | `0x4c9c72` | the node box (skipped when `node+0x38`'s `+4` is below the double at `0x6033e8`), then recurses into the children |
| 7 | `0x4c9d12` | the node box, then recurses through `node+0x38`'s `+0x14` index into `+0x20` |
| 8 | `0x4c9d76` | the node box through `0x4cc010` → `0x55d270` (`0x4c9e10`) |
| 9 | `0x4c9d9c` | `0x4ca140` |
| 10 | `0x4c9dd7` | nothing: returns the count unchanged |

The contact cap is **32** (`0x4c9a9d`): a 33rd hit logs `Database intersections
array is full` (`0x62cbb0`) and aborts the query with 1. Each node is tested with
its **current** transform (`0x4c9b28`..`0x4c9b5e` invert the geometry's matrix,
`0x53bb20` composes it), so a moving aircraft is hit at its pose for that frame.

**Aircraft answer the query.** An aircraft's node lives at `V+0xc` and carries
the same `+0x24` flags, `+0x34` type and `+0x38` geometry as any world node; the
query has no notion of a surface class. Two independent consumers confirm it
from the other side: the ground blow resolves a hit owner to a vehicle (section
5), and the contact response gives class 0/4 hits a grace period
(`0x48d31e`..`0x48d395`).

## 4. The hit record

`out` is `0x584` bytes: `out[0]` a byte the record appenders increment (the
count during the sweep, the nearest index after `0x4c8ec0`), `out[1]` the count
saved by `0x4c8ec0`, then up to 32 records of `0x2c` bytes from `out+4`.

| offset | content | written by | read by |
| --- | --- | --- | --- |
| `+0x00` | contact normal, 3 × `f32`, **unnormalised** (the face's edge cross product; magnitude = the face's area) | `0x55d6d2`..`0x55d73a` (`0x55d6c0`) | the ground blow (`0x48c0f9`), the contact response (`0x48dc24`, which normalises first) |
| `+0x0c` | contact point, 3 × `f32`, metres, world space | `0x55d7d0`..`0x55d805` | the ground blow (`0x48c131`), the contact response |
| `+0x18` | no writer found in this image | — | — |
| `+0x1c` | `|p0 - point|^2`, squared metres | `0x4c8ec0` only, and only when the count exceeds 1 | nothing measured (the ground blow does not) |
| `+0x20` | a face selector; the box test zeroes it before testing the six faces | `0x55d27f` | not measured |
| `+0x24` | the hit **node** (the owner) | `0x4c7f5a`, `0x4c9e54`, `0x4c800b`, `0x4ca06e` | the ground blow's owner gate, the contact response |
| `+0x28` | the node's `+0x40` sub-object, or the first descendant that has one | `0x4c7f69`..`0x4c7f9f`, `0x4c8044` | `0x5abcf0`'s surface index (`[record+0x20][+0x20]` in `2026-10-25-m01-hit-dispatch-team-trace.md`) |

`0x4c7f50` appends one record: it stores the node at `+0x24`, resolves `+0x28`
down the first-child chain, advances `0x71e9f4` by `0x2c` and increments
`out[0]`.

This corrects one detail of `2026-10-21-m01-contact-response-static-locate.md`
(b): the sub-object is at `+0x28`, not `+0x20`; `+0x20` is the face selector.
The owner at `+0x24` matches that finding.

## 5. The player's hit-owner gate (`0x48c036`..`0x48c0e9`)

```
index  = out[0] & 0xff; record = out + 4 + index*0x2c
if V != player                                  -> use the hit            (0x48c0ed)
owner = record[+0x24]
if owner == 0                                   -> use the hit            (terrain, buildings)
if (owner[+0x28] & 0x40000000) == 0             -> use the hit            (0x48c076)
vehicle = 0x4afee0(0x71dab8, owner)                       ; registry lookup by node
while vehicle == 0 and owner has children:                ; walk the first-child chain
    owner = owner[+0x58][0]; if owner == 0: break; vehicle = 0x4afee0(0x71dab8, owner)
if vehicle == 0                                 -> use the hit            (0x48c0e6)
if vehicle[+0xcc] == 0                          -> amount = 0, return     (0x48c0ac..0x48c0e5)
else                                            -> use the hit
```

* `0x4afee0(map, key)` walks an intrusive list at `0x71dab8` and returns the
  entry whose value's `+0xc` equals `key` — the map from a world node to the
  vehicle that owns it (`V+0xc` is the vehicle's node).
* Bit `0x40000000` in `node+0x28` is put on an **aircraft's node and its whole
  subtree** by `0x4848f0` (recursive, `0x47d8f5` at the vehicle's own update, for
  class 0 or 4 only). It is the "this node belongs to a vehicle" marker, and it
  is why only aircraft hits need the registry walk.
* **The polarity:** `test cl, cl; jne 0x48c0e6` — a non-zero `V+0xcc` jumps to
  *keep* the hit; the fall-through zeroes the amount with `fld [0x6032c8]` (0.0)
  and returns. `2026-10-21` (e) states the opposite ("`[vehicle+0xcc] != 0`
  zeroes the term"); the instruction stream is unambiguous.

## 6. `V+0xac`, `V+0xb4`, `V+0xcc`

* **`V+0xac` — the collision-grace deadline, seconds.** `[0x71c470]` is the
  simulation clock (`0x4897c4`: `[0x71c470] += [0x9ad744]`, the frame time, every
  frame). The only writers are the contact response, which stores
  `[0x71c470] + 1.0` (`0x6032dc`) into **both** vehicles when a class 0/4 hit
  resolves to a class 0/4 vehicle (`0x48d31e`..`0x48d395`), and the reset
  (`0x4b057f`, 0). `0x48d807` makes the vehicle's own contact sweep return at
  once while `now < V+0xac`, and `0x48c241`..`0x48c252` skips a **non-player's**
  ground blow over the same window. The player's ground blow is not gated by it.
* **`V+0xb4` — a second deadline, same shape.** `0x48c317`..`0x48c345`: while
  `now < V+0xb4`, a non-player's `ai_groundblow * groundblow_mag` factor and its
  amount are both multiplied by `0.15` (`0x6036a8`). **No writer sets it to
  anything but 0** in this image: the scan for `[reg+0xb4]` stores finds only the
  reset (`0x4b058b`) for the vehicle class (the other hits are the airframe
  params struct at `0x47a796`, another class at `0x4bd576`, and stack frames).
  So in a retail session the damping is unreachable. Recorded as measured, not
  assigned a meaning.
* **`V+0xcc` — "the record carried a path", a byte.** Set to 1 only at
  `0x47c568`, in the loader's path branch: when the record's `+0x11c` resolves
  through `0x4511b0(0x6541b0, 0, name, 1)` to a path with at least two points,
  the loader stores the path at `V+0xc8`, `V+0xcc = 1`, `V+0xd0 = 0`,
  `V+0xd4 = 1` and places the vehicle at the path's first point. The container
  `0x6541b0` is the named-path registry (`0x4940d0` is the mission directive that
  assigns one). Two consumers read the flag: the wake path (`0x4b1025`) skips
  restoring the vehicle's stored placement when it is set, and the per-frame
  update (`0x4899ea`) forces the far/simplified branch, i.e. the path follower
  drives the aircraft instead of the normal AI. The reset clears it
  (`0x4b005e`, `0x48a863`).
  In the ground blow it is the player's test for "this aircraft is a valid ground
  surface": a registered aircraft with `+0xcc == 0` is discarded, one with
  `+0xcc != 0` (on a path, so on the ground) is kept.

## 7. Does the sea answer?

Yes, through its tiles, and only through them.

* The sweep has **no sea plane and no surface-type test**: a contact exists only
  for a node that carries `0x04|0x10` and whose geometry the type dispatch can
  intersect (`2026-10-21` (b) reached the same conclusion for the sweep family;
  section 3 confirms it for the one-segment form the ground blow uses).
* `2026-10-22-m01-sea-contact.md` measured M01's water tiles through the
  production container reader: 347 nodes whose every material group is
  `wtr0xxxx` / `water1`, storing `0x0308831c` (251, indexed by the partition
  grid) or `0x0308031c` (96, not indexed). **Both carry `0x04|0x10`.** Both
  places are walked by `0x4c8f70` (the grid walk and the world node list), so
  the tiles answer the ground blow at their own altitude — y = 0 for the flat
  sea, up to 13 m for pools and falls.
* What is **not** settled: the water-tile nodes' runtime class type
  (`node+0x34`). The container's `node_type` is the container kind
  (`object3d`/`lod`/…), not the runtime class, and the loader that assigns the
  runtime class was not read. Only types 2, 3 and 4 are unintersectable, so the
  conclusion holds for any type the loader can give a mesh-bearing node, but the
  exact one is unmeasured (section 9).

## 8. What the Rust side already has

| candidate | what it answers | verdict for the ground blow |
| --- | --- | --- |
| `SpatialQuery::cast_ray_predicate` — `crates/cs_app/src/mission_session/turret_fire.rs` (`line_of_sight`), `targeting_input.rs`, `mission_session/smoke/crash.rs` | the nearest **static** collider along a ray, as `RayHitData { entity, distance, normal }` (avian3d 0.7.0, `spatial_query/ray_caster.rs`) | the right **transport**: the probe is exactly "from the position along an axis, `elev` long". Missing: dynamic targets (aircraft), the rest of the scenery as colliders, the unnormalised normal, the chord distance |
| `cs_sim::weapons::WeaponState::sweep(segment, targets)` — `crates/cs_sim/src/weapons/guns.rs` | swept OBBs of **actors only**, as `SweptHit { time_of_impact, .. }` | cannot see the world, gives no surface normal, no owner node |
| `crates/cs_app/src/physics/contacts.rs` (`shape_hits_callback` over the body's own collider), `crates/cs_app/src/world/crossings.rs` | which colliders the body's volume crossed this tick | reports pairs for damage, not a segment→surface query |
| `crates/cs_app/src/playtest/scene.rs` (`PlaytestSea`, `PlaytestGround`) | markers on the bodies the scene spawns; no query of its own | it is the scene, not a query |
| `cs_app::playtest_retail::CONTACT_SWEEP_NODE_FLAGS` = `0x14` — `world_draw.rs` | which scenery placements become trimesh colliders: the flag test this task measured | the right **predicate**; today only the water tiles get colliders (#1850 "Only water tiles") |

So the seam for #2210 is: a segment query over the Avian world (static **and**
dynamic, excluding the prober), returning a normal, a point and an owner, with
the `0x14` flag predicate deciding which scenery is solid — plus the normal
convention of section 1 and the chord distance of section 2.

## 9. Unknown, and who resolves it

| unknown | affects | evidence | resolved by |
| --- | --- | --- | --- |
| the sign convention of a retail face's record normal relative to the solid side (which winding a mesh stores) | whether `dot(back_axis, N) > 0` selects "the surface ahead" for a given mesh, i.e. whether the probe fires in a dive | `0x55d6c0` builds the normal from the face's edge order; the corner enumeration of `0x55d270`'s box faces was not decoded | read the box corner enumeration (`0x4cd9d0`/`0x4ce1b0`) and one retail mesh's winding, or a runtime capture of a low pass |
| whether retail's hit faces are unit-sized (the `sqrt(|N|)` scale of the amount) | the magnitude of the torque and of the velocity alignment over water and terrain | `0x55d6d2`..`0x55d73a` writes the unnormalised cross product; no retail mesh was measured here | measure the M01 water tile and terrain quad edge lengths through the container reader |
| the water-tile nodes' runtime class type (`node+0x34`) | nothing in the conclusion (only 2/3/4 are skipped), but the port's collider mapping | container `node_type` is the container kind, not the runtime class | read the loader that assigns `node+0x34` |
| what writes `V+0xb4` | the unreachable `0.15` damping | only the reset (0) writes it | a wider scan (directive table, network snapshot) or leave it recorded as unreachable |
| `[0x64f66e]`, the gate on writing `V+0x924` after the velocity alignment | whether the alignment reaches the velocity in a live session | writers `0x43db4d`, `0x464693`, `0x4654fa`; `2026-10-21` (d) leaves its meaning unestablished | trace the three writers |
| `record+0x18` and `record+0x20` | nothing measured reads them for the ground blow | no writer found for `+0x18`; `+0x20` is zeroed by `0x55d270` | #2208 (the hit record's position field and the target contact geometry) |

## Instruction bytes cited (re-read by the ignored test)

```bytes
48bfa3: a120c471008b178945088b450cd94508d8088d4de0894d0c8bcfd95de0c745fc00000000d94508d84804d95de4d94508d84808d95de8ff12  # probe.elev_axis: 0x48bf60: [0x71c420] scales the caller's axis; p1 = position + axis*elev
48bf84: 8b4f0c8b41248d770cc1e80224016a00518975cc8845c8e8900a0400  # probe.self_node: 0x48bf60: save node+0x24 bit 2, deactivate the probing vehicle's own node
48c00d: 8d8d40faffff518d55e05250a1acc2710050e89cce030083c410  # probe.query_call: 0x48bf60: 0x4c8ec0([0x71c2ac], p0, p1, out)
48c036: 8b8540faffff25ff0000008d0c808d14483b3d98c271008d9c9544faffff895dd00f8590000000  # probe.pick: 0x48bf60: out[0] & 0xff indexes out+4+0x2c*index; player compare
48c05d: 8b432485c00f848500000089450cf7402800000040c645fc0174718bf056b9b8da7100e85b3e020085c0752366837e540074568b765885f6744f8b3685f6744956b9b8da7100e8383e020085c074dd8a88cc00000084c975308a5dc88b4dcc8b1133c084db0f95c05052e864090400d905c832600083c4085f5e5b8b4df464890d000000008be55dc38b75ccc645fc00  # probe.player_owner: 0x48bf60: player-only hit-owner gate (0x48c036..0x48c0e9)
48c0ed: 895dec8d9f98010000895dc48b4dc48b55ecd901d80ad94104d84a04d94108d84a08d9c9dec2dec1d95d0cd9450cd81dc8326000dfe0f6c4410f85c50000008b45d08b1783c00c508bcfff1250e891c70a00d86d08d9450c8d45d4d9fa89450c895dd0dec9d87508d95d08  # probe.amount: 0x48bf60: dot(Z,record normal), >0 test, (elev-|point-pos|)/elev*sqrt(dot)
48c158: 8b5dec8b4dd08b550cd903d9c0d84904d94304d9c0d84908d94308d9c0d809d9cdd84908d9cbd809d9cbdeedd84904d9cadeebd9c9dee9d9cad95a04d95a08d91a8d4dd451e8ee64f9ffddd8  # probe.blow: 0x48bf60: B = normalize(N x Z) * amount
48c1a4: d945d48d55d4d84d0883c40c89550cd95dd4d945d8d84d08d95dd8d945dcd84d08d95ddc8b5d108b4d0c8b5510d903d801d94304d84104d94308d84108d9cad91ad95a04d95a088b45088945f0  # probe.accumulate: 0x48bf60: *out += B; return amount
48c2c5: 8d45e08945f88b4d0c8b55f8d901d80ad94104d84a04d94108d84a08d9c9dec2dec1d95dfcd945fcd945fcd81dc8326000dfe0f6c4017412ddd8c7450800000000d945fcd80dac366000d80d24c47100eb31  # caller.torque: 0x48c220 player: d = dot(accum,B); d<0 zeroes amount and scales d by 0x6036ac
48c3a1: 8b96340900008d45c889550c8945f8897dfc8b5dfc8b4df88b038b53048b5b08350000008081f20000008081f3000000808901895104895908d905bc2b6200d84d08518d4dc88d55ecd91c245152e80c43fdff  # caller.velocity: 0x48c220: velocity pulled toward the probe axis at 0x622bbc*amount, speed restored
4c8ec0: 8b44240c8b48088b50048b00558b6c240c568b74241856518b4d08528b5504508b4500518b4c2420525051e88000000083c420  # query.entry: 0x4c8ec0: unpack (world,p0,p1,out), call 0x4c8f70
4c8eff: 8a063c01884601766053578d56105255e86cf90600d956208a06d95c24288d7e2033db83c408fec8895c241c880683c72c8d47f0505543e845f90600d854242883c408d917dfe0f6c401740ad95c2420895c241ceb02ddd8fe0e75d28a4c241c5f5b880e5e33c05dc3  # query.nearest: 0x4c8ec0: out[1]=count; nearest by |p0-point|^2 into record+0x1c; out[0]=index
4c8f68: c606005e33c05dc3  # query.one: 0x4c8ec0: a single contact keeps index 0 and leaves record+0x1c unwritten
4c9716: 8b842494000000c705b4e87100000000008a1832d23ad31bc05f405b83c46cc3  # sweep.return: 0x4c8f70: return 0 when out[0] (the count) is non-zero, 1 otherwise
4c8fdc: d944247c55d89c248c000000568b7338dfe0f6c401741b8b8424840000008b8c2490000000a3d0e87100890ddce87100eb198b9424900000008b8424840000008915d0e87100a3dce87100d9842488000000  # sweep.box: 0x4c8f70: componentwise min/max of the two endpoints
4c9141: 8b466085c07550d905d0e87100d85e44dfe0f6c4010f848a050000d905dce87100d85e34dfe0f6c4010f8576050000d905d8e87100d85e38dfe0f6c4410f8462050000d905e4e87100d85e48dfe0f6c4410f854e050000  # sweep.world_box: 0x4c8f70: reject the segment against the world box at world+0x34..0x48
4c9311: 0fbf563a85d2c7442420000000007e7233db8b463c8b0c038b4124a804744ba8107447837c2414007405  # sweep.grid_flags: 0x4c8f70: a grid cell's node list, same 0x04|0x10 flag test
4c998f: 8b475c8b34988b4624a8047450a810744c33c98a4e3051e8852a0a0083c40485c0743aa1ace8710085c0740e8b96c40000003b15f8e69b0077230fbf4756405056e82b000000  # sweep.node_list: 0x4c9980: the world's node list world+0x56/+0x5c, same flag test
4c9a06: 8b4624a8045775095f5eb8010000005bc3a81075095f5eb8010000005bc38505b4e87100  # sweep.node_flags: 0x4c9a00: 0x04 and 0x10 required, mask 0x71e8b4 rejects
4c9a8f: 816624fffffffda1f0e971008a08b3203acb7225  # sweep.cap: 0x4c9a00: the contact cap 0x20
4c9ac8: 8b46348d48ff83f909550f87d7020000  # sweep.type_dispatch: 0x4c9a00: class type dispatch on node+0x34
4c9de0: 899d4c00af9d4c00af9d4c00af9d4c00df9a4c00729c4c00129d4c00769d4c009c9d4c00d79d4c00  # sweep.type_table: 0x4c9a00: ten-entry class-type table
4c7f50: 8b4424048b0df4e97100894124  # record.owner: 0x4c7f50: record+0x24 = the hit node
4c7f69: 8b5040a1f4e97100895028eb2c85c0741c85482474178b504085d27510668378540074098b50588b0285c075e48b40408b0df4e97100894128  # record.sub_object: 0x4c7f50: record+0x28 = node+0x40 or its first descendant with one
4c7fa2: 8b15f4e97100a1f0e9710083c22c8915f4e97100fe00  # record.stride: 0x4c7f50: stride 0x2c and the count byte at out[0]
4c8f0a: 8d56105255e86cf90600d956208a06d95c24288d7e2033db83c408fec8895c241c880683c72c8d47f0505543e845f90600d854242883c408  # record.distance: 0x4c8ec0: |p0-point|^2 into record+0x1c via 0x538880
55d6d2: d9410cd82153d9411056d861048b74241cd941148b542424d8610857d94118d821d95c2410d9411cd86104d95c2414d94120d86108d9c0d8cbd9442414d8cbdee9d954242cd91ed9442410decad8cbdee9d954240cd95e04d9442414decad9442410d8c9deeaddd8d95608  # record.normal: 0x55d6c0: record+0/+4/+8 = the unnormalised cross product of the face's edges
55d7d0: d902d820d8c9d800d95e0cd94204d86004d8c9d84004d95e10d942088b5604d8600881e2ffffff7fd8c9d84008d95c242c8b44242c894614  # record.point: 0x55d6c0: record+0xc/+0x10/+0x14 = the intersection point
47d8d9: 8b817c06000085c0740983f8040f85540700008b410c8b50288d790c81ca000000408950288b07  # owner.aircraft_gate: 0x47d6xx: class 0 or 4 marks the node subtree with 0x40000000
48490a: 8b07814828000000408b07  # owner.group_bit: 0x4848f0: or [node+0x28], 0x40000000
48c078: 8bf056b9b8da7100e85b3e020085c0752366837e540074568b765885f6744f8b3685f6744956b9b8da7100e8383e020085c074dd  # owner.map_walk: 0x48bf60: registry 0x71dab8 walk from the hit node to its vehicle
47c558: 3bde74598b55f0899ac80000008b45f0c680cc000000018b4df089b1d00000008b55f08b4de4c682d4000000018b45f0  # veh.cc_set: 0x47c210: a record path gives vehicle+0xc8 and +0xcc = 1
4b1005: 389ecc0000005557889e45090000889e1d090000889e1f090000889e1e0900000f8500010000  # veh.cc_wake: 0x4b0f40: +0xcc != 0 skips the placement restore on wake
4899ea: 8a86cc00000084c0756a  # veh.cc_far: 0x4899ea: +0xcc != 0 forces the far/simplified update
48d31e: f7432800000040c745fc00000000746d53b9b8da7100e8a72b020085c0752366837b540074578b5b5885db74508b1b85db744a53b9b8da7100e8842b020085c074dd8b887c06000085c9740583f904752cd90570c47100c645f201d805dc326000c645f300d99eac000000d90570c47100d805dc326000d998ac000000  # veh.ac_grace: 0x48d2c0: an aircraft-into-aircraft hit sets +0xac = now + 1.0 on both
48c241: d90570c47100d89eac000000dfe0f6c4010f850c020000  # veh.ac_skip: 0x48c220: a non-player's ground blow is skipped while now < +0xac
48c317: d90528c47100d80d24c47100d90570c47100d89eb4000000dfe0f6c4017412d80da8366000d94508d80da8366000d95d08  # veh.b4_damp: 0x48c220: a non-player's ground blow is scaled by 0.15 while now < +0xb4
4b057f: 899eac000000899eb0000000899eb4000000  # veh.b4_reset: 0x4b05xx: the reset clears +0xac, +0xb0 and +0xb4
4cca5e: 8b4e348d41ff83f80977678b7c241033d28a9008cb4c00ff2495f8ca4c00  # lock.dispatch: 0x4cca30: node activation dispatch on node+0x34
4ccab5: 83ff017509834e24045f33c05ec385ff7504836624fb5f33c05e  # lock.bit: 0x4cca30: arg 1 sets node+0x24 bit 2, arg 0 clears it
6036a8: 9a99193ecdcc4cbd  # const.groundblow: 0x6036a8 = 0.15 and 0x6036ac = -0.05
6032c8: 00000000  # const.zero: 0x6032c8 = 0.0 (the hit-dot threshold and the no-hit amount)
6032dc: 0000803f  # const.one: 0x6032dc = 1.0 (the collision grace)
622bbc: 00000040  # const.two: 0x622bbc = 2.0 (the velocity-alignment rate per unit amount)
62cb54: 556e7265636f676e697a6564206e6f646520636c61737320747970653a20206e6f6465203d20257320636c6173735f74  # message.class_type: 0x62cb54: the class types the query cannot intersect
```

## Tests

Prefix `accept_groundblow_sweep_`, `crates/cs_content/tests/accept_groundblow_sweep.rs`:

* `accept_groundblow_sweep_rows_match_the_finding_and_the_table` (no original
  data): the finding's `bytes` block and the committed
  `[[groundblow_sweep_fact]]` rows name the same addresses and the same bytes,
  the constants the finding quotes decode from those bytes, and the two floats
  `0.15` / `-0.05` are the cited dwords.
* `accept_groundblow_sweep_bytes_match_the_engine_image` (ignored, needs
  `CS_ENGINE_IMAGE`): loads the image through the production loader
  (`cs_content::coordinates::load_engine_image`, which refuses a drifted digest)
  and re-reads every row.
