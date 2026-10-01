# F35-A: capital-ship subsystem and bay contracts

Task #143. Runtime contract `cs_sim::capital`; declared schema `cs_content::capital`;
lowering boundary `cs_app::capital`. Tests:
`crates/cs_sim/tests/accept_f35_a_capital_ship.rs`,
`crates/cs_content/tests/accept_f35_a_capital_schema.rs`,
`crates/cs_app/tests/accept_f35_a_capital_boundary.rs`.

All behavior is **designed**, synthetic and not original-verified. Awards at most
*checked*. No original capital-ship data exists on this machine and none was used:
`Origin::SyntheticFixture`, subject `airframe/synthetic.leviathan`, every `Resolved`
carrying designed provenance.

## Designed semantics

- **Subsystems, not one health bar.** `SubsystemGraph` keys parts by stable
  `SubsystemKey`, pairs each `SubsystemKind` with the `SubsystemEffect` its
  destruction changes, and refuses an effect its kind does not allow. `disable`
  is the single transition: it applies the part's effect, is idempotent, and
  destroys the actor only when the part is `GasCell`/`StructuralSection`
  (`can_be_lethal`). One observable failure: a weapon-bay hit that also changed
  propulsion, or a gas-cell hit that left the actor alive, would show here.
- **Engine loss is a motion change.** `EngineSpec` holds a validated unit axis
  and a `Resolved<f64>` thrust; `CapitalShip::engine_thrust_n` /
  `propulsive_force_n` / `propulsive_acceleration_m_s2` sum only the intact
  engines. The F35-A minimum scenario disables one engine (acceleration drops
  from 4.0 to 2.0 m/s²), then both (0.0), while `is_destroyed()` stays false and
  the keel stays `Intact`. An unresolved thrust makes the whole sum
  `Err(UnknownThrust)` rather than substituting zero.
- **Bays are time-indexed weakpoints.** `ExposureWindow::state_at(tick)` returns
  `Concealed`/`Opening`/`Exposed`/`Closing` from one pure tick; `is_weakpoint`
  is true only for `Exposed`. A destroyed bay is `BayState::Destroyed` at every
  tick and never exposed. A closed bay is therefore not an always-hittable
  invisible bar (non-negotiable 2). A cycle whose four phases would sum past
  `u64::MAX` is refused (`ExposureError::CycleOverflow` /
  `ExposureSchemaError::CycleOverflow`) rather than wrapped into a shorter or
  zero-length cycle, which would overflow (or divide by zero) at query time.
- **Release once, cancel on destruction.** `LaunchSocket` + `release_aircraft`
  use the F34 `anchor_sample` so a spawn pose is the anchor pose (carrier
  velocity, rotational part and ejection included) and `dynamic_authority` is
  granted exactly once. `LaunchLedger` assigns a stable `LaunchId` per bay,
  releases each at most once, and moves a destroyed bay's pending launches to
  `cancelled` instead of deferring them — the data half of AC03.
- **Staged capture.** `CaptureTransaction` advances
  `Approaching → Eligible → Latching → Transferring → Completed` and produces
  `Ownership { captured: true }` only at `Completed`; an `Aborted` or terminal
  transaction refuses to advance or abort again and never transfers ownership.
- **Unknown stays unknown.** Every load-bearing declared value is `Resolved`.
  `lower_capital_ship` carries `Unknown` through verbatim for engines, turret
  weapons, traverse, anchors and cargo; only an unknown initial **owner** is
  refused (`CapitalLowerError::UnknownOwnership`), because guns, targeting and
  docking eligibility cannot switch coherently under a guessed owner.

## Not done here (deliberately)

- Movement integration, weakpoint hit resolution and turret behaviour: F35-B.
- Script-driven spawn, the capture/cargo state wiring and staged destruction:
  F35-C; the AC03 script path and the AC04 in-flight-projectile relation switch
  are not present, only their identity/ordering contract.
- Three declared fields are validated and carried but not lowered into the
  F35-A runtime aggregate, which has no counterpart for them: a launch bay's
  `socket_offset_m` and `capacity` (consumed by F35-C launch wiring) and a
  gas/structural section's `integrity` pool (consumed by F35-B damage
  resolution). The boundary refuses to invent runtime state for them; the
  `cs_app::capital` module doc says so explicitly.
- Original mission validation: F35-D (needs `retail`).
- No Avian body, no renderer, no file access: `cs_sim` depends only on
  `cs_types` and `cs_script`.

## Unknowns

- The original subsystem set, engine coefficients, bay cycle timing, capture
  precondition and its gun/targeting/docking switch are unrecovered. Every
  number here (400 kN thrusts, 200 t mass, the 40/10/60/10 and 30/5/45/5 cycles,
  5 t cargo, the capture stage order) is authored design, not a measured rule.
- Whether the original models bay opening as a tick-indexed cycle at all, and
  whether capture is staged, are unverified. A retail format lead is required
  before any of this can become a fidelity claim; the affected content is every
  capital-ship definition and every mission that targets a subsystem or bay.

## Files

`crates/cs_sim/src/capital/{mod,subsystem,motion,bay,launch,capture,parts,ship,synthetic}.rs`;
`crates/cs_content/src/capital.rs`; `crates/cs_app/src/capital.rs`; the three
test files above; module wiring and doc bullets in each crate's `lib.rs`; and
`cs_script.workspace = true` in `crates/cs_app/Cargo.toml` (the boundary names
`cs_script::ir::ActorId`, which `cs_sim` does not re-export).
