# F34-B: rail/road/water/kinematic world-actor runtime

Task #148. Modules `cs_sim::world_actors::route` and `cs_sim::world_actors::runtime`;
test `crates/cs_sim/tests/accept_f34_b_world_actors.rs`. All behavior is **designed**,
synthetic and not original-verified. Awards at most *checked*.

## What was missing (the observable failure)

Before this stage `cs_sim::world_actors` was contracts only: no session registry, no
route follower, nothing that could move a convoy or stop it at a gate
(non-negotiable 3), and `cargo test --workspace -- accept_f34_b_` discovered zero
tests.

## Designed semantics

- **`RoutePlan`** (`route.rs`): a validated polyline (≥2 non-coincident waypoints,
  finite positions, positive cruise speed) plus declared `RouteGate` passages —
  `(gate: ActorId, at_m, stop_before_m)` sorted ascending, each inside the route's
  arc length. Position and direction read by arc-length lookup; a waypoint tick
  uses the outgoing segment, same rule as `Trajectory`.
- **`WorldActorSet`** (`runtime.rs`): one session's registry — `BTreeMap<ActorId,
  WorldActor>` plus the F34-A `SupportGraph` — stepping `ticks_per_second` fixed
  ticks. Actor kinds are a catalog enum (`Rail`, `Road`, `Water`, `Kinematic`);
  kind is identity, not motion capability.
- **Four motion forms.** `Held` (machinery at a fixed pose), `Trajectory`
  (authored tick-indexed paths — timetables, elevators), `Route` (the
  gate-stoppable follower), `Free` (a released payload drifting on inherited
  velocity). Route followers orient by yaw of the segment direction
  (`FLIGHT-PHYSICS` heading convention, shared with `ai::navigation`);
  near-vertical segments keep the last yaw.
- **Gate rule** (non-negotiable 3, AC02). Each step clamps a follower's progress
  at `min` over intact gates of `at_m − stop_before_m`. Presence is monotonic, so
  destroying the gate opens the passage permanently and can never pull back a
  follower that legitimately crossed. Both AC02 orders fall out of the one rule:
  destroy before arrival → never held; destroy after arrival → `HeldAtGate`,
  pinned with zero velocity, then `ResumedFromGate` and `RouteCompleted`.
- **Velocity is the real displacement** (non-negotiable 1). A follower's reported
  velocity is `(pos_new − pos_old)/dt` for the tick just stepped, so the partial
  move that lands on a stop line reports the reduced speed, a held follower
  reports zero, and angular velocity is the actual orientation delta over the
  same tick.
- **Destruction** (non-negotiable 2). `destroy` is `SupportGraph::destroy`:
  dependents cascade by declared edge, `Presence` is the single
  geometry+collision state, and each destroyed actor freezes as a zero-velocity
  wreck at its current pose. Session ids are never reused: registering over a
  destroyed record is `DuplicateActor`.
- **Release** (non-negotiable 4). `release` wraps `release_payload`: the new
  actor spawns as `Free` at the anchor with anchor velocity + authored ejection,
  the spec's faction and its objective identity.
- **Input validation.** `register` refuses a `Trajectory` whose tick rate
  differs from the set's — its sampled velocity is derived in the
  trajectory's own timebase and would misreport (non-negotiable 1). `release`
  refuses a non-finite ejection or anchor before the registry changes. A
  follower spawned exactly on a stop line shared by several intact gates is
  held by the first gate along the route, the same one `step` reports.
- **Offscreen motion** (non-negotiable 5). `step` has no visibility, residency or
  presentation parameter; a never-sampled actor and a per-tick-sampled one end
  identical. `pickup_eligible_pose` (anchor.rs) judges eligibility on the same
  `Pose` the renderer reads — moving pickup uses relative velocity.

## Events

`advance_to`/`step` return edge-only events in actor-id order: `HeldAtGate`,
`ResumedFromGate`, `RouteCompleted`. Destruction and release are caller-invoked
and return their facts directly; they emit no events.

## Test sensitivity

Removing the gate clamp (`limit` forced to `+∞`) fails 3 tests:
`…_convoy_held_at_closed_gate_resumes_when_gate_destroyed`,
`…_partial_move_into_a_gate_reports_the_reduced_speed`,
`…_catalog_kinds_and_a_water_follower_at_its_lock_gate`.

## Not done here (deliberately)

- `cs_content::world_actors` declared schema and `cs_app::world_actors` wiring:
  no original actor-kind record is measured, and the mission host wiring
  (pickups, scripted gate transitions, cargo, objective failure links) is F34-C.
- Gates open only by destruction here; scripted open/close transitions are F34-C.
- `Free` drift is constant-velocity (no water drag/deceleration — unmeasured).
- Route followers orient by yaw only; pitch/roll from terrain is unmeasured.
- Multiple gates sharing one stop line report the first by `at_m`.

## Unknowns (unchanged from F34-A, now blocking F34-D)

- Original convoy/gate encoding, path representation and stop semantics.
- Original pickup/docking tolerances (`PickupEnvelope` stays caller-supplied).
- Whether the original ever re-closes a passage (re-closing is impossible in
  this design because destruction is monotonic).
