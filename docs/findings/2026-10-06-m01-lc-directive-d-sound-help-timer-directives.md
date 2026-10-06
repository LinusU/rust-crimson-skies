# M01-LC-DIRECTIVE-D: what M01's sound, help and timer directives do, measured in the retail executable

Date: 2026-10-06. Task: `M01-LC-DIRECTIVE-D` (#682), "measure sound, help and
timer directive semantics". Parents: `M01-LC-DIRECTIVE-MEANING` (#675);
stage A (#679) located the parser and is the base this document extends.
Capability used: `retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/M01-LC-DIRECTIVE-D/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-DIRECTIVE-D.json`.

**Everything about the original program below is static code evidence** from
disassembling the owner's executable. The original program was never run for
this task. Nothing here is `verified_original` runtime behaviour, and no
finding self-awards that status (AGENTS.md rule 8). Where a semantic could not
be reduced from the code alone it is recorded as **unknown**, not guessed
(AGENTS.md rule 4).

## Provenance

| Item | Value |
| --- | --- |
| File | `$CS_GAME_DIR/crimson.decrypted.exe` |
| Format | PE32, image base `0x400000` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (identical to stage A's, so both stages read one binary) |
| Address convention | virtual address (VA); in `.text`, `VA = file offset + 0x400000` |
| Reader | `0x466b70` — `CZMission`'s mission-file parser (stage A), re-read here for the keys in scope |
| Per-objective tick | `0x46a490` (`CZMission::Update`), per-objective completion tail inside it |
| Wake | `0x469af0` (per-objective wake, up to 15 records per call) |
| Objective record | `0x5e4` bytes at `this+0xc4c` |
| Mission object | global `0x71b480` (set at `0x47e167`: `mov ecx, 0x71b480; call 0x4696f0`) |
| Mission sound manager | global `0x71b438` |
| Mission timer | global `0x71b468` (constructor/reset `0x46c4f0`, called from the parser at `0x466bba`) |
| Sound engine | `zsnd_play.cpp` (string cluster at `0x63a3ac`: the source path `D:\zipper\gamez\zSound\zsnd_play.cpp` at `0x63a3ac`, then `Playing: %s` at `0x63a3d4` and `%s: queued sound is also looped.` at `0x63a3e0`) |
| Sound archives named by the engine | `soundsL.zbd`, `soundsM.zbd` (`0x63a544`, `0x63a550`) |
| Tools | `r2` `aaa` + a linear `pD` sweep of `.text` (`0x401000`, 0x202000 bytes) into a private scratch listing; `python3` for byte/string reads; `grep` on that listing |
| M01 data read | production `cs_app::mission_control::survey_mission_control_programs` and `cs_content::objectives::measure_dormant_declarations` over `zbd/c1c/m01` |

Stage A's parse model is used as given: an objective record is `0x5e4` bytes,
value tags are `1` int / `2` real / `3` string / `4` list, and record offsets
are relative to the record pointer (`esi`/`edi`/`ebp` in the parse, `esi` in the
tick).

## Reviewer spot-check (2026-10-06)

Re-read from the same bytes by the reviewing session — a PE section-table walk
with `python3`, read-only, no original run. The reviewing session is the same
agent id as the implementer (`bunny-alpha-2`) in a fresh context, so this is a
second reading of the code, **not** independent original-run evidence.

| Claim here | Result |
| --- | --- |
| Provenance SHA-256 `43540fc9…` | verified, file is 2 580 480 bytes |
| Seven `music_*_sg` names, in the stated order | verified, at `0x63a4a8`, `0x63a4c0`, `0x63a4d8`, `0x63a4f0`, `0x63a504`, `0x63a51c`, `0x63a530` — this row's table extent was corrected from `0x63a538` to `0x63a540` |
| `Playing: %s` / `%s: queued sound is also looped.` | string cluster verified; both addresses were corrected (`0x63a3d4`, `0x63a3e0`; `0x63a3ac` holds the `zsnd_play.cpp` source path) |
| `soundsL.zbd` `0x63a544`, `soundsM.zbd` `0x63a550`, `DELETE_ON_SUCCESS` `0x626350` | verified byte for byte |
| `NOLOSS` string exists | verified, one occurrence, `0x625fbc` |
| constants: `0.0f` at `0x6032c8`, `-1.0f` at `0x6034e8`, `1000.0` at `0x603464`, `10.0` at `0x603390` | verified as IEEE-754 values |
| start rule `0x469741` | verified: `fcomp dword ptr [0x6032c8]` at `0x469746`, then `fnstsw ax` / `test ah,0x41` / `jne`, then `mov ecx, 0x71b468` and the start call |
| `Update` calls `0x46c5f0` at `0x46a4d8` and `0x46c640` at `0x46a4e2` | verified from both relative-call targets |
| `STOP_QUEUED_SOUNDS` cap of 10 | `cmp esi, 0xa` (`83 fe 0a`) at `0x4688bc`, `jg` over the copy, inside the routine whose first argument address is the `STOP_QUEUED_SOUNDS` string `0x626778` |
| `Expecting real value, found string %s`, `MISSION_WON_SOUND`, `OBJECTIVES_WON_SOUND` strings | verified at `0x625f28`, `0x626040`, `0x626010` |

The remaining rows of the tables below are the implementer's reading and were
not re-derived line by line; they stay static code evidence either way, and the
**Unknowns** section still governs what is not measured.

## The sound handle a `*_SOUND_GROUP` name becomes

* `0x596120(const char* name)` is the **name → handle** lookup: it walks a
  global list head at `[0x9ad6dc]`, comparing each node's name with `0x596af0`
  per node, returns the node's payload on a match, and on failure calls
  `0x599a50(name)`; it returns `0` when the global table pointer `[0x639c7c]`
  is null or the name is null.
* Three mission-level play entry points wrap the engine's `0x593590`:
  * `0x46cc50(mgr, handle)` → `0x46caf0(mgr, handle, 1.0f, 3, 1.0f)` — the
    **objective-completed** channel;
  * `0x46cc70(mgr, handle)` — the **objective-woken** channel, and the only one
    that special-cases: `handle == 0` returns immediately; `handle == mgr+0x10`
    plays `(…, 1.0f, 2, 0)`; `handle == mgr+0x14` plays `(…, 1.0f, 5, 0)` when
    `mgr+0x18` is null and stores the result in `mgr+0x18`, otherwise returns;
    `handle == mgr+0x1c` plays `(…, 1.0f, 4, 0)` when `mgr+0x18` is set,
    otherwise returns; anything else goes straight to `0x593590(1.0f, handle)`.
  * `0x463c30` (the mission-end routine) plays `mgr+0x10`-shaped handles with
    `(…, 1.0f, 0, 1)` / `(…, 1.0f, 1, 0)`.
* `mgr+0x10/+0x14/+0x18/+0x1c` are four handle slots that
  `0x46ca50` (the sound manager's reset, called at mission reset `0x46972d`)
  clears to null. **Which groups occupy them at runtime is not measured here**;
  the routing *shape* is.
* The engine's own string table (`0x63a4a8..0x63a540`: the seventh name
  `music_battle_sg` ends at `0x63a53f`) holds exactly seven
  built-in music sound-group names — `music_battlesuccess_sg`,
  `music_missionsuccess_sg`, `music_tertiaryobj_sg`, `music_primaryobj_sg`,
  `music_secondaryobj_sg`, `music_prebattle_sg`, `music_battle_sg` — each
  spelled in the same string cluster as the `.wav` it names
  (`music_primaryobj.wav`, …, in the cluster `0x63a404..0x63a490`, in a
  different order) and immediately before the archives `soundsL.zbd`
  (`0x63a544`) / `soundsM.zbd` (`0x63a550`). `0x5954f0(name)` is the dispatcher
  that compares an incoming name against those spellings (byte compares, in the
  order prebattle, secondary, primary, …) and selects the music state; it is
  called once per music request from `0x595140`.
* **Consequence, measured:** a `WAKEUP_SOUND_GROUP` naming one of those seven
  names is a *music state request*, not an ordinary cue. M01 spells **five of
  the seven**, over **six** of its 18 wakeup sites (measured below).

## Objective sound, help and timer keys

Every row below was read out of the executable. "Effect" is what the code
*does*; "unknown" marks the part that no static reading settles.

| Key | Parse site | Args read | Record field | Runtime consumer and measured effect |
| --- | --- | --- | --- | --- |
| `WAKEUP_SOUND_GROUP` | `0x468a4a` | requires value tag `4`, ≥1 child, child0 tag `3`; `0x596120(child0)` | `+0x54c` handle, default 0 | **Wake** `0x469ce0`: if non-null → `0x46cc70(0x71b438, handle)` (the woken channel above). This is the *only* reader of `+0x54c`. |
| `COMPLETED_SOUND_GROUP` | `0x468a8a` | same requirement as above | `+0x550` handle, default 0 | **Completion** `0x46a95b`: if non-null → `0x46cc50(0x71b438, handle)` (channel 3). Only reader of `+0x550`. |
| `STOP_QUEUED_SOUNDS` | `0x468884` | requires tag `4`; copies children `strdup`'d, capped at **10** (`cmp esi, 0xa; jg end`); `+0x510` = the copied count | `+0x510` count, `+0x514..` names | **Completion** `0x46ad48`: for each stored name → `0x5920f0(name)`. |
| `SET_HELP_LABEL` | `0x4687eb` | requires tag `4`; child0 (a list of node names) → `0x465fa0` 20-byte name-vector record at `+0x508`; child1, only when the list holds >1 child, `strdup`'d if non-empty → `+0x50c` else 0 | `+0x508`, `+0x50c` | **Completion** `0x46ad29`: if `+0x508` → `0x466260(+0x508, +0x50c)` then `0x46a2d0(handle)`. |
| `END_TIMER` | `0x468ac8` | **presence only** — no tag or child check at all (`test eax,eax; je`) | `+0x18 = 1` | **Completion** `0x46ae75`: if non-zero → `0x46c5c0(0x71b468)`, i.e. stop the mission timer (clear its running flag). |
| `RESET_TIMER` | `0x467d69` | real/int (`0xdee` = mission.cpp line 3566 for a type error) | `+0x5e0` seconds | **Wake** `0x469d14`: only when the objective's `+0x5c8` was **0** (dormant) at entry, and only when `+0x5e0 >= 0.0f` → `0x46c510(0x71b468, +0x5e0)` (set remaining) then `0x46c5a0(0x71b468)` (**start**), then `+0x5e0 = 0` — consumed once. |
| `TIMER_ADJUST` | `0x468e66` | real/int (`0xff3` = 4083) | `+0x5c4` = value, `+0x5c0 = 2` (adjust) | **Completion** `0x46ae41` (below). |
| `ADJUST_TIMER_WHEN_I_COMPLETE` | `0x468f27` | needs ≥2 children, child0 tag `3`; `cmpsb "SET"` (4 B) → `+0x5c0 = 1`, else `cmpsb "ADJUST"` (7 B) → `+0x5c0 = 2`, **else no write**; child1 real/int (`0x1005` = 4101) → `+0x5c4` | `+0x5c0`, `+0x5c4` | Same completion site; because it is parsed **after** `TIMER_ADJUST` it overrides that key's mode when both are spelled. |

### What `STOP_QUEUED_SOUNDS`'s consumer does (`0x5920f0`)

```
for (;;) {
    entry = 0x5791a0([0x639eac], compare@0x592090, name);   // byte-compare of the entry's name
    if (!entry) return;
    0x5790a0([0x639eac], entry);                          // take it off the list
    0x592010(entry);                                      // flag and reschedule it
}
```

`0x592010(entry)` sets `entry+0x10 = 1` and `entry+0x38 = [0x639ea8] + 10.0f`
(the constant at `0x603390` is `10.0`), then re-links the entry through
`0x578fa0`. The sweeper `0x592040` removes an entry once
`[0x639ea8] >= entry+0x38`. The name is read out of the entry as
`entry+4 -> +0x0 == 0 -> +8` (a two-step list dereference, then a byte
compare).

**Measured:** every queued sound whose name matches is flagged and scheduled
for removal 10.0 (in `[0x639ea8]`'s units) later, at objective completion.
**Unknown:** whether playback stops at the flag or at the removal, i.e. what
reads `entry+0x10`. Not traced here.

### What `SET_HELP_LABEL`'s consumer does

`0x466260(rec, label)` (with `rec` = the `+0x508` name vector) walks its names,
resolving each through `0x465e30` and merging with `0x4d8cf0`, and returns one
handle. `0x46a2d0(handle, label)` then:

1. `0x4afee0(0x71dab8, handle)` — first registry; if that misses,
   `0x4a9890(0x71d910, handle)` — second registry;
2. `0x59cd20(label)` — resolve the label **string** as a localized id; when it
   resolves, `0x59ce40(id)` renders the text through the 16-slot rotating buffer
   at `0x9ad9c0`; when it does not, the text is the empty wide string at
   `0x64e73c`;
3. writes the text into the object's `std::string` at `+0x38` (through
   `__Grow__basic_string`) and the resolved id at `+0x48`.

**Measured:** at objective completion the named object is given a localized
text label. **Unknown:** which HUD element reads that `+0x38` label, and whether
an unresolvable label blanks the label or is meant to.

### The mission timer, measured end to end

The global timer at `0x71b468` is reset by `0x46c4f0` at the top of the
mission parser (`0x466bba`): `[+0]` total, `[+4]` remaining seconds, `[+8]`
remaining milliseconds, `[+0x10]` running byte, `[+0x14]` no-loss flag — all
zeroed.

| Operation | Site | Measured effect |
| --- | --- | --- |
| set remaining | `0x46c510(t, seconds)` | `[+4] = seconds`; `[+8] = (int)(seconds * 1000.0f)` (the factor at `0x603464` is `1000.0`) |
| add to remaining | `0x46c550(t, seconds)` | `[+8] += (int)(seconds*1000)`; `[+4] = seconds` after settling |
| start | `0x46c5a0(t)` | `[+0xc] = GetTickCount()`; `[+0x10] = 1` |
| stop | `0x46c5c0(t)` | settle, then `[+0x10] = 0` |
| per-frame tick | `0x46c5f0(t)`, called from `Update` `0x46a4d8` | `[+0] += frame dt` always; `[+4] -= frame dt` while running |
| settle | `0x46c610(t)` | `[+8] += [0xc] - GetTickCount()`, `[0xc] = now` — wall-clock correction while running |
| expiry poll | `0x46c640(t)`, called from `Update` `0x46a4e2` | if not running → 0; else settle and compare `[+4]` with the constant at `0x6032c8`, which is **`0.0f`**; expiry iff `[+4] <= 0.0`. If `[+0x14]` (no-loss) is non-zero, the poll zeroes `[+4]` and reports **no** expiry. |
| start at mission start | `0x4696f0` → `0x469741`/`0x469758`, reached from `0x47e080` case `0x47e167` | reads remaining (`0x46c580`) and starts the timer **iff remaining `> 0.0f`** |
| stop at mission load | `0x4646be`, in the mission-load routine `0x464680` (the one that logs `Load Mission Sounds`) | `0x46c5c0` unconditionally (unless `[0x64f750]->[0]` is set) |

`MISSION_TIMER`'s parse (`0x466c31`, real or int; a type error logs
`Expecting real value, found string %s` at mission.cpp line `0xd12` = 3346)
calls `0x46c510(0x71b468, value)`. Its optional second child, compared with
`cmpsb "NOLOSS"` (7 B) at `0x466cd4`, calls `0x46c540(0x71b468, 1)` → `[+0x14] =
1`. **Any other second-child spelling is ignored** — only the exact 7 bytes
`NOLOSS` set the flag.

#### What timer expiry does (`CZMission::Update` `0x46a4e2`)

```
if (!expired) skip
if (0x440ad0() != 0) skip                    ; a global game-state byte
if (0x463c00() != 0) skip                    ; this+0xc54, the mission's "ended" flag
if (remaining < -1.0f) skip                  ; constant at 0x6034e8 is -1.0f
0x463c30(mission, 1, 3.0f)                   ; end the mission (plays MISSION_WON/LOST_SOUND)
0x59ce40(0x1772) -> 0x4587d0(text, 5.0f)    ; a localized message, 5.0f
if (0x440ad0() == 0 && 0x4639b0() == 0)
    0x59ce40(0x89) -> 0x4587d0(text, 5.0f)  ; a second localized message, 5.0f
0x46c5c0(0x71b468)                           ; stop the timer
```

`0x4587d0(text, seconds)` is a wrapping text display: it `strdup`s the string,
and only if it is longer than 48 bytes (`cmp ecx, 0x30`) does it split it into
space-separated lines. `0x59ce40(id)` copies a localized string into one of 16
rotating 0x1000-byte buffers at `0x9ad9c0`. **Measured:** expiry ends the
mission and asks for two localized messages, ids **6002** (`0x1772`) and **137**
(`0x89`), each displayed for 5.0f. **Unknown:** the text behind those ids (no
shipped file resolved here) and what `0x440ad0()`'s global state byte and
`0x4639b0()` mean — the latter is measured only as "the mission's `[+0x700]`
equals 3" (`0x4639b0`: `mov edx,[ecx+0x700]; cmp edx,3; sete al`), a
mission-phase predicate whose phase names are not recoverable here.

### The mission-end routine `0x463c30` and the seven `*_SOUND` record keys

`0x463c30(mission, ended, arg)` is the measured mission-end path:

* `[+0xc3c] = [+0x6f0]` (the mission clock at that moment), `[+0xc54] = 1`;
* the sound it plays is chosen by the mission's **won** flag `[+0xc58]`:
  non-zero → the `MISSION_WON_SOUND` handle `[+0xc78]`, zero → the
  `MISSION_LOST_SOUND` handle `[+0xc7c]`; a null handle plays nothing;
* a non-null handle is played `0x46caf0(0x71b438, handle, 1.0f, 0, 1)`.

All seven mission-level keys are parsed in the same block, immediately after
`MISSION_TIMER`, each as `0x596120(child0)` into its own field with a
zero default:

| Key | Parse site | Field | Measured consumer |
| --- | --- | --- | --- |
| `PRIMARY_COMPLETE_SOUND` | `0x466ced` | `+0xc64` | Objective completion `0x46a9a0`: an objective of class 1 (`IDENTITY PRIMARY`) plays it through `0x46cc50` |
| `SECONDARY_COMPLETE_SOUND` | `0x466d1a` | `+0xc68` | `0x46a994`, class 2 |
| `TERTIARY_COMPLETE_SOUND` | `0x466d47` | `+0xc6c` | `0x46a988`, class 3 |
| `OBJECTIVES_WON_SOUND` | `0x466d74` | `+0xc70` | End of mission `0x46afd2`: when the won flag `[+0xc58]` is set and the handle is non-null → `0x46caf0(…, 1.0f, 1, 0)` |
| `OBJECTIVES_LOST_SOUND` | `0x466da1` | `+0xc74` | `0x46af9f`: when the lost flag `[+0xc5c]` is set → `0x46caf0(…, 1.0f, 1, 0)` |
| `MISSION_WON_SOUND` | `0x466dce` | `+0xc78` | inside `0x463c30` (see above) |
| `MISSION_LOST_SOUND` | `0x466dfb` | `+0xc7c` | inside `0x463c30` |

Note the class dispatch at completion is a `dec/je` chain over `[esi+0]`
(`0x46a97d`), so an objective with **no** `IDENTITY` class (`[+0] == 0`) plays
no class sound at all, and the three class handles are mutually exclusive.
**M01 spells none of the seven** (measured below).

### The completion-time HUD pair (the UI sibling of `SET_HELP_LABEL`)

At `0x46a9c0`, guarded by `0x463c00() == 0` (mission not yet ended) and the
one-shot flag `[esi+0x558]`, an objective with a class (`[+0] != 0`) calls:

* `0x4a2350(0x71d2a0, class, ordinal)` — appends the ordinal to that class's
  12-byte-per-entry list (`[mgr + class*12 + 0x58]`, count at `+8`);
* `0x4ad240(ordinal)` — walks a global table `[0x71d92c..0x71d930)` of 0x1c-byte
  records and sets `[entry+0x10] = 1` on the one whose `[entry+0x14]` equals the
  ordinal.

Both then clear `[+0x558]`, so the pair happens once per objective per mission.
**Measured:** the completion marks the objective in two HUD bookkeeping
structures. **Unknown:** what either structure drives on screen; no reader of
the class list or of `entry+0x10` was traced here.

### `DELETE_ON_SUCCESS` and `START_TAXI` (measured, but siblings of other stages)

* `DELETE_ON_SUCCESS` is **not a directive key**: the string at `0x626350` is
  consumed only as `TRAVELERS`'s sixth argument, by `repe cmpsb` (18 B) at
  `0x467d57`, after the parse has required ≥6 children and child5 to be tag `3`
  (`0x467d3c`). A match sets `+0x5bc = 1`; the default is `0`.
  The travelers evaluator `0x465b40` reads it twice (`0x465c9a`, `0x465cf5`):
  when the distance test passes and `+0x5bc != 0`, it resolves the subject
  `+0x594` through `0x4afee0(0x71dab8, name)` and calls `0x47bab0(object)` —
  or `0x4cca30(name, 0)` when the name does not resolve — and then reports the
  objective complete. **Measured:** the token makes the travelers subject get
  deleted when the objective succeeds. The rest of the travelers evaluation
  (`+0x598`, `+0x5b4`, `+0x5b8`, `+0x59c`) belongs to stage C's family; only the
  deletion was read here because this task names the token.
* `START_TAXI` (`+0x4dc` count, `+0x4e0` names) is consumed at completion by
  `0x46acfe`: each name → `0x46a2b0(name)` → resolve in `0x71dab8` and write
  `[object+0xd4] = 0`. **Measured:** one byte on the named object; **unknown:**
  what that byte controls. M01 spells it nowhere.

## What M01 itself spells (measured from the installation)

`survey_mission_control_programs` over `zbd/c1c/m01`: **58** numbered objective
blocks, **353** directive sites, **43** distinct keys. The four sound/help keys
in scope:

| Key | Blocks | Sites | Measured shapes | Disposition today |
| --- | --- | --- | --- | --- |
| `WAKEUP_SOUND_GROUP` | 18 | 18 | `[text]x18` | `Unmeasured { MeaningNotMeasured }` |
| `COMPLETED_SOUND_GROUP` | 23 | 23 | `[text]x23` | `Unmeasured { MeaningNotMeasured }` |
| `STOP_QUEUED_SOUNDS` | 3 | 3 | `[text]x3` | `Unmeasured { MeaningNotMeasured }` |
| `SET_HELP_LABEL` | 2 | 2 | `[text,text]x1`, `[[text,text],text]x1` | `Unmeasured { DisagreeingArgumentShape { shapes: 2 } }` |

`MISSION_TIMER` is M01's only in-scope record-level field: measured as
**`[0.0]`** (`List([Float(0.0)])`), one site, support `shape_measured`.

The two `SET_HELP_LABEL` sites, verbatim: `OBJECTIVE2` carries
`[["piratezep", "rock_zeppelin"], " "]` and `OBJECTIVE50` carries
`["workersvoyagezep", " "]`. Both labels are a single space, and the first
site's node list holds **two** names. Measured against the parse, the second
child is `strdup`'d whenever it is non-empty, so M01's labels are the one-byte
string `" "` — not a message id.

**The `WAKEUP_SOUND_GROUP` vocabulary M01 spells — 13 distinct names over 18
sites**, five of which are the engine's built-in music groups:

```
music_battlesuccess_sg   music_missionsuccess_sg  music_prebattle_sg
music_primaryobj_sg      music_secondaryobj_sg
snd_NW1Fass              snd_NW1Start
snd_c2-NW-m1_Jack_7      snd_c2-NW-m1_Jack_21     snd_c2-NW-m1_Jack_34
snd_c2-NW-m1_Tex_35      snd_c2-NW-m1_WorkersVoyage_12
snd_c2-NW-m1_WorkersVoyage_13
```

**The `COMPLETED_SOUND_GROUP` vocabulary — 21 distinct names over 23 sites**:
`snd_NW1BSwan`, `snd_NW1FirstHook`, `snd_NW1Prim2`, `snd_NW1Prim5Suc`,
`snd_NW1Sec1Suc`, `snd_c2-NW-m1_Jack_11`, `snd_c2-NW-m1_Jack_44`,
`snd_c2-NW-m1_Sparks_3`, `snd_c2-NW-m1_Sparks_4`, `snd_c2-NW-m1_Sparks_6`,
`snd_c2-NW-m1_Sparks_24`, `snd_c2-NW-m1_Sparks_40`, `snd_c2-NW-m1_Tex_5`,
`snd_c2-NW-m1_Tex_17`, `snd_c2-NW-m1_Tex_18`, `snd_c2-NW-m1_WorkersVoyage_15`,
`snd_c2-NW-m1_WorkersVoyage_19`, `snd_c2-NW-m1_Zachary_14`,
`snd_c2-NW-m1_Zachary_20`, `snd_c2-NW-m1_Zachary_37`, `snd_c2-NW-m1_Zachary_45`.
The two repeats are `snd_NW1Prim2` (`OBJECTIVE3`, `OBJECTIVE48`) and
`snd_c2-NW-m1_Jack_11` (`OBJECTIVE42`, `OBJECTIVE43`).

**Three names are used in both roles**, which is the sharpest measured
observation M01 offers about `STOP_QUEUED_SOUNDS`:

| Name | Played as | Stopped at completion of |
| --- | --- | --- |
| `snd_c2-NW-m1_Jack_21` | `WAKEUP_SOUND_GROUP` of `OBJECTIVE29/30/31` | `OBJECTIVE11` |
| `snd_c2-NW-m1_Tex_35` | `WAKEUP_SOUND_GROUP` of `OBJECTIVE33/34/35` | `OBJECTIVE15` |
| `snd_c2-NW-m1_Zachary_14` | `COMPLETED_SOUND_GROUP` of `OBJECTIVE6` | `OBJECTIVE55` |

That is consistent with the measured handler (stop the queued/pending entry with
this name when this objective completes) and rules out "stop everything this
objective ever started", since `OBJECTIVE11` completes long before
`OBJECTIVE29` wakes the same group. It is **consistent with**, not proof of, the
handler's purpose.

### M01's mission timer, measured against the measured start rule

M01 writes `MISSION_TIMER [0.0]`, and the mission-start path starts the timer
**iff** the parsed remaining time is `> 0.0f` (`0x469741`: `fcomp` against the
constant `0.0f` at `0x6032c8`, `test ah,0x41`, `jne` skips the start). `0.0`
is not `> 0`, so **M01's mission timer never starts**, and the expiry poll
(`0x46c640`) reports no expiry for a stopped timer. M01 spells no `NOLOSS`, no
`END_TIMER`, no `RESET_TIMER`, no `TIMER_ADJUST` and no
`ADJUST_TIMER_WHEN_I_COMPLETE`, so nothing else in M01's control record starts
it either: **the mission-timer timeout path cannot fire in M01 as authored**,
and the localized ids 6002/137 above are unreachable from M01's program.
M01's `IDENTITY` classes are `PRIMARY` (4 sites) and `SECONDARY` (1), and it
declares none of the seven `*_SOUND` record keys, so M01's own program names no
mission-level completion, objective-set or outcome sound group.

## Unknowns and limits (each with its evidence)

1. **The handles themselves.** `0x596120` returns a node payload from a runtime
   list, and `0x599a50(name)` is called when a name is absent. No shipped file
   declares what a handle *is* for a given name, so nothing here states which
   sound a name ultimately plays — only what the original does with the handle.
   The only group names with independent evidence are the seven `music_*_sg`
   built-ins, which appear in the executable's own table.
2. **Which groups occupy `mgr+0x10/+0x14/+0x18/+0x1c`.** The routing shape in
   `0x46cc70` is measured; the population of those four slots is not.
3. **The queued-sound entry's `[+0x10]` flag.** `0x592010` writes it; no reader
   was traced, so "stop" is a measured *scheduling* effect (flag + removal 10.0
   later), not a measured audible one.
4. **`0x440ad0()`'s global state byte** (`[[0x64f750]]`), which gates the
   timer-expiry path twice, and `0x4639b0()`, whose predicate is measured but
   whose meaning is not: it is `[+0x700] == 3` on the mission, and no shipped
   file names that phase.
5. **The text behind localized ids 6002 and 137.** They are requested through
   `0x59ce40`; no production reader resolves them, and F39-E1 already measured
   that the installation's two generated headers define no `MSG_*` id. Unknown.
6. **`SET_HELP_LABEL`'s UI.** Assigning the label is measured; which element
   shows it, and the meaning of the empty-label case, are not. The two M01 sites
   pass a single space, so on M01's data the label text measured here is a
   space.
7. **The completion HUD pair (`0x4a2350` / `0x4ad240`).** What the class list and
   the `[entry+0x10]` flag drive is not traced.
8. **`START_TAXI`'s `[object+0xd4]`.** One byte is written; nothing else about
   it was read.
9. **The remainder of the travelers evaluation** (`+0x598`, `+0x59c`, `+0x5b4`,
   `+0x5b8`) is stage C's family; only `+0x5bc` was read here.
10. **No original run.** Every claim above is static code evidence. The
    `music_*_sg` route, the two-message timeout and the "delete the travelers
    subject" effect are code paths, not observed play.
11. **`ENTRY_TIMER` interactions with `MISSION_TIMER` absent.** `END_TIMER` and
    `TIMER_ADJUST` only reach the timer at **objective completion**, so an
    objective that never completes cannot move or stop it; measured from the
    single completion site, not from a run.
12. **The parse's tag fragility.** `STOP_QUEUED_SOUNDS`'s `strdup`, the
    sound-group names and the mission-level `*_SOUND` names are read from child
    payloads without a per-child tag check, so a non-string child would be
    misread. Static observation only.

## What this delivers to stage E (`M01-LC-DIRECTIVE-E`)

For M01's four in-scope keys the measured dispositions can be stated as
operations rather than names, and each is falsifiable against this document:

* `WAKEUP_SOUND_GROUP(name)` — at the objective's **wake** transition, request
  sound group `name` through the woken channel; if `name` is one of the seven
  built-in `music_*_sg` names it is a music state request. M01: 18 sites, 13
  distinct names, 5 of them music (over 6 sites).
* `COMPLETED_SOUND_GROUP(name)` — at the objective's **completion**, request
  sound group `name` through the completed channel (category 3). M01: 23 sites,
  21 names.
* `STOP_QUEUED_SOUNDS(name…)` — at the objective's **completion**, for each name
  (at most 10), flag the matching queued sound entry and schedule its removal
  10.0 time units later. M01: 3 sites, 3 names, each also used as a cue
  elsewhere in M01.
* `SET_HELP_LABEL(nodes…, label)` — at the objective's **completion**, resolve
  the node names to one world object and give it the localized label text.
  M01: 2 sites; both labels are `" "`.
* `MISSION_TIMER` — M01's `[0.0]`, and the measured start rule is
  `value > 0.0`, so the mission timer is **not started** for M01. Stage E must
  not turn it into a countdown.

Keys M01 does not spell (`END_TIMER`, `RESET_TIMER`, `TIMER_ADJUST`,
`ADJUST_TIMER_WHEN_I_COMPLETE`, the seven `*_SOUND` record keys, `START_TAXI`)
have measured handlers above, so a later mission that spells them can be bound
from this document rather than re-measured; none of them is M01's, so none of
them may be invented into M01's program.