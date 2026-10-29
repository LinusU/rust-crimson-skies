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

Which of the five callers of `0x5ac7a0` a given shot reaches is not fully
distinguished, but each site was checked statically (r2 over the image, base
`0x400000`; no original run). Every one of them is a burst inside the weapon's
own code:

| site | enclosing function | how it is reached |
| --- | --- | --- |
| `0x5ac2b1` | `0x5ac210` | a detonation handler, called from `0x5ac381` and from the proximity test `0x5b0970` at `0x5b0a02` |
| `0x5ac410` | `0x5ac3a0` | the common impact/burst entry, called from the per-frame update at `0x5af7d1`, from the range-end detonation at `0x5b0310`, and from `0x4b6441` |
| `0x5b0713` | `0x5b05c0` | the per-frame update at `0x5af8b1`, when weapon flag `0x4000` (`[weapon+0x74]`) is clear and the impact point resolved |
| `0x5b0bec` | `0x5b0a50` | the same update at `0x5af8bf` when that flag is set; the burst is gated on the contact test `0x5acc70` |
| `0x5b147c` | `0x5b0cb0` | the hit/damage resolution the impact handler calls at `0x5ac50a`, which builds the hit record it bursts with |

For `wep_12` the one measured end of flight is the silent end of `RANGE`
(`0x5b0318`), which reaches none of the five: the range end only bursts when
`DETONATE_AT_RANGE` is set (`0x5b0310`), and no measured record is both unguided
and range-end detonating (`T452-REG-SILENT-EXPIRY`). A `wep_12` field therefore
opens only on a contact — the collision path (`0x5b0a50` → `0x5b0bec`) or the
impact entry (`0x5ac3a0` → `0x5ac410`) — and that is the path this runtime keeps
ahead of the range end. The runtime's two burst paths each have a call site: a
fuse trigger reaches one of these detonation/contact sites, and a guidance-loss
`Detonate` (the measured LOCK_ON range-end detonation) reaches `0x5b0310` →
`0x5ac410`. No runtime path opens the field where the image has no caller.

Not measured: which of those contact sites a real `wep_12` physical contact
takes, because the weapon flags that select between them (`0x4000` in
`[weapon+0x74]` at `0x5af8a9`, and the `0x80000000` test inside `0x5b05c0`) are
not traced.

Not expressed, recorded here with its content: on the contact path `0x5b05c0`
the original writes the hit record's position (`[record+0xc]`) into the
projectile's own slot (`rocket+0x48`, `0x5b06d5..0x5b06fd`) before it calls
`0x5ac7a0`, so the field is centred on the point of contact. This runtime passes
`LiveOrdnance::current` — the item's position at the end of the burst tick's
swept segment, which the task's "the projectile's position at that tick" names —
and that can differ from the interpolated contact point by up to one tick's
displacement (40 m for `wep_12` at 1200 m/s and 1/30 s). Affected content:
`wep_12`'s field centre, and therefore which actors the field catches near its
edge, on a contact burst. No task resolves it yet; the runtime's trigger routing
persists the same non-interpolated geometry.

## Claim

`t452_reg.limit.tangler_field_origin` is resolved and removed from the
registration report's kept claims and from `original_kept_claim_context`, so a
registered `TANGLER` record keeps no claim of the field's own: the force
scalar's identity was resolved separately by task #2179 (it is the force block's
thrust magnitude) and its claim stays removed. Main's #2179 flight tests now
fire, fly and burst before they read a tangle. Earlier evidence reports under
`docs/findings/evidence/` are historical records of their own tasks and are not
rewritten; the harness that regenerates `T452-REG-TANGLER-RUNTIME`'s report was
updated to observe the burst open instead of the removed fire-time open.
