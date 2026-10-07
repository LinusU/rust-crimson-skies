//! Import validation and the user-facing migration report (F64-C).
//!
//! Spec `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-C`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`
//! ("Persistent profile data receives only an explicit outcome transaction";
//! "Retry restores the authored initial state, not a mutated copy of the
//! just-failed world").
//!
//! [`cs_content::legacy_import`] *produces* the read-only import plan and the
//! blueprint verdicts; nothing there asks the player anything and nothing
//! there draws. This module is their **consumer**: the boundary a screen
//! drives to offer one legacy source, read back one migration report and —
//! only on an explicit owner action — hand a confirmed import to whoever
//! persists it. It owns four things:
//!
//! * [`ImportContext`] — what stays put while the dialog is open: the declared
//!   legacy-id table, the catalog identities resolve against, the stock
//!   construction rules/policy/price book, the layout-admission policy and
//!   the `cs.profile.legacy_save_import` switch.
//! * [`ImportOffer`] — one attempt: the candidate source with its **declared**
//!   fingerprint, the bytes, the layout the screen found for the offered
//!   class (or [`None`] while this build has measured none), the
//!   [`BlueprintFieldMap`] when the class carries blueprint roles, and the
//!   **new** [`TargetProfile`] the import would land in.
//! * [`ImportFlow`] — the state machine. [`ImportFlow::offer`] runs the
//!   production pipeline and tears down whatever the previous attempt left
//!   behind, [`ImportFlow::dismiss`] drops an attempt without residue,
//!   [`ImportFlow::retry`] re-runs from that clean state, and
//!   [`ImportFlow::confirm`] is the explicit owner action non-negotiable 4
//!   requires before anything is imported.
//! * [`MigrationView`] and [`ReportLine`] — what the screen shows: a verdict
//!   of full, partial, unsupported or refused, the retained source
//!   fingerprint, one line per record, unresolved row, limit breach and
//!   notice, each carrying a stable machine [`ReportLine::code`].
//!
//! # What the boundary guarantees
//!
//! 1. **No write path exists here.** The module holds no file handle, no
//!    writer and no host path; [`ImportOffer`] carries bytes the caller
//!    already read and a *relative* spelling. Every refusal therefore leaves
//!    the source and every profile untouched as a structural fact (spec F64
//!    non-negotiable 1), and the acceptance tests still pin it on a real
//!    filesystem (AC01).
//! 2. **The source is verified before it is described.** The
//!    [`SourceFingerprint`] a [`MigrationView`] shows is the one
//!    `plan_import` compared against the offered bytes — a proposal whose
//!    declared size or digest does not describe those bytes is refused before
//!    any document is read, so a report can never describe a file that was
//!    not the one fingerprinted.
//! 3. **A refusal is propagated, never translated into a partial success.**
//!    Every refusal the producer can return — a hostile source, a
//!    fixture-only layout, the disabled optional enhancement, an unreadable
//!    document, a field map that cannot describe the layout — reaches the
//!    screen as [`RefusedView`] carrying the original structured error and
//!    its code, together with the record that nothing was written. When this
//!    build has measured no layout for the class at all, the offer is refused
//!    **before any byte is judged** by [`FlowRefusal::NoMeasuredLayout`],
//!    which names the class and its inventory-row evidence instead of
//!    pretending to read a format that has never been seen.
//! 4. **Teardown and retry are first-class.** An attempt's report never
//!    survives the next offer: [`ImportFlow::offer`] starts from a clean
//!    stage, [`ImportFlow::dismiss`] returns the flow to
//!    [`ImportFlow::is_idle`], and [`ImportFlow::retry`] is that teardown
//!    followed by a fresh run of the whole pipeline. Nothing of a failed
//!    attempt — no line, no partial verdict — can be read after it.
//! 5. **Confirming is an owner action that validates first.**
//!    [`MigrationView::confirmable`] refuses a report whose plan carries
//!    nothing (never "a blank profile called imported"), a report in which
//!    every blueprint was rejected or refused by the stock rules, and a
//!    report the producer refused. Only then does [`ImportFlow::confirm`]
//!    produce a [`ConfirmedImport`], which retains the source fingerprint,
//!    the migration report, the blueprint verdicts and the exact record
//!    indices a persistence layer may carry ([`ConfirmedImport::importable_records`])
//!    — the ones that are both planned and stock-conforming.
//!
//! # Designed, synthetic
//!
//! The fixture path is only reachable when the caller names
//! [`LayoutAdmission::AllowDesignedFixtures`], which the production default
//! ([`LayoutAdmission::MeasuredOnly`]) does not; the resulting view is marked
//! [`MigrationView::fixture_admitted`] so a fixture report can never present
//! itself as a measured import. Nothing here is `verified_original`: no
//! legacy save or custom-plane file ships with the installation (F64-B's
//! retail measurement), so no original byte layout exists to import yet, and
//! the retail test in `tests/accept_f64_c_import_report.rs` pins that every
//! file this installation *can* offer is refused by name and writes nothing.

use std::fmt;

use cs_content::catalog::Catalog;
use cs_content::construction::{
    ConstraintViolation, ConstructionPolicy, ConstructionRules, LimitBreach, PriceBook,
};
use cs_content::legacy_import::{
    BlueprintFieldMap, BlueprintImportRefusal, BlueprintImportReport, BlueprintImportRequest,
    BlueprintRecordOutcome, BlueprintRecordRefusal, ImportClass, ImportPlan, ImportRefusal,
    ImportRequest, LayoutAdmission, LegacyIdMap, MigrationReport, SourceFingerprint, TargetProfile,
    UnresolvedReason, assess_imported_blueprints, plan_import,
};
use cs_formats::legacy_profile::{
    ArtifactProposal, ImportRequirement, LegacyArtifactClass, LegacyIdClass, LegacyLayout,
    LegacyLimits, LegacyProfileError, layout_record, read_legacy_profile,
};
use cs_types::content::{ContentId, Origin, Provenance};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::InstallIdentity;

/// What stays put while the import dialog is open.
///
/// The screen builds one of these when it opens and hands it to every
/// [`ImportFlow::offer`] until the dialog closes. Everything here is by shared
/// reference: the context reads content and rules, it never owns them and
/// never mutates them, so offering a source cannot change a catalog row, a
/// construction rule or a price.
#[derive(Clone, Debug)]
pub struct ImportContext<'a> {
    /// The declared legacy-id to content-identity table.
    pub ids: &'a LegacyIdMap,
    /// The catalog identities are resolved against.
    pub catalog: &'a Catalog,
    /// The stock rules every imported blueprint is judged by.
    pub rules: &'a ConstructionRules,
    /// The host policy every imported blueprint is judged by.
    pub policy: &'a ConstructionPolicy,
    /// The component prices the budgets are measured against.
    pub book: &'a PriceBook,
    /// Which layout evidence an offer may be read through.
    ///
    /// Production callers leave this at [`LayoutAdmission::MeasuredOnly`];
    /// naming [`LayoutAdmission::AllowDesignedFixtures`] is how the synthetic
    /// acceptance tests reach the fixture layout, and every view produced
    /// under it is marked [`MigrationView::fixture_admitted`].
    pub admission: LayoutAdmission,
    /// Whether the optional legacy-save enhancement is switched on.
    ///
    /// AC04: with this off, an optional save class is refused by name while
    /// the required classes and a fresh profile are unaffected.
    pub legacy_save_import_enabled: bool,
    /// The origin every assembled blueprint is stamped with.
    pub origin: Origin,
    /// The provenance every assembled blueprint carries.
    pub provenance: Provenance,
}

/// One attempt: what the player chose to import, and from what.
///
/// The layout is an [`Option`] because *having a layout for the class is a
/// capability, not a detail*: while no byte layout for any class has been
/// measured (`cs_formats::legacy_profile::LEGACY_LAYOUT_INVENTORY` is
/// `Unknown` on every row), the screen has nothing honest to pass and the
/// offer is refused by name before a single byte is judged. The fingerprint
/// fields are the caller's **declared** values; `plan_import` verifies them
/// against `bytes` before any report describes them.
pub struct ImportOffer<'a> {
    /// The candidate source, with its declared size, digest and class.
    pub source: &'a ArtifactProposal,
    /// The legacy bytes, borrowed for the duration of the attempt.
    pub bytes: &'a [u8],
    /// The layout the screen found for the offered class, when it found one.
    pub layout: Option<&'a LegacyLayout>,
    /// The bounds every read must respect.
    pub limits: LegacyLimits,
    /// The declared field-to-blueprint map, when the class carries blueprint
    /// roles. [`None`] runs the plan alone — a class with no measured
    /// blueprint roles has no map, and inventing one would be a guess.
    pub field_map: Option<&'a BlueprintFieldMap>,
    /// The **new** profile the import would land in.
    pub target: &'a TargetProfile,
    /// The installation identity the source was found under, when known.
    pub install_identity: Option<InstallIdentity>,
}

/// What the screen shows after an attempt: a report, or a propagated refusal.
///
/// `PartialEq` but not `Eq`: the blueprint report inside it is measured data
/// whose equality the producer defines.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportOutcome {
    /// The source read cleanly enough to be described. Boxed because a
    /// rendered report is large and the flow passes outcomes around by value.
    Presented(Box<MigrationView>),
    /// The attempt was refused; the structured reason is retained whole.
    Refused(RefusedView),
}

impl ImportOutcome {
    /// The attempt this outcome belongs to.
    #[must_use]
    pub fn attempt(&self) -> u64 {
        match self {
            Self::Presented(view) => view.attempt(),
            Self::Refused(refused) => refused.attempt(),
        }
    }

    /// The migration report, when the attempt produced one.
    #[must_use]
    pub fn view(&self) -> Option<&MigrationView> {
        match self {
            Self::Presented(view) => Some(view.as_ref()),
            Self::Refused(_) => None,
        }
    }

    /// The propagated refusal, when the attempt was refused.
    #[must_use]
    pub fn refusal(&self) -> Option<&RefusedView> {
        match self {
            Self::Presented(_) => None,
            Self::Refused(refused) => Some(refused),
        }
    }

    /// The stable code of whatever this attempt produced: the verdict label
    /// for a report, the refusal's code otherwise.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Presented(view) => view.verdict().label(),
            Self::Refused(refused) => refused.code(),
        }
    }
}

/// One user-facing line of a migration report.
///
/// Every line has a stable machine [`Self::code`] so a screen can style,
/// count and test a line without parsing its prose, and a [`Display`] that
/// says the same thing in words. The typed variants keep the fields a report
/// must not lose: the resolved identities, the exact [`LimitBreach`] pairs
/// and the broken [`ConstraintViolation`]s are carried as values, never as a
/// formatted approximation of them.
#[derive(Clone, Debug, PartialEq)]
pub enum ReportLine {
    /// The source the report was made from, with the **verified** fingerprint.
    Source {
        /// The relative spelling, exactly as inventoried.
        spelling: String,
        /// The size of the bytes the report was made from.
        size_bytes: u64,
        /// The SHA-256 of those bytes, as the inventory declared it and the
        /// planner verified it.
        sha256: ContentHash,
    },
    /// Which class was offered and whether importing it is required.
    Class {
        /// The artifact class the proposal declared.
        class: LegacyArtifactClass,
        /// The inventory's requirement for that class.
        requirement: ImportRequirement,
    },
    /// What the import-surface inventory row for the class knows — `Unknown`
    /// for every class until an original file is measured.
    InventoryEvidence {
        /// The artifact class.
        class: LegacyArtifactClass,
        /// That row's evidence state.
        evidence: ClaimStatus,
    },
    /// The layout the document was read through, and its evidence.
    Layout {
        /// The layout's label.
        id: String,
        /// The evidence state recorded at read time.
        evidence: ClaimStatus,
    },
    /// The document's declared version.
    Version {
        /// Version major.
        major: u32,
        /// Version minor.
        minor: u32,
    },
    /// The overall verdict.
    Verdict {
        /// Full, partial or unsupported. (A refused attempt has no report.)
        verdict: MigrationVerdict,
    },
    /// One imported record and the content identities its id slots resolved
    /// to — in layout declaration order, never in catalog order.
    Record {
        /// The record's index in the legacy document.
        index: u32,
        /// The identities, paired with the id class that declared each slot.
        identities: Vec<(LegacyIdClass, ContentId)>,
    },
    /// One unresolved piece of the document: a stable reason code and the
    /// reason verbatim (spec F64 non-negotiable 2).
    Unresolved {
        /// [`UnresolvedReason::code`] of the row this line is about.
        code: &'static str,
        /// The row, rendered as the reader reports it.
        detail: String,
    },
    /// One blueprint the stock rules accept.
    BlueprintOk {
        /// The record's index in the legacy document.
        index: u32,
        /// The blueprint identity that record assembled into.
        blueprint: ContentId,
    },
    /// One blueprint the stock rules reject, with a summary line before its
    /// breach lines (spec sheet AC02: rejected **with specific fields**).
    BlueprintRejected {
        /// The record's index in the legacy document.
        index: u32,
    },
    /// One limit the blueprint is over, carried as the measured value pair.
    Breach {
        /// The record's index in the legacy document.
        index: u32,
        /// The breach: `limit` and `total`/`used` in their own units.
        breach: LimitBreach,
    },
    /// One host constraint the blueprint breaks.
    Constraint {
        /// The record's index in the legacy document.
        index: u32,
        /// The broken rule.
        violation: ConstraintViolation,
    },
    /// One blueprint that could not be judged at all, with the refusal's own
    /// code and message (an unjudged blueprint is never presented as OK).
    BlueprintRefused {
        /// The record's index in the legacy document.
        index: u32,
        /// Which kind of refusal [`BlueprintRecordRefusal`] is.
        code: &'static str,
        /// The refusal, rendered verbatim.
        detail: String,
    },
    /// Something the screen must show alongside the verdict.
    Notice {
        /// The notice's stable code (for example `fixture_admitted`).
        code: &'static str,
        /// What the notice says.
        detail: String,
    },
    /// The attempt was refused; this is the propagated reason.
    Refusal {
        /// [`FlowRefusal::code`] of the refusal.
        code: &'static str,
        /// The refusal, rendered verbatim.
        detail: String,
    },
}

impl ReportLine {
    /// The stable machine code of this line.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Source { .. } => "source",
            Self::Class { .. } => "class",
            Self::InventoryEvidence { .. } => "inventory_evidence",
            Self::Layout { .. } => "layout",
            Self::Version { .. } => "version",
            Self::Verdict { .. } => "verdict",
            Self::Record { .. } => "record",
            Self::Unresolved { code, .. } => code,
            Self::BlueprintOk { .. } => "blueprint_ok",
            Self::BlueprintRejected { .. } => "blueprint_rejected",
            Self::Breach { .. } => "breach",
            Self::Constraint { .. } => "constraint",
            Self::BlueprintRefused { code, .. } => code,
            Self::Notice { code, .. } => code,
            Self::Refusal { code, .. } => code,
        }
    }
}

impl fmt::Display for ReportLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source {
                spelling,
                size_bytes,
                sha256,
            } => write!(f, "source {spelling}: {size_bytes} bytes, sha256 {sha256}"),
            Self::Class { class, requirement } => {
                write!(f, "{class}: {}", requirement_text(*requirement))
            }
            Self::InventoryEvidence { class, evidence } => {
                write!(f, "the inventory row for {class} is {evidence} evidence")
            }
            Self::Layout { id, evidence } => {
                write!(f, "read through layout {id} ({evidence} evidence)")
            }
            Self::Version { major, minor } => write!(f, "document version {major}.{minor}"),
            Self::Verdict { verdict } => write!(f, "verdict: {}", verdict.label()),
            Self::Record { index, identities } => {
                write!(f, "record {index} carries ")?;
                for (position, (class, id)) in identities.iter().enumerate() {
                    if position > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{} {id}", class.label())?;
                }
                Ok(())
            }
            Self::Unresolved { detail, .. } => f.write_str(detail),
            Self::BlueprintOk { index, blueprint } => {
                write!(
                    f,
                    "record {index} blueprint {blueprint} is inside every limit"
                )
            }
            Self::BlueprintRejected { index } => write!(
                f,
                "record {index} is rejected by the stock construction rules"
            ),
            Self::Breach { index, breach } => write!(f, "record {index}: {breach}"),
            Self::Constraint { index, violation } => {
                write!(f, "record {index}: {violation}")
            }
            Self::BlueprintRefused { index, detail, .. } => {
                write!(f, "record {index} could not be judged: {detail}")
            }
            Self::Notice { detail, .. } | Self::Refusal { detail, .. } => f.write_str(detail),
        }
    }
}

/// How much of the source the report says would be imported.
///
/// The three labels of spec F64 non-negotiable 5, plus the refusal of the
/// whole attempt. "Refused" is not a fourth import class: it means no report
/// exists at all, which the screen must not confuse with an unsupported
/// document that *was* read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationVerdict {
    /// Every record resolved and no byte was left over.
    Full,
    /// Some records resolved; the rest are named row by row.
    Partial,
    /// Nothing would be carried, and the reason is named.
    Unsupported,
    /// The attempt was refused before any report could exist.
    Refused,
}

impl MigrationVerdict {
    /// The stable label used in reports and by [`ImportOutcome::code`].
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "full import",
            Self::Partial => "partial import",
            Self::Unsupported => "unsupported",
            Self::Refused => "refused",
        }
    }

    /// Derives the verdict from the producer's import class.
    #[must_use]
    pub fn of(class: &ImportClass) -> Self {
        match class {
            ImportClass::Full => Self::Full,
            ImportClass::Partial { .. } => Self::Partial,
            ImportClass::Unsupported { .. } => Self::Unsupported,
        }
    }
}

impl fmt::Display for MigrationVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The migration report one attempt produced, ready to show.
///
/// A view is a value: it retains the verified [`SourceFingerprint`], the
/// producer's [`MigrationReport`], the optional [`BlueprintImportReport`] and
/// the rendered [`ReportLine`]s, so a screen can draw it, a log can replay it
/// and a later persistence layer can store it without re-deriving anything.
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationView {
    attempt: u64,
    plan: ImportPlan,
    blueprints: Option<BlueprintImportReport>,
    lines: Vec<ReportLine>,
    importable: Vec<u32>,
    fixture_admitted: bool,
}

impl MigrationView {
    fn new(attempt: u64, plan: ImportPlan, blueprints: Option<BlueprintImportReport>) -> Self {
        let fixture_admitted = plan.admitted_designed_layout()
            || blueprints.as_ref().is_some_and(|report| {
                report.admitted_designed_layout() || report.admitted_designed_map()
            });
        let importable = importable_records(&plan, blueprints.as_ref());
        let lines = lines_for(&plan, blueprints.as_ref(), fixture_admitted);
        Self {
            attempt,
            plan,
            blueprints,
            lines,
            importable,
            fixture_admitted,
        }
    }

    /// The attempt this report belongs to.
    #[must_use]
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }

    /// The producer's plan, whose [`Self::report`] is retained whole.
    #[must_use]
    pub fn plan(&self) -> &ImportPlan {
        &self.plan
    }

    /// The retained migration report (source fingerprint, class, records).
    #[must_use]
    pub fn report(&self) -> &MigrationReport {
        self.plan.report()
    }

    /// The verified source this report describes.
    #[must_use]
    pub fn source(&self) -> &SourceFingerprint {
        self.plan.report().source()
    }

    /// The blueprint verdicts, when the offer declared blueprint roles.
    #[must_use]
    pub fn blueprints(&self) -> Option<&BlueprintImportReport> {
        self.blueprints.as_ref()
    }

    /// The rendered report, in display order.
    #[must_use]
    pub fn lines(&self) -> &[ReportLine] {
        &self.lines
    }

    /// The overall verdict (never [`MigrationVerdict::Refused`]: a refused
    /// attempt has no view).
    #[must_use]
    pub fn verdict(&self) -> MigrationVerdict {
        MigrationVerdict::of(self.plan.report().class())
    }

    /// The records a persistence layer may actually carry: planned **and**,
    /// when the offer declared blueprint roles, stock-conforming. A rejected
    /// or refused blueprint is never in this list, so "what the report shows"
    /// and "what an import would carry" cannot drift apart.
    #[must_use]
    pub fn importable_records(&self) -> &[u32] {
        &self.importable
    }

    /// Whether this report could only be produced through
    /// [`LayoutAdmission::AllowDesignedFixtures`].
    ///
    /// A screen must label such a report as fixture data; it can never be
    /// presented as a measured import of an original file.
    #[must_use]
    pub const fn fixture_admitted(&self) -> bool {
        self.fixture_admitted
    }

    /// Whether the explicit owner action may be taken on this report.
    ///
    /// # Errors
    ///
    /// [`ConfirmError::Unsupported`] when the plan carries nothing (confirming
    /// would create the blank profile non-negotiable 5 forbids),
    /// [`ConfirmError::NoConformingRecord`] when no record is both planned and
    /// stock-conforming (confirming would import blueprints the stock rules
    /// reject, which non-negotiable 4 forbids).
    pub fn confirmable(&self) -> Result<(), ConfirmError> {
        if let ImportClass::Unsupported { reason } = self.plan.report().class() {
            return Err(ConfirmError::Unsupported {
                reason: reason.clone(),
            });
        }
        if self.importable.is_empty() {
            return Err(ConfirmError::NoConformingRecord);
        }
        Ok(())
    }
}

/// Why the explicit owner action was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmError {
    /// The dialog has no attempt waiting for a decision (it is idle, or the
    /// decision has already been taken).
    NothingToConfirm,
    /// The attempt was refused, so there is no report to act on.
    Refused {
        /// [`FlowRefusal::code`] of the refusal that ended the attempt.
        code: &'static str,
    },
    /// The plan itself reports that nothing would be carried.
    Unsupported {
        /// The producer's reason.
        reason: UnresolvedReason,
    },
    /// No record is both planned and conforming, so confirming would carry an
    /// empty or rule-breaking set of blueprints.
    NoConformingRecord,
}

impl fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingToConfirm => write!(f, "there is no import waiting to be confirmed"),
            Self::Refused { code } => {
                write!(
                    f,
                    "the attempt was refused ({code}), so there is nothing to confirm"
                )
            }
            Self::Unsupported { reason } => {
                write!(f, "the import would carry nothing: {reason}")
            }
            Self::NoConformingRecord => write!(
                f,
                "no record is both planned and inside the stock rules, so nothing \
                 may be imported"
            ),
        }
    }
}

impl std::error::Error for ConfirmError {}

/// What a refused attempt leaves on screen: the reason, kept whole.
///
/// The refusal is *not* translated into a partial report and never into a
/// success: [`Self::lines`] carries one [`ReportLine::Refusal`] with the
/// original error's code and message plus the notice that nothing was
/// written. The declared spelling is retained so the screen can say which
/// offer was declined; it is **not** a fingerprint, because a refused
/// attempt has no verified source.
#[derive(Clone, Debug, PartialEq)]
pub struct RefusedView {
    attempt: u64,
    spelling: String,
    refusal: FlowRefusal,
    lines: Vec<ReportLine>,
}

impl RefusedView {
    fn new(attempt: u64, spelling: String, refusal: FlowRefusal) -> Self {
        let lines = vec![
            ReportLine::Refusal {
                code: refusal.code(),
                detail: refusal.to_string(),
            },
            ReportLine::Notice {
                code: "nothing_written",
                detail: "the offer was declined: no profile was created and no file was \
                         written, the source included"
                    .to_owned(),
            },
        ];
        Self {
            attempt,
            spelling,
            refusal,
            lines,
        }
    }

    /// The attempt this refusal belongs to.
    #[must_use]
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }

    /// The **declared** spelling of the offer that was declined.
    ///
    /// Declared, not verified: nothing was read far enough to check it.
    #[must_use]
    pub fn source_spelling(&self) -> &str {
        &self.spelling
    }

    /// The structured refusal, with its original error retained.
    #[must_use]
    pub fn refusal(&self) -> &FlowRefusal {
        &self.refusal
    }

    /// The refusal's stable code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.refusal.code()
    }

    /// What the screen shows: one refusal line and the nothing-was-written
    /// notice.
    #[must_use]
    pub fn lines(&self) -> &[ReportLine] {
        &self.lines
    }
}

/// Why an offer was declined, with the original error retained whole.
///
/// Every variant propagates a real producer refusal rather than re-deriving
/// one, so the code a screen shows (`source_too_large`, `layout_evidence`,
/// `enhancement_disabled`, `unreadable`, ...) is the code the producer
/// stamped on the condition.
#[derive(Clone, Debug, PartialEq)]
pub enum FlowRefusal {
    /// This build has measured no layout for the class, so no byte of it can
    /// be read yet. Refused before any byte is judged, and the inventory row
    /// says why.
    NoMeasuredLayout {
        /// The class the offer declared.
        class: LegacyArtifactClass,
        /// That class's inventory-row evidence (`Unknown` today).
        inventory_evidence: ClaimStatus,
    },
    /// `plan_import` refused: the hostile source, the fixture layout, the
    /// switched-off enhancement or the unreadable document.
    Plan(ImportRefusal),
    /// The document could not be re-read for the blueprint stage.
    Read(LegacyProfileError),
    /// The blueprint assessment refused: a field map that cannot describe the
    /// layout, or evidence the admission does not allow.
    Blueprint(BlueprintImportRefusal),
}

impl FlowRefusal {
    /// The stable lowercase identifier for reports and logs.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoMeasuredLayout { .. } => "no_measured_layout",
            Self::Plan(refusal) => refusal.code(),
            Self::Read(_) => "unreadable",
            Self::Blueprint(refusal) => match refusal {
                BlueprintImportRefusal::LayoutMismatch { .. } => "layout_mismatch",
                BlueprintImportRefusal::LayoutEvidence { .. } => "layout_evidence",
                BlueprintImportRefusal::MapEvidence { .. } => "map_evidence",
                BlueprintImportRefusal::Map(_) => "map_invalid",
            },
        }
    }
}

impl fmt::Display for FlowRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMeasuredLayout {
                class,
                inventory_evidence,
            } => write!(
                f,
                "no measured layout for {class}: its inventory row is {inventory_evidence} \
                 evidence, so no file of this class can be read yet"
            ),
            Self::Plan(refusal) => write!(f, "{refusal}"),
            Self::Read(error) => write!(f, "the source could not be read: {error}"),
            Self::Blueprint(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for FlowRefusal {}

/// The import dialog's state machine.
///
/// One flow serves one open dialog. [`Self::offer`] is the only way out of
/// [`Self::is_idle`], and it always tears the previous attempt down first, so
/// a report, a refusal or a confirmation can never be read after the attempt
/// that produced it has been superseded (STATE-TRANSACTIONS: retry restores
/// the authored initial state, not a mutated copy of the just-failed world).
#[derive(Clone, Debug, Default)]
pub struct ImportFlow {
    attempt: u64,
    stage: Stage,
}

#[derive(Clone, Debug, Default, PartialEq)]
enum Stage {
    /// Nothing has been offered in this dialog, or the last attempt was
    /// dismissed.
    #[default]
    Idle,
    /// The last attempt produced a report or a refusal.
    Attempted(ImportOutcome),
    /// The owner confirmed the last attempt's report.
    Confirmed(Box<ConfirmedImport>),
}

impl ImportFlow {
    /// A dialog that has not been offered anything yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many attempts this dialog has made, including refused ones.
    ///
    /// The counter is monotone for the life of the flow: teardown drops an
    /// attempt's *data*, never the record that the attempt happened, so a log
    /// line "attempt 3 refused (source_too_large)" still names the attempt it
    /// came from after the screen moved on.
    #[must_use]
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }

    /// Whether the dialog is waiting for an offer.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        matches!(self.stage, Stage::Idle)
    }

    /// The outcome of the current attempt, when there is one.
    #[must_use]
    pub fn outcome(&self) -> Option<&ImportOutcome> {
        match &self.stage {
            Stage::Idle => None,
            Stage::Attempted(outcome) => Some(outcome),
            Stage::Confirmed(_) => None,
        }
    }

    /// The migration report of the current attempt, when it produced one.
    #[must_use]
    pub fn view(&self) -> Option<&MigrationView> {
        self.outcome().and_then(ImportOutcome::view)
    }

    /// The propagated refusal of the current attempt, when it was refused.
    #[must_use]
    pub fn refusal_view(&self) -> Option<&RefusedView> {
        self.outcome().and_then(ImportOutcome::refusal)
    }

    /// The confirmed import, once the owner has taken the explicit action.
    #[must_use]
    pub fn confirmed(&self) -> Option<&ConfirmedImport> {
        match &self.stage {
            Stage::Confirmed(confirmed) => Some(confirmed),
            _ => None,
        }
    }

    /// Runs one attempt end to end and stores its outcome.
    ///
    /// Any previous attempt — report, refusal or confirmation — is dropped
    /// before this one starts, so what the caller reads back describes this
    /// offer and nothing else.
    ///
    /// # Errors
    ///
    /// Never: the refusal *is* the result. Read it back through
    /// [`Self::refusal_view`] (or match on [`Self::outcome`]); the structured
    /// [`FlowRefusal`] keeps the producer's original error so no caller has
    /// to re-derive why an offer was declined.
    pub fn offer(
        &mut self,
        context: &ImportContext<'_>,
        offer: &ImportOffer<'_>,
    ) -> &ImportOutcome {
        self.attempt = self.attempt.saturating_add(1);
        // Teardown first: an attempt's data never survives its successor.
        self.stage = Stage::Idle;
        let outcome = run_attempt(self.attempt, context, offer);
        self.stage = Stage::Attempted(outcome);
        self.outcome()
            .expect("the attempt was just stored, so there is an outcome")
    }

    /// Drops the current attempt without leaving a trace of it.
    ///
    /// This is the dialog's teardown: after it the flow reports
    /// [`Self::is_idle`], and no line, verdict or confirmation of the
    /// dismissed attempt can be read any more. The attempt counter is kept.
    pub fn dismiss(&mut self) {
        self.stage = Stage::Idle;
    }

    /// Re-runs the whole pipeline from a clean state.
    ///
    /// Teardown plus a fresh [`Self::offer`]: the retry reads the bytes
    /// again, plans again and renders again, so its outcome cannot inherit
    /// anything — a line, an unresolved row, a partial verdict — from the
    /// attempt it replaces.
    pub fn retry(
        &mut self,
        context: &ImportContext<'_>,
        offer: &ImportOffer<'_>,
    ) -> &ImportOutcome {
        self.dismiss();
        self.offer(context, offer)
    }

    /// The explicit owner action: turn the current report into a confirmed
    /// import a persistence layer may carry.
    ///
    /// # Errors
    ///
    /// [`ConfirmError`] when there is no report to confirm, when the report
    /// was refused, when its plan carries nothing, or when no record is both
    /// planned and stock-conforming. The flow is left exactly as it was, so
    /// the screen can keep showing the report and explain the refusal.
    pub fn confirm(&mut self) -> Result<&ConfirmedImport, ConfirmError> {
        let Stage::Attempted(ImportOutcome::Presented(view)) = &self.stage else {
            return Err(match &self.stage {
                Stage::Attempted(ImportOutcome::Refused(refused)) => ConfirmError::Refused {
                    code: refused.code(),
                },
                _ => ConfirmError::NothingToConfirm,
            });
        };
        view.confirmable()?;
        let confirmed = ConfirmedImport {
            attempt: view.attempt(),
            target: view.plan.target().clone(),
            importable: view.importable.clone(),
            view: view.as_ref().clone(),
        };
        self.stage = Stage::Confirmed(Box::new(confirmed));
        Ok(self.confirmed().expect("the import was just confirmed"))
    }
}

/// The import the owner confirmed: everything a persistence layer needs.
///
/// This value is the *outcome transaction* STATE-TRANSACTIONS asks for. It
/// carries no path and no writer — persisting it (into a **new** profile,
/// atomically, never over the source) belongs to the layer that owns the
/// profile store — so holding one cannot overwrite anything either.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmedImport {
    attempt: u64,
    target: TargetProfile,
    importable: Vec<u32>,
    view: MigrationView,
}

impl ConfirmedImport {
    /// The attempt that produced this import.
    #[must_use]
    pub const fn attempt(&self) -> u64 {
        self.attempt
    }

    /// The new profile the import lands in — never an existing one.
    #[must_use]
    pub fn target(&self) -> &TargetProfile {
        &self.target
    }

    /// The report the owner confirmed, retained whole.
    #[must_use]
    pub fn view(&self) -> &MigrationView {
        &self.view
    }

    /// The retained migration report (source fingerprint included).
    #[must_use]
    pub fn report(&self) -> &MigrationReport {
        self.view.report()
    }

    /// The verified source this import came from.
    #[must_use]
    pub fn source(&self) -> &SourceFingerprint {
        self.view.source()
    }

    /// The blueprint verdicts, when the offer declared blueprint roles.
    #[must_use]
    pub fn blueprints(&self) -> Option<&BlueprintImportReport> {
        self.view.blueprints()
    }

    /// The rendered report, exactly as the owner saw it.
    #[must_use]
    pub fn lines(&self) -> &[ReportLine] {
        self.view.lines()
    }

    /// The record indices a persistence layer may carry.
    ///
    /// Planned **and** stock-conforming: a record the stock rules rejected is
    /// never in this list, whatever its plan said.
    #[must_use]
    pub fn importable_records(&self) -> &[u32] {
        &self.importable
    }

    /// Whether the confirmed report could only be produced from fixture data.
    #[must_use]
    pub const fn fixture_admitted(&self) -> bool {
        self.view.fixture_admitted()
    }
}

/// Runs one attempt: plan, optional blueprint assessment, then a report or a
/// propagated refusal.
fn run_attempt(
    attempt: u64,
    context: &ImportContext<'_>,
    offer: &ImportOffer<'_>,
) -> ImportOutcome {
    let spelling = offer.source.spelling().as_str().to_owned();
    let refuse = |refusal: FlowRefusal| {
        ImportOutcome::Refused(RefusedView::new(attempt, spelling.clone(), refusal))
    };

    // The class is the proposal's own declaration: it is never inferred from
    // the file name, and an undeclared one is the producer's refusal.
    let class = match offer.source.require_class() {
        Ok(class) => class,
        Err(error) => return refuse(FlowRefusal::Plan(ImportRefusal::Source(error))),
    };

    // A layout is a capability, not a detail: without one for this class the
    // offer is declined by name before a single byte is judged. Nothing about
    // the file itself is claimed or read here.
    let Some(layout) = offer.layout else {
        return refuse(FlowRefusal::NoMeasuredLayout {
            class,
            inventory_evidence: layout_record(class).evidence,
        });
    };

    let plan = match plan_import(&ImportRequest {
        source: offer.source,
        bytes: offer.bytes,
        layout,
        limits: offer.limits,
        ids: context.ids,
        catalog: context.catalog,
        target: offer.target,
        admission: context.admission,
        legacy_save_import_enabled: context.legacy_save_import_enabled,
        install_identity: offer.install_identity.clone(),
    }) {
        Ok(plan) => plan,
        Err(refusal) => return refuse(FlowRefusal::Plan(refusal)),
    };

    let blueprints = if let Some(field_map) = offer.field_map {
        // The blueprint stage judges the *same* bytes through the *same*
        // layout the plan just read, so a second read is deterministic; the
        // reader's own refusal propagates rather than being swallowed.
        let document = match read_legacy_profile(offer.bytes, layout, &offer.limits) {
            Ok(document) => document,
            Err(error) => return refuse(FlowRefusal::Read(error)),
        };
        match assess_imported_blueprints(&BlueprintImportRequest {
            document: &document,
            layout,
            field_map,
            ids: context.ids,
            catalog: context.catalog,
            rules: context.rules,
            policy: context.policy,
            book: context.book,
            origin: context.origin.clone(),
            provenance: context.provenance.clone(),
            admission: context.admission,
        }) {
            Ok(report) => Some(report),
            Err(refusal) => return refuse(FlowRefusal::Blueprint(refusal)),
        }
    } else {
        None
    };

    ImportOutcome::Presented(Box::new(MigrationView::new(attempt, plan, blueprints)))
}

/// The records a plan and (when present) the blueprint verdicts agree on.
///
/// The plan decides what *resolved*; the stock rules decide what *fits*. Only
/// their intersection may be carried, so neither report alone can authorise
/// a record.
fn importable_records(plan: &ImportPlan, blueprints: Option<&BlueprintImportReport>) -> Vec<u32> {
    let planned: Vec<u32> = plan
        .report()
        .records()
        .iter()
        .map(|record| record.record_index)
        .collect();
    let Some(blueprints) = blueprints else {
        return planned;
    };
    planned
        .into_iter()
        .filter(|index| {
            blueprints.records().iter().any(|row| {
                row.record_index() == *index
                    && matches!(row.outcome(), BlueprintRecordOutcome::Conforming { .. })
            })
        })
        .collect()
}

/// Renders one plan (and its optional blueprint verdicts) as report lines.
fn lines_for(
    plan: &ImportPlan,
    blueprints: Option<&BlueprintImportReport>,
    fixture_admitted: bool,
) -> Vec<ReportLine> {
    let report = plan.report();
    let mut lines = vec![
        ReportLine::Source {
            spelling: report.source().spelling().to_owned(),
            size_bytes: report.source().size_bytes(),
            sha256: *report.source().sha256(),
        },
        ReportLine::Class {
            class: plan.class(),
            requirement: plan.requirement(),
        },
        ReportLine::InventoryEvidence {
            class: plan.class(),
            evidence: layout_record(plan.class()).evidence,
        },
        ReportLine::Layout {
            id: report.layout_id().to_owned(),
            evidence: report.layout_evidence(),
        },
        ReportLine::Version {
            major: report.version_major(),
            minor: report.version_minor(),
        },
        ReportLine::Verdict {
            verdict: MigrationVerdict::of(report.class()),
        },
    ];

    for record in report.records() {
        lines.push(ReportLine::Record {
            index: record.record_index,
            identities: record.resolved_ids.clone(),
        });
    }

    if let ImportClass::Partial { unresolved, .. } = report.class() {
        for row in unresolved {
            lines.push(ReportLine::Unresolved {
                code: row.reason.code(),
                detail: row.to_string(),
            });
        }
    }

    if let Some(blueprints) = blueprints {
        for record in blueprints.records() {
            let index = record.record_index();
            match record.outcome() {
                BlueprintRecordOutcome::Conforming { verdict, .. } => {
                    lines.push(ReportLine::BlueprintOk {
                        index,
                        blueprint: verdict.assessment().blueprint().clone(),
                    });
                }
                BlueprintRecordOutcome::Rejected { verdict, .. } => {
                    lines.push(ReportLine::BlueprintRejected { index });
                    for breach in verdict.assessment().breaches() {
                        lines.push(ReportLine::Breach {
                            index,
                            breach: *breach,
                        });
                    }
                    for violation in verdict.violations() {
                        lines.push(ReportLine::Constraint {
                            index,
                            violation: violation.clone(),
                        });
                    }
                }
                BlueprintRecordOutcome::Refused { reason } => {
                    lines.push(ReportLine::BlueprintRefused {
                        index,
                        code: blueprint_refusal_code(reason),
                        detail: reason.to_string(),
                    });
                }
            }
        }
    }

    if fixture_admitted {
        lines.push(ReportLine::Notice {
            code: "fixture_admitted",
            detail: "this report was produced from designed fixture data, never from an \
                     original file"
                .to_owned(),
        });
    }

    lines
}

/// The stable code of one per-record blueprint refusal.
const fn blueprint_refusal_code(reason: &BlueprintRecordRefusal) -> &'static str {
    match reason {
        BlueprintRecordRefusal::Unresolved(_) => "blueprint_unresolved",
        BlueprintRecordRefusal::Schema(_) => "blueprint_schema",
        BlueprintRecordRefusal::Identity(_) => "blueprint_identity",
        BlueprintRecordRefusal::Validation(_) => "blueprint_validation",
    }
}

/// How the inventory words one class's requirement.
fn requirement_text(requirement: ImportRequirement) -> String {
    match requirement {
        ImportRequirement::RequiredWhenReferenced { referenced_by: [] } => {
            "required when an original content path references it, none does yet".to_owned()
        }
        ImportRequirement::RequiredWhenReferenced { referenced_by } => {
            format!("required, referenced by {}", referenced_by.join(", "))
        }
        ImportRequirement::OptionalEnhancement {
            label,
            disable_switch,
        } => format!("optional enhancement {label}, refused while {disable_switch} is off"),
    }
}
