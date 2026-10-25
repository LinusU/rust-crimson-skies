# T2120 — how a guided (`LOCK_ON`) weapon record obtains a projectile target

Follow-up of #2116 (claim `t2116.limit.lock_on_target_source`). That task
found no `[reg+0x70]` store that assigns a target to the eleven
non-`BEEPER_SEEKER` guided records (`wep_04`–`wep_10`, `wep_14`, `wep_15`,
`wep_24`, `wep_27`). This task finds the store it could not see.

Every statement is **static analysis** of the owner-supplied decrypted
executable (`$CS_ENGINE_IMAGE`, base `0x400000`; Capstone linear disassembly
with skip-data, not committed). Nothing is original-run behaviour; claim level
`checked`, never `verified_original`. The byte windows and the closed lists
below are pinned by
`accept_t2120_image_pins_the_launch_time_target_hand_over`
(`crates/cs_content/tests/accept_t2120_lock_on_target_source.rs`).

## The answer

**The launch routine `0x5aef40` itself hands a target over at launch time.**
After it zeroes `+0x70`/`+0x74` (`0x5af610`), the block at `0x5af616` runs:

1. `[weapon+0x74]` bit 15 (`test ch, 0x80`) — the same `LOCK_ON` bit the
   steering guard at `0x5af7f4` tests — must be set, otherwise the block is
   skipped;
2. the global `0xa1d7d0` (a pointer to a three-dword *pending triple*) must be
   non-zero;
3. the global `0xa1d7cc` (a pointer to a count) must be non-zero and
   `[[0xa1d7cc]] > 0`;
4. then `lea edx, [edi + 0x6c]` and three dword copies take
   `[0xa1d7d0][0..3]` to the projectile's `+0x6c`, **`+0x70` and `+0x74`**
   (`0x5af64d`–`0x5af65b`); and `0xa1d7cc` is cleared (`0x5af65e`).

The #2116 scan looked for `89 /r 70` / `c7 /0 70` stores; these stores are
`[edx + 4]` and `[edx + 8]` with `edx = edi + 0x6c`, so it could not match
them. (The same block copies the launch position argument to `+0x30` for such
weapons.)

Consequences:

* Every `LOCK_ON` record gets the triple, not only `wep_11`. The steering
  guard `cmp [esi+0x70], 0` at `0x5af7f9` therefore passes for these rockets
  whenever the pending triple's second word was non-zero at launch.
* `+0x70`/`+0x74` are **pointers** (to a position copy and a second vector),
  matching the pair the `BEEPER_SEEKER` callback stores; `+0x6c` is a first
  word whose meaning is not established.
* `BEEPER_SEEKER` (`wep_11`) re-assigns every step on top of this; the other
  eleven keep what the launch copied, until the removed-object sweep
  (`0x5ac0f1`) clears it.

## Who sets the pending triple

The globals are written only by the setter `0x5ac0a0(count_ptr, triple_ptr)`
(`0x5ac0a8`, `0x5ac0ad`), two resets (`0x5ad522`/`0x5ad527`,
`0x5ae91f`/`0x5ae925`) and the launch's own clear (`0x5af65e`). The setter has
exactly five direct callers and its address occurs nowhere as data.

| call | passes | then |
| --- | --- | --- |
| `0x4ab385` | `esi+0x1ec`, `esi+0x1f0` (a fire context) | launch `0x4ab3cc`, then setter `(0,0)` at `0x4ab40e` |
| `0x4b6ba1` | `&0x62b11c` (the constant `1`), `esi+0x290` (a gun controller's triple) | launch `0x4b6e7b`; setter `(0,0)` at `0x4b6ec3` after the loop |
| `0x4b7193` | `&0x62b11c`, `player+0x290`; `player+0x728 = 1` just before | launch `0x4b7289` |

### Player (`0x4b70d8`–`0x4b7193`)

For a weapon record with `LOCK_ON` and while `player+0x294` is still zero
(no lock held), the fire path computes a free-aim triple and stores it at
`player+0x290` (`0x4b7164`): an offset word, a pointer to a ring copy
(`0x71dbc8`, 20 entries of 12 bytes) of the aim position, and a pointer to
`0x75d1b8` (the global zero vector the launch also uses). It then sends
network message `0x10` (size `0x1c`) through `0x5b2640` carrying the gun index,
`player+0x728`, `player+0x72c` and the 12 bytes at `player+0x294`
(`0x4988e0`). A lock already held in `player+0x294` skips the free-aim block
and is what the message carries.

### Order handler (`0x498950`)

Message `0x10` is registered to `0x498950` (`0x495344`). For a mode
word 1 the handler copies the position into a 200-entry ring (`0x71c870`) and
sets the actor's `+0x294` to the copy and `+0x298` to `0x75d1b8`; for mode 2 it
sets `+0x294`/`+0x298` to the two virtual-call values (slots 0 and 1) of an
object — the same shape `BEEPER_SEEKER`'s callback stores; mode 0 clears both.
It then refreshes the gun controller (`0x4b6820`).

So the guided rockets get a target when the actor's triple is set (a player
lock or free aim, or an order from message `0x10`), and none otherwise.

## Still unknown (recorded, not claimed)

| unknown | claim id | affects |
| --- | --- | --- |
| which game events fill the actor triple (`+0x290`/`+0x294`/`+0x298`, fire context `+0x1ec`/`+0x1f0`) for AI actors, what sends message `0x10` to them, and whether the pair left set by the player path (it is cleared only by a `LOCK_ON` launch or the two resets) reaches the launches at `0x4b7559`, `0x4c01e0`, `0x4c0d59` | `t2120.limit.pending_triple_producers` | `wep_04`–`wep_10`, `wep_14`, `wep_15`, `wep_24`, `wep_27`; F28-B/C/D, F32-A |
| whether an original run steers these rockets as the code suggests | `t2120.limit.guided_flight_original` | the same, and `wep_11`; needs an owner-supplied original run |
| the meaning of the triple's first word and of the two pointers for the steering routine `0x5af960` / `0x538ca0` | `t2116.limit.target_value_meaning` (carried) | the steering law |

Claim `t2116.limit.lock_on_target_source` is **partly resolved**: the source of
the target is the launch-time hand-over above. The contract's "no writer for
the eleven" statement in the #2116 finding is superseded by this one. Claim
level `checked`; no agent review replaces the owner's approval.
