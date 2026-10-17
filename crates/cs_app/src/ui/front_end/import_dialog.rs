//! The legacy-import dialog screen (F64-C's report, drawn by the F45 front
//! end).
//!
//! Spec: `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`
//! (`### F64-C`) presented through the state table of
//! `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`
//! (`### F45-A`/`### F45-B`). Shared contracts:
//! `docs/contracts/UI-NETWORK.md` ("Use an explicit state table for every
//! screen and Back/Cancel path. A UI action requests a domain transaction; it
//! does not directly edit campaign cash, ownership or objective fields",
//! "Focus and accessibility state reset sensibly on entry") and
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Persistent profile data receives
//! only an explicit outcome transaction", "Retry restores the authored
//! initial state, not a mutated copy of the just-failed world").
//!
//! F64-C built [`crate::ui::import`] as the value boundary: [`ImportFlow`]
//! produces one [`MigrationView`] (or a [`crate::ui::import::RefusedView`])
//! per attempt, and [`ImportFlow::confirm`] is the explicit owner action.
//! Nothing drew it: the front end had no import dialog and
//! `crates/cs_app/src/ui/front_end/` was not an F64-C owner path. This module
//! is that screen, and [`super::FrontEnd`] carries it as the
//! [`super::Screen::ImportDialog`] state:
//!
//! * [`ImportDialog`] owns exactly what a screen may own: the **context** the
//!   dialog was opened with (an immutable snapshot, cloned once, precisely as
//!   [`ImportContext`] documents), the **offered source** (its bytes, its
//!   declared proposal, the layout the caller found for the class, its limits,
//!   its optional field map, the new profile it would land in and the
//!   installation identity it was found under), and the [`ImportFlow`] whose
//!   attempt is on screen.
//! * `Confirm`, `Cancel` and `Retry` are [`super::Action`]s the front-end
//!   table routes here, and every one of them goes through the same
//!   machine-wide path: [`super::FrontEnd::plan`] runs the transition on a
//!   throwaway copy, the domain transaction runs, and only then does the real
//!   machine move.
//! * Confirming emits the one domain transaction this screen asks for,
//!   [`super::Request::ConfirmImport`], **only** when
//!   [`MigrationView::confirmable`] is `Ok`; otherwise the machine refuses by
//!   the reason ([`super::Refusal::Import`] carries the producer's own
//!   [`ConfirmError`]) and the report stays on screen. The value itself — the
//!   [`ConfirmedImport`] with its retained
//!   [`SourceFingerprint`](cs_content::legacy_import::SourceFingerprint) and
//!   migration report — stays in this screen, which is what lets a later
//!   persistence
//!   layer (Rally #756) read it instead of re-deriving anything.
//! * Cancel is the **teardown**: [`ImportFlow::dismiss`] drops the attempt and
//!   the dialog, so no line, verdict or refusal of it can be read afterwards.
//!   Retry is the teardown followed by a fresh run of the whole production
//!   pipeline over the same source.
//!
//! # What this screen cannot do
//!
//! * **No write path exists here.** The dialog holds bytes and a relative
//!   spelling; it holds no file handle, no writer and no host path. It is
//!   therefore structurally incapable of touching the source or a save, which
//!   is spec F64 non-negotiable 1's "never overwrite original save/blueprint
//!   files" as a fact about the type rather than a rule this file obeys.
//! * **No line's meaning is re-derived.** [`Self::lines`] hands back the
//!   flow's own [`ReportLine`]s, in the producer's order, so the screen cannot
//!   rename a code, reorder a record, reformat a breach pair or soften a
//!   verdict: what F64-C rendered is what is drawn. The fixture marker is not
//!   special-cased here either — when [`MigrationView::fixture_admitted`] is
//!   true the producer has already emitted its `fixture_admitted`
//!   [`ReportLine::Notice`], and the screen shows it exactly like every other
//!   line.
//! * **No persistence.** Creating the profile a confirmed import lands in is
//!   the profile store's transaction (Rally #756); this screen stages the
//!   outcome and nothing more.
//!
//! Everything synthetic this module's tests offer is designed fixture data
//! read through the production `read_legacy_profile`, resolved by the
//! production `LegacyIdMap`/`Catalog` pair and judged by the production
//! `ConstructionRules::validate`. Nothing here is `verified_original`: no
//! legacy save or custom-plane byte layout has been measured (F64-B), so a
//! retail offer is still refused by name before a byte is judged.

use cs_content::catalog::Catalog;
use cs_content::construction::{ConstructionPolicy, ConstructionRules, PriceBook};
use cs_content::legacy_import::{BlueprintFieldMap, LayoutAdmission, LegacyIdMap, TargetProfile};
use cs_formats::legacy_profile::{ArtifactProposal, LegacyLayout, LegacyLimits};
use cs_types::content::{Origin, Provenance};
use cs_types::install::InstallIdentity;

use crate::ui::import::{
    ConfirmError, ConfirmedImport, ImportContext, ImportFlow, ImportOffer, ImportOutcome,
    MigrationVerdict, MigrationView, ReportLine,
};

/// The context a dialog was opened with, owned for the life of the dialog.
///
/// [`ImportContext`] is documented as "what stays put while the dialog is
/// open" and "the screen builds one of these when it opens and hands it to
/// every [`ImportFlow::offer`] until the dialog closes". A screen is an owned
/// value, so the screen holds its own copy: the catalog, the stock
/// rules/policy/price book, the declared id table and the enhancement switch
/// are read-only data no producer mutates, and the copy is taken once at
/// open, never re-read per attempt.
#[derive(Clone, Debug)]
struct DialogContext {
    ids: LegacyIdMap,
    catalog: Catalog,
    rules: ConstructionRules,
    policy: ConstructionPolicy,
    book: PriceBook,
    admission: LayoutAdmission,
    legacy_save_import_enabled: bool,
    origin: Origin,
    provenance: Provenance,
}

impl DialogContext {
    fn of(context: &ImportContext<'_>) -> Self {
        Self {
            ids: context.ids.clone(),
            catalog: context.catalog.clone(),
            rules: context.rules.clone(),
            policy: context.policy.clone(),
            book: context.book.clone(),
            admission: context.admission,
            legacy_save_import_enabled: context.legacy_save_import_enabled,
            origin: context.origin.clone(),
            provenance: context.provenance.clone(),
        }
    }

    fn borrow(&self) -> ImportContext<'_> {
        ImportContext {
            ids: &self.ids,
            catalog: &self.catalog,
            rules: &self.rules,
            policy: &self.policy,
            book: &self.book,
            admission: self.admission,
            legacy_save_import_enabled: self.legacy_save_import_enabled,
            origin: self.origin.clone(),
            provenance: self.provenance.clone(),
        }
    }
}

/// The offered source, owned so the dialog can offer, retry and re-read it
/// without the caller handing the same bytes over again.
///
/// The bytes are the ones the caller read; the proposal carries the
/// **declared** fingerprint, which `plan_import` verifies against those bytes
/// before any report describes them.
#[derive(Clone, Debug)]
struct DialogOffer {
    proposal: ArtifactProposal,
    bytes: Vec<u8>,
    layout: Option<LegacyLayout>,
    limits: LegacyLimits,
    field_map: Option<BlueprintFieldMap>,
    target: TargetProfile,
    install_identity: Option<InstallIdentity>,
}

impl DialogOffer {
    fn of(offer: &ImportOffer<'_>) -> Self {
        Self {
            proposal: offer.source.clone(),
            bytes: offer.bytes.to_vec(),
            layout: offer.layout.cloned(),
            limits: offer.limits,
            field_map: offer.field_map.cloned(),
            target: offer.target.clone(),
            install_identity: offer.install_identity.clone(),
        }
    }

    fn borrow(&self) -> ImportOffer<'_> {
        ImportOffer {
            source: &self.proposal,
            bytes: &self.bytes,
            layout: self.layout.as_ref(),
            limits: self.limits,
            field_map: self.field_map.as_ref(),
            target: &self.target,
            install_identity: self.install_identity.clone(),
        }
    }
}

/// The front end's legacy-import dialog: one offered source, its migration
/// report, and the three decisions a player can take on it.
///
/// The dialog is opened on [`super::Screen::ImportDialog`] with
/// [`super::FrontEnd::open_import_dialog`], which is when the first attempt
/// runs; from then on ascending, retrying and tearing the attempt down are
/// [`super::Action`]s applied by the state machine, so a click, a key press
/// and the equivalent automation are the same transition.
#[derive(Clone, Debug)]
pub struct ImportDialog {
    context: DialogContext,
    offer: DialogOffer,
    flow: ImportFlow,
}

impl ImportDialog {
    /// Opens the dialog on `offer` and runs its first attempt immediately.
    ///
    /// The context is the caller's, handed over by shared reference exactly as
    /// [`ImportFlow::offer`] takes it; the dialog keeps its own immutable copy
    /// for later attempts. The source's bytes are copied for the same reason:
    /// a retry must read *these* bytes again, not whatever the caller's buffer
    /// holds by then.
    #[must_use]
    pub fn open(context: &ImportContext<'_>, offer: &ImportOffer<'_>) -> Self {
        let mut dialog = Self {
            context: DialogContext::of(context),
            offer: DialogOffer::of(offer),
            flow: ImportFlow::new(),
        };
        dialog.run();
        dialog
    }

    /// Re-runs the whole production pipeline over the same source from a clean
    /// stage: teardown first, then plan, optional blueprint assessment and
    /// render. Nothing of the attempt it replaces — no line, no unresolved
    /// row, no partial verdict — can be read afterwards.
    pub fn retry(&mut self) -> &ImportOutcome {
        let context = self.context.borrow();
        let offer = self.offer.borrow();
        self.flow.retry(&context, &offer)
    }

    /// The explicit owner action: turn the current report into the
    /// [`ConfirmedImport`] a persistence layer may carry.
    ///
    /// # Errors
    ///
    /// [`ConfirmError`] of [`ImportFlow::confirm`]: no report, a refused
    /// attempt, a plan that would carry nothing, or no record that is both
    /// planned and stock-conforming. The flow is left exactly as it was, so
    /// the screen keeps showing the report that explains the refusal.
    pub fn confirm(&mut self) -> Result<&ConfirmedImport, ConfirmError> {
        self.flow.confirm()
    }

    /// Tears the attempt down without leaving a trace of it: the flow returns
    /// to idle and no line, verdict, refusal or confirmation of the dismissed
    /// attempt can be read any more. The attempt counter is kept, so a log
    /// line still names the attempt it came from.
    pub fn cancel(&mut self) {
        self.flow.dismiss();
    }

    /// How many attempts this dialog has made, including refused ones.
    #[must_use]
    pub const fn attempt(&self) -> u64 {
        self.flow.attempt()
    }

    /// Whether the dialog is waiting for an attempt (nothing offered, or the
    /// last one was cancelled).
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.flow.is_idle()
    }

    /// The declared spelling of the offered source.
    ///
    /// Declared, not verified: it is what the caller proposed. The verified
    /// spelling and digest of a report that was actually read are the ones its
    /// [`ReportLine::Source`] carries.
    #[must_use]
    pub fn source_spelling(&self) -> &str {
        self.offer.proposal.spelling().as_str()
    }

    /// The rows the screen draws, in the producer's order.
    ///
    /// These are [`crate::ui::import`]'s own [`ReportLine`]s, not a second
    /// rendering of them: a refused attempt's refusal line and
    /// nothing-was-written notice, or a report's source, class,
    /// inventory-evidence, layout, version, verdict, record, unresolved,
    /// blueprint, breach, constraint and notice lines. The dialog never
    /// renames a code, reorders a row or reformats a value.
    ///
    /// Empty while the dialog is idle or has already been confirmed: there is
    /// no attempt on screen to draw.
    #[must_use]
    pub fn lines(&self) -> &[ReportLine] {
        match self.flow.outcome() {
            Some(ImportOutcome::Presented(view)) => view.lines(),
            Some(ImportOutcome::Refused(refused)) => refused.lines(),
            None => &[],
        }
    }

    /// The overall verdict of the attempt on screen, when it produced a report
    /// (a refused attempt has no verdict at all — never
    /// [`MigrationVerdict::Refused`], which is not an import class).
    #[must_use]
    pub fn verdict(&self) -> Option<MigrationVerdict> {
        self.flow.view().map(MigrationView::verdict)
    }

    /// The propagated refusal's stable code, when the attempt was refused.
    #[must_use]
    pub fn refusal_code(&self) -> Option<&'static str> {
        self.flow
            .outcome()
            .and_then(ImportOutcome::refusal)
            .map(|refused| refused.code())
    }

    /// Whether the attempt on screen could only be produced from designed
    /// fixture data.
    ///
    /// True exactly when the producer marked its report so; the screen then
    /// draws the producer's `fixture_admitted` [`ReportLine::Notice`], so a
    /// fixture report can never read as a measured import of an original
    /// file.
    #[must_use]
    pub fn fixture_admitted(&self) -> bool {
        self.flow
            .view()
            .is_some_and(MigrationView::fixture_admitted)
    }

    /// Whether the explicit owner action may be taken on the attempt on
    /// screen, and why not when it may not.
    ///
    /// This is the screen's own reading of the same rule
    /// [`ImportFlow::confirm`] enforces, so a screen can draw a disabled
    /// Confirm button without asking the domain anything.
    ///
    /// # Errors
    ///
    /// As [`MigrationView::confirmable`], plus [`ConfirmError::Refused`] while
    /// a refused attempt is on screen and
    /// [`ConfirmError::NothingToConfirm`] when there is no attempt at all.
    pub fn confirmable(&self) -> Result<(), ConfirmError> {
        match self.flow.view() {
            Some(view) => view.confirmable(),
            None => Err(match self.flow.refusal_view() {
                Some(refused) => ConfirmError::Refused {
                    code: refused.code(),
                },
                None => ConfirmError::NothingToConfirm,
            }),
        }
    }

    /// The confirmed import, once the owner has taken the explicit action.
    ///
    /// Retained whole: the new profile it lands in, the verified source
    /// fingerprint, the migration report the owner was shown, the blueprint
    /// verdicts and the record indices a persistence layer may carry.
    #[must_use]
    pub fn confirmed(&self) -> Option<&ConfirmedImport> {
        self.flow.confirmed()
    }

    /// Runs one attempt over the owned context and source.
    fn run(&mut self) -> &ImportOutcome {
        let context = self.context.borrow();
        let offer = self.offer.borrow();
        self.flow.offer(&context, &offer)
    }
}
