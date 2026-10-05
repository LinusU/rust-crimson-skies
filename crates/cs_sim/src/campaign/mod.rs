//! The runtime half of the campaign contract (F43-A): outcome identity,
//! the validated campaign graph and the exactly-once outcome transaction.
//!
//! Spec: `specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
//! stage `### F43-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! * [`identity`] — the persisted identities the contract's outcome tuple
//!   names: [`ProfileId`], [`CampaignRunId`], [`DifficultyId`] and
//!   [`OutcomeId`] = `(profile, run, session, terminal event)`, the key the
//!   dedup ledger refuses to apply twice.
//! * [`graph`] — the runtime [`CampaignGraph`]: `cs_sim` cannot depend on
//!   `cs_content`, so this is the sim's own validated node/edge vocabulary
//!   the `cs_app` lowering fills. Outcomes are `cs_script::Outcome`s —
//!   the same terminal vocabulary the mission runtime emits.
//! * [`outcome`] — [`MissionOutcome`], the single immutable transaction
//!   input, and [`OutcomeAuthority`]: whether the evidence that produced it
//!   was an authorized playthrough or a modified/synthetic run (spec F43
//!   non-negotiable behavior 1 — modified runs mark progression, they do
//!   not silently mix).
//! * [`state`] — [`CampaignState`]: current node, per-node completion
//!   records (best *and* latest — replay never overwrites progression,
//!   spec F43 behavior 3), currency in minor units, unlocks, the
//!   [`OutcomeId`] dedup ledger and the monotonic profile revision.
//!   [`CampaignState::apply_outcome`] is the contract's outcome
//!   transaction: check eligibility and prior application, compute the
//!   plan in memory, commit it atomically, return the receipt.
//!   [`CampaignState::advance_interludes`] (F43-B) is the second
//!   progression transaction — the one that walks a selected narrative
//!   beat forward, which no mission outcome can do — and
//!   [`CampaignState::purchase`] (F43-B) is the economy draft, validated
//!   against the expected profile revision before it writes;
//!   [`CampaignState::sell`] (F43-B.1) is its mirror, refunding the price paid.
//!
//! The declared schema (`cs_content::campaign`) and the lowering boundary
//! (`cs_app::campaign`) hold the provenance-carrying records; nothing here
//! guesses original campaign rules — every edge is whichever transition the
//! record declared.

mod graph;
mod identity;
mod outcome;
mod state;

// The terminal vocabulary and the session/event identities are the mission
// runtime's own types — the campaign contract re-exports them so consumers
// name one identity space, not two spellings of it.
pub use cs_script::ir::{Outcome, SymbolId};
pub use cs_script::runtime::{EventKey, SessionGeneration};
pub use graph::{
    CampaignGraph, GraphError, Reward, RosterGate, RuntimeEdge, RuntimeNode, RuntimeNodeKind,
};
pub use identity::{CampaignNodeKey, CampaignRunId, DifficultyId, OutcomeId, ProfileId};
pub use outcome::{MissionOutcome, OutcomeAuthority};
pub use state::{
    AppliedOutcome, CampaignError, CampaignSnapshot, CampaignState, InterludeAdvance,
    LoadoutWeight, NodeProgress, OutcomeReceipt, PurchaseDraft, PurchaseReceipt, SellDraft,
    SellReceipt, TransactionPlan,
};
