# HIT-RECORD-CONTACT-POINT: what fills `[record+0xc]` on a contact, and what that point is on the target's collision volume

Task #2208. Follows `2026-10-29-t452-tangler-contact-centre.md`
(`T452-REG-TANGLER-CONTACT-CENTRE`, task #2205), which
measured the consumer: the contact-burst path copies `[record+0xc]` into
`rocket+0x48` before `0x5ac7a0`, so a burst — including `wep_12`'s tangler field
— is centred on this slot. This finding measures the producer: the instructions
that write `[record+0xc]` and the target geometry the point is taken on.

Static analysis only (Capstone 5 over `$CS_ENGINE_IMAGE`, image base
`0x400000`). No original executable ran; nothing here is
`verified_original`. The committed byte citations are the
`[[hit_record_fact]]` rows of `data/owner-static/engine-facts.toml`, re-read by
the `accept_hit_record_` image test.

## The writer of `[record+0xc]`

`[record+0xc..0x14]` is written by **`0x55d6c0`**, the engine's
segment-vs-convex-face test — and it is the *only* writer of the field in the
collision family: `0x55d270` (box), `0x55c9c0` (soup) and their siblings
`0x55db90`/`0x55bbc0`/`0x55d0xx` all delegate the accepted-face point to it;
`0x55db90` and `0x55bbc0` never store to `record+0xc` themselves. Per axis it
evaluates

    record+0xc = seg_start + t * (seg_end - seg_start)

at the parameter `t` where the query segment crosses the accepted face's plane
(`0x55d7d8` stores x, `0x55d7e6` y, `0x55d7ff..0x55d805` z; the face normal goes
to `record+0x00..0x0b` at `0x55d713`). So the record position is **a point on
the projectile's own swept segment, at the place it pierces the accepted
collision face** — the same construction the runtime now uses
(`previous + t * (current - previous)`); only the surface `t` is measured
against can differ.

The segment itself is the projectile's tick sweep. The per-frame update builds
`rocket+0x54 = rocket+0x48 +` this tick's displacement (`0x5af854..0x5af889`)
and calls `0x5b03f0(weapon, rocket, &rocket+0x54)` (`0x5af899`). `0x5b03f0`
runs the scene query `0x4c8ec0(db, &rocket+0x48, &rocket+0x54, &list)` at
`0x5b0483`, and again for the stored-contact-queue path at `0x5b0554` — both
sites sweep the identical `rocket+0x48 → rocket+0x54` segment.

The other burst consumers produce their records through the same query:
`0x5acc70` (the contact test `0x5b0a50` runs) calls `0x4c8ec0` at `0x5acfb8`
and `0x5ad330` at `0x5acfd1`, and `0x5b0a50` itself re-queries from the first
contact point at `0x5b0b9e` — it copies `record+0xc` into `rocket+0x48`
(`0x5b0bae..0x5b0bbe`) before its `0x5ac7a0` call at `0x5b0bec`, the same move
`0x5b05c0` makes at `0x5b06df..0x5b06f4`. Every hit record a burst path
consumes is therefore a `0x55d6c0`-written segment∩face point.

## The record and the pick

`0x4c8ec0` → `0x4c8f70` walks the scene spatial grid and tests each admitted
node with `0x4c9a00`, which dispatches on `[node+0x34]` through the ten-entry
jump table at `0x4c9de0` (other classes are skipped). Each accepted node
appends one 44-byte element at the query tail (`+0x2c` stride, `0x4c9e96`):

| field | content | written by |
| --- | --- | --- |
| `+0x00..0x0b` | accepted face's normal | `0x55d6c0` (`0x55d713`) |
| `+0x0c..0x14` | contact point on the segment | `0x55d6c0` (`0x55d7d8..0x55d805`) |
| `+0x1c` | distance from `seg_start` | `0x4c8ec0`'s sort pass (`0x538880` at `0x4c8f0a`) |
| `+0x20` | hit face's surface object | `0x55c9c0` (`0x55cdce`), or `0` for box hits (`0x55d27f`) |
| `+0x24` | the node | tail write `0x4c9e54` |
| `+0x28` | the node's surface row | tail write `0x4c9e8d` (`[node+0x40]`; the `0x200000` chain at `0x4c9e68`) |

`0x4c8ec0` then computes each element's `+0x1c` and stores the index of the
nearest into `[list]` (`0x4c8f5b`) — the earliest pierced face across nodes,
since all points sit on the same segment. `0x5ad330` (the pick `0x5b03f0` calls
with the weapon, zero, the list) normally returns that element. Two measured
exceptions:

* weapon flag `0x40000000` (`[weapon+0x74]`) returns the nearest element
  unconditionally;
* if the nearest element's surface object exists and `[surf+0x20] == 1`, the
  face is effect-only: `0x5ad330` spawns the surface's impact effect via
  `0x4edc10` at the recorded point, then returns the nearest remaining
  **solid** contact (`fcom [ecx+0x1c]` rescan, `0x5ad3f9`), or `0` when the
  effect-only face was the only record. The earlier `hit.best_contact_filter`
  fact names this skip; the `+0x20` it tests is the face's surface descriptor,
  which is `0` on a bound-box hit.

## The target's collision volume

For each node `0x4c9a00` first rejects on the node's bound (the slab test
inside `0x4cc010`), then the exact contact surface is one of:

* **the node's bound box** — `[node+0x24] & 0x20` → `0x55d270` tests the
  segment against six quad faces built from the node's eight stored bound
  corners, in the query's world frame. It returns on the **first** accepted
  face in authored corner order (`0x55d31e`), not the minimum-`t` face: for a
  segment that crosses the box completely, the recorded point may be on the
  exit face, not the entry face.
* **the node's polygon soup** — flag clear → the leaf class handlers run
  `0x55c9c0` over `[node+0x3c]`, transforming the segment into node-local
  space, testing faces in soup order, and transforming the accepted point back
  to world in place (`0x55cde7..0x55ce4e`, normal rotated without the
  translation row at `0x55ce51..0x55cea9`).

So on an aircraft-scale target "the point of contact" is where the projectile's
swept segment pierces **a per-part collision face**: either that part node's
bound-box face (flag `0x20`) or a polygon of its collision soup — never the
actor centre, and not one monolithic hull. Container classes (5/6/7) only route
the traversal to their children. The query's own gates before that are the node
flags (`[node+0x24] & 4 && & 0x10`), the `0x40000` mask (`0x5b0330` sets
`[0x71e8b4]`), the node-type filter `0x56c430` on `[node+0x30]` (the round's
own filter set is staged at `0xa070a8`), and the serial cutoff
`[node+0xc4] > [0x71e8b0]` (set to `rocket+0x7c`, the self-exclusion).

## Shift vs the runtime's swept-segment point

The runtime interpolates `previous + t * (current - previous)` with
`t = SweptHit::time_of_impact` measured against an axis-aligned swept box: each
target part's `target_half_extents_m` is the node's stored bound in world axes
(`mission_targets.rs` / `mission_actors.rs`, already labelled unmeasured as
F27-D). The original's `t` is measured against the node's collision volume —
the 8-corner bound (oriented in the node's frame) or its polygon soup.

Both points lie on the same swept segment, so the shift is **along the
segment**: `Δs = |t_runtime − t_orig| × 40 m` for `wep_12` (1200 m/s × 1/30 s),
i.e. the distance the segment travels between the runtime's box face and the
original's pierced face.

* Bound-box parts: the original's surface is the node's bound as stored —
  oriented; the runtime's is the world-axis hull of that bound. `t_orig ≥ t_runtime`,
  and `Δs` is bounded by the axis-aligned-vs-oriented slack along the flight
  direction, bounded in turn by the part's bound depth (metre-scale for
  aircraft parts).
* Soup parts: the mesh lies inside the bound, so `t_orig > t_runtime` by the
  bound-to-mesh gap along the ray — bounded by the part's bound depth in that
  direction. Categorically, a segment that enters the runtime's box but never
  reaches the mesh is a runtime contact the original does not produce (or
  produces on a later node); an effect-only face ahead of the solid surface
  pushes `t_orig` further still.
* First-accepted-face ordering inside a node means the original can report a
  later (far-side) face than the entry face for a through-and-through segment
  — a further small positive shift, bounded by the part's bound depth.

In all measured paths `Δs` is bounded by the contacted part's collision depth
along the segment — part-scale (metres) for aircraft-scale targets, far under
the 40 m one-tick bound the previous finding had to assume. Per-part numbers
stay unmeasured with F27-D; `1 m` of surface gap is `~0.83 ms`, `~2.5 %` of the
tick in `time_of_impact`, and the full gap in the field centre's displacement.

Affected content: `wep_12`'s tangler field centre on a contact burst — which
actors the 10 m field catches near its edge — and every other contact
detonation consuming `record+0xc` (both `0x5b05c0` and `0x5b0a50` burst paths
write it into `rocket+0x48` for `0x5ac7a0`). No runtime value changes in this
task; the resolving follow-up is measuring each part's collision volume and
sweeping against it instead of the stored bound's axis-aligned hull.

## Relation to `t452_reg.limit.tangler_field_origin`

Consistent, not contradicting: #2203 measured that the field centres on
`rocket+0x48` after `0x5b05c0` copies `record+0xc` into it. This measurement
shows `record+0xc` is a point on the projectile's swept segment — the
interpolated contact point the claim name describes. The claim stays closed;
the residual error moves from "up to one tick of displacement" to the
per-part surface gap above.

## Not measured

* Which of the five `0x5ac7a0` callers a real `wep_12` contact reaches
  (unchanged from #2203; the selecting weapon flags are untraced).
* Which class and which volume each retail target part actually uses — the
  dispatch shows both box and soup volumes exist; which applies to a given
  node is in the data (`[node+0x24]` flag, `[node+0x3c]` soup), not yet
  extracted per part.
* Whether the class-data bound `0x55d270` reads is byte-identical to the
  stored display bound the runtime reads; both derive from the node's bound
  record, but the store is unmeasured.
* The `rocket+0x608`/`rocket+0x610` cached-contact and `rocket+0x78` queue
  writers `0x5b03f0` also consumes: same 44-byte layout, filled by the same
  `0x55d6c0`-family writers through other query sites, not individually
  traced.
