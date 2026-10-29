# T452-REG-TANGLER-CONTACT-CENTRE: a contact burst centres the field on the point of contact

Task #2205. Follow-up to `2026-10-29-t452-tangler-burst-open.md`; static
measurement only (Capstone/r2 over `$CS_ENGINE_IMAGE`, base `0x400000`); no
original executable was run and nothing here is `verified_original`.

## Measured

The burst hook `0x4ba660` receives `rocket+0x48` and the field constructor
`0x4b94e0` copies those three floats into the field (`0x4ba6ca`,
`2026-10-29-t452-tangler-origin.md`). On the contact path the slot is not left
at the position the projectile happened to end the frame at: the per-frame
update's contact function `0x5b05c0(weapon, rocket, record)` writes the hit
record's position into it before it bursts:

| address | instruction | meaning |
| --- | --- | --- |
| `0x5b06df` | `lea eax, [esi + 0x48]` | the projectile's own slot: `rocket+0x48` |
| `0x5b06e2` | `lea edx, [ebx + 0xc]` | the hit record's position: `record+0xc` |
| `0x5b06e5..0x5b06f4` | three `mov dword` through `ecx = eax` | copies `record+0xc..0x14` into `rocket+0x48..0x50` |
| `0x5b06fd..0x5b0706` | `mov eax, [eax]` / pushes / `call 0x4d1d50` | moves the projectile's node to the copied position |
| `0x5b0713` | `call 0x5ac7a0` | the impact/burst function, which calls the TANGLER hook |

`0x5b05c0` is reached from the per-frame update at `0x5af8b1` when the weapon's
flag word has `0x4000` clear (`0x5af8a9 test ch, 0x40`); the record it is handed
is the hit record the contact resolution `0x5b03f0` returns. So the field is
centred on the point of contact, not on the projectile's position at the end of
the burst tick.

## Change

`cs_app::ordnance::step_ordnance_session` now centres a burst's tangler field
with `burst_field_centre`:

* a swept contact (`FuseTrigger::Impact`, carrying `SweptHit::time_of_impact`, a
  fraction of the tick) interpolates the item's own swept segment:
  `previous + t * (current - previous)`, using `LiveOrdnance::previous` and
  `LiveOrdnance::current` and the fraction the runtime already holds;
* every trigger that is not a contact — a timed fuse, a proximity radius — keeps
  the item's tick-end position (`LiveOrdnance::current`);
* the guidance-loss `Detonate` path keeps the last position the runtime
  recorded, which is what #2203 already did and what that path's own caller
  passes.

A contact at `t = 1.0` evaluates to the tick-end position exactly, so the
interpolation adds nothing where the original's record would already name the
item's own end position.

## Not expressed

The record position `[record+0xc]` is filled by the collision/contact code
(`0x5b03f0` and the `0x4c8ec0` query it runs), which this task did not trace to
its writer. The runtime uses the point on the projectile's own swept segment,
so it agrees with the record position when the contact is at the projectile's
centre and can differ by the contact geometry of the target, a sub-metre shift
for an aircraft-scale target. Affected content: `wep_12`'s field centre, and
therefore which actors the field catches near its 10 m edge, on a contact burst.
The original collision shapes are unmeasured (the same limitation
`SweepTarget` names as F27-D), so the resolving work is that measurement, not a
further interpolation rule; task #2208 ("Measure what fills the hit record
position `[record+0xc]` and the target contact geometry") carries it. This task
opens no new claim and does not reopen `t452_reg.limit.tangler_field_origin`,
which #2203 measured and closed.

A **proximity** fuse also ends the item away from its tick-end position, and this
change deliberately does not interpolate it: `FuseTrigger::Proximity` carries a
fraction of the tick, but under the measured `StartOfStepSample` sampling
(`0x5b0970`'s start-of-step sample) that fraction is always `0.0`, and nothing
measured says the proximity site `0x5ac2b1` (reached from `0x5b0970`) writes a
contact position into `rocket+0x48` the way `0x5b05c0` does. Centring it on the
item's tick-end position is the honest state, not a claim that the original does
so; the affected content is any proximity-fused record's burst centre (`wep_12`
is unguided — T452-R.02 measured the proximity path over `LOCK_ON` — so it does
not take this path). Measuring it is part of #2208.

Which of the five callers of `0x5ac7a0` a real `wep_12` contact takes remains
unmeasured from `2026-10-29-t452-tangler-burst-open.md`; this change moves the
centre for every contact site that does reach `0x5ac7a0`, and leaves each other
path as it was.

One more residual is visible from here and belongs to the session, not to this
interpolation: `step_ordnance_session` hands the whole `OrdnanceStep::impacts`
slice to every live item's fuse decision, and `fuse_decision` takes
`impacts.first()` without checking `SweptHit::projectile`, so in a session with
two live items one contact ends both — and, now, centres the other item's field
at a fraction taken from a hit on a different segment. Task #2207 ("Filter swept
contacts by projectile in `step_ordnance_session` before the fuse decision")
resolves it; every test here keeps one live item, so this change neither
introduces nor hides it.

## Tests

Prefix `accept_t452_tangler_`, `crates/cs_app/tests/accept_t452_tangler_runtime.rs`:

* `accept_t452_tangler_runtime_a_contact_opens_the_field_at_the_interpolated_point`
  — a `wep_12` launched and burst by a `SweptHit` at `t = 0.25` opens its field
  at `[4, 5, -12.5]`, not at the tick-end `[4, 5, -20]` and not at the mount.
* `accept_t452_tangler_runtime_a_contact_at_the_end_of_the_tick_opens_at_the_tick_end`
  — `t = 1.0` opens at the tick-end position.
* `accept_t452_tangler_runtime_a_timed_fuse_without_a_contact_opens_at_the_tick_end`
  — a timed fuse with no swept hit keeps the item's tick-end position.

The runtime, ledger and flight suites' contact cases were moved from the
tick-end centre to `[4, 5, -15]` (`t = 0.5` on their `[4, 5, -10] → [4, 5, -20]`
segment), not weakened: the centre and the victim distances are still asserted
exactly.
