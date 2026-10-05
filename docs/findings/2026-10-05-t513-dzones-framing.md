# #513: the campaign `dzones.zrd` member's framing, decoded

Date: 2026-10-05. Task: #513 (follow-up of #427, whose finding record is
`2026-10-02-t427-retail-trigger-volume-thickness.md`). Capability used: **`retail`**
(read access to `$CS_GAME_DIR`). **Nothing here is `verified_original`**; no original
run happened. A reviewer with a fresh context should check this format work.

## The measurement that resolves #427's open question

#427 found that reading the member as tag / count / counted values breaks on the first
list. The word after a `4` tag is **not** the child count: it is the child count **plus
one** — a list word `N` holds `N - 1` children. That is the rule `cs_content`'s general
`.zrd` decoder (`stunts::decode_zrd`) already used, found by F09; #427's grammar tried
`N` and so saw "the same shape storing different counts". Read with `N - 1`, **all 23**
members (`ZBD/C1/{M02,M04,M05}`, `C1B/M03`, `C2/{M01,M02,M03,M05}`, `C3/M01..M05`,
`C4/M01..M05`, `C5/{IA1,M01..M04}`; 9 197 bytes, 155 to 826 each) decode with **no
trailing byte**.

## The grammar (all measured)

Tags: `1` int (one `u32`), `3` string (`u32` length + bytes), `4` list (`u32` `N`, then
`N - 1` children). Tag `2` occurs in no member. The member is one list of alternating
key and value. Three keys, each at most once per member:

| key | value | members |
| --- | --- | --- |
| `disable` | list of `dzpath<N>` strings | 16 |
| `nosnapshot` | list of `dzpath<N>` strings | 12 |
| `objective_numbers` | list of `[dzpath<N>, int]` pairs | 20 |

Objective integers measured 18 to 31, unique within a member.

## What is still unknown

What `disable`, `nosnapshot` and an objective number **mean** to the original
(`KeyMeaning::Unknown` for all three, `ClaimStatus::Unknown`). In particular: the
integer is an objective number by the key's spelling only; which objective it indexes
is `objectives.zrd`'s question (F13-D / F39, opcode table unmeasured). A zone a member
does not name is not thereby unused. The **trigger placement** claims #427 gated stay
gated.

## What changed

- `crates/cs_formats/src/zbd/detection_zones.rs` (new): `read_detection_zones`,
  refusing by name `Truncated`, `UndefinedTag`, `LengthDoesNotFit`, `ZeroListWord`,
  `InvalidText`, `DepthExceeded`, `TrailingBytes` and the structural `NotAKeyedRecord`,
  `UnknownKey`, `DuplicateKey`, `WrongValueShape`.
- `crates/cs_content/src/world.rs`: `MissionZoneDeclaration`, `ZoneDeclarationKey`,
  `ZoneDeclarationGap`, and on `RetailTriggerVolumeSurvey`: `with_declarations`,
  `declarations`, `declaration_gaps`. `zone_declarations_are_decoded()` is now `true`
  once the mission side is attached (it was a constant `false`).
- `crates/cs_app/src/world/triggers.rs`: the survey walks every `zrdr.zbd` through
  production discovery, decodes each `dzones.zrd`, and attaches the rows. An
  undecodable member aborts the survey by name (`Declarations`).
- Wiring: `crates/cs_formats/src/zbd/mod.rs`, `crates/cs_formats/tests/zbd/main.rs`.
- Tests: `accept_t427_dzones_*` in `crates/cs_formats/tests/zbd/dzones.rs` (4) and
  `crates/cs_app/tests/world/triggers.rs` (3 unignored, 1 retail). The two #427
  `zone_declarations_are_decoded` assertions flipped; the retail
  `..._still_undecoded` test is replaced by the retail cross-check.

## Cross-check result

Over the installation: 23 declarations, 16 / 12 / 20 members state
`disable` / `nosnapshot` / `objective_numbers`, and **zero** declared zones lack a node
in their mission's world container (`declaration_gaps()` is empty). The gap mechanism
is exercised by a synthetic test, so an empty result is a measurement, not a dead path.

## Mutation check

`read_mission_declarations` made to skip every member: the retail test and both
synthetic survey tests fail (3 of 4 `accept_t427_dzones_` survey tests; the fourth is
the pure content-layer gap test).

`MissionWithoutWorld` is defensive: production discovery treats any directory holding a
mission reader as a world group, so no fixture reaches it; it is untested.
