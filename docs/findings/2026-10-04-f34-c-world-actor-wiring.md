# F34-C: world-actor pickups, gates, cargo and scripted transitions

Task #149. Modules `cs_content::world_actors`, `cs_app::world_actors` and the
carriage/scripted-gate additions to `cs_sim::world_actors::runtime`; tests
`crates/cs_app/tests/accept_f34_c_world_actors.rs` plus the schema tests in
`cs_content::world_actors::tests`. All behavior is **designed**, synthetic and
not original-verified. Awards at most *checked*.

## What was missing (the observable failure)

F34-B left the runtime as contracts plus unit-level operations: no declared
schema, no lowering boundary, no session producer→consumer path, no scripted
gate transitions, no cargo, and `cargo test --workspace -- accept_f34_c_`
discovered zero tests.

## Designed semantics

- **Declared schema** (`cs_content::world_actors`): a
  `DeclaredWorldActorProgram` carries the tick rate, actors (kind, faction,
  optional objective, `DeclaredMotion`, named sockets), support edges,
  `DeclaredPickup`s (envelope, taker, `Latch`/`Attach`/`Collect` completion)
  and the scripted `DeclaredGateTransition` schedule. Every load-bearing
  value is a `Resolved`, so an unmeasured speed or envelope stays an
  explicit unknown with its claim. `try_new` guarantees unique actor/socket/
  pickup ids, finite known values and the two self-evident pickup rules
  (no self-take, no `Attach` on an external taker); every cross-reference
  resolves at the lowering boundary.
- **Lowering** (`cs_app::world_actors::lower_world_actors`): field-wise
  conversion to `WorldActorSpec`s, `AnchorSocket`s, `RoutePlan`s and
  `Trajectory`s. `Resolved::Unknown` refuses by name
  (`UnknownValue { field, claim_id, reason }`); dangling carriers, sockets,
  pickup targets and declared carriage cycles refuse by name; runtime
  refusals on routes/paths propagate wrapped (`Route`, `Trajectory`).
- **`WorldActorSession`** (`cs_app`): owns the lowered program, the live
  `WorldActorSet`, the pending gate schedule, the latched-pickup set and the
  collected-actor ledger for one `SessionGeneration`. `step` applies the
  producer's `WorldActorCommand`s at the current tick, fires scheduled gate
  transitions before each stepped tick, judges world-actor-taker pickups on
  every stepped tick (a carrier crossing an envelope mid-advance latches on
  the tick it crossed) and judges external-taker probes at the target tick
  (the probe pose is only defined there). It answers the ordered
  `WorldActorSessionEvent` stream plus a typed `WorldActorRefusal` list —
  a refused order is reported, never silently dropped and never aborts the
  tick. Only `NonMonotonicTick`/`TickOverflow` propagate as `Err`.
- **Carriage** (`runtime.rs`): `MotionState::Carried` resolves the cargo's
  pose from the carrier's canonical pose through `anchor_sample` on every
  read, chained for cargo-on-cargo. `attach` refuses wrecks, collected
  actors, socket-owner mismatches and carriage cycles; `detach` converts
  the actor to `Free` on the socket's velocity plus the authored ejection,
  keeping faction and objective (the released-boat scenario, AC03);
  `collect` freezes the actor as `Collected(pose)` — ids are never reused
  and a collected gate can never hold again. `destroy` resolves every wreck
  pose before mutating records, so cargo freezes at the carrier's
  pre-destruction pose in one cascade.
- **Scripted gate transitions** (`runtime.rs`): `set_gate_open` adds/removes
  the gate from `open_passages`. The step's closed-gate snapshot now
  counts only closed stop lines **at or ahead of** a follower's progress —
  a scripted close behind a legitimately-crossed follower neither pulls it
  back nor pins it where it crossed (the naive `min` clamp would have
  frozen it forever). A destroyed gate's flag is dead state: presence is
  monotonic, the passage stays open permanently.
- **Teardown/retry**: `retry` reports a `WorldActorTeardown` (registered
  actors, collected cargo, latched pickups, unfired transitions), rebuilds
  a fresh set before releasing the old one, clears every per-generation
  ledger and refuses `SameGeneration` — no destroyed actor, open gate,
  latched pickup or fired transition survives (STATE-TRANSACTIONS).
- **`WorldActorBinding`**: actor + catalog subject + `SceneGeneration`,
  stamped like `SceneNodeBinding` so a stale binding is identified by
  generation mismatch.

## Test sensitivity

Removing pieces fails the tests: no `open_passages` →
`…_the_declared_schedule_drives_gate_followers` and
`…_commanded_transitions_and_a_close_never_pulls_back`; no per-tick judging
→ `…_the_winch_pickup_attaches_the_crate_mid_advance`; no `detach`/`collect`/
`attach`/`release` → the cargo tests; no refusal plumbing →
`…_refusals_are_named_and_errors_propagate` and
`…_a_refused_completion_is_reported_every_tick`; no retry rebuild →
`…_retry_restores_the_authored_state_and_names_the_teardown`.

## Not done here (deliberately)

- No mission-host/ECS schedule wires `WorldActorSession::step` into Bevy
  yet; the session is the boundary a host drives (F34-D and mission-host
  tasks own that). `WorldActorBinding` is the record the host will stamp.
- The original pickup tolerances, actor-kind names, socket tables and gate
  schedules remain unmeasured; everything is caller/declared-supplied.
- An `Attach` pickup whose taker is destroyed between declaration and
  latch simply never satisfies the envelope (the taker presence check) —
  no partial-carriage failure mode is invented.

## Unknowns (unchanged from F34-A/B)

- Original convoy/gate encoding, path representation and stop semantics.
- Original pickup/docking tolerances (`PickupEnvelope` stays declared).
- Whether the original ever re-closes a passage — this design supports it;
  nothing asserts the original did.
- Whether the original models cargo as sockets on carriers at all.
