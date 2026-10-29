# #2214 GROUNDBLOW-SWEEP-FILL: the port's world sweep over M01's colliders, and the probe filled at the playtest flight step

Date: 2026-10-29. Task #2214 (`GROUNDBLOW-SWEEP-FILL`), the third
prerequisite of #2107 (the ground-blow assist). It builds on #2209's decode
(`docs/findings/2026-10-29-groundblow-world-segment-sweep.md`, "the sweep
finding"), #2210's seam (`docs/findings/2026-10-29-groundblow-probe-seam.md`),
and #2213's class-type measurement
(`docs/findings/2026-10-29-groundblow-class-type.md`). Implementer:
`deepseek-1`.

Every original-game statement below is the decode's static analysis
(`OWNER-STATIC-2026-10-29`), never `verified_original`: the original executable
was not run. The port-side numbers are measured by the task's own acceptance
tests against `$CS_GAME_DIR`, and are values of this port, not of the original.

## 0. What landed

1. **`crates/cs_app/src/playtest/world_sweep.rs`** is the query.
   `ground_blow_segment(position, orientation, groundblow_elev_m)` builds the
   probe's segment from the body's position along `-V[+0x198]` — in this port
   the nose, `nose_direction`, because the port's body forward is `-Z` and the
   original's negated third matrix row is that axis — `groundblow_elev_m` (400)
   long. `nearest_contact(segment, targets)` returns the nearest accepted
   contact as `WorldSweepContact { entity, point_m, normal, distance_m, owner }`,
   and `hit()` reduces it to the three facts `SegmentProbeHit` carries.
2. **The population** is the colliders the port spawns for original content:
   `PlaytestSea` water tiles and `WorldSceneryCollider` land, mountains and
   cliffs (#1814), plus every body on the `CollisionLayer::Aircraft` layer but
   the prober. `owner_of` states the resolution: scenery and sea are
   `SegmentProbeOwner::World`, an aircraft body is
   `SegmentProbeOwner::Vehicle { on_path: false }`, the prober is not tested.
3. **The record's conventions** are produced as the decode measures them:
   * the normal is the hit face's **unnormalised** edge cross product — for a
     triangle the collider's own three vertices (`|N| = 2 * area`), for a
     cuboid the engine's measured box rule `0x55d270` (`|N| = width * height`,
     outward). parry's normalised normal is never stored;
   * the distance is the chord `|point - from|`, recomputed from the hit point,
     not the along-ray time of impact;
   * the measured one-sided face gate (`0x55d6c0`: the segment starts strictly
     on the `+N` side and ends strictly on the `-N` side) selects faces;
   * the class-type filter is `class_type_intersects` over the dispatch of
     `0x4c9ac8` (`1, 5, 6, 7, 8, 9, 10` intersect; `2, 3, 4` and anything
     outside `1..=10` are refused). Every target carries
     `OBJECT3D_CLASS_TYPE = 5`, which #2213 measured for every M01 collider, so
     the filter removes nothing today and a future population with a refused
     class has one place to say so.
4. **The fill** is `fill_groundblow_probes`, a fixed-update system that casts
   the player record's probe from the tick's **pre-step** `Position`/`Rotation`
   (before the physics step and before the law steps) and stores the answer in
   the `GroundBlowProbes` resource. It is a system of its own because the
   flight driver holds `&mut Rotation` while the sweep reads it; Bevy refuses
   the two queries in one system. `OriginalFlightSet::{GroundBlowFill,
   OriginalFlight}` names the order for callers that need to insert a step
   between them.
   `drive_original_flight` hands the record's input to the law through
   `world_sweep::overlay_groundblow_probe`, the one place that reads
   `GroundBlowProbes`: it sets `OriginalInput::segment_probe` for the player's
   seat (and only that seat), and the driver counts the application in
   `PlaytestState::groundblow_probes`. `PlaytestOriginalFlight::input()` still
   states `None` — it has no world — and says where the driver overlays it.
5. **Nothing consumes the probe yet**, so M01 must fly exactly as before: the
   acceptance test runs two composed M01s in lockstep, one with the driver
   handed the sweep's answer and one with the answer withheld between the fill
   and the driver, and asserts every tick's position, attitude and velocity is
   identical.

## 1. Measured properties of this port's world

* **The mission's world area does not cover the hand-over pose.** `build_headless`
  for M01 spawns 96 `PlaytestSea` colliders (the non-indexed water tiles) and
  513 `WorldSceneryCollider`s; their combined extent is
  x ∈ [-15360, -2048], z ∈ [-14336, -2048], while the hand-over pose is
  (-1426, 150, -1813). A 400 m probe from that pose meets nothing in any
  direction, and even a 4 000 m probe straight down meets nothing: the port has
  no collider there at all. The retail test now asserts the first half of that
  from the colliders' own world bounds (no swept collider lies within the probe's
  400 m) and that the fill cast at the hand-over pose is empty. The flat water
  tiles the mission does carry are flat `y = 0` quads elsewhere: the reviewed
  test dives 60 degrees from 150 m above the widest one and asserts the
  answering collider is that tile (a 173 m chord).
  This is recorded rather than "fixed": it is the collider population the port
  spawns for M01's world selection, the original's world database has no sea
  plane either (sweep finding §7), and whether the original's backdrop dome or
  any other node answers at the hand-over pose was not measured. **#2107 must
  not assume a probe hit near the mission start.**
* **The per-collider winding is up/outward.** Every sea triangle inspected
  points `+Y` (with a handful of `-Y` on the island-side tiles), matching the
  stored-polygon census of `2026-10-29-groundblow-normal-scale.md`, so the
  one-sided gate accepts a dive and rejects a climb from below — the behaviour
  the original's stored winding produces.

## 2. What this query does not reproduce

Each of these is visible to #2107 and is recorded rather than approximated:

| gap | why | effect |
| --- | --- | --- |
| the stored polygon's two-sided bit (`(vertex_info >> 10) & 1`, pushed as the face test's `flag` at `0x55cc73`) | the port's collision view is a triangle soup and drops the stored flag word | a two-sided stored outline hit from its back side is reported here as no contact where the original keeps it. C3's measured water and island polygons are one-sided; no count for the rest is committed |
| a quad's parallelogram area | the same popup: the port has triangles, the original takes the stored polygon's first three corners | on a stored quad the port reports `2 * triangle area` where the original reports the quad's parallelogram: `sqrt(1/2)` on the probe's `sqrt(dot)` for those faces |
| an aircraft's mesh | the port's player aircraft body carries the designed 12 x 2 x 10 m box (an AI aircraft 12 x 4 x 12), not the airframe's mesh nodes | an aircraft hit follows the measured box rule, not the mesh's triangles |
| the vehicle's `+0xcc` taxi-path byte | the plane record's `+0x11c` path branch (`0x47c568`) is not imported, and no component carries it | every port vehicle states `on_path: false`; a parked aircraft the original would count as ground is discarded by the player's gate |
| props, turrets, ground targets | they are not `PlaytestSea`/`WorldSceneryCollider`; the owner's scope for this stage is the scenery and the live aircraft | the original sweeps those nodes too (its polygon census names `gun`, `turret` and `hull` faces), so a dive at a turret finds nothing here |
| the layer filter `0x56c430(node+0x30)` and the query thresholds `0x71e8ac`/`0x71e8b0` | runtime state whose value at ground-blow time is unmeasured (#2213 §6) | the port applies none of them |
| the 32-contact abort (`0x4c9a9d`) | the abort counts the original's *stored polygons*; the port's faces are triangles (the quad gap above), so reproducing it now would abort earlier than the original | a segment crossing 33+ port triangles still reports its nearest contact where the original would report none. Recorded with #2217 |

## 3. Tests

Prefix `accept_groundblow_fill_`,
`crates/cs_app/tests/campaign/accept_groundblow_fill.rs` (registered from
`tests/campaign/mod.rs`, so `cs_app_tests_d`):

* `accept_groundblow_fill_class_type_filter_matches_the_measured_dispatch` — the
  dispatch's truth table, `0..=12`, and that `object3d` (5) intersects.
* `accept_groundblow_fill_sweep_reports_the_unnormalised_triangle_normal` —
  a 200 x 200 m water quad as two triangles: the normal is `(0, 40000, 0)`
  (`2 * triangle area`, not a unit vector), the point is 100 m below the start,
  the distance equals the chord of the point, and an empty population is no
  contact.
* `accept_groundblow_fill_sweep_rejects_a_back_face_and_a_refused_class` — the
  same quad from below is rejected by the one-sided gate and accepted from
  above; classes 2, 3 and 4 never answer.
* `accept_groundblow_fill_population_is_the_marked_colliders` — a bare world
  with a `PlaytestSea` quad and an `Aircraft`-layer box: the sea answers as
  `World` at its own surface, the box as `Vehicle { on_path: false }` at its
  `+Z` face with `|N| = 24` (12 x 2), and the prober never hits its own box.
* `accept_groundblow_fill_sweep_resolves_a_box_to_its_vehicle_and_skips_the_prober`
  — the same box record through `nearest_contact`, with `owner_of`'s refusals
  for debris and for a sea-marked prober.
* `accept_groundblow_fill_overlays_the_players_probe_only` — the driver's
  overlay: the player's input carries the sweep's hit (replacing any stale
  probe, changing no command), a tick with no contact stays `None`, another
  record's hit is not used, and a non-player's input is not overlaid.
* `accept_groundblow_fill_degenerate_segment_answers_nothing`.
* `accept_groundblow_fill_m01_fills_the_probe_from_the_production_world`
  (ignored, needs `CS_GAME_DIR`) — M01 composed headless: on every tick the
  stored fill equals the query's own answer for the same pre-step pose
  (compared in-tick, after the fill and before the driver), the elevation is
  400 m, and over M01's mapped flat sea the probe answers with the
  unnormalised normal and the chord distance. The aircraft is placed **from the
  tile's own world pose** (its `Position`/`Rotation` around the shape's local
  box — the local box alone is not a world place), 150 m above the widest flat
  tile and backed off by the 60-degree dive's own 87 m horizontal reach, and the
  test asserts the answering collider is that `PlaytestSea` tile.
* `accept_groundblow_fill_m01_trajectory_is_identical_with_the_fill_on`
  (ignored, needs `CS_GAME_DIR`) — two composed M01s in lockstep, both placed
  over the same flat sea tile on the same 60-degree dive, one with the fill's
  answer withheld from the driver by a test system between
  `OriginalFlightSet::GroundBlowFill` and `OriginalFlightSet::OriginalFlight`:
  every tick's `Position`, `Rotation` and `LinearVelocity` is identical, over a
  run where the sweep answers on at least 100 of 120 ticks, and both runs
  count the driver's overlay on every stepped tick
  (`PlaytestState::groundblow_probes`).

The two retail tests take a mutex: composing M01 stages the launch-sound
container into one private cache path, and two compositions in parallel were
measured to fail `ZBD/soundsh.zbd does not match the installation's digest`.

## 4. Commands run

On the task branch, after the change (exit codes as observed):

```sh
cargo fmt --all -- --check                                            # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0
cargo test --locked -p cs_app                                         # exit 0 (277 + 1290 + 336 + 813 + 10 + 1 passed, 0 failed)
cargo test --locked -p cs_app --test cs_app_tests_d -- accept_groundblow_fill_   # exit 0, 6 passed, 2 ignored
cargo test --locked -p cs_app --test cs_app_tests_d -- accept_groundblow_fill_ --include-ignored
#   exit 0: 8 discovered, 8 passed (the two retail members compose M01; ~8 min)
```

The owner-command smoke (`docs/PLAYTEST.md`) on a release build of this
branch's head, with M01 composed from `$CS_GAME_DIR`:

```sh
cargo build --release -p cs_app --bin cs                             # exit 0
./target/release/cs --profile-dir private/evidence/2214-smoke/profile \
  --cs-path "$CS_GAME_DIR" --mission M01 --smoke-seconds 45 \
  --capture-dir private/evidence/2214-smoke/run2                     # exit 0
#   "mission smoke: M01 — 81 checks, 0 failed, framebuffers 79 in
#    private/evidence/2214-smoke/run2"
#   "mission smoke: 4128 frames, 8082 ticks, 81 checks, failures: none"
```

The retained frames are the smoke's own scene shots (for example
`frame_02342_scene_39s.png`: the zeppelin, its boats, the sea and the sky).
`private/` is Git-ignored, so they are owner-visible evidence, not committed
bytes. **The same command without `--profile-dir` exits 1 after the 81 checks
pass**, on `cs: accessibility settings: the change is live but could not be
saved: No such file or directory` — the default user-data directory does not
exist on this host; filed as #2219.

`cargo test --workspace --locked` was not run locally: CI runs the full suite
on the pushed commit (AGENTS.md). The only crate this branch changes is
`cs_app`.

