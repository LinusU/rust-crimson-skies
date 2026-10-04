# F35-C: launch, capture, cargo and staged destruction wiring

Task #152. Session runtime `cs_sim::capital::runtime`; the state its tick
pass drives in the new `cs_sim::capital::wiring`; capture control in
`cs_sim::capital::capture`; the declared launch wiring lowers through
`cs_app::capital`. Tests:
`crates/cs_sim/tests/accept_f35_c_capital_wiring.rs`,
`crates/cs_app/tests/accept_f35_c_capital_boundary.rs`.

All behavior is **designed**, synthetic and not original-verified. Awards at most
*checked*. No original capital-ship data exists on this machine and none was
used; every value in the fixture carries designed provenance.

## Designed semantics

- **One pass, one resolution per launch.** `CapitalShipSet::step` moves the
  ships, then runs the F35-C pass: it resolves every waiting launch through
  the F35-A `LaunchLedger` and turns each resolution into an event. A release
  samples the pose the *same* tick committed (the pass runs after the motion),
  allocates a fresh session actor id and emits `LaunchReleased { carrier, id,
  aircraft, at }` — the authoritative spawn order, carrying the socket
  transform from `anchor_sample`, the carrier's motion plus the mission's
  ejection, and dynamic authority granted once. The whole id block is
  reserved before anything is committed, so an exhausted id space refuses the
  tick instead of releasing half of it. Ids are per-carrier `(bay, sequence)`
  pairs, never reused, so the session-wide identity is `(carrier, id)`.
  `launch_status` answers how an id resolved — `Pending`, `Released` (with
  the one aircraft) or `Cancelled` (with its reason) — and a resolved id is
  settled for good.
- **Cancellation is a teardown, not a deferral (AC03).** A destroyed launch
  bay, a dying carrier and a despawned carrier each cancel their waiting
  launches with a distinct `LaunchCancelReason` and an event. Nothing spawns
  for a cancelled id at any later tick, `release_launch` refuses it with
  `Cancelled`, and the destroyed bay accepts no new schedule. The minimum
  scenario destroys the bay while the launch waits, then advances through the
  launch's own ready tick and the rest of the session: no aircraft appears,
  while a control ship with the identical schedule and an intact bay releases
  exactly once.
- **Capacity bounds what waits, not what left.** `LaunchBayRig.capacity` — the
  declared `capacity` F35-A parked — limits the launches *waiting* aboard a
  bay; a released aircraft has left and freed its slot. An unresolved capacity
  refuses every schedule by claim (`CapacityUnknown`): an unbounded hangar is
  never assumed. An unresolved socket refuses the same way (`SocketUnknown`),
  so no aircraft is ever spawned at an invented transform. The declared socket
  and capacity lower verbatim through `cs_app::capital`, provenance included.
- **Retry is a real door.** `release_launch` releases exactly one id through
  the ledger's `release_one`, under the same bay-open and ready-tick gate the
  tick pass uses: a refused attempt (`NotOpen`, `NotReady`) leaves the launch
  waiting so the caller may try again later, and a released or cancelled id
  refuses. A caller that releases inside an open window beats the next pass by
  a tick and the pass then finds nothing left to do.
- **Capture commits in one step.** `begin_capture` records the ship's current
  owner and hands back a `CaptureTicket { ship, claimant, session, attempt }`.
  The latch (`Latching`) moves the dedicated control owner to the claimant's
  owner while the guns stay with the previous owner; the completing stage
  moves `Ownership` (through the one `CapitalShip::adopt_ownership`), the
  control owner and the guns owner in the same step, so no consumer can read a
  new owner beside the previous owner's guns. The docking gate is re-read from
  the ship's own anchors after every hit and disable, so a destroyed anchor
  closes boarding immediately and a dead ship never boards.
  `ShipControl::is_coherent_for` states the invariant the commit satisfies and
  an in-flight capture deliberately does not.
- **Stale callbacks cannot commit.** An abort frees the ship and the retry
  gets a new attempt ordinal, so the aborted ticket refuses forever
  (`StaleCaptureTicket`) and a ticket from another session refuses
  (`ForeignSession`) — the interaction-transaction discipline in
  `STATE-TRANSACTIONS`. Destruction aborts an in-flight attempt without
  transferring ownership, and only one attempt may hold a ship at a time
  (`InProgress`).
- **Destruction and despawn are separate.** A lethal subsystem freezes the
  wreck pose (F35-B), starts `DestructionState::Sinking` and tears down the work
  that cannot survive it: pending launches cancel, an in-flight capture aborts,
  docking closes. The wreck stays in the world and still takes part damage
  while it is there — destruction is monotonic, the F29 convention. Despawn is
  the *next* transition: `DespawnPolicy::after_ticks(n)` closes the record n
  ticks later and reports `Despawned`; `DespawnPolicy::HOLD` (the default, and
  the only policy F35-B behavior depends on) keeps the wreck until the mission
  calls `despawn_ship`. A closed record refuses every new order and every live
  state query with `ShipDespawned`, naming the tick; `pose` keeps answering,
  because the pose a ship left the world on is what a diagnostic needs. An
  intact ship never despawns through this door (`NotDestroyed`): removing a
  live ship is a mission removal, not this module's transition.
- **Cargo is bounded and honest.** `CargoLedger` enforces the ship's declared
  capacity exactly (a refused load or unload changes nothing), refuses a
  negative or non-finite amount, and refuses every load by claim when the
  capacity is unresolved. A dying ship may still be *emptied* — unloading
  reads state that exists — but never filled.
- **Unknown stays unknown, ticks stay canonical.** The launch pass resolves
  against the committed tick's bay phase (the F35-B weakpoint window), a hit
  stamped for another tick still refuses, and no value crosses the boundary as
  a default: an unknown socket, capacity or cargo capacity surfaces its claim
  id and reason to the caller.

## Test sensitivity

Each behavior was mutated to confirm the tests fail without it: dropping the
destroyed-bay cancellation branch (AC03 fails in both the sim and the
boundary test), short-circuiting the capture commit, removing the cargo capacity
bound, removing the launch capacity bound, removing the closed-record gate,
removing the docking refresh, and stopping the boundary from lowering the rig
(three boundary tests fail).

## Not done here (deliberately)

- Any relation table. `ShipControl` guarantees guns, targeting, the AI and the
  docking gate read *one* owner; whether two owners are hostile is the
  mission's and F33 `AlliesRoster`'s data, so nothing here decides hostility.
- Projectiles, and the world-actor path that *spawns* the released aircraft and
  hands it to the mission: `LaunchReleased` is the spawn order a consumer
  acts on, the runtime does not create the actor.
- A sinking or crashing wreck *trajectory*, and any claim that a wreck is
  dangerous to others: the staged phase models how long the wreck stays and
  what still happens to it, not how it moves. That needs original evidence.
- Cargo *units*: the declared capacity carries no unit, so the ledger works in
  the declared value's own scale and names nothing.
- Original mission validation: F35-D (needs `retail`).
- No Avian body, no renderer, no file access: `cs_sim` depends only on
  `cs_types` and `cs_script`.

## Unknowns

- Whether the original couples launch cadence to a bay's exposure cycle, what
  a launch bay's capacity counts (waiting versus airborne aircraft), whether
  one declared bay had several release sockets, the ejection a launched
  aircraft received, the capture stage order and its gun/targeting/docking
  switch, the cargo unit and whether cargo could be moved at all, and how long
  a destroyed capital ship stayed in the world are all **unrecovered**. Every
  number and rule here (the capacity of four waiting aircraft, the
  `[0, -5, 0]` socket, the `[0, 0, 2]` ejection, the hold-by-default wreck) is
  authored design, not a measured rule.
- The `HOLD` default exists because the alternative — inventing a destruction
  duration — would be a guess in a place the original almost certainly defines.
  Affected content: every capital-ship definition and every mission that
  targets a subsystem, bay, cargo hold or wreck.

## Files

`crates/cs_sim/src/capital/{mod,ship,parts,bay,capture,launch,synthetic,runtime}.rs`;
`wiring.rs` (new); `crates/cs_app/src/capital.rs` (launch rig lowered); the two
test files above; the `cs_sim` and `cs_app` lib doc bullets. No existing
acceptance test needed a change: `Bay::new` keeps its signature and the rig is
attached by a builder, so the F35-A and F35-B fixture literals still compile
and still pass unchanged.
