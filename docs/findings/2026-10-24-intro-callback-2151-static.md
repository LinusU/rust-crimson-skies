# #1964 Intro movie: engine callback 2151 read statically

Date: 2026-10-24. Static analysis of `$CS_ENGINE_IMAGE` (read-only, never committed), disassembled with `llvm-objdump`
into `private/`; the original executable did not run, so every claim is `static` and nothing is `verified_original`.
Implementer: `claude-1` (Sonnet 5.5); no independent review yet. Addresses are virtual addresses in the image.

## Dispatch

`callback($$E$$, 2151, FC)` reaches the dispatcher at `0x4093ab`: ids above `0x833` go through `id - 0x834` into a byte
table at `0x40f788` and a jump table at `0x40f5b4`. Id `0x867` (2151) is the only id mapped to byte 35, whose target is
`0x40bf80`.

## The rule

1. `0x40bf80` calls the engine query `0x407670` with `{7, -2}` (handler `0x40793c`): with `-2` it takes the current
   mission index `[0x64b678]`, and `0x419ce0` maps it through the mission table (`0x463bb0`, the dword at entry
   `+0x730` of 52-byte entries, 25 entries, out of range gives 0) to a value 1..5 (0 when unset). Call it the chapter
   `ch`: the cabin script builds `chap<FC>.mpg` from the result, and `chap1..chap5` are the five chapter movies.
2. It reads the 168-byte mission record number `5 * (ch - 1)` of the array at `0x64cc50` (24 records): the dwords at
   record offsets `+0x04` (`0x64cc54`) and `+0x58` (`0x64cca8`) (`0x40bfb1..0x40bfc3`).
3. If **either is non-zero** the result is 0, else the result is `ch`. (`0x40bfc5..0x40bfd6`, `0x40bfe4..0x40bff2`: the
   `neg`/`sbb` pair turns "non-zero" into the mask.) `ch == 0` gives 0.

So the value domain of `FC` is **{0} ∪ {1..5}**, and `PASSENGERCABIN.SCRIPT`'s `if (FC != 0)` plays the intro exactly
when the chapter of the current mission is known and the first mission of that chapter has neither flag set. The
owner's capture (a new pilot, `chap1`) agrees: a fresh pilot's mission is 0, records are zero, `ch == 1`.

## Per-profile state

* The pilot state block `0x64b340..0x64dd84` (`0xa91` dwords) holds `nMission` (`0x64b340`), the current mission
  `[0x64b678]` and the records. A new pilot zeroes it (`0x40278d`, and `0x411414..0x411431` zero the records and the
  current mission).
* The block is written and read by the save/load callbacks at `0x416be0` / `0x416c00` / `0x416c20` / `0x416c50`
  (`0x2a44` bytes from `0x64b340`). So the gate's inputs are part of the saved state. Whether that save is the pilot
  file or only the game save is not traced.
* Writers of the two flags: callback 2104 (`0x409f27`, "start mission") sets `+0x04` and `+0x58` to 1 on every record
  it advances past when the target mission is more than one ahead (the `idaho` cheat path), and copies the previous
  record's loadout words (`0x64cc28`, `0x64cc2c`) into it. The ordinary completion writer, and whether the flags mean
  "passed" or "loadout set", is **not** established.

## Unknown

* The writer of the flags on a normal mission pass and their meaning (affects: when chapters 2..5 and a returning
  pilot's chapter 1 see the intro).
* The mission-to-chapter table (`+0x730` entries; filled from the campaign data at load, source not traced).
* Whether the save that holds the state is the pilot's own file.
* The audio of the intro and the same-instant frames: see below.

## Changed

`cs_app::ui::front_end::intro`: `chapter_gate`, `gate_record`, `MissionFlags`, `MISSIONS_PER_CHAPTER`, `GATE_CALLBACK`
(the rule, with tests `accept_parity5_intro_2151_*`). **Not wired** into the pilot-page entry: the app has no mission
progress or flags to feed it, so the intro still plays on every entry from the pilot page, as every pilot is a fresh
one. Wiring is an open follow-up.

## Not done

* Intro audio assertion/hearing: needs `human_review` (an agent cannot hear it).
* Chap1 frames at 15.4 s and 31.4 s beside the owner's: the owner's intro frames are not in this checkout
  (`private/ref-drydock` holds menu, pilot and cabin only) and this host has no frame extractor; the instants and the
  match scores are in `2026-10-21-parity5-cabin-intro-measurement.md`.
