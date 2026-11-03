# The wingman's pilot gains, limits and rudder tolerance, read (#2266, #2276)

Date: 2026-11-02. Task #2266, follow-up of #2263
(`2026-11-02-m01-wingman-stick-law.md`); §6 answers added by task #2276
(same date, implementer `swe2-max-1`); §7 added by task #2277
(same date, implementer `sonnet-2`). Implementer: `deepseek-1`. Static
claims are `static`, read from the owner image with `llvm-objdump -d -M intel`
and `r2`; retail claims are reads of `$CS_GAME_DIR`. The original executable
was **not** run: nothing here is `verified_original`, and the flight numbers
are port behaviour only. Image sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, address
convention `VA = file offset + 0x400000`.

## 1. What was unknown

#2263 ported `0x41b560`'s stick half and flew it with gain 1 and limit 1,
because the pilot words `vehicle+0x8c4..+0x8d8` were unmeasured. With them the
law looped over the slot and Betty never closed (final gap 4155 m). Three
words were also unknown: the fine-aim gate `def+0x260`, and the spawn copy's
source words `arg1+0xa8..+0xbc`.

## 2. The six words: where they are written and what they mean

### 2.1 The spawn copy (`0x47d1e9`..`0x47d334`)

The loader `0x47c210(def, document, …)` copies each of six words into the new
vehicle, taking the first source when it is not `-1.0` (`0x47d1ef`) and the
second otherwise:

| vehicle slot | axis (from §2.3) | first source | second source |
| --- | --- | --- | --- |
| `+0x8c8` | pitch scale | `arg1+0xa8` (`0x47d1e9`, store `0x47d1fb`) | `def+0x268` (`0x47d209`) |
| `+0x8c4` | roll scale | `arg1+0xac` (`0x47d215`, store `0x47d222`) | `def+0x264` (`0x47d230`) |
| `+0x8cc` | yaw scale | `arg1+0xb0` (`0x47d23c`, store `0x47d249`) | `def+0x26c` (`0x47d257`) |
| `+0x8d4` | pitch limit | `arg1+0xb4` (`0x47d263`, store `0x47d270`) | `def+0x274` (`0x47d2a0`) |
| `+0x8d0` | roll limit | `arg1+0xb8` (`0x47d2ac`, store `0x47d2b9`) | `def+0x270` (`0x47d2e9`) |
| `+0x8d8` | yaw limit | `arg1+0xbc` (`0x47d2f5`, store `0x47d334`) | `def+0x278` (`0x47d32e`) |

`arg1` is a **definition** object: `arg1+0xa4` is the kind the definition
parser writes (`0x47b00f` ship = 3, `0x47b05c` wingman = 4, `0x47b077` plane =
5, …), and the loader's ship/turret branch tests it against 2 and 3
(`0x47c2b4`). The definition constructor (`0x478a00`) initialises the six
words to `2.0`, `0.5`, `0.15`, `100.0`, `100.0`, `0.25`
(`0x478b10`..`0x478b44`).

The three **limit** rows have a third, intermediate source: the loader's third
argument, `arg3+0x38`/`+0x3c`/`+0x40`, taken when `arg3` is non-null and its
word is not below `0.0` (`0x47d278`..`0x47d298`, and the same shape for
`+0x8d0` at `0x47d2c1` and `+0x8d8` at `0x47d306`). M01's spawn path passes
zero there (`0x47ff26`..`0x47ff32`: `push 0; push 0; lea eax, [doc]; push
ebx`), so the definition's limit words apply.

### 2.2 The definition parser (`0x479240`)

The six words are *not* the `ai_input_*` keys. The parser writes:

* `def+0x260` ← key `rudder_tol` (`0x47b37e`, key text `0x6281a4`);
* `def+0xa8` ← key `mode_alt` (`0x47b0a6`, key text `0x6280b8`);
* `def+0xac`/`+0xb0` ← key `collision_d`'s two components (`0x47b0c4`,
  `0x47b0d0`, key text `0x6280c4`).

The `ai_input_*` keys land on a **different** block:

| key (text VA) | slot | constructor default |
| --- | --- | --- |
| `ai_input_scale_pitch` (`0x628220`) | `def+0x268` (`0x47b4bd`) | `3.5` |
| `ai_input_scale_roll` (`0x628238`) | `def+0x264` (`0x47b4db`) | `3.5` |
| `ai_input_scale_yaw` (`0x62824c`) | `def+0x26c` (`0x47b4f9`) | `3.5` |
| `ai_input_limit_pitch` (`0x628260`) | `def+0x274` (`0x47b517`) | `1.0` |
| `ai_input_limit_roll` (`0x628278`) | `def+0x270` (`0x47b535`) | `1.0` |
| `ai_input_limit_yaw` (`0x62828c`) | `def+0x278` (`0x47b553`) | `1.0` |

with the defaults written at `0x478dd6` (`def+0x260` = `0x3e4ccccd`, `0.2`),
`0x478de0`..`0x478dec` (`3.5` from `0x40600000` at `0x478dcb`) and
`0x478df2`..`0x478dfe` (`1.0` from `0x3f800000` at `0x478af5`).

**So the two readings conflict.** The spawn copy's *first* source
(`def+0xa8..+0xbc`) holds `mode_alt`/`collision_d` and the constructor's
`100/100/0.25`; the *fallback* (`def+0x264..+0x278`) holds the words whose
names say "AI input scale/limit". The fallback is only reached when the first
source is still the `-1.0` sentinel, which the definition constructor never
leaves it at; §3 covers which words the retail wingman's definition holds and
what stays unmeasured.

### 2.3 The law's step 7 (`0x41be53`..`0x41bf8e`)

Each word is multiplied by its scale and then clamped to `±limit`, the order
the original applies them in:

| word | scale load | limit load |
| --- | --- | --- |
| `+0x100` (roll) | `+0x8c4` (`0x41be53`) | `+0x8d0` (`0x41bedd`) |
| `+0x108` (pitch) | `+0x8c8` (`0x41be80`) | `+0x8d4` (`0x41bf30`) |
| `+0x10c` (yaw) | `+0x8cc` (`0x41bead`) | `+0x8d8` (`0x41bf83`) |

The global byte `0x64ee4c` (read at `0x41be59`) adds `0.5` (`0x6032e0`) to a
scale and `0.25` (`0x6034f4`) to a limit when set; the port flies it clear.

Step 7 also selects between **two** gain sets with the law's fourth argument
(`bl = [ebp+0x14]`, `0x41ba93`). Zero reads the `+0x8c4/+0x8c8/+0x8cc` scales
and `+0x8d0/+0x8d4/+0x8d8` limits listed above; non-zero reads the
`+0x8dc/+0x8e0/+0x8e4` scales and `+0x8e8/+0x8ec/+0x8f0` limits
(`0x41be47`..`0x41bf89`). The spawn copy fills that second set from
`arg1+0xc0..+0xd8` or, on the sentinel, the definition's `ai_emerg_input_*`
words `def+0x27c..+0x294` (`0x47d344`..`0x47d445`; keys
`ai_emerg_input_scale_pitch` `0x6282a0` … , constructor defaults `3.5`/`1.0` at
`0x478e04`..`0x478e28`). Those defaults are the same `3.5`/`1.0` as the
`ai_input_*` block, so the second set and the fallback reading are numerically
one candidate. The mode-4 follower's station call passes `bl = 0`
(`0x41eed3`, §6.1), so the port flies the correct set — the `bl = 1`
emergency set is only the escape-climb sub-state's.

## 3. What M01's wingman actually gets (retail)

* The `wingman` `vehicle.zrd` record (`ZBD/zrdr.zbd`, member offset 1397861,
  97917 bytes) declares **none** of `rudder_tol`, `ai_input_scale_*`,
  `ai_input_limit_*`, `mode_alt` or `collision_d`. Its `kind_of` chain is
  `wingman` → `devastator` → `basic_airplane`; `basic_airplane` declares
  `ai_input_*` = `3.5`/`1.0`, `ai_emerg_input_*` = `3.5`/`1.0`,
  `mode_alt` = `0.0`, `collision_d` = `[0.6, 0.15]` and no `rudder_tol`.
  `devastator` and `wingman` declare none of them.
* **`kind_of` merges — the merged reading is the measured one** (#2276,
  §6.2): the registry loader builds each `kind_of` child definition as a
  deep copy of its parent's *already-parsed* definition (`0x477b70`) before
  `0x479240` parses the child's own record over it, and the document lists
  the records in that order (`basic_airplane` index 0, `devastator` index
  19, `wingman` index 61). So the `wingman` definition's six words are
  `basic_airplane`'s `mode_alt` `0.0`, `collision_d` `0.6`/`0.15` and the
  constructor's `100.0`/`100.0`/`0.25` limit words — not the constructor's
  `2.0`/`0.5`/`0.15` scale defaults the earlier no-inheritance reading
  assumed.
* M01's `aiv.zrd` (`ZBD/C3/M01/zrdr.zbd`) `wingman_1` record carries 66
  positional fields, and fields **44..49** — the instance document's own
  slots `doc+0xa8..doc+0xbc` (`0x437ca9`..`0x437cf4`, the constructor's `-1.0`
  defaults at `0x437420`..`0x43743e`) — are all `-1.0`. The spawn copy does
  **not** read them: its first source is the *definition*, resolved by name
  from the class registry (`0x47b68b` in the definition path, `0x47fe59` in
  M01's spawn path), and the instance document is only the loader's second
  argument (`0x47ff2a`). So the words come from the definition's
  `+0xa8..+0xbc`; the instance's six slots are a separate, unset block.

The port therefore flies, for M01's wingman (`M01_WINGMAN_PILOT_GAINS`,
`cs_app::mission_ai`):

| axis | scale | limit |
| --- | --- | --- |
| roll | `0.6` | `100.0` |
| pitch | `0.0` | `100.0` |
| yaw | `0.15` | `0.25` |

and `RUDDER_TOL` = `0.2` for `def+0x260`. The inherited `mode_alt` `0.0`
lands on the pitch *scale*, so the law's step 7 zeroes the AI pitch word
outright — the original's wingman genuinely has no pitch authority through
this law. The `ai_input_*` words (`PILOT_GAINS_AI_INPUT_KEYS`, `3.5`/`1.0`)
stay in the crate as the never-taken fallback, and are flown as the second
reading below.

## 4. The fine-aim gate is `rudder_tol` = 0.2

`0x41bc83` compares the aim magnitude with `def+0x260`, which §2.2 shows is
the key `rudder_tol` with the constructor default `0.2`. Because the aim is
normalised to `1.0` whenever the goal is ahead (`0x41bc28`), the fine branch
is only reachable with the goal *behind* and within 0.2 m. #2263's experiment
with the gate at 1.0 is therefore not a reading the retail data supports.

## 5. Hillside re-flight (retail M01, AI blow on, 120 Hz, 45 s)

`accept_m01_wingman_stick_betty_over_the_hillside` (ignored, retail), spawn
gap 118 m, one flight per reading:

| pilot words | closest | worst after 30 s | final |
| --- | --- | --- | --- |
| merged `kind_of` words `0.6/0.0/0.15`, `100/100/0.25` (the measured M01 set, #2276) | 80 m | 662 m | 662 m |
| no-inheritance words `0.5/2.0/0.15`, `100/100/0.25` (superseded reading) | 119 m | 464 m | 464 m |
| `ai_input_*` fallback `3.5/1.0` | 119 m | 4101 m | 4101 m |

The measured set closes to 80 m — closer than the superseded reading's
119 m, because its dead pitch axis no longer oscillates vertically — but it
still drifts to a 662 m worst excursion, and the fallback diverges
steadily. Nothing holds the 19 m station: `original_stick` therefore
**stays off**, and the M01 pins keep flying the designed follower. This
says nothing about the original wingman: the port's force model, the
slot's sign convention and the 80 m push
(`2026-10-18-m01-wingman-player-leader.md` §2–§3) are all still unmeasured,
so the law cannot yet be judged by flying it here.

## 6. The two open items, settled statically (#2276)

Both items §6 of #2266 left open resolve from the image; no original run is
needed for either.

### 6.1 The follower's call site is `0x41eed3`, `bl = 0` — the normal set

The AI step `0x41c270(esi = the AI object)` dispatches on the mode word
`[esi+0x67c]`:

| site | bytes | effect |
| --- | --- | --- |
| `0x41c2dd` | `83 be 7c 06 00 00 04` / `74 72` | `cmp dword [esi+0x67c], 4` / `je 0x41c358` |
| `0x41c358` | `56` / `e8 02 24 00 00` | `push esi` / `call 0x41e760` |

Mode 4 is the follower path; `0x41e760` is its routine (its goal work reads
the leader object `[esi+0x2fc]->[edi+0x204]` — `edi` is compared against
the player-vehicle global `0x71c298` at `0x41e78a` to pick between the two
param tables `0x61fb88`/`0x61fb98`). Inside it the sub-state `[esi+0x358]`
gates first:

* `0x41e7bd`: `mov eax, [esi+0x358]` / `cmp eax, 3` / `jne 0x41e815`. When
  it **is** 3 the routine calls `0x41b560` at **`0x41e7ff`** with
  `push 0` (`0x41e7d6`, arg5) then **`push 1` (`0x41e7d8`, the `bl`
  argument)** — an escape climb whose goal is the aircraft's own position
  plus `1000.0` (`fadd [0x603464]` at `0x41e7e0`). `+0x358 = 3` is latched
  at `0x41f98e` when the `0x4c8f70` probe against a `0x8480`-byte query
  returns a hit, and cleared at `0x41f9a6`.
* `0x41e815`: `cmp eax, 4` / `je 0x41eff9` — sub-state 4 is the stunned
  latch (written at `0x420127` under the "Stunned for %f seconds based on
  stun recovery." log, which also zeroes `+0x100/+0x108/+0x10c`); it
  returns without calling the law.
* Every other value runs the follower phase machine on `[esi+0xd8]`: a
  5-entry table at `0x41f00c` (`0x41e82d`: `jmp [eax*4 + 0x41f00c]`,
  cases `0x41e834`, `0x41ea6c`, `0x41e8bb`, `0x41e9ec`, `0x41ea2d`) which
  recomputes the phase and re-dispatches through a second 5-entry table at
  `0x41f020` (`0x41ea95`: `jmp [eax*4 + 0x41f020]`, cases `0x41ea9c`,
  `0x41ebc9`, `0x41ec5a`, `0x41edd2`, `0x41ed35`). All phase bodies place a
  leader-relative goal in `[esi+0xe0]` and converge on the shared call.

**The station call** (`0x41eebd`..`0x41eed3`):

```
0x41eebd  6a 00                 push 0            ; [ebp+0x18] arg5
0x41eebf  6a 00                 push 0            ; [ebp+0x14] — bl = 0
0x41eec1  68 28 fb 61 00        push 0x61fb28     ; params (0.4/1.5/0.06/0.06/0.15/0.025/0.35/0)
0x41eec6  8d 55 d8              lea edx, [ebp-0x28]
0x41eec9  52                    push edx          ; [ebp+0xc]  leader velocity
0x41eeca  8d 86 e0 00 00 00     lea eax, [esi+0xe0]
0x41eed0  50                    push eax          ; [ebp+0x8]  the station goal
0x41eed1  8b ce                 mov ecx, esi      ; this
0x41eed3  e8 88 c6 ff ff        call 0x41b560
```

So the station-flying follower reaches `0x41eed3` and passes **`bl = 0`** —
the law applies the `vehicle+0x8c4..+0x8d8` words (the spawn copy's first
set). The `bl = 1` `ai_emerg_input_*` set is reached only by the
escape-climb sub-state (`0x41e7ff`) and by the two climb sites inside the
non-follower routines (`0x41d301` in the `0x41d1f0` subtree, `0x41dab7` in
the `0x41d9f0` subtree — both `push 0`/`push 1` with the `0x61fb48` params,
dispatched from `0x41c270` only when `[esi+0x67c] != 4`). The remaining
three sites pass `bl = 0` outside mode 4 (`0x41d4e0`, `0x41e2e4` in the
maneuver legs; `0x42176a` in the `0x4216e0` route leg, which also latches
`+0x358 = 5` at `0x42180f`). The `accept_m01_wingman_callsite_` image test
pins every window above.

### 6.2 `kind_of` merges — at the definition level, before the child's parse

The `vehicle.zrd` registry loader iterates the document's `(name, record)`
pairs (`0x474b93`, `ebx += 0x10` per pair). For each record it looks up
`kind_of` **in the record's own node** (`push "kind_of"` at `0x474ba5`,
text `0x627b24`; `call 0x57a090` at `0x474bbb`):

* absent (`je 0x474c3a` at `0x474bc5`) → the plain constructor `0x478a00`
  (`0x474c66`) fills the new `0x298`-byte definition with its defaults;
* present → the referenced name (`[eax+4]+0xc`) is `_stricmp`-matched
  against the already-registered definitions (`0x71daac..0x71dab0`,
  `0x474bcd`..`0x474bf6`), a new `0x298`-byte object is allocated, and
  **`0x477b70(this, name, parent)` (`0x474c25`) deep-copies the parent's
  parsed definition into it**.

Then `0x479240(this = the definition, edx = the child's record node)` runs
at `0x474c82` and the finished definition is pushed into the registry
(`call 0x45c590` at `0x474c99`, vector `0x71daa8`) — so each `kind_of`
child inherits its parent's *fully parsed* fields, and registration order
is document order.

`0x477b70` is a whole-object copy: the scalars are moved `ebx` (parent) →
`ebp` (child) field by field — `+0xa0`..`+0xc4` at `0x477fd0`..`0x478042`
(including all six spawn words `+0xa8`..`+0xbc` at `0x477fe2`..`0x478024`),
`+0x210`/`+0x214`/`+0x260` at `0x4788c5`..`0x4788e3`, the `+0x220` string
via `std::string::_assign`, `+0x230`..`+0x238` at `0x478905`..`0x478923`,
the nine-dword block `+0x23c`..`+0x25f` by `mov ecx, 9`/`rep movsd` at
`0x478935`/`0x47893a`, `+0x264`..`+0x294` at `0x47893c`..`0x4789d2` (the
`rudder_tol`/`ai_input_*`/`ai_emerg_input_*` block), the flag byte `+0x21c`
at `0x4789d8`/`0x4789df` — and its earlier half deep-copies the parent's
embedded vectors through the element copy helpers (`0x485ba0`, `0x4820f0`,
`0x4849b0`, `0x482aa0`, `0x482a70`).

**For M01's `wingman`:** the document order is topological
(`basic_airplane` index 0 → `devastator` index 19 → `wingman` index 61),
so `wingman`'s definition is `devastator`'s parsed definition — itself a
copy of `basic_airplane`'s — overlaid with `wingman`'s own keys, which name
none of the seven relevant fields. The merged definition holds:

| def word | value | source |
| --- | --- | --- |
| `+0xa8` (`mode_alt` slot) | `0.0` | `basic_airplane`'s `mode_alt` |
| `+0xac`/`+0xb0` (`collision_d`) | `0.6`/`0.15` | `basic_airplane`'s `collision_d` |
| `+0xb4`/`+0xb8`/`+0xbc` | `100.0`/`100.0`/`0.25` | constructor defaults (`0x478b10`..`0x478b44`); no key writes these slots |
| `+0x260` (`rudder_tol`) | `0.2` | constructor default; no `rudder_tol` in the chain |
| `+0x264`..`+0x278` (`ai_input_*`) | `3.5`/`1.0` | `basic_airplane`'s keys |
| `+0x27c`..`+0x294` (`ai_emerg_input_*`) | `3.5`/`1.0` | `basic_airplane`'s keys |

Through the spawn copy's slot map (§2.1) that is scale
`(roll, pitch, yaw) = (0.6, 0.0, 0.15)` and limit `(100.0, 100.0, 0.25)` —
`M01_WINGMAN_PILOT_GAINS` is now this set. The inherited `mode_alt` `0.0`
on the pitch scale means the original's follower has a dead pitch axis on
this law; together with §6.1's `bl = 0` this is the measured set.

| unknown | affects | resolving step |
| --- | --- | --- |
| the ported law's remaining gaps (§5) | whether `original_stick` can ever be turned on | #2262's slot signs, the 80 m push, and the force-model comparison |

Nothing here is `verified_original`; the numbers are static reads and the
flight numbers are port behaviour.

## 7. The station goal, and what holds altitude with a dead pitch word (#2277)

Static reads of `0x41e760`, `0x41b560`, `0x41f810` and the vehicle update at
`0x489a60`; claim id `OWNER-STATIC-2026-10-29`, nothing `verified_original`.
`accept_m01_wingman_station_goal_` (image-gated) re-reads every window below.

### 7.1 The goal `[esi+0xe0]`, per phase

The phase is `[esi+0xd8]`, re-dispatched at `0x41ea95` (`jmp [eax*4+0x41f020]`).
`ebx` is the slot vector: **`0x61fb88` = (6, 0, 18)** when the leader `edi` is
the player vehicle (`cmp edi,[0x71c298]` at `0x41e78a`, `mov ebx,0x61fb88` at
`0x41e792`), else **`0x61fb98` = (8, -2, -8)** (`0x41e7a6`). For the AI
leader `0x41e7b0` also stores phase 2 and `[esi+0xdd] = 1` on every call.

| phase | body | goal written to `[esi+0xe0]` | reference velocity |
| --- | --- | --- | --- |
| 0 | `0x41ea9c` | route object `[esi+0x948]` present and within `0x6035c8` (1800 m squared): route-derived point (`0x41eab8`..`0x41eb91`, vtable calls, not decoded); **otherwise the leader position (`edi+0x204`) with `+200.0` on y** (`0x41eb96`..`0x41ebbe`, `0x6035c4`) | the zero default `0x75d1b8` |
| 1 | `0x41ebc9` | **slot**: `x*row0 + y*row1 + z*row2 + t` (below) | the leader's, `call [edi->vtable+4]` (`0x41ec52`) |
| 2 | `0x41ec5a` | route-derived point (`[esi+0x948]` vtable `+0xc`/`+0x8`, a speed-banded scalar times a vector, added to the route point; `0x41ec5a`..`0x41ed2b`) | the route object's (`0x41ed25`) |
| 3 | `0x41edd2` | **none**: the previous goal is flown | zero default |
| 4 | `0x41ed35` | identical to phase 1 (`0x41ed4d`..`0x41edbe`) | the leader's |

The first table (`0x41f00c`) moves the phase: 0 -> 1 when the leader is awake
(`[edi+0x91d]`), within 700 m (`0x6035e0`) and own speed `[esi+0x934]` >
20.576; 2 -> 1 at once without a route object (`0x41e8d1`), else on the route
distance (`0x41e992`..); 3 -> 4 and 4 -> 1 inside 50 m (`0x6035cc`, 2500
squared) of the held goal.

**Frame.** The slot is rotated by **all nine floats at `leader+0x180`** (the
`lea edx,[edi+0x180]` of `0x41ebe1`/`0x41ed4d`; rows `+0x00`, `+0x0c`,
`+0x18`) and translated by **the triple at `leader+0x1a4`**
(`fadd [ebx+0x24/0x28/0x2c]`, `0x41ec37`..`0x41ec41`): the leader's full
orientation, so the slot banks and pitches with the leader, not a
heading-only frame. This is the shape the port already uses
(`station_point` = `leader_position + leader_orientation * offset`).

**Signs.** The table values are raw, along rows 0/1/2. Row 2 is the component
the control law tests as "ahead" (`bz > 0`, the player-leader finding §4), so
the player slot is **18 m along the nose** (ahead of the player), 6 along
row 0, 0 along row 1, and the AI slot is 8 along row 0, 2 along **minus**
row 1, 8 **against** row 2 (astern). The sign of row 0 (left/right) and of
row 1 (up/down) is not decided by anything read here; the law's own `bz`
convention is the only evidence for row 2, an inference, not a measurement.
The port flies `(6, 0, -18)` / `(8, -2, +8)` (nose is `-Z`, `+x` right, `+y`
up), which is this reading and **agrees on magnitude and on the ahead/astern
split**; it is unproven only on the row 0/row 1 signs.

**The 80 m push** (`0x41edd2`..`0x41eeba`) runs after every phase. It reads
`Δ = own - leader` into `[ebp-0x4c]` (`0x41edde`), tests `0 < |Δ|² < 6400`,
and then scales the vector at **`[ebp-0x40]`** by `80/|Δ|` (`0x41ee74`,
`0x41ee65`: `fdivr [0x6035bc]`) and adds it to the goal. `[ebp-0x40]` is a
different local than `Δ`; the routine stores nothing to it (see
`2026-10-18-m01-wingman-player-leader.md` §3), so the push direction is
stale stack data. This is why the push cannot be ported: it is not
`own - leader`. Still unknown: what the caller chain leaves there.

### 7.2 What moves altitude when the pitch word is dead

1. **The follower routine itself stores no stick or throttle word.** No
   `[reg+0x108/0x10c/0x114/0x11c/0x120/0x124/0x128]` operand exists in
   `0x41e760..0x41f00a`; after the shared call at `0x41eed3` it returns when
   there is no route object (`0x41eed8`, `je 0x41eff9`). The AI step
   `0x41c270` adds only `0x41fe10` (state upkeep; its only stick writes are the
   stunned zeroing at `0x42015a`..`0x420173`) before the follower.
2. **The law's pitch is `0.0` on the station call.** The recovery stores
   `+0x108 = ±1.0` (`0x41be28`/`0x41be34`, speed under `0x603510`), but step 7
   multiplies the word by `+0x8c8` (`0x41be80`/`0x41be97`), which is `0.0`
   for M01's wingman (§6.2). Altitude therefore comes from the bank/yaw
   words, the throttle `+0x124`, and the ground blow (the AI blow of
   `2026-11-02-m01-wingman-hillside-divergence.md`), none of which steers a
   climb to a goal.
3. **The escape climb is the pitch authority.** The vehicle update calls
   `0x41f810` at `0x489abc` each tick for an AI flagged `[esi+0x944]` in modes
   0/1/4 (gates `0x489a65`..`0x489aa2`; `+0x358 != 5`, `+0x91f == 0`, call only
   while `+0x358 <= 3`). It latches `+0x358 = 3` (`0x41f98e`) when the vehicle's
   y is under the world floor `[0x71c3f0]` (`0x41f827`), or, between the
   floor and the ceiling `[0x71c3f4]` (`0x41f83e`), when the probe `0x4c8f70`
   from the position to the position plus **velocity x 4.5** (`0x603644`,
   `0x41f8b5`) hits the world; the latch clears at `0x41f9a6` once nothing is
   hit. While it is 3 the follower flies `0x41e7ff` with the goal "own
   position + 1000.0 up" and `bl = 1`, so the law uses the **emergency**
   words: pitch scale `+0x8e0` (`def+0x280`, `0x47d355`) and limit `+0x8ec`
   (`def+0x28c`, `0x47d3ca`), the `ai_emerg_input_*` block, inherited as
   3.5 / 1.0 (§6.2), not `0.0`. That is the one non-zero pitch path in the
   follower's step, and the only thing in the original that turns a
   terrain-bound wingman up the hillside.

### 7.3 Port gap, and what is left

The port has no terrain look-ahead (`0x41f810`) and no escape climb: with
`original_stick` on it flies the dead pitch word with nothing to replace it,
which matches the 662 m drift of §5. The port was not changed in this task
(the probe needs the world query behind `0x4c8f70`, not a number): the
follow-up is filed as its own task, and `original_stick` stays off.

| unknown | affects | resolving step |
| --- | --- | --- |
| row 0/row 1 signs of the `leader+0x180` basis | which side/height the slot lies on | the pose-to-basis writer, or a capture (#358) |
| what `[ebp-0x40]` holds at `0x41ee80` | the push direction | caller stack trace, or a capture |
| the world query `0x4c8f70` and `[0x71c3f0]`/`[0x71c3f4]` values | when the escape climb latches | decode the query; read the world setup |
| whether M01's wingmen hold a route object `[esi+0x948]` | phases 0/2 goals | already open in §5 |


### 7.4 The escape-climb latch is ported (task #2280)

`MissionAircraftAi::escape` (`EscapeClimbLatch`) ports `0x41f810` with the
terrain probe injected (`TerrainProbe`, `MissionTerrainProbe`; default: never
hits). Re-read from the image:

- Order: `y < [0x71c3f0]` latches at once; else `y >= [0x71c3f4]` clears; else,
  only when the clock `[0x71c470] >= [esi+0xf0]`, it re-arms
  `+0xf0 = (rand * 0x603598 + 1.0) * 0.5 + clock` and probes
  `pos -> pos + velocity * 4.5`; a hit sets `+0x358 = 3`, no hit clears it.
- **Correction to §7.2/the task text**: `[0x71c470]` is an accumulated
  clock, not the step `dt` (`0x4897cb`..`0x4897e4`: `[0x71c470] += [0x9ad744]`),
  and the caller seeds `+0xf0` with it when `+0x944` was just raised
  (`0x489aab`), so the first call probes at once and the re-arm is 0.5-1.0 s.
- The probe `0x4c8f70` reports a hit as a **zero return with a non-zero out
  byte** (`0x41f95b`..`0x41f96b`); the injected trait is the boolean hit.
- Latched, the port flies goal "own position + 1000 up" with
  `PILOT_GAINS_AI_INPUT_KEYS` (3.5 / 1.0, the `bl = 1` words).
- `rand()` is a deterministic stand-in (not the CRT sequence): only the
  0.5-1.0 s re-arm spread is original. The floor 20 m / ceiling 7000 m are
  runtime globals (zero in the image) taken from the task's reading of the
  world setup; they are not re-read from the image.

`original_stick` stays off and the default M01 follower is unchanged (the
default probe never hits). Still needed for the hillside re-fly (#2281): the
decoded `0x4c8f70` wired to the runtime terrain, and the world setup that
writes `[0x71c3f0]`/`[0x71c3f4]`.

Nothing here is `verified_original`.

### 7.5 The look-ahead is wired to the runtime colliders (task #2281)

`WorldSegmentProbe` (`playtest/world_sweep.rs`) answers the latch's probe
from the port's colliders: the same population and face rule as the
ground-blow sweep (`target_of`, `nearest_contact`; scenery, sea tiles, other
aircraft, never the caster). `MissionTerrainProbe` stays the injection point
and wins when set; unset, the decision system uses the world probe, which is
only reached when `original_stick` is on, so the default M01 follower, the
pins and the owner-command smoke are unchanged.

**This is an approximation, not the decoded query.** `0x4c8f70` walks a cell
grid whose builder is undecoded and tests node bounds
(`2026-11-02-ai-terrain-query-4c8f70.md` §8); the port tests the segment
against collider faces. The 32-record cap, stop-at-first mode and the cell
walk are not reproduced. `[0x71c3f0]`/`[0x71c3f4]` are still the task's
20 m / 7000 m, not image-read.

Re-fly of `accept_m01_wingman_stick_betty_over_the_hillside` (retail M01,
45 s at 120 Hz, `original_stick` on, port behaviour only, not
`verified_original`):

| gains | closest | worst after 30 s | final | latched |
| --- | --- | --- | --- | --- |
| merged (0.6 / 0.0 / 0.15, 100 / 100 / 0.25) | 80 m | 688 m | 688 m | 741 / 5400 ticks |
| `ai_input_*` (3.5 / 1.0) | 119 m | 4102 m | 4102 m | 4646 / 5400 ticks |

The merged reading was a 662 m drift without the probe; with the world probe
it is 688 m, so the escape climb as probed against the port's colliders does
not recover the station. The fallback reading is latched for most of the run
and leaves the player behind (4102 m). Neither holds station, so
`original_stick` stays off. What is still open: the grid contents for M01
(whether the hillside is in the original's grid), the floor/ceiling globals,
and the stick's `+0x358 == 3` interplay with the follower's phases.

Nothing here is `verified_original`.

### 7.6 Integration check of the terrain look-ahead (task #2278)

The parent of #2279-#2281 re-ran the pieces together on retail M01
(`accept_m01_wingman_stick`, `accept_m01_escape` and `accept_m01_ai_terrain`
tests with `--include-ignored`, 13 passed; the hillside re-fly alone takes
about 445 s). Results are the §7.5 table: closest 80 m / worst 688 m / final
688 m for the merged gains, 119 m / 4102 m / 4102 m for `ai_input_*`.

Acceptance read against the task text: the latch and escape goal are ported
behind `original_stick` (default off), the default M01 follower, pins and
smoke are unchanged, and the re-fly is recorded. The re-fly does **not**
recover the station, so `original_stick` stays off. Open, unchanged from §7.5:
the cell-grid contents for M01, the image-read floor/ceiling globals and the
`+0x358 == 3` interplay with the follower's phases.

Nothing here is `verified_original`.

## 8. The "route object" is the hostile target handle, and the phase an M01 wingman flies (task #2289)

Static reads of the owner image (`llvm-objdump -d -M intel`, `r2`) and
retail `vehicle.zrd`; claim id `OWNER-STATIC-2026-10-29`, nothing
`verified_original`. `accept_m01_wingman_route_phase_` (image/retail-gated)
re-reads every window below. Implementer `sonnet-1`.

### 8.1 What `[esi+0x948]` is, and where it comes from

**Correction to §7.1/§7.3: it is not a route.** `[esi+0x948]` is the AI's
current **target handle**, a 0x14-byte reference object (`[+4]` the target
entity reference, `[+8]` a use count, vtable slot `+0x48` releases it). The
phase bodies' "route-derived point" (phases 0 and 2) is the target's point,
and the code after the station call (`0x41eed8`..`0x41eff9`, aim and lead
through `0x460be0`) is the shooting solution against it — which is why the
routine returns right after the station call without one.

| step | window | effect |
| --- | --- | --- |
| constructor | `xor ebx,ebx` `0x4affaf`; stores `0x4b039f` (`+0x948`), `0x4b03b6` (`+0x94c`), `0x4b05a3` (`+0xd8`), `0x4b08bf` (`+0x358`), `0x4b00e8` (`+0x2fc`), `0x4b05a9`/`0x4b05af` (`+0xdc`/`+0xdd`) | handle null, timer 0, phase 0, sub-state 0 at spawn |
| per-tick upkeep | AI step `0x41c270`: `cmp byte [esi+0xcc],0` / `jne 0x41c361` (`0x41c273`..`0x41c27b`), then `call 0x41fe10` at `0x41c281` (its only caller) | runs every AI step unless `+0xcc` (the parked-on-a-path byte) is set; returns at once while the global byte `[0x64f66e]` is set (`0x41fe1e`, `0x41fe35`; written at `0x43db4d` from the result of `0x5b7910`, cleared at `0x464693`/`0x4654fa`, not decoded further) |
| scorer | mode 0/4 → local with vtable `0x603544` (slot 0 `0x421ad0`), every other mode `0x60354c` (`0x421950`) (`0x41fe75`..`0x41fe8f`); built by `0x422850`/`0x421e00` with the AI's team (`lea eax,[esi+8]`, `0x41fe3f`) at scorer `+0x10` | candidate cost |
| follower path | mode 4 skips the leader branch (`cmp eax,4 / je 0x41ff8e`, `0x41feb0`); a null handle (`0x41ff8e`) or an expired timer `[esi+0x94c] <= [0x71c470]` (`0x41ff98`) calls `0x41f9c0(scorer, [esi+0x344], 0x420070(esi,1))` at `0x420014` and stores the result at `0x420031` | rebuild |
| re-arm | `0x4b0f20`: `+0x94c = [0x71c470] + [0x6035e4]` (20.0 s) | at `0x41ff60` and `0x42003b` |

`0x41f9c0` walks four candidate sets — the vehicle list `[0x71dabc]`, the list
`[0x71d914]`, the vector `[0x71d33c]..[0x71d340]` and the list `[0x64f78c]` —
scores each with the scorer (`0x422890` first asks the candidate's virtual
`+0x34` with the scorer's `+0x10`, the AI's **team**: a hostility test; an
invalid candidate scores `1e21`, `[0x6034bc]`), adds the AI's bias
(`+0x340` to vehicles, `+0x344` to structures) and keeps the lowest score under
`1e20` (`[0x6034c0]`); it returns a new handle, or null when nothing
qualifies. The scorer `0x421ad0` is a range gate (horizontal squared distance
against `+0x328`, y against `+0x32c`/`+0x330`) followed by a priority
weighting (+ a bonus when the candidate is a mode-4 follower, `0x421c26`;
a bonus when the candidate is the player vehicle, `0x421bea`; angular terms
below `0x421c3b`, not decoded).

**Retail data.** Those AI fields are *definition* words, copied by the loader
(`0x475a5f`..`0x475c8e`, `ebp` = the definition): `def+0xa4` → mode `+0x67c`,
`def+0x44/+0x48/+0x4c` → `+0x328/+0x32c/+0x330`, `def+0x138/+0x13c` →
`+0x340/+0x344`. The parser writes them from the keys `attack`
(`0x627e30`, `0x479d11`..`0x479d49`), `target_bias` (`0x627e7c`,
`0x479e56`) and `struct_bias` (`0x627e88`, `0x479e74`); constructor defaults
`160000`/`-400`/`400` and `0`/`0` (`0x478a69`..`0x478a77`). In retail
`ZBD/zrdr.zbd` `vehicle.zrd`: `basic_airplane` has `attack = [2000.0]`
(→ band ±2000 m, squared range 4,000,000), `wingman` has
`target_bias = [-100.0]` and `struct_bias = [-200.0]` (negative = preferred),
`devastator` none, and no record in the chain declares `pursuit_range`.
`wingman_1` (aiv.zrd member of `ZBD/C3/M01/zrdr.zbd`) carries none of these:
they are not mission data. So **no M01 wingman is given a handle by the
retail mission data**; it holds one exactly while a hostile candidate sits in
its 2000 m band and scores under `1e20`, re-evaluated every 20 s.

What static reading cannot decide: whether a hostile candidate is in that
band during the first 45 s, which depends on the hostile roster and
positions at run time (team ids, `0x41ae40`'s cost, the `0x71d914`/`0x64f78c`
populations) and on the `[0x64f66e]` gate. The port has no hostile search, so
its hillside re-fly says nothing about it either way.

### 8.2 The phase word `[esi+0xd8]`

Initial value **0** (`0x4b05a3`). Inside the AI code (`0x41c270..0x4222ff`,
scanned for every `[esi+0xd8]` store) the only stores are:

| site | stores | condition |
| --- | --- | --- |
| `0x41e7b0` | 2 (`ecx`) and `+0xdd = 1` | the leader is not the player vehicle (`0x41e78a`..`0x41e7a6`), every call |
| `0x41e8d1` | 1 | phase 2, no target |
| `0x41e9f6` | 2 (`ecx`, set by `mov ecx,2` at `0x41e79a`/`0x41e7a1`) | phase 3 with a target |
| `0x41ea21` | 4 | phase 3, no target, within 2500 (`0x6035cc`) of the held goal |
| `0x41ea37` | 2 (`ecx`) | phase 4 with a target |
| `0x41ea62` | 1 | phase 4 within 2500 of the goal; phase 2 with a target farther than the range test (`0x41e9dc`..`0x41e9ea`); phase 0 once leader awake, gap² < 490000, own speed > 20.576 |

(Other `[reg+0xd8]` stores in the image sit in other classes: `0x402b72`,
`0x4578bd` (a HUD-style reader of `[obj+0x948]`), `0x4c0b09`/`0x4c0c12`,
`0x59be3a`, `0x5b58f4`/`0x5b5970`, `0x4d21e1`, `0x55025c` and the aiv
document copies at `0x436e7e`..`0x439053`; `0x4679cf`/`0x468e1d` touch no
vehicle field within ±120 lines and were not classified further.)

**Phase 3 is stored nowhere**, so 3 and 4 are unreachable from the
constructor's 0; phase 1 has no exit in the first table (`0x41f010` is the
join).

**What an M01 wingman flies.** Betty's leader resolves to the player vehicle
(`primary target` absent → the literal `player`, finding
`2026-10-15-m01-aiv-layout.md` §2 field 6), so the forced-2 store does not
run and the table is the player-leader one:

* tick 1: phase 0. Its body (`0x41ea9c`) without a target flies the stick law
  at **leader position + 200 m on y** (`0x41eb96`..`0x41ebbe`); with a target
  within 1800² (`0x6035c8`) the target's point.
* 0 → 1 on the first call where the player is awake (`[edi+0x91d] == 0`),
  the squared gap is under 490000 (700 m) and own speed `[esi+0x934]` exceeds
  20.576 m/s. The spawn gap is 118 m. In the port the speed gate opens at tick
  58 (decision 60, about half a second).
* then phase 1 (the slot, §7.1) for the rest of the 45 s: nothing leaves it.

So the **phase sequence over 45 s is 0 → 1 inside the first second, then 1**,
provided the player stays awake and inside the table's conditions; phases 2,
3 and 4 are not reached by a player-led wingman. The runtime state the
sequence depends on is the speed at tick 1 (the port's value; the original's
is the vehicle's launch velocity, not read here), the player's asleep flag
`+0x91d` and nothing about targets once in phase 1.

### 8.3 The port's `original_stick` branch against this

* Phase 1 (station): the same slot, frame and 20.576/700 m gate as the port
  (§7.1) — no difference found beyond the unproven row 0/row 1 signs.
* **Gap found and corrected:** before the engage gate the port held course
  with the designed law even with `original_stick` on; the original flies the
  stick law at leader + 200 m on y. The port now does so behind
  `original_stick` (`PHASE0_LEADER_CLIMB_M`, default off, so the default M01
  follower, its pins and the smoke are unchanged). It affects the first
  ~0.5 s only.
* The port has no target handle (no hostile search), so phases 0/2 with a
  target and the shooting solution after the station call are not modelled.
  Phase 2 is never reached by a player-led wingman, so this changes only
  phase 0's goal, and only while a hostile is inside the band.

Re-fly of `accept_m01_wingman_stick_betty_over_the_hillside` with that change
(retail M01, 45 s at 120 Hz, `original_stick` on, port behaviour only):

| gains | closest | worst after 30 s | final | latched |
| --- | --- | --- | --- | --- |
| merged (0.6 / 0.0 / 0.15, 100 / 100 / 0.25) | 84 m | 577 m | 577 m | 193 / 5400 ticks |
| `ai_input_*` (3.5 / 1.0) | 119 m | 4131 m | 4131 m | 4687 / 5400 ticks |

Before the change: 80 / 688 / 688 and 741 latched (§7.5). The drift moves but
the station (19 m) is not held, so **`original_stick` stays off**; with a
chaotic dependence (the latch count changed 741 → 193 from a half-second
change) the numbers are not evidence of convergence either way. The largest
remaining unknowns are therefore not the phase: the pitch-dead law's
authority, the slot signs, the 80 m push direction and the force model.

| unknown | affects | resolving step |
| --- | --- | --- |
| hostile roster/positions in M01's first 45 s | whether a target handle exists (phase 0 goal; shooting) | the M01 spawn population and `0x41ae40` / `0x421ad0` angular terms decoded, or a capture (#358) |
| `[0x64f66e]` | whether the upkeep runs at all | decode `0x5b7910` and its caller `0x43db4d` |
| launch speed of Betty at tick 1 | how long phase 0 lasts | the vehicle's launch velocity setup |
| `0x4679cf`/`0x468e1d` stores | completeness of the writer list | classify the two functions |

The last two rows are resolved in §9: neither store writes the phase word (they
write an objective record), so the writer list of §8.2 is complete, and
`[0x64f66e]` is the engine's debug `suspend` latch, which no M01 data sets.

Nothing here is `verified_original`.

## 9. The last two `[+0xd8]` stores are the objectives loader, and `[0x64f66e]` is the engine's `suspend` latch (task #2294)

Static reads of the owner image (`llvm-objdump -d -M intel`) and retail
`ZBD/C3/M01/zrdr.zbd`; claim id `OWNER-STATIC-2026-10-29`, nothing
`verified_original`. `accept_m01_wingman_phase_writers_` (image/retail-gated)
re-reads every window below. Implementer `deepseek-1`.

§8 left two `[+0xd8]` stores unclassified and the `[0x64f66e]` gate open. Both
resolve from the image; **neither changes the phase sequence of §8.2**, and the
port is unchanged.

### 9.1 `0x4679cf` and `0x468e1d` write an objective record's `hide_obj`, not the phase word

The enclosing function is `0x466b70`..`0x469063` — one member (`ecx` = `this`,
SEH frame, `ret 4`) of `mission.cpp`: `push -0x1; push 0x5ff49f; … ; sub
esp,0xec; push ebx; push ebp; push esi; mov ebx,ecx` (`0x466b70`), the document
opened **by name** from the stack argument (`mov esi,[esp+0x10c]` at
`0x466bee`, `call 0x579c60` at `0x466bf8`), failure logging
`"Failed to read %s"` (`0x625ee4` pushed at `0x466c0b`, `mission.cpp` text
`0x626128` at `0x466c15`), and every key it parses is an objective directive
(`TICK_DEPENDS_ON_OBJ` `0x6260cc` pushed `0x4679e1`, `WAKE_OBJECTIVE_WHEN_I_SLEEP`
`0x6269c0` at `0x468dc5`, `HIDE_OBJ` `0x6269dc` at `0x468dfd`, `TIMER_ADJUST`
`0x6269e8` at `0x468e66`, …). Both callers pass the name:
`"objectives.zrd"` (`0x625cf8`) at `0x4647ba`/`call 0x4647c4` (inside the
mission-data loader `0x463f40`, which logs `"Load Mission Sounds"` at
`0x4646f3` and `"Finish LoadMissionData"` at `0x464951`, and which the function
logging `"Begin CZMission::Load"` calls at `0x4653b0`) and `"objectives.zrd"`
(`0x625e2c`) at `0x46557b`/`call 0x465582` (the mission re-start `0x4654e0`,
which also pushes `NEW_GAME_START` at `0x46564d`).

Each numbered block is one record in the array `[this+0xc4c]`:

| window | effect |
| --- | --- |
| `0x467956` `mov ecx,[edi+0xc4c]` | the array base |
| `0x46795c`..`0x467967` `lea edx,[eax+2*eax]; shl edx,4; sub edx,eax; lea eax,[eax+8*edx]; shl eax,2` | byte stride `0x5e4` (377·4) |
| `0x46796c` `call [0xa20338]` / `0x467972` `mov [edi+0xc4c],eax` | `realloc(base, 0x5e4·n)`, new base stored |
| `0x46798e` `mov ecx,0x179; mov edi,ebp; rep stosd` | the record zeroed (377 dwords = `0x5e4`) |
| `0x467983` `lea ebp,[eax+4*ecx]` | `ebp` = record base (`ecx` = 377·index) |
| `0x469043` `mov [eax+0xc48],edx` | the block count |

That record layout is the one `2026-10-09-m02-b-fu3-out-of-range-wake-address.md`
already measured (one `0x5e4`-byte record per numbered block, allocated at
`0x467956`), and `2026-10-06-m01-lc-directive-a-objective-directive-parser.md`
already names `+0xd8` `HIDE_OBJ` and `+0xdc`
`WAKE_OBJECTIVE_WHEN_I_SLEEP`. The two contested stores are therefore not
writers of any vehicle field:

| site | bytes | writes |
| --- | --- | --- |
| `0x4679cf` | `89 9d d8 00 00 00` `mov [ebp+0xd8], ebx` with `ebx = -1` (`83 cb ff` at `0x467953`) | the per-record default `hide_obj = -1`, inside the initialiser that also sets `+0x558 = 1` (`0x467997`) and `+0x580 = -1` (`0x4679db`) |
| `0x468e1d` | `89 85 d8 00 00 00` `mov [ebp+0xd8], eax`, `eax = [edx+0xc] - 1` (`0x468e19`/`0x468e1c`) | the `HIDE_OBJ` key's value minus one, in the `HIDE_OBJ` handler |
| `0x468e25` | `89 bd d8 00 00 00` `mov [ebp+0xd8], edi` (`edi = 0`) | the same key's **absent** branch |

The `-1` default is overwritten on the straight-line path (the `HIDE_OBJ`
lookup always runs and takes one of the two branches above). The store at
`0x468e03` (`89 bc b5 d8 00 00 00`) is **not** a `+0xd8` write: it is
`mov [ebp + 4·esi + 0xd8], edi`, where `esi` is the *1-based* list index (set
to 1 at `0x468dcb`; the loop at `0x468dea` writes item `i` at
`ebp + 0xd8 + 4·i`, i.e. from `+0xdc`, for `i = 1..n-1`). After the loop
`esi = n`, so the store lands at `+0xdc + 4·(n-1)` — the `0` terminator of the
`+0xdc` sleep-wake list, never a plain `+0xd8` field and never `[esi+0xd8]`.

**Completeness.** A scan of `.text` for every `+0xd8` dword store form (`89 /r`
and `c7 /0` with a `disp32` of `0xd8`, any base register, SIB byte or not)
finds 32 sites. Three are stack slots (`89 84 24 d8 …`, `[esp+0xd8]`, at
`0x45ab48`, `0x4793d8`, `0x5720a9`) and one is the `+0xdc` list terminator above
(`0x468e03`), leaving **28 object-field stores**; those inside the loader are
exactly the three above, all `[ebp+…]`, and those inside the AI code
`0x41c270..0x4222ff` are exactly §8.2's six `[esi+…]` phase-word stores (plus
the constructor's `0x4b05a3` elsewhere). The loader contains **no** `[esi+0xd8]`
store, and the AI window contains no store through a register other than `esi`.
The record is `0x5e4` bytes and so has no `+0x948`/`+0x94c`/`+0x934`, while the
object whose phase machine reads `+0xd8` has all three: the record cannot be
that object.

### 9.2 `[0x64f66e]` is the debug `suspend` latch — four writers, 28 readers, no phase reader

A scan of `.text` for the four immediate bytes finds 32 references: **4
writers** and 28 readers. This **corrects** §8's list, which named three
writers and missed the automatic one.

| site | bytes | effect |
| --- | --- | --- |
| `0x43db4d` | `a2 6e f6 64 00` `mov [0x64f66e], al` | the debug console command `suspend <bool>`: the dispatcher `0x43d640` (entered from the `execute(text)` wrapper `0x45cdf0`, `call 0x43d640` at `0x45cdf5`) matches the 8-byte key `"suspend"` (`0x622c7c`, between `gbc` `0x622c84`, `exit` `0x622c88`, `debug` `0x622c90` and the cheats `gimme` `0x622cf8`), reads the boolean with `0x5b7910(this,0)` (`0x43db46`), stores it (`0x43db4d`), and on **false** resets the deadline `[0x622bb8] = 1e9` (`0x43db5a`) |
| `0x4a0252` | `c6 05 6e f6 64 00 01` `mov [0x64f66e], 1` | automatic: the per-frame routine `0x4a0220` sets it when the counter has reached the deadline — `mov eax,[0x9be6f8]` (`0x4a023e`), `cmp eax,[0x622bb8]` (`0x4a0247`), `jb` past (`0x4a0250`) |
| `0x464693` | `c6 05 6e f6 64 00 00` | the mission load clears it and re-arms `[0x622bb8] = [0x622bb4] + [0x9be6f8]` (`0x464680`..`0x46469a`): the small function `0x464680`, which the caller logging `"Begin CZMission::Load"` (`0x465395`) invokes at `0x4653be`, and which also sets `[mission+0x6fc] = 1` at `0x4646a1` |
| `0x4654fa` | `c6 05 6e f6 64 00 00` | the mission (re)start `0x4654e0` clears it, resets the sim clock `[0x71c470] = 0` (`0x4654f0`) and re-arms the same way |

`[0x622bb4]` is the command line's `-suspend <n>` argument (`-suspend` text
`0x62991c` compared in the option parser at `0x4a76c3`, stored at `0x4a76ee`);
`[0x9be6f8]` is a counter incremented by one in `0x5a8550` (`inc eax` at
`0x5a857b`). **Both globals start at `0x3b9aca00` (1e9)** — the "never"
sentinel. An ordinary launch passes no `-suspend`, so the latch stays 0 for the
whole session; `0x4a0252` only fires if the switch armed a deadline that
elapses.

The 28 readers, by region: six in the AI code (`0x41d9f9`, `0x41db73`,
`0x41dec4`, `0x41e674`, `0x41f060`, `0x41fe1e`), eleven in the aircraft /
AI-object tick region (`0x489ac4`, `0x48a12f`, `0x48b4ae`, `0x48b530`,
`0x48b5b4`, `0x48c3fa`, `0x48d2db`, `0x48e632`, `0x48e6df`, `0x48e7e3`,
`0x48ea74`), one at `0x452684`, and ten in the `0x4aad40`..`0x4bf716` region.
The six AI readers each skip a **timer-driven** agent transition (the sleeps,
re-arms and stun latches) and never the phase machine; the twenty-two outside
the AI code are clock- and state-gated updates in other routines (sampled here,
not decoded exhaustively) and none lies in the follower routine. The two that
bear on the follower:

* `0x41fe1e`/`0x41fe35` — the whole upkeep `0x41fe10` returns before the scorer
  and before `0x41f9c0`, so the target handle `+0x948` is **not** rebuilt while
  suspended;
* `0x489ac4`/`0x489acb` — the vehicle update skips the stun-recovery clear
  `+0x358 = 0` (the `[esi+0xc0]` timer).

**In the phase 0 -> 1 transition, the latch gates nothing.** No reader lies in
`0x41e760..0x41f00a`: the follower routine, its two 5-entry phase tables and the
0/2/4 -> 1 join (`0x41e834`..`0x41ea62`) run while suspended, and the transition's
three gates are read directly (leader awake `[edi+0x91d]` at `0x41e834`, gap²
against `490000` `0x6035e0`, own speed against `20.576` `0x603594` at
`0x41e869`). What changes while the latch is set is phase 0's *goal* (no rebuilt
handle ⇒ no target ⇒ leader + 200 m) and the shooting solution after the
station call.

### 9.3 What M01 reaches in its first 45 s

* **The two stores run before the first tick**, because `objectives.zrd` is
  loaded by `0x4647c4` at mission load, and the loader writes `+0xd8` for every
  numbered block. They touch objective records only; the phase word is
  untouched (§9.1).
* **M01 declares neither key.** `ZBD/C3/M01/zrdr.zbd`'s `objectives.zrd` has 39
  `OBJECTIVE<n>` blocks (the count the win-route census uses) and **no
  `HIDE_OBJ` and no `WAKE_OBJECTIVE_WHEN_I_SLEEP`** (`TICK_DEPENDS_ON_OBJ`, 2,
  and `NAP_OBJECTIVE_WHEN_I_COMPLETE`, 12, are present as positive controls).
  So every M01 block takes the absent branches: `hide_obj` ends at 0 (the
  `0x468e25` store, after the `-1` default) and the sleep-wake list at `+0xdc`
  is empty. What a stored 0 means to the wake walk is not decided here and does
  not matter for M01, which never reaches the key.
* **M01's data cannot set `[0x64f66e]`.** Its four writers are the console
  command, the `-suspend` deadline and the two mission-lifecycle clears; none
  is in the objectives loader or the AI code. The exact runtime state that would
  set it is a debug/cheat input: the `-suspend <n>` command-line switch whose
  counter expires, or `suspend on` in the console. With neither, M01's first
  45 s runs with the latch 0, the upkeep runs every AI step, and §8.2's sequence
  (phase 0 -> 1 inside the first second, then 1) stands.

Nothing here is `verified_original`.

| unknown | affects | resolving step |
| --- | --- | --- |
| the unit of `[0x9be6f8]` (a per-call `inc` in `0x5a8550`, not the float clock `[0x9ad744]`) | how long the debug `-suspend <n>` switch delays the freeze; no product behaviour depends on it | the caller chain of `0x5a8550` (fixed step vs frame) |
| what a stored `hide_obj` of `0` means to the objective wake walk | the semantics of the absent-`HIDE_OBJ` branch; **no M01 block reaches it** (it declares no `HIDE_OBJ`) | the walk's `+0xd8` consumer, owned by the objective-executor line `M02-B-FU4` (#807) |

## 10. The hillside drift is a dead pitch axis: vertical error first, then the escape climb's overshoot (task #2295)

Port behaviour plus static reads of the owner image; claim id
`OWNER-STATIC-2026-10-29`, nothing `verified_original`. Trace by
`accept_m01_wingman_drift_trace_hillside` (`CS_DRIFT_TRACE=1 cargo test -p
cs_app --test cs_app_tests_a -- accept_m01_wingman_drift_trace --include-ignored
--nocapture`; retail M01, 120 Hz, `original_stick` on, merged gains, row every
0.5 s over 45 s; raw rows not committed). The slot is
`station_point(player, STATION_OFFSET_PLAYER_M)`; *along* is the slot error on
the leader's velocity direction, *vert* is `slot.y - Betty.y`, *cross* the
remaining horizontal part (unsigned). Implementer `sonnet-2`.

### 10.1 Trace summary

| t (s) | along | cross | vert | gap | Betty / leader m/s | roll | pitch | thr | latch |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0.5 | 158 | 38 | 2 | 143 | 22 / 69 | +0.60 | 0 | 0.76 | – |
| 3 | 208 | 32 | 19 | 193 | 101 / 97 | −0.33 | 0 | 1.00 | – |
| 10 | 132 | 30 | 50 | 129 | 113 / 100 | +0.10 | 0 | 1.00 | – |
| 17 | 51 | 60 | 73 | 97 | 113 / 100 | 0 | 0 | 1.00 | – |
| 23 | 19 | 36 | 86 | 95 | 106 / 100 | −0.12 | 0 | 0.86 | – |
| 28 | 3 | 128 | 90 | 162 | 105 / 100 | 0 | 0 | 0.79 | – |
| 34.5 | 2 | 2 | 118 | 119 | 104 / 100 | +1.00 (sat) | 0 | 0.83 | – |
| 35.5 | 7 | 44 | 119 | 126 | 105 / 100 | −0.02 | +1.00 (sat) | 0.89 | climb |
| 37 | 44 | 106 | 84 | 134 | 77 / 100 | +0.31 | 0 | 1.00 | – |
| 40 | 206 | 195 | −74 | 277 | 80 / 98 | +0.09 | 0 | 1.00 | – |
| 45 | 355 | 369 | −295 | 577 | 93 / 96 | 0 | 0 | 1.00 | – |

Yaw is 0.00 on every row (the yaw word needs a fine-aim goal, §4). The
phase is 1 (`follow_engaged`) from the first row.

### 10.2 Which error dominates, and when

* **0..~20 s, along-track: a launch transient, not a fault.** Betty spawns at
  22 m/s behind a player already at 69 m/s; the throttle target is pegged at
  1.0 by 1.5 s and she cruises at 113 m/s against the leader's 100, so the
  158..208 m along-track error closes at ~12 m/s and is gone by ~28 s. The
  throttle path is not the drift's cause: by 23 s the speed deficit is zero
  and the throttle sits at 0.8..0.9.
* **From the first row, vertical: the dominant term.** `vert` grows
  monotonically from 2 m (0.5 s) to 86 m (23 s) and 118 m (34.5 s) while the
  pitch command is **exactly 0.00 on every row outside the escape climb**
  (pinned by `accept_m01_wingman_drift_pitch_command_is_zero_outside_the_climb`).
  The law asks for nose-up (its `+0x108` word is positive); step 7 zeroes it.
  By 28 s the vertical error is larger than the along-track one and stays the
  largest term until the climb.
* **Cross-track follows from it.** With no pitch the only authority is roll;
  the coarse branch rolls toward `-bx` and cannot raise the nose, so the
  slot slides past: `cross` swings 2 → 60 → 128 m (roll saturating at 25 s and
  34.5 s).
* **~35 s, the escape climb is the overshoot.** The probe latches at 35.0 s
  (`esc`); for ~1.5 s the emergency words (3.5 / 1.0) saturate pitch at +1.00.
  That is the one pitch input in the whole flight, and nothing takes it out:
  after the latch drops (36.5 s) Betty keeps climbing with a zero pitch word,
  `vert` goes 103 → −295 m (Betty 295 m *above* the slot), speed falls
  104 → 76 m/s so `along` opens to 355 m, and `cross` to 369 m. The 577 m
  final gap is these three at comparable size (|slot error| ≈ 585 m).

So the 577 m is **a vertical error that the dead pitch axis cannot correct,
and its one correction, the escape climb, is an uncorrected overshoot.** It
starts at tick 1; the speed deficit is only the first 20 s.

### 10.3 The port against the image

The pitch scale is the term to check. Every link of "dead" is a pinned window
(`accept_m01_wingman_drift_pitch_scale_windows_match_the_image`):

| claim | window |
| --- | --- |
| the key `mode_alt` stores into the definition's `+0xa8` | text `0x6280b8`; `push 0x6280b8` at `0x47b08e`; `mov [edi+0xa8], eax` `89 87 a8 00 00 00` at `0x47b0a6` (constructor default 2.0, `0x478b10`, is overwritten there) |
| the spawn copy takes `+0xa8` unless it is −1.0 and stores vehicle `+0x8c8` | `mov eax,[ebx+0xa8]` `0x47d1e9`; `cmp` with `0xbf800000` `0x47d1ef`; `mov [esi+0x8c8], eax` `0x47d1fb` |
| step 7 multiplies the pitch word `+0x108` by `+0x8c8` | `fld [esi+0x8c8]` `0x41be80`; `fmul [esi+0x108]` `0x41be97` |
| nothing else rewrites `+0x8c8` | a byte scan of `.text` for `89`/`c7`/`d9` stores of disp32 `0x8c8` finds only `0x47d1fb` and `0x47d20f` (the second is the `def+0x268` fallback) |

The port's mapping is the image's: `+0x108` is the pitch word (`by`, the
projection on the follower's up row), scaled by `+0x8c8` = `mode_alt` = 0.0,
and `+0x100` takes the `+0x8c4` (0.6) roll scale. **No port bug found in the
pitch scale, so nothing was changed**; the default off `original_stick` and
the default M01 follower are untouched. The throttle path (`0x41b560` →
`follow_throttle_target`) is not the dominant term, so it was not re-derived
here.

What the statics leave open, and so what to look at next: the original
wingman is a vehicle with *no* pitch authority from this law, yet it holds
altitude in the shipped game. Either something outside the law supplies pitch
(the flight model's own trim or auto-level in the force block `0x48c..`, which
the port's `OriginalFlightModel` may not reproduce), or the follower of M01 is
not flown by this law. A second AI routine with the same words exists:
`0x4209b0` (called from `0x41d4e9` and `0x41da69`) multiplies `+0x100`,
`+0x108`, `+0x10c` by `+0x8c4`/`+0x8c8`/`+0x8cc` (`0x420ea3`..`0x420ec7`) and
writes `+0x108` from constants at `0x420d93`/`0x420dd0` (∓0.05); it is not the
follower (§1) and was not decoded here.

| unknown | affects | resolving step |
| --- | --- | --- |
| what supplies the follower's altitude authority in the original with `+0x8c8` = 0 | whether `original_stick` can ever hold station | the force block's use of `+0x114/+0x11c/+0x120` (`0x48caea`, `0x48cb07`, `0x48cbb7`) against the port's flight model |
| the role of `0x4209b0` (callers `0x41d4e9`, `0x41da69`) | whether it flies M01's wingman in any phase | **resolved in §11 (task #2297): no — it is the mode-0 scripted attitude-program driver, gated behind `+0x67c` = 0 and `+0x358` = 1, which the mode-4 follower never reaches** |

Nothing here is `verified_original`.

## 11. `0x4209b0` is the mode-0 scripted attitude-program driver — it cannot fly M01's wingman (task #2297)

Static reads of the owner image; claim id `OWNER-STATIC-2026-10-29`,
nothing `verified_original`. Every claimed byte window is pinned by an
`accept_m01_wingman_4209b0_*` test in
`crates/cs_app/tests/campaign/m01_wingman_4209b0.rs`. Implementer
`swe2-max-1`. This section answers §10.3's open row: what `0x4209b0`
computes, which goal it steers toward, what the ∓0.05 writes mean, the
mode gates at both callers, and whether M01's player-led wingman in
phase 0 or 1 can reach either one.

### 11.1 The two call sites are the `+0x358` = 1 slot of a mode-0 switch

A byte scan of `.text` for `e8` rel32 call sites reaching `0x4209b0`
finds exactly two:

| caller | containing routine | gate chain to the call |
| --- | --- | --- |
| `0x41d4e9` | `0x41d1f0` (the mode-0 maneuver law) | the AI step `0x41c270` requires `+0x67c` ≠ 4 and `+0x2f0` ∈ {0,2} (`0x41c2dd`..`0x41c305`); inside `0x41d1f0`, `+0x67c` must be 0 (`0x41d2a6`: `jne 0x41d671`) and `+0x358` is dispatched through the four-entry table `0x41d8d4` = {`0x41d30b`, `0x41d4e7`, `0x41d4f0`, `0x41d2ca`}; slot 1 lands on the call site at `0x41d4e7` |
| `0x41da69` | `0x41d9f0` (the mode-0 pursuit law) | the AI step requires `+0x67c` ≠ 4 and `+0x2f0` = 1 (`0x41c2f9`); inside `0x41d9f0`, `+0x67c` must be 0 (`0x41da4b`: `jne 0x41e2f5`) and `+0x358` is dispatched through the four-entry table `0x41e744` = {`0x41dac1`, `0x41da67`, `0x41da73`, `0x41da80`}; slot 1 lands on the call site at `0x41da67` |

Both tables share the `+0x358` sub-state vocabulary: 0 is the law's
default leg (maneuver leg `0x41d30b` → `0x41b560` `bl=0`, or pursuit
`0x41dac1`), **1 is the program driver `0x4209b0`**, 2 is the route
driver `0x4216e0` (`0x41d4f2` / `0x41da75`), 3 is the climb `0x41b560`
`bl=1` (`0x41d2ca` / `0x41da80`), and `+0x358` > 3 falls out of the
switch (`0x41d4fd` / `0x41e594`).

`+0x358` writers (a byte scan for `mov [reg+0x358], …`): constructor 0
(`0x4b08bf`), program arm **1** (`0x4208f7`, inside `0x4201a0`), route
arm 2 (`0x4214d4`; also `0x489b0c` in the vehicle update, which raises
`+0x358` to 2 whenever the route handle `+0x9bc` is non-null and
`+0x358` < 2 — a pending route outranks the program), escape latch 3
(`0x41f98e`), stun 4 (`0x420127`), route-in-progress 5 (`0x42180f` —
the vehicle update skips the whole AI step `0x41c270` while it stands,
`0x489a73`), and clears to 0 (`0x41f9a6` escape end, `0x489ae9` stun
recovery when `clock ≥ +0xc0`, `0x421098` program exhausted,
`0x490d09`).

### 11.2 What `0x4209b0` computes: a per-leg target attitude, not a position

`0x4209b0(this=esi)` drives a vector of 32-byte program entries held at
`[esi+0x9a4]..[esi+0x9a8]` with index `[esi+0x9b0]` (the builder
`0x4201a0` fills the vector from a 28-byte-row descriptor table indexed
at `.bss` `0x71b210`, sets `+0x9b0` = −1 and arms `+0x358` = 1 at
`0x4208ed`/`0x4208f7`). Each entry is {target attitude quaternion
`+0x00..+0x0c` (w first), minimum dwell seconds `+0x10`, scripted stick
triple `+0x14`/`+0x18`/`+0x1c`}. Per tick:

1. **First tick** (`+0x9b0` = −1 → 0, `0x4209c2`..`0x4209fd`): the leg
   deadline `+0x89c` = `clock + entry.dwell` (`fld [eax+ecx+0x10]`,
   `fadd [0x71c470]`, `fstp [esi+0x89c]`).
2. **Throttle** (`0x420a03`..`0x420a77`): `+0x124` slews by `dt·0.35`
   (`fld [0x9ad744]` × `0x603508` = 0.35) up when own speed `+0x934` is
   under **80.467 m/s** (`0x6036b8`) and down otherwise, clamped to
   [0.8, 1.1] (`0x61fb68`/`0x61fb6c`) — a fixed cruise-speed hold, not
   a station range law.
3. **Scripted-stick leg** (entry `+0x14`/`+0x18`/`+0x1c` not all
   `0x6032c8` = 0.0, `0x420a86`..`0x420ac5` → `0x420e60`): the triple is
   written literally — `+0x14` → `+0x108` (pitch), `+0x18` → `+0x10c`
   (yaw), `+0x1c` → `+0x100` (roll) — and the residual is forced to the
   identity quaternion `(1,0,0,0)` at `[ebp-0x30]` (`0x420e75`), so the
   leg advances on its deadline alone.
4. **Attitude leg** (all-zero triple): the body-frame attitude error
   `Δ = conj(+0x150) ⊗ entry` is built by `0x53f8d0` (copies dword 0,
   sign-flips dwords 1..3 — a quaternion conjugate) and `0x53f920`
   (quaternion product, w-first layout), at `0x420ac5`..`0x420b17`.
   `+0x150` is the AI's own orientation quaternion: `Δ` is the rotation
   that carries the current attitude to the leg's target attitude —
   the routine steers toward an **attitude**, not a waypoint.
   * **Fine band** (`|Δ.x|` and `|Δ.z|` both under the `f64` 0.05 at
     `0x6036b0`, `0x420b17`..`0x420b63`): `+0x100` = 0, `+0x10c` =
     `Δ.y·sign(Δ.w)` (shortest-arc yaw), `+0x108` = 0.
   * **General** (`0x420b64`..): `0x53fb40(out, Δ, (0,0,−1))` rotates
     the forward axis constant `0x6034c8` = `(0,0,−1,37.5)` (only the
     first three components are read) by `Δ` — the **desired nose
     direction in the current body frame**. `r = √(x²+y²)`; when
     `dir.z > 0` (the target attitude points behind) `x,y` are
     normalized by `r`, else kept raw (`0x420b79`..`0x420bd9`). Then a
     bank-to-turn split: `+0x10c` = 0 (`0x420be2`); `+0x100` = `−x′`
     when `y′ ≥ 0`, else `x′` or a hard ±1.0 bank split at `V ≥ 0.35`
     (`0x61fb80`, where `V` is `r` ahead and 1.0 behind,
     `0x420be8`..`0x420c29`); `+0x108` = `y′` only while `|x′| < 0.06`
     (`0x61fb70`, `0x420c2f`..`0x420c56`), else 0; and when `|x′|` and
     `|y′|` are both under 0.06 (`0x61fb74`) the roll word is replaced
     by `Δ.z·sign(Δ.w)`, the residual heading term
     (`0x420c56`..`0x420c8d`).
5. **Gain finish** (`0x420e9d`..`0x421015`): each stick word is scaled
   by `+0x8c4`/`+0x8c8`/`+0x8cc`, clamped to ±`+0x8d0`/`+0x8d4`/`+0x8d8`,
   multiplied by the AI skill word `+0x974`, and stored to the same
   outputs `+0x114`/`+0x11c`/`+0x120` as `0x41b560` through the identity
   stub `0x460890` (`fld [esp+4]; ret`).
6. **Leg advance** (`0x42101b`..`0x4210d3`): `0x53fca0` extracts the
   residual's axis·angle form (`fpatan` of the vector magnitude —
   `bits>>1 + 0x1fc00000` square-root hack — against the scalar, so the
   output's magnitude is the quaternion half-angle); when it falls
   under **0.15** (`0x6036a8`) and `clock ≥ +0x89c`, `+0x9b0`
   increments, the next entry reloads `+0x89c` = `clock + dwell`, and
   on exhaustion `+0x358` = 0 with `+0x89c` = `clock + 3.5`
   (`0x6036a4`, a 3.5 s re-arm cooldown, `0x421098`..`0x4210b5`).

So the goal `0x4209b0` steers toward is **the current leg's target
attitude quaternion** (or a literal stick script when the triple is
nonzero) — a scripted aerobatic/maneuver program player, the mode-0
sibling of the `+0x358` = 2 route driver `0x4216e0`. It never reads a
station slot, a leader, or the `+0xe0` goal the follower writes.

### 11.3 The ∓0.05 writes are a minimum-authority floor on the dominant channel

Between the attitude law and the gain finish (`0x420c9d`..`0x420e5e`)
the routine picks the channel whose word has the strictly largest
magnitude — `|+0x10c|` against `|+0x108|` and `|+0x100|` in that order —
and, when that dominant word lies inside (−0.05, 0) ∪ (0, +0.05], snaps
it to exactly −0.05/+0.05 (literal `0xbd4ccccd`/`0x3d4ccccd`; the same
values also live at `0x6036ac`/`0x6034d8`). `+0x10c` is floored at
`0x420cf9`/`0x420d36`, `+0x108` at `0x420d93`/`0x420dd0`, `+0x100` at
`0x420e22`/`0x420e54`. Words above 0.05 pass through, and a zero word
can never be the dominant one, so **the writes are a minimum-effective
kick on the dominant channel — not a trim, not an altitude hold**. On
the `+0x108` channel the floor raises a small proportional pitch demand
to 0.05; downstream `+0x11c` = `+0x108·(+0x8c8)·(+0x974)` still scales
it by the vehicle's pitch gain.

### 11.4 Does it fly M01's wingman, in any phase? No.

* M01's player-led wingman is spawned `+0x67c` = 4. At `0x41c2dd` the AI
  step jumps `+0x67c` == 4 to `0x41c358`, whose only call is
  `0x41e760` — the follower of §6.1. The `+0x2f0` split that leads to
  either `0x4209b0` caller is on `+0x67c` ≠ 4, and both containing
  routines re-test `+0x67c` == 0 before the `+0x358` switch.
* The wingman's phases `[ai+0xd8]` 0 and 1 are states *inside* the
  follower `0x41e760`; `0x4209b0` is not called there under any
  `+0x358`. **A mode-4 wingman cannot reach either call site in any
  phase.** The state words required are `+0x67c` = 0 ∧ `+0x358` = 1 ∧
  `+0x2f0` ∈ {0,1,2} — mode 0 with an armed program — which is a
  different AI, not a player-led follower.
* `+0x358` = 1 itself is only armed by `0x4201a0`, whose callers are the
  mode-0 pursuit branch (`0x41db9d`, reached from `0x41dac1`), a
  directive path (`0x43e10e`) and an assignment path (`0x4b9ff8`) — and
  even then a non-null route handle `+0x9bc` forces `+0x358` = 2 in the
  vehicle update before the step runs (`0x489b0c`).
* `+0x67c` does have runtime writers (e.g. `0x49c920` writes mode 0
  with the asleep byte `+0x91d` = 1), so a mode-4→0 demotion is not
  structurally impossible; but a demoted wingman is no longer the
  player-led follower the question is about, and whether M01's wingman
  ever takes that path is untraced (see unknowns).
* Even hypothetically, the routine's pitch channel could not supply the
  missing altitude authority of §10.3: `+0x11c` = `+0x108·(+0x8c8)·(+0x974)`
  and the wingman's `+0x8c8` = `mode_alt` = 0.0, so every pitch word
  `0x4209b0` writes — including the ∓0.05 floor — still lands on zero.

So `0x4209b0` is **no** for M01's wingman in any phase, and it is also
not the hidden altitude authority: the dead pitch axis is identical in
both stick laws, which keeps §10.3's force-block/trim hypothesis open.

### 11.5 Byte windows and unknowns

| claim | window |
| --- | --- |
| `0x41d4e9` / `0x41da69` are `call 0x4209b0` | `e8 c2 34 00 00` at `0x41d4e9`; `e8 42 2f 00 00` at `0x41da69` |
| both callers gate `+0x67c` == 0 | `8b 96 7c 06 00 00 85 d2 0f 85 bd 03 00 00` at `0x41d2a6` (`jne 0x41d671`); `8b 86 7c 06 00 00 85 c0 0f 85 a4 08 00 00` at `0x41da43` (`jne 0x41e2f5`) |
| both dispatch `+0x358` by jump table | `83 fa 03 0f 87 … ff 24 95 d4 d8 41 00` at `0x41d2b4`..`0x41d2c9`; `83 f8 03 0f 87 … ff 24 85 44 e7 41 00` at `0x41da51`..`0x41da66`; tables `0x41d8d4`, `0x41e744` |
| mode 4 → follower, not the split | `83 be 7c 06 00 00 04 74 72` at `0x41c2dd` (`je 0x41c358`); `56 e8 02 24 00 00` at `0x41c358` (`call 0x41e760`); `+0x2f0` dispatch `0x41c2e6`..`0x41c305` |
| program vector walk and dwell | `+0x9b0`/`+0x9a4`/`+0x9a8`/`+0x89c` at `0x4209b9`..`0x4209fd`; 32-byte stride `c1 e0 05`; `sar edx,5` at `0x4209e5`/`0x421091` |
| throttle slew and clamp | `0x420a03`..`0x420a77`; `0x6036b8` = 80.467, `0x603508` = 0.35, `0x61fb68`/`0x61fb6c` = 0.8/1.1, `0x9ad744` = dt |
| entry stick triple → literal words | `0x420a86`/`0x420a9d`/`0x420ab1` test `0x6032c8` = 0.0; `0x420e60`..`0x420e7c` writes; `c7 45 d0 00 00 80 3f` forces the identity residual |
| `Δ = conj(+0x150) ⊗ entry` | `lea ecx,[esi+0x150]` `0x420ad9`; `call 0x53f8d0` `0x420b01`; `call 0x53f920` `0x420b12`; `0x53f8d0` flips dwords 1..3 via `xor …,0x80000000` at `0x53f8f8`..`0x53f909` |
| desired-forward construction | `push 0x6034c8` `0x420b6b`; `call 0x53fb40` `0x420b71`; `0x6034c8` = `(0,0,−1,37.5)`; small band `fcomp qword [0x6036b0]` `0x420b1f` (f64 0.05) |
| ∓0.05 floor | six stores `c7 86 <off> 01 00 00 cd cc 4c (3d|bd)` at `0x420cf9`/`0x420d36`/`0x420d93`/`0x420dd0`/`0x420e22`/`0x420e54` |
| gain finish and outputs | `+0x8c4`/`+0x8c8`/`+0x8cc` at `0x420e9d`/`0x420eb2`/`0x420ec7`; `+0x8d0`/`+0x8d4`/`+0x8d8` at `0x420edc`/`0x420f19`/`0x420f55`; `+0x974` at `0x420f91`; `+0x114`/`+0x11c`/`+0x120` at `0x420fe3`/`0x420ff9`/`0x421015`; `0x460890` = `d9 44 24 04 c3` |
| leg advance | `call 0x53fca0` `0x421023`; `fcomp [0x6036a8]` `0x421052` (0.15); `fcomp [esi+0x89c]` `0x421065`; `+0x9b0`++ `0x421072`; exhaust `89 be 58 03 00 00` `0x421098`, cooldown `fadd [0x6036a4]` `0x4210a4` (3.5) |
| `+0x358` = 1 arm | `c7 87 b0 09 00 00 ff ff ff ff` `0x4208ed` (`+0x9b0` = −1); `c7 87 58 03 00 00 01 00 00 00` `0x4208f7` (`+0x358` = 1); `0x4201a0` callers `0x41db9d`/`0x43e10e`/`0x4b9ff8` |
| route outranks program | `89 86 58 03 00 00` `0x489b0c` (`+0x358` = 2); stun clear `c7 86 58 03 00 00 00 00 00 00` `0x489ae9`; `call 0x41c270` `0x489b14` |

| unknown | affects | resolving step |
| --- | --- | --- |
| what fills the `0x71b210` descriptor table at runtime (it is `.bss`, not image data) | which entities can ever run a `+0x358` = 1 program, and whether M01's wingman is ever scripted into one | trace the `0x71b210` writers and the `0x4201a0` directive/assignment call paths (`0x43e10e`, `0x4b9ff8`) against M01's script |
| whether M01's wingman's `+0x67c` is ever demoted from 4 (runtime writers exist: `0x49c920` writes mode 0 with `+0x91d` = 1) | the strict form of "in any phase" — a demoted wingman is no longer the follower, but the demotion path is untraced | trace `0x49c920`'s caller chain in retail M01 |
| the exact semantic of `+0x2f0`/`+0x2f4`/`+0x300`/`+0x308` (leg selector, next selector, re-eval deadline) | which mode-0 entities take the maneuver vs pursuit split | decode the `+0x2f0` writers (`0x49ca25`, `0x41c299`) |
| whether `+0x150` is maintained as the orientation quaternion or refreshed per tick | the precision of "attitude error" phrasing | trace the `+0x150` writers |

Nothing here is `verified_original`.

## 11. No AI trim or auto-level exists: `+0x114/+0x11c/+0x120` are the stick inputs, and the Level-Off assist is the player's (task #2296)

Static reads of the owner image (`llvm-objdump -d -M intel`) and of `vehicle.zrd`; claim id
`OWNER-STATIC-2026-10-29`, nothing `verified_original`. `accept_m01_wingman_altitude_windows_match_the_image`,
`accept_m01_wingman_altitude_has_no_ai_pitch_authority` (image-gated) and
`accept_m01_wingman_altitude_level_off_is_the_players_toggle` (retail-gated) re-read every window below.
Implementer: `deepseek-1`.

### 11.1 Answer

**No such term exists.** Nothing outside the AI flight laws gives this wingman pitch authority — the only two
AI routines that write the force block's pitch input are the follower law itself and `0x4209b0`'s `∓0.05`
(§11.2) — and the port reproduces every term that acts with a zero pitch word. §10.3's three candidates are not
force coefficients at all: `+0x114`, `+0x11c` and `+0x120` are **the three stick words themselves** — the force
block's roll, pitch and yaw torque inputs — so a zero pitch word means the pitch torque term is skipped outright
(`0x48cafb`). The one term in the block gated on both inputs being zero is the Level-Off assist, and it needs a
per-vehicle byte `+0x12c` that only the **player vehicle's** `Shift+L` handler ever sets; every AI vehicle is
constructed with it clear and no AI code touches it.

What holds an M01 wingman's altitude is therefore the **lift law**, not a pitch term: the AI's lift target is
always its own nose (`0x48c6e9`), the demanded acceleration is `lift_accel_rate * (Vt - V) + (0, g, 0)`
(`0x48c746`, `0x48c77b`), and `CL` is chosen so that `qS * CL` along the normal `(nx, ny)` reproduces that
demand's component in the plane normal to the nose. With the velocity along the nose the demand is pure gravity,
so the lift equals the weight and the aircraft accelerates neither up nor down: it holds *its own* altitude and
cannot follow a leader that climbs. That is the whole of the original's "holds altitude", and the port flies it
already (§11.5). Nothing was changed: the default M01 follower, the pins, the smoke and the player's flight model
are untouched, and `original_stick` stays off.

### 11.2 The three torque terms and their inputs

`0x48c470..0x48d2b7` is the force/rotation block. Each of the three control torques is
`input * fade * torque * dt`, skipped entirely while the input is exactly `0.0`:

| term | input read (skip test) | torque word | axis | window |
| --- | --- | --- | --- | --- |
| roll | `+0x114` (`fld` `0x48ca7a`, `fcomp 0.0` `0x48ca80`, `jne` `0x48ca8b`) | `+0x644` (`0x48ca8d`) | body Z `+0x198` (`0x48caaa`) | `d9 86 14 01 00 00`, `d9 86 44 06 00 00` |
| pitch | `+0x11c` (`fld` `0x48caea`, `fcomp 0.0` `0x48caf0`, `jne 0x48cba0` `0x48cafb`) | `+0x648` (`0x48cb07`) | body X `+0x180` (`0x48cb0d`) | `d9 86 1c 01 00 00`, `d8 8e 48 06 00 00`, `8d 86 80 01 00 00` |
| yaw | `+0x120` (`fld` `0x48cba0`, `fcomp` `0x48cba6`, `jne 0x48cc56` `0x48cbb1`) | `+0x64c` (`0x48cbb7`) | body Y `+0x18c` (`0x48cbc6`) | `d9 86 20 01 00 00`, `d9 86 4c 06 00 00`, `8d 86 8c 01 00 00` |

The skip is a jump over the whole term, not a multiply by zero: for the pitch word the term's multiplies
(`fmul [esi+0x648]` at `0x48cb07` and `fmul [esi+0x11c]` at `0x48cb1f`, with the axis load between them) are
simply not executed. The torque word names are the definition keys, read from the `vehicle.zrd` parser and loader:

| key (text VA) | parser slot | loader → vehicle slot | windows |
| --- | --- | --- | --- |
| `roll_torque` `0x627fa4` | `def+0x100` `0x47ad5b` `89 87 00 01 00 00` | `0x475bdc` `8b 8d 00 01 00 00` → `0x475be2` `89 8b 44 06 00 00` (`+0x644`) | see parser/loader |
| `pitch_torque` `0x627f94` | `def+0x104` `0x47ad3d` `89 8f 04 01 00 00` | `0x475be8` `8b 95 04 01 00 00` → `0x475bee` `89 93 48 06 00 00` (`+0x648`) | see parser/loader |
| `rudder_torque` `0x627fb0` | `def+0x108` `0x47ad79` `89 97 08 01 00 00` | `0x475bf4` `8b 85 08 01 00 00` → `0x475bfa` `89 83 4c 06 00 00` (`+0x64c`) | see parser/loader |
| `level_off_rate` `0x627fc0` | `def+0x10c` `0x47ad97` `89 8f 0c 01 00 00` | `0x475c00` `8b 8d 0c 01 00 00` → `0x475c06` `89 8b 50 06 00 00` (`+0x650`) | see parser/loader |

Each parser store is `mov [edi+def_slot], <value>` after the key lookup (`push <key text>` at `0x47ad43`,
`0x47ad25`, `0x47ad61`, `0x47ad7f`); the loader copies the record's `+0x100..+0x10c` into the vehicle at
`0x475bdc`..`0x475c06`.

**Where the inputs come from.** Step 7 of the follower law writes the words `+0x100/+0x108/+0x10c`
(§2.3), then, unless the emergency set is in use (`bl != 0`, `jne 0x41c01e` at `0x41bfc6`), scales all three by
`+0x974` (`0x41bfcd`, `0x41bff8`, `0x41c006`) and finally passes each through `0x460890` into the force block's
inputs:

| word | curve call | store |
| --- | --- | --- |
| `+0x100` → `+0x114` | `mov ecx,[esi+0x100]` `0x41c01e`, `call 0x460890` `0x41c02f` | `fstp [esi+0x114]` `0x41c034` (`d9 9e 14 01 00 00`) |
| `+0x108` → `+0x11c` | `mov edx,[esi+0x108]` `0x41c03a`, `call` `0x41c04b` | `fstp [esi+0x11c]` `0x41c050` (`d9 9e 1c 01 00 00`) |
| `+0x10c` → `+0x120` | `mov eax,[esi+0x10c]` `0x41c056`, `call` `0x41c067` | `fstp [esi+0x120]` `0x41c06c` (`d9 9e 20 01 00 00`) |

`0x460890` is `fld dword [esp+4]; ret` (`d9 44 24 04 c3`) — **the response curve is the identity**, and the two
other arguments the law pushes (`0x3f000000` = `0.5` at `0x41c029`, `0x3f800000` = `1.0` at `0x41c024`) are
ignored. So the force block's pitch input equals the law's pitch word exactly, and with `mode_alt` `0.0` (§6.2,
§10.3) that is `0.0`. The `+0x974` scale and the `+0xb4` clock gate around it do not matter for a zero word.

**Writers of the pitch input.** A scan of `.text` for every dword store to a register-based `[reg+0x11c]`
(`89`/`c7`, and `d9 /2`/`/3` for the x87 stores, `mod=10`, base neither `esp` nor `ebp`) finds 21 sites; the ones
that write a *vehicle* are:

| site | window | writes | seat |
| --- | --- | --- | --- |
| `0x41c050` | `d9 9e 1c 01 00 00` | the follower law's curve of `+0x108` | every vehicle the law flies |
| `0x420185` | `89 9e 1c 01 00 00` | `0` — the stun latch zeroing (`0x42015a`..`0x420185`) | any |
| `0x420ff9` | `d9 9e 1c 01 00 00` | the curve of `0x4209b0`'s word, whose only sources are `∓0.05` (`0x420d93` `cd cc 4c bd`, `0x420dd0` `cd cc 4c 3d`) | modes other than 4 only |
| `0x487dcc` | `d9 9e 1c 01 00 00` (+ the clamps `mov [esi+0x11c], 1.0` `0x487e3c`, `mov [esi+0x11c], -1.0` `0x487e5b`) | the player input routine's curve of `+0x108`, clamped to `±1` | the player |
| `0x4b062d` | `89 9e 1c 01 00 00` | `0` — the vehicle's constructor/reset, which also clears `+0x100/+0x108/+0x10c/+0x114/+0x118/+0x120/+0x124/+0x128` (`0x4b0603`..`0x4b063f`, `ebx = 0`) | any |

Inside the AI code (`0x41b560..0x421800`) the writer set is exactly `{0x41c050, 0x420185, 0x420ff9}`, and the
follower routine `0x41e760..0x41f00a` and the force block `0x48c470..0x48d2b7` contain **none**. The `0x4209b0`
words are unreachable for a mode-4 follower: `0x41c270` tests `[esi+0x67c] == 4` at `0x41c2dd`/`0x41c2e4` and
calls `0x41e760` at `0x41c359`; the two other bodies it can call (`0x41d9f0` at `0x41c2fa`, `0x41d1f0` at
`0x41c305`) are the only callers of `0x4209b0` (`0x41d4e9` `e8 c2 34 00 00`, `0x41da69` `e8 42 2f 00 00` —
both inside `0x41d1f0..0x41d63e` and `0x41d9f0..0x41da7f`). And even if it ran, its `∓0.05` word goes through
the same `+0x8c8 = 0.0` scale, so it is zero for this wingman too.

### 11.3 The only zero-input term is the Level-Off assist, and it is the player's

`0x48ce45`..`0x48cf7b`, entered from the weathervane's AI skip (`cmp esi,[0x71c298]` `0x48cd3e` /
`jne 0x48ce45` `0x48cd44`):

| window | bytes | effect |
| --- | --- | --- |
| `0x48ce45` | `8a 86 2c 01 00 00` | `al = [veh+0x12c]` — the assist's per-vehicle flag |
| `0x48ce4d` | `0f 84 2b 01 00 00` | clear → `0x48cf7e`, no level-off at all |
| `0x48ce53` | `d9 86 14 01 00 00` | `fld [veh+0x114]`; `fcomp 0.0`, `je 0x48cf7e` (`0x48ce64`) — a roll input cancels the term |
| `0x48ce6a` | `d9 86 1c 01 00 00` | `fld [veh+0x11c]`; `fcomp 0.0`, `je 0x48cf7e` (`0x48ce7b`) — a pitch input cancels it |
| `0x48ce81` | `8d 86 8c 01 00 00` | `lea eax,[esi+0x18c]` — the rotation is **from the body Y axis** |
| `0x48ce92`..`0x48cea4` | — | world up `(0, 1, 0)` read at `0x6379c0` (`00 00 80 3f` at `+0x4`), the far end of the rotation |
| `0x48ced6` | `d9 05 6c c5 71 00` | `dt` (`[0x71c56c]`) |
| `0x48cedc` | `d8 8e 50 06 00 00` | `fmul [veh+0x650]` — `level_off_rate`, so the increment is `r(Y→up) * level_off_rate * dt`, scaled by `lambda` when it opposes the attack (`0x48cf2a`..`0x48cf52`) |

So it is a **wing-leveler**, not a pitch term: it rotates the body's up axis toward world up, which changes the
lift direction and the heading, never the angle of attack or the climb. It cannot supply altitude authority even
when it runs.

**Who sets `+0x12c`.** A scan of `.text` for every byte store to a register-based `[reg+0x12c]` (`88`/`c6`,
`mod=10`, base neither `esp` nor `ebp`) finds **exactly three** sites:

| site | window | writes |
| --- | --- | --- |
| `0x487d96` | `c6 86 2c 01 00 00 00` | `0` from the player input routine — reached when **any** of `+0x100`/`+0x108`/`+0x10c` is non-zero (`je 0x487d96` at `0x487d51`, `0x487d64`, `0x487d77`), i.e. any stick input turns the assist off |
| `0x48866e` | `88 81 2c 01 00 00` (with `mov ecx,[0x71c298]` at `0x488640`, `sete al` at `0x48866b`) | the toggle: `[player+0x12c] = !old`, the `Shift+L` command handler |
| `0x4b0645` | `88 9e 2c 01 00 00` | `0` — the vehicle constructor/reset, next to the whole control block it clears |

The toggle loads the **player vehicle global** `0x71c298`, so no other vehicle can be toggled. Every AI vehicle
therefore has `+0x12c = 0` for its whole life and the level-off term never runs for it — the port's own
`OriginalState::level_off` (false at every spawn, toggled only by the playtest's `L`) is the same thing by another
name.

### 11.4 What is left for an AI attitude with a zero pitch word

Both remaining terms in the block are unconditional (no seat gate, no stick gate):

| term | window | effect |
| --- | --- | --- |
| bank coupling, yaw part | `0x48cc61` `d9 05 f8 89 62 00` (`fall_off`), `0x48cc6a` `d8 8e 84 01 00 00` (`X.y`) | `up * fall_off * X.y * dt` about body Y |
| bank coupling, pitch part | `0x48ccbb` `fld [X.y]`, `0x48ccc1` `d9 e1` (`fabs`), `0x48ccc3` `d8 0d fc 89 62 00` (`bank_off`) | about body X: `(bank_off*|X.y| - fall_off*Y.y [Y.y < 0]) * dt` — a bank-proportional nose-up term, not a trim toward level |

The two constants are image globals (`0x6289f8`, `0x6289fc`), not record words, so every vehicle gets the same
coupling. The ground blow (`0x48c220`, `2026-10-25-force-block-reread.md` §4.1) also runs for every non-player,
and is the only other AI attitude term; it needs a surface ahead and is a torque about the surface normal, not a
climb command. Neither is a level-flight, trim or lift-versus-weight term, and neither can hold an altitude the
lift law is not already holding.

The AI's *seat* differences are all elsewhere and already ported: the lift target is always `|v| * nose`
(`0x48c6e9` `fld [ebp-0x80]` / `fchs` / `fmul [esi+0x198]`), the demand is
`lift_accel_rate * (Vt - V) + (0, g, 0)` (`0x48c746` `d9 05 48 c4 71 00`, `0x48c77b` `d8 81 c4 00 00 00`), the
weathervane is player-only (`0x48cd3e`/`0x48cd44`), the stall is player-only, the ground blow is unconditional
for AI, and AI aircraft get the nose-ward speed floor.

### 11.5 The port against this

| claim | `cs_sim` | verdict |
| --- | --- | --- |
| the three inputs are the stick words, torques about Z/X/Y | `rotation_increment`, `airframe.roll_torque/pitch_torque/rudder_torque` about `back/right/up` (`2026-10-25-force-block-reread.md` §2) | match |
| a zero pitch word gives a zero pitch torque | `scale(right, pitch_torque * input.pitch * fade * dt)` with `input.pitch == 0` is the zero vector; the `lambda` sign test on a zero vector is never taken | match (computed, not skipped) |
| the identity response curve | none needed: the law's words reach the block unscaled | match |
| the Level-Off assist is gated on `+0x12c` and both inputs zero | `if level_off && input.roll == 0.0 && input.pitch == 0.0` (`crates/cs_sim/src/flight/original.rs`) | match |
| `+0x650` is `level_off_rate` | `Airframe::level_off_rate`, imported from the record, default `DEFAULT_LEVEL_OFF_RATE` = 4.0 (`0x478a00`) | match |
| every AI body starts with the assist clear | `OriginalState { level_off: false }` at every spawn (`mission_session::compose`, `mission_ai`, `playtest::scene`); only the playtest's `L` handler toggles it | match |
| bank coupling, weathervane, stall, ground blow, AI lift target | the same functions (§2 of the re-read) | match |

**Divergence found (player side, not fixed here).** The original *clears* `+0x12c` on any stick input
(`0x487d51`/`0x487d64`/`0x487d77` → `0x487d96`), so after a player uses the stick the assist stays off until
`Shift+L` is pressed again; the port keeps the flag and only suppresses the term inside the step, so the assist
resumes as soon as the stick returns to zero. This is the player's seat, outside this task's owner paths and
outside its "do not change the player's flight model" limit; it is filed as a follow-up. It cannot affect an AI
vehicle, whose flag is never set.

No term is missing, so nothing was added and no re-fly was needed; `original_stick` stays off and §10.1's trace is
unchanged.

### 11.6 Unknowns

| unknown | affects | resolving step |
| --- | --- | --- |
| `veh+0xf4`, whose `> 0` value also clears the assist hands-off (`0x487d83`..`0x487d96`) | the player's assist lifecycle, nothing on the AI seat | its writers (`0x4b05df` clears it at spawn) and readers |
| the `+0x974` scale's record key and `veh+0xb4` clock value | how fast the stick words ramp in, never a zero word | the key table around `0x479240`, and `+0xb4`'s setup |
| what `+0x110`/`+0x118` (written by the second AI law at `0x41c080` from `+0xfc`/`+0x104`) feed | a second AI law's output path; the force block reads neither (its only control reads are `+0x114`/`+0x11c`/`+0x120`) | the consumer of `+0x110`/`+0x118`, in another flight model |
| whether the *shipped* wingman really tracks the player's altitude | whether "holds altitude" means "does not sink" or "follows the leader" — the static reading only supports the first | an original capture (#358) of Betty's altitude against the player's |
| the 14 non-vehicle `[reg+0x11c]` stores (pane rects, aiv documents, other classes) | completeness of the writer scan for other classes only | not reached by this law |

Nothing here is `verified_original`.
