# M14-A: the declared title is carried by no retail string, so the stage cannot start

Date: 2026-10-02. Task: M14-A "Bind original mission data and branches" (#297,
`missions/M14.md`, work order `M14-A`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written). Implementer: **bunny-alpha-2/bunny-alpha-2**
(session of 2026-10-02T00:19Z). Review: none; the task is **blocked** and nothing
was submitted for review.

**Outcome: M14-A is blocked, not delivered.** Its minimum acceptance scenario —
"Source-derived binding has no unresolved critical dependencies" — cannot be met
on this installation, because the declared work-order title `Clash of the
Dreadnaoughts` is spelled nowhere in the original data. This finding records the
measurement so the owner decision already filed as #470 (`M05-A-GUIDE-TITLES`)
and #448 (`M02-T1`) has a complete, freshly measured basis for M14. No
production code, no binding record, no acceptance test and no evidence report
was written: producing a partial M14-A would mean publishing an evidence report
for a scenario the installation contradicts.

## What the work order asks for

`missions/M14.md` names this stage's minimum scenario and its `M14-BIND` row:

> **M14-A - Bind original mission data and branches** — Dependencies: F14-C,
> F13-C, F50-A. Test prefix: `accept_m14_a_`. Required capabilities: retail.
> Source-derived binding has no unresolved critical dependencies.
>
> **M14-BIND:** Find the corresponding original catalog/program/world
> identities. Confirm the title against local strings; do not key runtime logic
> by this discovery label.

The sheet also states, above those rows: "Title spelling, localization and
bindings must be checked against the owners original installation", and "Unknown
values remain null and block readiness."

## The installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| Localized UI table | `GOSDATA/ASSETS/BINARIES/langui.dll`, 282 624 bytes, 1616 rows, one language (1033) |
| Other string images | `strings.dll` (131 072 bytes, 1792 rows), `GOSDATA/ASSETS/BINARIES/language.dll` (32 768 bytes, 48 rows) |

## What was measured

### 1. The campaign-length row blocks

`cs-inspect config --file "$CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/langui.dll"`
decodes the PE string image. Grouping every row whose display text is non-empty
into maximal consecutive runs leaves exactly two runs of the campaign's length
(24):

| Block | Rows | Content |
| --- | --- | --- |
| long names | `3450..=3473` | `<region> - <long name>`, region groups of `5/5/5/5/4` |
| short names | `3480..=3503` | the bare short name of the same mission |

The declared inventory order (`missions/bindings/campaign-inventory.tsv`, M01 …
M24) is compared with the two blocks index by index below. The `14` in the
leading display tag `[AB14I]` is **not** a mission number: that exact tag is on
all 48 rows of both blocks.

### 2. Every declared title against both rows of its offset

Read from the decoded report; `long tail` is the part after the first `" - "`.

| # | Work order | Declared title | Short-name row | Long-name tail |
| --- | --- | --- | --- | --- |
| 0 | M01 | The Lost Treasure | `The Lost Treasure` (3480) | `The Lost Treasure of Sir Francis Drake` (3450) |
| 1 | M02 | The Bomber Heist | `The Bomber Heist` (3481) | `The Great British Bomber Heist` (3451) |
| 2 | M03 | The Secret Invasion | `The Secret Invasion` (3482) | `Nathan Zachary & The Secret Invasion` (3452) |
| 3 | M04 | The Sinister Sub | `The Sinister Sub` (3483) | `Nathan Zachary & The Sinister Sub` (3453) |
| 4 | M05 | The Union Jack's Revenge | `Union Jack's Revenge` (3484) | `The Union Jack's Revenge` (3454) |
| 5 | M06 | The Red Menace | `The Red Menace` (3485) | `Nathan Zachary & The Red Menace` (3455) |
| 6 | M07 | The Pilfered Prototype | `The Pilfered Prototype` (3486) | `Nathan Zachary & The Pilfered Prototype` (3456) |
| 7 | M08 | The Petrol Plot | `The Petrol Plot` (3487) | `Nathan Zachary & The Petrol Pit` (3457) |
| 8 | M09 | Perils for Blake | `Peril for Blake` (3488) | `Peril for Paladin Blake` (3458) |
| 9 | M10 | Mercy's Errand | `Mercy's Errand` (3489) | `Nathan Zachary & Mercy's Errand` (3459) |
| 10 | M11 | The Stolen Scarlet | `The Stolen Starlet` (3490) | `Nathan Zachary & The Stolen Starlet` (3460) |
| 11 | M12 | The Great Plane Robbery | `The Great Plane Robbery` (3491) | `The Great Plane Robbery` (3461) |
| 12 | M13 | The Nefarious Trap | `The Nefarious Trap` (3492) | `Nathan Zachary & The Nefarious Trap` (3462) |
| **13** | **M14** | **`Clash of the Dreadnaoughts`** | **`Clash of Dreadnaughts` (3493)** | **`Nathan Zachary & The Clash of Dreadnaoughts` (3463)** |
| 14 | M15 | The Fight for the Figaro | `Fight for the FIGAROA` (3494) | `The Fight for the FIGAROA` (3464) |
| 15 | M16 | Raid on the Rocky Express | `Raid on the Rocky Express` (3495) | `Raid on the Rocky Express` (3465) |
| 16 | M17 | The Pirate's Duel | `The Pirate's Duel` (3496) | `Nathan Zachary & The Pirate's Duel` (3466) |
| 17 | M18 | Deceit at Devil's Horn | `Deceit at Devil's Horn` (3497) | `Deceit at Devil's Horn` (3467) |
| 18 | M19 | Rescue the Black Swan | `Rescue the Black Swan` (3498) | `Rescue the Black Swan` (3468) |
| 19 | M20 | Unholy Alliance | `The Unholy Alliance` (3499) | `Nathan Zachary & The Unholy Alliance` (3469) |
| 20 | M21 | Death on the Docks | `Death on the Docks` (3500) | `Death on the Docks` (3470) |
| 21 | M22 | Runaway Witness | `The Runaway Witness` (3501) | `Nathan Zachary & The Runaway Witness` (3471) |
| 22 | M23 | Criminal Exodus | `The Criminal Exodus` (3502) | `Nathan Zachary & The Criminal Exodus` (3472) |
| 23 | M24 | Battle over Broadway | `Battle over Broadway` (3503) | `Battle over Broadway` (3473) |

M14 is one of the seven work orders whose declared title is carried in **neither**
display form: its declared title has both a leading article the short name drops
**and** the misspelling `Dreadnaoughts`, where the installation consistently
writes `Dreadnaughts` in both of its rows. This is the same set M05-A measured
(`docs/findings/2026-10-01-m05-a-source-binding.md`), re-measured here for M14.

### 3. What production code derives from it

`SourceContext::confirm_title` and `SourceContext::bind`
(`crates/cs_content/src/campaign_bindings.rs`) were run against the installation
with the declared inventory title. Verbatim output:

```text
confirm_title -> Uncarried
MissionId        => unresolved: the discovery title is carried by no single localized row
InstallHash      => resolved (ObservedTool)
TitleString      => unresolved: no localized string carries the discovery title, neither
                             verbatim nor as the title part of a region-prefixed long name
ProgramSourceMap => unresolved: the discovery title is carried by no single localized row
WorldGroupVariant=> unresolved: the discovery title is carried by no single localized row
unresolved_critical = [MissionId, TitleString, ProgramSourceMap, WorldGroupVariant]
campaign_position = None
localized_title_id = None
join state = Agreed
title blocks = [TitleBlock { first_id: 3450, last_id: 3473 },
                TitleBlock { first_id: 3480, last_id: 3503 }]
chapter_sizes = [5, 5, 5, 5, 4]
campaign[13] = Some(CampaignMission { chapter: 3, mission_number: 4,
                                    world_group: "c3",
                                    program_asset: "ZBD/C3/M04/zrdr.zbd",
                                    program_present: true })
```

So **four of the five** critical dependencies of the data-binding checklist are
unresolved for M14, and the minimum acceptance scenario fails by exactly those
four. `source_spans` is empty, `catalog_id`, `world_id`, `program_id` and
`closure_sha256` are null, and `verified` is `false`.

Note what is *not* wrong here: the localized table corroborates the campaign
directory layout (`join state = Agreed`, both blocks are exactly 24 rows and the
long names group into the layout's `5/5/5/5/4` chapter sizes). M02-A's join
machinery works on this installation. What M14 lacks is a title to feed it.

The campaign layout's fourteenth mission is `chapter 3 / mission 4`,
`ZBD/C3/M04/zrdr.zbd`, 34 287 bytes, SHA-256
`ea5243a227e97b54d3970d5f36722545ca308fae61c22ba16457305397277c3e`. **This
finding does not claim that archive is M14.** Reading M14 as the fourteenth
mission would key the binding to the discovery label's ordinal, which
`missions/M14.md` forbids ("do not key runtime logic by this discovery label")
and which no observation in the installation supports.

### 4. The declared spelling exists nowhere in the installation

A byte search over all 228 files (842 048 219 bytes) for `Clash of the
Dreadnaoughts`, `Clash of Dreadnaoughts`, `Dreadnaoughts` and `Dreadnaughts`, in
both ASCII and UTF-16LE, finds exactly one file:

```text
GOSDATA/ASSETS/BINARIES/langui.dll  Dreadnaughts [utf16]: 2
```

Two UTF-16LE occurrences, i.e. the two rows `3463` and `3493`, both spelled
correctly by the original program. The guide's `Dreadnaoughts` appears in no
stored byte of any file in the installation.

The limit of that statement, stated so it is not over-read: a byte search cannot
see text inside a compressed container member. A mission briefing that carries
the misspelled name would be inside a `ZBD` member and is **not** excluded by
this measurement. What is excluded is the localized UI string surface, which is
decoded by production code, not searched raw.

### 5. The other two string images carry nothing

`strings.dll` (1792 rows) and `GOSDATA/ASSETS/BINARIES/language.dll` (48 rows)
were decoded the same way. Neither contains a row whose text holds `dreadn` or
`clash` in any case. The two rows of §2 are the whole of the original program's
evidence about this mission's name.

## Why the stage cannot be completed

Two routes would produce a record that *reads* as bound. Both are forbidden, and
the second is forbidden twice over:

1. **Fuzzy or normalizing title matching** — tolerating a leading article,
   collapsing case, or matching `Dreadnaoughts` against `Dreadnaughts`. This is
   the guess AGENTS.md rule 4 rules out and that M05-A deliberately declined to
   implement ("tolerating an article is a guess about how the public guide spells
   a title"). A binding that reads as resolved while pointing at a row the
   program never spelled that way is the exact failure these stages exist to
   prevent (#448, "What must NOT happen here").
2. **Ordinal binding** — treating M14 as the fourteenth work order and therefore
   campaign position 13, i.e. `mission/ch3-m04` / `world/c3` /
   `script/c3-m04-zrdr`. This is what the *evidence* the join provides: the
   declared inventory order and the original's mission names agreeing row by
   row. Assuming it for M14 is assuming the thing the stage exists to check, and
   it is the label-keyed substitution the M14 sheet names as not passing.

Either way `CriticalDependency::TitleString` would still have no answer, because
no retail string carries the declared title: the checklist entry "title string"
is not satisfied by a row the original program does not spell that way.

## What the owner must decide

Already filed and open:

- **#470 `M05-A-GUIDE-TITLES`** (blocked on the owner) — rule on the seven
  declared titles the installation spells differently. Its criterion 2 offers
  both arms: each affected work order *either* binds with no unresolved critical
  dependency, *or* "stays explicitly unresolved with a reason that names the
  owner decision it waits on". M14 is in the second arm today.
- **#448 `M02-T1`** (todo) — the same eight-title question with the acceptance
  criteria, including "the chosen resolution is implemented without loosening
  the exact-match rule".

Concretely the owner is asked for, for M14:

(a) a ruling that the work order `M14` names the retail mission whose two
localized rows read `Clash of Dreadnaughts` (3493) and
`Nathan Zachary & The Clash of Dreadnaoughts` (3463), on the stated evidence,
recorded as `ClaimStatus::Inferred` and never as `verified_original`; or
(b) an edit of the declared title in `missions/README.md` **and**
`missions/bindings/campaign-inventory.tsv`, which must stay in step because
`crates/cs_app/tests/campaign/inventory.rs` asserts they match — both are
protected paths and this task has `allowProtectedChanges: false`; or
(c) a ruling that the M14-A sheet's minimum scenario is wrong for a
guide-misspelled title and states what the stage should accept instead.

Until one of those is recorded, M14-A stays blocked, and so do its dependents
M14-B (#298) and M14-C, whose regression priorities (exposed weapon bays,
capital survival, chapter transition state) all require the mission, world and
program identities to be bound first.

## What was deliberately not done

- **No production change.** The exact-comparison rules
  (`TitleForm::Verbatim`, `TitleForm::RegionPrefixedLongName`, `title_form`,
  `confirm_title`) are left exactly as M05-A left them.
- **No `missions/bindings/M14.json`.** A record with four unresolved critical
  dependencies would be honest as a *report of a blocker*, but writing it as a
  stage output would present a mission binding that does not exist. The
  unresolved state is reported here instead.
- **No `accept_m14_a_*` test and no evidence report.** An acceptance suite for
  this stage would have to assert that M14 has no unresolved critical
  dependency, and that assertion is false on this installation. AGENTS.md rule 5
  and the "Done when" clause of #297 both say to block instead of producing
  partial evidence.
- **No ordinal binding**, for the reasons in the previous section.

## Reproduce

From the workspace root, with `CS_GAME_DIR` set to the read-only installation:

```sh
# 1. the two campaign-length blocks and M14's two rows
cs-inspect config --file "$CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/langui.dll" \
  --out private/langui.json

# 2. the other two string images
cs-inspect config --file "$CS_GAME_DIR/strings.dll" --out private/strings.json
cs-inspect config --file "$CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/language.dll" \
  --out private/language.json

# 3. what production code derives for the declared title, through a scratch
#    test module in crates/cs_app/tests/campaign/ wired into main.rs:
#      SourceContext::read(&game_dir)?.confirm_title("Clash of the Dreadnaoughts")
#      SourceContext::read(&game_dir)?.bind(MissionLabel::new("M14")?,
#                                          "Clash of the Dreadnaughts")?
#    (the probe used for §3 was removed again; nothing of it is committed)
```

The scratch probe was removed rather than committed because a test named for
this stage would be picked up by an `accept_m14_a_` selection that has no
passing scenario to select. Its output is reproduced verbatim in §3.