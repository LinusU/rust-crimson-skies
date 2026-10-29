# T452-REG-TANGLER-EFFECT: what a zero-damage TANGLER does to an actor

Task #2179. Static analysis of the decrypted engine image (provenance `static`;
no original run). Cited rows: `[[tangler_fact]]` in
`data/owner-static/engine-facts.toml`; test prefix `accept_t452_tangler_`.

## Result

`t452_r02.limit.tangler_flight_effect` assumed that no drag or control-lock
store was reachable from the per-hit routine `0x4b9bc0`. That was wrong: the
routine has a branch of its own for the `TANGLER` flag (`0x40000`) at
`0x4b9ded`, taken before the damage path.

1. **The branch ignores the damage channels.** It zeroes both of them
   (`0x4b9df5`), so the record's zero armor/health damage is the design, not a
   missing effect.
2. **It tangles the victim for a duration.**
   `duration = max(2.0 s, 10.0 * (1 - d2 / RADIUS))`, `d2` the squared distance
   the caller passes (the field tick `0x4b9590` passes the `0x538880` result
   unchanged) and `RADIUS` the unsquared record radius at weapon `+0x1c`.
   For `wep_12` (`RADIUS` 10 m) the duration is above the 2 s floor only for
   `d2 < 8` (about 2.8 m from the field position). The constants are `1.0`
   (`0x6032dc`), `10.0` (`0x62b124`) and `2.0` (`0x62b120`). The routine calls
   `0x4b1690(flag 2, set, duration)` on the victim, owner included.
3. **The tangle is flag 2 of the actor status word `+0x2dc`** with a timer at
   `+0x2e0`. A new hit only raises the timer (`max`), never shortens it. The
   flag-2 transitions attach and release a presentation object
   (`0x4b15c0`/`0x4b1630`).
4. **Consumers.** The force routine `0x48fc40` zeroes its local force scalar
   `[ebp-8]` while flag 2 is set (`0x48fdd0`..`0x48fdd8`), instead of
   `+0x66c * +0x678 * scalar`; the scalar's contribution
   `-[ebp-8] * (+0x198, +0x19c, +0x1a0)` is therefore zero. The same routine
   counts the timer down by the frame time (`0x9ad744`) and clears the flag
   at `<= 0`; two other per-actor updates (`0x47027c`, `0x48c606`) run the
   identical countdown. The boost setter `0x4b2110` refuses to enable while
   the flag is set and `0x4b1690` calls it with 0 whenever flag 2 is touched,
   so a tangle ends any boost. An AI decision path (`0x4202e3`) branches on
   the flag.

## Not decoded

- What `+0x66c`, `+0x678` and the vector at `+0x198` mean in physical terms
  (throttle, engine output, thrust axis are plausible, none is measured). The
  claim therefore stays named: the measured effect is "the force scalar is
  zero while tangled", not "engine thrust is zero".
- What the AI path at `0x4202e3` skips (`0x420695`).
- Which of the two update paths (`0x47027c`, `0x48c606`) belongs to which
  actor kind.

## Runtime consequence

The runtime `AreaEffect` applies damage once at detonation. The measured
field is created at fire time, lasts `TIME`, is hit-tested every tick against
every live actor, and its effect is a per-victim timed status with a
distance-dependent duration. The vocabulary needs: a persistent area field, a
per-tick re-application, and a timed "force suppressed" actor status. That
expression is split from this measurement.

**Update (2026-10-29).** That expression now exists: the vocabulary landed in
#2185, the per-tick field ledger and tangle timer in #2196, and the production
fire path that opens `wep_12`'s field — with the readable tangle timer and the
measured boost refusal — in #2186
(`docs/findings/2026-10-29-t452-reg-tangler-runtime.md`). The claim
`t452_r02.limit.tangler_flight_effect` is resolved by that effect; the
unpinned parts above stay named
(`t452_reg.limit.tangler_force_scalar_meaning`,
`t452_reg.limit.tangler_field_origin`). This note records where the
measurement is expressed; it claims no original-run behaviour.

**Update (2026-10-29), task #2179.** The "Not decoded" entry for `+0x66c`,
`+0x678` and the vector at `+0x198` is now decoded: the scalar is the force
block's thrust magnitude, so storing `0.0` into it removes the thrust force
and nothing else. The claim `t452_reg.limit.tangler_force_scalar_meaning` is
therefore resolved, and the flight model takes the measured cut as
`FlightState::thrust_suppressed`
(`docs/findings/2026-10-29-t452-tangler-flight-effect.md`, rows
`tangler.thrust_scalar` and `tangler.thrust_vector`). The AI path and the
per-actor-update ownership stay named.

