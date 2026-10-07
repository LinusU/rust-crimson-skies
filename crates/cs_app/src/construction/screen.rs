//! The construction, paint and loadout screen (F44-C): the producer and
//! consumer of the [`ConstructionSession`] path.
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-C`; contract `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! A front end drives a [`ConstructionScreen`]:
//!
//! * [`ConstructionScreen::open`] begins the draft session on the blueprint the
//!   hangar offers; nothing is staged yet.
//! * [`ConstructionScreen::view`] is what the screen draws — the draft, the
//!   validator's live verdict normalized into [`QuantityMeter`]s, and the
//!   transaction a commit would stage. The numbers are the shared validator's,
//!   never recomputed here.
//! * The `set_*`/`fit_*` edits and [`ConstructionScreen::queue_sale`] mutate
//!   only the draft; [`ConstructionScreen::import`] adopts a foreign blueprint
//!   only after the session's own policy accepts it, so an import cannot meet
//!   a weaker rule than a hand-built loadout.
//! * [`ConstructionScreen::commit`] applies the staged transaction to a
//!   [`CampaignState`]; [`ConstructionScreen::commit_saved`] additionally
//!   persists it through the [`ProfileSession`] as one revision, the way a
//!   real campaign screen leaves its purchases durable.
//! * [`ConstructionScreen::cancel`] tears the screen down with nothing staged;
//!   every refused commit or save leaves the draft intact for a retry.

use std::fmt;

use cs_content::construction::{
    AircraftBlueprint, ArmorFitment, BlueprintVerdict, BudgetQuantity, ConstructionRules,
    ConstructionSchemaError, GunFitment, OrdnanceFitment, PaintSelection, ValidationRefusal,
};
use cs_sim::campaign::CampaignState;
use cs_sim::economy::{BuyLine, CommitReceipt, ConstructionDraft};
use cs_types::content::ContentId;

use crate::campaign::{CampaignSaveError, read_snapshot, write_snapshot};
use crate::profile::{ChangeRefusal, ChangeRefusalReason, ProfileSession, SessionError};

use super::{ConstructionContext, ConstructionError, ConstructionSession};

/// One budget quantity normalized against its limit — what a loadout meter
/// draws.
///
/// `used` and `limit` are in the quantity's own integer units: weight units
/// for [`BudgetQuantity::Mass`], minor units for [`BudgetQuantity::Cost`], a
/// slot count for the two position quantities. `permille` is `used` of `limit`
/// in thousandths, saturated above the limit; a zero limit with any use is
/// `u64::MAX`, so a bar can always distinguish "full" from "over".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantityMeter {
    /// Which budget quantity the meter shows.
    pub quantity: BudgetQuantity,
    /// How much of it the draft uses.
    pub used: u64,
    /// Its ceiling.
    pub limit: u64,
    /// `used / limit` in permille, saturated.
    pub permille: u64,
}

fn meter(quantity: BudgetQuantity, used: u64, limit: u64) -> QuantityMeter {
    let permille = if limit == 0 {
        if used == 0 { 0 } else { u64::MAX }
    } else {
        u64::try_from(u128::from(used) * 1000 / u128::from(limit)).unwrap_or(u64::MAX)
    };
    QuantityMeter {
        quantity,
        used,
        limit,
        permille,
    }
}

/// The meters for the four measured quantities, in [`BudgetQuantity::ALL`]
/// order. A quantity whose limit is unmeasured is absent — but a verdict then
/// cannot be `Ok`, so a view only ever shows measured bars.
fn meters(rules: &ConstructionRules, verdict: &BlueprintVerdict) -> Vec<QuantityMeter> {
    let totals = verdict.assessment().totals();
    let mut out = Vec::with_capacity(BudgetQuantity::ALL.len());
    if let Some(limit) = rules.max_mass().clone().known() {
        out.push(meter(
            BudgetQuantity::Mass,
            totals.mass().as_units(),
            limit.as_units(),
        ));
    }
    if let Some(limit) = rules.max_cost().clone().known() {
        out.push(meter(
            BudgetQuantity::Cost,
            totals.cost().as_minor(),
            limit.as_minor(),
        ));
    }
    if let Some(limit) = rules.gun_positions().clone().known() {
        out.push(meter(
            BudgetQuantity::GunPositions,
            u64::from(totals.gun_positions()),
            u64::from(limit),
        ));
    }
    if let Some(limit) = rules.rocket_hardpoints().clone().known() {
        out.push(meter(
            BudgetQuantity::RocketHardpoints,
            u64::from(totals.rocket_hardpoints()),
            u64::from(limit),
        ));
    }
    out
}

/// The transaction a commit would stage, projected for display.
///
/// `charge` is the buy lines' prices summed and `credit` what the staged sales
/// would refund from `state`'s paid ledger — the same numbers
/// [`economy::commit`](cs_sim::economy::commit) reports on success. The sums
/// saturate only where `u64` cannot hold them, a case `commit` itself refuses
/// as a currency overflow, so a displayed total is never a promise the commit
/// could not keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingTransaction {
    /// What the commit would buy, each with the price it will charge.
    pub buys: Vec<BuyLine>,
    /// What the commit would sell.
    pub sells: Vec<ContentId>,
    /// The exact charge, in minor units.
    pub charge: u64,
    /// The refund the staged sales would credit, in minor units.
    pub credit: u64,
}

impl PendingTransaction {
    fn project(draft: &ConstructionDraft, state: &CampaignState) -> Self {
        let charge = draft
            .buys()
            .iter()
            .fold(0, |sum: u64, line| sum.saturating_add(line.price));
        let credit = draft.sells().iter().fold(0, |sum: u64, item| {
            sum.saturating_add(state.paid_for(item).unwrap_or(0))
        });
        Self {
            buys: draft.buys().to_vec(),
            sells: draft.sells().to_vec(),
            charge,
            credit,
        }
    }
}

/// What the screen draws, projected for a renderer that holds no session.
#[derive(Clone, Debug)]
pub struct ConstructionView {
    /// The draft as edited — its slots and the paint references the renderer
    /// resolves through the livery path. Nothing in it is pixel data.
    pub blueprint: AircraftBlueprint,
    /// The validator's verdict on the draft, or the refusal when it cannot be
    /// measured. It is produced by the same policy a commit enforces, so what
    /// the screen shows can never disagree with what a commit would do.
    pub verdict: Result<BlueprintVerdict, ValidationRefusal>,
    /// Per-quantity usage normalized against the rule profile's limits, in
    /// [`BudgetQuantity::ALL`] order; empty when the draft cannot be measured.
    pub meters: Vec<QuantityMeter>,
    /// The transaction a commit would stage — `None` when the draft cannot be
    /// priced, in which case `verdict` or `notice` says why.
    pub pending: Option<PendingTransaction>,
    /// The last refusal the screen kept for display. Cleared by the next edit
    /// or by a successful commit.
    pub notice: Option<String>,
    /// The receipt of the screen's last successful commit.
    pub committed: Option<CommitReceipt>,
    /// Whether the draft or the staged sales differ from the committed state.
    pub dirty: bool,
}

/// Why an imported blueprint was not adopted. The screen's draft is unchanged
/// in either case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportRejection {
    /// The foreign blueprint could not be measured at all — an unmeasured
    /// limit, price or pairing rule is a refusal, not a guess.
    Refused(ValidationRefusal),
    /// It breaks a constraint or a limit; the verdict names which, and
    /// [`super::banned_components`] picks the host bans out of it.
    Invalid(Box<BlueprintVerdict>),
}

impl fmt::Display for ImportRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => {
                write!(f, "the imported blueprint cannot be judged: {refusal}")
            }
            Self::Invalid(verdict) => write!(
                f,
                "the imported blueprint is invalid: {} limit breach(es), {} violation(s)",
                verdict.assessment().breaches().len(),
                verdict.violations().len()
            ),
        }
    }
}

impl std::error::Error for ImportRejection {}

/// Why a commit-and-save attempt failed. The draft, the in-memory state and
/// the stored save are all left as they were — the screen stays open for a
/// retry.
#[derive(Debug)]
pub enum ScreenSaveError {
    /// The commit was refused before anything was staged.
    Refused(ConstructionError),
    /// The commit succeeded in memory but the profile save refused or failed;
    /// the in-memory state is also left unchanged, so what the player sees
    /// still matches the disk.
    Save(CampaignSaveError),
}

impl fmt::Display for ScreenSaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(error) => write!(f, "{error}"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScreenSaveError {}

/// The construction / paint / loadout screen: the draft session plus the
/// screen state a front end needs.
///
/// The screen *is* the producer the F44-B session was built for: it holds the
/// edited blueprint, the staged sales and the other active blueprints, and it
/// is the only place that decides what a commit means — every mutating method
/// other than a commit mutates the draft only.
#[derive(Debug)]
pub struct ConstructionScreen {
    session: ConstructionSession,
    /// The profile's other active blueprints, kept so a successful commit can
    /// reopen the session against the moved state.
    other_active: Vec<AircraftBlueprint>,
    /// Whether the draft or the staged sales differ from the committed state.
    dirty: bool,
    /// The last refusal, kept for [`Self::view`] to display.
    notice: Option<String>,
    /// The last successful commit's receipt, still on screen.
    committed: Option<CommitReceipt>,
}

impl ConstructionScreen {
    /// Opens the screen on `blueprint` against `state`. `other_active` are the
    /// profile's other blueprints; components they use cannot be sold.
    #[must_use]
    pub fn open(
        state: &CampaignState,
        blueprint: AircraftBlueprint,
        other_active: Vec<AircraftBlueprint>,
    ) -> Self {
        Self {
            session: ConstructionSession::begin(state, blueprint, other_active.clone()),
            other_active,
            dirty: false,
            notice: None,
            committed: None,
        }
    }

    /// The draft as currently edited.
    #[must_use]
    pub const fn blueprint(&self) -> &AircraftBlueprint {
        self.session.blueprint()
    }

    /// Whether the draft or the staged sales differ from the committed state —
    /// what a "discard changes?" prompt keys on.
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The last refusal kept for display.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The receipt of the screen's last successful commit.
    #[must_use]
    pub const fn committed(&self) -> Option<&CommitReceipt> {
        self.committed.as_ref()
    }

    /// What the screen draws. `state` is the profile the draft is priced and
    /// owned-checked against — the same state a commit would be handed.
    #[must_use]
    pub fn view(&self, ctx: &ConstructionContext<'_>, state: &CampaignState) -> ConstructionView {
        let verdict = self.session.verdict(ctx);
        let meters = verdict
            .as_ref()
            .map(|verdict| meters(ctx.rules, verdict))
            .unwrap_or_default();
        let pending = self
            .session
            .commit_draft(ctx, state)
            .ok()
            .map(|draft| PendingTransaction::project(&draft, state));
        ConstructionView {
            blueprint: self.session.blueprint().clone(),
            verdict,
            meters,
            pending,
            notice: self.notice.clone(),
            committed: self.committed.clone(),
            dirty: self.dirty,
        }
    }

    /// Replaces the whole draft with `blueprint` — the structural edits a
    /// rebuilt record expresses. The blueprint's own construction already
    /// kind-checked every slot, so this cannot fail.
    pub fn edit(&mut self, blueprint: AircraftBlueprint) {
        self.replace(blueprint);
    }

    /// The paint editor's edit: a different mask/decal selection on the same
    /// draft. The selection carries catalog references only — no texture data
    /// ever enters the blueprint.
    pub fn set_paint(&mut self, paint: PaintSelection) {
        self.replace(self.session.blueprint().clone().with_paint(paint));
    }

    /// The airframe picker's edit.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] when `airframe` is not an `airframe` id; the
    /// draft is unchanged.
    pub fn set_airframe(&mut self, airframe: ContentId) -> Result<(), ConstructionSchemaError> {
        let blueprint = self.session.blueprint().clone().with_airframe(airframe)?;
        self.replace(blueprint);
        Ok(())
    }

    /// The engine picker's edit.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] when `engine` is not an `engine` id; the
    /// draft is unchanged.
    pub fn set_engine(&mut self, engine: ContentId) -> Result<(), ConstructionSchemaError> {
        let blueprint = self.session.blueprint().clone().with_engine(engine)?;
        self.replace(blueprint);
        Ok(())
    }

    /// The armor editor's edit.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a repeated armor zone; the draft is
    /// unchanged.
    pub fn fit_armor(&mut self, armor: Vec<ArmorFitment>) -> Result<(), ConstructionSchemaError> {
        let blueprint = self.session.blueprint().clone().with_armor(armor)?;
        self.replace(blueprint);
        Ok(())
    }

    /// The gun rack's edit.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a repeated weapon mount; the draft is
    /// unchanged.
    pub fn fit_guns(&mut self, guns: Vec<GunFitment>) -> Result<(), ConstructionSchemaError> {
        let blueprint = self.session.blueprint().clone().with_guns(guns)?;
        self.replace(blueprint);
        Ok(())
    }

    /// The ordnance editor's edit.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a repeated hardpoint; the draft is
    /// unchanged.
    pub fn fit_ordnance(
        &mut self,
        ordnance: Vec<OrdnanceFitment>,
    ) -> Result<(), ConstructionSchemaError> {
        let blueprint = self.session.blueprint().clone().with_ordnance(ordnance)?;
        self.replace(blueprint);
        Ok(())
    }

    /// The equipment picker's edit.
    ///
    /// # Errors
    ///
    /// [`ConstructionSchemaError`] on a wrong-namespace or repeated item; the
    /// draft is unchanged.
    pub fn set_equipment(
        &mut self,
        equipment: Vec<ContentId>,
    ) -> Result<(), ConstructionSchemaError> {
        let blueprint = self.session.blueprint().clone().with_equipment(equipment)?;
        self.replace(blueprint);
        Ok(())
    }

    /// Stages the sale of an owned component. Whether the sale is legal —
    /// owned, paid for, and unreferenced by every active blueprint — is the
    /// commit's call, made at commit time against the then-current state.
    pub fn queue_sale(&mut self, item: ContentId) {
        self.session.queue_sale(item);
        self.dirty = true;
        self.notice = None;
    }

    /// The draft as a portable record for the export path.
    ///
    /// Every field is a catalog id or an authored number — paint and decals
    /// stay references, so an export never embeds a source texture. Importing
    /// the record goes through [`Self::import`], never around the validator.
    #[must_use]
    pub fn export(&self) -> AircraftBlueprint {
        self.session.blueprint().clone()
    }

    /// The import path: judges `foreign` by this screen's own policy — the
    /// same [`ConstructionRules::validate`] and the same policy a hand edit and
    /// a commit face — and adopts it as the draft only when the verdict is
    /// valid.
    ///
    /// An imported blueprint therefore cannot bypass paired-gun, banned or
    /// availability rules: there is no second, weaker rule for records that
    /// arrive from outside.
    ///
    /// # Errors
    ///
    /// [`ImportRejection`]; the draft is unchanged, and the rejection is also
    /// kept as the screen's [`Self::notice`] for display.
    pub fn import(
        &mut self,
        ctx: &ConstructionContext<'_>,
        foreign: AircraftBlueprint,
    ) -> Result<BlueprintVerdict, ImportRejection> {
        let verdict = match self.session.verdict_for(ctx, &foreign) {
            Ok(verdict) => verdict,
            Err(refusal) => {
                let rejection = ImportRejection::Refused(refusal);
                self.notice = Some(rejection.to_string());
                return Err(rejection);
            }
        };
        if !verdict.is_valid() {
            let rejection = ImportRejection::Invalid(Box::new(verdict));
            self.notice = Some(rejection.to_string());
            return Err(rejection);
        }
        self.replace(foreign);
        Ok(verdict)
    }

    /// Commits the staged transaction to `state`.
    ///
    /// A refusal changes nothing — `state`, the draft and the staged sales are
    /// exactly as before, so the screen is still open and the player can fix
    /// the loadout and retry. A success re-bases the session on the committed
    /// state: the committed blueprint is the new draft base, the staged sales
    /// are spent and the revision tracks the profile again.
    ///
    /// # Errors
    ///
    /// [`ConstructionError`]; kept as the screen's [`Self::notice`] as well.
    pub fn commit(
        &mut self,
        ctx: &ConstructionContext<'_>,
        state: &mut CampaignState,
    ) -> Result<CommitReceipt, ConstructionError> {
        match self.session.clone().commit(ctx, state) {
            Ok(receipt) => {
                self.rebase(state);
                self.committed = Some(receipt.clone());
                Ok(receipt)
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                Err(error)
            }
        }
    }

    /// Commits the staged transaction to `state` *and* writes it to `session`'s
    /// profile — the save a real campaign screen owes.
    ///
    /// The commit is computed on a copy of the state and that copy becomes the
    /// live state only after the save returned, so the disk and memory hold the
    /// same revision or the same old revision — never a mixture. The save is
    /// one [`ProfileSession::commit_with`] that refuses when the stored
    /// campaign moved from `state`'s base revision: a concurrent writer's
    /// progression is never overwritten by a stale screen.
    ///
    /// # Errors
    ///
    /// [`ScreenSaveError`]; kept as the screen's [`Self::notice`]. On
    /// `ScreenSaveError::Save` the commit succeeded in memory but was not
    /// written, and the live state is reverted so a retry is safe.
    pub fn commit_saved(
        &mut self,
        ctx: &ConstructionContext<'_>,
        state: &mut CampaignState,
        session: &mut ProfileSession,
    ) -> Result<CommitReceipt, ScreenSaveError> {
        let base = state.revision();
        let mut next = state.clone();
        let receipt = match self.session.clone().commit(ctx, &mut next) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.notice = Some(error.to_string());
                return Err(ScreenSaveError::Refused(error));
            }
        };
        if let Err(error) = save_committed(session, &next, base) {
            self.notice = Some(error.to_string());
            return Err(ScreenSaveError::Save(error));
        }
        *state = next;
        self.rebase(state);
        self.committed = Some(receipt.clone());
        Ok(receipt)
    }

    /// Tears the screen down, discarding the draft. Nothing was ever staged
    /// outside it, so cancel — like dropping the screen — cannot move the
    /// profile.
    pub fn cancel(self) {
        self.session.cancel();
    }

    /// A draft-changing edit: marks the screen dirty and drops the stale
    /// notice, which described the old draft.
    fn replace(&mut self, blueprint: AircraftBlueprint) {
        self.session.edit(blueprint);
        self.dirty = true;
        self.notice = None;
    }

    /// Reopens the session on the committed state: the committed blueprint is
    /// the draft base, the spent sales are gone and the expected revision is
    /// the profile's new one.
    fn rebase(&mut self, state: &CampaignState) {
        self.session = ConstructionSession::begin(
            state,
            self.session.blueprint().clone(),
            self.other_active.clone(),
        );
        self.dirty = false;
        self.notice = None;
    }
}

/// Writes `next`'s snapshot into the selected profile, refusing when the
/// stored campaign is not still at `base`.
///
/// This is [`CampaignRun`](crate::campaign::CampaignRun)'s persist transaction
/// restated for a caller that holds the bare [`CampaignState`]: the stored
/// revision is re-read inside the `commit_with` change so a conflict retry
/// sees a concurrent writer's progression and refuses rather than overwriting
/// it. `campaign.rs` is outside F44-C's owner paths; when a task owns it, a
/// `CampaignRun` method can wrap this same transaction.
fn save_committed(
    session: &mut ProfileSession,
    next: &CampaignState,
    base: u64,
) -> Result<(), CampaignSaveError> {
    let snapshot = next.snapshot();
    let mut failure: Option<CampaignSaveError> = None;
    let committed = session.commit_with(|document| {
        let refuse = |reason: &str| ChangeRefusal {
            subject: "campaign".to_owned(),
            reason: ChangeRefusalReason::Malformed(reason.to_owned()),
        };
        // A conflict retry re-reads the store: if another writer moved the
        // campaign, applying this commit over it would overwrite progression
        // this screen never saw.
        match read_snapshot(document) {
            Ok(stored) => {
                let stored = stored.map(|held| held.revision);
                if stored != Some(base) {
                    failure = Some(CampaignSaveError::Stale {
                        expected: base,
                        stored: stored.unwrap_or(0),
                    });
                    return Err(refuse("the stored campaign moved"));
                }
            }
            Err(error) => {
                failure = Some(error);
                return Err(refuse("the stored campaign is damaged"));
            }
        }
        if let Err(error) = write_snapshot(document, &snapshot) {
            failure = Some(error);
            return Err(refuse("the campaign cannot be represented"));
        }
        Ok(())
    });
    match committed {
        Ok(_) => Ok(()),
        Err(SessionError::Refused(_)) if failure.is_some() => {
            Err(failure.expect("checked just above"))
        }
        Err(error) => Err(CampaignSaveError::Session(error)),
    }
}
