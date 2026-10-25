# Per-mission playtest routes (F50-C)

One probe route per declared campaign work order: the retry contract of spec
F50 acceptance test AC03 ("retry selected missions after death, bailout,
skip-media, save/restart and settings changes") anchored to the identities
`SourceContext::bind_campaign` resolved from the original installation.
The plan is production data — `probe_routes` in
`crates/cs_content/src/campaign_bindings.rs` derives it from the bound
campaign — and `crates/cs_app/tests/campaign/f50_c.rs` pins this document to
that plan, so a row here that drifts from production fails the acceptance
suite instead of quietly misleading a playtester.

## What a route is

A **ready** route states the whole minimum scenario for one work order:
whichever of the five interruptions ends a run — `DEATH`, `BAILOUT`,
`SKIP_MEDIA`, `SAVE_RESTART` or `SETTINGS_CHANGE` — the next entry re-enters
the **same** mission, world and program, under the same installation
fingerprint the campaign was read under. Each work order therefore needs only
one row: its five reentries are identical by construction, and the acceptance
suite asserts exactly that against a fresh production pass.

A **refused** route is a work order whose identity cannot be resolved (today
none, since the owner's title ruling; without it, seven): there is no mission
to re-enter, so the route names
its refusal, stays in the table and can never be read as probed (spec F50
non-negotiable behavior 5). Any refusal is listed below the table with the
reason production plans.

## How a human plays one

For each ready work order, in any order (the campaign progression is
unmeasured, so this document deliberately does not sequence the missions):

1. Enter the mission at the campaign position below and confirm it is the
   mission the row names (`mission/...` in the `world/...` variant, driven by
   `script/...`).
2. Exercise each interruption once — die; bail out; skip the linked media;
   save the profile and restart the application, then re-enter; change a
   setting mid-session, then re-enter — and confirm every re-entry lands back
   in the same mission rather than another one, a menu or a crash.
3. Record what you saw against the work order. Ordinary-play evidence for
   every mission and ending is F50-D's; automated input is never `human_play`.

Nothing here has been played yet. Executing a route needs the mission launch
path (`VS-M01-RUNTIME`) and the controlled runs (`VS-M01-CONTROLLED-RUNS`);
this document and its plan are the contract those runs and the owner's human
play follow, not evidence that any mission runs.

## The routes

Campaign positions are 0-based rows of the retail campaign the installation
declares; they are the same positions `SourceContext::bind_campaign`
resolved. The installation fingerprint every row was derived under is
recorded in the task's evidence report
(`docs/findings/evidence/F50-C.json`).

| Work order | Position | Mission | World | Program | State |
| --- | --- | --- | --- | --- | --- |
| M01 | 0 | `mission/ch1-m01` | `world/c3` | `script/c3-m01-zrdr` | ready |
| M02 | 1 | `mission/ch1-m02` | `world/c3` | `script/c3-m02-zrdr` | ready |
| M03 | 2 | `mission/ch1-m03` | `world/c3` | `script/c3-m03-zrdr` | ready |
| M04 | 3 | `mission/ch1-m04` | `world/c3` | `script/c3-m04-zrdr` | ready |
| M05 | 4 | `mission/ch1-m05` | `world/c3` | `script/c3-m05-zrdr` | ready |
| M06 | 5 | `mission/ch2-m01` | `world/c1c` | `script/c1c-m01-zrdr` | ready |
| M07 | 6 | `mission/ch2-m02` | `world/c1` | `script/c1-m02-zrdr` | ready |
| M08 | 7 | `mission/ch2-m03` | `world/c1b` | `script/c1b-m03-zrdr` | ready |
| M09 | 8 | `mission/ch2-m04` | `world/c1` | `script/c1-m04-zrdr` | ready |
| M10 | 9 | `mission/ch2-m05` | `world/c1` | `script/c1-m05-zrdr` | ready |
| M11 | 10 | `mission/ch3-m01` | `world/c2` | `script/c2-m02-zrdr` | ready |
| M12 | 11 | `mission/ch3-m02` | `world/c2` | `script/c2-m01-zrdr` | ready |
| M13 | 12 | `mission/ch3-m03` | `world/c2` | `script/c2-m03-zrdr` | ready |
| M14 | 13 | `mission/ch3-m04` | `world/c2b` | `script/c2b-m04-zrdr` | ready |
| M15 | 14 | `mission/ch3-m05` | `world/c2` | `script/c2-m05-zrdr` | ready |
| M16 | 15 | `mission/ch4-m01` | `world/c4` | `script/c4-m01-zrdr` | ready |
| M17 | 16 | `mission/ch4-m02` | `world/c4` | `script/c4-m02-zrdr` | ready |
| M18 | 17 | `mission/ch4-m03` | `world/c4` | `script/c4-m03-zrdr` | ready |
| M19 | 18 | `mission/ch4-m04` | `world/c4` | `script/c4-m04-zrdr` | ready |
| M20 | 19 | `mission/ch4-m05` | `world/c4` | `script/c4-m05-zrdr` | ready |
| M21 | 20 | `mission/ch5-m01` | `world/c5` | `script/c5-m01-zrdr` | ready |
| M22 | 21 | `mission/ch5-m02` | `world/c5` | `script/c5-m02-zrdr` | ready |
| M23 | 22 | `mission/ch5-m03` | `world/c5` | `script/c5-m03-zrdr` | ready |
| M24 | 23 | `mission/ch5-m04` | `world/c5` | `script/c5-m04-zrdr` | ready |

## Refusals

None. All 24 work orders resolve their identity on the installation. Seven of
them (M09, M11, M14, M15, M20, M22, M23) declare a guide title the installation
spells differently; they resolve through the owner's title ruling (Rally #470,
`missions/bindings/title-rulings.tsv`), recorded as inferred rather than
`verified_original`, and the retail spelling is still confirmed exactly. With
that ruling withdrawn the same seven are refused routes again — named in the
table, never dropped and never counted as probed — and the acceptance suite
pins that case. See `docs/findings/2026-10-25-owner-ruling-guide-titles.md`.
