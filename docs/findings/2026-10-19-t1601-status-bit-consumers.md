# T1601 — the consumers of the SONIC / FLASH / BEEPER / BEEPER_SEEKER bits

Follow-up to task #1318 (`2026-10-18-t1318-blast-unknowns-measured.md`, Q6),
which pinned the stores of the four weapon-record flag bits (`SONIC 0x200`,
`FLASH 0x800`, `BEEPER 0x4000`, `BEEPER_SEEKER 0x8000`; record at
`weapon + 0x210`, flag word at record `+0x00`) and found no code testing them.
Resolves claim `t1318.limit.status_consumers`.

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`). Nothing is original-run
behaviour; claim level `checked`, never `verified_original`. The byte windows
are pinned by `accept_t1601_image_pins_the_status_bit_consumers`
(`crates/cs_content/tests/accept_t1601_status_bit_consumers.rs`).

## Why #1318 missed them

The consumers read the flag word through a loaded pointer and test its
**second byte**: `test ah/ch/dh, imm8` (bits 8–15), or `shr ecx, 0xb`. A scan
for `test/and r, 0x200|0x800|0x4000|0x8000` never matches them. The sites were
found by following every load of `[obj + 0x210]` to the instruction that tests
the loaded word.

## SONIC + FLASH — measured: a per-hit exposure effect (stun/flash)

Per-hit routine (prologue `0x4b9bc0`, test at `0x4b9d84`–`0x4b9d8f`):
`test ch, 0x0a` — **SONIC | FLASH together** — jumps to `0x4ba564`.

At `0x4ba564`:

* `0x4ba576`: `shr ecx, 0xb; and cl, 1` — bit 11, **FLASH**, is passed as the
  first argument of the exposure routine `0x42e840`.
* `0x42e840` returns false at once for a victim whose `+0x91d` byte is set,
  and otherwise computes a falloff from a ratio of two floats (a hit measure
  over the weapon's `+0x3c` value), producing an out-value (a duration) and a
  second out-value. Its exact intensity formula is **not decoded** (unknown,
  below).
* If it returns true:
  * **the victim is the player** (`cmp esi, [0x71c298]`): `0x4ba5cf test ah, 2`
    — **SONIC** selects parameters `(1, 0, 0)` and sound name `snd_stun`
    (`0x62b1d0`); otherwise (FLASH) `(1, 1, 1)` and `snd_nitro` (`0x62b1dc`).
    The sound name is resolved by `0x596120` and, with the duration, the
    parameters go to `0x42e9d0`. (That the triple is a colour is not shown;
    it is recorded as three floats.)
  * **any other victim**: `0x4200d0(victim, duration)`. For a victim that is
    not the player, has `+0x91d == 0`, `+0xf8 == 0` and `+0x67c` in `{0, 4}` it
    sets mode word `+0x358 = 4`, zeroes the control words `+0x100`, `+0x108`,
    `+0x10c`, `+0x114`, `+0x11c`, `+0x120`, and stores *clock + duration* at
    `+0xc0` (`0x420179`). Measured meaning: an AI actor is **incapacitated
    for the duration** — controls zeroed, mode 4 until the timestamp.
* The shot is consumed: the routine then zeroes its two result floats
  (`0x4ba632`–`0x4ba63c`) and returns.

So SONIC and FLASH share one effect path; they differ only in the player-side
parameters/sound, and FLASH is additionally passed into `0x42e840` as a flag.
This is the "choke/stall" status F28 calls non-negotiable, kept separate from
particles (it is a status applied to the victim, not an emitter).

## BEEPER — measured: a timed beacon placed on the hit actor

Two sites test `ch/ah & 0x40` of the flag word: the per-hit routine
(`0x4b9d95`) and `0x49920a` (a second hit path, prologue `0x498170`); a third
(`0x4b9a65`) selects, for BEEPER, `0x499110` instead of the damage routine
`0x498720`.

* `0x4b8ce0(manager 0x71dcc0, shooter, victim)` is the admission gate:
  with both actors present, false when their `+8` words are equal or either
  is zero (a same-side or unaffiliated pair is refused); false when the victim already holds a live beeper (`0x4b8ca0` scans the list
  at `0x71dbac` for an entry on that actor with remaining time > 0), false
  when the manager's stamp (`+8`, written from clock `0x71c470` by `0x4b8d40`)
  equals the current clock (one placement per tick).
* On success a `0x10`-byte beeper object is built by `0x4b88a0(victim,
  TIME)` with **TIME = record `+0x18`** (default 5.0 s from the parser):
  `+0` the victim, `+0xc` the remaining seconds. It registers the object in
  the list `0x71dbac` and stamps the manager.
* The list is aged by `0x4b89c0` (`remaining -= dt`, forced to `-1.0` for an
  actor with `+0x91d`/`+0x91f` set when `0x4b8960` is true) and swept
  at `0x4b8ad0` once the remaining time is no longer positive.
* The beeper shot itself is consumed (`0x4b9ddb`: result floats zeroed).

## BEEPER_SEEKER — measured: homes on live beepers

Projectile setup `0x441864`: `test dh, 0x80` (bit 15) installs the callback
`0x441780` into the projectile's `+0x688` slot (cleared to 0 otherwise,
`0x441852`). The projectile update loop calls that slot once per live
projectile per step (`0x5af745`–`0x5af757`). The callback calls the beeper
search `0x4b8b50(pos, dir)` — which walks the list at `0x71dbac`, skips
beepers with remaining time ≤ 0, and prefers the beeper best aligned with the
projectile's heading and nearest (`0x4b8c09`–`0x4b8c72`) — and stores the
returned object's two vtable values (`[0]`, `[+4]`) at projectile `+0x70` and
`+0x74`. With no live beeper it zeroes both. A second slot `+0x68c` is set to
`0x4417c0` (`0x4419fd`), which unlinks and frees a list entry — a teardown
hook; its caller was not traced.

Measured meaning: a BEEPER_SEEKER missile steers toward whichever live beeper
(placed by a BEEPER shot, lifetime TIME) the search selects; with none it
flies unguided.

## Still unknown (recorded, not claimed)

* `0x42e840`'s exact falloff/duration formula and what `0x42e9d0` renders
  (the screen effect behind the `(1,0,0)`/`(1,1,1)` triple) — affects the
  SONIC/FLASH intensity and the player's visible/audible result.
* The function behind `0x499110` (BEEPER-specific alternative to damage at
  `0x4b9a84`) is not decoded.
* How the `+0x70`/`+0x74` values are consumed by the guidance step (the
  steering law) and the selection weights in `0x4b8b50` beyond the shape above.
* Whether `snd_stun`/`snd_nitro` are the intended sounds or reused names.

No new runtime consumer exists in `cs_sim` yet; a follow-up that implements
these statuses must start from this sheet.
