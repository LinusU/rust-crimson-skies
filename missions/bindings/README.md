# Mission bindings

The binding records for the campaign: one work order at a time, bound to the
original data the private installation actually contains. Nothing in this
directory is original game data — no assets, scripts or extracted bytes.

## What is here

- `campaign-inventory.tsv` — the frozen denominator of the campaign: one
  `label<TAB>title` line per mission work order, read by
  `cs_content::campaign_bindings::CampaignInventory`. It declares which
  missions coverage is measured against; it does not claim any of them is
  bound, playable or verified. `crates/cs_app/tests/campaign/` asserts the
  list still matches the work orders in `../README.md`, so the denominator
  cannot shrink without a failing test.
- `M01.json` — the M01 binding output (stage M01-A), in the shape
  `schemas/mission-binding.schema.json` describes. It is **generated** by
  production code: `SourceContext::read` + `SourceContext::bind`
  (`crates/cs_content/src/campaign_bindings.rs`) derive it from
  `$CS_GAME_DIR`, and `accept_m01_a_the_committed_record_is_what_the_installation_derives`
  fails if the committed file and the freshly derived record differ. It
  carries identities, hashes and byte-range spans — never original bytes.

  Its `catalog_id`, `world_id` and `program_id` are resolved, and its
  `install_sha256` matches production discovery, so the record has no
  unresolved critical dependency. `verified` is deliberately `false` and
  `unknowns` deliberately non-empty: the checklist entries M01-A does not
  bind (actors, objectives, media, rewards, difficulty branches, precedence,
  progression, `closure_sha256`) are recorded there and keep the mission out
  of any readiness claim.
- `M02.json` — the M02 binding output (stage M02-A), generated and pinned the
  same way by `accept_m02_a_the_committed_record_is_what_the_installation_derives`.
  It is source-derived under the same five critical dependencies and equally
  unverified. M02-A adds the check that makes the work-order ↔ retail-mission
  join more than one structure's word: `SourceContext::join_agreement`
  compares the localized table's campaign-length row blocks against the
  campaign directory layout, and `campaign_position_for` derives no position
  at all when they contradict each other. `docs/findings/2026-10-01-m02-a-source-binding.md`
  records what that does and does not establish.
- `M03.json` — the M03 binding output (stage M03-A), generated and pinned the
  same way by `accept_m03_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it: it runs M02-A's join and corroboration at
  the third campaign position. Its world group, `world/c1b`, is the opposite of
  M02's: one campaign mission, but a directory that also holds subdirectories
  outside the campaign layout (see
  `docs/findings/2026-10-01-m03-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M04.json` — the M04 binding output (stage M04-A), generated and pinned the
  same way by `accept_m04_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: it runs M02-A's join and
  corroboration at the fourth campaign position. Its world group, `world/c1`,
  is shared with two other campaign missions, and its mission number `4` is
  reused by one mission in every chapter, so neither the world id nor the
  mission number identifies the mission — only the campaign position does (see
  `docs/findings/2026-10-01-m04-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M05.json` — the M05 binding output (stage M05-A), generated and pinned the
  same way by `accept_m05_a_the_committed_record_is_what_the_installation_derives`.
  M05 is the first work order whose declared discovery title the installation
  does not spell the way its bare short-name row does: `The Union Jack's
  Revenge` is carried only as the title part of the region-prefixed long name
  of campaign position 4, while the short name of that same position reads
  `Union Jack's Revenge`. Production code therefore confirms a title in either
  of two observed display forms — verbatim, which wins whenever a row offers
  it, or as the tail of a region-prefixed long name — always by exact
  comparison, and it records the second spelling in `unknowns` instead of
  choosing between the two. `docs/findings/2026-10-01-m05-a-source-binding.md`
  records what that does and does not establish.
- `M06.json` — the M06 binding output (stage M06-A), generated and pinned the
  same way by `accept_m06_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M06 is the sixth campaign position
  and the first mission of chapter 2, so it is the first binding stage to cross
  out of chapter 1. Its world group, `world/c2`, is shared with four
  campaign missions and chapter 2's fifth mission lives in the separate `c2b`
  directory (see `docs/findings/2026-10-01-m06-a-source-binding.md`). Same five
  resolved critical dependencies, same unverified status.
- `M07.json` — the M07 binding output (stage M07-A), generated and pinned the
  same way by `accept_m07_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M07 is the seventh campaign
  position, chapter 2's second mission, one row after the chapter and region
  boundary M06-A bound. Its world group `world/c2` is shared with M06's own
  mission, so the world id names no single mission, and its mission number `2`
  is reused by one mission in every chapter (see
  `docs/findings/2026-10-02-m07-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M08.json` — the M08 binding output (stage M08-A), generated and pinned the
  same way by `accept_m08_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M08 is the eighth campaign position
  and the third mission of chapter 2, so — unlike M06 — neither the layout's
  chapter boundary nor the localized region boundary falls on it; both
  structures must place it strictly inside a group. It shares `world/c2` with
  M06 (see `docs/findings/2026-10-02-m08-a-source-binding.md`). Same five
  resolved critical dependencies, same unverified status.
- `M10.json` — the M10 binding output (stage M10-A), generated and pinned the
  same way by `accept_m10_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M10 is the tenth campaign position
  and the *last* mission of chapter 2, so it is the first binding stage to pin
  a trailing boundary — the chapter and the localized region group both end
  immediately after it. Inside that chapter the campaign order is not the
  directory order: position 8 lives in `C2B` while position 9 is back in `C2`,
  which sorts earlier (see
  `docs/findings/2026-10-02-m10-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M12.json` — the M12 binding output (stage M12-A), generated and pinned the
  same way by `accept_m12_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M12 is the twelfth campaign
  position and the second mission of chapter 3, the first binding stage in
  that chapter. Its world group, `world/c3`, is the whole chapter, so the
  world row identifies the chapter and not the mission (see
  `docs/findings/2026-10-02-m12-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M13.json` — the M13 binding output (stage M13-A), generated and pinned the
  same way by `accept_m13_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M13 is the thirteenth campaign
  position and the middle (third of five) mission of chapter 3, so both the
  layout's chapter boundary and the long names' region boundary lie at least two
  rows away. It shares `world/c3` with M12, which is the whole chapter (see
  `docs/findings/2026-10-02-m13-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M16.json` — the M16 binding output (stage M16-A), generated and pinned the
  same way by `accept_m16_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M16 is the sixteenth campaign
  position and the first mission of chapter 4, so — as at M06 — the layout's
  chapter boundary and the localized region boundary fall on the same row.
  Its world group, `world/c4`, is the whole chapter, so the world row
  identifies the chapter and not the mission (see
  `docs/findings/2026-10-02-m16-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M17.json` — the M17 binding output (stage M17-A), generated and pinned the
  same way by `accept_m17_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: it runs M02-A's join and
  corroboration at the seventeenth campaign position. Two things are measured
  here that the earlier stages did not have to be. M17's declared title is
  carried by the installation's bare short-name row and by **no** long name —
  the region-prefixed form inserts `Nathan Zachary &` between the prefix and
  the title — so unlike `M05.json` (whose declared title only the
  region-prefixed long-name row carries) this record carries no
  `title spelling` unknown. `M12.json` and `M16.json` also carry none, but for
  the other reason: their titles are carried both verbatim and as a long-name
  tail, so the verbatim form wins and the spelling arm is never reached. What
  M17's own tail does resolve is the same campaign position, and therefore the
  same three retail identities. And position 16 is the *second* mission of
  chapter 4, one row past the boundary M16 measured, whose world group
  `world/c4` holds all five chapter-4 missions while the mission number `2`
  names one mission in every chapter: the program archive is the only
  per-mission discriminator, and `ZBD/C4/M02/zrdr.zbd` is byte-length-distinct
  from every sibling in its chapter (see
  `docs/findings/2026-10-02-m17-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M18.json` — the M18 binding output (stage M18-A), generated and pinned the
  same way by `accept_m18_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M18 is the eighteenth campaign
  position and the *third* mission of chapter 4, so — as at M12 and M13, unlike
  M16 — both the layout's chapter boundary and the localized region boundary
  lie two rows away. Its world group, `world/c4`, is the whole chapter, and
  the mission number `3` names a mission directory in every chapter
  (`ZBD/C1B/M03` … `ZBD/C5/M03`) with five different program archives, so
  neither the world id nor the mission number identifies the mission. Unlike
  M05-A, the two display forms *agree* on the title, so the record carries no
  title-spelling note. M18-A adds the one refusal arm no earlier stage reached
  on real data: a *confirmed* localized row that names no campaign position
  because it sits outside every campaign-length block — measured on M18's own
  region prefix, which the installation carries as a standalone string (see
  `docs/findings/2026-10-02-m18-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M19.json` — the M19 binding output (stage M19-A), generated and pinned the
  same way by `accept_m19_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M19 is the nineteenth campaign
  position and the fourth of chapter 4's five missions, so the layout's
  chapter end and the localized region end both fall one row after it. It
  shares `world/c4` with M16, which is the whole chapter (see
  `docs/findings/2026-10-02-m19-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M21.json` — the M21 binding output (stage M21-A), generated and pinned the
  same way by `accept_m21_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M21 is the twenty-first campaign
  position and the first of chapter 5, whose four missions share `world/c5`,
  so the world row identifies the chapter and not the mission (see
  `docs/findings/2026-10-02-m21-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.
- `M24.json` — the M24 binding output (stage M24-A), generated and pinned the
  same way by `accept_m24_a_the_committed_record_is_what_the_installation_derives`.
  No production code changed for it either: M24 is the twenty-fourth and last
  campaign position, the fourth mission of chapter 5 (`world/c5`, shared with
  the other three, so the world row identifies the chapter and not the mission),
  and the last row of the localized long names' final region group (see
  `docs/findings/2026-10-02-m24-a-source-binding.md`). Same five resolved
  critical dependencies, same unverified status.

The typed records those missions fill in — the seven required content
categories, one unresolved dependency row per required subsystem, coverage
totals and closure reports — live in
`crates/cs_content/src/campaign_bindings.rs`.

## The whole campaign (F50-B)

Stage F50-B is the one call that binds *all* of them at once instead of one
work order at a time: `SourceContext::bind_campaign` reads the installation
once, binds every line of `campaign-inventory.tsv` in file order through
`SourceContext::bind`, and assembles the result over the frozen denominator
with `assemble_campaign`. What comes back is a `BoundCampaign`: the
`CampaignBindings` record coverage and closures are measured against,
beside the `SourceBinding`s it was derived from (so a reader can check that
every work order carries the one installation fingerprint the call read
under).

It produces no file here — the per-mission `M01.json` … `M24.json` records
stay the output of `M01-A` … `M24-A`, and nothing this stage does changes
them. What it establishes is that the campaign as a whole assembles the way
the records promise: one record per declared work order, an identity the
installation resolves recorded as a complete `mission_identity` cell, an
identity it does not resolve recorded as an explicitly unknown cell that
keeps its refusal, and a refusal (not a silent drop) for any work order the
assembly would otherwise lose. The stage's acceptance suite is
`crates/cs_app/tests/campaign/f50_b.rs`, and
`docs/findings/missions/2026-10-08-f50-b-campaign-binding.md` records what it
measured.

## Per-mission probe routes and playtest routes (F50-C)

Stage F50-C turns the bound campaign into the retry contract every later
mission stage plugs into: `probe_routes` (also in
`crates/cs_content/src/campaign_bindings.rs`) plans **one probe route per
declared work order**, anchored to the identities the installation resolved
for it and to the one fingerprint the campaign was read under. A ready route
states the minimum acceptance scenario of spec F50-C — after a death, a
bailout, skipped media, a save/restart or a settings change, the next entry
re-enters the same mission, world and program (`ProbeInterruption::ALL` is
exactly those five). A work order whose identity did not resolve gets a
*refused* route: named, counted, in its declared position, never dropped.

`playtest-routes.md` in this directory is the human half: the same plan as a
table a playtester follows, one row per work order plus the refusals verbatim.
It is pinned to production by
`accept_f50_c_the_playtest_route_document_lists_every_work_order_as_planned`,
so it cannot drift from what the installation derives. The stage produces no
file for any `M*.json` record and changes none of them.

What F50-C does **not** claim: no mission has been played or retried in a
runtime. Executing a route needs `VS-M01-RUNTIME` and
`VS-M01-CONTROLLED-RUNS`; ordinary-play evidence for every mission and ending
stays with F50-D, and the owner's human approval with M*C. The plan is the
contract those consumers drive.

## What is not here yet

- `M09.json`, `M11.json`, `M14.json`, `M15.json`, `M20.json`,
  `M22.json` and `M23.json` — the per-mission binding outputs of
  M09-A, M11-A, M14-A, M15-A, M20-A, M22-A and M23-A, created from
  original data the same way. Every one of them starts unresolved. (Listed
  one by one rather than as ranges, because M19, M21 and M24 are bound while
  M20, M22 and M23 are not.)
- Titles the installation spells differently again — M09, M11, M14, M15, M20,
  M22 and M23 — match neither display form and stay `Uncarried`. Which retail
  mission they name is not established here; see
  `docs/findings/2026-10-01-m05-a-source-binding.md`.

Readiness, coverage and closure are reported, never awarded: synthetic
fixtures prove the schema and its validation only (F50 owner ruling,
2026-09-28). F50-B binds the campaign's identities from the installation and
runs its prerequisite closures, and F50-C plans the per-mission probe routes
and pins the human playtest route document to them; the campaign progression
is still unmeasured and the campaign is still not ready, no mission has been
played, and collecting ordinary-play evidence stays with F50-D.
