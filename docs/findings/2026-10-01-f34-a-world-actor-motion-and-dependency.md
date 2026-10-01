# F34-A: world-actor motion and dependency contract

Task #142. Module `cs_sim::world_actors`; test `crates/cs_sim/tests/accept_f34_a_world_actors.rs`.
All behavior is **designed**, synthetic and not original-verified. Awards at most *checked*.

## Designed semantics

- **One motion function.** `Trajectory::sample(tick)` yields position, orientation, linear and
  angular velocity from the same piecewise-linear/slerp path, so velocity is the exact derivative of
  position. It takes only a `Tick`: no visibility or culling input exists, so an offscreen actor moves
  identically (AC04's motion half). At a key tick the outgoing segment's velocity applies; before the
  first key and from the last key the actor is held with zero velocity (no extrapolation).
- **One anchor pose.** `anchor_sample` is the only computation of an anchor's world pose/velocity
  (`v + omega x r`). Pickup eligibility (`pickup_eligible`) calls it, and `cs_app` presentation must
  call the same function with the actor's sampled `Pose` (AC01). Eligibility uses relative velocity.
- **Explicit support graph.** `SupportGraph` edges are actor-id based, reject cycles/unknowns, and
  `destroy` flips a whole dependent set to `Presence::Destroyed` (geometry and collider as one state).
- **Released payload.** `release_payload` starts at the anchor with anchor velocity plus ejection,
  a fresh `ActorId`, the given faction and the objective identity (AC03's data half).

## Not done here (deliberately)

- The gate-blocks-ground-route rule (non-negotiable 3), gate-before/after-convoy ordering (AC02)
  and the actual rail/road/water actors: F34-B (#148).
- Wiring into `cs_app` presentation (renderer reading `anchor_sample`), the pickup/cargo state
  machine and the objective-failure link (AC03 wiring, AC04 failure): F34-C/F34-D.
- No `cs_content::world_actors` schema or `cs_app::world_actors` adapter exist yet: no original
  actor-kind record has been measured, so a schema would be guessed. The envelope numbers, quaternion
  convention (`[x, y, z, w]`, coordinate axes as in `cs_types::space`) and orientation interpolation
  are designed values/choices, not measured original rules.

## Unknowns

- Original train/boat path representation (keyframed, spline or rail-segment), its tick/time unit
  and whether orientation is authored or derived from the tangent: unknown, needs retail format
  evidence (F34-D). Affects every moving non-aircraft actor.
- Original pickup/docking tolerances: unknown; `PickupEnvelope` is caller-supplied.
