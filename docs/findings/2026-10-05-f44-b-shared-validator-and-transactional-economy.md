# F44-B: Shared validator and transactional economy

Date: 2026-10-05. Task: F44-B (`specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
`### F44-B`). Contract: `docs/contracts/STATE-TRANSACTIONS.md`, "Outcome and
economy transaction". Capabilities: ordinary build/test only.

## Files and the one observable failure

- `crates/cs_content/src/construction.rs`: `ConstructionPolicy`,
  `ConstraintViolation`, `ValidationRefusal`, `BlueprintVerdict`,
  `AircraftBlueprint::components`, `ConstructionRules::validate`,
  `synthetic_policy`.
- `crates/cs_sim/src/economy.rs` (new): `ConstructionDraft`, `commit`,
  `EconomyError`, `CommitReceipt`.
- `crates/cs_app/src/construction/mod.rs` (new): `ConstructionSession`
  (edit / verdict / cancel / commit) joining the two.
- `crates/cs_app/tests/accept_f44_b_transactional_economy.rs`: seven tests.
- Wiring only: `pub mod economy;` in `cs_sim/src/lib.rs`, `pub mod construction;`
  in `cs_app/src/lib.rs`.

Observable failure: an editor that applied purchases while editing would change
`CampaignState::snapshot()` before commit; the AC02 test compares the snapshot
before and after an edited-then-cancelled session and also commits the same edit
to show the comparison is not vacuous.

## Decisions

- One validator (`ConstructionRules::validate`) = F44-A budget + paired-gun,
  banned-component and availability constraints. Imports and hand edits call the
  same function (AC03).
- A gun selection of 1 position is single, 2 is a pair (needs the pairing
  rule), anything else is `UnsupportedGunSelection`. Symmetry is never checked.
- `commit` builds the complete new snapshot and installs it through
  `CampaignState::restore`, so a multi-item draft bumps the revision exactly
  once and a refusal leaves the state bit-identical. An empty draft on an old
  revision is reported stale.
- A sale is refused while the edited blueprint or any other active blueprint
  references the component.

## Recorded unknowns (not guessed)

- Which guns the original lets a player mate: the pairing rule is
  `Resolved`; unmeasured refuses a paired selection (F44-D).
- Whether the original has starting stock outside roster gates. Here a
  component is buyable only through a roster gate or already owned.
- Original warnings: none are produced because no warning rule is known.
- Normalized performance preview (F44 behavior 4) and spawn equality (AC04) are
  F44-C/F44-D.

## Follow-up for F44-C

`CampaignRun` (`cs_app/src/campaign.rs`, outside this task's paths) saves through
its private `transact`. `ConstructionSession::commit` commits to a
`CampaignState`; F44-C must persist it, ideally via a `CampaignRun` method that
runs `economy::commit` inside `transact` so the save is one revision.
