# F22-H: Measure the original game's control vocabulary and bindings

Date: 2026-10-02. Task: **F22-H** "Measure the original game's control
vocabulary and bindings" (`specs/F22-input-bindings-devices-and-control-ownership.md`,
which lists no `### F22-H` section; the task itself is the only description,
see Rally #411). Shared contract: `docs/contracts/CLI-EVIDENCE.md`. Capabilities
used: **`retail`** (read-only `$CS_GAME_DIR`) plus ordinary build/test. No
`gpu`, `audio`, `human_play`, `human_review` or `network_real` was used or
needed.

Owner path: `docs/findings/`. The acceptance tests live in
`crates/cs_content/tests/` and the evidence copy in `docs/findings/evidence/`.

## Summary

The original 2000 PC Crimson Skies keeps its control vocabulary in two shipped
files and one script container, all readable without running the game:

1. **`strings.dll`** — its `RT_STRING` resource tree holds every label, and its
   PE `.data` section holds a `{name, id}` table (stored twice, identically)
   that names **1 023 labels**, including **all 65 command labels** the control
   UI enumerates, the 7 control-category headers, the **2 device-mode labels**
   and all 44 key/button labels.
2. **`crimson.icd`** — the packed original game executable — carries a
   plaintext pool of **120 `key_*` names**, the DirectInput keyboard names the
   original recognises.
3. **`GOSDATA/ASSETS/crimson.rof`** — the control UI scripts `CTL.SCRIPT`,
   `KEYS.SCRIPT` and `CONTROLSPREFS.SCRIPT` describe the rebinding screens and
   fetch every command row and its current binding through **native callbacks**
   (`2115`, `2120`, `2139`, `2140`, …).

The **vocabulary is observable; the default bindings are not.** The ordered
command list and the binding each command currently holds are produced by code
in the packed executable, so every default binding is recorded as **unknown**,
not guessed. Which physical key fires which command in the running game, what
text entry suppresses and what focus loss does are runtime questions that need
an owner-supplied original run (#358 `REF-OWNER-FIRST-CAPTURE`).

## What was read, and how (all read-only)

Nothing was written inside `$CS_GAME_DIR`. Only ids, symbolic names, byte
offsets, counts and SHA-256 digests leave the measurement; no original display
text is reproduced here or committed.

Read through **production** readers (`crates/`):

- `cs_assets::install::{discover, fingerprint, content_fingerprint, sha256}`
  over the whole installation;
- `cs_content::config::StringCatalog` over `strings.dll` (`RT_STRING` tree);
- `cs_formats::{read_tree, read_member}` over
  `GOSDATA/ASSETS/crimson.rof`.

Read by a **test-local** parser in
`crates/cs_content/tests/f22_h_support/mod.rs`, because no production reader
owns a PE `.data` section: the `{pointer, id}` table in `strings.dll`'s `.data`
and the `key_*` name pool in `crimson.icd`. Every id the test-local parser
returns is then resolved through the production `StringCatalog`, so a wrong
parse fails the acceptance test instead of merely agreeing with itself.

## Fingerprints

| Item | Value |
| --- | --- |
| Installation fingerprint (`fingerprint`) | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| Canonical-content fingerprint (`content_fingerprint`) | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| `strings.dll` | `7582fecaca42d21dd44eb95f896dcb415af790c7057ad81c8e1600ac0b445c21`, 131 072 bytes |
| `crimson.icd` | `0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b`, 2 580 578 bytes |
| `GOSDATA/ASSETS/crimson.rof` | 846 members |

`crimson.icd` is the packed original game executable: its version resource
carries `ProductName "Microsoft Crimson Skies"` and `OriginalFilename`
`"Crimson.exe"`, and the payload begins with the packed `InternalName
"Crimson"`. It imports only `KERNEL32`, `USER32`, `ADVAPI32` and `VERSION`;
direct input reading is not done by that image.

## Observation 1 — `strings.dll` carries the named control vocabulary

### The `{name, id}` table

`strings.dll` is PE32 (ImageBase `0x10000000`) with a `.data` section at file
offset `0x6000` (`VirtualAddress 0x6000`, raw size `0xC000`). Inside it is a
`{u32 name_pointer; u32 string_id}` table, stored **twice and identically**:

- first run: file offset `0x6030`–`0x8027`, 1 023 records;
- second run: file offset `0xcb88`–`0xeb87`, 1 023 records.

A name pointer is an ImageBase-relative VA into the image's own string pool;
the id is an `RT_STRING` id. Deduplicating the two runs yields **1 023 names**
with ids from **100** to **17 142**. One further printable pair in the scan,
`PDT` with id 0, is not an `RT_STRING` label (`RT_STRING` ids start at 1 and no
other record is below 100), so it is not part of the vocabulary.

Representative spans (name offset is in the image, record offset is in the
first table run):

| Symbol | id | name offset | record offset |
| --- | --- | --- | --- |
| `MSG_CMD_TRIGGER` | 11045 | `0x0c4b4` | `0x06350` |
| `MSG_NOSE_UP` | 11058 | `0x0c1d8` | `0x06460` |
| `MSG_ROLL_LEFT` | 11059 | `0x0c1c8` | `0x06468` |
| `MSG_RUDDER_LEFT` | 11062 | `0x0c198` | `0x06480` |
| `MSG_THROTTLE_0` | 11069 | `0x0c14c` | `0x064a0` |
| `MSG_FIRE_MISSILE` | 11086 | `0x0c0a4` | `0x064f0` |
| `MSG_KEYBOARD` | 1023 | `0x0ba90` | `0x067b0` |
| `MSG_MOVEMENT_CONTROLS` | 3005 | `0x0bdb8` | `0x06638` |

### The command labels (the observable vocabulary)

The control UI enumerates two command-label id ranges: `10 000…10 030` and
`11 000…11 100`. They hold **15** and **50** non-empty `RT_STRING` labels, and
**every one of those 65 labels has a symbolic name** in the `.data` table. It
follows that a control the original exposes cannot hide behind an anonymous
label: either its name is in the list below or the original does not expose it.

The complete measured list is the `COMMAND_LABELS` constant in
`crates/cs_content/tests/f22_h_support/mod.rs` (65 entries). The comparison
table later in this finding names the 32 labels that correspond to a command
this project also declares; the other 33 have no project counterpart and are
listed on their own below.

### Control categories and wingman orders

- The 7 category headers are `MSG_MOVEMENT_CONTROLS` (3005),
  `MSG_WEAPON_CONTROLS` (3006), `MSG_VIEW1_CONTROLS` (3007),
  `MSG_THROTTLE_CONTROLS` (3008), `MSG_TARGETING_CONTROLS` (3009),
  `MSG_VIEW2_CONTROLS` (3010) and `MSG_OTHER_CONTROLS` (3011). The dialog title
  `MSG_DLG_CONTROLS` (142) is not a list header.
- Four wingman-order labels sit just below the categories:
  `MSG_WINGMAN_ENGAGE` (3001), `MSG_WINGMAN_FIRE` (3002),
  `MSG_WINGMAN_BKT_LEFT` (3003), `MSG_WINGMAN_BKT_RIGHT` (3004).

### Device modes and bindable sources

The original offers exactly **two device-mode labels**:

| Symbol | id | Meaning |
| --- | --- | --- |
| `MSG_KEYBOARD` | 1023 | keyboard mode |
| `MSG_KEYBOARD_JOYSTICK` | 1024 | keyboard + joystick mode |

There is **no gamepad mode label**. Bindable non-keyboard sources are named
separately: `MSG_JOYBTN` (15002) and `MSG_MOUSEBTN` (15003) are the button
headings, with ten joystick buttons `MSG_JBTN_1…MSG_JBTN_10` (15129–15138) and
three mouse buttons `MSG_MBTN_LEFT/RIGHT/MIDDLE` (15139–15141). `MSG_KEYA`
(15000) and `MSG_KEYB` (15001) are the two letter-key placeholders. This is a
**two-mode, keyboard-centric** vocabulary: joystick and mouse are sources
inside those modes, not separate families.

### Key and button labels

The `15 000…15 141` range holds **44** non-empty labels, all 44 named: the four
device sources above, 27 special-key labels (numpad, backslash, kana/convert,
Windows keys, …; e.g. `MSG_KEY_NUMPAD0` 15093, `MSG_KEY_LWIN` 15126,
`MSG_KEY_APPS` 15128) and the 13 button labels. The full list is the
`KEY_LABELS`, `BUTTON_LABELS`, `DEVICE_SOURCES` and `DEVICE_FAMILIES` constants
in the support module.

### `RT_STRING` accounting (`cs_content::config::StringCatalog`)

The production reader reports **112** `RT_STRING` blocks, **1 792** string
units, one language **1033**, **2** other (non-`RT_STRING`) leaves, **0**
undecodable units and **0** duplicate ids. This is what makes an id a reliable
identity: no command label is ambiguous.

## Observation 2 — `crimson.icd` carries the `key_*` name pool

`crimson.icd` holds **120** distinct plaintext `key_*` names spanning file
offsets `0x20ce10`–`0x20d30a`. They are the DirectInput keyboard constants
lowercased (`key_lshift`, `key_rshift`, `key_lcontrol`, `key_rcontrol`,
`key_return`, `key_escape`, `key_numpad0`, `key_f12`, `key_space`, `key_tab`,
`key_lwin`, `key_rwin`, `key_apps`, …). The pool starts at `key_pause` and ends
at `key_escape`. These names are the original's own keyboard vocabulary and are
distinct from the `MSG_KEY_*` labels above (the labels name the keys a user
sees; the `key_*` names are the executable's constants).

## Observation 3 — the control UI scripts and their native callbacks

The three control scripts decode cleanly through the production container
reader; `read_member` reports no trailing bytes and the decoded digests are:

| Member | decoded length | SHA-256 |
| --- | --- | --- |
| `ASSETS/SCRIPTS/CTL.SCRIPT` | 27 465 | `8ca5a9439a2c22cb63fca166f6f6403e08b62dc30626bd32096c772d546ecb95` |
| `ASSETS/SCRIPTS/KEYS.SCRIPT` | 4 552 | `abb5a703819017643587f214177cd7815b6a750e1fa89c53691cd89ebf012d29` |
| `ASSETS/SCRIPTS/CONTROLSPREFS.SCRIPT` | 1 385 | `360dbe9d4f4d614310ce4db823aad577d5b7c323e36aa2be739ad2c4e327157d` |

`CTL.SCRIPT` itself handles ten special keys by name (`key_back`, `key_delete`,
`key_end`, `key_escape`, `key_home`, `key_insert`, `key_left`, `key_return`,
`key_right`, `key_space`), covering the two key-edit fields the widget draws.
Everything else is native:

- `KEYS.SCRIPT` (the rebinding screen) fetches its rows through native
  callbacks **2115**, **2139** and **2140** (and resets through **2138**);
  callback **108** displays the command list.
- `CONTROLSPREFS.SCRIPT` (the device-preferences screen) uses native callback
  **2120** to read/write a device selection, and **2110** to list devices.

Because the rows and their current bindings arrive from native code, the
**ordered command list** and the **default bindings** are not in any readable
file. They live in the packed executable's code/data.

## Default bindings: unknown

No shipped file contains a default binding. There is no control-profile
`.ini`/`.scr`, no registry-style blob and no config member in `crimson.rof`
that holds a key/mouse/joystick → command map: the scripts ask the engine for
each row through the callbacks above. Therefore **every original default
binding is unknown**, and this task does not guess one. Recovering them needs
either an owner-supplied original run that shows the controls screen (the same
gate as #358) or a bounded static analysis of `crimson.icd` that records only
ids/offsets/digests and commits no decompiled code. A follow-up task is filed
(see below).

## Comparison: original vocabulary vs `FlightCommand::ALL` / `UiAction::ALL`

Statuses are the task's own: **observed-in-original**, **absent-from-observation**
and **not-yet-observable** (runtime-only). The evidence column lists the
measured original `MSG_*` command labels. The table is asserted complete by the
unignored test `accept_f22_h_the_comparison_covers_every_declared_command`, and
the evidence must partition the 65 measured command labels together with the
original-only set below.

| Project command | Status | Original evidence | Note |
| --- | --- | --- | --- |
| `pitch` | observed-in-original | `MSG_NOSE_UP` (11058), `MSG_NOSE_DOWN` (11057) | pitch up/down |
| `roll` | observed-in-original | `MSG_ROLL_LEFT` (11059), `MSG_ROLL_RIGHT` (11060) | |
| `yaw` | observed-in-original | `MSG_RUDDER_LEFT` (11062), `MSG_RUDDER_RIGHT` (11063) | original calls it rudder |
| `throttle` | observed-in-original | `MSG_INC_THROTTLE`/`MSG_DEC_THROTTLE` (11067/11068), `MSG_THROTTLE_0…8` (11069–11077) | step + nine direct settings |
| `fire_primary` | observed-in-original | `MSG_CMD_TRIGGER` (11045) | |
| `fire_secondary` | observed-in-original | `MSG_FIRE_MISSILE` (11086) | |
| `cycle_weapon` | observed-in-original | `MSG_MISSILE_NEXT` (11085), `MSG_CMD_CANNON_PREV/NEXT` (11089/11090), `MSG_CMD_MISSILE_PREV/NEXT` (11091/11092) | original cycles cannon and missile **separately**; no single generic "next weapon" |
| `drop_ordnance` | absent-from-observation | — | no bomb/drop label in either command range |
| `eject` | observed-in-original | `MSG_CMD_BAIL_OUT` (11053) | |
| `toggle_gear` | absent-from-observation | — | no gear label |
| `flap_step` | absent-from-observation | — | no flap label |
| `target_next` | observed-in-original | `MSG_CMD_TARGET_NEXT_ENEMY/ALLY/GROUND` (10015/10018/10021) | original selects per target class |
| `target_prev` | observed-in-original | `MSG_CMD_TARGET_PREVIOUS_ENEMY/ALLY/GROUND` (10016/10019/10022) | |
| `countermeasure` | absent-from-observation | — | no chaff/flare label |
| `throttle_step_up` | observed-in-original | `MSG_INC_THROTTLE` (11067) | |
| `throttle_step_down` | observed-in-original | `MSG_DEC_THROTTLE` (11068) | |
| `throttle_idle` | observed-in-original | `MSG_THROTTLE_0` (11069) | direct setting 0 |
| `throttle_full` | observed-in-original | `MSG_THROTTLE_8` (11077) | direct setting 8 (nine positions, 0–8) |
| `confirm` | not-yet-observable (runtime-only) | — | menu action, native |
| `cancel` | not-yet-observable (runtime-only) | — | menu action, native |
| `navigate_up` | not-yet-observable (runtime-only) | — | menu action, native |
| `navigate_down` | not-yet-observable (runtime-only) | — | menu action, native |
| `navigate_left` | not-yet-observable (runtime-only) | — | menu action, native |
| `navigate_right` | not-yet-observable (runtime-only) | — | menu action, native |
| `pause` | observed-in-original | `MSG_CMD_PAUSE_GAME` (11037) | |

`absent-from-observation` is a strong statement here only because every label
of both command ranges is named (see Observation 1): the original's command
vocabulary is fully enumerated by name, so a bomb, gear, flap or
countermeasure command could not be hiding anonymously. It still means
"absent from the shipped observation", not "the original cannot do it"; the
runtime behaviour stays unknown.

The project also declares four continuous axes with no direct original
counterpart command event; they are `observed` through the named movement
labels above. The project's `UiAction` menu actions have no file-visible label
and stay `runtime-only`.

## Original commands with no project counterpart (33)

These 33 named, measured original command labels are not in
`FlightCommand::ALL`/`UiAction::ALL`. Some are gaps the project may want to
model; others (padlock directions, look controls, chat) are deliberately not in
this project's small vocabulary.

**First control range (9):** `MSG_CMD_CYCLE_MODE` (10006), `MSG_CMD_INTERP`
(10007), `MSG_CMD_TARGET_NOTHING` (10008),
`MSG_CMD_TARGET_UNDER_RETICULE` (10009), `MSG_CMD_NITROUS` (10010),
`MSG_CMD_TARGET_NEAREST_ENEMY/ALLY/GROUND` (10017/10020/10023),
`MSG_CMD_KEYMAP_DISP` (10025).

**Second control range (24):** `MSG_LOOK_DOWN/BACK/LEFT/RIGHT`
(11020–11023), `MSG_CAM2_TOG` (11024), `MSG_CMD_PADLOCK_SNAP/WATCH/STICK`
(11025/11026/11036), the nine padlock directions `MSG_PADLOCK_DL/D/DR/L/M/R/UL/U/UR`
(11027–11035), `MSG_CMD_ZONE_TOGGLE` (11040),
`MSG_CMD_COLLISION_TOGGLE` (11041), `MSG_CMD_LAUNCH_AUTO_LAND` (11054),
`MSG_CMD_DISPLAY_SCORES` (11056), `MSG_LEVEL_TOG` (11061), `MSG_CHAT_ALL`
(11087) and `MSG_CHAT_TEAM` (11088).

Plus the four wingman orders (`MSG_WINGMAN_*`, 3001–3004) and the target
`NEAREST_*` events, which sit outside the project's declared set. The full,
machine-checked list is `ORIGINAL_ONLY_COMMANDS` in the support module.

## Device families the original distinguishes

| Project `DeviceClass` | Original observation | Status |
| --- | --- | --- |
| `keyboard` | `MSG_KEYBOARD` (1023) device mode; `key_*` names in `crimson.icd` | observed |
| `mouse` | `MSG_MOUSEBTN` (15003) + `MSG_MBTN_LEFT/RIGHT/MIDDLE` (15139–15141) | observed as bindable buttons |
| `joystick` | `MSG_KEYBOARD_JOYSTICK` (1024) mode, `MSG_JOYBTN` (15002) + `MSG_JBTN_1…10` (15129–15138) | observed as bindable buttons |
| `gamepad` | no gamepad label at all | absent-from-observation |

So the original distinguishes **two control modes** (keyboard; keyboard +
joystick) with mouse and joystick **buttons as binding sources**, and has no
gamepad vocabulary. This project's four `DeviceClass` families are a superset
of the original's observable ones.

## Unknowns (not guessed, not removed)

1. **Every original default binding.** Which key, mouse button/axis, joystick
   button/axis or gamepad control each command starts bound to is native data
   in `crimson.icd` and is not in any shipped file. The control scripts fetch
   each row's binding from native callbacks.
2. **The ordered command list.** The order the controls screen enumerates
   commands, and the mapping from each measured label id to a command slot, is
   produced by callbacks `2115`/`2120`/`2139`/`2140` in the packed executable.
3. **Runtime behaviour.** Which key actually fires which command while playing,
   what the original does while text is entered, and what it does on focus loss
   are runtime questions; they need an owner-supplied original run
   (`REF-OWNER-FIRST-CAPTURE`, #358), which is blocked on the owner.
4. **The full original device model.** Whether a joystick axis is bound, how
   axes are calibrated, and how the original's two modes interact with the
   mouse are not visible from files. The mode labels show the *options*, not
   the behaviour behind them.

## Follow-up work

A separate task is filed with Rally `create_tasks` to recover the default
bindings under an explicit capability gate: **#505 `F22-J` "Recover the
original default control bindings"**. It is the one part of the deliverable
that no `retail` read can produce, so it must not be guessed. The runtime
questions are already covered by #358 and are cross-referenced there.

## Research boundary

`docs/research/` and `schemas/` are protected. The discoveries worth folding
into the research notes are: (a) `strings.dll`'s duplicated PE `.data`
`{name, id}` table as the symbol source for the control labels, and (b) the
`crimson.icd` `key_*` pool. If the owner wants them recorded in
`docs/research/FORMAT-NOTES.md` or `SOURCES.md`, that needs a scoped exception;
this finding states the measurement and does not edit those files.

## Tests

Task-test prefix `accept_f22_h_`:

- `crates/cs_content/tests/accept_f22_h_original_control_vocabulary.rs`
  - `accept_f22_h_retail_strings_dll_names_the_original_command_vocabulary`
    (`#[ignore = "requires CS_GAME_DIR"]`) — production `discover`/`fingerprint`,
    `StringCatalog` accounting, the test-local `.data` table and the 65 command
    labels resolved through `StringCatalog`, the category/device/key/button
    labels, and the measured 15/50/44 range counts.
  - `accept_f22_h_retail_the_game_executable_and_control_scripts_expose_the_key_vocabulary`
    (`#[ignore = "requires CS_GAME_DIR"]`) — the `crimson.icd` fingerprint and
    its 120 `key_*` names, and the three control scripts decoded through the
    production `read_tree`/`read_member` with their digests and native callbacks.
  - `accept_f22_h_the_comparison_covers_every_declared_command` (unignored, runs
    in CI) — the comparison classifies every `FlightCommand`/`UiAction` exactly
    once, every `observed` entry cites real measured labels, and the cited plus
    original-only labels partition the 65 measured command labels.
- `crates/cs_content/tests/evidence_report_f22_h.rs` — the CLI-EVIDENCE harness
  (writes `acceptance.json`/`vocabulary.json`; not part of the acceptance
  suite).

## Checks

Run locally by the implementer before hand-over
(`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
`cargo test --workspace --locked`,
`cargo test --workspace --locked -- accept_f22_h_ --include-ignored`).

## Review

**Not yet reviewed.** This branch is submitted for review by a different agent
instance; per `AGENTS.md` the reviewer must record the implementer and reviewer
identities and whether the reviewer's context was fresh, and no agent review is
the owner's human approval. What the reviewer should verify: that every
`observed` classification is backed by the cited measured label, that the
`absent` claims rest on the "every label in both command ranges is named"
argument, that the two retail tests really read `$CS_GAME_DIR` and fail when it
is absent, and that no default binding has been asserted anywhere. This stage
is **checked** at most; it is not `verified_original`.
