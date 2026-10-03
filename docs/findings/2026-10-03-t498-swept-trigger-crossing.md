# T498: a world trigger volume reports a swept crossing, once, as a read

Date: 2026-10-03. Task: #498 "Give a world trigger volume a swept crossing
report" (`F18-trigger-swept-crossing`). Producer:
`crates/cs_app/src/world/crossings.rs` (this task). Stream:
`crates/cs_app/src/objectives.rs` `TriggerCrossings` (task #415's record,
stream and once-per-pair ledger). Overlay hand-off:
`crates/cs_app/src/world/overlays.rs` `OverlayTriggerRequests`. Contracts:
`docs/contracts/FLIGHT-PHYSICS.md` ("Collision and ballistic tests": *"For
interaction triggers use a swept center/shape appropriate to the original
rule"*). Sibling task: **#415** decided this same rule at the spawn tick; its
finding (`2026-10-02-t415-spawn-tick-trigger-crossing.md`) is the one this
task's path implements the second half of.

Capabilities used: ordinary build/test only. No `CS_GAME_DIR` read, no evidence
report required. This stage can award at most **checked**.

## The defect this closes, measured

Task #401 left the world trigger volume reported by **sampled overlap**:
`overlays::queue_overlay_triggers` reads Avian's `CollisionStart` stream, which
the narrow phase emits only when a discrete sample lands inside the volume. On
the pinned pair (`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz
fixed, gravity zero) the world composition's own fixtures show two holes in
that stream, both of them now measured end-to-end by the acceptance tests:

| flight | geometry | what the sampled stream reports | what the sweep reports |
| --- | --- | --- | --- |
| 400 m/s probe, 3.333 m/tick | depot `trigger.depot`, cuboid 1 m thick | nothing — no sample lands inside (contact log names every other object and not the volume) | one `Entry`, `distance_m` ≈ 0.583 m into the segment |
| 400 m/s probe | harbor `trigger.sensor`, `FromMesh` box | nothing — the body lands **deep inside** the volume where no triangle is within `max_contact_distance`, so parry produces no manifold even for a body that ends the tick inside | one `Entry`, `distance_m` ≈ 2.42 m into the segment |

The second row is #401's mesh-specific gap, now carried through to a decision:
`tests/world/trigger.rs` pins that `WorldContacts` *still* reports nothing for
that flight — the sampled stream's blindness is unchanged — while
`tests/world/crossing.rs` pins the crossing in `TriggerCrossings` anyway. The
two streams say different things because they measure different things; that
split is the point of the change.

## The rule, and which half of it this path adds

#415 decided the crossing rule once so the two entry points cannot drift;
quoted here with the half this task implements marked:

1. **Decide from the body's own motion over the tick** — the segment from the
   `Position` the body ended the previous tick at to the one it ended this
   tick at, swept against the volume's own collider. Never a sampled overlap.
   *(New here: the spawn tick had one preflight cast; ordinary flight must
   track the per-tick segment itself.)*
2. **Rate-independent by construction** — the decision is the segment the body
   covered, not how often a clock sampled it.
3. **Exactly once per `(actor, volume)` pair**, enforced by the one place a
   crossing enters the stream, `TriggerCrossings::record`. A body that dwells,
   turns away inside, or crosses and returns cannot fire twice; refused
   re-decisions are counted in `duplicates`.
4. **Delivery is a read.** `sweep_volume_crossings` takes `Position`,
   `Rotation`, `Collider`, `RigidBody`, `CollisionLayers` and
   `SpatialQuery` — all shared — plus its own tracking resource, the stream
   and the hand-off. No `Query<&mut _>`, no `Commands`. A trigger cannot stop,
   delay or nudge the body it reports; the tests assert every tick that spans
   a volume is a full free tick and the velocity is untouched.
5. **The state machine is the `SweptTrigger` one.** `cs_sim`'s
   `SweptTrigger::advance(inside, ends_inside)` table is applied to the swept
   path's own bits:

   | was inside | ends inside | segment touched | decision |
   | --- | --- | --- | --- |
   | false | true | — | **Entry** (delivered; `distance_m` is the cast hit, or the whole segment when no boundary was met — a teleport or a volume that moved onto the body) |
   | false | false | yes | **Entry** (pass-through: entry and exit inside one tick; the entry is the record) |
   | true | false | — | exit — tracked in `SweptBodyTracks::exits`, **deliberately not a stream record** |
   | true | true | — | nothing |

   The inside bit lives per **body** (`SweptBodyTracks`), not per pair,
   because the pairs are not declared anywhere: the set of volumes a body ends
   each tick overlapping *is* every `(body, volume)` inside-bit at once.
   Exits are bookkeeping, never records: the ledger is once-per-pair and a
   cross-and-return is the same pair, so a second record for "leaving" would
   only consume the entry the pair was owed. `CrossingKind` admits an `Exit`
   variant; this producer emits `Entry` only, and a consumer that wants exits
   is a F39 decision, not an F18-C one.

## How the decision is made

Two parry queries through `avian3d::prelude::SpatialQuery`, run in
`FixedPostUpdate` after `PhysicsSystems::StepSimulation` (both segment
endpoints are then integrated poses) and before `apply_overlay_requests` (a
crossing decided this tick applies its overlay this tick — the same latency
the `CollisionStart` producer gives the discrete path):

* `shape_hits_callback` casts the body's **own collider** along the tick's
  segment (`ShapeCastConfig::from_max_distance(travel)`), keeping every hit on
  a `WorldColliderInstance` whose role is `Sensor` — on either layout (hand
  cuboid or `TrimeshFromMesh`), at any depth, because the cast asks the
  volume's own geometry and not a contact manifold. `hit.distance` is the
  `distance_m` the record carries.
* `shape_intersections_callback` tests the collider at the segment's end pose
  for the inside bit.

Both queries are masked to `CollisionLayer::StaticWorld` ANDed with the body's
own `CollisionLayers.filters`, so a body meets exactly the world colliders the
designed collision matrix already lets it meet, and `with_excluded_entities`
removes the body itself. The `Sensor` role check — not the layer — is what
keeps solid world geometry out of the stream, and it is the same check
`queue_overlay_triggers` makes, so the two producers cannot disagree about
what a trigger volume is.

A body the pass sees for the **first** time is registered at its current pose
and overlap set and swept nothing — there is no previous sample to form a
segment with, "already inside" is the honest inside bit, and the spawn tick's
entry is #415's producer. Tracks for bodies no longer observed are pruned.

Delivered entries request the volume's authored object's overlay through the
same `OverlayTriggerRequests` the `CollisionStart` producer writes, via the
collider entity's `WorldObjectBinding` — one hand-off, one consumer, one
trace. Whether the load declares an overlay is the consumer's question; the
harbor test authors one on the mesh volume so the claim is exercised on that
layout too.

## Evidence

`cargo test -p cs_app --test world --locked accept_f18_c_` — 22 tests pass, of
which the four this task adds are:

* `accept_f18_c_a_swept_body_through_a_thin_cuboid_trigger_reports_once_and_opens_the_door` —
  the first table row end-to-end: contact stream silent on the trigger, one
  `OrdinaryFlightSweep` entry naming the volume's collider entity with
  `distance_m` inside the segment, the door's *collided* half displaced by
  the authored offset, every volume-spanning tick a full free one, velocity
  untouched, `SweptBodyTracks::entries == 1`.
* `accept_f18_c_a_swept_body_through_a_mesh_trigger_reports_once_and_opens_its_overlay` —
  the second row end-to-end on the `FromMesh` volume, including the return
  ticks where re-decisions are refused by the ledger; the authored overlay on
  that trigger displaces the hangar shell once.
* `accept_f18_c_a_body_that_turns_away_inside_a_trigger_reports_only_the_entry_it_earned` —
  both ways "turn away" can go wrong: a body first observed inside a volume
  reports nothing even as it leaves, and a body that entered, dwelled and
  reversed out holds exactly its entry (with `exits >= 1` proving the inside
  bit watched it leave).
* `accept_f18_c_a_body_that_crosses_and_returns_is_still_the_same_pair` —
  two real crossings, one stream record, `duplicates >= 1` (the return
  crossing was decided and refused rather than never attempted), one overlay
  application.

## Boundaries of the claim

* **First-observed tick.** A body the pass first meets already inside a
  volume registers "inside" and reports nothing; a body teleported between
  samples reads as one long sweep. Both are recorded, not invented away.
* **Kinematic bodies are not swept** — `RigidBody::Kinematic` moves by
  prescription; a one-word extension (`is_dynamic() → !is_static()` or an
  explicit list) if gameplay ever needs it.
* **Rotation is sampled at the end pose** — a body that pivots hard inside a
  tick sweeps its current orientation along the whole segment. The measured
  flights go straight; a tumbling body's swept *orientation* is a real but
  unmeasured difference.
* **The trimesh inside-bit is shallower than the cuboid's.** Parry's
  `intersection_test` reports a body strictly inside a mesh volume only while
  a face is penetrated (#401's deep-inside gap applies to the *bit* too). The
  delivered stream is unaffected — a touched pass-through is still an entry
  and the ledger contains any flicker — but `SweptBodyTracks::inside` for a
  mesh volume means "overlapping a face", not "contained". A body that is
  dropped deep inside a mesh volume and *stays* there is therefore not in its
  own inside-set; it exits nothing because nothing thought it entered.
* **Exit records are a F39 decision.** This stream holds entries; whether a
  mission language wants an `Exit` kind delivered is content semantics, not a
  geometry fact.
* **Nothing about the original game.** Which volumes the 2000 PC original
  used as triggers, how its zones were reported, and its stored-vertex unit
  are all unmeasured (`world::triggers` / #427 is the survey). Everything
  measured here is the synthetic depot and harbor fixtures on the pinned
  engine pair, and the fixtures claim nothing retail.
