# T2116 — where a projectile's target pointer (+0x70) is first assigned

Follow-up of #454 (claim `t454.limit.target_assignment`, finding
`2026-10-25-f28-guidance-loss-per-weapon-audit.md`): the launch routine
`0x5aef40` zeroes projectile `+0x70` and no store in `0x5ab000..0x5b2000`
assigns a target. This task looks in the whole of `.text`.

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`; a linear disassembly with
Capstone, not committed, cross-checked by a raw byte scan). Nothing is
original-run behaviour; claim level `checked`, never `verified_original`. The
byte windows and the store inventory are pinned by
`accept_t2116_image_pins_the_target_writers`
(`crates/cs_content/tests/accept_t2116_projectile_target_writers.rs`); the
record census by `accept_t2116_only_wep_11_declares_beeper_seeker`.

## The answer

**There is exactly one writer of a non-zero target outside the launch and the
sweep: the `BEEPER_SEEKER` projectile callback `0x441780`.** It is called once
per live projectile per step (`0x5af745`–`0x5af757`, slot `+0x688`), so it
*assigns and re-assigns every step*. No plain `LOCK_ON` rocket has any writer.

| store | what it is |
| --- | --- |
| `0x5af610` | launch tail: `+0x70` and `+0x74` from a zeroed `ebx` (`xor ebx, ebx` at `0x5af581`) |
| `0x5ac0f1` | the removed-object sweep: clears `+0x70` / `+0x74` where they name the removed object |
| `0x4417a7`, `0x4417b2` | callback `0x441780`: the search result's two virtual-call values to `+0x70` / `+0x74` |
| `0x4417b8`, `0x4417bb` | the same callback with no beeper found: zero for both |

### The callback

`0x441830`, called by every launch caller on the new projectile, tests
`dh & 0x80` of the weapon's `+0x210` flag word (`BEEPER_SEEKER`, `0x8000`;
`0x441864`/`0x44186c`) and installs `0x441780` into `+0x688`
(`0x441877`); otherwise the slot is zero. The callback runs the beeper search
`0x4b8b50` on the projectile's position (`+0x48`) and heading (`+0x3c`) and
stores the found beeper's two values at `+0x70` / `+0x74`, or zero for both.
It runs *before* the steering guard of the step (`0x5af7f9`), so within one
step the sweep's clear is overwritten by the next search.

Consequences for the guided records:

* A `BEEPER_SEEKER` projectile's loss is **not terminal**: when its beeper
  expires or is removed the next step picks the best other live beeper, or
  leaves it with no target. The contract's "loss is terminal per item"
  (#454) holds for no projectile that has a writer.
* Only **`wep_11`** of the twelve guided (`LOCK_ON`) records declares
  `BEEPER_SEEKER` (census test). It is also the record with `RANGE` 10000.
* For the other eleven (`wep_04`–`wep_10`, `wep_14`, `wep_15`, `wep_24`,
  `wep_27`) **no store anywhere in `.text` assigns a target**. Their `+0x70`
  stays zero from the launch on, so by the code the steering call and the
  `DETONATION_DISTANCE` proximity branch never run for them. That conflicts
  with their being declared guided and is **not resolved here**: see
  "Still unknown".

## How the search was bounded

1. **Callers.** Exactly six `e8` calls reach `0x5aef40`
   (`0x4ab3cc`, `0x4b6e7b`, `0x4b7289`, `0x4b7559`, `0x4c01e0`, `0x4c0d59`),
   and the address occurs nowhere as a data word, so there is no indirect
   caller. After the call, four callers only test the returned node and pass
   it to `0x441830`; the other two discard it. None stores through it. The
   launch's own arguments (`a8`..`a10`) feed `+0x608`/`+0x60c`/`+0x610`, a
   delay/arming block read by `0x5b0406`..`0x5b052e`, not a target.
2. **Stores.** A raw byte scan of `.text` for `89 /r 70` (mod=01) and
   `c7 /0 70` with a register base other than `esp` finds 65 sites, equal to
   the disassembler's non-stack `[reg+0x70]` stores. Stores through `esp`
   are stack locals and cannot be a heap projectile node. The 65 are pinned
   as a closed list. Every one was read in context: besides the four rows
   above, they are constructors and destructors zeroing other structures
   (`0x4bd4e8`, `0x4bd728`, `0x451892`, `0x453819`, `0x478ab2`, `0x4de8a5`,
   `0x55016a`, `0x470b20`), record-field parses (`0x4a9e9b`, `0x4a9ea3`,
   `0x42fa21`, `0x5ad95e`), list or iterator links (`0x4cc5f2`, `0x4d7e7a`,
   `0x4e2a86`..`0x4e3849`), copies of other structures' words (`0x436d4f`,
   `0x477f79`), handle or call-result slots of other structures (`0x4b1a1b`,
   `0x4b1b51`, `0x520949`, `0x523248`, `0x4eda4c`) and unrelated scalars. None sits on the
   projectile list. A scan for stores indexed by a register
   (`[reg+reg*k+0x70]`) finds only a texture-slot table (`0x4b14ae`..).
3. **List walkers.** No code outside the module loads an actor's projectile
   list head (`[actor+0x88]`) and then touches `+0x70` / `+0x74` of the node
   within forty instructions; inside the module only the sweep does.

## Still unknown (recorded, not claimed)

| unknown | claim id | affects |
| --- | --- | --- |
| how the eleven non-`BEEPER_SEEKER` guided records obtain a target: no direct store exists. Candidates not excluded: a write by data the scan cannot see (a native called with a field offset from the mission script, or a whole-node copy), a flag read that differs from `LOCK_ON` = bit 15 of `+0x74`, or those rockets flying unguided in the original | `t2116.limit.lock_on_target_source` | `wep_04`–`wep_10`, `wep_14`, `wep_15`, `wep_24`, `wep_27`; F28-B/C/D, F32-A |
| what the two values the callback stores are (the beeper object's virtual calls `[0]`, `[+4]`) and how `0x538ca0` turns `+0x70` into a position | `t2116.limit.target_value_meaning` | `wep_11`, the steering law |

Static evidence cannot show that no mission script or native writes the field;
only a run of the original could. Claim level `checked`; no agent review
replaces the owner's approval.
