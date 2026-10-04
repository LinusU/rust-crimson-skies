//! The outcome transaction (F43-A).
//!
//! `docs/contracts/STATE-TRANSACTIONS.md` fixes the shape: "Processing
//! checks eligibility and whether it was already applied, computes all
//! cash/unlock/record changes in memory, validates constraints, writes one
//! atomic profile revision, then publishes a UI acknowledgment." In this
//! stage's in-memory form, [`CampaignState::apply_outcome`] is that
//! transaction: the [`TransactionPlan`] is the computed change set, the
//! commit is the single mutation point, the [`OutcomeId`] ledger is the
//! already-applied check and `revision` is the monotonic profile revision a
//! later persistence layer (F43-B/C) writes atomically.
//!
//! The two load-bearing rules:
//!
//! * **Exactly once.** A repeated outcome — a result packet replayed after
//!   a crash, a double-submitted result screen — carries the same
//!   [`OutcomeId`], and the ledger answers `AlreadyApplied` without touching
//!   currency, unlocks, records or progression (spec F43 AC01).
//! * **Replay is not progression.** An outcome for a node the run already
//!   completed records `latest` and `best` on that node but moves `current`
//!   only when the outcome belongs to the selected node — replaying an old
//!   mission cannot overwrite the selected next mission or re-grant its
//!   first-completion rewards (spec F43 behavior 3).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_script::ir::Outcome;
use cs_types::content::ContentId;

use super::graph::{CampaignGraph, RuntimeNodeKind};
use super::identity::{CampaignNodeKey, CampaignRunId, DifficultyId, OutcomeId, ProfileId};
use super::outcome::{MissionOutcome, OutcomeAuthority};

/// Per-node completion records: per-kind tallies, the best score any
/// application recorded and the most recent result — kept separately so a
/// worse replay can record `latest` without erasing `best` (spec F43 AC02)
/// and a defeat cannot masquerade as a completion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeProgress {
    /// Applied `Succeeded` outcomes.
    pub victories: u32,
    /// Applied `Failed` outcomes.
    pub defeats: u32,
    /// Applied `Aborted` outcomes.
    pub aborts: u32,
    /// The highest score any applied victory recorded.
    pub best_score: u64,
    /// The most recent outcome applied, with its score.
    pub latest: Option<(Outcome, u64)>,
}

impl NodeProgress {
    /// Every applied outcome, of any kind.
    pub fn attempts(&self) -> u32 {
        self.victories + self.defeats + self.aborts
    }

    /// How many applied outcomes of `kind` the node has — the
    /// first-of-its-kind test the grant rule uses.
    fn of_kind(&self, kind: Outcome) -> u32 {
        match kind {
            Outcome::Succeeded => self.victories,
            Outcome::Failed => self.defeats,
            Outcome::Aborted => self.aborts,
        }
    }
}

/// The change set [`CampaignState::apply_outcome`] computed and committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionPlan {
    /// Currency moved, in minor units (grant minus nothing at this stage —
    /// purchases are F43-B's separate draft transaction).
    pub currency_delta: i64,
    /// Content ids this application unlocked.
    pub unlocks: Vec<ContentId>,
    /// Where `current` moved, when the outcome progressed the run.
    pub progress_to: Option<CampaignNodeKey>,
    /// Whether this application raised the node's best score.
    pub best_score_raised: bool,
    /// Whether this application marked the progression modified.
    pub marked_modified: bool,
}

/// What [`CampaignState::apply_outcome`] answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutcomeReceipt {
    /// The outcome committed; the plan names every change.
    Applied(TransactionPlan),
    /// This exact [`OutcomeId`] was already applied; nothing changed. The
    /// revision the original application produced is reported for the
    /// caller's acknowledgment.
    AlreadyApplied,
}

/// Why an outcome was refused — the state is untouched in every case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CampaignError {
    /// The outcome's `(profile, run)` names a different state — a packet for
    /// someone else's save, refused rather than applied.
    ForeignOutcome {
        /// The outcome's id.
        outcome: OutcomeId,
    },
    /// The outcome names a node the run has neither selected nor previously
    /// visited — a future mission cannot report results.
    IneligibleNode {
        /// The node the outcome names.
        node: CampaignNodeKey,
        /// The currently selected node.
        current: CampaignNodeKey,
    },
    /// The outcome names a node that is not in the graph.
    UnknownNode {
        /// The undeclared node.
        node: CampaignNodeKey,
    },
    /// The outcome names a node that is not a mission — interludes and
    /// endings produce no outcomes.
    NotAMission {
        /// The node.
        node: CampaignNodeKey,
    },
    /// The grant would overflow the currency counter.
    CurrencyOverflow {
        /// The currency before.
        before: u64,
        /// The attempted grant.
        delta: u64,
    },
    /// The selected node is an interlude with no `Victory` edge, so it has no
    /// way onward — the declared graph cannot be walked past it.
    ///
    /// A **refusal**, never a silent stop: a beat the runtime cannot leave is a
    /// dead end in the *declared* campaign, and reporting it is what lets F43-D
    /// tell a bad import from a rule this stage has not implemented.
    InterludeDeadEnd {
        /// The interlude with no onward victory edge.
        node: CampaignNodeKey,
    },
    /// The draft was written against an older profile revision than the state
    /// holds (contract: "Conflicting revisions fail and refresh the view").
    ///
    /// The state is **untouched**: a stale draft must never overwrite unrelated
    /// progression, which is exactly what an unguarded write would do.
    StaleRevision {
        /// The revision the draft expected.
        expected: u64,
        /// The revision the state actually holds.
        actual: u64,
    },
    /// The item is not open in this run — no roster gate the run has completed
    /// declares it available.
    ItemUnavailable {
        /// The requested item.
        item: ContentId,
    },
    /// The profile already owns the item, so buying it again would charge twice.
    AlreadyOwned {
        /// The item already owned.
        item: ContentId,
    },
    /// The balance cannot cover the price.
    InsufficientFunds {
        /// The price in minor units.
        price: u64,
        /// The balance in minor units.
        balance: u64,
    },
}

impl fmt::Display for CampaignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignOutcome { outcome } => write!(
                f,
                "outcome {}.{} does not belong to this profile/run",
                outcome.profile, outcome.run
            ),
            Self::IneligibleNode { node, current } => write!(
                f,
                "node {node} is neither selected ({current}) nor previously visited"
            ),
            Self::UnknownNode { node } => {
                write!(f, "node {node} is not in the campaign graph")
            }
            Self::NotAMission { node } => {
                write!(f, "node {node} is not a mission and reports no outcome")
            }
            Self::CurrencyOverflow { before, delta } => {
                write!(f, "grant of {delta} would overflow the balance {before}")
            }
            Self::InterludeDeadEnd { node } => write!(
                f,
                "interlude {node} has no victory edge, so the declared campaign \
                 cannot be walked past it"
            ),
            Self::StaleRevision { expected, actual } => write!(
                f,
                "the draft was written against revision {expected} but the profile \
                 is at {actual}: refresh the view and retry"
            ),
            Self::ItemUnavailable { item } => {
                write!(f, "item {item} is not available in this run")
            }
            Self::AlreadyOwned { item } => {
                write!(
                    f,
                    "item {item} is already owned; buying it again would charge twice"
                )
            }
            Self::InsufficientFunds { price, balance } => write!(
                f,
                "price {price} exceeds the balance {balance} in minor units"
            ),
        }
    }
}

impl std::error::Error for CampaignError {}

/// What one applied outcome reported back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedOutcome {
    /// The receipt — applied plan or already-applied acknowledgment.
    pub receipt: OutcomeReceipt,
    /// The profile revision after this call (unchanged when deduped).
    pub revision: u64,
}

/// One run's progression state: the contract's `CampaignState` —
/// progression, owned resources (currency + unlocks), best/latest records,
/// difficulty and profile identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignState {
    profile: ProfileId,
    run: CampaignRunId,
    difficulty: DifficultyId,
    current: CampaignNodeKey,
    progress: BTreeMap<CampaignNodeKey, NodeProgress>,
    currency: u64,
    unlocks: BTreeSet<ContentId>,
    applied: BTreeSet<OutcomeId>,
    revision: u64,
    modified: bool,
}

impl CampaignState {
    /// Starts a run at the graph's entry node.
    pub fn begin(
        profile: ProfileId,
        run: CampaignRunId,
        difficulty: DifficultyId,
        graph: &CampaignGraph,
    ) -> Self {
        Self {
            profile,
            run,
            difficulty,
            current: graph.entry().clone(),
            progress: BTreeMap::new(),
            currency: 0,
            unlocks: BTreeSet::new(),
            applied: BTreeSet::new(),
            revision: 0,
            modified: false,
        }
    }

    /// The profile this state belongs to.
    pub fn profile(&self) -> &ProfileId {
        &self.profile
    }

    /// The run inside the profile.
    pub fn run(&self) -> &CampaignRunId {
        &self.run
    }

    /// The difficulty the run was started under.
    pub fn difficulty(&self) -> &DifficultyId {
        &self.difficulty
    }

    /// The selected node — "the selected next mission" resume restores.
    pub fn current(&self) -> &CampaignNodeKey {
        &self.current
    }

    /// The completion record of a node, when it has one.
    pub fn progress(&self, node: &CampaignNodeKey) -> Option<&NodeProgress> {
        self.progress.get(node)
    }

    /// Whether the run has *won* `node` at least once — the roster gate
    /// condition. Defeats and aborts record progress but do not complete.
    pub fn has_completed(&self, node: &CampaignNodeKey) -> bool {
        self.progress.get(node).is_some_and(|p| p.victories > 0)
    }

    /// Cash, in minor units.
    pub fn currency(&self) -> u64 {
        self.currency
    }

    /// Granted unlocks.
    pub fn unlocks(&self) -> impl Iterator<Item = &ContentId> {
        self.unlocks.iter()
    }

    /// The monotonic profile revision — the optimistic-concurrency counter
    /// the contract's persistence transaction names.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a modified-authority outcome has touched this progression —
    /// the explicit mark spec F43 non-negotiable behavior 1 requires when a
    /// non-clean run shares a profile.
    pub fn modified(&self) -> bool {
        self.modified
    }

    /// Whether the selected node is an ending — the campaign is over.
    pub fn is_finished(&self, graph: &CampaignGraph) -> bool {
        graph
            .node(&self.current)
            .is_some_and(|node| node.kind == RuntimeNodeKind::Ending)
    }

    /// The outcome transaction: apply `outcome` exactly once.
    ///
    /// Order of checks (the contract's): foreign `(profile, run)` → already
    /// applied → known node → mission node → eligible → compute plan →
    /// validate (currency overflow) → commit → bump revision. A refusal
    /// changes nothing.
    ///
    /// An edge's grant fires only on the node's first outcome *of that
    /// kind* — a replayed victory cannot re-pay its reward (which is what
    /// makes a post-restart replay safe even under a fresh session
    /// generation), and a defeat cannot burn the victory grant before the
    /// first success earns it.
    ///
    /// # Errors
    ///
    /// [`CampaignError`].
    pub fn apply_outcome(
        &mut self,
        graph: &CampaignGraph,
        outcome: &MissionOutcome,
    ) -> Result<AppliedOutcome, CampaignError> {
        if outcome.id.profile != self.profile || outcome.id.run != self.run {
            return Err(CampaignError::ForeignOutcome {
                outcome: outcome.id.clone(),
            });
        }
        if self.applied.contains(&outcome.id) {
            return Ok(AppliedOutcome {
                receipt: OutcomeReceipt::AlreadyApplied,
                revision: self.revision,
            });
        }
        let node_key = outcome.node.clone();
        let Some(node) = graph.node(&node_key) else {
            return Err(CampaignError::UnknownNode { node: node_key });
        };
        if !matches!(node.kind, RuntimeNodeKind::Mission { .. }) {
            return Err(CampaignError::NotAMission { node: node_key });
        }
        let visited_before = self.progress.contains_key(&node_key);
        let progresses = node_key == self.current;
        if !progresses && !visited_before {
            return Err(CampaignError::IneligibleNode {
                node: node_key,
                current: self.current.clone(),
            });
        }

        // Compute the plan in memory, then commit.
        let mut plan = TransactionPlan {
            currency_delta: 0,
            unlocks: Vec::new(),
            progress_to: None,
            best_score_raised: false,
            marked_modified: matches!(outcome.authority, OutcomeAuthority::Modified { .. }),
        };
        let prior = self.progress.get(&node_key);
        let prior_of_kind = prior.map_or(0, |p| p.of_kind(outcome.outcome));
        let prior_best = prior.map_or(0, |p| p.best_score);
        if outcome.outcome == Outcome::Succeeded && outcome.score > prior_best {
            plan.best_score_raised = true;
        }
        if progresses && let Some(edge) = graph.transition(&node_key, outcome.outcome) {
            plan.progress_to = Some(edge.to.clone());
            // First outcome of this kind only: a replayed node cannot
            // re-pay, and a defeat cannot burn the victory grant.
            if prior_of_kind == 0 {
                plan.currency_delta = edge.grant.currency as i64;
                plan.unlocks = edge.grant.unlocks.clone();
            }
        }
        let new_currency = self
            .currency
            .checked_add(plan.currency_delta as u64)
            .ok_or(CampaignError::CurrencyOverflow {
                before: self.currency,
                delta: plan.currency_delta as u64,
            })?;

        // Commit — the single mutation point.
        let record = self.progress.entry(node_key.clone()).or_default();
        match outcome.outcome {
            Outcome::Succeeded => {
                record.victories += 1;
                record.best_score = record.best_score.max(outcome.score);
            }
            Outcome::Failed => record.defeats += 1,
            Outcome::Aborted => record.aborts += 1,
        }
        record.latest = Some((outcome.outcome, outcome.score));
        if let Some(to) = &plan.progress_to {
            self.current = to.clone();
        }
        self.currency = new_currency;
        self.unlocks.extend(plan.unlocks.iter().cloned());
        self.applied.insert(outcome.id.clone());
        self.modified |= plan.marked_modified;
        self.revision += 1;

        Ok(AppliedOutcome {
            receipt: OutcomeReceipt::Applied(plan),
            revision: self.revision,
        })
    }

    /// Walks the selected node forward across declared **interludes** and stops
    /// on the first node that can report an outcome of its own (spec F43 stage B,
    /// "progression transactions").
    ///
    /// An interlude has no outcome: the vocabulary carries it
    /// ([`RuntimeNodeKind::Interlude`]) and the declared schema declares its
    /// edges, but no mission ever produces one. F43-A therefore left `current`
    /// parked on an interlude with nothing able to move it — a dead end in any
    /// campaign whose path crosses a briefing or a cutscene. This is the one
    /// production path that crosses it, and it is deliberately **not** folded
    /// into [`Self::apply_outcome`]: an interlude edge can carry its own grant,
    /// and paying it inside the mission's transaction would merge two revisions
    /// into one and hide the beat from the run's own record.
    ///
    /// The walk is idempotent by construction: it moves `current` off each beat,
    /// so a second call starts on a node that is not an interlude, traverses
    /// nothing, pays nothing and does **not** bump the revision (spec F43
    /// behavior 2 — rewards and unlocks are idempotent).
    ///
    /// # Errors
    ///
    /// [`CampaignError::InterludeDeadEnd`] when the selected interlude declares no
    /// `Victory` edge, and [`CampaignError::CurrencyOverflow`] when a beat's grant
    /// would overflow the balance. Either way the state is untouched.
    pub fn advance_interludes(
        &mut self,
        graph: &CampaignGraph,
    ) -> Result<InterludeAdvance, CampaignError> {
        let mut traversed: Vec<CampaignNodeKey> = Vec::new();
        let mut currency_delta: i64 = 0;
        let mut unlocks: Vec<ContentId> = Vec::new();
        // Compute the whole walk before touching anything: a beat with no onward
        // edge must leave the run exactly where it was, not half-way down a chain
        // it cannot finish.
        let mut landed: Option<CampaignNodeKey> = None;
        let mut cursor = self.current.clone();
        while graph
            .node(&cursor)
            .is_some_and(|node| node.kind == RuntimeNodeKind::Interlude)
        {
            let Some(edge) = graph.transition(&cursor, Outcome::Succeeded) else {
                return Err(CampaignError::InterludeDeadEnd { node: cursor });
            };
            traversed.push(cursor.clone());
            currency_delta += edge.grant.currency as i64;
            unlocks.extend(edge.grant.unlocks.iter().cloned());
            cursor = edge.to.clone();
            landed = Some(cursor.clone());
        }
        if traversed.is_empty() {
            return Ok(InterludeAdvance::default());
        }
        let new_currency = self.currency.checked_add(currency_delta as u64).ok_or(
            CampaignError::CurrencyOverflow {
                before: self.currency,
                delta: currency_delta as u64,
            },
        )?;

        // Commit — one revision for the whole chain.
        if let Some(to) = landed {
            self.current = to;
        }
        self.currency = new_currency;
        self.unlocks.extend(unlocks.iter().cloned());
        self.revision += 1;
        Ok(InterludeAdvance {
            traversed,
            currency_delta,
            unlocks,
        })
    }

    /// Buys one item for the run: the contract's purchase draft
    /// ("Validate current availability, money and weight before writing.
    /// Conflicting revisions fail and refresh the view; they do not overwrite
    /// unrelated progression.").
    ///
    /// Order of checks — availability, then ownership, then money, then the
    /// expected revision last, so a stale draft is *always* reported as stale and
    /// never as "you cannot afford it": the view the caller refreshes on a
    /// conflict must describe the same reason the conflict had. Every refusal
    /// leaves the state bit-identical.
    ///
    /// Idempotent by the same argument as the interlude walk: the item joins
    /// [`Self::unlocks`], so a repeated draft is refused with
    /// [`CampaignError::AlreadyOwned`] instead of charging twice (spec F43
    /// behavior 2).
    ///
    /// # Errors
    ///
    /// [`CampaignError`].
    pub fn purchase(
        &mut self,
        graph: &CampaignGraph,
        draft: &PurchaseDraft,
    ) -> Result<PurchaseReceipt, CampaignError> {
        if !graph.available_items(self).any(|item| *item == draft.item) {
            return Err(CampaignError::ItemUnavailable {
                item: draft.item.clone(),
            });
        }
        if self.unlocks.contains(&draft.item) {
            return Err(CampaignError::AlreadyOwned {
                item: draft.item.clone(),
            });
        }
        if self.currency < draft.price {
            return Err(CampaignError::InsufficientFunds {
                price: draft.price,
                balance: self.currency,
            });
        }
        if draft.expected_revision != self.revision {
            return Err(CampaignError::StaleRevision {
                expected: draft.expected_revision,
                actual: self.revision,
            });
        }

        // Commit — the single mutation point.
        self.currency -= draft.price;
        self.unlocks.insert(draft.item.clone());
        self.revision += 1;
        Ok(PurchaseReceipt {
            item: draft.item.clone(),
            paid: draft.price,
            revision: self.revision,
        })
    }
}

/// What one interlude walk traversed and granted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterludeAdvance {
    /// The beats walked, in order. Empty when the selected node was not an
    /// interlude — which is also the case that leaves the revision untouched.
    pub traversed: Vec<CampaignNodeKey>,
    /// Currency the beats granted, in minor units.
    pub currency_delta: i64,
    /// Content the beats unlocked.
    pub unlocks: Vec<ContentId>,
}

impl InterludeAdvance {
    /// Whether the walk moved the run at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.traversed.is_empty()
    }
}

/// A request to buy one item, written against the revision the view was built
/// from.
///
/// The draft is the contract's optimistic-concurrency token: it names the
/// revision the caller believed it was buying against, so a save written by
/// anything else in between is detected instead of silently overwritten.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PurchaseDraft {
    /// The item to buy.
    pub item: ContentId,
    /// The price in minor units.
    pub price: u64,
    /// The profile revision the view was built from.
    pub expected_revision: u64,
}

/// What one accepted purchase did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PurchaseReceipt {
    /// The item bought.
    pub item: ContentId,
    /// What was charged, in minor units.
    pub paid: u64,
    /// The profile revision after the purchase.
    pub revision: u64,
}
