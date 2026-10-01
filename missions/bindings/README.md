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

The typed records those missions fill in — the seven required content
categories, one unresolved dependency row per required subsystem, coverage
totals and closure reports — live in
`crates/cs_content/src/campaign_bindings.rs`.

## What is not here yet

- `M04.json` … `M24.json` — the per-mission binding outputs of M04-A …
  M24-A, created from original data the same way. Every one of them starts
  unresolved.

Readiness, coverage and closure are reported, never awarded: synthetic
fixtures prove the schema and its validation only (F50 owner ruling,
2026-09-28). Binding the rest of the identities, running the real campaign
and collecting ordinary-play evidence stay with F50-B, F50-C and F50-D.
