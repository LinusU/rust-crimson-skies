# F49-D.03-FU-MENU: where the Instant Action titles live, and what stays unmeasured

Date: 2026-10-18. Task #1584, follow-up of F49-D.03 (#785). Capability:
**`retail`** (read-only `$CS_GAME_DIR`). Nothing was run, rendered or played.
No original text is committed; this document carries message names and ids only.

## Measured (through `MessageCatalog` over `strings.dll`)

Test: `cargo test --locked -p cs_content -- accept_f49_d_03_fu_menu --include-ignored`
(`crates/cs_content/tests/accept_f49_d_03_fu_menu_titles.rs`, exit 0, 1 test; it
fails when an id below is changed).

1. **Titles are per mission type, not per scenario.** Four offered mission types
   each own a four-message briefing block, ids consecutive:

   | mission type | message stem | ids (HEAD1, HEAD2, OBJ1, OBJ2) |
   | --- | --- | --- |
   | dogfight_ace | `MSG_BRF_IAS_*` | 16680-16683 |
   | dogfight_squadron | `MSG_BRF_IAT_*` | 16700-16703 |
   | stunt_flying | `MSG_BRF_IASF_*` | 16720-16723 |
   | zeppelin_run | `MSG_BRF_IAZ_*` | 16740-16743 |

   `HEAD1` is the same text in all four ("Instant Action" heading); `HEAD2`
   names the type. Stem-to-type pairing is by reading each `HEAD2`/`OBJ`
   text, which describes the type unambiguously (ace duel, squadron,
   stunt course, zeppelin); it is not an engine table lookup. The engine-side
   binding of mission_type to stem was **not** measured.
2. **No title per scenario.** No message name carries a scenario (c1..c5) and
   none is of the `MSG_IA_C<digit>` / `MSG_BRF_IAG*` form. The only other
   Instant Action messages are the 17 `MSG_IA_*` customize-screen labels
   (ids 4000-4016: ordnance, fuel, time, wingman, guns, enemy type/qty/skill,
   ground targets, ...). A scenario therefore has no localized title in
   `strings.dll`.
3. **`ground_target` has no string**: no briefing block exists for it,
   consistent with F49-D.01 (disallowed in all eight directories).

## Consequence for `f49.d.03.unmeasured.title`

The original's presets are distinguished on its briefing by the type heading;
the stable id scheme `ia_preset/<group>-<leaf>-<type>` is ours. The claim
stays `unmeasured` for the **scenario-level label** (whatever the menu shows to
choose among c1, c1b, ..., c5): no data names it. The type-level heading ids
above may be consumed as the briefing title of a launched preset.

## Still unmeasured (needs the owner, `human_play`)

* Whether the menu offers every (directory, offered type) pair, i.e. all 30
  ids. The descriptors (`spawn_points` minus `disallow_missions`) support the
  product; the menu's own selection logic is engine code and was not traced.
  Resolved only by an original run or a disassembly-backed trace (not
  committable).
* How the menu labels scenarios (it may show a picture, a location drawn
  from the world rather than a string, or just an index).
