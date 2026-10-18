# M01-WINGMAN-PLAYER-LEADER: a wingman whose leader is the player (#1571)

Date: 2026-10-18. Task #1571. Implementer: claude-2 (Sonnet 5.5). Capabilities
used: `retail` (reads of `aiv.zrd` and `vehicle.zrd`) and the owner's decrypted
image (`crimson.decrypted.exe`, address convention `VA = file offset +
0x400000`; Apple `objdump -d` linear disassembly, read by hand). Every claim is
`static`; nothing is `verified_original`, no original executable ran, and **no
runtime code changed**. Constants are committed as `[[ai_player_leader]]` rows
(and the existing `[[ai_follow]]` player-leader rows) of
`data/owner-static/engine-facts.toml`, and pinned by
`accept_m01_wingman_player_leader_*` in
`crates/cs_app/tests/campaign/m01_wingman_player_leader.rs`.

Builds on `2026-10-17-m01-wingman-formation-control-law.md` (#1563) and
`2026-10-17-m01-wingman-formation-follow-law.md` (#1564).

## Which archive each conclusion was checked against

#1565 found that M01 "The Lost Treasure" is `ZBD/C3/M01`, not `ZBD/C1C/M01`.
Both were read through the production decoder (`read_aircraft_records`):

| archive | `aiv.zrd` records | wingmen and their stored `primary_target` |
| --- | --- | --- |
| `ZBD/C1C/M01` (the archive #1563/#1564/#1550 measured) | `player`, `bsfury_1/2`, `devastator_2/3`, `wingman_2/3`, `rusdevastator_1..5` | `wingman_2` -> `devastator_2`, `wingman_3` -> `devastator_3`. **No record names the player as a wingman's leader**; `bsfury_1` names `player` but is not a `mode: wingman` record |
| `ZBD/C3/M01` ("The Lost Treasure") | `player`, `medkestrel_1..3` (asleep), `wingman_1` | **`wingman_1` (Betty) -> `player`**, team 1 like the player, awake, stored at `(-1378, 150, -1705.92)` heading 40, i.e. `(+48, 0, +107.08)` from the player at `(-1426, 150, -1813)` heading 40 |

So the player-leader branch is the live one for the real M01 (C3/M01) and is
dead for C1C/M01. The earlier #1563/#1564 work measured and implemented the
branch C1C/M01 uses.

## 1. Which record or flag makes the player a wingman's leader

Chain, all `static` (virtual addresses; bytes re-read by the image test):

1. **`aiv.zrd` field 6, "primary target"** (aiv-layout §2) is the leader's
   *name*. The loader `strdup`s non-empty text into `vehicle+0x2f8`
   (`0x47c6a0`..`0x47c6c0`).
2. **Default.** If the field is empty and `vehicle+0x67c == 4`
   (`cmp [eax+0x67c], 4` at `0x47c6cb`), `push 0x628364` (`0x47c6d4`) stores
   the literal **`player`** instead (row `default_leader_name`). So a
   `mode: wingman` vehicle with no stored primary target follows the player by
   default; an explicit `player` (Betty in C3/M01) and the default are the same
   thing.
3. **Resolution.** After the whole spawn loop, the pass at `0x475347`..`0x475371`
   walks the vehicle list `0x71dabc`: for each vehicle with `+0x2f8 != 0` it
   calls `0x4a3360(name)`, stores the result at `vehicle+0x2fc` (`0x475369`),
   frees the name and zeroes `+0x2f8`. `0x4a3360` first searches the vehicle
   registry `0x71dab8` (`0x4a3380`) by comparing the name with `vehicle+0xc`
   through the import pointer `0xa20214` (a string compare; whether it ignores
   case is not measured) and wraps the hit in a 0x14-byte link object
   (`0x4a6330`, link `+4` = the vehicle). Only if no vehicle matches does it try
   two other registries (`0x71d910`, `0x71d338`) and finally allocate a new
   entry; these are not traced.
4. **Which vehicle is "the player".** The spawn code at `0x47c7af` compares the
   vehicle name with the literal `player` (7 bytes including the NUL,
   `rep cmpsb`), and the `player` record is the first record, so it is in the
   list when the pass runs. The follower tests *identity*, not the name:
   `leader = link->vehicle` (`[[vehicle+0x2fc]+4]`), and `leader ==
   [0x71c298]` (`cmp edi, [0x71c298]` at `0x41e78a`, the global player vehicle)
   selects the player branch.
5. **Only `mode: wingman` runs the follower** (`0x41c2dd`, `+0x67c == 4`). For
   `wingman_1` the spawn dispatch picks the `vehicle.zrd` record named by the
   airframe row's first pointer (`0x426d60`: `wingman` on the Devastator row 5,
   `wkestrel`, `wfury`, … on the others; row 5 when the global `0x71db4c` is
   11). **All eleven of those records, and `bswingman`, carry `mode: wingman`**
   in retail `ZBD/zrdr.zbd#vehicle.zrd` (tested), so Betty runs `0x41e760`
   whichever airframe the player has.
6. **Leader death / replacement (read, not run).** `0x4a6610(link)` takes the
   vehicle `V` behind a link and walks the vehicle list; for each vehicle whose
   `+0x2fc` link points at `V` (`0x4a6654`..`0x4a666c`): a vehicle that is not
   mode 4 has its `+0x2f8` name and `+0x2fc` link cleared; a mode-4 vehicle
   whose `V` has a non-negative `+0x2e4` (the net-id slot, aiv-layout §2) is
   cleared the same way, then set to mode 0 with `+0x2ec = -1`, `0x475fc0(this,
   V+0x2e4)`, and takes `V`'s own `+0x2f8` name and a clone of `V`'s `+0x2fc` link
   (`0x4a66be`..`0x4a672c`) — it inherits `V`'s leader. A mode-4 vehicle with
   `V == 0` or `V+0x2e4 < 0` keeps its link. The player-replacement routine
   around `0x47ff4e`..`0x480024` also rewrites `+0x2fc` links (with a fresh link
   object at `0x480004`/`0x480024`); which vehicle it points them at was not
   traced. None of this is exercised for a living player in M01.

## 2. The player-leader slot vector, and what is and is not known of its signs

* Vector: **`(6, 0, 18)`** at `0x61fb88` (rows `station_offset_player_leader_x/y/z`,
  committed by #1563). Selected at `0x41e792` only when the leader is the
  player.
* **Combination, now measured** (`0x41ebf0`..`0x41ec4b`, same shape at
  `0x41ed59`): `goal = x·row0 + y·row1 + z·row2 + t`, where `row0..row2` are
  the three float triples at `leader+0x180`, `+0x18c`, `+0x198` and `t` is the
  float triple at `+0x1a4` (copied `movl` from `0x61fb88` first, then rotated in
  place; the stores are `x`, `z`, `y`). The control writer applies the transpose
  to the goal delta (`0x41ba2e`..`0x41ba88`): `body = (Δ·row0, Δ·row1, Δ·row2)`,
  with `bz = Δ·row2` the component the writer treats as "ahead" (§4: the writer
  normalises the aim and forces its magnitude to 1.0 only when `bz > 0`).
* **Not forced.** For a non-player leader `0x41e7a1`..`0x41e7b6` forces the
  sub-state `+0xd8 = 2` and `+0xdd = 1` on every call; for the player leader
  neither is written (`0x41e792`..`0x41e79f` jumps straight to `0x41e7bd`). A
  player-led wingman therefore *runs the transition table*:
  state 0 -> 1 when the player is awake, within 700 m (`490000`, row
  `follow_engage_dist2`) and own speed `> 20.576`; state 2 -> 1 at once when it
  has no route object, else when the squared route distance exceeds 1 440 000;
  state 1 and 4 fly the station goal. The initial `+0xd8` and whether
  `wingman_1` holds a route object (`+0x948`) are **not traced** (below).
* **Signs: unknown.** Whether `+row0` is the leader's right or left, whether
  `+row1` is up, and whether `+row2` (`bz > 0`, the "ahead" the writer tests) is
  the leader's nose direction or its tail, depend on how the pose-to-basis
  writer orients the rows. That writer was not decoded. Betty's stored pose
  (`+48, 0, +107.08` from the player, both heading 40) cannot decide it without
  the heading convention. The reading "18 m ahead of the player" is therefore
  **only the sign-convention reading of `+row2`**, not a measurement; so is
  "6 m to one side".

## 3. The 80 m push

At `0x41edd2` (every sub-state, after the goal is chosen) the routine takes
`Δ = own position - leader position` (`+0x204`, not the slot) into `[ebp-0x4c]`,
`d² = |Δ|²`, and if `0 < d² < 6400` (rows `follow_near_push_dist2`, 0.0 at
`0x6032c8`) forms `scale = 80 / sqrt_approx(d²)` (`0x41ee5b`..`0x41ee68`: the
integer-shift square-root trick `(bits >> 1) + 0x1fc00000`, divisor 80.0 =
`follow_near_push_radius`), multiplies the three floats at **`[ebp-0x40]`** by
`scale` and **adds them to the goal `+0xe0`** (`0x41ee80`..`0x41eeba`).

**Direction: unresolved.** `[ebp-0x40..-0x35]` is not stored to anywhere in the
routine before that read: the only references to it are the address-taking `lea`
at `0x41ee74`, the loads at `0x41ee80`, `0x41ee88`, `0x41ee90`, the in-place
stores `0x41ee85`, `0x41ee8d`, `0x41ee95` (all after the loads), and two later
`lea`s passed to `0x460be0` at `0x41ef3b` / `0x41ef6c` (after the push). The test
re-reads that exact reference list (`[ebp-0x40], -0x3c, -0x38` and their
neighbours) from the image. A push built from a variable the routine never
initialises means that the vector is whatever the stack held; the likely intent
(`Δ` rescaled so the goal is moved 80 m away from the leader along `own -
leader`) would be written to `[ebp-0x4c]`, which is the *adjacent* slot. This
is a statement about the instruction bytes. Whether the original behaves as the
intent suggests, or reads stale data, is only decidable by running the original
(`REF-OWNER-FIRST-CAPTURE`, #358) or by tracing the callers that leave data at
that stack depth. Do not implement the push.

## 4. The stick thresholds, decoded

`0x41b560(goal, refvel, P = 0x61fb28, bool a, bool b)` after the speed and
throttle part (`2026-10-17-...-control-law.md` §3):
`(bx, by, bz) = body` as in §2; `m = sqrt(bx² + by²)` (`0x41bc08`..`0x41bc1a`).
Axes `A100`, `A108`, `A10c` are the words `vehicle+0x100/+0x108/+0x10c`.

1. **Normalise when ahead** (`0x41bc28`): if `bz > 0`, `(bx, by)` is divided by
   `m` (numerator `stick_unit_scale = 1.0`; if `m == 0`, `by = 1`), and `m := 1`.
   If `bz <= 0` neither happens.
2. `A108 := 0`, `A10c := 0` (`0x41bc77`).
3. **Fine aim** when `m <= def+0x260` (`vehicle+0x64` is the definition; what the
   word at `+0x260` is is unmeasured) **and** `|bx| > |by|` (`0x41bc90`..): `A100
   := by` (`-by` if `bx < 0`); and if `|by| < P[2]` (0.06, row `follow_params_2`)
   then `A10c := -bx`.
4. **Otherwise (coarse)** (`0x41bcea`): if `by < 0`: with the second `bool`
   clear and `m >= P[6]` (0.35, row `follow_params_6`) the aim `bx` is replaced by
   `+/-1.0` with `bx`'s sign; in every other `by < 0` case `bx := -bx`.
   Then `A100 := -bx`, and if `|bx| < P[2]` then `A108 := by`.
5. **Wings-level assist** (second `bool` clear only, `0x41bd5c`): if `|bx| <
   P[2]` and `|by| < P[3]` (0.06, row `follow_params_3`) and `|basis[0x19c]| <
   0.9` (row `stick_level_forward_y_max`, a double), `A100` is set to `+/-1` from
   the signs of `basis[0x190]`/`basis[0x184]`, times 0.2 (row `stick_level_gain`).
6. **Dive/stall recovery** (`0x41bdef`): if `basis[0x19c] < -0.5` (row
   `stick_recover_forward_y_max`) and speed `vehicle+0x934 < 26.8224` (row
   `follow_speed_band`), `A108 := +1` when `basis[0x190] < 0` else `-1`, and the
   throttle target `vehicle+0x124 := P[1] = 1.5`.
7. Each axis is multiplied by its pilot gain (`vehicle+0x8c4/0x8c8/0x8cc`, or
   `0x8dc/0x8e0/0x8e4` when the first `bool` is set; plus 0.5 when the global
   byte `0x64ee4c` is non-zero, row `stick_gain_bonus`) and clamped to the limit
   (`+0x8d0..` / `+0x8e8..`; plus 0.25, row `stick_limit_bonus`, under the same
   byte).

**Axis names are inferred, not measured.** The structure (`A100` follows the
sideways error `bx`, `A108` the vertical error `by`, `A10c` is a small-error
`bx` trim) is what a roll/pitch/yaw stick would do, but the mapping of
`+0x114/+0x11c/+0x120` onto the flight handler's inputs was not read here, and
`basis[0x184]/[0x190]/[0x19c]` are named by offset only.

## 5. Unknown, with the affected content and the resolving step

| unknown | affects | resolving step |
| --- | --- | --- |
| the signs of `row0`, `row1`, `row2` (§2) | which side of, how far above and ahead/behind the player Betty's `(6, 0, 18)` slot is | decode the pose-to-basis writer for `vehicle+0x180` (the quaternion helper `0x53fa40` and the heading-to-quaternion step in the spawn path) and the thrust axis of `0x48e580` |
| direction of the 80 m push (§3) | Betty's behaviour within 80 m of the player | a stack-depth trace of what the caller chain leaves at `[ebp-0x40]`, or a reference capture (#358) |
| initial `+0xd8`, and whether `wingman_1` gets a route object `+0x948` | whether Betty starts in the station law or the route law (§2) | the vehicle constructor's initial values and the route assignment in `0x41fe10` |
| the route-following goals `0x41ea9c` / `0x41ec5a` | Betty if she holds a route | as in #1563 §5 |
| `def+0x260`, `vehicle+0x668`, `def+0x1e4/0x1e8`, `0x64ee4c` | the stick branch choice and the speed cap | read their writers |
| the roll/pitch/yaw names of `A100/A108/A10c` (§4) | the exact bank/pitch law for the runtime | the flight handler's reads of `+0x114/+0x11c/+0x120` |
| string compare case rules and the fallbacks in `0x4a3360` | name resolution with an unmatched leader | trace `0xa20214` and the two other registries |
| whether Betty is near the player in the first seconds | the owner's "wingmen near the player" judgement | `human_play` / `REF-OWNER-FIRST-CAPTURE`; not available to an agent |

## 6. Consequence for the dependent task

#1550 (`M01-WINGMAN-FORMATION`) is about the real M01 (C3/M01) where Betty
follows the **player**. The measured player-leader facts it can use: the
leader-selection rule (§1), that the sub-state is *not* forced (§2) and so the
700 m engage / 1200 m route-near / state-1 station chain runs, the slot
combination formula, and the stick conditions in §4 with the constants in the
rows. It must keep §5's items as unknown or designed: in particular the slot's
three signs, the push, and Betty's initial state.
