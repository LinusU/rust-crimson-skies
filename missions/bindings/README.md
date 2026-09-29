# Mission bindings

The binding records for the campaign: one work order at a time, bound to the
original data the private installation actually contains. Nothing in this
directory is original game data — no assets, scripts or extracted bytes —
and nothing here has been read from `$CS_GAME_DIR` yet.

## What is here now (stage F50-A)

- `campaign-inventory.tsv` — the frozen denominator of the campaign: one
  `label<TAB>title` line per mission work order, read by
  `cs_content::campaign_bindings::CampaignInventory`. It declares which
  missions coverage is measured against; it does not claim any of them is
  bound, playable or verified. `crates/cs_app/tests/campaign/` asserts the
  list still matches the work orders in `../README.md`, so the denominator
  cannot shrink without a failing test.

The typed records those missions will fill in — the seven required content
categories, one unresolved dependency row per required subsystem, coverage
totals and closure reports — live in
`crates/cs_content/src/campaign_bindings.rs`.

## What is not here yet

- `M01.json` … `M24.json` — the per-mission binding output named by the
  work-order sheets (for example `missions/M01.md`: "Binding output:
  `missions/bindings/M01.json` created from the template in
  `examples/mission-bindings/M01.json`"). Those are created by the
  per-mission binding stages (M01-A … M24-A) from original data, with
  `schemas/mission-binding.schema.json` as their contract. Every binding
  starts unresolved.

Readiness, coverage and closure are reported, never awarded: synthetic
fixtures prove the schema and its validation only (F50 owner ruling,
2026-09-28). Binding real identities, running the real campaign and
collecting ordinary-play evidence stay with F50-B, F50-C and F50-D.
