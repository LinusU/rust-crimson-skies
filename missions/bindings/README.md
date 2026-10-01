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

The typed records those missions fill in — the seven required content
categories, one unresolved dependency row per required subsystem, coverage
totals and closure reports — live in
`crates/cs_content/src/campaign_bindings.rs`.

## What is not here yet

- `M07.json` … `M24.json` — the per-mission binding outputs of M07-A …
  M24-A, created from original data the same way. Every one of them starts
  unresolved.
- Titles the installation spells differently again — M09, M11, M14, M15, M20,
  M22 and M23 — match neither display form and stay `Uncarried`. Which retail
  mission they name is not established here; see
  `docs/findings/2026-10-01-m05-a-source-binding.md`.

Readiness, coverage and closure are reported, never awarded: synthetic
fixtures prove the schema and its validation only (F50 owner ruling,
2026-09-28). Binding the rest of the identities, running the real campaign
and collecting ordinary-play evidence stay with F50-B, F50-C and F50-D.
