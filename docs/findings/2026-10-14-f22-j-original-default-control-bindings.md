# F22-J: Recover the original default control bindings

Date: 2026-10-14. Task: **F22-J** "Recover the original default control
bindings" (Rally #505). `specs/F22-input-bindings-devices-and-control-ownership.md`
lists F22-A…F22-D only; the F22-J follow-up is the task itself, so this
finding is its record. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) plus the
owner-supplied decrypted engine image (`CS_ENGINE_IMAGE`) and ordinary
build/test. No `gpu`, `audio`, `human_play`, `human_review` or `network_real`
was used or needed.

Owner path: `docs/findings/`. Production code: `crates/cs_types/src/input.rs`.
Acceptance tests: `crates/cs_content/tests/accept_f22_j_original_default_bindings.rs`
(+ `f22_j_support/`). Evidence copy: `docs/findings/evidence/F22-J.json`.

## Summary

The original 2000 PC game's default control bindings are **recorded**, 64
commands in 7 categories, each row citing the address of the `call` that
registers it:

1. **The table.** `0x4936c0`–`0x493d00` registers 64 commands through
   `0x493630(command_id, label_string_id, key_a, key_b, joystick_button,
   mouse_button)`. Every row of
   [`ORIGINAL_DEFAULT_BINDINGS`](../../crates/cs_types/src/input.rs) carries
   its own `registration_va` inside that span; the retail test re-decodes all
   64 calls from the image and matches them field for field.
2. **The encoding.** A packed `u32` per command: key A in bits 0–10, key B in
   bits 11–21, the 1-based DirectInput joystick button in bits 22–25 (0–10 are
   polled), the mouse button in bits 26–27 (none observed). A key code is a
   DirectInput scancode plus `0x100` Alt, `0x200` Ctrl, `0x400` Shift.
3. **The analog defaults.** `FlyMode` defaults to `2` (joystick/key),
   `MouseSens` to `1.3`; the joystick axes map X→roll (inverted), Y→pitch,
   Z-or-slider-0→throttle, Rz→yaw (inverted) and the hat to the look
   commands. A Japanese keyboard (`GetKeyboardType(0) == 7`) remaps three
   key codes at `0x493600`.
4. **The comparison.** Against `ActionMap::designed_default`, of the 26
   declared subjects 2 agree exactly (`throttle_idle`, `pause`), 1 shares an
   input (`fire_primary`), 13 differ, 4 have no original row
   (`drop_ordnance`, `toggle_gear`, `flap_step`, `countermeasure`) and 6 UI
   actions have no measured default row at all; 32 original commands the
   project declares nothing for are recorded as original-only.

**Status: static code evidence, claim `implemented`.** No original executable
ran, so nothing here is `verified_original`, and the project's default map
**stays `ActionMap::designed_default`** — explicitly designed, never
original-verified. Nothing in the recorded table is wired into the flight
path. The comparison records differences and changes nothing.

## What was read, and how (all read-only)

Nothing was written inside `$CS_GAME_DIR`. Only ids, symbols, codes,
addresses, spans, counts and SHA-256 digests leave the measurement: no
original bytes, no executable, no screenshot, no decompiled listing and no
original display text is committed (`AGENTS.md` rule 3, and the commit rules
of the Rally #505 owner note).

Sources:

- **The owner's bounded static analysis** of the decrypted `crimson.icd`
  image (Rally #505 owner note, 2026-10-05): the default table, the binding
  encoding, the modifier-matching and uniqueness rules, the analog defaults
  and the axis mapping.
- **This task's own bounded scan** of the same image, read-only through the
  production engine-image loader (`cs_content::coordinates::load_engine_image`,
  which refuses bytes that do not hash to the measured digest): the test-local
  scanner `decode_registrations` in
  `crates/cs_content/tests/f22_j_support/mod.rs` finds every `call 0x493630`
  inside the table span and decodes the six `push` operands before it;
  `decode_japanese_remap` decodes the three code pairs the Japanese-keyboard
  remap encodes; and the two analog defaults are read at their recorded
  `push` addresses. Every check reads the image at a recorded address (or
  scans a recorded span) and compares what it finds with a **recorded code,
  id, address or constant**; the sources carry no disassembly listing and no
  reproduced function body — only the x86 opcode forms a scanner needs to
  recognise each instruction. The retail acceptance test runs it again;
  all 64 rows agree with the recorded table.
- **Production readers** for the labels: `cs_assets::install::{discover,
  fingerprint, content_fingerprint, sha256}` over the installation and
  `cs_content::config::StringCatalog` over `strings.dll`, so every recorded
  label id resolves and every symbol is named with the id the `.data` table
  gives it.

Address convention (owner note): for `.text`, `.rdata` and `.data` below VA
`0x643000`, `file offset = VA − 0x400000`
(`ORIGINAL_IMAGE_VA_BIAS`).

## Fingerprints

| Item | Value |
| --- | --- |
| Installation fingerprint (`fingerprint`) | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| Canonical-content fingerprint (`content_fingerprint`) | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| Decrypted engine image (`CS_ENGINE_IMAGE`) | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, 2 580 480 bytes |
| Packed `crimson.icd` (F22-H) | `0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b`, 2 580 578 bytes |
| `strings.dll` (F22-H) | `7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21`, 131 072 bytes |

The decrypted image is the owner's decryption of `crimson.icd`, supplied at
`$CS_ENGINE_IMAGE` (outside the installation, so it does not change the
installation fingerprint). It is read-only and never committed.

## Cited spans

| What | Span / address |
| --- | --- |
| Controls table builder (64 registration calls) | `0x4936c0`–`0x493d00` |
| Registration function `0x493630(id, label, key_a, key_b, joy, mouse)` | `0x493630` |
| Packed binding store (getters `0x5370f0`/`0x537110`/`0x537130`/`0x537150` read it) | `0x537360` |
| Key-name builder (modifier prefixes + `GetKeyNameText`) | `0x5375f0` |
| Keyboard modifier-matching | `0x535ba0`–`0x535c3d` |
| Player flight input (joystick axes) | `0x487460` |
| `FlyMode` / `MouseSens` registration | `0x43ff61`–`0x43ffa7` (`FlyMode` push at `0x43ff7a`, `MouseSens` push at `0x43ff9d`) |
| Japanese-keyboard remap | `0x493600` |
| FlyMode input-mode flags | `0x440500` (0→1, 1→3, 2→5) |
| Temporary in-flight remap of commands 1/2 | `0x450550` (save/clear), `0x450e30` (restore) |

## The recovered default table (64 rows)

Screen order; key codes are the original's packed values (`0x000` = unbound);
the joystick button is 1-based, the mouse button column is `0` (none) for
every row. `—` marks an unbound key B slot.

| Cmd | Label id | Symbol | Category | Key A | Key B | Joy | Mouse | Registration VA |
|---|---|---|---|---|---|---|---|---|
| 1 | 11057 | MSG_NOSE_DOWN | Movement | `0x0c8` | — | 0 | 0 | `0x00493718` |
| 2 | 11058 | MSG_NOSE_UP | Movement | `0x0d0` | — | 0 | 0 | `0x0049372f` |
| 3 | 11059 | MSG_ROLL_LEFT | Movement | `0x0cb` | — | 0 | 0 | `0x00493746` |
| 4 | 11060 | MSG_ROLL_RIGHT | Movement | `0x0cd` | — | 0 | 0 | `0x00493760` |
| 5 | 11062 | MSG_RUDDER_LEFT | Movement | `0x033` | `0x052` | 0 | 0 | `0x00493774` |
| 6 | 11063 | MSG_RUDDER_RIGHT | Movement | `0x034` | `0x053` | 0 | 0 | `0x00493788` |
| 47 | 11061 | MSG_LEVEL_TOG | Movement | `0x426` | — | 0 | 0 | `0x004937a2` |
| 7 | 11067 | MSG_INC_THROTTLE | Throttle | `0x00d` | — | 0 | 0 | `0x004937cb` |
| 8 | 11068 | MSG_DEC_THROTTLE | Throttle | `0x00c` | — | 0 | 0 | `0x004937df` |
| 9 | 11069 | MSG_THROTTLE_0 | Throttle | `0x002` | — | 0 | 0 | `0x004937f6` |
| 10 | 11070 | MSG_THROTTLE_1 | Throttle | `0x003` | — | 0 | 0 | `0x0049380a` |
| 11 | 11071 | MSG_THROTTLE_2 | Throttle | `0x004` | — | 0 | 0 | `0x0049381e` |
| 12 | 11072 | MSG_THROTTLE_3 | Throttle | `0x005` | — | 0 | 0 | `0x00493835` |
| 13 | 11073 | MSG_THROTTLE_4 | Throttle | `0x006` | — | 0 | 0 | `0x00493849` |
| 14 | 11074 | MSG_THROTTLE_5 | Throttle | `0x007` | — | 0 | 0 | `0x0049385d` |
| 15 | 11075 | MSG_THROTTLE_6 | Throttle | `0x008` | — | 0 | 0 | `0x00493874` |
| 16 | 11076 | MSG_THROTTLE_7 | Throttle | `0x009` | — | 0 | 0 | `0x00493888` |
| 17 | 11077 | MSG_THROTTLE_8 | Throttle | `0x00a` | — | 0 | 0 | `0x0049389c` |
| 19 | 11045 | MSG_CMD_TRIGGER | Weapons | `0x039` | — | 1 | 0 | `0x004938c8` |
| 20 | 11086 | MSG_FIRE_MISSILE | Weapons | `0x02d` | — | 2 | 0 | `0x004938dc` |
| 25 | 11089 | MSG_CMD_CANNON_PREV | Weapons | `0x03d` | — | 0 | 0 | `0x004938f0` |
| 26 | 11090 | MSG_CMD_CANNON_NEXT | Weapons | `0x03e` | — | 7 | 0 | `0x00493907` |
| 32 | 11091 | MSG_CMD_MISSILE_PREV | Weapons | `0x03f` | — | 0 | 0 | `0x0049391b` |
| 33 | 11092 | MSG_CMD_MISSILE_NEXT | Weapons | `0x040` | — | 8 | 0 | `0x0049392f` |
| 36 | 10015 | MSG_CMD_TARGET_NEXT_ENEMY | Targeting | `0x012` | — | 3 | 0 | `0x0049395b` |
| 37 | 10016 | MSG_CMD_TARGET_PREVIOUS_ENEMY | Targeting | `0x412` | — | 0 | 0 | `0x00493972` |
| 38 | 10017 | MSG_CMD_TARGET_NEAREST_ENEMY | Targeting | `0x212` | — | 0 | 0 | `0x00493989` |
| 39 | 10018 | MSG_CMD_TARGET_NEXT_ALLY | Targeting | `0x011` | — | 0 | 0 | `0x004939a0` |
| 40 | 10019 | MSG_CMD_TARGET_PREVIOUS_ALLY | Targeting | `0x411` | — | 0 | 0 | `0x004939b7` |
| 41 | 10020 | MSG_CMD_TARGET_NEAREST_ALLY | Targeting | `0x211` | — | 0 | 0 | `0x004939ce` |
| 42 | 10021 | MSG_CMD_TARGET_NEXT_GROUND | Targeting | `0x013` | — | 6 | 0 | `0x004939e5` |
| 43 | 10022 | MSG_CMD_TARGET_PREVIOUS_GROUND | Targeting | `0x413` | — | 0 | 0 | `0x004939fc` |
| 44 | 10023 | MSG_CMD_TARGET_NEAREST_GROUND | Targeting | `0x213` | — | 0 | 0 | `0x00493a13` |
| 45 | 10009 | MSG_CMD_TARGET_UNDER_RETICULE | Targeting | `0x010` | — | 0 | 0 | `0x00493a2a` |
| 46 | 10008 | MSG_CMD_TARGET_NOTHING | Targeting | `0x014` | — | 0 | 0 | `0x00493a3e` |
| 48 | 11024 | MSG_CAM2_TOG | View1 | `0x41f` | — | 5 | 0 | `0x00493a6a` |
| 49 | 158 | MSG_LOOK_FORWARD | View1 | `0x042` | — | 4 | 0 | `0x00493a81` |
| 50 | 11020 | MSG_LOOK_DOWN | View1 | `0x043` | — | 0 | 0 | `0x00493a95` |
| 51 | 11021 | MSG_LOOK_BACK | View1 | `0x044` | — | 0 | 0 | `0x00493aa9` |
| 52 | 11022 | MSG_LOOK_LEFT | View1 | `0x057` | — | 0 | 0 | `0x00493ac0` |
| 53 | 11023 | MSG_LOOK_RIGHT | View1 | `0x058` | — | 0 | 0 | `0x00493ad4` |
| 54 | 143 | MSG_LOOK_FLYBY | View1 | `0x041` | — | 0 | 0 | `0x00493ae8` |
| 55 | 11025 | MSG_CMD_PADLOCK_SNAP | View1 | `0x025` | — | 0 | 0 | `0x00493aff` |
| 57 | 11036 | MSG_CMD_PADLOCK_STICK | View1 | `0x024` | — | 0 | 0 | `0x00493b13` |
| 56 | 11026 | MSG_CMD_PADLOCK_WATCH | View1 | `0x026` | — | 0 | 0 | `0x00493b27` |
| 58 | 11027 | MSG_PADLOCK_DL | View2 | `0x04f` | — | 0 | 0 | `0x00493b53` |
| 59 | 11028 | MSG_PADLOCK_D | View2 | `0x050` | — | 0 | 0 | `0x00493b67` |
| 60 | 11029 | MSG_PADLOCK_DR | View2 | `0x051` | — | 0 | 0 | `0x00493b7b` |
| 61 | 11030 | MSG_PADLOCK_L | View2 | `0x04b` | — | 0 | 0 | `0x00493b92` |
| 62 | 11031 | MSG_PADLOCK_M | View2 | `0x04c` | — | 0 | 0 | `0x00493ba6` |
| 63 | 11032 | MSG_PADLOCK_R | View2 | `0x04d` | — | 0 | 0 | `0x00493bba` |
| 64 | 11033 | MSG_PADLOCK_UL | View2 | `0x047` | — | 0 | 0 | `0x00493bd1` |
| 65 | 11034 | MSG_PADLOCK_U | View2 | `0x048` | — | 0 | 0 | `0x00493be5` |
| 66 | 11035 | MSG_PADLOCK_UR | View2 | `0x049` | — | 0 | 0 | `0x00493bf9` |
| 68 | 176 | MSG_CAM_ZOOM_OUT | View2 | `0x04a` | — | 0 | 0 | `0x00493c10` |
| 67 | 175 | MSG_CAM_ZOOM_IN | View2 | `0x04e` | — | 0 | 0 | `0x00493c24` |
| 106 | 11054 | MSG_CMD_LAUNCH_AUTO_LAND | Other | `0x01e` | — | 0 | 0 | `0x00493c4d` |
| 18 | 10010 | MSG_CMD_NITROUS | Other | `0x031` | — | 0 | 0 | `0x00493c64` |
| 34 | 11053 | MSG_CMD_BAIL_OUT | Other | `0x22d` | — | 0 | 0 | `0x00493c7b` |
| 69 | 11037 | MSG_CMD_PAUSE_GAME | Other | `0x001` | — | 0 | 0 | `0x00493c8f` |
| 35 | 11056 | MSG_CMD_DISPLAY_SCORES | Other | `0x00f` | — | 0 | 0 | `0x00493ca6` |
| 109 | 11087 | MSG_CHAT_ALL | Other | `0x029` | — | 0 | 0 | `0x00493cba` |
| 110 | 11088 | MSG_CHAT_TEAM | Other | `0x429` | — | 0 | 0 | `0x00493cd1` |
| 111 | 10025 | MSG_CMD_KEYMAP_DISP | Other | `0x03b` | — | 0 | 0 | `0x00493ce8` |

Notes on the table:

- Seven categories, in screen order, with header label ids 3005, 3008, 3006,
  3009, 3007, 3010, 3011 (movement, throttle, weapons, targeting, view 1,
  view 2, other).
- Commands 57 and 67 are registered before 56 and 68, so they sit above them
  on screen.
- No default row binds a mouse button. Joystick buttons 1–8 are pre-assigned
  as the `Joy` column shows.
- **Labels:** 60 of the 64 rows carry one of the 65 command labels F22-H
  measured from `strings.dll`. The four outside F22-H's measured ranges are
  `MSG_LOOK_FORWARD` (158), `MSG_LOOK_FLYBY` (143), `MSG_CAM_ZOOM_IN` (175)
  and `MSG_CAM_ZOOM_OUT` (176). Five of F22-H's 65 labels are **not** on the
  controls screen: `MSG_CMD_CYCLE_MODE` (10006), `MSG_CMD_INTERP` (10007),
  `MSG_CMD_ZONE_TOGGLE` (11040), `MSG_CMD_COLLISION_TOGGLE` (11041) and
  `MSG_MISSILE_NEXT` (11085). The acceptance test recomputes this split from
  F22-H's own list rather than asserting the five by hand.

## Binding encoding and rules

Per-command packed `u32` (getters `0x5370f0`/`0x537110`/`0x537130`/`0x537150`):

| Bits | Field |
| --- | --- |
| 0–10 | key A (scancode 0–7 + `0x100` Alt, `0x200` Ctrl, `0x400` Shift) |
| 11–21 | key B, same encoding |
| 22–25 | joystick button, 1-based DirectInput, `0` = none (buttons 1–10 polled: `0x537440`/`0x537530`) |
| 26–27 | mouse button: 1 left, 2 right, 3 middle, 0 none |

- **Modifier matching** (`0x535ba0`–`0x535c3d`): when a plain key is bound
  *and* some modifier combination of it is also bound, the full code with the
  currently held modifiers must match exactly; otherwise held modifiers are
  ignored. The recorded table's own chords are the ten codes `0x211`, `0x212`,
  `0x213`, `0x22d`, `0x411`, `0x412`, `0x413`, `0x41f`, `0x426`, `0x429`.
- **Uniqueness:** each slot has its own reverse map (manager offsets `+0x18`,
  `+0x1f90`, `+0x3f08`, `+0x3f48`); assigning an input clears it from any
  other command in that slot (`0x5371d0`/`0x537230`/`0x5372a0`/`0x537300`),
  and the rebind UI (`0x405900`) applies the same rule across rows.
- **Japanese keyboards** (`GetKeyboardType(0) == 7`) remap three codes at
  `0x493600`: `0x0d → 0x90`, `0x29 → 0x7d`, `0x429 → 0x47d`.
- **Temporary remap:** while an in-flight flag (`0x654120`) is set,
  `0x450550` saves and clears the bindings of commands 1 and 2 and binds
  commands 100/101 to the mouse buttons; `0x450e30` restores them. Recorded,
  not exercised.

The encoding and these rules are the **owner's static analysis**, not runtime
observations; this task verified the *bytes* the recorded table derives from
(the 64 registration calls, the two default immediates, the remap encoding),
not the behaviour they produce.

## Analog and device defaults

- **`FlyMode` default `2`** (`0x43ff7a`): 0 keyboard-only, 1 mouse/keyboard,
  2 joystick/key. `0x440500` maps it to input-mode flags 0→1, 1→3, 2→5
  (bit 2 = joystick axes, bit 1 = mouse). Without a joystick, mode 2 behaves
  as keyboard-only (`0x418d30`).
- **`MouseSens` default `1.3`** (`0x43ff9d`).
- **Joystick axes** (`0x487460`, device setup `0x536590`):

  | Axis | Flight axis | Inverted | Condition |
  | --- | --- | --- | --- |
  | X | roll | yes | — |
  | Y | pitch | no | — |
  | Z (else slider 0) | throttle | no | device reports ≥ 3 axes; `0.5 − 0.5·normalised`, clamped `0..1` |
  | Rz | yaw | yes | device reports ≥ 4 axes |
  | Hat (POV 0) | none (drives the look commands) | no | centred hat falls back to the numpad look commands 58–66 (`0x42d010`) |

  A gamepad is a DirectInput joystick with the same mapping; there is no
  separate gamepad path.
- **Keyboard/mouse into the flight model** (`0x487460`): commands 1/2 drive
  pitch, 3/4 roll, 5/6 yaw; 7/8 step the throttle and 9–17 set it to
  `0, 0.125 … 1.0`. In mouse/keyboard mode the mouse adds to pitch (Y) and
  roll (X), with a third channel driving yaw (swapped for airframes with flag
  `+0x21c`).

## Comparison with `ActionMap::designed_default`

`cs_types::input::compare_with_original_default` walks every declared
`FlightCommand` and `UiAction` once, then every original command the project
declares nothing for. A designed `Key` source matches an original row when
`original_scancode` equals one of that row's key codes; mouse, gamepad and
joystick-button sources never match (the original's joystick field is 1-based
and its table binds no mouse button, so the index conventions are not assumed
to line up).

| Subject | Agreement | Designed default sources | Original default row(s) |
|---|---|---|---|
| `pitch` | different | `key.w`, `key.s`, `mouse_axis.y`, `gamepad_axis.left_stick_y`, `joystick_axis.1` | `cmd 1 label 11057 key_a 0x0c8 key_b - joy 0 mouse 0`<br>`cmd 2 label 11058 key_a 0x0d0 key_b - joy 0 mouse 0` |
| `roll` | different | `key.q`, `key.e`, `gamepad_axis.left_stick_x`, `joystick_axis.0` | `cmd 3 label 11059 key_a 0x0cb key_b - joy 0 mouse 0`<br>`cmd 4 label 11060 key_a 0x0cd key_b - joy 0 mouse 0` |
| `yaw` | different | `key.a`, `key.d`, `mouse_axis.x`, `gamepad_axis.right_stick_x` | `cmd 5 label 11062 key_a 0x033 key_b 0x052 joy 0 mouse 0`<br>`cmd 6 label 11063 key_a 0x034 key_b 0x053 joy 0 mouse 0` |
| `throttle` | different | `gamepad_axis.right_trigger` | `cmd 7…17` (eleven rows, key A `0x00d`, `0x00c`, `0x002`…`0x00a`) |
| `fire_primary` | partial | `key.space`, `mouse_button.left`, `gamepad_button.south`, `joystick_button.0` | `cmd 19 label 11045 key_a 0x039 key_b - joy 1 mouse 0` |
| `fire_secondary` | different | `key.left_control`, `gamepad_button.east`, `joystick_button.1` | `cmd 20 label 11086 key_a 0x02d key_b - joy 2 mouse 0` |
| `cycle_weapon` | different | `key.tab` | `cmd 25, 26, 32, 33` (key A `0x03d`–`0x040`; joy 7 and 8) |
| `drop_ordnance` | project_only | `key.z` | — |
| `eject` | different | `key.digit3` | `cmd 34 label 11053 key_a 0x22d key_b - joy 0 mouse 0` |
| `toggle_gear` | project_only | `key.g` | — |
| `flap_step` | project_only | `key.b` | — |
| `target_next` | different | `key.t`, `key.arrow_right` | `cmd 36, 39, 42` (key A `0x012`, `0x011`, `0x013`; joy 3 and 6) |
| `target_prev` | different | `key.arrow_left` | `cmd 37, 40, 43` (key A `0x412`, `0x411`, `0x413`) |
| `countermeasure` | project_only | `key.x` | — |
| `throttle_step_up` | different | `key.r` | `cmd 7 label 11067 key_a 0x00d key_b - joy 0 mouse 0` |
| `throttle_step_down` | different | `key.f` | `cmd 8 label 11068 key_a 0x00c key_b - joy 0 mouse 0` |
| `throttle_idle` | **same** | `key.digit1` | `cmd 9 label 11069 key_a 0x002 key_b - joy 0 mouse 0` |
| `throttle_full` | different | `key.digit4` | `cmd 17 label 11077 key_a 0x00a key_b - joy 0 mouse 0` |
| `level_off` | different | `key.l` | `cmd 47 label 11061 key_a 0x426 key_b - joy 0 mouse 0` |
| `confirm` | runtime_only | `key.enter` | — |
| `cancel` | runtime_only | `key.backspace` | — |
| `navigate_up` | runtime_only | `key.arrow_up` | — |
| `navigate_down` | runtime_only | `key.arrow_down` | — |
| `navigate_left` | runtime_only | `key.arrow_left` | — |
| `navigate_right` | runtime_only | `key.arrow_right` | — |
| `pause` | **same** | `key.escape` | `cmd 69 label 11037 key_a 0x001 key_b - joy 0 mouse 0` |

Counts: 2 `same`, 1 `partial`, 13 `different`, 4 `project_only`,
6 `runtime_only`, plus 32 `original_only` commands — 58 rows in total.

The 32 original-only command ids (the project declares nothing for them):
`18, 35, 38, 41, 44, 45, 46, 48–68 (all of them), 106, 109, 110, 111` —
spelled out: 18, 35, 38, 41, 44, 45, 46, 48, 49, 50, 51, 52, 53, 54, 55, 56,
57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 106, 109, 110, 111.

**The project default is unchanged.** The comparison is a record, not a
migration: `ActionMap::designed_default` keeps its own bindings (for example
the designed `level_off` slot is the bare letter while the original's row is
the chord `0x426`), and the acceptance test asserts the map still equals
`ActionMap::designed_default()` after the comparison runs. A `Same` or
`Partial` row says the two defaults share an input, not that the project's
map is original-verified.

## Relationship to F22-H

`docs/findings/2026-10-02-f22-h-original-control-vocabulary.md` measured the
control vocabulary from shipped files and recorded every default binding as
unknown, because the defaults live in the packed executable. This finding
closes exactly that gap with the owner-approved static analysis (Rally #505
owner note, 2026-10-05) plus this task's bounded read-only scan of the
decrypted image. The runtime questions F22-H listed remain unknown and are
repeated below.

The two findings count different things and their "no counterpart" numbers
are not expected to match: F22-H counted 33 measured **labels** with no
project counterpart, this task records 32 original **default rows** the
project declares nothing for. Five measured labels have no default row at
all, and four default rows carry labels outside F22-H's measured ranges.

## Unknowns (not guessed, not removed)

1. Which key, joystick button or mouse control actually fires which command
   in the **running game**, what the original does while text is entered and
   what it does on focus loss are runtime questions; they need an
   owner-supplied original run (task #358 `REF-OWNER-FIRST-CAPTURE`).
   `human_play` is never available to an agent.
2. The binding **encoding** itself — which argument is key A, the 11-bit
   modifier layout, the modifier-matching rule and the per-slot uniqueness
   rule — is the owner's static analysis, not a runtime observation. This
   task verified the recorded rows against the image bytes, not the
   behaviour those bytes produce.
3. The **joystick axis mapping** and the `FlyMode` input-mode flags are the
   owner's static analysis over `0x487460` and `0x440500`; the temporary
   in-flight remap (`0x450550`/`0x450e30`) is recorded but not exercised.
4. The project's own default map is unchanged and stays **designed**: the
   comparison records where the two defaults differ, and none of the
   original's bindings was wired into the flight path.

These four are the report's `unknowns` array. They are kept deliberately:
`tools/validate_evidence.py --require-pass` exits 3 on them, and deleting
them to turn that flag green would be exactly the shortcut the owner
directive forbids. The structural check (`--artifact-root`, no
`--require-pass`) passes.

## Tests

`cargo test --workspace --locked -- accept_f22_j_ --include-ignored`
discovers 8 tests, all passing (6 unignored, so CI runs them without original
data; 2 retail, `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`):

| Test | What it pins |
| --- | --- |
| `accept_f22_j_the_recorded_table_matches_the_production_artifact` | the production `ORIGINAL_DEFAULT_BINDINGS` equals `MEASURED_ROWS` field for field, plus ordering, uniqueness and range checks; removing the table fails to compile |
| `accept_f22_j_the_screen_enumerates_the_measured_labels_exactly` | the 60/4/5 label split, recomputed against F22-H's list |
| `accept_f22_j_the_analog_defaults_and_remap_are_the_recorded_ones` | `FlyMode` 2, `MouseSens` 1.3, the axis mapping and the three remap pairs |
| `accept_f22_j_the_recorded_chords_match_the_scancode_crosswalk` | the ten chord codes and the project's scancode crosswalk where both name the same key |
| `accept_f22_j_the_remap_decoder_reads_what_it_is_given` | the remap decoder against a synthetic chain with foreign codes (and against an image that holds no chain), so CI catches a decoder that stopped reading the image |
| `accept_f22_j_the_designed_default_comparison_is_recorded` | every declared subject and original-only command is compared once, with the recorded agreement for each |
| `accept_f22_j_retail_the_image_registers_the_recorded_defaults` (ignored) | production discovery + fingerprint, the engine-image loader's digest, the 64 decoded registration calls, the decoded Japanese-remap code pairs and the two default immediates |
| `accept_f22_j_retail_strings_dll_names_every_recorded_label` (ignored) | every recorded label id resolves through `StringCatalog`, and every symbol is named with its id |

The evidence harness `crates/cs_content/tests/evidence_report_f22_j.rs` is
**not** named `accept_f22_j_*`: it is not part of the acceptance suite, needs
`CS_EVIDENCE_DIR`, `CS_CANDIDATE_TREE`, `CS_EVIDENCE_ARGV`,
`CS_EVIDENCE_EXIT_CODE`, `CS_EVIDENCE_REVIEWER`, `CS_GAME_DIR` and
`CS_ENGINE_IMAGE`, and fails loudly when they are missing. It re-runs the
production discovery, the image loader and the label reader, and writes the
report; it also checks that the report and its `bindings.json` artifact are
structurally JSON outside their string literals, which is how two rendering
defects (an unquoted `0x…` address and an unquoted `declared:` subject
prefix) were caught before this report was produced.

## Evidence

`private/evidence/F22-J/` (Git-ignored) holds `cargo-test.log`,
`bindings.json` and `acceptance.json`; a copy of the report is committed as
`docs/findings/evidence/F22-J.json`. Claim: **`implemented`**, capability
`retail`, 8/8 tests passing, 8 assertions.

```sh
cargo test --workspace --locked -- accept_f22_j_ --include-ignored \
  2>&1 | tee private/evidence/F22-J/cargo-test.log

CS_EVIDENCE_DIR=private/evidence/F22-J \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f22_j_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_content --test evidence_report_f22_j -- --ignored

python3 tools/validate_evidence.py private/evidence/F22-J/acceptance.json \
  --artifact-root private/evidence/F22-J
# → {"structurally_valid": true, "artifact_count": 2, "claims_semantically_verified": false}
python3 tools/validate_evidence.py private/evidence/F22-J/acceptance.json \
  --artifact-root private/evidence/F22-J --require-pass
# → exit 3, "Unresolved issues": the four unknowns above, kept on purpose
```

`bindings.json` is the table, the spans, the analog defaults, the label
resolution (symbol, id and the number of code units the reader decoded — not
the text itself) and the recorded comparison; `artifacts` in the report
reference it by SHA-256 only.

## Checks

Run locally by the implementer before hand-over, and re-run by the reviewer
after the review fix of 2026-10-14, on the branch head. The reviewer is the
same agent identity that implemented the task (`bunny-alpha-2`), in a fresh
session — self-review, recorded as such in the review hand-over, not
independent evidence (all exit 0):

```sh
cargo fmt --all -- --check                                        # exit 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
                                                                   # exit 0
cargo test --workspace --locked                                   # exit 0,
                                                                   # 562 ok results
cargo test --workspace --locked -- accept_f22_j_ --include-ignored
                                                                   # exit 0, 8/8
```

Each of the 8 discovered `accept_f22_j_` tests also passes alone with
`--exact --include-ignored`. CI runs the same first three checks on the
pushed commit without original data; the two retail tests are `#[ignore]`
there and were run locally.

The reviewer's one change to this branch: the retail test asserted the
Japanese-remap function against a **reconstructed byte sequence of the
original's code**, which the task's own commit rules forbid. It now decodes
the three code pairs out of the image and asserts them against the recorded
pairs, plus a new CI test that exercises the decoder against a synthetic
chain with foreign codes. Everything else in the branch was reviewed and
kept.

## Research boundary

- Read-only: `$CS_GAME_DIR` was never written to, and the decrypted image at
  `$CS_ENGINE_IMAGE` was only read.
- Committed: ids, symbols, codes, addresses, spans, counts, digests and
  project-authored analysis. Not committed: original bytes, the executable,
  screenshots, decompiled listings and original display text.
- Claim level: `implemented`. A Rally merge awards `checked` only;
  `verified_original` needs an original run, which no agent can produce.
