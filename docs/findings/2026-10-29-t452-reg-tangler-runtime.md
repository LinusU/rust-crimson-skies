# T452-REG-TANGLER-RUNTIME: `wep_12` opens its measured field at fire time

Task #2186 (`T452-REG-TANGLER-RUNTIME`). Static measurement only: no original
executable was run, and nothing here is `verified_original`. Test prefix
`accept_t452_tangler_` (`crates/cs_app/tests/accept_t452_tangler_runtime.rs`).

## What was already measured

* `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §1
  (`TANGLER`): parsing `TANGLER` installs the tick callback `0x4ba660`; the
  fire path builds the field object at `0x4b94e0` (`+0x00` owner, `+0x04`
  weapon, `+0x08` position, `+0x14` `RADIUS²`, `+0x18` `TIME`); its tick
  `0x4b9590` subtracts the frame time from `+0x18`, and while that stays
  positive walks the actor list and calls the weapon's per-hit routine
  `0x4b9bc0` for every live actor whose squared distance is below `RADIUS²`,
  the owner included; at `<= 0` the field is unlinked and freed. `wep_12`
  states `TIME 2` and no `RADIUS`, so the field is the image's 10 m default.
* `docs/findings/2026-10-28-t452-tangler-effect.md`: the per-hit routine's own
  `TANGLER` branch (`0x4b9ded`) zeroes both damage channels and applies a timed
  tangle of `max(2 s, 10 * (1 - d2 / RADIUS))`; a re-hit raises the timer and
  never shortens it; the actor force routine `0x48fc40` stores `0.0` into its
  local force scalar `[ebp-8]` while the tangle flag is set; the boost setter
  `0x4b2110` refuses to enable while the flag is set, so a tangle ends a boost.

## What this stage expressed

1. **The record registers.** `measured_ordnance_declaration` no longer turns a
   `TANGLER` record's measured area into a one-shot `AreaEffect` (which the
   runtime refuses when it outlives the item, and which would have applied once
   at detonation, not per tick). A `TANGLER` record declares no area effect at
   all; the field is measured out of band by `measured_tangler_field`, which is
   `cs_content`'s own `lower_tangler_field` over the row (radius, `TIME`, the
   measured `TangleStatus`). `wep_12` now registers, and so does every other
   declared id: the retail report has sixteen of sixteen registered and nothing
   blocked.
2. **The fire path opens it.** `OrdnanceSession::register_tangler_field`
   attaches the measured declaration to a component id; the launch order for
   that component opens the field on the same tick, at the pose the fire path
   read (the same `transform.origin` the launch effect reports), owned by the
   shooter. A launch of a component with no registered field opens none. A
   declaration that is not created at fire time, or not re-applied every tick,
   is refused by name.
3. **The tangle is readable.** The session already exposed the timed tangle
   (`tangle_status`, `is_tangled`, `tangled`, `tangle_status().remaining_ticks`).
   This stage adds the narrowest form of the measured boost consequence,
   `boost_allowed(actor)`: `false` while the tangle is live, `true` once it
   clears. That is what a flight producer passes to
   `FlightState::boost_available`.

## What stays named

* `t452_reg.limit.tangler_force_scalar_meaning` — what the suppressed force
  scalar `[ebp-8]` is in physical terms. The measured statement is "the scalar
  is zero while tangled", not "thrust is zero".
* `t452_reg.limit.tangler_field_origin` — which world position the original's
  fire path hands `0x4b94e0`. The field's own `+0x08` slot is measured; the
  caller's source is not, so the runtime takes the centre from its own fire
  path and never defaults it to the launcher's actor record or the world origin.
* The aircraft-side consumer. Nothing in a live mission opens a `wep_12` field
  (M01 flies the designed rocket loadout), so no production flight model reads
  this state yet; the task that measures and expresses the flight-model
  consumption is `T452-REG-TANGLER-EFFECT` (#2179).
* The victim list. The session's per-tick pass reads the `OrdnanceStep`
  target list, and M01's rocket target list omits the player's own actor. The
  measured tick walks the whole actor list with the owner included, so the
  flight consumer that drives a tangler must supply every actor it wants
  tangled — including the player and the shooter. The runtime does not add
  victims of its own; the ledger's `apply` docs state that obligation.

`t452_r02.limit.tangler_flight_effect` is therefore resolved rather than kept:
the effect it left unnamed is exactly the measured timed force-scalar
suppression above, and the two unpinned parts stay named on the record (both
appear in the registration report's kept claims and in the evidence report's
`unknowns`).

## Traceability

The measurement above was authored on #2179's branch and is carried into this
branch in its own commit (`docs/findings/2026-10-28-t452-tangler-effect.md`, the
`[[tangler_fact]]` rows in `data/owner-static/engine-facts.toml` and
`accept_t452_tangler_measurement`), because main's `TANGLER` vocabulary and the
tangler ledger both cited a finding that was not in the tree. The ignored image
test re-reads every cited window.

## Evidence

`docs/findings/evidence/T452-REG-TANGLER-RUNTIME.json`, with
`original-field.json`: the owner's catalogue registered through the production
path, `wep_12` launched through `step_ordnance_session`, the field opened from
the mount pose, 2 s of per-tick re-application, and the tangle cleared on its
own measured boundary after the field was gone.
