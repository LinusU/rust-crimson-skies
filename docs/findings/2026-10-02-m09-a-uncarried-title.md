# M09-A: "Perils for Blake" is carried by no retail string — blocked on the guide-title ruling

Date: 2026-10-02. Task: M09-A "Bind original mission data and branches"
(#282, `missions/M09.md`, work order `M09-A`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(`$CS_GAME_DIR` read-only). Agent: **Jakob - Devin SWE-2/devin-1**. Outcome:
**blocked**, waiting on the owner ruling filed as `M05-A-GUIDE-TITLES` (#470).

## What was measured (independently, this session)

Installation `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`
(same install as M01-A…M06-A).

- The declared M09 discovery title `Perils for Blake` is carried by **no**
  localized row, in either observed display form. The byte sequence `Perils`
  does not occur **anywhere** in the installation — a case-sensitive sweep of
  every file under `$CS_GAME_DIR`, ASCII and UTF-16LE, returned zero hits —
  so no third string table or headline row can rescue it either.
- The two campaign-length row blocks of `GOSDATA/ASSETS/BINARIES/langui.dll`
  both carry the ninth row as a *different* spelling: short-name row 3488
  reads `Peril for Blake`; long-name row 3458 reads `Northwest - Peril for
  Paladin Blake` (tail `Peril for Paladin Blake`). Neither equals the
  declared title byte for byte. This matches the table in
  `docs/findings/2026-10-01-m05-a-source-binding.md` and task #470.
- Running production code — `SourceContext::read` + `SourceContext::bind`
  (`MissionLabel::new("M09")`, `"Perils for Blake"`) — produces a record with
  `MissionId`, `TitleString`, `ProgramSourceMap` and `WorldGroupVariant`
  **unresolved** (`Uncarried` → `NO_CONFIRMED_ROW_REFUSAL`); only
  `InstallHash` resolves. `catalog_id`, `world_id`, `program_id`,
  `campaign_position` and `localized_title_id` are all `None`; no source
  span is cited. The stage's acceptance scenario — "source-derived binding
  has no unresolved critical dependencies" — therefore cannot be met on this
  installation with the declared title.

## What the ruling would resolve to, pre-measured

If the owner rules that work order M09 is the ninth campaign position, the
directory layout gives: campaign position 8 of 24 → chapter 2, mission 4 →
`ZBD/C2B/M04` (world group `c2b`), program archive `ZBD/C2B/M04/zrdr.zbd`,
45 519 bytes, SHA-256
`1ba5b75637e72d5c569cb50b45589f4d63482a256fae66ecc750c08bb0e4145f`. The
remaining title rows for that position are 3488 (short) and 3458 (long), as
above. These are recorded here so the ruling can be applied without
re-measuring; they establish nothing until the owner names the mapping.

## Why this is blocked, not implemented

- `M05-A-GUIDE-TITLES` (#470) is the standing owner decision covering exactly
  this case: seven declared titles (M09, M11, M14, M15, M20, M22, M23) that
  the installation spells differently. Its record states that until the owner
  rules, these work orders "stay explicitly unresolved and wait on this
  decision", and that the exact comparison in `campaign_bindings.rs` should
  remain unchanged. It is still blocked.
- Treating `Perils for Blake` as `Peril for Blake` or `Peril for Paladin
  Blake` is proximity matching: a guess about which retail mission the guide
  label names (AGENTS.md rule 4). A plural-vs-singular plus inserted-word
  difference is well outside the two measured display forms.
- Binding by work-order ordinal (M09 ↔ the ninth campaign position) would be
  a *new* join rule introduced precisely where the established join fails —
  again an inference the owner reserved, not an agent choice.
- The alternative routes are owner-only: an edit of the declared titles in
  `missions/README.md` + `missions/bindings/campaign-inventory.tsv`
  (protected / inventory-synced), or a recorded per-work-order ruling in
  `docs/findings/`.

## What remains for M09-A once unblocked

1. Apply the owner's ruling inside the owner paths (a confirmed row for the
   ruled spelling, or the corrected declared title if the inventory is
   edited).
2. Generate `missions/bindings/M09.json` via `SourceBinding::to_json` and pin
   it with `accept_m09_a_the_committed_record_is_what_the_installation_derives`.
3. Write the `accept_m09_a_*` suite in `crates/cs_app/tests/campaign/`
   following the M06-A shape (m06_a.rs), including the failure arms
   (`Uncarried`/`Ambiguous` titles derive no position), the M09-specific pin
   — position 8 lives in `c2b`, the second "b" variant group — and the
   retail-only `#[ignore = "requires CS_GAME_DIR"]` markers.
4. Add the `evidence_report_m09_a_*` harness to `evidence.rs`, produce
   `private/evidence/M09-A/acceptance.json`, validate with
   `tools/validate_evidence.py --require-pass`, commit a copy as
   `docs/findings/evidence/M09-A.json`.
