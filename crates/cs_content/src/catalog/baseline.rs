//! The complete private baseline inventory and its coverage denominator
//! (F14-D).
//!
//! Spec F14 non-negotiable behavior 4 ("Never filter unsupported missions out
//! and divide successes by the smaller list. The declared baseline inventory
//! fixes the denominator") needs a denominator that comes from the owner's
//! installation, not from an authored fixture. [`retail_baseline`] reads one
//! installation once and builds that inventory:
//!
//! * one [`ContentKind::InstallFile`] row per inventoried regular file, so the
//!   inventory covers **every** file the installation holds — an unparsed or
//!   failed file is still a row (`IDENTITY-CONTENT`: "An opaque unparsed
//!   member is still an inventory row");
//! * one [`ContentKind::Script`] row per campaign mission reader archive, using
//!   the `script/<world>-m<nn>-zrdr` identity the published mission bindings
//!   (`missions/bindings/M01.json`) already carry;
//! * one [`ContentKind::Mission`] row per `ZBD/<chapter><variant>/<mission>`
//!   directory the shared campaign walk declares, each **declared launchable**:
//!   those launchable missions are the coverage denominator, and every one of
//!   them is declared even though none of them is ready yet;
//! * one [`ContentKind::IaScenario`] or [`ContentKind::MultiplayerScenario`]
//!   row (plus its [`ContentKind::Script`] row) per instant-action or
//!   multiplayer scenario directory whose reader archive's own member index
//!   classifies it as one (F14-D.1, [`super::reader_dirs`]), also declared
//!   launchable and part of the denominator. The world-group readers and the
//!   top-level reader are classified as not launchable and get no such row; a
//!   reader directory no rule classifies stays in
//!   [`Baseline::unrecognized_program_dirs`], named and uncounted;
//! * one [`ContentKind::MultiplayerRules`] row per multiplayer mode the
//!   installation's string image names (F14-D.2), read by the producing
//!   stage's own parser ([`crate::multiplayer::discover_modes`]) rather than a
//!   reader derived here — including a name the parser could not pair with a
//!   briefing, which stays a row with an explicit unknown instead of being
//!   excluded from the collection. Those rules are **not** launchable, so they
//!   add nothing to the denominator; what the producing parser could not answer
//!   is reported in [`CollectionStatus`] instead of being dropped.
//!
//! Every row's [`Origin`] is [`Origin::Installation`] with a checked
//! [`SourceSpan`] and the installation fingerprint of the bytes that were read,
//! so a row traces back to original data. The symmetric half of spec F14 AC04
//! holds on any catalog this function's rows are mixed with: an authored
//! [`Origin::SyntheticFixture`] launchable row is counted by
//! [`Catalog::synthetic_launchable_count`] and never by
//! [`Catalog::original_launchable_count`], so a synthetic launchable row can
//! never be mistaken for a retail catalog entry.
//!
//! Nothing here is ready and nothing claims to be: no mission program is
//! decoded at this stage (F37/F38 measure those instructions) and no row
//! claims a runtime consumer, so every row carries an explicit
//! [`UnsupportedReason`] and the coverage report says `0` ready instead of a
//! designed green. [`Coverage`] additionally accounts for the rows no declared
//! root reaches, because the contract keeps unreachable unknowns in the global
//! accounting report rather than dropping them.
//!
//! [`retail_baseline`] derives nothing twice: the campaign walk is
//! [`crate::campaign_bindings::campaign_layout`], the same production
//! derivation the per-mission bindings and the `cs-inspect campaign` report
//! use, the file inventory is `cs_assets::install::discover`, and the
//! multiplayer rows are F56-A's [`crate::multiplayer::discover_modes`] over
//! the string rows `cs_content::config::StringCatalog` reads out of
//! [`MODE_STRING_IMAGE`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

use cs_assets::vfs::SessionBuilder;
use cs_assets::zbd::{ContainerVerdict, audit_containers};
use cs_formats::LANG_ENGLISH_US;
use cs_types::asset_id::{AssetKey, ResolveContext, SourceSpan, SourceSpanError};
use cs_types::content::{
    CatalogElement, ContentId, ContentIdError, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, UnsupportedReason,
};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash, Fingerprint, FingerprintKind};
use cs_types::install::InstallFileRecord;

use crate::config::StringCatalog;
use crate::multiplayer::{ModeEntry, TextRef, discover_modes, mode_name_id};

use super::closure::{Closure, ClosureError, CompatibilityOptions, json_string};
use super::reader_dirs::{ClassifiedReaderDir, ReaderDirRole, classify};
use super::{Catalog, CatalogError};

/// The report format version of [`baseline_report_json`].
pub const BASELINE_REPORT_VERSION: &str = "cs-content-baseline/1";

/// The claim id behind the observation that a campaign mission's reader
/// archive is the file its directory names.
const CLAIM_MISSION_PROGRAM: &str = "f14.d.baseline.mission_program";

/// The claim id behind the observation that a scenario directory's reader
/// archive is the file its directory holds (F14-D.1).
const CLAIM_SCENARIO_PROGRAM: &str = "f14.d.1.baseline.scenario_program";

/// The claim id behind the observation that a mission program's bytes are
/// the inventoried install file of the same span.
const CLAIM_INSTALL_FILE: &str = "f14.d.baseline.install_file";

/// The claim id behind the observation that a multiplayer mode's name and
/// briefing live in the inventoried string image its row points at.
const CLAIM_MODE_STRINGS: &str = "f14.d.2.baseline.mode_strings";

/// The claim id behind the observation that a mode name the string table
/// carries pairs with no briefing block of the multiplayer family, so the
/// name cannot be resolved to a mode this engine can read.
const CLAIM_MODE_PAIRING: &str = "f14.d.2.baseline.mode_pairing";

/// The installation-relative spelling of the string image the multiplayer mode
/// table is read from.
///
/// Measured in the original installation and recorded by stage F56-A
/// (`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`): the mode-name
/// run `7011..=7014` and the briefing blocks from `16600` live in
/// `strings.dll`'s `RT_STRING` resources, not in the UI image
/// (`GOSDATA/ASSETS/BINARIES/langui.dll`, which the campaign bindings read).
/// The spelling is the installation's own and is compared case-insensitively,
/// so a differently-cased install still resolves.
pub const MODE_STRING_IMAGE: &str = "strings.dll";

/// The language id the multiplayer mode table is read in.
///
/// Every surveyed image records `LANG_ENGLISH_US` (`0x0409`) for its
/// third-level resource entries (`cs_formats::pe_resources`), and F56-A
/// measured that this installation carries that language only; another
/// localization could carry a different run, which is why the language is
/// passed in rather than searched for. An image without this language yields a
/// named [`CollectionStatus::diagnostic`] instead of rows.
pub const MODE_STRING_LANGUAGE: u32 = LANG_ENGLISH_US;

/// Encodes one installation-relative spelling into a `ContentId` key.
///
/// The id grammar accepts only ASCII lowercase alphanumerics plus `.`, `_`
/// and `-` (`cs_types::content`), so a path separator cannot appear in a key
/// and identity can never be joined back into a path. This encoding folds the
/// spelling to lowercase — the installation inventory is case-insensitively
/// unique by construction, so folding never merges two files — and escapes
/// every remaining byte as `_` + two lowercase hex digits + `_`, which is
/// injective because `_` itself is escaped and a bare `_` therefore only ever
/// opens an escape.
///
/// ```text
/// ZBD/C1C/M01/zrdr.zbd -> zbd_2f_c1c_2f_m01_2f_zrdr.zbd
/// mis_anim.zbd         -> mis_5f_anim.zbd
/// ```
///
/// The original spelling stays outside identity as the row's
/// `display_name`. A key that still exceeds [`MAX_CONTENT_KEY_LEN`] is not
/// shortened: [`retail_baseline`] refuses the file by name instead.
///
/// [`MAX_CONTENT_KEY_LEN`]: cs_types::content::MAX_CONTENT_KEY_LEN
pub fn install_file_key(spelling: &str) -> String {
    let mut key = String::with_capacity(spelling.len());
    for byte in spelling.bytes() {
        let folded = byte.to_ascii_lowercase();
        match folded {
            b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' => key.push(folded as char),
            other => {
                let _ = write!(key, "_{other:02x}_");
            }
        }
    }
    key
}

/// One campaign mission's mission id key: the identity the published mission
/// bindings use (`mission/ch1-m01` in `missions/bindings/M01.json`).
///
/// `cs_content::campaign_bindings` derives the same key privately for
/// `SourceBinding::catalog_id`; the retail acceptance test cross-checks this
/// derivation against that published binding record so the two cannot drift.
fn mission_key(chapter: u32, mission_number: u32) -> String {
    format!("ch{chapter}-m{mission_number:02}")
}

/// One campaign mission's program id key: the reader archive of the world
/// group the mission is stored in (`script/c1c-m01-zrdr` in
/// `missions/bindings/M01.json`).
fn program_key(world_group: &str, mission_number: u32) -> String {
    format!("{world_group}-m{mission_number:02}-zrdr")
}

/// How far one inventoried file got, restated as an unsupported reason.
///
/// Parsing, normalization and readiness are separate states (F14
/// non-negotiable behavior 1): a file that was never parsed says so, a parsed
/// file that nothing normalized says that, and a failed parse keeps its
/// diagnostic.
fn unparsed_reason(state: &cs_types::install::ParseState) -> UnsupportedReason {
    match state {
        cs_types::install::ParseState::Unparsed => UnsupportedReason::NotParsed,
        cs_types::install::ParseState::Parsed => UnsupportedReason::NotNormalized,
        cs_types::install::ParseState::Failed { diagnostic } => UnsupportedReason::ParseFailed {
            diagnostic: diagnostic.clone(),
        },
    }
}

/// Why the baseline inventory could not be built.
///
/// Missing data blocks the baseline (`AGENTS.md` rule 5): a mission the
/// campaign layout declares but whose program archive is absent is an error,
/// never a shorter denominator, and a file whose id cannot be built is named
/// rather than dropped.
#[derive(Debug)]
pub enum BaselineError {
    /// Installation discovery refused the directory.
    Discover(cs_assets::install::DiscoveryError),
    /// The campaign directory layout could not be read.
    Campaign(crate::campaign_bindings::SourceBindingError),
    /// The installation declares a campaign mission whose reader archive is
    /// absent, so its launchable row could not be located in original bytes.
    MissingProgram {
        /// The mission id the row would have had.
        mission: String,
        /// The archive the layout names.
        asset: String,
    },
    /// A present reader archive is not in the inventory (it was skipped as a
    /// symbolic link or a non-regular entry), so no fingerprinted row can be
    /// built for it.
    UninventoriedProgram {
        /// The mission id the row would have had.
        mission: String,
        /// The archive the layout names.
        asset: String,
    },
    /// An inventoried file could not be read. Installation discovery hashed
    /// every regular file before this walk, so a read failure here means the
    /// installation is no longer readable as inventoried: a collection's
    /// bytes cannot be located at all, which is never a silently empty
    /// collection.
    Read {
        /// The file that could not be read.
        path: String,
        /// The error.
        source: io::Error,
    },
    /// An installation-relative spelling has no valid id key.
    Key {
        /// The spelling that could not be keyed.
        spelling: String,
        /// Why the id grammar refused it.
        source: ContentIdError,
    },
    /// An identity the producing stage derived for one of its entries is not a
    /// valid catalog id. The identity itself came from the producing stage, so
    /// the refusal is named here instead of being worked around by writing the
    /// identity out a second time.
    Identity {
        /// The identity the producing stage refused.
        identity: String,
        /// Why it was refused.
        reason: String,
    },
    /// A source span could not be built from a file that was read.
    Span {
        /// The file the span locates.
        path: String,
        /// Why the span was refused.
        source: SourceSpanError,
    },
    /// A row failed the catalog's admission rules (a duplicate identity, a
    /// broken record or an undeclarable launchable).
    Row {
        /// The row that was refused.
        id: String,
        /// Why the catalog refused it.
        source: Box<CatalogError>,
    },
    /// The provenance of one recorded claim could not be built.
    Provenance {
        /// The claim id that was refused.
        claim: String,
        /// Why it was refused.
        reason: String,
    },
    /// The dependency closure over the declared roots could not be computed.
    Closure(ClosureError),
    /// The installation could not be mounted to list the reader archives the
    /// campaign layout leaves over.
    Session(String),
}

impl fmt::Display for BaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discover(error) => write!(f, "cannot inventory the installation: {error}"),
            Self::Campaign(error) => write!(f, "cannot read the campaign layout: {error}"),
            Self::MissingProgram { mission, asset } => write!(
                f,
                "the installation declares {mission} but holds no program archive at {asset}; \
                 the baseline inventory is refused rather than built with a shorter denominator"
            ),
            Self::UninventoriedProgram { mission, asset } => write!(
                f,
                "the program archive {asset} of {mission} is present but not inventoried, so no \
                 fingerprinted row can be built for it"
            ),
            Self::Key { spelling, source } => {
                write!(f, "no content id key for {spelling:?}: {source}")
            }
            Self::Identity { identity, reason } => {
                write!(f, "no content id for {identity:?}: {reason}")
            }
            Self::Span { path, source } => write!(f, "no source span for {path}: {source}"),
            Self::Read { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::Row { id, source } => write!(f, "catalog refused the row {id}: {source}"),
            Self::Provenance { claim, reason } => {
                write!(
                    f,
                    "the provenance of claim {claim} does not validate: {reason}"
                )
            }
            Self::Closure(error) => write!(f, "cannot compute the baseline closure: {error}"),
            Self::Session(reason) => {
                write!(
                    f,
                    "cannot mount the installation to classify its readers: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for BaselineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discover(error) => Some(error),
            Self::Campaign(error) => Some(error),
            Self::Key { source, .. } => Some(source),
            Self::Span { source, .. } => Some(source),
            Self::Row { source, .. } => Some(source),
            Self::Read { source, .. } => Some(source),
            Self::Closure(error) => Some(error),
            Self::MissingProgram { .. }
            | Self::UninventoriedProgram { .. }
            | Self::Identity { .. }
            | Self::Provenance { .. }
            | Self::Session(_) => None,
        }
    }
}

/// One directory that holds a reader archive the campaign layout does not
/// classify as a campaign mission.
///
/// The installation stores reader archives outside the `M<nn>` mission
/// directories (the world-group readers, the top-level reader and the
/// instant-action and multiplayer scenario directories). Those whose archive
/// member index decides their role are [`ClassifiedReaderDir`]s; this record
/// is what remains when it does not (an unreadable archive, or members that
/// fit no rule). It keeps such a directory visible in the inventory instead of
/// either counting it in the denominator or filtering it out (spec F14
/// non-negotiable behavior 4 and the `IDENTITY-CONTENT` rule that unreachable
/// unknowns stay in the global accounting report).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramDirRecord {
    /// The directory's spelling inside the installation.
    pub path: String,
    /// The reader archive it holds, as spelled.
    pub program: String,
    /// The digest of that archive's bytes, taken from the inventory.
    pub program_sha256: String,
}

/// The coverage accounting over the declared launchable roots.
///
/// The closure is computed once for every declared root with the production
/// [`Closure`], so the counts come from the same walk the `closure` command
/// reports; rows no root reaches stay here as unreachable instead of
/// disappearing from the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// How many declared launchable roots the walk started from (the
    /// denominator).
    pub roots: usize,
    /// Rows reachable from a declared root.
    pub reachable: usize,
    /// Rows no declared root reaches.
    pub unreachable: usize,
    /// Reached rows whose closure readiness is true.
    pub ready: usize,
    /// Reached rows whose closure readiness is false.
    pub unavailable: usize,
    /// References a reached row made to a row that does not exist. The
    /// baseline builds none, and a nonzero count is a defect, not a pass.
    pub unresolved_references: usize,
    /// Unreachable rows per content kind, in canonical kind order.
    pub unreachable_by_kind: BTreeMap<&'static str, usize>,
    /// Unreachable rows that are not ready either: they are unknown, so they
    /// still need an explicit unused/optional classification before a full
    /// release (`IDENTITY-CONTENT`).
    pub unreachable_needing_classification: usize,
}

/// What one source-derived collection of the inventory holds, and what the
/// producing stage's parser could not answer about it.
///
/// `IDENTITY-CONTENT` requires a catalog collection per content family and
/// forbids excluding failed entries. A collection whose producing parser
/// cannot read its source therefore has no row to carry a diagnostic — there
/// is no identity to attach one to, and inventing an id from a file name would
/// be exactly the guess rule 4 rejects. This record is where such a gap stays
/// visible instead: `rows` is zero, `diagnostic` says why, and the report
/// renders both. A later collection stage fills its own record in the same
/// shape rather than dropping the previous one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionStatus {
    /// The collection's content kind.
    pub kind: ContentKind,
    /// The installation-relative file whose bytes the rows come from, as the
    /// collection's stage reads it.
    pub source: String,
    /// The language the rows were read in, for a localized table; `None` when
    /// the source has no language dimension.
    pub language: Option<u32>,
    /// How many catalog rows the collection holds.
    pub rows: usize,
    /// Entries the producing parser read but could not turn into a complete
    /// row, counted under its own stable label (the mode table's
    /// `name_without_briefing` and `briefing_without_name` are the first).
    ///
    /// A name that pairs with no briefing still has a row (its identity and
    /// its bytes are both known), so its count here is not a count of missing
    /// rows; a briefing with no name has no identity at all and cannot become
    /// one, so its count is entries this collection can only account for.
    pub gaps: BTreeMap<&'static str, usize>,
    /// The id whose content ended the producing stage's measured walk, when
    /// it reports one (the first briefing block outside the mode family).
    pub boundary_id: Option<u32>,
    /// Why the collection holds no rows, when it holds none: the source is
    /// absent from the inventory, or the producing parser refused its bytes.
    /// `None` when the parser read the source.
    pub diagnostic: Option<String>,
}

/// The complete private baseline inventory of one installation.
#[derive(Clone, Debug)]
pub struct Baseline {
    /// The installation root as the caller spelled it, named by every report.
    pub source: String,
    /// The installation fingerprint of the read bytes (lowercase hex).
    pub install_sha256: String,
    /// The content fingerprint of the inventoried manifest (lowercase hex).
    pub content_sha256: String,
    /// The rows: every inventoried file, every campaign mission, every
    /// mission program archive and every row of each source-derived
    /// collection this stage can produce.
    pub catalog: Catalog,
    /// The declared launchable ids: every campaign mission, then every
    /// instant-action and multiplayer scenario directory
    /// ([`Baseline::classified_reader_dirs`]). This is the coverage
    /// denominator.
    pub roots: Vec<ContentId>,
    /// Reachable/unreachable accounting over [`Baseline::roots`].
    pub coverage: Coverage,
    /// Reader-archive directories the campaign layout does not claim and
    /// whose role the archive's own member index decides (F14-D.1). The
    /// launchable ones are rows of the denominator; the rest are recorded as
    /// not launchable.
    pub classified_reader_dirs: Vec<ClassifiedReaderDir>,
    /// Reader-archive directories neither the campaign layout nor the member
    /// evidence classifies. They are named, never counted and never dropped.
    pub unrecognized_program_dirs: Vec<ProgramDirRecord>,
    /// What each source-derived collection contributed, in collection order.
    pub collection_status: Vec<CollectionStatus>,
}

impl Baseline {
    /// The id of the install-file row holding `spelling`'s bytes, if any.
    pub fn install_file_id(spelling: &str) -> Result<ContentId, ContentIdError> {
        ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
    }
}

/// Builds the complete private baseline inventory of `install_root`.
///
/// The walk reads the installation four ways and nothing else: the F02
/// inventory (`cs_assets::install::discover`) for every regular file, the
/// shared campaign layout ([`crate::campaign_bindings::campaign_layout`]) for
/// the mission directories, each mission's reader archive for its span and
/// digest, and the string image F56-A reads the multiplayer mode table from
/// ([`MODE_STRING_IMAGE`], one row per mode). Rows are inserted in a fixed
/// order and every report array is rendered from canonical id order, so the
/// same installation yields the same bytes (spec F14 AC02).
///
/// # Errors
///
/// [`BaselineError`] — in particular [`BaselineError::MissingProgram`] when a
/// declared mission has no reader archive (the denominator is refused, never
/// shortened), [`BaselineError::Read`] when an inventoried file cannot be
/// read, [`BaselineError::Key`] when a spelling has no valid id and
/// [`BaselineError::Row`] when two rows would collide.
///
/// A collection whose producing parser refuses its source is **not** an error
/// here: it is reported in [`Baseline::collection_status`] with a diagnostic,
/// because refusing the whole inventory would hide the collections that do
/// read while a missing row would hide the failure.
pub fn retail_baseline(install_root: &Path) -> Result<Baseline, BaselineError> {
    let discovery = cs_assets::install::discover(install_root).map_err(BaselineError::Discover)?;
    let manifest = &discovery.manifest;
    let install_hash = cs_assets::install::fingerprint(manifest);
    let layout =
        crate::campaign_bindings::campaign_layout(install_root).map_err(BaselineError::Campaign)?;

    // The inventory rows: one per regular file, none skipped.
    let mut catalog = Catalog::new();
    let mut files: BTreeMap<String, &InstallFileRecord> = BTreeMap::new();
    for record in &manifest.files {
        let spelling = record.relative_spelling.as_str();
        let id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .map_err(|source| BaselineError::Key {
                spelling: spelling.to_owned(),
                source,
            })?;
        let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None)
            .map_err(|source| BaselineError::Span {
                path: spelling.to_owned(),
                source,
            })?;
        let element = CatalogElement {
            kind: ContentKind::InstallFile,
            id: id.clone(),
            display_name: Some(spelling.to_owned()),
            origin: Origin::Installation { source: span },
            dependencies: Vec::new(),
            parse_state: record.parse_state.clone(),
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![unparsed_reason(&record.parse_state)],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        };
        insert(&mut catalog, element)?;
        files.insert(record.relative_spelling.logical_key(), record);
    }

    // The launchable rows: one per declared campaign mission, each naming its
    // reader archive and the inventory row that holds its bytes.
    let mut roots = Vec::with_capacity(layout.len());
    for entry in &layout {
        let mission = &entry.mission;
        let mission_id = ContentId::from_source(
            ContentKind::Mission,
            &mission_key(mission.chapter, mission.mission_number),
        )
        .map_err(|source| BaselineError::Key {
            spelling: mission_key(mission.chapter, mission.mission_number),
            source,
        })?;
        if !mission.program_present {
            return Err(BaselineError::MissingProgram {
                mission: mission_id.to_string(),
                asset: mission.program_asset.clone(),
            });
        }
        let logical = mission.program_asset.to_ascii_lowercase();
        let Some(record) = files.get(&logical) else {
            return Err(BaselineError::UninventoriedProgram {
                mission: mission_id.to_string(),
                asset: mission.program_asset.clone(),
            });
        };
        let spelling = record.relative_spelling.as_str();
        let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None)
            .map_err(|source| BaselineError::Span {
                path: spelling.to_owned(),
                source,
            })?;

        // The program row: the reader archive's own bytes.
        let program_id = ContentId::from_source(
            ContentKind::Script,
            &program_key(&mission.world_group, mission.mission_number),
        )
        .map_err(|source| BaselineError::Key {
            spelling: program_key(&mission.world_group, mission.mission_number),
            source,
        })?;
        let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
            .map_err(|source| BaselineError::Key {
                spelling: spelling.to_owned(),
                source,
            })?;
        let program = CatalogElement {
            kind: ContentKind::Script,
            id: program_id.clone(),
            display_name: Some(mission.program_asset.clone()),
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: file_id,
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_INSTALL_FILE, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Unparsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotParsed],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        };
        insert(&mut catalog, program)?;

        // The mission row: located by the reader archive its directory names.
        let element = CatalogElement {
            kind: ContentKind::Mission,
            id: mission_id.clone(),
            display_name: None,
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target: program_id,
                kind: DependencyKind::Static,
                provenance: observed(CLAIM_MISSION_PROGRAM, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Unparsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotParsed],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        };
        insert(&mut catalog, element)?;
        roots.push(mission_id);
    }

    // The reader archives the campaign layout leaves over: classified from
    // their own member index, the launchable ones join the denominator.
    let candidates = unrecognized_program_dirs(manifest, &layout);
    let world_groups: BTreeSet<String> = layout
        .iter()
        .map(|entry| entry.mission.world_group.to_ascii_lowercase())
        .collect();
    let (classified_reader_dirs, unrecognized_program_dirs) = classify_reader_dirs(
        install_root,
        &discovery,
        install_hash,
        candidates,
        &world_groups,
    )?;
    for dir in classified_reader_dirs
        .iter()
        .filter(|dir| dir.role.is_launchable())
    {
        let Some(record) = files.get(&dir.program.to_ascii_lowercase()) else {
            return Err(BaselineError::UninventoriedProgram {
                mission: dir.path.clone(),
                asset: dir.program.clone(),
            });
        };
        roots.push(scenario_rows(&mut catalog, install_hash, dir, record)?);
    }
    for root in &roots {
        catalog
            .declare_launchable(root)
            .map_err(|source| BaselineError::Row {
                id: root.to_string(),
                source: Box::new(source),
            })?;
    }

    // The source-derived collections that are not launchable: one record each
    // so an unreadable one is reported instead of vanishing.
    let mut collection_status = Vec::new();
    let (rules, rules_status) = multiplayer_rules_rows(install_root, install_hash, &files)?;
    for element in rules {
        insert(&mut catalog, element)?;
    }
    collection_status.push(rules_status);

    let coverage = coverage(&catalog, &roots)?;

    Ok(Baseline {
        source: install_root.display().to_string(),
        install_sha256: install_hash.to_hex(),
        content_sha256: cs_assets::install::content_fingerprint(manifest).to_hex(),
        catalog,
        roots,
        coverage,
        classified_reader_dirs,
        unrecognized_program_dirs,
        collection_status,
    })
}

/// The `multiplayer_rules` rows the installation's string image names, read
/// by stage F56-A's own parser, plus the record of what that parser could not
/// answer.
///
/// The rows are the [`ModeEntry`] list [`discover_modes`] produces from the
/// string rows [`StringCatalog`] reads out of [`MODE_STRING_IMAGE`]. Each row
/// is located by the span of the `RT_STRING` block its name was read from —
/// a checked range inside the string image — and points at the inventory row
/// of the image that holds its bytes, so the closure can walk from a mode to
/// the file it came from. The ids are F56-A's: the mode's *name string id*,
/// never its position in a walk.
///
/// Everything F56-A could not read stays on the row: each rule the mode leaves
/// unknown becomes an [`UnsupportedReason::Unknown`] carrying F56-A's own claim
/// id and reason, so the row is a faithful inventory of what is known about
/// the mode rather than a claim that it is playable. A name F56-A could not
/// pair with a briefing gets a row of its own (see [`unpaired_mode_row`]),
/// because the collection of a content family cannot exclude an entry it failed
/// to complete. Nothing here is normalized (the row holds no quantity to
/// convert) and nothing claims a runtime consumer, so every row is unavailable,
/// exactly like every other row of this baseline.
///
/// # Errors
///
/// [`BaselineError::Read`] when the inventoried image cannot be read and
/// [`BaselineError::Span`] when its span does not validate. A source the
/// parser refuses yields no rows and a [`CollectionStatus::diagnostic`]
/// instead, which is a reported gap and not an error.
fn multiplayer_rules_rows(
    install_root: &Path,
    install_hash: ContentHash,
    files: &BTreeMap<String, &InstallFileRecord>,
) -> Result<(Vec<CatalogElement>, CollectionStatus), BaselineError> {
    let mut status = CollectionStatus {
        kind: ContentKind::MultiplayerRules,
        source: MODE_STRING_IMAGE.to_owned(),
        language: Some(MODE_STRING_LANGUAGE),
        rows: 0,
        gaps: BTreeMap::new(),
        boundary_id: None,
        diagnostic: None,
    };

    let Some(record) = files.get(&MODE_STRING_IMAGE.to_ascii_lowercase()) else {
        return Ok(unpopulated(
            status,
            format!(
                "the installation inventories no {MODE_STRING_IMAGE}, so the multiplayer mode \
                 table has no bytes to read"
            ),
        ));
    };
    let spelling = record.relative_spelling.as_str();
    let path = install_root.join(spelling);
    let bytes = std::fs::read(&path).map_err(|source| BaselineError::Read {
        path: spelling.to_owned(),
        source,
    })?;
    let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None).map_err(
        |source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        },
    )?;

    let mut context = cs_formats::ParseContext::with_defaults(spelling);
    let strings = match StringCatalog::read(&mut context, span, &bytes) {
        Ok(strings) => strings,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the string image {spelling} does not read as the PE resource image the \
                     multiplayer mode table is read from: {error}"
                ),
            ));
        }
    };
    let modes = match discover_modes(strings.rows(), MODE_STRING_LANGUAGE) {
        Ok(modes) => modes,
        Err(error) => {
            return Ok(unpopulated(
                status,
                format!(
                    "the multiplayer mode table of {spelling} (language {MODE_STRING_LANGUAGE}) \
                     does not read: {error}"
                ),
            ));
        }
    };

    let file_id = ContentId::from_source(ContentKind::InstallFile, &install_file_key(spelling))
        .map_err(|source| BaselineError::Key {
            spelling: spelling.to_owned(),
            source,
        })?;
    let mut rows: Vec<CatalogElement> = modes
        .modes
        .iter()
        .map(|mode| mode_row(mode, &file_id, record.sha256))
        .collect::<Result<Vec<_>, _>>()?;

    // A name F56-A could not pair with a briefing is still an entry of the
    // collection: the name's own bytes were read, so its row is built from
    // them and says, as an explicit unknown, that nothing describes it. A
    // collection cannot exclude an entry it failed to complete
    // (IDENTITY-CONTENT). The briefing with no name has no identity at all —
    // F56-A's id is built from a name id — so it stays in the record's gap
    // counts below and no row may be minted for it.
    rows.extend(
        modes
            .names_without_briefing
            .iter()
            .map(|name| unpaired_mode_row(name, &file_id, record.sha256))
            .collect::<Result<Vec<_>, _>>()?,
    );

    status.rows = rows.len();
    status
        .gaps
        .insert("name_without_briefing", modes.names_without_briefing.len());
    status
        .gaps
        .insert("briefing_without_name", modes.briefings_without_name.len());
    status.boundary_id = modes.boundary.as_ref().map(|row| row.id);
    Ok((rows, status))
}

/// The empty row set of a collection whose producing parser could not read its
/// source, carrying the diagnostic that says so.
///
/// The collection keeps its record — kind, source and language stay visible —
/// because a collection that silently held nothing would read like an
/// installation that has none of that content.
fn unpopulated(
    mut status: CollectionStatus,
    diagnostic: String,
) -> (Vec<CatalogElement>, CollectionStatus) {
    status.rows = 0;
    status.diagnostic = Some(diagnostic);
    (Vec::new(), status)
}

/// One multiplayer mode as a catalog row.
///
/// The span is the name row's own `RT_STRING` block, so the row names the
/// bytes it was read from rather than the file as a whole; the fingerprint and
/// the single dependency both point at the inventory row of the image, which is
/// what makes the edge walkable from a mode to its file.
fn mode_row(
    mode: &ModeEntry,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    // F56-A recorded one unknown per rule the installation does not state;
    // each keeps that stage's claim id and reason, so the row says which rule
    // is unknown instead of only that the mode is unusable.
    let unsupported_reasons = mode
        .unknown_rules
        .iter()
        .map(|rule| UnsupportedReason::Unknown {
            claim_id: rule.claim.clone(),
            reason: rule.reason.clone(),
        })
        .collect();
    Ok(CatalogElement {
        kind: ContentKind::MultiplayerRules,
        id: mode.id.clone(),
        display_name: Some(mode.name.text.clone()),
        origin: Origin::Installation {
            source: mode.name.span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_MODE_STRINGS, &mode.name.span)?,
        }],
        parse_state: cs_types::install::ParseState::Parsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        unsupported_reasons,
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256,
        }),
    })
}

/// Reads the member index of every candidate reader archive and splits the
/// candidates into the classified and the still-unknown.
///
/// A candidate whose archive cannot be listed is **unknown**, not an error:
/// the evidence is not there, so the directory stays named in the
/// unrecognized list instead of being guessed from its spelling.
fn classify_reader_dirs(
    install_root: &Path,
    discovery: &cs_assets::install::Discovery,
    install_hash: ContentHash,
    candidates: Vec<ProgramDirRecord>,
    world_groups: &BTreeSet<String>,
) -> Result<(Vec<ClassifiedReaderDir>, Vec<ProgramDirRecord>), BaselineError> {
    let mut builder = SessionBuilder::new(ResolveContext::new(install_hash));
    builder
        .mount_installation(install_root, &discovery.diagnosis)
        .map_err(|error| BaselineError::Session(error.to_string()))?;
    let session = builder.open();

    let present: BTreeSet<String> = discovery
        .manifest
        .files
        .iter()
        .map(|record| record.relative_spelling.logical_key())
        .collect();
    let mut classified = Vec::new();
    let mut unknown = Vec::new();
    for record in candidates {
        let members = AssetKey::from_spelling("install", &record.program, "default")
            .ok()
            .map(|key| audit_containers(&session, [&key]))
            .and_then(|audit| audit.containers.into_iter().next())
            .filter(|row| matches!(row.verdict, ContainerVerdict::Listed))
            .map(|row| {
                row.members
                    .iter()
                    .map(|member| String::from_utf8_lossy(&member.name).to_ascii_lowercase())
                    .collect::<BTreeSet<String>>()
            });
        let Some(members) = members else {
            unknown.push(record);
            continue;
        };
        let mis_anim = format!("{}/mis_anim.zbd", record.path).to_ascii_lowercase();
        let decision = classify(
            &record.path,
            &members,
            present.contains(&mis_anim),
            world_groups,
        );
        match decision {
            Some((role, evidence)) => classified.push(ClassifiedReaderDir {
                path: record.path,
                program: record.program,
                program_sha256: record.program_sha256,
                role,
                members: members.len(),
                evidence,
            }),
            None => unknown.push(record),
        }
    }
    Ok((classified, unknown))
}

/// The id key of one scenario directory's content id: `<world group>-<leaf>`
/// (`c1c-ia1`), lowercased.
fn scenario_key(dir: &ClassifiedReaderDir) -> String {
    let mut segments = dir.path.rsplit(['/', '\\']);
    let leaf = segments.next().unwrap_or_default();
    let group = segments.next().unwrap_or_default();
    format!("{group}-{leaf}").to_ascii_lowercase()
}

/// Inserts the program row and the scenario row of one launchable reader
/// directory and returns the scenario id (the root to declare).
///
/// The shape mirrors a campaign mission: scenario → script → install file,
/// every row `Origin::Installation` over the reader archive's own span.
fn scenario_rows(
    catalog: &mut Catalog,
    install_hash: ContentHash,
    dir: &ClassifiedReaderDir,
    record: &InstallFileRecord,
) -> Result<ContentId, BaselineError> {
    let kind = match dir.role {
        ReaderDirRole::InstantActionScenario => ContentKind::IaScenario,
        _ => ContentKind::MultiplayerScenario,
    };
    let key = scenario_key(dir);
    let spelling = record.relative_spelling.as_str();
    let span = SourceSpan::new(install_hash, spelling, None, 0, record.size_bytes, None).map_err(
        |source| BaselineError::Span {
            path: spelling.to_owned(),
            source,
        },
    )?;
    let id_of = |kind: ContentKind, key: &str| {
        ContentId::from_source(kind, key).map_err(|source| BaselineError::Key {
            spelling: key.to_owned(),
            source,
        })
    };
    let program_id = id_of(ContentKind::Script, &format!("{key}-zrdr"))?;
    let file_id = id_of(ContentKind::InstallFile, &install_file_key(spelling))?;
    let scenario_id = id_of(kind, &key)?;
    let row = |kind: ContentKind, id: ContentId, target: ContentId, claim: &str, name| {
        Ok::<_, BaselineError>(CatalogElement {
            kind,
            id,
            display_name: name,
            origin: Origin::Installation {
                source: span.clone(),
            },
            dependencies: vec![Dependency {
                target,
                kind: DependencyKind::Static,
                provenance: observed(claim, &span)?,
            }],
            parse_state: cs_types::install::ParseState::Unparsed,
            normalize_state: NormalizeState::NotNormalized,
            runtime_consumers: Vec::new(),
            readiness: Readiness::Unavailable,
            unsupported_reasons: vec![UnsupportedReason::NotParsed],
            fingerprint: Some(Fingerprint {
                kind: FingerprintKind::Installation,
                sha256: record.sha256,
            }),
        })
    };
    insert(
        catalog,
        row(
            ContentKind::Script,
            program_id.clone(),
            file_id,
            CLAIM_INSTALL_FILE,
            Some(dir.program.clone()),
        )?,
    )?;
    insert(
        catalog,
        row(
            kind,
            scenario_id.clone(),
            program_id,
            CLAIM_SCENARIO_PROGRAM,
            None,
        )?,
    )?;
    Ok(scenario_id)
}

/// A mode **name** the producing parser could not pair with a briefing, as a
/// catalog row.
///
/// The identity is F56-A's own ([`mode_name_id`]), so an unpaired name and a
/// paired one can never disagree about what a mode is called; what differs is
/// what the row can say. This row is built from bytes that were read — the name
/// and its `RT_STRING` block — so it exists rather than being excluded from the
/// collection, and its one explicit unknown says that no briefing of the
/// multiplayer family answers it: no rule, no points, no team play, nothing
/// that would let this engine say what the name refers to.
///
/// # Errors
///
/// [`BaselineError::Identity`] when the producing stage's identity is refused
/// and [`BaselineError::Provenance`] when the claim id is refused.
fn unpaired_mode_row(
    name: &TextRef,
    file_id: &ContentId,
    sha256: ContentHash,
) -> Result<CatalogElement, BaselineError> {
    let identity = format!("mode.name-{}", name.id);
    let id = mode_name_id(name.id).map_err(|error| BaselineError::Identity {
        identity,
        reason: error.to_string(),
    })?;
    Ok(CatalogElement {
        kind: ContentKind::MultiplayerRules,
        display_name: Some(name.text.clone()),
        origin: Origin::Installation {
            source: name.span.clone(),
        },
        dependencies: vec![Dependency {
            target: file_id.clone(),
            kind: DependencyKind::Static,
            provenance: observed(CLAIM_MODE_STRINGS, &name.span)?,
        }],
        parse_state: cs_types::install::ParseState::Parsed,
        normalize_state: NormalizeState::NotNormalized,
        runtime_consumers: Vec::new(),
        readiness: Readiness::Unavailable,
        // The entry the collection failed to complete is recorded here rather
        // than left out of it: one unknown that says the whole mode is
        // unresolved, instead of a rule list this engine cannot fill in.
        unsupported_reasons: vec![UnsupportedReason::Unknown {
            claim_id: ClaimId::new(CLAIM_MODE_PAIRING).map_err(|error| {
                BaselineError::Provenance {
                    claim: CLAIM_MODE_PAIRING.to_owned(),
                    reason: error.to_string(),
                }
            })?,
            reason: format!(
                "the string table names a mode at id {} but no briefing block of the \
                 multiplayer family pairs with it, so no rule of the mode is known",
                name.id
            ),
        }],
        fingerprint: Some(Fingerprint {
            kind: FingerprintKind::Installation,
            sha256,
        }),
        id,
    })
}
}

/// Inserts one row, naming it if the catalog refuses it.
fn insert(catalog: &mut Catalog, element: CatalogElement) -> Result<(), BaselineError> {
    let id = element.id.to_string();
    catalog
        .insert(element)
        .map_err(|source| BaselineError::Row {
            id,
            source: Box::new(source),
        })
}

/// The observed provenance of one dependency edge: the span the observation
/// was taken from, classed `observed_tool` — never `verified_original`, which
/// no agent-observed claim may award (`AGENTS.md` rule 8).
///
/// # Errors
///
/// [`BaselineError::Provenance`] when the claim id or the span is refused.
fn observed(claim: &str, source: &SourceSpan) -> Result<Provenance, BaselineError> {
    let claim_id = ClaimId::new(claim).map_err(|error| BaselineError::Provenance {
        claim: claim.to_owned(),
        reason: error.to_string(),
    })?;
    Provenance::new(claim_id, ClaimStatus::ObservedTool, Some(source.clone())).map_err(|error| {
        BaselineError::Provenance {
            claim: claim.to_owned(),
            reason: error.to_string(),
        }
    })
}

/// The reachability accounting over the declared launchable roots.
fn coverage(catalog: &Catalog, roots: &[ContentId]) -> Result<Coverage, BaselineError> {
    let closure = Closure::compute(catalog, roots, CompatibilityOptions::default())
        .map_err(BaselineError::Closure)?;
    let reached: BTreeSet<&ContentId> = closure.node_ids().into_iter().collect();

    let mut unreachable_by_kind: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut unreachable = 0usize;
    let mut unreachable_needing_classification = 0usize;
    for element in catalog.elements() {
        if reached.contains(&element.id) {
            continue;
        }
        unreachable += 1;
        *unreachable_by_kind.entry(element.kind.label()).or_default() += 1;
        if !element.is_ready() {
            unreachable_needing_classification += 1;
        }
    }

    Ok(Coverage {
        roots: roots.len(),
        reachable: reached.len(),
        unreachable,
        ready: reached.iter().filter(|id| closure.is_ready(id)).count(),
        unavailable: closure.unavailable().len(),
        unresolved_references: closure.unresolved().len(),
        unreachable_by_kind,
        unreachable_needing_classification,
    })
}

/// The reader-archive directories the campaign layout does not classify.
///
/// Every inventoried file whose name is a reader archive (`zrdr.zbd`) is
/// compared against the program assets the campaign layout declares; the
/// rest are recorded with their digest. Filenames are compared
/// case-insensitively, so the record does not depend on the installation's
/// letter case.
fn unrecognized_program_dirs(
    manifest: &cs_types::install::InstallManifest,
    layout: &[crate::campaign_bindings::CampaignLayoutEntry],
) -> Vec<ProgramDirRecord> {
    let declared: BTreeSet<String> = layout
        .iter()
        .map(|entry| entry.mission.program_asset.to_ascii_lowercase())
        .collect();
    let mut records = Vec::new();
    for record in &manifest.files {
        let spelling = record.relative_spelling.as_str();
        let logical = spelling.to_ascii_lowercase();
        if declared.contains(&logical) {
            continue;
        }
        if logical.rsplit(['/', '\\']).next() != Some("zrdr.zbd") {
            continue;
        }
        // The record keeps the installation's own spelling; only the
        // comparison above folds case.
        let path = match spelling.rsplit_once(['/', '\\']) {
            Some((parent, _)) => parent.to_owned(),
            None => ".".to_owned(),
        };
        records.push(ProgramDirRecord {
            path,
            program: spelling.to_owned(),
            program_sha256: record.sha256.to_hex(),
        });
    }
    records.sort_by(|left, right| (&left.path, &left.program).cmp(&(&right.path, &right.program)));
    records
}

/// Renders the deterministic JSON baseline report.
///
/// Every array comes from canonical id order (or a sorted key), so the same
/// rows serialize byte-for-byte identically whatever order the filesystem
/// walked them in (spec F14 AC02). The nested `coverage` object is the
/// production [`Coverage`] computed by [`retail_baseline`], and each row
/// carries its own origin and source span, so a synthetic fixture row mixed
/// into the catalog renders `synthetic_fixture` beside the retail rows'
/// `installation` (spec F14 AC04).
pub fn baseline_report_json(baseline: &Baseline) -> String {
    let catalog = &baseline.catalog;
    let mut rows = 0usize;
    let mut ready = 0usize;
    let mut collections: BTreeMap<&'static str, usize> = BTreeMap::new();
    for element in catalog.elements() {
        rows += 1;
        if element.is_ready() {
            ready += 1;
        }
        *collections.entry(element.kind.label()).or_default() += 1;
    }
    let launchable = catalog.launchable_count();
    let unsupported_launchable = catalog.unsupported_count();
    let original_launchable = catalog.original_launchable_count();
    let synthetic_launchable = catalog.synthetic_launchable_count();
    let is_fully_ready = catalog.is_fully_ready();
    let is_retail_ready = catalog.is_retail_ready();
    // Same meaning as the `cs-inspect catalog` report's `retail` field: any
    // row whose origin is original installation data.
    let has_original_rows = catalog
        .elements()
        .any(|element| element.origin.is_original());

    let mut out = String::new();
    let _ = write!(
        out,
        "{{\"schema\":{},\"source\":{},\"retail\":{},\"install_sha256\":{},\
         \"content_sha256\":{},\"rows\":{},\"ready\":{},\"unavailable\":{},\
         \"launchable\":{},\"unsupported_launchable\":{},\"original_launchable\":{},\
         \"synthetic_launchable\":{},\"is_fully_ready\":{},\"is_retail_ready\":{},\
         \"collections\":{{",
        json_string(BASELINE_REPORT_VERSION),
        json_string(&baseline.source),
        has_original_rows,
        json_string(&baseline.install_sha256),
        json_string(&baseline.content_sha256),
        rows,
        ready,
        rows - ready,
        launchable,
        unsupported_launchable,
        original_launchable,
        synthetic_launchable,
        is_fully_ready,
        is_retail_ready,
    );
    join_map(&collections, &mut out);
    let _ = write!(out, "}},\"coverage\":{{");
    let coverage = &baseline.coverage;
    let _ = write!(
        out,
        "\"roots\":{},\"reachable\":{},\"unreachable\":{},\"ready\":{},\"unavailable\":{},\
         \"unresolved_references\":{},\"unreachable_needing_classification\":{},\
         \"unreachable_by_kind\":{{",
        coverage.roots,
        coverage.reachable,
        coverage.unreachable,
        coverage.ready,
        coverage.unavailable,
        coverage.unresolved_references,
        coverage.unreachable_needing_classification,
    );
    join_map(&coverage.unreachable_by_kind, &mut out);
    out.push_str("}},\"collection_status\":[");
    for (index, status) in baseline.collection_status.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        collection_status_json(status, &mut out);
    }
    out.push_str("],\"unrecognized_program_dirs\":[");
    for (index, record) in baseline.unrecognized_program_dirs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"path\":{},\"program\":{},\"program_sha256\":{}}}",
            json_string(&record.path),
            json_string(&record.program),
            json_string(&record.program_sha256),
        );
    }
    out.push_str("],\"classified_reader_dirs\":[");
    for (index, dir) in baseline.classified_reader_dirs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let evidence: Vec<String> = dir.evidence.iter().map(|name| json_string(name)).collect();
        let _ = write!(
            out,
            "{{\"path\":{},\"program\":{},\"program_sha256\":{},\"role\":{},\
             \"launchable\":{},\"members\":{},\"evidence\":[{}]}}",
            json_string(&dir.path),
            json_string(&dir.program),
            json_string(&dir.program_sha256),
            json_string(dir.role.label()),
            dir.role.is_launchable(),
            dir.members,
            evidence.join(","),
        );
    }
    out.push_str("],\"elements\":[");
    for (index, element) in catalog.elements().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&element_json(element));
    }
    out.push_str("]}");
    out
}

/// Renders one collection record: what it holds and, when it holds nothing,
/// why.
///
/// The `rows` count is rendered beside the catalog's own `collections` map so
/// the two cannot disagree silently, and `diagnostic` is a JSON string or
/// `null` — a collection with rows has no diagnostic, and one without rows
/// always has one.
fn collection_status_json(status: &CollectionStatus, out: &mut String) {
    let _ = write!(
        out,
        "{{\"kind\":{},\"source\":{},\"language\":{},\"rows\":{},\"gaps\":{{",
        json_string(status.kind.label()),
        json_string(&status.source),
        match status.language {
            Some(language) => language.to_string(),
            None => "null".to_owned(),
        },
        status.rows,
    );
    join_map(&status.gaps, out);
    let _ = write!(
        out,
        "}},\"boundary_id\":{},\"diagnostic\":{}}}",
        match status.boundary_id {
            Some(id) => id.to_string(),
            None => "null".to_owned(),
        },
        match &status.diagnostic {
            Some(diagnostic) => json_string(diagnostic),
            None => "null".to_owned(),
        },
    );
}

/// Renders one `key":count` map with its keys in canonical order.
fn join_map(map: &BTreeMap<&'static str, usize>, out: &mut String) {
    for (index, (key, count)) in map.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "{}:{count}", json_string(key));
    }
}

/// Renders one baseline row: identity, origin, states, edges and digest.
///
/// The origin is split into a label and a `source` object so a reader can
/// filter retail rows by `"origin":"installation"` and see which exact span
/// backs each of them; a synthetic or designed row carries `null`.
fn element_json(element: &CatalogElement) -> String {
    let source = match &element.origin {
        Origin::Installation { source } => format!(
            "{{\"container_path\":{},\"member_key\":{},\"offset\":{},\"length\":{}}}",
            json_string(source.container_path()),
            match source.member_key() {
                Some(key) => json_string(key),
                None => "null".to_owned(),
            },
            source.offset(),
            source.length(),
        ),
        _ => "null".to_owned(),
    };
    let consumers: Vec<String> = element
        .runtime_consumers
        .iter()
        .map(|consumer| {
            format!(
                "{{\"kind\":{},\"claim\":{},\"class\":{}}}",
                json_string(consumer.kind.label()),
                json_string(consumer.provenance.claim_id.as_str()),
                json_string(consumer.provenance.class.label()),
            )
        })
        .collect();
    let mut dependencies: Vec<String> = element
        .dependencies
        .iter()
        .map(|dependency| {
            format!(
                "{{\"target\":{},\"kind\":{},\"claim\":{},\"class\":{}}}",
                json_string(dependency.target.as_str()),
                json_string(dependency.kind.label()),
                json_string(dependency.provenance.claim_id.as_str()),
                json_string(dependency.provenance.class.label()),
            )
        })
        .collect();
    dependencies.sort();
    let reasons: Vec<String> = element
        .unsupported_reasons
        .iter()
        .map(|reason| {
            format!(
                "{{\"code\":{},\"detail\":{}}}",
                json_string(reason.code()),
                match reason.detail() {
                    Some(detail) => json_string(detail),
                    None => "null".to_owned(),
                }
            )
        })
        .collect();
    format!(
        "{{\"id\":{},\"kind\":{},\"display_name\":{},\"origin\":{},\"source\":{},\
         \"parse_state\":{},\"normalize_state\":{},\"readiness\":{},\"reasons\":[{}],\
         \"dependencies\":[{}],\"consumers\":[{}],\"fingerprint\":{}}}",
        json_string(element.id.as_str()),
        json_string(element.kind.label()),
        match &element.display_name {
            Some(name) => json_string(name),
            None => "null".to_owned(),
        },
        json_string(element.origin.label()),
        source,
        json_string(parse_state_label(&element.parse_state)),
        json_string(normalize_state_label(&element.normalize_state)),
        json_string(element.readiness.label()),
        reasons.join(","),
        dependencies.join(","),
        consumers.join(","),
        match &element.fingerprint {
            Some(fingerprint) => json_string(&fingerprint.sha256.to_hex()),
            None => "null".to_owned(),
        },
    )
}

/// The stable label of a parse state.
fn parse_state_label(state: &cs_types::install::ParseState) -> &'static str {
    match state {
        cs_types::install::ParseState::Unparsed => "unparsed",
        cs_types::install::ParseState::Parsed => "parsed",
        cs_types::install::ParseState::Failed { .. } => "failed",
    }
}

/// The stable label of a normalize state.
fn normalize_state_label(state: &NormalizeState) -> &'static str {
    match state {
        NormalizeState::NotNormalized => "not_normalized",
        NormalizeState::Normalized => "normalized",
        NormalizeState::Failed { .. } => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key encoding is injective modulo case: it folds case (the
    /// installation inventory is case-insensitively unique by construction),
    /// escapes every byte outside the id grammar and never maps two
    /// case-distinct-in-more-than-case spellings onto one key, so the
    /// inventory cannot silently merge two installation files.
    #[test]
    fn accept_f14_d_install_file_key_is_injective_and_keeps_the_grammar() {
        let spellings = [
            "ZBD/C1C/M01/zrdr.zbd",
            "ZBD\\C1C\\M01\\zrdr.zbd",
            "mis_anim.zbd",
            "GOSDATA/ASSETS/BINARIES/langui.dll",
            "a_b_c",
            "a/b/c",
            "a_2f_b",
            "EULA.RTF",
            "00000409.016",
            "space name.dll",
            "ünicode.bin",
        ];
        let mut seen = BTreeMap::new();
        for spelling in spellings {
            let key = install_file_key(spelling);
            assert!(
                key.chars().all(|ch| {
                    ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
                }),
                "{spelling} encoded to {key:?}, outside the id grammar"
            );
            assert!(
                key.bytes().any(|byte| byte.is_ascii_alphanumeric()),
                "{spelling} encoded to a key with no alphanumeric character"
            );
            let previous = seen.insert(key.clone(), spelling);
            assert!(
                previous.is_none(),
                "{spelling} and {} encode to the same key {key:?}",
                previous.expect("a collision")
            );
            // The id constructor accepts the encoded key unchanged (its own
            // lowercasing is a no-op on it), so encoding and identity agree.
            let round_trip =
                ContentId::from_source(ContentKind::InstallFile, &key).expect("valid key");
            assert_eq!(round_trip.key(), key, "the id keeps the encoded key");
        }

        // Case folding is the one lossy step and is safe only because the
        // installation inventory is case-insensitively unique; the escape
        // still separates the two spellings that differ in more than case.
        assert_ne!(
            install_file_key("ZBD/C1C/M01/zrdr.zbd"),
            install_file_key("ZBD/C1C/M01/zrdr.zbx"),
            "different files must not share a key"
        );
        assert_eq!(
            install_file_key("ZBD/C1C/M01/zrdr.zbd"),
            install_file_key("zbd/c1c/m01/zrdr.zbd"),
            "case folds by design: the inventory itself refuses two files that differ only in \
             case, so no two inventoried rows can collide here"
        );
        assert_eq!(
            install_file_key("mis_anim.zbd"),
            "mis_5f_anim.zbd",
            "an underscore is escaped, so it can never open an escape itself"
        );
        assert_eq!(
            install_file_key("ZBD/C1C/M01/zrdr.zbd"),
            "zbd_2f_c1c_2f_m01_2f_zrdr.zbd",
            "the documented spelling of a retail reader archive"
        );
    }

    /// A key longer than the id grammar allows is refused by name, never
    /// truncated into another file's identity.
    #[test]
    fn accept_f14_d_install_file_key_refuses_a_key_that_is_too_long() {
        let deep = "ZBD/".to_owned() + &"verylongdirectoryname/".repeat(12) + "file.zbd";
        let key = install_file_key(&deep);
        assert!(
            key.len() > cs_types::content::MAX_CONTENT_KEY_LEN,
            "the fixture must exceed the id limit, got {} bytes",
            key.len()
        );
        let error = ContentId::from_source(ContentKind::InstallFile, &key)
            .expect_err("an over-long key must be refused");
        assert!(
            matches!(error, ContentIdError::KeyTooLong { .. }),
            "got {error:?}"
        );
        assert!(
            BaselineError::Key {
                spelling: deep,
                source: error,
            }
            .to_string()
            .contains("no content id key"),
            "the refusal names the spelling"
        );
    }

    /// The mode table is read from the string image stage F56-A measured, and
    /// a row of that collection joins to the inventory row of the image by
    /// identity alone: the dependency the closure walks is derivable from the
    /// spelling, with no second reader in between.
    #[test]
    fn accept_f14_d_2_mode_string_image_joins_its_inventory_row() {
        assert_eq!(MODE_STRING_IMAGE, "strings.dll", "the measured spelling");
        assert_eq!(
            MODE_STRING_LANGUAGE,
            cs_formats::LANG_ENGLISH_US,
            "the only language the surveyed images record"
        );
        let id = Baseline::install_file_id(MODE_STRING_IMAGE).expect("the image is keyable");
        assert_eq!(id.as_str(), "install_file/strings.dll");
        assert_eq!(id.key(), install_file_key(MODE_STRING_IMAGE));
    }

    /// A collection record renders both of its states: rows with no
    /// diagnostic, and no rows with the diagnostic that says why. A report that
    /// dropped either half would read like a complete inventory.
    #[test]
    fn accept_f14_d_2_collection_status_renders_rows_and_diagnostics() {
        let populated = CollectionStatus {
            kind: ContentKind::MultiplayerRules,
            source: MODE_STRING_IMAGE.to_owned(),
            language: Some(MODE_STRING_LANGUAGE),
            rows: 4,
            gaps: BTreeMap::from([
                ("briefing_without_name", 0usize),
                ("name_without_briefing", 0usize),
            ]),
            boundary_id: Some(16_680),
            diagnostic: None,
        };
        let mut out = String::new();
        collection_status_json(&populated, &mut out);
        assert_eq!(
            out,
            "{\"kind\":\"multiplayer_rules\",\"source\":\"strings.dll\",\"language\":1033,\
             \"rows\":4,\"gaps\":{\"briefing_without_name\":0,\"name_without_briefing\":0},\
             \"boundary_id\":16680,\"diagnostic\":null}"
        );

        let unread = CollectionStatus {
            rows: 0,
            diagnostic: Some("the installation inventories no strings.dll".to_owned()),
            ..populated.clone()
        };
        let mut out = String::new();
        collection_status_json(&unread, &mut out);
        assert!(
            out.contains("\"rows\":0")
                && out.contains("\"diagnostic\":\"the installation inventories no strings.dll\""),
            "{out}"
        );
        // Deterministic: the same record renders the same bytes.
        let mut again = String::new();
        collection_status_json(&unread, &mut again);
        assert_eq!(out, again);
    }

    /// The published binding identity and this module's derivation agree, so
    /// the baseline's mission and program rows are the rows the mission
    /// bindings already point at.
    #[test]
    fn accept_f14_d_baseline_keys_match_the_published_mission_binding() {
        let published = include_str!("../../../../missions/bindings/M01.json");
        let field = |name: &str| {
            published
                .split(&format!("\"{name}\": \""))
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_else(|| panic!("M01.json carries {name}"))
                .to_owned()
        };
        let catalog_id = field("catalog_id");
        let program_id = field("program_id");
        // The reader archive the binding cites, not the string table it also
        // cites: `ZBD/<world>/<M nn>/zrdr.zbd`.
        let program_asset = published
            .split("\"asset_id\": \"")
            .filter_map(|rest| rest.split('"').next())
            .find(|asset| asset.to_ascii_lowercase().ends_with("/zrdr.zbd"))
            .expect("M01.json cites its program archive")
            .to_owned();

        let mut parts = program_asset.split('/');
        assert_eq!(parts.next(), Some("ZBD"), "the campaign container");
        let world = parts.next().expect("the world group directory").to_owned();
        let mission_dir = parts.next().expect("the mission directory");
        let mission_number: u32 = mission_dir[1..].parse().expect("M<nn>");
        // The published world group spells its chapter: `C1C` is chapter 1.
        let chapter: u32 = world
            .trim_start_matches(['c', 'C'])
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("the chapter digits of the world group");

        let mission =
            ContentId::from_source(ContentKind::Mission, &mission_key(chapter, mission_number))
                .expect("mission id");
        let program = ContentId::from_source(
            ContentKind::Script,
            &program_key(&world.to_ascii_lowercase(), mission_number),
        )
        .expect("program id");
        assert_eq!(mission.as_str(), catalog_id, "the published mission id");
        assert_eq!(program.as_str(), program_id, "the published program id");
        assert_eq!(
            ContentId::from_source(ContentKind::World, &world)
                .expect("world id")
                .as_str(),
            "world/c1c",
            "the world group keeps its published identity"
        );
    }
}
