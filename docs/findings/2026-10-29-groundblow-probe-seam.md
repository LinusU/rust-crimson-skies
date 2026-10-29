# #2210 GROUNDBLOW-PROBE-SEAM: the imported globals, the probe seam, and why the world sweep is not wired

Date: 2026-10-29. Task #2210 (`GROUNDBLOW-PROBE-SEAM`), the second prerequisite
of #2107 (the ground-blow assist), built on #2209's
`docs/findings/2026-10-29-groundblow-world-segment-sweep.md` ("the sweep
finding"). Implementer: `deepseek-1`. Every original-game statement below is the
sweep finding's static analysis (`OWNER-STATIC-2026-10-29`), never
`verified_original`: the original executable was not run.

## 1. What landed

1. **The three ground-blow globals are imported as data.** `player.zrd`'s first
   entry states `groundblow_elev = 400`, `groundblow_mag = 10`,
   `ai_groundblow = 0.5` (sweep finding §1; §4.1 of
   `2026-10-25-force-block-reread.md`). They are now part of
   `cs_content::original_airframe::GLOBAL_FIELDS` and its mirror
   `cs_sim::flight::original::GLOBAL_FIELDS`, with the elevation's name carrying
   its unit (`groundblow_elev_m`, metres, §2 of the sweep finding).
   `OriginalGlobals` carries them as `groundblow_elev_m`, `groundblow_mag` and
   `ai_groundblow`; `OriginalDocuments::globals()` reads them through the
   production importer, and a list that omits any of the three is
   `OriginalParamsError::MissingField`, never a default. The retail test
   `accept_groundblow_probe_reads_the_retail_ground_blow_globals` reads
   400 / 10 / 0.5 from `$CS_GAME_DIR` and feeds the same flat list into the law.
2. **The segment-probe seam exists on `OriginalInput`.**
   `OriginalInput::segment_probe: Option<SegmentProbeHit>` carries exactly the
   three facts the force block (`0x48c0ed`..`0x48c155`) and the player's skip
   gate (`0x48c036`..`0x48c0e9`) read from the hit record:
   * `distance_m` — the chord `|p0 - point|` the original computes with
     `0x5388d0` (record `+0x0c` is the point; `+0x1c` is the sweep's own
     squared chord, used only to pick the nearest record);
   * `normal` — the record's `+0x00`, **unnormalised** (the face's edge cross
     product, magnitude = face parallelogram area), so `sqrt(dot)` keeps the
     original's `sqrt(area)` scale;
   * `owner` — `SegmentProbeOwner::World` / `UnresolvedVehicleNode` /
     `Vehicle { on_path }`, the three resolutions §5 of the sweep finding
     distinguishes, where `on_path` is the vehicle's `+0xcc` byte.
   Nothing reads it yet: #2107 adds the consumer, and
   `accept_groundblow_probe_filled_probe_changes_no_trajectory` proves a filled
   probe leaves every step and the state identical over 200 ticks for both
   seats.

## 2. Why the probe is not filled: the task's own condition is met

Item 3 of the task says to fill the probe "from the existing M01 terrain/sea
query as the decode finding prescribes", and to block with the evidence
"if the decode says the existing query cannot answer what `0x4c8ec0` answers".
The decode says exactly that (§0.4 and §8 of the sweep finding):

| the original's `0x4c8ec0` needs | the port's nearest query (`SpatialQuery::cast_ray_predicate`, Avian 0.7) |
| --- | --- |
| dynamic targets: an aircraft's node answers the sweep (§3) and is then filtered by the player's `V+0xcc` gate (§5) | the two existing call sites filter to **static** bodies (`turret_fire.rs::line_of_sight`, `targeting_input.rs`) |
| the **unnormalised** face normal (magnitude = area) | Avian returns a **normalised** ray-hit normal: this workspace's `parry3d` 0.31.1 builds it as `n.normalize()` in `src/query/ray/ray_triangle.rs`, so `sqrt(dot)` would lose the original's `sqrt(area)` factor |
| the chord `|point - position|` from the record's contact point | the existing callers keep the along-ray distance and no point |
| the class-type filter (`node+0x34` of 2/3/4 and outside 1..10 produce no contact, §3) | the port spawns a collider for **every** placement storing `0x14` (#1814: water tiles as `PlaytestSea`, the rest as `WorldSceneryCollider`), and the runtime `node+0x34` of even the water tiles is unmeasured (§7, §9) |

Filling the seam from what exists would therefore hand #2107 a normalised normal,
a static-only hit and an unfiltered collider set while calling it "the record".
That is the approximation item 3 forbids, so it is not done here.

## 3. And the call site the task names does not step the player's law

Item 3 names `crates/cs_app/src/mission_session/compose.rs` or `content.rs` as
"whichever steps the player's `OriginalFlightModel`". Neither does:

* `mission_session::content::prepare_flight` only **builds** the
  `OriginalFlightModel` from the imported airframe and globals;
* `mission_session::compose::spawn_player` **spawns** the body and inserts the
  `PlaytestOriginalFlight` record (model, state, held command);
* the law is **stepped** by `playtest::drive_original_flight`
  (`crates/cs_app/src/playtest/mod.rs`), a fixed-tick system that seeds the
  state from the body's `Position`/`Rotation`/`LinearVelocity` and hands the
  step's force back; and the `OriginalInput` is built by
  `PlaytestOriginalFlight::input()` (`crates/cs_app/src/playtest/scene.rs`),
  which has no physics world to query.

So even a complete, exact query could not be wired from the owner paths this
task lists. The seam's `segment_probe` field is carried by
`PlaytestOriginalFlight::input()` as `None` (the one line this task added
there, because the struct's field list is exhaustive), with a comment naming
the completing query (#2214) and the consumer (#2107).

## 4. What the follow-up needs

A task that fills the probe needs, in one place:

1. an owner path that includes `crates/cs_app/src/playtest/mod.rs` and
   `crates/cs_app/src/playtest/scene.rs` (the step and the input builder), or a
   new query module plus a way for the input builder to reach it;
2. a segment query over the Avian world that keeps **dynamic** bodies, excludes
   the prober (the original does it with `0x4cca30(node, 0)`; the port filters
   by entity), and returns the hit entity, its **contact point** and its
   **unnormalised** triangle normal;
3. the class-type predicate of `0x4c9a00` — the runtime `node+0x34` of M01's
   water tiles, mountains, cliffs and land is still unmeasured (§7, §9 of the
   sweep finding), so this predicate cannot be written from what the container
   reader exposes today;
4. the `0x40000000` owner marker and the `V+0xcc` byte for the player's gate,
   which is what `SegmentProbeOwner` already names: the port has to say which
   of its aircraft entities corresponds to a registered vehicle and which of
   them is "on a path" (the record loader's flag).

Until then `OriginalInput::segment_probe` stays `None` in every production
path, which is what makes the owner smoke unchanged by this task.

## 5. Owner decision, 2026-10-29 (post-block)

The owner retitled this task to items 1 and 2 only and moved item 3 out of it:
#2214 (`GROUNDBLOW-SWEEP-FILL`) owns building the sweep and filling
`segment_probe` at the playtest flight step, with owner paths
`crates/cs_app/src/playtest/mod.rs`, `crates/cs_app/src/playtest/scene.rs` and
a new `crates/cs_app/src/playtest/world_sweep.rs` — the site §3 derives. The
class-type predicate of §4.3 got its own measurement task, #2213
(`GROUNDBLOW-CLASS-TYPE`), which #2214 waits on. This branch therefore lands
the globals and the seam only, exactly as §1 lists them.
