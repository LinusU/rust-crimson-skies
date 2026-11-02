# The wingman's pilot gains, limits and rudder tolerance, read (#2266, #2276)

Date: 2026-11-02. Task #2266, follow-up of #2263
(`2026-11-02-m01-wingman-stick-law.md`); §6 answers added by task #2276
(same date, implementer `swe2-max-1`). Implementer: `deepseek-1`. Static
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
