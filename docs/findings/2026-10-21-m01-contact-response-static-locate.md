# #1848: where the contact response gets its points, its sea and its damage (static)

Date: 2026-10-21. Implementer: `claude-1`. Provenance: **`OWNER-STATIC`** reading
of the owner's decrypted image `crimson.decrypted.exe` (image base `0x400000`,
`llvm-objdump -d`, output kept in the ignored `private/`). Nothing here is an
observed behaviour and nothing is `verified_original`. No code changed.

This continues the static reading of #1752 (`docs/findings/2026-10-21-m01-collision-response.md`
on branch `rally/1752-collision-response-in-m01-the-original-s`, not yet on
`main`). Addresses are virtual addresses in the image. `V` is a vehicle object;
`player` is `[0x71c298]`. Item letters are the task's.

## (a) Who fills the collision-point list `V+0x6a4..0x6a8`

`V+0x6a0` is a std::vector-style triple (`+0x6a0` allocator byte, `+0x6a4`
begin, `+0x6a8` end, `+0x6ac` capacity). Records are 36 bytes (`0x24`):
`+0x00` local point, `+0x0c` new world point, `+0x18` previous world point.

* **Source of the local points: the `collision` block of the vehicle's
  airframe record.** The parser at `0x47a82c..0x47acb7` reads the child `platform`
  (`0x627f68`) into a point list at params`+0xc8`, then the child `collision`
  (`0x627f74`, `0x47aa95`) into a second list at params`+0xd8` (begin
  params`+0xdc`, end params`+0xe0`). Each entry is a block child with three
  numbers (`[node+0xc]`, `[node+0x14]`, `[node+0x1c]`), pushed as a 12-byte point.
* **Copy into the vehicle.** The vehicle constructor `0x475820` takes params
  (`V+0x64 = params`, `0x47583d..0x475842`) and, for every point of params`+0xdc..+0xe0`,
  inserts one record at the end of `V+0x6a0` (`0x475add..0x475b3d`, insert at
  `0x484370`): local point = the 12 bytes, both world points = the zero vector
  `0x75d1b8`. Nothing else appends to the list; the other writers of
  `+0x6a4/+0x6a8` are the zeroing constructor `0x4b022d` and the destructors.
* **Re-seeding.** `0x476250` (called at `0x47c77b`) sets the previous world
  point of every record to the vehicle position `V+0x204` through `0x486100`
  and clears `V+0x6b0`. `0x48e517..0x48e56f` repeats that at the end of every
  contact sweep: previous := new (`+0x0c` to `+0x18`), `V+0x6b0 := 0`.
* **Per sweep (`0x48d88b..0x48d9a8`).** new world point = `local * R + T + delta`,
  with `R` = the 3x3 at `V+0x180..0x1a0`, `T` = `V+0x1a4..0x1ac` (the y and z
  results are stored swapped, `0x48d920..0x48d925`) and `delta` the caller's
  step. Each record yields one segment *previous world point -> new world
  point* (24 bytes) in a local array; the segment count handed down is `2 * records`.
* **Which markers or hull points.** Only the airframe record's `collision`
  points. Model markers are not read here. Whether a retail airframe record
  has the block, and how many points the player's Flight Check airframe has,
  is **unknown**: no airframe record was read in this task (retail data read
  was out of scope for a static task). Resolving task: read the `collision`
  block of the M01 player airframe with `OriginalDocuments`.
* Params`+0xac/+0xb0` are loaded from a key `collision_d` (`0x6280c4`,
  `0x47b0ac..0x47b0d6`, two numbers). Its consumer was not located: **unknown**.

## (b) The world query and the sea

* `0x4ca320(world, segments, count, out)` and `0x4c8f70(world, p0, p1, out)`
  (the one-segment form, taken when the vehicle has exactly one record,
  `0x48d9ca`) are the same sweep. `count` is halved (`0x4ca36f`: segments =
  `count/2`, at most 12). `out` is 0x584 bytes per segment: a count byte, then
  up to 0x2c-byte contact records from `out+4` (normal at `+0`, point at `+0xc`,
  `+0x20` a sub-id, `+0x24` the object that owns the hit surface).
* The sweep rejects a segment whose box is outside the world's box
  (`0x4ca3c0..0x4ca498`, world`+0x34..0x48`). It then tests two sets:
  1. the world's **node list** (`world+0x56` count, `world+0x5c` array,
     `0x4ca506..0x4ca55e`): a node is tested through `0x4cad00` only when
     `node+0x24` has **both bits `0x04` and `0x10`**, and the node's layer byte
     `node+0x30` passes the filter `0x56c430`, which accepts everything unless a
     layer list at `0xa070a8` is armed (`0x56c43a`, `0x56c44d`). `0x4cad00`
     dispatches on `node+0x34` (type 1..10, jump table `0x4cb0b0`);
  2. the world's **grid** (cells indexed from world`+0x84/+0x88` scales, dimensions
     `+0x98/+0x9c`, cell rows `+0xa0`, `0x4ca6d0..0x4ca77a`): per cell a list of
     `cell+0x3a` nodes (`cell+0x3c`), tested with the same two flag bits through
     `0x4cad00`, plus a second list `cell+0x40` through `0x4caa80`.
* **There is no sea plane or sea level constant in the sweep.** None of the
  prologues or loops read above compare a coordinate against `0` or a water
  height, and the sweep never asks a surface type. A contact with the sea exists
  **only if some node or grid node has bits `0x04|0x10` set and a type
  `0x4cad00` can intersect**. Whether M01's water tiles (y = 0) carry those bits
  is a question about the world data and its loader, **unknown here**: the
  loader that sets `node+0x24` was not followed, and #1752's note that our build
  has no sea collider is a consequence of the importer, not evidence about the
  original. (A name table `default, water, seafloor, quicksand, lava, fire` at
  `0x637b14..0x637b28` is a material/surface-name list; it has no reader in
  the sweep.)
* The image also asks "how far above ground" with its own downward segment:
  `0x48bc40` finds the vehicle's child node `ground_level` (`0x628a04`), casts
  from it with the one-segment sweep (`0x4c8f70` at `0x48bd7e`) and maps the
  distance through the range globals `0x71c3f8..0x71c418`. It is a second consumer
  of whatever the sea reports; its caller and the meaning of the globals are
  **unknown**.
* Resolving task: read the M01 water nodes' flag word (`node+0x24`) and type from
  the world container, and check bits `0x04|0x10`.

## (c) Contact selection, `0x48dc09..0x48def5` (player only)

Per point `i` and per contact `j` of that point (`0x48dc24..0x48dc78` first
normalises every contact normal through `0x422690`):

1. `P = contact_point(i,j) + previous_world_point(i)` (`0x48dd1c..0x48dd33`;
   the sum is as the code adds it, a unit or frame reading is not made here);
2. for every point `q`, every contact `k` of `q`, and every list record `m`:
   `d = P - contact_point(q,k)`, `u = dot(d, n_k)` (`0x48ddc1..0x48ddfd`),
   `val = dot(previous_world_point(m), n_k) + u` (`0x48de55..0x48de71`), and the
   **minimum** `val` over all `(q,k,m)` is the score of `(i,j)`
   (`0x48de74` keeps the smaller; the minimum starts at `+FLT_MAX`, `0x48dd00`);
3. the candidate with the **largest** score wins (`0x48ded3..0x48def5`, starting
   at `-FLT_MAX`); its point index `[ebp-0x1c]` and contact index `[ebp-0x4]`
   select the record used by the response.

So a contact scores high when it is the one whose plane keeps every other
point and contact on its inner side, i.e. the deepest/most opposed plane. This
is the reading of the instruction stream; the signs of the two terms were not
cross-checked against a trace. After the response, `0x48e4ff..0x48e50f` copies the
chosen 0x2c-byte contact record to the caller's `info` (arg 2), so `info` is the
contact record, with `info+0xc` the point and `info+0x24` the hit owner.
Non-player vehicles take a different branch from `0x48da4e`: they use the first
point that has any contact (`0x48da56..0x48da8b`) and skip the clip when
`V+0x920` is set (`0x48dac1..0x48dac9`).

## (d) `V+0x384`, the damage pair and the `crash_*` keys

* **`V+0x384` is a debug/cheat flag, not gameplay.** It is zeroed in the constructor
  (`0x4b08c5`) and set to 1 only by: the command-line flag `-fd` (sets `0x71dac9`,
  `0x4a74da`, applied to the player at `0x4753ef`); the console command `fd <0|1>`
  (`0x43e5dc`, string `fd` at `0x622ee8`); and the console command `ifon`
  (`0x43e626`, string `0x622eec`), which also sets `V+0x920`. Under it the player's
  contact response is skipped (`0x48dfb8`) and no damage is armed (`0x48d3aa`
  jumps to the damage arithmetic without the effect call). `V+0x920` is set by
  `-nodie` (`0x71dac8`, applied at `0x4753da`), the console `nodie` (`0x43ea71`)
  and `ifon`; it skips the whole self-damage block (`0x48d563`). In a normal
  retail session both are 0. (`-win`, `-too`, `-start` are the neighbouring
  flags in the same parser.)
* **The pair.** `0x48d2c0` computes `(armor, health) = (max(a2*s^3, a1), max(h2*s^3, h1))`
  from the `crash` record values (`0x71c34c/50/54/58`; #1752 has the table) and
  doubles both when its local `-0xe` is set. `-0xe` is **not** "the player is
  hit": it is set only on the non-player path (`0x48d375`) when the contact's
  owner resolves to a vehicle of class 0 or 4 (`[vehicle+0x67c]`); the same
  branch gives both vehicles a collision grace period (`V+0xac = now + 0x6032dc`,
  and `0x48d807..0x48d819` makes the sweep return at once while `now < V+0xac`).
  For the player the pair is never doubled.
* **Two consumers of the pair.**
  1. *The hit owner*: when `info+0x24` is non-null, `0x5abcf0` is called with
     the weapon record found by name `wep_24` (`0x628a14`, `0x48d539`), the
     owner, `info` and the pair (`0x48d54b..0x48d55b`). Inside, an owner whose
     handler `owner+0xbc` is the value 1 loses `pair[0]` from `[owner+0x40]+0x670` and
     `pair[1]` from `+0x674`, floored at 0, and the callback `[0xa1e190]` is
     called with kind 1 (`0x5abd4e..0x5abdcf`); any other handler is called
     through its function pointer `[handler+4]` (`0x5abe68`). Plain terrain with
     no owner (`info+0x24 == 0`) gets **no** call at all (`0x48d537`).
  2. *The vehicle itself*: unless `V+0x920` (`0x48d563`) and when `V+0x9c..+0xa0`
     (88-byte damage zones) is non-empty, `0x4b3950(V, info)` picks a zone and
     `0x4b3bf0(V, zone, &pair)` applies the pair to it (`0x48d70c..0x48d724`,
     repeated while armor or health of the pair remains and `V+0x2d0 > 0`,
     `0x48d729..0x48d75c`). A zone has `+0x24` max armor, `+0x28` armor,
     `+0x2c` max health, `+0x30` health (`0x4b3cd0..0x4b3ce4`); afterwards
     `V+0x2d0 = sum(zone+0x30) * V[0x2cc] / sum(zone+0x2c)` (0 when the divisor is 0)
     and `V+0x2c8 = sum(zone+0x28) * V[0x2c4] / sum(zone+0x24)` (`0x4b3cf3..0x4b3d4f`).
  How `0x4b7f80` splits one pair between a zone's armor and health was not read.
* **Destruction threshold.** After the pair is applied the test is on
  `V+0x2d0` (the vehicle's health fraction scaled by `V[0x2cc]`): at `<= 0` (or
  when the local `-0xd` is set, which only the non-player branch sets,
  `0x48d306`) `0x48b920(V, info)` runs (`0x48d7c6`); otherwise `0x4b3800` and
  `0x4b1790` refresh the damage state (`0x48d7a5..0x48d7ac`). `0x48b920` is the
  removal path (virtual calls `+0x3c`, `+0x48`, `0x493110`, `0x440ad0` for the
  player); its body was not decoded. So the **threshold is 0 on the zone-health
  sum**, and what a given `(armor, health)` of 50 does to the player's zones
  depends on the airframe's zone values (**unknown**, not read).
* **The effect call.** For the player `min(|v| * s * 0.03, 0.15)` is handed to
  `0x42c070(camera 0x64ef78, 5, value)` (`0x48d3cc..0x48d409`; `0x3e19999a =
  0.15`, `0x6080c4 = 0.03`). For non-players `0x473430(V, 2)` spawns the effect
  `[0x71c2f4 + 4*2]` once (`V+0x6ec` holds it). Both are cosmetic.
* The caller gate `[0x64f66e] != 0` at `0x48d2db` skips the whole damage block;
  writers `0x43db4d`, `0x464693`, `0x4654fa`, meaning **not established**.
* **The five `crash_*`/`death_*` keys are camera parameters, not physics.**
  They sit in the `camparam.zrd` loader (`0x42fa85`; keys `back_dist_min`,
  `pos_catch_up`, `thirdp_*`, `death_*`, `flyby_*`) and fill the camera
  parameter struct read at `0x42f700` (one default at `0x64efd8`, eleven per-mode
  copies from `0x64f058`, 0x80 bytes each, current one in `[0x64efd0]`). Offsets
  (this corrects #1752's list, which was one slot early): `death_alt +0x3c`,
  `death_min_alt +0x40`, `crash_horiz +0x44`, `crash_y +0x48`, `crash_chord_y +0x4c`,
  `crash_min_elev +0x50`. Consumers, all camera placement:
  * `0x42df90` (called from the death/crash camera dispatch at `0x42ce15`,
    `0x42dc85`): the camera position is `V+0x204` plus `crash_horiz` along the
    horizontal direction of `V`'s matrix, plus `crash_y` in height; then
    `0x42c580(pos, crash_chord_y, crash_min_elev)` (`0x42e009..0x42e04e`);
  * `0x42e0b0` (from `0x42cf24`, `0x42dc99`): height `+= death_alt`, clamped
    below at `death_min_alt`, then the same `0x42c580` (`0x42e1ae..0x42e1e4`);
  * `0x42e387..0x42e394` calls `0x42c580` with the same two values.
  None of them touches the vehicle's state, velocity or damage. They matter for
  the view after a crash and not for the crash itself.

## (e) The ground-blow term `0x48bf60` in the control law `0x48c220`

* `0x48c220(V, accumulator)` returns at once when `V+0x358 == 4` (`0x48c22b`) and,
  for non-players, while `now < V+0xac` (`0x48c241..0x48c252`).
* It casts a segment from the vehicle's position along the **negated third row of
  `V`'s matrix** (`V+0x198..0x1a0`, negated at `0x48c28f..0x48c29a`; the vehicle's
  third axis, not world down), of length `groundblow_elev` (`[0x71c420]`,
  retail 400), with the one-segment sweep `0x4c8ec0` (`0x48c01f`) from `0x48bf60`.
* On a hit it builds a blow direction as a normalised cross product of the contact
  normal with that axis (`0x48c161..0x48c19d`), scaled by the strength
  `s = (elev - distance) / elev * |...|` (`0x48c13a..0x48c152`), and returns `s`.
  A hit owner that is a vehicle with `[vehicle+0xcc] != 0` zeroes the term
  (`0x48c0ac..0x48c0e6`, player only).
* **It applies to the player.** The `V == player` branch (`0x48c2c3`) takes
  `dot(accumulator, blow)`; a negative dot drops `s` to 0 and scales the dot by
  `0x6036ac`; the result is multiplied by `groundblow_mag` (`[0x71c424]`, retail
  10) and the blow vector times that is **added to the accumulator**
  (`0x48c348..0x48c369`). For others the factor is `ai_groundblow *
  groundblow_mag` (`0x48c317`), cut by `0x6036a8` while `now < V+0xb4`. The
  accumulator then feeds the velocity update that writes `V+0x924`
  (`0x48c3a1..0x48c43a`).
* Whether it acts on the **sea** follows (b): it is a push only when the cast
  finds a surface with the two flag bits within 400 units, so over a sea
  that is not a node or grid hit it is zero. For the measured no-input flight
  over the water this is therefore **unknown**, not zero.

## What this changes for #1752

* The collision-point list exists to be read from the airframe `collision` block;
  the importer needs that block and a consumer, not a model-marker guess.
* The sea needs the world data's flag word, not a rule in the engine.
* The `crash_*` keys have no place in the contact response: they are camera
  framing and belong to the death/crash camera.
* The player's death is the zone-health sum reaching 0, fed by the pair; a
  graze costs the floor of 50 on armor and 50 on health, spread over the
  airframe's zones.

## Unknown, and the task that resolves each

| item | evidence | resolved by |
| --- | --- | --- |
| number and positions of the player airframe's `collision` points | block parsed at `0x47aa95`, not read | read the M01 player airframe |
| M01 water nodes' `node+0x24` flags and type | sweep needs bits `0x04|0x10` | read the M01 world container |
| zone armor/health of the player airframe, and `0x4b7f80`'s split | zone fields `+0x24..+0x30` | read the airframe `armor`/`health` blocks and decode `0x4b7f80` |
| `0x48b920` body (destruction) and what ends M01 on it | called at `0x48d7c6` | decode `0x48b920` |
| `collision_d` consumer (params`+0xac/+0xb0`) | parsed at `0x47b0ac` | find readers of the airframe struct fields |
| meaning of `[0x64f66e]` | gate at `0x48d2db` | trace the writers |
| `ground_level` distance consumer `0x48bc40` | one-segment sweep | find its caller |
