# T1603 — timed, proximity and impact detonations use the area blast

Closes `t548.limit.session_wiring` / `t1318.limit.session_wiring` for the
trigger path. #1317 had wired the guidance-loss blast; this wires the rest.

* `cs_sim::weapons::OrdnanceRuntime::route_trigger_area` is the trigger
  counterpart of `route_detonation_area`. The centre is the item's current
  position, the attacker its shooter; both paths share one routine
  (`route_area_blast`), so the falloff `1 - d^2/r^2` and the once-only latch are
  the same.
* On `FuseTrigger::Impact` the directly-hit actor is skipped (the measured
  impact-path exclusion, `2026-10-18-t1318-blast-unknowns-measured.md` Q3).
  `Timed` and `Proximity` exclude nobody (the original passes key 0 there).
* `step_ordnance_session` uses it when the item declares an `AreaEffect` and
  `OrdnanceStep::blast_recipients` is `Some`; otherwise the single-target
  `route_trigger` path is unchanged.

## Still open

* The exclusion key is the hit object's damage group; its domain is unmeasured
  (`t1318.limit.exclusion_domain`). This layer treats one `ActorId` as one group,
  so a second node of the hit actor's own group is not separately excluded or
  included.
* No producer yet fills `blast_recipients` from live actors in the running game;
  the recipients are still supplied by the caller of the step.
* The +0x678 blast scale (arming-window multiplier) is not applied to the trigger
  blast.
* The committed evidence reports for T548/T1318 still carry their original
  `session_wiring` limitation text; they are not edited here.
