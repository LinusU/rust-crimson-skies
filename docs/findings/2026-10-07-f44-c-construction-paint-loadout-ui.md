# F44-C: Construction, paint and loadout UI with preview

Date: 2026-10-07. Task: F44-C (`specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
`### F44-C`). Contract: `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities:
ordinary build/test only.

## Files and the one observable failure

- `crates/cs_app/src/construction/screen.rs` (new): `ConstructionScreen` — the
  UI object that owns a `ConstructionSession` — plus `ConstructionView`,
  `QuantityMeter`, `PendingTransaction`, `ImportRejection`, `ScreenSaveError`
  and the `save_committed` persist.
- `crates/cs_app/src/construction/mod.rs`: `ConstructionSession::commit_draft`
  (validate + price + stage, split out of `commit`) and `verdict_for` (the same
  policy applied to a blueprint that is not the draft — the import path);
  `pub mod screen` and re-exports.
- `crates/cs_content/src/construction.rs`: `AircraftBlueprint::with_engine`,
  `with_equipment`, `with_paint`; the equipment checks in `try_new` extracted
  to `check_equipment`.
- `crates/cs_sim/src/economy.rs`: `ConstructionDraft::buys`/`sells`/`weight`
  accessors so the screen's pending-transaction preview reads the real staged
  draft.
- `crates/cs_app/tests/accept_f44_c_construction_screen.rs`: six tests.

Observable failure before this change: no producer or consumer used
`ConstructionSession` — an imported blueprint had no path through the shared
validator at all, and a committed session changed only an in-memory
`CampaignState`, never the save. `accept_f44_c_an_imported_blueprint_cannot_bypass_pairing_or_banned_rules`
drives `ConstructionScreen::import` and
`accept_f44_c_the_screen_previews_then_commits_and_saves_one_revision` re-reads
the profile document after `commit_saved`.

## Decisions

- The screen is the producer, `CampaignState`/`ProfileSession` the consumers.
  `view` projects the draft, the live verdict and the staged draft
  (`commit_draft` is the same builder `commit` uses, so the displayed buy list
  is the list that would be charged). The meters are the validator's own
  integer totals normalized to each profile limit; nothing is recomputed.
- `import` judges the foreign record with `verdict_for` — one policy, shared
  with `verdict` and `commit` — and adopts only a valid verdict. A refused or
  invalid import leaves the draft untouched and the rejection on `notice`.
- `commit_saved` = commit + `save_committed`, which restates
  `CampaignRun::persist`'s shape (`read_snapshot` guard inside `commit_with`,
  `write_snapshot`, adopt only after the write) because `campaign.rs` is
  outside this task's owner paths and `CampaignRun::transact` is private.
- Edits include per-slot setters; `set_paint` carries `PaintSelection`
  references only, and `export` returns the draft record itself, so import and
  export can never move texture data (non-negotiable 5).
- Retry/teardown: a refused commit or save leaves draft, state and save
  unchanged and the refusal on `notice`; a successful commit re-bases the
  session on the moved state so the screen stays open and consistent; `cancel`
  consumes the screen having staged nothing.

## Recorded unknowns (not guessed)

- The normalized *performance* preview of behavior 4 needs the game weight
  unit's conversion to SI and the spawned-aircraft equality harness (AC04) —
  both unmeasured and both F44-D. What the view shows is the real budget
  verdict and the staged transaction, not invented flight bars.
- Where blueprint *records* are stored for a profile (a hangar list) is not yet
  declared anywhere; `open` takes the record from the caller.
- Whether the original construction screen lets an invalid draft be edited in
  is unmeasured; here `edit` accepts any well-formed record and the verdict —
  plus the commit's refusal — is what blocks it, matching "the UI blocks
  illegal builds" through the validator rather than through input filtering.

## Follow-up (not this task)

- When a task owns `crates/cs_app/src/campaign.rs`, `CampaignRun` should grow a
  method that runs the construction commit inside `transact` so
  `commit_saved`'s restated persist has one canonical home.
