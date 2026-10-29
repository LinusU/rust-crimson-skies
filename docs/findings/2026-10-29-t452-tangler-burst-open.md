# T452-REG-TANGLER-BURST-OPEN: the runtime opens the tangler field at the burst

Task #2203. Follows `2026-10-29-t452-tangler-origin.md` (static measurement, nothing
here is `verified_original`; no original run).

## Change

`cs_app::ordnance::step_ordnance_session` no longer opens a registered tangler
field on the launch tick. It opens it on the tick the projectile bursts, centred
on the projectile's position that tick (`LiveOrdnance::current`, the
`rocket+0x48` the hook reads), owned by the projectile's shooter:

* a fuse trigger (impact, proximity, timed detonation): at the trigger, before
  the item is retired;
* a guidance-loss `Detonate`: at the position the runtime last recorded.

The silent end of `RANGE` (`FuseDecision::Consumed`) does not reach the burst
function (`2026-10-23-flak-c-burst-effect-parts-static.md`) and opens nothing.
The tangler pass now runs after those paths so a field opened by a burst applies
on the burst tick.

Which of the five callers of `0x5ac7a0` a given shot reaches is not
distinguished: every burst path the runtime models opens the field; the
caller-by-caller split stays unmeasured.

## Claim

`t452_reg.limit.tangler_field_origin` is resolved and removed from the
registration report's kept claims and from `original_kept_claim_context`.
`t452_reg.limit.tangler_force_scalar_meaning` stays named. Earlier evidence
reports under `docs/findings/evidence/` are historical records of their own
tasks and are not rewritten.
