//! The construction draft session (F44-B): where the shared validator of
//! `cs_content::construction` meets the transactional economy of
//! `cs_sim::economy`.
//!
//! A [`ConstructionSession`] holds an *edited copy* of a blueprint. Editing,
//! inspecting the verdict and cancelling never touch the profile;
//! [`ConstructionSession::commit`] validates the edited blueprint with the same
//! [`ConstructionRules::validate`] every other path uses, prices what the
//! profile does not yet own, and hands one [`ConstructionDraft`] to
//! [`cs_sim::economy::commit`].
//!
//! Persisting the committed state through `CampaignRun`'s profile session is
//! the UI stage's wiring (F44-C); this module commits to a [`CampaignState`].

use std::collections::BTreeSet;
use std::fmt;

use cs_content::construction::{
    AircraftBlueprint, BlueprintVerdict, BudgetRefusal, ConstraintViolation, ConstructionPolicy,
    ConstructionRules, PriceBook, ValidationRefusal,
};
use cs_sim::campaign::{CampaignGraph, CampaignState};
use cs_sim::economy::{self, CommitReceipt, ConstructionDraft, EconomyError};
use cs_types::content::ContentId;

use crate::campaign::loadout_weight;

/// The immutable rules a session is judged by.
#[derive(Clone, Copy, Debug)]
pub struct ConstructionContext<'a> {
    /// The airframe's rule profile.
    pub rules: &'a ConstructionRules,
    /// Pairing, banned list and catalog availability.
    pub policy: &'a ConstructionPolicy,
    /// Declared masses and prices.
    pub book: &'a PriceBook,
    /// The campaign graph, whose roster gates say what can be bought.
    pub graph: &'a CampaignGraph,
}

/// Why a commit was refused. The profile is unchanged in every case.
#[derive(Debug, PartialEq, Eq)]
pub enum ConstructionError {
    /// The validator could not judge the blueprint.
    Refused(ValidationRefusal),
    /// The blueprint is over a limit or breaks a constraint.
    Invalid(Box<BlueprintVerdict>),
    /// The economy refused the draft.
    Economy(EconomyError),
}

impl fmt::Display for ConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "{refusal}"),
            Self::Invalid(verdict) => {
                write!(
                    f,
                    "the blueprint is invalid: {} limit breach(es), {} violation(s)",
                    verdict.assessment().breaches().len(),
                    verdict.violations().len()
                )
            }
            Self::Economy(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ConstructionError {}

/// One editing session over a copy of a blueprint.
#[derive(Clone, Debug)]
pub struct ConstructionSession {
    expected_revision: u64,
    edited: AircraftBlueprint,
    sells: Vec<ContentId>,
    other_active: Vec<AircraftBlueprint>,
}

impl ConstructionSession {
    /// Opens a session on `blueprint` against the profile's current revision.
    /// `other_active` are the profile's other blueprints; components they use
    /// cannot be sold.
    #[must_use]
    pub fn begin(
        state: &CampaignState,
        blueprint: AircraftBlueprint,
        other_active: Vec<AircraftBlueprint>,
    ) -> Self {
        Self {
            expected_revision: state.revision(),
            edited: blueprint,
            sells: Vec::new(),
            other_active,
        }
    }

    /// The blueprint as currently edited.
    #[must_use]
    pub const fn blueprint(&self) -> &AircraftBlueprint {
        &self.edited
    }

    /// Replaces the edited blueprint.
    pub fn edit(&mut self, blueprint: AircraftBlueprint) {
        self.edited = blueprint;
    }

    /// Stages the sale of an owned component.
    pub fn queue_sale(&mut self, item: ContentId) {
        self.sells.push(item);
    }

    /// The validator's live verdict on the edited blueprint, for the preview.
    ///
    /// # Errors
    ///
    /// [`ValidationRefusal`].
    pub fn verdict(
        &self,
        ctx: &ConstructionContext<'_>,
    ) -> Result<BlueprintVerdict, ValidationRefusal> {
        ctx.rules
            .validate(&self.policy_for(ctx, None), &self.edited, ctx.book)
    }

    /// Abandons every edit. The profile was never touched.
    pub fn cancel(self) {}

    /// The caller's policy with availability widened by what the profile owns.
    fn policy_for(
        &self,
        ctx: &ConstructionContext<'_>,
        state: Option<&CampaignState>,
    ) -> ConstructionPolicy {
        let mut available: BTreeSet<ContentId> = ctx.policy.available().clone();
        if let Some(state) = state {
            available.extend(state.unlocks().cloned());
        }
        ConstructionPolicy::new(
            ctx.policy.pairable_guns().clone(),
            ctx.policy.banned().clone(),
            available,
        )
    }

    /// Validates, then commits purchases and sales as one profile revision.
    ///
    /// # Errors
    ///
    /// [`ConstructionError`]; on any error `state` is unchanged.
    pub fn commit(
        self,
        ctx: &ConstructionContext<'_>,
        state: &mut CampaignState,
    ) -> Result<CommitReceipt, ConstructionError> {
        let policy = self.policy_for(ctx, Some(state));
        let verdict = ctx
            .rules
            .validate(&policy, &self.edited, ctx.book)
            .map_err(ConstructionError::Refused)?;
        if !verdict.is_valid() {
            return Err(ConstructionError::Invalid(Box::new(verdict)));
        }
        let owned: BTreeSet<&ContentId> = state.unlocks().collect();
        let mut draft = ConstructionDraft::new(
            self.expected_revision,
            loadout_weight(ctx.rules, ctx.book, &self.edited),
        );
        let mut needed: BTreeSet<&ContentId> = BTreeSet::new();
        for (_, component) in self.edited.components() {
            if !owned.contains(component) && needed.insert(component) {
                // `validate` priced every component, so a missing price here is
                // refused rather than defaulted to free.
                let price = ctx
                    .book
                    .quote(component)
                    .and_then(|q| q.cost().clone().known())
                    .ok_or_else(|| {
                        ConstructionError::Refused(ValidationRefusal::Budget(
                            BudgetRefusal::UnknownCost {
                                component: component.clone(),
                            },
                        ))
                    })?
                    .as_minor();
                draft = draft.buy(component.clone(), price);
            }
        }
        for item in self.sells {
            draft = draft.sell(item);
        }
        let referenced = self
            .edited
            .components()
            .into_iter()
            .chain(self.other_active.iter().flat_map(|b| b.components()))
            .map(|(_, component)| component.clone());
        draft = draft.keep_referenced(referenced);
        economy::commit(state, ctx.graph, &draft).map_err(ConstructionError::Economy)
    }
}

/// Every violation of `verdict` that is a banned component, for the import path
/// to report why a foreign blueprint was refused.
#[must_use]
pub fn banned_components(verdict: &BlueprintVerdict) -> Vec<&ContentId> {
    verdict
        .violations()
        .iter()
        .filter_map(|violation| match violation {
            ConstraintViolation::BannedComponent { component, .. } => Some(component),
            _ => None,
        })
        .collect()
}
