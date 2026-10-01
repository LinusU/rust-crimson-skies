# F43-A: campaign graph and the transactional outcome schema — declared contract

**Task:** #170, branch `rally/170-define-campaign-graph-and-transactional`,
prefix `accept_f43_a_`. Stage A of
`specs/F43-campaign-progression-outcomes-and-economy-rules.md`; shared
contract `docs/contracts/STATE-TRANSACTIONS.md`.

## What was built

* `crates/cs_content/src/campaign.rs` — the declared half:
  `CampaignNodeId`, `NodeKind` (`Mission{mission: Resolved<ContentId>}`,
  `Interlude`, `Ending`), `EdgeCondition` (`Victory`/`Defeat`/`Abort` — the
  designed projection of the mission runtime's `Outcome`),
  `RewardSpec` (`Resolved<u64>` minor-unit currency + `ContentId` unlocks),
  `CampaignEdge` (per-edge provenance — a designed edge and a measured one
  are never indistinguishable), `RosterEntry` (availability gated on a
  node's first victory), and `CampaignDefinition::try_new` enforcing unique
  ids, declared entry, no dangling edges, ≤1 edge per condition, no edges
  out of endings, no non-ending dead ends, roster gates on declared nodes.
  `declared_synthetic_campaign()` is the minimal fixture: `m01 → m02 →
  ending`, victory-forward, defeat-in-place (a declared retry loop, not an
  implied rule), one roster gate.
* `crates/cs_sim/src/campaign/` — the runtime half:
  - `identity.rs`: `ProfileId`, `CampaignRunId`, `DifficultyId`,
    `CampaignNodeKey` and `OutcomeId = (profile, run, SessionGeneration,
    EventKey)` — the contract's persisted tuple; session-qualified so a
    restarted mission can never collide with the attempt it replaced.
  - `graph.rs`: `CampaignGraph`, the sim's own re-validated node/edge
    vocabulary (cs_sim cannot see `cs_content`) keyed on
    `cs_script::Outcome`.
  - `outcome.rs`: `MissionOutcome`, the immutable transaction input, and
    `OutcomeAuthority::{Authorized, Modified}` — a modified/debug run
    applies but permanently marks the progression it touched (spec F43
    behavior 1).
  - `state.rs`: `CampaignState` (current node, per-node
    `NodeProgress{victories,defeats,aborts,best_score,latest}`, minor-unit
    currency, unlocks, `OutcomeId` dedup ledger, monotonic `revision`,
    `modified` flag) and `apply_outcome` — the contract's transaction:
    foreign → already-applied → eligible → mission-node → compute
    `TransactionPlan` → validate → single commit → bump revision.
* `crates/cs_app/src/campaign.rs` — `lower_campaign`: declared → runtime.
  `UnknownMissionBinding` and `UnknownReward` refuse here rather than pay
  or route on a guess; `Interlude` assets are optional and an unbound one
  lowers to a silent beat.
* `crates/cs_app/tests/accept_f43_a_campaign.rs` — 10 acceptance tests plus
  2 content unit tests; 12 `accept_f43_a_` tests total.

## Decisions that needed a rule the sheet does not fix

* **Grant idempotence vs. replay.** The `OutcomeId` ledger dedups the
  *same* packet; a *replayed* mission is a legitimately different outcome
  id. The declared rule: an edge's grant fires on the node's **first
  outcome of that kind** — a second victory cannot re-pay, and a defeat
  cannot burn the victory grant before the first success earns it. This is
  designed semantics (documented, provenance `Designed`); whether the
  original campaign re-pays on replay is unmeasured and listed below.
* **Eligibility.** An outcome applies iff its node is the currently
  selected node (progression) or already has progress records (replay).
  A future node refuses with `IneligibleNode`; a foreign `(profile, run)`
  refuses with `ForeignOutcome`.
* **"Completed" means won.** `has_completed` counts victories — defeats
  and aborts record progress but do not open roster gates.
* **Currency** is `u64` minor units (contract: "integer minor game units");
  grants are `checked_add`, overflow is a named refusal. Purchases/sell
  drafts and the expected-revision check are F43-B's separate transaction
  family and are **not** in this slice.

## The minimum scenario

`accept_f43_a_the_same_outcome_applies_once`: apply a victory outcome →
+500 minor units, `m01 → m02`, one unlock, revision 1. Apply the identical
`OutcomeId` again → `AlreadyApplied`, and currency, progression, records
and revision are bit-identical.

## Recorded unknowns for F43-B..D (do not guess)

- **Whether replay re-pays in the original** — our first-of-kind grant
  rule is designed; F43-D's retail playthrough must measure it.
- **Defeat/retry/skip rules** (spec behavior 4) — the fixture declares a
  self-loop; original edge conditions are unmeasured. There is no hidden
  three-failure rule: no declared edge means no transition.
- **The original campaign's actual graph** — `campaign_bindings` (F50-A)
  records the observed `ZBD/<chapter><variant>/<mission>` layout; turning
  it into a `CampaignDefinition` is an importer task, not stage A.
- **Difficulty levels** — `DifficultyId` is an opaque selector; the
  original's levels and their effects are unmeasured.
- **Abort routing** — the vocabulary carries `Abort` edges; whether the
  original distinguishes quit from defeat is unmeasured.
- **Economy display mapping** — minor units to rendered dollars/points is
  a UI concern; unmeasured.
- **Persistence** — AC03's crash-mid-save recovery needs F48's atomic
  revision writes; `revision` here is the counter that layer persists.

## Checks run

- `cargo test --workspace --locked -- accept_f43_a_ --include-ignored`:
  12 tests pass (10 in `cs_app/tests/accept_f43_a_campaign.rs`, 2 in
  `cs_content::campaign` unit tests).
- Workspace fmt/clippy/test results are in the handover summary.
