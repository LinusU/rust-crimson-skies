# T2121 — what projectile `+0x70`/`+0x74` hold and how the steering routine uses them

Follow-up of #2116 / #2120 (claim `t2116.limit.target_value_meaning`). The
`BEEPER_SEEKER` (`wep_11`) callback `0x441780` stores two virtual-call values
of the beeper it found into projectile `+0x70`/`+0x74`; this task establishes
what they are and how `0x538ca0` and the steering routine `0x5af960` consume
them.

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`; Capstone disassembly, not
committed). Nothing is original-run behaviour; claim level `checked`, never
`verified_original`. The byte windows below are pinned by
`accept_t2121_image_pins_the_beeper_target_values_and_their_consumers`
(`crates/cs_content/tests/accept_t2121_beeper_target_values.rs`).

## The answer

**`+0x70` is a pointer to three floats: the target's world position. `+0x74`
is a pointer to three floats: the target's velocity.** Both are read live every
step through the pointer, not copied at launch.

### What the callback stores (`0x441780`)

It calls the beeper search `0x4b8b50` with the projectile's `+0x48` (position)
and `+0x3c`. If a beeper is found, `[esi+0x70]` is the beeper object's virtual
slot 0 result and `[esi+0x74]` its slot 1 result; if none is found both are
zeroed.

The beeper list (`0x71dbac`) entries hold a tracked object at `[entry+0]`. The
search (`0x4b8b88`–`0x4b8bc6`) calls that object's slot 0 and uses the result
as the pointer in `query_point - [ptr][0..3]`: **slot 0 returns a pointer to a
position**. Slot 1 is the next entry of the same table.

Three classes in the image have a vtable whose slots 0 and 1 are
`lea eax, [ecx+N]; ret` (a pointer into the object itself):

| vtable | slot 0 | slot 1 |
| --- | --- | --- |
| `0x608848` | `this+0x74` | `this+0x80` |
| `0x608b48` | `this+0x220` | `this+0x22c` |
| `0x608b88` | `this+0x240` | `this+0x924` |

In the first two the second vector is 12 bytes after the first (two adjacent
`vec3`s). Which of the three classes the beeper tracks is **not** established
(see unknowns).

### How the steering routine consumes them

`0x5af960`, first thing (`0x5af96e`): `0x538ca0(&proj+0x48, [proj+0x70],
&dir)`. `0x538ca0(from, to, out)` computes `out = *to - *from` and normalizes
it (`0x422690`), so `dir` is the **unit vector from the projectile to the
position `[+0x70]` points at**. This is the `+0x70` → position step: a plain
dereference, no transform.

Then, when the weapon record's `+0x74` has bit 16 set (`test eax, 0x10000`), the age
test `[proj+0x668]` against the record's `+0x7c` passes and `[proj+0x74]` is
non-zero (`0x5af9aa`), it calls the solver `0x53e56d(&proj+0x48,
[proj+0x640], [proj+0x70], [proj+0x74], &lead)`:

* `rel = P - T` (projectile minus target position, `0x53e57d`);
* it takes `V` (the `+0x74` vector) dotted with `rel` and `|V|²`, with the
  scalar `[proj+0x640]` squared, and solves a quadratic (`0x53e5dc`–`0x53e6bc`);
* on success `lead = normalize(V - rel * k)` (`0x53e6dc`–`0x53e70d`), a unit
  direction, and returns 1; with no real solution it returns 0.

This is the shape of a constant-speed intercept against a target moving with
velocity `V`, which is why `+0x74` is read as the **target velocity**
(consistent with the free-aim path of #2120, whose second pointer is the zero
vector `0x75d1b8`: a stationary aim point has zero velocity). Whether
`[proj+0x640]` is the projectile speed is inferred from the solver's use, not
established from its store.

If the solver succeeds, `dir` is replaced by `lead` or blended toward it by
`0x538d70` (an interpolation between the two unit vectors built with
`sin`/`cos`, weight `([proj+0x668] - rec+0x7c) * rec+0x84`), becoming `lead` outright once
`[proj+0x668] >= rec+0x80`. The record fields are named by offset only; their
units and meaning are not established.

`+0x6c` (the triple's first word, copied by the launch hand-over of #2120) is
not read by any of the code above; its meaning stays unknown.

## Still unknown (recorded, not claimed)

| unknown | claim id | affects |
| --- | --- | --- |
| which of the three vtable classes the beeper's tracked object is, and so what the slot 1 vector `0x608b88 + 0x924` is for the third class | `t2121.limit.beeper_tracked_class` | `wep_11`; F28, F32-A |
| meaning and units of `[proj+0x640]`, `[proj+0x668]` and the record fields `+0x7c`/`+0x80`/`+0x84` that gate and blend the lead | `t2121.limit.lead_gate_fields` | the steering law of every guided record |
| whether an original run steers as the code suggests | `t2120.limit.guided_flight_original` (carried) | all guided records; needs an owner-supplied original run |

Claim `t2116.limit.target_value_meaning` is **resolved at level checked** for
`+0x70`/`+0x74`. Its `+0x6c` part stays unknown: no code on this path reads it.
No agent review replaces the owner's human approval.
