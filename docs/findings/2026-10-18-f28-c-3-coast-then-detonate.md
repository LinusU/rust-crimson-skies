# F28-C.3: the original's lost-target behavior as a declared rule

Date: 2026-10-18. Task: #1319 (follow-up of #548). Source of the rule:
`docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`, "What the
projectile step then does". No new measurement was made and nothing here is
`verified_original`.

`cs_sim::weapons::ordnance::LostTargetBehavior::CoastThenDetonate` (declared as
`cs_content::ordnance::DeclaredLostTargetBehavior::CoastThenDetonate`, lowered
by `cs_app::ordnance`) expresses the measured rule: a lost target is a cleared
pointer, the item flies on its last vector, its declared `FuseRule::Timed`
still detonates it through `OrdnanceRuntime::decide`, and at the end of its
lifetime it detonates where it is (`OrdnanceTick::range_end_detonated`)
instead of expiring silently as `Coast` does. `Detonate` and `Coast` are
unchanged. The behavior is selected by declaring it; no retail record is
lowered to it yet because the retail record to rule mapping is not part of
this task.

## Not modelled (still unknown)

- The unit of `RANGE`: the item's `lifetime_ticks` stands for it, and no
  conversion is invented.
- What removes a projectile past `RANGE` that does not detonate.
- The `EXPIRES` / `DETONATE_AT_RANGE` flag split: a retail locked-on rocket
  with `EXPIRES` and without `DETONATE_AT_RANGE` would not detonate at range
  end. Choosing this behavior for such a record is the caller's declaration.
- A tracked (not lost) item still expires silently here; the retail
  range-end detonation of a locked-on rocket is outside the lost-target rule.
