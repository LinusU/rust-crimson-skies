# The Level-Off assist is cleared by stick input, and the port now does it (#2298)

Date: 2026-11-03. Task #2298, follow-up of #2296 §11
(`2026-11-02-m01-wingman-gains.md`). Implementer: `deepseek-1`.

Static claims are `static`, read from the owner image with
`llvm-objdump -d -M intel`; retail claims are reads of `$CS_GAME_DIR`. The
original executable was **not** run: nothing here is `verified_original`, and
nothing here measures how the original behaves. Image sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, address
convention `VA = file offset + 0x400000`, claim `OWNER-STATIC-2026-10-29`.

## 1. What was wrong

#2296 §11 decoded the Level-Off assist's per-vehicle byte `vehicle+0x12c`:
the player-vehicle `Shift+L` handler sets it and the vehicle constructor clears
it, and the assist's term inside the force block runs only while the byte is
set **and** both stick inputs it tests are zero (`0x48ce53`, `0x48ce6a`).

It also recorded, as an unfixed divergence, that the original *clears* the byte
from the **player input routine** on any stick input, while the port kept the
flag set and only suppressed the term inside the step. So after a player
deflected the stick and returned it to centre the original's assist was **off**
(it needed a second `Shift+L`) and the port's was **on**. This task closes that
divergence for the player's seat.

## 2. The clear (`0x487d3d`..`0x487d96`)

The player input routine loads the three raw stick words and compares each
against the image's `0.0` at `0x6032c8`:

| window | bytes | effect |
| --- | --- | --- |
| `0x487d3d` | `d9 86 00 01 00 00` | `fld [esi+0x100]` — the roll word |
| `0x487d46` | `d8 1d c8 32 60 00` | `fcomp [0x6032c8]` — against `0.0` |
| `0x487d4e` | `f6 c4 40` | `test ah, 0x40` — the C3 (equal) bit |
| `0x487d51` | `74 43` | `je 0x487d96` — non-zero → clear |
| `0x487d53` | `d9 86 08 01 00 00` | `fld [esi+0x108]` — the pitch word |
| `0x487d59` | `d8 1d c8 32 60 00` | `fcomp 0.0` |
| `0x487d61` | `f6 c4 40` | `test ah, 0x40` |
| `0x487d64` | `74 30` | `je 0x487d96` |
| `0x487d66` | `d9 86 0c 01 00 00` | `fld [esi+0x10c]` — the yaw word |
| `0x487d6c` | `d8 1d c8 32 60 00` | `fcomp 0.0` |
| `0x487d74` | `f6 c4 40` | `test ah, 0x40` |
| `0x487d77` | `74 1d` | `je 0x487d96` |
| `0x487d96` | `c6 86 2c 01 00 00 00` | `mov byte [esi+0x12c], 0` |

So **any** of the three stick words being non-zero clears the byte, in the same
frame the input reaches the law — not only the roll and pitch words the term
itself gates on.

The three words are the *raw* stick. Each is copied through the response curve
`0x460890` (`d9 44 24 04 c3`, `fld [esp+4]; ret` — the identity) into the force
block's inputs the term reads (`0x487db0` `d9 9e 14 01 00 00` → `+0x114`,
`0x487dcc` `d9 9e 1c 01 00 00` → `+0x11c`, `0x487de8` `d9 96 20 01 00 00` →
`+0x120`). With an identity curve a non-zero raw word and a non-zero force-block
word are the same event, which is why the port can apply the clear to the
`OriginalInput` words it already carries.

## 3. Who writes the byte (`0x488640`..`0x48866e`, `0x4b0645`)

The clear never reaches an AI body, because no AI body ever has the byte set:

| window | bytes | effect |
| --- | --- | --- |
| `0x488640` | `8b 0d 98 c2 71 00` | `mov ecx, [0x71c298]` — the **player vehicle** global |
| `0x488663` | `8a 81 2c 01 00 00` | `mov al, [ecx+0x12c]` |
| `0x48866b` | `0f 94 c0` | `sete al` — `!old` |
| `0x48866e` | `88 81 2c 01 00 00` | `mov [ecx+0x12c], al` — the `Shift+L` toggle |
| `0x4b0645` | `88 9e 2c 01 00 00` | the constructor/reset clearing it beside the control block |

`0x71c298` is the player vehicle, so the toggle cannot arm any other body, and
the constructor starts every body with the byte clear. The assist's own gate is
unchanged: `0x48ce45` `8a 86 2c 01 00 00` reads the byte, `0x48ce53` /
`0x48ce64` gate on `+0x114 == 0` and `0x48ce6a` / `0x48ce7b` on `+0x11c == 0`.

## 4. What changed in the port

`crates/cs_sim/src/flight/original.rs`, `OriginalFlightModel::step`: before the
step's first term, while `input.is_player` and any of
`input.roll`/`input.pitch`/`input.yaw` is non-zero, `state.level_off` is set to
`false`. The clamped words are the ones the force block would read, so a
clamped-to-`±1` command still counts as input, as the original's non-zero raw
word does.

The AI seat is untouched: `is_player == false` never clears the flag (and no AI
body has it set). The term's gate inside `rotation_increment`
(`level_off && input.roll == 0.0 && input.pitch == 0.0`) stays exactly as §11.3
reads it, so the assist still cannot run with a deflected stick.

Consequences a player sees, all following from the clear:

* arming the assist and then using the stick leaves it **off** when the stick
  returns to centre; it needs a second `Shift+L` — the toggle — to arm it again;
* pressing `Shift+L` while the stick is deflected arms it for that instant and
  the same frame's input clears it again, which is the original's order of
  operations (the input routine runs every frame, the toggle only on the press);
* a hands-off step never changes the flag, so an armed assist stays armed.

## 5. Tests

* `crates/cs_sim/tests/accept_t2298_level_off_clears_on_stick_input.rs` — the
  behaviour: the clear on each of the three words in the same step, that the
  cleared state is the unarmed state, that a hands-off step neither clears nor
  re-arms, that the re-armed assist still levels a 30-degree bank while the
  cleared one keeps it, and that the AI seat is never cleared. Removing the
  clear fails two of them.
* `crates/cs_app/tests/accept_t2298_level_off_input_windows.rs` — re-reads every
  window in §2 and §3 from the image (`requires CS_ENGINE_IMAGE`).
* `crates/cs_app/tests/playtest_retail_launch.rs`,
  `accept_flight_original_levelop_bound_key_toggles_the_retail_level_off` — the
  retail path: the bound key arms the assist, the first deflected frame clears
  it, the deflected aircraft banks away, centring the stick does not re-arm it,
  and a second `Shift+L` with the stick centred levels a fresh 30-degree bank.
  Its earlier "assist on but the stick deflected" step asserted the flag stayed
  set; this task corrects it to the image's behaviour (a correction, not a
  weakened assertion: the test now asserts the flag, not just the bank).
* The law's own unit test
  `accept_flight_original_levelop_torque_path_runs_only_when_on_and_hands_off`
  now also asserts the on case ends the deflected step with the flag clear.

## 6. Not covered

* `vehicle+0xf4` (`0x487d83`..`0x487d94`) also clears the byte hands-off when
  its value is `> 0`, and gates the toggle (`0x488650`..`0x488661`). Its writers
  and meaning are still unknown (#2296 §11.6); the port does not model it.
* The assist's torque maths, the AI seat and the second AI law are untouched.
* Nothing here is calibrated against an original run (#358); no screenshot,
  sound or ordinary-play claim follows from a static window.
