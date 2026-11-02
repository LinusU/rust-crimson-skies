# The wingman's pilot gains, limits and rudder tolerance, read (#2266)

Date: 2026-11-02. Task #2266, follow-up of #2263
(`2026-11-02-m01-wingman-stick-law.md`). Implementer: `deepseek-1`. Static
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
names say "AI input scale/limit". Which one the retail wingman gets is decided
below; this finding records both and does not pick a winner by taste.

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

## 3. What M01's wingman actually gets (retail)

* The `wingman` `vehicle.zrd` record (`ZBD/zrdr.zbd`, member offset 1397861,
  97917 bytes) declares **none** of `rudder_tol`, `ai_input_scale_*`,
  `ai_input_limit_*`. Its `kind_of` chain is `wingman` → `devastator` →
  `basic_airplane`; `basic_airplane` declares `ai_input_*` = `3.5`/`1.0` and
  `mode_alt` = `0.0`, `collision_d` = `[0.6, 0.15]`. `devastator` and
  `wingman` declare neither. **Whether the definition parser's config tree is
  the record's own node or the `kind_of`-merged node is not measured**; the
  no-inheritance reading gives the constructor's words, the inheritance
  reading gives `mode_alt = 0.0` (which would disable the AI's pitch axis
  outright — implausible, and the reason the port flies the no-inheritance
  reading).
* M01's `aiv.zrd` (`ZBD/C3/M01/zrdr.zbd`) `wingman_1` record carries 66
  positional fields, and fields **44..49 — the record document's optional
  override slots `doc+0xa8..doc+0xbc`** (`0x437ca9`..`0x437cf4`, the
  constructor's `-1.0` sentinels at `0x437420`..`0x43743e`) — are all `-1.0`.
  So the record overrides nothing and the definition's words apply.

The port therefore flies, for M01's wingman (`M01_WINGMAN_PILOT_GAINS`,
`cs_app::mission_ai`):

| axis | scale | limit |
| --- | --- | --- |
| roll | `0.5` | `100.0` |
| pitch | `2.0` | `100.0` |
| yaw | `0.15` | `0.25` |

and `RUDDER_TOL` = `0.2` for `def+0x260`. The `ai_input_*` words
(`PILOT_GAINS_AI_INPUT_KEYS`, `3.5`/`1.0`) stay in the crate as the
never-taken fallback, and are flown as the second reading below.

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
| spawn words `0.5/2.0/0.15`, `100/100/0.25` (the port's M01 reading) | 119 m | 464 m | 464 m |
| `ai_input_*` fallback `3.5/1.0` | 119 m | 4101 m | 4101 m |

Neither reading holds the 19 m station: the first stays bounded and
oscillates between roughly 170 m and 464 m, the second diverges steadily.
`original_stick` therefore **stays off**, and the M01 pins keep flying the
designed follower. This says nothing about the original wingman: the port's
force model, the slot's sign convention and the 80 m push
(`2026-10-18-m01-wingman-player-leader.md` §2–§3) are all still unmeasured, so
the law cannot yet be judged by flying it here.

## 6. Open items

| unknown | affects | resolving step |
| --- | --- | --- |
| whether the definition parser reads the record's own keys or the `kind_of`-merged ones | which of §2.1's two sources the `wingman` definition holds (`2.0/0.5/0.15` or `0.0/0.6/0.15`) | trace `0x57a090`'s tree construction and whether `0x477b70`'s copy ctor overlays the parent record |
| why the spawn copy's first source is `mode_alt`/`collision_d` and not the `ai_input_*` block | whether the original's AI stick really uses those six numbers | an original run (`REF-OWNER-FIRST-CAPTURE`) or a stack trace of `arg1` at `0x47c210` in the retail binary |
| the ported law's remaining gaps (§5) | whether `original_stick` can ever be turned on | #2262's slot signs, the 80 m push, and the force-model comparison |

Nothing here is `verified_original`; the numbers are static reads and the
flight numbers are port behaviour.
