# M14-A: source binding of "Clash of the Dreadnaughts"

Task Rally #297 (M14-A), implemented 2026-10-25 by sonnet-1 after the owner
ruling on guide titles (Rally #470,
`docs/findings/2026-10-25-owner-ruling-guide-titles.md`). Install
`b4e780ab…c631978`.

## What changed since the 2026-10-02 block

The declared title `Clash of the Dreadnaughts` is carried by no localized row.
The earlier attempt therefore blocked. The owner ruled that M14 names the
retail mission at its own campaign index, with the short name `Clash of
Dreadnaughts` (row 3493) and the long name `Hollywood - Nathan Zachary & The
Clash of Dreadnaughts` (row 3463). `SourceContext::bind` now confirms that
ruled spelling exactly and records the title join as `Inferred`.

Correction to the earlier note and to the ruling's table: both retail rows
spell `Dreadnaughts`. `Dreadnaoughts` appears only in the guide-derived task
text, not in the installation (re-read from rows 3463 and 3493 by
`accept_m14_a_the_ruled_spelling_is_carried_exactly_and_the_declared_one_is_not`).

## Result

`missions/bindings/M14.json`: campaign position 13 (the fourteenth inventory
entry, which the join selects independently of the declared order), mission
`mission/ch3-m04`, program `script/c2b-m04-zrdr` (`ZBD/C2B/M04/zrdr.zbd`),
world `world/c2b`. The layout entry for chapter 3 mission 4 lives under the
`C2B` world-group directory; the record cites it as the installation spells it.
All five critical dependencies are resolved; `TitleString`, `MissionId` and
`WorldGroupVariant` are `Inferred`, `InstallHash` and `ProgramSourceMap` are
`ObservedTool`. `verified` is false and the checklist unknowns (objective
graph, actors, difficulty, script/native coverage, closure hash, …) remain,
plus the `title ruling:` unknown naming Rally #470.

## What this does not establish

The ruling is an owner decision resting on order agreement and word overlap,
not an original observation. Nothing here is `verified_original`.
