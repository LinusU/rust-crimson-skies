# T452-REG-TANGLER-EFFECT: what a zero-damage `TANGLER` field does to a flight model

Task #2179 (`T452-REG-TANGLER-EFFECT`). Static analysis of the decrypted
engine image (provenance `static`; the original executable was not run), plus
the runtime expression of the measured effect. Test prefix
`accept_t452_tangler_` (this stage: `accept_t452_tangler_flight_*`). Nothing
here is `verified_original`.

## Result

`t452_reg.limit.tangler_force_scalar_meaning` asked what the force scalar
`[ebp-8]` that a tangle zeroes is in physical terms. It is the **thrust
magnitude** of the actor force block, so a tangled aircraft produces **no
thrust force**, and its boost is refused.

Row `tangler.force_zero` (`data/owner-static/engine-facts.toml`, `0x48fdd0`)
already measured that the force routine `0x48fc40` stores `0.0` into `[ebp-8]`
while the actor status flag 2 is set. Two further committed rows close the
identity of that slot:

1. **It is the thrust scalar.** `0x48fde1..0x48fdf0` multiplies the
   thrust coefficient it already holds in `[ebp-8]` by the actor's `+0x66c` and
   `+0x678` stores and writes the product back into `[ebp-8]`; the force-block
   reread labels those two stores throttle and wing area `S`
   (`docs/findings/2026-10-25-force-block-reread.md`). The tangle branch jumps
   from `0x48fdd8` straight to `0x48fdf3`, so it skips exactly that multiply and
   leaves `[ebp-8]` at `0.0` (`tangler.thrust_scalar`).
2. **Its only use is the thrust term.** `0x48fe91` loads `[ebp-8]`, negates it
   (`0x48fe97`) and multiplies it into the force by the actor's
   `+0x198/+0x19c/+0x1a0` body Z axis (`0x48fe9e`, `0x48fea9`, `0x48feb2`):
   `force += -[ebp-8] * body Z`, i.e. thrust along the nose (`tangler.thrust_vector`).

This agrees with the independent force-block reread of task #2071, whose rows
`48fde1` ("thrust = Tc * throttle ... * S") and `48fe91` ("thrust along -Z (the
nose)") annotate the same two windows (`docs/findings/2026-10-25-force-block-reread.md`
section 1 and its byte block).

What the branch does **not** touch, from the same windows:

* the throttle store `+0x66c` is not written, so the first call after the flag
  clears recomputes a full thrust from the untouched throttle;
* lift, drag and gravity keep their own stores (`0x48fdf3..0x48fe8f`), so the
  measured effect is a thrust cut, not a control lock and not a drag change
  (this is also what `docs/findings/2026-10-28-t452-tangler-effect.md` recorded
  against the `0x4b9bc0` premise);
* the control torque terms (the `0x48c470` caller's `veh+0x114/0x11c/0x120`
  path) are never read by the tangle branch.

The second measured consequence, boost, is `tangler.boost_refuse`
(`0x4b2136`): the boost setter `0x4b2110` refuses to enable while flag 2 is
set, and `0x4b1690` calls it with `false` whenever the flag is touched, so a
tangle also ends a boost that was already running.

## What the runtime expresses

* `cs_sim::flight::FlightState::thrust_suppressed` is the input. While it is
  true the fixed-wing law produces no thrust at all — base and boost alike —
  and accepts no boost consumption. Every other term (lift, drag, gravity,
  control torque, spool advance) is unchanged, because that is what the two
  windows show. The exceptional law reuses the same base computation, so it
  inherits the cut.
* `cs_app::physics::flight::FlightAircraft::set_tangle` carries the ordnance
  session's own `TangleState`, and the driver derives both measured
  consequences from it on every tick: `thrust_suppressed` is `true` and
  `boost_available` is `false` while the status is live. One status, two
  places the image checks it, kept as two inputs because the image checks them
  in two different routines.
* `cs_app::physics::FlightTangle` is the producer's component. A mission that
  owns an `OrdnanceSession` writes `session.tangle_status(&actor)` onto the
  body's entity and the driver binds it before the equations run, exactly as it
  binds `FlightEquipment`.

**Absence is "not tangled".** A body with no `FlightTangle` component has its
tangle cleared on the next driven tick. That is deliberate: the status is a
timer, so a producer that stops writing must not leave an aircraft with no
thrust forever. The timer itself is never run by the flight side — the ledger
remains its only owner.

The runtime introduces no number of its own: what a tangled aircraft does not
get is the thrust its own declared tuning computes.

## Verified expression

`accept_t452_tangler_flight_*` (`crates/cs_app/tests/accept_t452_tangler_flight.rs`)
drives the production path:

1. A registered `wep_12` — the synthetic fixture carries the record's measured
   shape through `measured_ordnance_declaration` and `register_measured_ordnance`
   — fires through `step_ordnance_session`, opens its measured field at the
   mount pose the fire path read, lives `TIME 2 s` = 60 whole 30 Hz ticks and
   re-applies the tangle to the body inside it on every one of those 60 ticks.
2. The body it reaches, flown by the real Avian schedule through
   `FlightForcesPlugin`, produces exactly zero thrust on every tick of the live
   tangle and its boost is refused; once the tangle clears on its own boundary
   the thrust returns at the held throttle.
3. Two identical cruisers on one tick — one tangled, one not — differ by
   exactly the thrust vector and nothing else: same lift, same drag, same
   gravity, same control authority, same torque.
4. A body whose producer record is removed is not tangled on the next tick.

## Claim state

* `t452_reg.limit.tangler_force_scalar_meaning` is **resolved**: the scalar is
  the force block's thrust magnitude. The registration report no longer lists
  it among `wep_12`'s kept claims, `measure_component_behavior` reports it
  resolved, and the status the field applies is named for the measured meaning
  (`cs_content::ordnance::TANGLE_STATUS_NAME`). The claim id stays in the code
  so an older report or finding still resolves its citation.
* `t452_r02.limit.tangler_flight_effect` stays resolved by #2186; this stage is
  the flight-model half of the expression it announced.
* `t452_reg.limit.tangler_field_origin` still stays named: which world position
  the original's fire path hands `0x4b94e0` is not measured.
* The AI decision path at `0x4202e3` still stays named (what it skips at
  `0x420695`), as does which per-actor update counts the timer down for which
  actor kind.

The three committed acceptance reports of the earlier T452 stages
(`docs/findings/evidence/T452-REG-TANGLER-RUNTIME.json`,
`T452-REG-SILENT-EXPIRY.json`, `T452-RUNTIME-REGISTER.json`) name the scalar
claim in their `unknowns` lists as of their own runs; those copies are records
of those runs and are not edited by hand. A regeneration of each harness drops
the entry, because the registration report no longer keeps the claim.

## Not claimed

No original-run behavior of any kind, and no observation of how the original
mission flies a tangled aircraft. Nothing in a live mission opens a `wep_12`
field today (M01 flies the designed rocket loadout), so the flight-model
consumer is wired and tested but not yet exercised by mission content; the
mission that first fires a tangler supplies its own victim list, including the
player, as the ledger's `apply` documents.
