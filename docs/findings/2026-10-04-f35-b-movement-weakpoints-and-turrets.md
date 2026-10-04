# F35-B: capital-ship movement, weakpoints and turrets

Task #151. Session runtime `cs_sim::capital::runtime`; ship-level behavior in
`cs_sim::capital::ship`; the boundary now lowers the turret boresight and the
section integrity pools. Tests:
`crates/cs_sim/tests/accept_f35_b_capital_runtime.rs`,
`crates/cs_content/tests/accept_f35_b_capital_schema.rs`,
`crates/cs_app/tests/accept_f35_b_capital_boundary.rs`.

All behavior is **designed**, synthetic and not original-verified. Awards at most
*checked*. No original capital-ship data exists on this machine and none was
used; every value in the fixture carries designed provenance.

## Designed semantics

- **Movement is a driven authored course.** A `CapitalShip` keeps its F31-style
  authored `Trajectory`; `CapitalShipSet` advances each ship's *drive* position
  (in trajectory ticks) by `propulsion_fraction` per committed tick — the intact
  engines' share of the declared total. A ship at full power keeps its authored
  schedule; a ship that loses an engine measurably falls behind it (half the
  drive rate, half the measured velocity); a ship with no intact engines stops
  without its hull being destroyed (non-negotiable 1). A ship with no engine
  subsystem has no propulsion to lose and keeps an unpowered schedule. The drive
  clamps at the course's final key and `CourseCompleted` reports the arrival
  once; from then on the ship holds the end pose at zero velocity.
- **A wreck is a pose, not a removal.** A lethal subsystem (the synthetic keel
  or gas cell) destroys the actor; the set captures the pose it died on with
  velocities zeroed and `pose()` serves the wreck from then on. Parts aboard a
  wreck still take hits — destruction is monotonic, per the F29 wreck
  convention. A course-less ship is moored: it registers on a required start
  pose and never moves, so its propulsion is never resolved.
- **Weakpoint hits resolve against the committed tick.** `CapitalShip::apply_hit`
  is the resolver a projectile path calls through `CapitalShipSet::apply_hit`,
  whose `CapitalHit::at` must equal the set's current tick so a stale or future
  hit can never resolve against the wrong phase. A weapon or launch bay is
  hittable only while its authored `ExposureWindow` reports `BayState::Exposed`;
  `NotExposed` carries the observed `Concealed`/`Opening`/`Closing` state and
  the part stays intact. The minimum scenario hits the synthetic weapon bay
  concealed (tick 20), opening (45), exposed (60 — destroyed, `WeaponAccess`
  applied), then closing and next-cycle concealed on a fresh ship (115, 130),
  and confirms a destroyed bay is never a weakpoint again (`AlreadyDisabled` at
  170, `BayState::Destroyed` thereafter).
- **Section pools absorb, then deplete.** A gas cell or structural section with
  a declared `IntegrityPool` absorbs damage into `remaining_integrity` and
  reports `Damaged` until the pool reaches zero, then destroys the part through
  the one `SubsystemGraph::disable` transition — the lethal cells take the ship
  with them. A section subsystem with *no* declared pool is destroyed by one
  landed hit like any other part. An `Unknown` integrity blocks the hit by
  claim (`HitOutcome::Blocked`): nothing is absorbed, nothing is guessed.
- **Turrets bear inside a traverse cone.** `TurretMount.boresight` is the
  mount's body-frame rest direction, normalized at construction; `aim_turret`
  commands a direction, returns `TurretAim` verbatim inside the `traverse_deg`
  cone and clamps to the rim on the great circle toward the command otherwise
  (`within_arc: false`, `off_boresight_deg` reported). An antiparallel command
  degenerates the great circle and deterministically bears on the rim nearest
  the axis the boresight least aligns with — a documented designed edge case.
  `may_fire` returns the current aim when the mount and ship are live and the
  weapon binding is known; an `Unknown` weapon refuses by claim, a destroyed
  turret refuses to aim or fire, and a destroyed ship's turrets stay silent.
- **Unknown stays unknown, ticks stay canonical.** An unresolved traverse
  refuses aiming by claim, an unresolved engine thrust refuses the set's whole
  step before any ship moves (`Propulsion`), a negative/NaN damage refuses at
  `CapitalHit::try_new` and at the ship, and a hit on a foreign tick refuses as
  `ForeignTick`. `Resolved::Unknown` values lower verbatim from the declared
  schema; the boundary still refuses only an unknown *owner*.

## Not done here (deliberately)

- Staged destruction visuals (a sinking/crashing wreck trajectory), launch-bay
  socket/capacity consumption, capture and cargo wiring: F35-C. The declared
  `socket_offset_m`/`capacity` fields stay parked on the declared record; the
  `cs_app::capital` module doc says so.
- Projectiles, their guidance and the world-actor path that *produces* a
  `CapitalHit`: weapon-side wiring lands elsewhere; the set is the consumer.
- Original mission validation: F35-D (needs `retail`).
- No Avian body, no renderer, no file access: `cs_sim` depends only on
  `cs_types` and `cs_script`.

## Unknowns

- Whether the original couples engine loss to course speed proportionally,
  whether weakpoint windows are tick-indexed cycles, whether gas cells/sections
  carry per-section health pools, and what traverse limits and boresights the
  original turrets used are all unrecovered. Every number here (the 20 m/s
  course, 120/200 integrity pools, the 180° cone) is authored design, not a
  measured rule; a retail format lead is required before any of this can become
  a fidelity claim.
- The antiparallel-aim rim selection is a deterministic designed choice, not a
  recovered convention.

## Files

`crates/cs_sim/src/capital/{mod,ship,parts,synthetic,runtime}.rs` (runtime is
new); `crates/cs_content/src/capital.rs` (declared `boresight` + validation);
`crates/cs_app/src/capital.rs` (boresight/sections lowered); the three test
files above; the F35-A test files (fixture literal updates for the new fields);
and the `cs_sim` lib doc bullet.
