# F49-D.03-FU-RULES-TYPE-ORDER (#2037): the Instant Action menu's mission type does not survive the load

Task #2037, branch `rally/2037-resolve-whether-the-instant-action-menu`.
Follow-up of #1639 (`2026-10-24-f49-d-03-fu-rules-roster.md`, "Unmeasured"):
does the type the menu writes to `[0x718cd8]` reach the spawn routine and the
end check, or does the descriptor's own `mission_type` run?

## Evidence class

**`static`** — static reading of the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, load base `0x400000`) and the production survey of the
retail `ZBD/*/IA*/zrdr.zbd#ia.zrd` documents. No original executable ran;
nothing here is `verified_original`. Claim id `f49.d.03.ia-type-order`.

## Answer

**The descriptor's `mission_type` is what runs. The menu's type is destroyed
by the load before anything reads it.** No path re-applies it.

## The order of events

1. *Menu handler* (inside `0x417xxx`): `mov eax,[0x64aba0]; cmp eax,3; ja`
   (`0x417543`), then `jmp [eax*4+0x4175d8]` (`0x41754d`); the table holds
   `0x417554/0x417560/0x41756c/0x417578`, each a `mov dword [0x718cd8], imm32`
   of `0/1/4/2`. Every row reaches `0x417582`, which pushes `1` and `esi` (the
   directory index the handler chose, `0x4174f0`..`0x417516`) and calls
   `0x4638f0` (`0x417594`).
2. `0x4638f0` stores only its two stack arguments, into `[this+0xc1c]` and
   `[this+0xc20]` (`0x463907`, `0x46390d`), and formats names. **The type is not
   an argument and `0x4638f0` never references `[0x718cd8]`.** So the only
   trace of the menu's type is the live value itself.
3. *Mission load* `0x466b90`..: `call 0x4639b0` (state `[this+0x700] == 3`,
   `0x466bcc`), and when true `call 0x45a150` (`0x466bd5`, its **only** caller).
4. `0x45a150` begins `call 0x45a120` (`0x45a152`). `0x45a120` clears the five
   spawn-point vectors (`0x459e90`, `0x718fe0` + 20·n) and runs the record
   initialiser `mov ecx,0x718cd8; call 0x458ff0` (`0x45a138`/`0x45a13d`).
   `0x458ff0` starts `mov dword [ebp],0` (`0x458ffa`): **the live type is reset
   to 0 (`dogfight_ace`).** `0x458ff0` is called from nowhere else.
5. `0x45a150` opens the archive's `ia.zrd` (`0x6256a4`, `0x579c60`), reads
   `disallow_missions` and `spawn_points` (`0x45a17e`, `0x45a1cd`), then
   `mov ecx,0x718cd8; call 0x459390` (`0x45a222`/`0x45a227`, the reader's only
   caller). The reader looks `mission_type` (`0x62555c`) up (`0x4593b5`), maps
   the name through `0x458c20` (`0x4593d7`) and stores the index into `[ebx]`
   unless it is the sentinel 5 (`0x4593df`..`0x4593e3`). An absent key or an
   unrecognised name leaves the reset 0.

## No other writer

The image holds 22 dword references to `0x718cd8` in `.text`
(`0x417556 0x417562 0x41756e 0x41757a 0x458bd1 0x458bf1 0x45a139 0x45a223
0x45a3ac 0x45aa25 0x45ad38 0x45b003 0x45b17e 0x45b4b3 0x45b8c2 0x45b912
0x45b9eb 0x45bd85 0x45bdab 0x45be03 0x4a142e 0x4a1f23`), classified by the
opcode before the address:

* 4 direct stores (`c7 05`): the menu's, `0x417554`..`0x417578`;
* 4 `mov ecx, imm32` loading the record as `this`: the static constructor and
  destructor `0x458bd0`/`0x458bf0` (start-up/exit), the reset `0x45a138` and
  the reader call `0x45a222`;
* 14 reads (`mov r,[m]`, `cmp [m],imm`, `cmp [m],r`): the spawn routine
  `0x45a390` (`0x45a3ac`), the ace gate (`0x45aa25`), the group loop
  (`0x45ad38`, `0x45b003`, `0x45b17e`, `0x45b4b3`), the tail and wave code, the
  end check (`0x45b9eb`) and two UI reads (`0x4a142e`, `0x4a1f23`).

The record's neighbours `0x718cd0` (wave counter) and `0x718cdc` (player
plane) are not stores to the first dword. The only code between the menu's store
and the spawn routine that writes the type is therefore the load's own reset and
reader. (A write through a computed pointer is not excluded by a static search;
none is evident and none was found.)

## Consequences

Every retail descriptor declares a recognised `mission_type` (production
survey, eight of eight), none is `dogfight_ace`, so the type that runs for a
directory is its descriptor default whatever row the menu offered:

| type that runs | directories |
| --- | --- |
| `dogfight_squadron` | `ZBD/c1/IA1`, `ZBD/c3/IA1` |
| `stunt_flying` | `ZBD/c1b/IA1`, `ZBD/c2/IA1`, `ZBD/c4/IA1`, `ZBD/c5/IA1` |
| `zeppelin_run` | `ZBD/c1c/IA1`, `ZBD/c2b/IA1` |

* **Types actually reachable**: `dogfight_squadron`, `stunt_flying`,
  `zeppelin_run`. `dogfight_ace` (reset value, no retail descriptor leaves the
  key out or misspells it) and `ground_target` (no menu row, no retail default)
  never run. The 30 menu (directory, type) rows collapse to **8 distinct
  launches**; for each directory the non-default rows run the default type. The
  default is always among the directory's `spawn_points` types, so the type
  that runs always has its spawn-point vector (4 or 8 points).
* **#1638 rules** (`measured_ia_victory_rules`, bound per `mission_type` name):
  the end check reads the live type at `0x45b9e9`, i.e. the descriptor default.
  The rules measured for `dogfight_ace` are never applied in retail Instant
  Action; for a preset, the rules that apply are those of its directory's
  default type, not of the menu row's `original_mission_type`.
* **#1639 roster** (`ia_wingman_template`, `ia_spawns_named_ace`): same
  re-keying. The named ace (type-0 gate `0x45aa24`) is **never seated** in a
  retail Instant Action, so every `ace_*` key is inert; wingmen are the
  `wingman` record for `stunt_flying`/`zeppelin_run` directories and the
  `devastator` record for `dogfight_squadron` directories, regardless of the
  menu row.
* **`num_wingmen` zeroing at `0x459433`**: it tests the record's type at read
  time, which is the descriptor's own `mission_type` (stored at `0x4593e3`
  before the count). All eight defaults are non-ace, so it can never fire for a
  retail directory; the menu's type plays no part.

## Not changed here, follow-up

The preset catalog still keys each preset on the menu row's type
(`IaOfferedType::mission_type`, `original_mission_type`), which this finding
shows is not what the original runs. Re-keying is a code change outside this
task's owner paths; a follow-up task is filed.

## Unmeasured, recorded

* Whether the menu **offers** (enables) a row whose type differs from the
  directory's default, and what the player sees: the handler stores the type
  first and the engine later overrides it. Why the original ships such rows is
  unmeasured; no original run was made.
* A write to the live type through a computed pointer cannot be excluded by a
  static search (none found).
* The mode codes, sides and default placement already listed in
  `2026-10-24-f49-d-03-fu-rules-roster.md` stay unmeasured.

Tests (`accept_f49_d_03_fu_rules_type_order_*`, `crates/cs_app/tests/`): the
image tests re-read the menu stores, the `0x4638f0` argument list, the reset and
reader sequence, the single callers and the full reference list; the retail test
re-derives the table above from the production survey.
Evidence: `docs/findings/evidence/F49-D.03-FU-RULES-TYPE-ORDER.json`.
Sources: the decrypted image at the VAs above; the installation's eight IA
`zrdr.zbd` (`ia.zrd`) read through the production survey.
