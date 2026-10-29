# #2107 the ground-blow assist (`0x48c220` / `0x48bf60`) in `OriginalFlightModel`

Date: 2026-10-29. Task #2107, the consumer of #2209's decode
(`2026-10-29-groundblow-world-segment-sweep.md`, "the sweep finding"), #2210's
seam (`2026-10-29-groundblow-probe-seam.md`) and #2214's fill
(`2026-10-29-groundblow-sweep-fill.md`). Static analysis only: the
owner-supplied decrypted image `crimson.decrypted.exe` (read-only, never
committed; sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file
offset + `0x400000` below VA `0x643000`) was disassembled with
`llvm-objdump -d -M intel`. **The original executable was not run**: every
original-game claim below is `static` (claim id `OWNER-STATIC-2026-10-29`),
nothing is `verified_original`. Implementer: `deepseek-1`. Reviewed on
2026-10-29 by `deepseek-1` in a fresh session on a rebased head: the reviewer
re-disassembled `0x48c220`, `0x48bf60`, `0x460410`, `0x460700`, `0x53e2e0` and
the `0x48cf7e` call site, measured the pre-#2107 flight on `38c0bf7f4`,
re-pinned `m01_collision` after #2224 landed, and corrected this note's earlier
before/after table, which had been copied from an older base. Same agent
instance and model as the implementer, so this is **not** independent review.

The cited bytes are the machine-readable block at the end. The task's test
prefix is `accept_ground_blow_`; its tests live in
`crates/cs_sim/src/flight/original.rs` and read the production `step`.

## 0. What landed

1. **`OriginalFlightModel::ground_blow`** is the force block `0x48c220` with its
   probe `0x48bf60`, called from `rotation_increment` **after** the level-off
   term and **before** the stall, which is where `0x48cf7e` sits in
   `0x48c470`. It reads only [`OriginalInput::segment_probe`]'s three facts; the
   world query stays in `cs_app` (#2214).
   * `d = dot(back, N)`, `back` the body's third axis (`V+0x198`, **not**
     negated — the probe segment is the negated row, the amount's dot is the
     un-negated one). `d <= 0` or unordered returns amount 0 (`0x48c126`).
   * The player's owner gate: a hit the sweep resolved to a registered vehicle
     with `V+0xcc == 0` is dropped (`0x48c0ac`..`0x48c0e5`); scenery, an
     unresolved vehicle node and an on-path vehicle keep it.
   * `amount = (groundblow_elev - chord) / groundblow_elev * sqrt(d)`
     (`0x48c13a`), so the record's **unnormalised** normal keeps its
     `sqrt(area)` scale.
   * `B = normalize(N x back) * amount`; `0x422690` leaves a zero vector alone,
     so a face normal parallel to the body's third axis gives no torque.
   * player: `factor = (T . B) * groundblow_mag` when `T . B >= 0`, else
     `(T . B) * -0.05 * groundblow_mag` **and the amount is zeroed**
     (`0x48c2f8`..`0x48c30f`); `T += B * factor`. `T` is the increment the step
     built before the block, exactly what the original passes as the second
     argument.
   * non-player: `factor = ai_groundblow * groundblow_mag`, no dot test
     (`0x48c317`).
   * velocity: normalise the current velocity, lerp it toward the nose by
     `1 - f(dt * 2 * amount)` and rescale by the pre-alignment speed
     (`0x48c3a1`..`0x48c434`, `0x460700`); the integration then starts from the
     new velocity, while the aerodynamics above already used the old one.
   * the call site runs the block for **every non-player** and for the player
     only while fake dynamics are off (`0x48cf7e`: `cmp esi,[0x71c298];
     jne call; test [esi+0x384]; jne skip`).
2. **`decay_factor`** is the original's `f(x)` (`0x460410`): the `exp` table at
   `0x75cd18` from `0.1` up, and
   `1 + x * ((0.5 - 0.166666 x) x - 1)` below it (the image's own rounded
   `0.166666`, `0x607bd4`, not `1/6`). The law's momentum damping already used
   a plain `exp(-damp dt)`; this one needed the polynomial branch because
   `dt * 2 * amount` is routinely below `0.1`.
3. **The three globals' runtime addresses are confirmed by the loader.**
   `0x474308`..`0x474385` reads `player.zrd`'s keys by name
   (`0x627860` = `groundblow_elev`, `0x627870` = `groundblow_mag`,
   `0x627880` = `ai_groundblow`) into `0x71c420`, `0x71c424`, `0x71c428`, with
   the image's own defaults `100.0`, `1.5`, `0.9` when a key is absent. So the
   shared `0x71c424` in both branches is `groundblow_mag` and the AI's
   `0x71c428` is `ai_groundblow` — not inferred from the retail values alone.

## 1. Corrections and refinements to the earlier findings

* **The alignment call is `0x460700`, not `0x460410`.** `0x460410` is the decay;
  `0x460700(a, b, factor)` is the lerp `a_i = (a_i - b_i) * f + b_i` with
  `f = 0x460410([0x9ad744] * factor)`. `2026-10-25-force-block-reread.md`
  section 4.1 cited `0x460410(dt * 2.0 * amount)` as the turn itself; the
  observable is the same lerp, the site is one level down.
* **`[0x64f66e]`, the gate on the `V+0x924` write, is the `suspend` console
  command.** `0x43db43` compares an 8-byte command name at `0x622c7c`
  (`suspend`) with argument count 2, calls `0x5b7910` and stores the answer
  here; `0x464693` and `0x4654fa` clear it. `0x48e632` skips the whole vehicle
  step while it is set. It is a BSS byte, zero in a normal session, so the
  alignment does reach the velocity; this settles section 9's open item of the
  sweep finding for the ground-blow path.
* **`V+0x358 == 4` is pilot incapacitation** (`0x48c22b`, the status-effect
  mode word of `2026-10-28-t452-r05-slot-map-and-incapacitation.md`), not a
  vehicle class.
* The player's negative-dot branch zeroes the **amount** (`0x48c2ff`), so the
  velocity steering is the identity write at `f(0) = 1`; the torque still
  changes. Both are pinned by tests.

## 2. What is not modelled, and why

| gap | why | effect |
| --- | --- | --- |
| `V+0x358 == 4` returns at once (`0x48c22b`) | `cs_sim` has no pilot-incapacitation state (`cs_sim::weapons::ordnance::StatusEffectKind` has no such variant, #2172) | a stunned actor's ground blow runs where the original's would not; only reachable once the incap citation lands |
| a **non-player's** blow is skipped while `now < V+0xac` (`0x48c241`) | `V+0xac` is written only by the contact response (`0x48d31e`..`0x48d395`), which is not part of this law or of `cs_sim`; `OriginalState` has no field for it and adding one changes literals in `cs_app` outside this task's owner paths | an AI within 1 s of an aircraft-into-aircraft contact keeps its ground blow. Filed as follow-up |
| the `0.15` damping while `now < V+0xb4` (`0x48c317`..`0x48c345`, `0x6036a8`) | no site in the image assigns `+0xb4` a non-zero value (sweep finding section 6); unreachable by construction | none in a session that never restores such a state |
| the `suspend` gate on the velocity write | the port has no `suspend` console command | none in normal play (the flag is 0) |
| the `[0x9ad744]` frame clock and the `V+0x930`/`V+0x934` cache | the law takes `dt_s` and recomputes `|v|`; the original caches the same value after each update | none measurable |
| the f32 arithmetic | this law works in f64 with f32 at the atmosphere and `approx_sqrt` boundaries, as the rest of the module does | the amount differs in the last f32 digits only |
| the `hold_attitude` dev tool | the whole rotation block, the ground blow included, is skipped there; the tool only measures a top speed (`0x491c60`) | no production flight leaves the flag on |
| `V+0x358`/`+0xac`/`+0xb4` field writes | see the rows above | as stated |
| the fake-dynamics path's seat rule | this law applies the block in `rotation_increment` for every seat with the call site's `V+0x384` gate; whether the original's fake-dynamics update ever reaches `0x48c470` is unmeasured | no fake AI is probed in a mission, so nothing observable; recorded, not guessed |
| the `exp` branch's own table | the port computes `exp(-x)` exactly, the original's `0x460410` else-branch reads a lazily filled table of `exp(-n/51)` steps (`0x53e2e0`, the fill at `0x75cd18`, `[0x60912c]` = `-1/51`, `[0x609128]` = `-51`) | above `x = 0.1` the decay differs by up to ~2 % (`exp(-0.2)` = 0.8193 against the table's `exp(-11/51)` = 0.8061). Not tuned; the alignment factor is `dt * 2 * amount` and is routinely above 0.1 in a dive |

**A fidelity caveat that matters for M01.** The block is **quadratic in the
hit's amount** (`T += B * (T . B) * mag`, and `B` already carries the amount),
and the amount scales with `sqrt(|N|)` where the original's `N` magnitude is the
stored face's parallelogram area (`0x55d6d2`) while the port's collider normals
are **triangle** cross products of whatever trimesh the placement carries
(#2214 section 2). On a large water tile that is a much larger normal than a
unit-sized face, so this port's dive is much stronger than the original's would
be on unit faces; `2026-10-29-groundblow-world-segment-sweep.md` section 9
records that retail's face size is still unmeasured. Nothing is tuned here: the
formulas are the measured ones and the population is #2214's. The affected
content is any dive onto M01's large water/terrain triangles.

## 3. M01's owner smoke and the composed no-input flight

`docs/PLAYTEST.md`, "The owner-command smoke"
(`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir <dir>`),
on release builds of the branch's base commit `b5aa2c9c0` (the props/taxi sweep
landed) and of this branch, both with `--profile-dir` under
`private/evidence/2107-smoke/`:

```
before (base b5aa2c9c0): mission smoke: M01 — 81 checks, 0 failed, framebuffers 79
                         mission smoke: 4129 frames, 8082 ticks, 81 checks, failures: none   exit 0
after  (this branch):    mission smoke: M01 — 81 checks, 0 failed, framebuffers 79
                         mission smoke: 4128 frames, 8082 ticks, 81 checks, failures: none   exit 0
```

The scripted smoke does not change: no swept collider lies within the
hand-over pose's 400 m probe (#2214 section 1), so the fill hands the law
nothing at the mission start. The one-frame difference is the run loop's own
frame budget, not the flight (the tick count is the same).

The reviewer re-ran it on the rebased head (with #2224's AI probes live) and got
the same result: `81 checks, 0 failed, framebuffers 79`, `4128 frames, 8082
ticks, failures: none`, exit 0, captures in
`private/evidence/2107-review-smoke/` (not committed).

**The composed no-input flight does change once the nose meets the island.**
`crates/cs_app/tests/campaign/m01_collision.rs` flies it for 80 s from the
hand-over pose. Both columns were measured on that test on the reviewed tree;
the "before" column on `38c0bf7f4` (the rebased base, #2224's AI probe filled
but read by nothing), the "after" column with this branch:

| | before | after |
| --- | --- | --- |
| first judged terrain contact | t=41 (second at t=42) | t=42 (second at t=43) |
| speed at t=40 / t=42 | 100.0 m/s / after its t=41-42 contacts | 97.8 / 95.2 m/s |
| altitude t=40 / t=60 / t=80 | 162 / 428 / 746 m | 157 / 411 / 693 m |
| judged contacts and hull spent | 3 · 581 of 1200 | 2 · 267 of 1200 |

From t≈38 the probe meets the hillside ahead and the term moves the contact a
second later and halves the hull it spends; the post-impact climb is slightly
lower than before, not higher. The pin moved with it, as that test's own header
says a landing cause task does ("A cause task that lands moves the pins with
it"), and the test's earlier 455 hull pin is superseded: it predated the
props/objects colliders that landed before this branch's base, so the same
flight was already judged three times and spent 581 on `38c0bf7f4` (measured;
the ignored test does not run in CI, which is how the pin went stale). This is
*not* closer to the measured original, which is thrown to 373.9 m at t=40 and
1925 m at t=70; it is a pin of what main does, not evidence about the original.
Follow-up work: re-derive `docs/findings/2026-10-23-m01-collision-comparison.md`
with the term on, and settle retail's face size (section 2 above).

## 4. Tests

Prefix `accept_ground_blow_`, `crates/cs_sim/src/flight/original.rs` (unit
tests; they call `OriginalFlightModel::step`, so every assertion runs the
production path):

* `accept_ground_blow_amount_turns_the_velocity_toward_the_nose` — a head-on
  normal (`N` parallel to the body axis) gives `B = 0`, so the only difference
  is `f * v_hat + (1 - f) * nose` times the old speed, with `f = exp(-0.2)` at
  amount 10.
* `accept_ground_blow_player_torque_uses_the_accumulated_dot` — a pitch command
  puts `T = X * 0.033` before the block, `B = X * 15`, and the reported
  `world_torque` is the damped `T + B * 4.95` over `dt` (7066.02 about X).
* `accept_ground_blow_player_backfire_scales_the_negative_dot_and_drops_the_steering`
  — the mirrored face makes `T . B < 0`: `-0.05` of the torque and the identity
  velocity write.
* `accept_ground_blow_player_discards_a_vehicle_hit_off_its_path_only` — the
  gate's truth table, and a non-player keeping the same hit.
* `accept_ground_blow_ai_term_uses_the_ai_gain_and_the_same_steering` — the AI's
  `0.5 * 10` factor with no dot test, and its own steering.
* `accept_ground_blow_amount_keeps_the_unnormalised_normal_and_the_chord` — the
  amount read back from the steering (`amount = -ln(f) / (dt * 2)`) for
  `|N| = 100 / 400` and chords `200 / 100`, i.e. the `sqrt(area)` scale and the
  chord factor.
* `accept_ground_blow_no_hit_or_an_against_normal_changes_nothing` — 200 ticks
  with no hit, a zero normal and a normal pointing the wrong way, for both
  seats, are the plain step exactly.
* `accept_ground_blow_fake_player_skips_the_blow_and_a_fake_ai_does_not` — the
  call site's `V+0x384` gate.
* `accept_ground_blow_zero_velocity_keeps_the_torque_without_a_steer` — the
  torque is added before the zero-velocity guard.

Two #2210/#2214 tests pinned the previous "nothing consumes the probe"
invariant and were updated rather than weakened:

* `cs_sim`'s `accept_groundblow_probe_filled_probe_changes_no_trajectory`
  became `accept_ground_blow_no_hit_or_an_against_normal_changes_nothing`: a
  *firing* probe now moves the step, so the invariance that remains true is the
  one for a probe the block does not fire on.
* `cs_app`'s `accept_groundblow_fill_m01_trajectory_is_identical_with_the_fill_on`
  became `accept_groundblow_fill_m01_ground_blow_consumes_the_filled_probe`: the
  two lockstep M01s still have one difference (the driver's probe) and now
  diverge, while both still cast and count the overlay. Its sibling
  `accept_groundblow_fill_m01_fills_the_probe_from_the_production_world` now
  queries its record from the **placed** pose, because the law consumes the hit
  and the aircraft is no longer there four ticks later.
* `cs_app`'s `accept_parity4_m01_collision_no_input_trajectory_is_pinned` moved
  its pins to the trajectory section 3 tabulates. That test records what main
  does against the owner's #1752 table and says a cause task moves its pins; the
  ground blow is that cause for the hillside episodes.

## Instruction bytes cited

```bytes
call.skip: 48c22b: 83be5803000004570f842b0200003b3598c271007417d90570c47100d89eac000000dfe0f6c4010f850c020000  # 0x48c220: skip for V+0x358 == 4; the player compare; a non-player skipped while now < V+0xac
call.probe: 48c258: a1b8d175008b0dbcd175008b15c0d175008945e08d45d48dbe98010000894de48955e8894508897dfc8b5dfc8b4d088b038b53048b5b08350000008081f20000008081f30000008089018951048959088d4de0518d55d45256e8aafcffffd95d08a198c2710083c40c3bf07552  # 0x48c220: out = zero triple; axis = negated V+0x198; call 0x48bf60; player compare
blow.call.player_branch: 48c2c5: 8d45e08945f88b4d0c8b55f8d901d80ad94104d84a04d94108d84a08d9c9dec2dec1d95dfcd945fcd945fcd81dc8326000dfe0f6c4017412ddd8c7450800000000d945fcd80dac366000d80d24c47100eb31  # 0x48c220: d = dot(T,B); d<0 zeroes amount and scales d by -0.05; both branches then x mag
blow.call.ai_branch: 48c317: d90528c47100d80d24c47100d90570c47100d89eb4000000dfe0f6c4017412d80da8366000d94508d80da8366000d95d08  # 0x48c220: AI factor = ai_groundblow * mag; x0.15 on factor and amount while now < V+0xb4
blow.call.torque_add: 48c348: 8b450cd945e0d8c98bced800d918d945e4d8c9d84004d95804d945e8d8c9d84008d958088b16ddd8  # 0x48c220: T += B * factor
blow.call.velocity: 48c370: ff52048b08894dec8b50048955f08b40088d4dec518945f4e80363f9ffd81dc832600083c404dfe0f6c4400f85c3000000  # 0x48c220: vtable slot 1 -> velocity, normalise (0x422690), return when |v| == 0
blow.call.align: 48c3a1: 8b96340900008d45c889550c8945f8897dfc8b5dfc8b4df88b038b53048b5b08350000008081f20000008081f3000000808901895104895908d905bc2b6200d84d08518d4dc88d55ecd91c245152e80c43fdffd945ecd84d0ca06ef6640083c40c84c0d95decd945f0d84d0cd95df0d945f4d84d0cd95df475498b55ec8d86240900008bc889118b55f08951048b55f489510889450c  # 0x48c220: speed = V+0x934; nose = -V+0x198; call 0x460700(v_hat, nose, 2*amount); x speed; write V+0x924 unless [0x64f66e]
blow.call.speed: 48c437: 8b4d0cd901d809d94104d84904d94108d84908d9c9dec2dec1d95d08d94508d99630090000d9fad99e34090000  # 0x48c220: V+0x930 = |v|^2, V+0x934 = sqrt
decay: 460410: d9442404d81dc8326000dfe0f6c4417536d9442404d81da8346000dfe0f6c4017425d9442404d80dd47b6000d82de0326000d84c2404d825dc326000d84c2404d805dc326000c38b44240450e87fde0d0059c3  # 0x460410: f(x): x<=0 or x>=0.1 -> exp table; else 1 + x*((0.5 - 0.166666 x)x - 1)
lerp: 460700: d90544d79a0051d84c2410d91c24e8fdfcffff8b4424088b4c240cd900d82183c404d8c9d801d918d94004d86104d8c9d84104d95804d94008d86108d8c9d84108d95808ddd8c3  # 0x460700: a += (a - b) * f(dt * factor)
normalize: 422690: 558bec515356578b4d08d901dcc8d94104d94108dcc8d9c9dcc8d9c9dec2dec1d9fad955fcf745fcffffff7f7423d9e8def1d9c0d809d9c1d84904d9cad84908d9c9d919d9c9d95904d95908d945fceb05ddd8d945fc5f5e5b8be55dc3  # 0x422690: leaves a zero vector alone (|v| bits & 0x7fffffff == 0)
loader.elev: 474308: 686078620055e87d5d100083c4083bc6740e8b40048b480c890d20c47100eb0ac70520c471000000c842  # 0x474308: player.zrd "groundblow_elev" -> 0x71c420, default 100.0
loader.mag: 474332: 687078620055e8535d100083c4083bc6740d8b50048b420ca324c47100eb0ac70524c471000000c03f  # 0x474332: "groundblow_mag" -> 0x71c424, default 1.5
loader.ai: 47435b: 688078620055e82a5d100083c4083bc6740e8b48048b510c891528c47100eb0ac70528c471006666663f  # 0x47435b: "ai_groundblow" -> 0x71c428, default 0.9
suspend.set: 43db43: 528bcbe8c59d170084c0a26ef664000f8551080000  # 0x43db43: the "suspend" console command stores its flag in [0x64f66e]
```
