# T452-REG-SMOKE-STUN: what the smoke emitter's stun does

Task #2187. Static analysis of the owner-supplied decrypted engine image (base
`0x400000`). Provenance `static`; nothing here is original-run behavior and
nothing is `verified_original`. Follows
`2026-10-28-t452-reg-smoke-radius.md`, which named the two routines.

## The emitter's pick (`0x4b8fd0`)

Every step the node is alive it walks the actor list. An actor is skipped when
its `+0x91d` byte is set or it is the node's owner. The owner-to-actor vector
is normalized in place by `0x422690`, which also returns the length; the actor
is reached when the length is below `smokescreen_stun_range` and the dot with
the owner's forward axis (`owner + 0x198`) is above the cosine stored from
`smokescreen_stun_angle` (`cos(angle * pi/180 * 0.5)`, so a half-angle: 85 deg
for the retail 170 deg). Both tests are strict. The node does not act once.

## A non-player actor: `0x4200d0`

Same incapacitation `SONIC`/`FLASH` apply (`T452-R.05`): skipped unless the
actor is not the player (`0x71c298`), `+0x91d == 0`, `+0xf8 == 0` and `+0x67c`
is `0` or `4`; then mode word `4` at `+0x358`, six control words zeroed
(`+0x100`, `+0x108`, `+0x10c`, `+0x114`, `+0x11c`, `+0x120`) and `+0xc0 =
clock (0x71c470) + argument`. The argument is `smokescreen_stun_interval`
(retail 5 s). The routine also formats a debug string from `+0x978`; nothing
reads it for gameplay and it is not recorded here. Because the emitter re-hits
every step, the stun lasts the interval after the last step the actor was
inside the cone.

## The player: `0x42e9d0`

Not a control loss. The emitter (`0x4b917f..0x4b91da`) tints the player's
screen only while its own timer (node `+0x14`) is below `0.5` (`0x6032e0`),
then stores `2.0` there. Call arguments: duration `2.0 s`, colour
(0.2, 0.29, 0.145), scalar `0.9`, or `0.97` when the timer was not positive.
`0x42e9d0` ignores a non-positive duration; otherwise it stores the duration
and its `0.15` and `0.35` fractions (`0x6036a8`, `0x603508`) and the colour and
scalar, blending into a tint already active. The timer at node `+0x14` is
decremented by the global at `0x9ad744` each step and clamped at zero
(`0x4b9072..0x4b908b`); that this global is the frame time is not decoded.

## Not measured

What the `0.15`/`0.35` fractions mean to the renderer (fade-in/out phases is a
guess, not asserted), and the meaning of `0x9ad744`.

## Runtime

`cs_content::ordnance::SmokeStunRule` (carried by `OriginalBehaviorMeasurement`
and `DeclaredProjectile::smoke_stun`) states all of the above with the three
globals as keys; `resolve` takes the retail values. `wep_13` no longer keeps
`t452_r02.limit.smoke_stun_effect`. Applying the rule per tick in the
simulation is a separate follow-up.

Tests: `accept_t452_smoke_stun_` in `crates/cs_content/tests/`.
