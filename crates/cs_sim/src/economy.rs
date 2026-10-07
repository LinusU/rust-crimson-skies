//! The construction economy transaction (F44-B).
//!
//! Spec `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-B`, non-negotiable 3; contract `STATE-TRANSACTIONS`,
//! "Outcome and economy transaction".
//!
//! A [`ConstructionDraft`] is plain data: the purchases and sales an edit would
//! make, the components that must stay owned because an active blueprint still
//! references them, the weight verdict the boundary resolved and the profile
//! revision the view was built from. **Nothing is written while editing**:
//! dropping the draft (cancel) is the whole of cancellation, and
//! [`commit`] is the only function that mutates a [`CampaignState`].
//!
//! `commit` computes the complete result in memory, validates every
//! constraint, and then replaces the state through [`CampaignState::restore`]
//! with exactly **one** revision bump for the whole draft — a refusal at any
//! step leaves the state bit-identical, and a multi-item draft is never half
//! applied. `cs_sim` cannot depend on `cs_content`, so prices and weights
//! arrive as integers resolved by `cs_app::construction`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::ContentId;

use crate::campaign::{CampaignError, CampaignGraph, CampaignState, LoadoutWeight};

/// One item the draft buys and what it costs, in minor units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuyLine {
    /// The item.
    pub item: ContentId,
    /// The price in minor units.
    pub price: u64,
}

/// A staged edit of the profile's inventory and currency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionDraft {
    expected_revision: u64,
    weight: LoadoutWeight,
    buys: Vec<BuyLine>,
    sells: Vec<ContentId>,
    referenced: BTreeSet<ContentId>,
}

impl ConstructionDraft {
    /// An empty draft written against `expected_revision`, whose resulting
    /// loadout has the resolved `weight` verdict.
    #[must_use]
    pub const fn new(expected_revision: u64, weight: LoadoutWeight) -> Self {
        Self {
            expected_revision,
            weight,
            buys: Vec::new(),
            sells: Vec::new(),
            referenced: BTreeSet::new(),
        }
    }

    /// Stages the purchase of `item`.
    #[must_use]
    pub fn buy(mut self, item: ContentId, price: u64) -> Self {
        self.buys.push(BuyLine { item, price });
        self
    }

    /// Stages the sale of `item`.
    #[must_use]
    pub fn sell(mut self, item: ContentId) -> Self {
        self.sells.push(item);
        self
    }

    /// Declares components that active blueprints still reference after the
    /// commit, so selling one is refused.
    #[must_use]
    pub fn keep_referenced(mut self, items: impl IntoIterator<Item = ContentId>) -> Self {
        self.referenced.extend(items);
        self
    }

    /// Abandons the draft. It never touched the state, so this changes nothing;
    /// it exists so a caller's cancel path is an explicit call.
    pub fn cancel(self) {}

    /// The revision the draft was written against.
    #[must_use]
    pub const fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    /// The weight verdict the draft was written with.
    #[must_use]
    pub const fn weight(&self) -> &LoadoutWeight {
        &self.weight
    }

    /// The staged purchases, in the order they were declared.
    ///
    /// [`commit`] is still the only place they are applied; reading them is how
    /// the construction screen's preview shows exactly what a commit would
    /// charge before it is attempted.
    #[must_use]
    pub fn buys(&self) -> &[BuyLine] {
        &self.buys
    }

    /// The staged sales, in the order they were declared.
    #[must_use]
    pub fn sells(&self) -> &[ContentId] {
        &self.sells
    }
}

/// Why a draft was refused. The state is untouched in every case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EconomyError {
    /// The draft stages nothing.
    EmptyDraft,
    /// An item appears twice among the draft's lines, or is both bought and
    /// sold.
    ConflictingLines {
        /// The repeated item.
        item: ContentId,
    },
    /// Selling the item would leave an active blueprint referencing a
    /// component the profile no longer owns.
    ActiveReference {
        /// The item.
        item: ContentId,
    },
    /// The campaign transaction rules refused the draft.
    Campaign(CampaignError),
}

impl fmt::Display for EconomyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDraft => f.write_str("the draft changes nothing"),
            Self::ConflictingLines { item } => {
                write!(f, "{item} is staged more than once")
            }
            Self::ActiveReference { item } => {
                write!(f, "{item} is still used by an active blueprint")
            }
            Self::Campaign(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EconomyError {}

impl From<CampaignError> for EconomyError {
    fn from(error: CampaignError) -> Self {
        Self::Campaign(error)
    }
}

/// What a committed draft did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitReceipt {
    /// Total charged, in minor units.
    pub charged: u64,
    /// Total refunded, in minor units.
    pub refunded: u64,
    /// The profile revision after the commit.
    pub revision: u64,
}

/// Applies `draft` to `state` as one revision.
///
/// Order of checks: non-empty (an empty draft on an old revision is stale) and no repeated item; every sale is owned,
/// bought (a refund is the price actually paid) and not still referenced;
/// every purchase is available and not owned; the balance after refunds covers
/// the charges; the loadout weight is measured and within its limit; the
/// expected revision **last**, as in [`CampaignState::purchase`].
///
/// # Errors
///
/// [`EconomyError`]; on any error `state` is unchanged.
pub fn commit(
    state: &mut CampaignState,
    graph: &CampaignGraph,
    draft: &ConstructionDraft,
) -> Result<CommitReceipt, EconomyError> {
    if draft.buys.is_empty() && draft.sells.is_empty() {
        // A draft written against an older revision may be empty only because
        // the profile moved on (its purchases were already made), so that is
        // reported as the conflict it is.
        return Err(if draft.expected_revision == state.revision() {
            EconomyError::EmptyDraft
        } else {
            CampaignError::StaleRevision {
                expected: draft.expected_revision,
                actual: state.revision(),
            }
            .into()
        });
    }
    let mut seen = BTreeSet::new();
    for item in draft
        .buys
        .iter()
        .map(|line| &line.item)
        .chain(draft.sells.iter())
    {
        if !seen.insert(item) {
            return Err(EconomyError::ConflictingLines { item: item.clone() });
        }
    }

    let mut snapshot = state.snapshot();
    let mut owned: BTreeSet<ContentId> = snapshot.unlocks.iter().cloned().collect();
    let mut paid: BTreeMap<ContentId, u64> = snapshot.paid.iter().cloned().collect();

    let mut refunded: u64 = 0;
    for item in &draft.sells {
        if !owned.contains(item) {
            return Err(CampaignError::NotOwned { item: item.clone() }.into());
        }
        let Some(refund) = paid.get(item).copied() else {
            return Err(CampaignError::NotPurchased { item: item.clone() }.into());
        };
        if draft.referenced.contains(item) {
            return Err(EconomyError::ActiveReference { item: item.clone() });
        }
        refunded = refunded
            .checked_add(refund)
            .ok_or(CampaignError::CurrencyOverflow {
                before: refunded,
                delta: refund,
            })?;
        owned.remove(item);
        paid.remove(item);
    }

    let mut charged: u64 = 0;
    for line in &draft.buys {
        if !graph.available_items(state).any(|item| *item == line.item) {
            return Err(CampaignError::ItemUnavailable {
                item: line.item.clone(),
            }
            .into());
        }
        if owned.contains(&line.item) {
            return Err(CampaignError::AlreadyOwned {
                item: line.item.clone(),
            }
            .into());
        }
        charged = charged
            .checked_add(line.price)
            .ok_or(CampaignError::CurrencyOverflow {
                before: charged,
                delta: line.price,
            })?;
        owned.insert(line.item.clone());
        paid.insert(line.item.clone(), line.price);
    }

    let available =
        snapshot
            .currency
            .checked_add(refunded)
            .ok_or(CampaignError::CurrencyOverflow {
                before: snapshot.currency,
                delta: refunded,
            })?;
    let currency = available
        .checked_sub(charged)
        .ok_or(CampaignError::InsufficientFunds {
            price: charged,
            balance: available,
        })?;

    match &draft.weight {
        LoadoutWeight::Measured { total, limit } if total > limit => {
            return Err(CampaignError::LoadoutOverweight {
                total: *total,
                limit: *limit,
            }
            .into());
        }
        LoadoutWeight::Measured { .. } => {}
        LoadoutWeight::Unknown { reason } => {
            return Err(CampaignError::LoadoutWeightUnknown {
                reason: reason.clone(),
            }
            .into());
        }
    }
    if draft.expected_revision != snapshot.revision {
        return Err(CampaignError::StaleRevision {
            expected: draft.expected_revision,
            actual: snapshot.revision,
        }
        .into());
    }

    snapshot.currency = currency;
    snapshot.unlocks = owned.into_iter().collect();
    snapshot.paid = paid.into_iter().collect();
    snapshot.revision += 1;
    let revision = snapshot.revision;
    // The whole result is validated before the one assignment.
    *state = CampaignState::restore(graph, snapshot)?;
    Ok(CommitReceipt {
        charged,
        refunded,
        revision,
    })
}
