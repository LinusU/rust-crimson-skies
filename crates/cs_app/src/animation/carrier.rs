//! Binding an animation carrier's own member index to the mission animation
//! document that references it (task #633, `M01-LC-ANIM-CARRIERS`).
//!
//! # The two halves this joins
//!
//! [`survey`](super::survey) established that every mission-scoped directory
//! carries a `mis_anim.zbd` beside a `zrdr.zbd`, that every world group carries
//! a `cam_anim.zbd` beside its own `zrdr.zbd`, and that each sibling reader
//! holds the paired record (`mis_anim.zrd` / `cam_anim.zrd`). It validated and
//! fingerprinted the carriers but read **no payload**: `ZbdFamily::Animation`
//! had a header rule and a reader slot, and nothing behind them.
//!
//! Two measured facts close that gap, and this module is the join they make:
//!
//! 1. **The carrier indexes itself.** Its own front index (read by
//!    [`cs_formats::zbd::anim`]) names the animation-definition sources whose
//!    records its payload carries — `..\data\common\zrdr\zeps\wv_turrets.zrd`,
//!    `..\data\c1c\m01\zrdr\zeps\climbladder.zan`, and so on, each with a
//!    build stamp. Those are the carrier's **members**.
//! 2. **The paired record names them back.** `mis_anim.zrd` decodes through the
//!    production `.zrd` grammar reader into one `ANIMATION_DEFINITIONS` record
//!    whose `ANIMATION_LIST` holds one `ANIMATION_DEFINITION_FILE` per
//!    definition source, each either a full `..\data\...` path or a bare file
//!    name to resolve against the record's `ANIMATION_PATH` roots.
//!
//! [`bind_animation_carrier`] performs that join and gives every member and
//! every reference a disposition. Nothing is dropped: a member no reference
//! names is reported by [`CarrierBinding::unreferenced_member_count`], and a
//! reference no member answers is an [`UnresolvedReference`] carrying every
//! spelling it was compared against — so `zbd/c1c/m01`'s
//! `..\data\common\zrdr\zeps\wv_tailhook.zrd`, which the document names while
//! the carrier stores the same basename under the mission root
//! `..\data\c1c\m01\zrdr\zeps\`, is reported rather than quietly matched by
//! basename. That measurement is 8 of 739 references corpus-wide.
//!
//! # Paths are compared verbatim
//!
//! The store keeps Windows separators, mixed case and — measured — a doubled
//! separator in the very first member row of every carrier. Nothing here
//! normalizes a spelling: a reference binds only on a byte-for-byte match
//! (spec F06 non-negotiable #3), and every other case is named.
//!
//! # What is still not decoded
//!
//! The carrier's **animation records** — the payload behind the fixed header
//! [`cs_formats::zbd::anim`] reads — are not decoded, so a member is bound here
//! but its *contents* are not. [`startup_identities`] names the animation
//! identities `startanims.zrd` carries beside them, which have nothing to bind
//! to until the records are decoded; they are reported as an open input with
//! the reason, never matched by a byte search dressed as a binding. Both gaps
//! are recorded in `docs/findings/2026-10-04-m01-lc-anim-carriers.md`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use cs_assets::install::{self, Discovery};
use cs_content::stunts::{ZrdValue, decode_zrd, zrd_flat_fields};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    AnimationIndex, AnimationRow, DispatchBasis, HeaderStatus, ZbdFamily, ZbdProbe, dispatch,
    family_record, read_animation_index, read_reader_archive, read_version_one_index,
};
use cs_types::evidence::SourceSpan;
use cs_types::install::RelativePath;

use super::survey::{
    CAMERA_CARRIER, CAMERA_MEMBER, CarrierKind, MISSION_CARRIER, MISSION_MEMBER, READER_ARCHIVE,
};

/// The key of the paired animation record in a scope's reader archive.
#[must_use]
pub const fn document_member(kind: CarrierKind) -> &'static str {
    match kind {
        CarrierKind::Mission => MISSION_MEMBER,
        CarrierKind::Camera => CAMERA_MEMBER,
    }
}

/// The carrier file name of a scope's family.
#[must_use]
pub const fn carrier_name(kind: CarrierKind) -> &'static str {
    match kind {
        CarrierKind::Mission => MISSION_CARRIER,
        CarrierKind::Camera => CAMERA_CARRIER,
    }
}

/// The `ANIMATION_DEFINITIONS` key every paired animation record carries.
pub const ANIMATION_DEFINITIONS_KEY: &str = "ANIMATION_DEFINITIONS";
/// The `GRAVITY` key: a float every paired animation record states.
pub const GRAVITY_KEY: &str = "GRAVITY";
/// The `ANIMATION_PATH` key: the roots a bare definition-file name resolves
/// against. Absent in 11 of the 61 retail records.
pub const ANIMATION_PATH_KEY: &str = "ANIMATION_PATH";
/// The `ANIMATION_LIST` key: the definition files themselves.
pub const ANIMATION_LIST_KEY: &str = "ANIMATION_LIST";
/// The key of one definition file inside [`ANIMATION_LIST_KEY`].
pub const ANIMATION_DEFINITION_FILE_KEY: &str = "ANIMATION_DEFINITION_FILE";

/// The separator a stored `ANIMATION_PATH` root list uses when one value
/// carries more than one root: measured in 19 of the 61 retail records.
pub const PATH_ROOT_SEPARATOR: char = ';';

/// The separator every stored animation path uses between components.
pub const PATH_COMPONENT_SEPARATOR: char = '\\';

/// The startup-animation record every mission-scoped reader carries and no
/// world-group reader does (53 of the 61 retail carriers' scopes).
pub const STARTUP_MEMBER: &str = "startanims.zrd";

/// Why a definition-file reference names no member of its carrier.
pub const UNRESOLVED_REASON_NO_MEMBER: &str = "no member row of this carrier spells the reference \
     (or any of its resolved candidates) byte for byte; the container keeps Windows separators, \
     case and a doubled separator verbatim, and this reader never rewrites a spelling";

/// Why the animation identities `startanims.zrd` names are not bound here.
pub const UNRESOLVED_REASON_NO_RECORD_NAMES: &str = "startanims.zrd names animation identities, and an animation record's name lives in the carrier \
     payload, whose records are not decoded (cs_formats::zbd::anim \
     RECORDS_NOT_DECODED_REASON); no measured rule maps an identity onto an index row, so the \
     identity is reported as an open input rather than matched by a byte search";

/// One index row of a carrier, as this module keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrierMember {
    /// Position of the row in the carrier's own member table (0-based, and
    /// independent of where the two external rows sit in [`Self::external`]).
    pub index: usize,
    /// Whether the row is one of the two external-container rows rather than a
    /// member row.
    pub external: bool,
    /// The row's path, verbatim.
    pub path: String,
    /// The row's stamp word, verbatim.
    pub stamp: u32,
    /// Where the row sits inside the carrier.
    pub span: SourceSpan,
    /// The ordinals of the document references that name this row.
    pub references: Vec<usize>,
}

impl CarrierMember {
    /// Whether a document reference names this row.
    #[must_use]
    pub fn is_referenced(&self) -> bool {
        !self.references.is_empty()
    }
}

/// One `ANIMATION_DEFINITION_FILE` reference and where it went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationReference {
    /// The reference as the document spells it.
    pub raw: String,
    /// The paths this reference was compared against, in order: the reference
    /// itself when it carries a separator, otherwise each `ANIMATION_PATH` root
    /// joined to it. Empty when the reference bound, so an unresolved row
    /// keeps its candidates and a bound one does not repeat them.
    pub candidates: Vec<String>,
    /// Index of the member row the reference names, when one does.
    pub member: Option<usize>,
}

/// A document reference no member of the carrier answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedReference {
    /// The reference as the document spells it.
    pub raw: String,
    /// The paths it was compared against.
    pub candidates: Vec<String>,
    /// Why it is unresolved.
    pub reason: &'static str,
}

/// What the carrier's payload states, as far as it is decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct PayloadFacts {
    /// Where the payload starts inside the carrier.
    pub span: SourceSpan,
    /// The container's declared animation-record count. **Inferred** to be a
    /// record count; nothing in this tree can index a record by it.
    pub declared_record_count: u16,
    /// The gravity the payload states (`-9.8` in every retail carrier).
    pub gravity: f32,
    /// Where the animation records begin.
    pub record_table_offset: u64,
    /// The payload's **first** record name, exactly as stored. The records are
    /// not walked, so this is one name and not a table.
    pub first_record_name: Vec<u8>,
    /// Why the records are not decoded.
    pub records_not_decoded_reason: &'static str,
}

/// The paired animation record of one scope, decoded through the production
/// `.zrd` grammar reader.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationDocument {
    /// The reader archive's logical key the record came from.
    pub reader_key: String,
    /// The record member's name inside that archive.
    pub member: String,
    /// Where the record's bytes sit inside the reader archive.
    pub span: SourceSpan,
    /// The `GRAVITY` float the record states, when it states one.
    pub gravity: Option<f32>,
    /// The `ANIMATION_PATH` roots, empty when the record has none.
    pub roots: Vec<String>,
    /// The definition files, in document order.
    pub references: Vec<String>,
}

/// One startup key of `startanims.zrd` and the animation identities it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupGroup {
    /// The key itself (`NEW_GAME_START` or `LOAD_GAME_START` in all 53
    /// measured records).
    pub key: String,
    /// The identities the key starts, in declared order. Empty is measured
    /// content: `zbd/c1c/ia1`'s `LOAD_GAME_START` names none.
    pub identities: Vec<String>,
}

/// The startup animation identities of one scope, read but not bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupIdentities {
    /// The reader archive's logical key the record came from.
    pub reader_key: String,
    /// The record member's name inside that archive.
    pub member: String,
    /// The startup keys and their identities, in declared order.
    pub groups: Vec<StartupGroup>,
    /// Why they are not bound to carrier members.
    pub reason: &'static str,
}

impl StartupIdentities {
    /// Every identity the record names, keys aside, in declared order.
    #[must_use]
    pub fn identity_count(&self) -> usize {
        self.groups.iter().map(|group| group.identities.len()).sum()
    }
}

/// The sibling reader of a scope, as the caller found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SiblingReader<'a> {
    /// The reader is present and these are its bytes.
    Bytes(&'a [u8]),
    /// The reader is present but its bytes could not be read.
    Unreadable(String),
    /// The scope has no sibling reader at all.
    Absent,
}

/// Why one scope's carrier could not be read or bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingBlocker {
    /// The carrier's bytes could not be read from disk.
    CarrierUnreadable {
        /// The carrier's logical key.
        key: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
    /// Two-key dispatch refused the carrier's bytes, or its index did.
    CarrierRefused {
        /// The carrier's logical key.
        key: String,
        /// The refusal code, verbatim.
        code: &'static str,
    },
    /// The carrier dispatched to another family.
    WrongFamily {
        /// The carrier's logical key.
        key: String,
        /// The family dispatch decided.
        family: ZbdFamily,
    },
    /// The carrier indexed, but its payload header could not be read.
    PayloadRefused {
        /// The carrier's logical key.
        key: String,
        /// The refusal code, verbatim.
        code: &'static str,
    },
    /// The scope has no sibling `zrdr.zbd`, so no document pairs with it.
    MissingReader {
        /// The reader key the layout expects.
        expected_key: String,
    },
    /// The sibling reader's bytes could not be read.
    ReaderUnreadable {
        /// The reader's logical key.
        key: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
    /// The sibling reader refused dispatch, its index or its listing.
    ReaderRefused {
        /// The reader's logical key.
        key: String,
        /// The refusal code, verbatim.
        code: &'static str,
    },
    /// The sibling reader carries no paired animation record.
    DocumentAbsent {
        /// The reader's logical key.
        reader_key: String,
        /// The member name that is missing.
        member: &'static str,
    },
    /// The paired record's bytes are not a `.zrd` record this reader accepts.
    DocumentRefused {
        /// The reader's logical key.
        reader_key: String,
        /// The `ZrdDecodeError` code, verbatim.
        code: &'static str,
    },
    /// The paired record decodes but does not have the measured shape.
    DocumentShapeRefused {
        /// The reader's logical key.
        reader_key: String,
        /// What was missing, named.
        reason: &'static str,
    },
}

impl BindingBlocker {
    /// Stable lowercase label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::CarrierUnreadable { .. } => "carrier_unreadable",
            Self::CarrierRefused { .. } => "carrier_refused",
            Self::WrongFamily { .. } => "wrong_family",
            Self::PayloadRefused { .. } => "payload_refused",
            Self::MissingReader { .. } => "missing_reader",
            Self::ReaderUnreadable { .. } => "reader_unreadable",
            Self::ReaderRefused { .. } => "reader_refused",
            Self::DocumentAbsent { .. } => "document_absent",
            Self::DocumentRefused { .. } => "document_refused",
            Self::DocumentShapeRefused { .. } => "document_shape_refused",
        }
    }
}

impl fmt::Display for BindingBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CarrierUnreadable { key, reason } => {
                write!(f, "the animation carrier {key} could not be read: {reason}")
            }
            Self::CarrierRefused { key, code } => {
                write!(f, "the animation carrier {key} was refused ({code})")
            }
            Self::WrongFamily { key, family } => write!(
                f,
                "the animation carrier {key} dispatched to the `{}` family",
                family.as_str()
            ),
            Self::PayloadRefused { key, code } => {
                write!(
                    f,
                    "the animation carrier {key} refused its payload ({code})"
                )
            }
            Self::MissingReader { expected_key } => {
                write!(
                    f,
                    "no reader archive at {expected_key} to pair the carrier with"
                )
            }
            Self::ReaderUnreadable { key, reason } => {
                write!(f, "the reader archive {key} could not be read: {reason}")
            }
            Self::ReaderRefused { key, code } => {
                write!(f, "the reader archive {key} was refused ({code})")
            }
            Self::DocumentAbsent { reader_key, member } => {
                write!(
                    f,
                    "the reader archive {reader_key} carries no {member} member"
                )
            }
            Self::DocumentRefused { reader_key, code } => write!(
                f,
                "the paired animation record of {reader_key} was refused ({code})"
            ),
            Self::DocumentShapeRefused { reader_key, reason } => write!(
                f,
                "the paired animation record of {reader_key} does not have the measured shape: \
                 {reason}"
            ),
        }
    }
}

impl std::error::Error for BindingBlocker {}

/// One carrier, its own member index, and the document references bound to it.
#[derive(Clone, Debug, PartialEq)]
pub struct CarrierBinding {
    /// The carrier's logical key (`zbd/<group>/cam_anim.zbd` or
    /// `zbd/<group>/<mission>/mis_anim.zbd`).
    pub container_key: String,
    /// Which family this carrier is.
    pub kind: CarrierKind,
    /// How dispatch identified the family.
    pub basis: DispatchBasis,
    /// The version word the header declared.
    pub version: u32,
    /// The carrier's size in bytes.
    pub size_bytes: u64,
    /// The two external rows, in declared order.
    pub externals: Vec<CarrierMember>,
    /// The member rows, in declared order.
    pub members: Vec<CarrierMember>,
    /// What the payload states, when the payload could be read.
    pub payload: Option<PayloadFacts>,
    /// The paired record, when the sibling reader carried one.
    pub document: Option<AnimationDocument>,
    /// Every definition-file reference, in document order.
    pub references: Vec<AnimationReference>,
    /// The references no member answers.
    pub unresolved: Vec<UnresolvedReference>,
    /// The startup animation identities of the scope, when its reader carried
    /// `startanims.zrd`. They are listed, never bound.
    pub startup: Option<StartupIdentities>,
    /// Everything that went wrong, in the order it was found. The row exists
    /// either way: a blocked scope is reported, not dropped.
    pub blockers: Vec<BindingBlocker>,
}

impl CarrierBinding {
    /// The startup identities this carrier's scope declares, keys aside.
    #[must_use]
    pub fn startup_identity_count(&self) -> usize {
        self.startup
            .as_ref()
            .map_or(0, StartupIdentities::identity_count)
    }

    /// How many member rows a document reference names.
    #[must_use]
    pub fn referenced_member_count(&self) -> usize {
        self.members
            .iter()
            .filter(|row| row.is_referenced())
            .count()
    }

    /// How many member rows no reference names.
    #[must_use]
    pub fn unreferenced_member_count(&self) -> usize {
        self.members
            .iter()
            .filter(|row| !row.is_referenced())
            .count()
    }

    /// How many references found their member.
    #[must_use]
    pub fn bound_reference_count(&self) -> usize {
        self.references
            .iter()
            .filter(|entry| entry.member.is_some())
            .count()
    }

    /// Whether the carrier indexed, its payload was read, its paired record
    /// decoded and no reference was left unresolved.
    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.blockers.is_empty() && self.unresolved.is_empty() && self.payload.is_some()
    }
}

/// The carriers of one installation, one row each, in container-key order.
#[derive(Clone, Debug)]
pub struct AnimationBindingSurvey {
    /// Every carrier the manifest declares, in container-key order.
    pub carriers: Vec<CarrierBinding>,
}

impl AnimationBindingSurvey {
    /// One carrier by its container key.
    #[must_use]
    pub fn carrier(&self, key: &str) -> Option<&CarrierBinding> {
        self.carriers.iter().find(|row| row.container_key == key)
    }

    /// The carriers of one family.
    pub fn carriers_of(&self, kind: CarrierKind) -> impl Iterator<Item = &CarrierBinding> {
        self.carriers.iter().filter(move |row| row.kind == kind)
    }

    /// Every blocker across the survey, with its carrier key.
    pub fn blockers(&self) -> impl Iterator<Item = (&str, &BindingBlocker)> {
        self.carriers.iter().flat_map(|row| {
            row.blockers
                .iter()
                .map(move |blocker| (row.container_key.as_str(), blocker))
        })
    }

    /// Every unresolved reference across the survey, with its carrier key.
    pub fn unresolved_references(&self) -> impl Iterator<Item = (&str, &UnresolvedReference)> {
        self.carriers.iter().flat_map(|row| {
            row.unresolved
                .iter()
                .map(move |entry| (row.container_key.as_str(), entry))
        })
    }

    /// Whether every carrier indexed, bound every reference and read its
    /// payload.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.carriers.is_empty() && self.carriers.iter().all(CarrierBinding::is_bound)
    }
}

/// Why a binding survey could not be produced at all.
#[derive(Debug)]
pub enum AnimationBindingError {
    /// Production discovery could not read the installation.
    Discovery(install::DiscoveryError),
}

impl fmt::Display for AnimationBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => {
                write!(f, "the installation could not be discovered: {error}")
            }
        }
    }
}

impl std::error::Error for AnimationBindingError {}

/// Reads the carrier's own index and binds it to the paired records of its
/// scope's reader archive.
///
/// `path` is the carrier's installation-relative spelling: two-key dispatch
/// needs it for the role half of the decision, and the logical key it derives
/// is the row's label. `reader_key` names the sibling reader for a
/// [`BindingBlocker::MissingReader`]; `reader_bytes` is `None` when the scope
/// has no sibling reader at all.
///
/// The bytes come from the caller, so a synthetic installation and the
/// original one take exactly this path.
#[must_use]
pub fn bind_animation_carrier(
    path: &RelativePath,
    reader_key: &str,
    kind: CarrierKind,
    carrier_bytes: &[u8],
    reader: SiblingReader<'_>,
) -> CarrierBinding {
    let key = path.logical_key();
    let mut blockers = Vec::new();
    let mut externals: Vec<CarrierMember> = Vec::new();
    let mut members: Vec<CarrierMember> = Vec::new();
    let mut references: Vec<AnimationReference> = Vec::new();
    let mut unresolved: Vec<UnresolvedReference> = Vec::new();

    let mut context = ParseContext::with_defaults(&key);
    // The dispatch probe evaluates the documented header rule, which needs
    // only the signature and version words; the index reader then takes the
    // whole container.
    let probe_bytes = header_prefix(carrier_bytes);
    let mut basis = DispatchBasis::HeaderOnly;
    let mut version = 0;
    let index: Result<AnimationIndex<'_>, BindingBlocker> =
        match dispatch(ZbdProbe::new(&key, path, probe_bytes)) {
            Ok(decision) => {
                basis = decision.basis();
                version = match decision.header_status() {
                    HeaderStatus::Validated { version, .. } => version,
                    HeaderStatus::Unvalidated { .. } => {
                        blockers.push(BindingBlocker::CarrierRefused {
                            key: key.clone(),
                            code: "unvalidated_header",
                        });
                        0
                    }
                };
                if decision.family() != ZbdFamily::Animation {
                    Err(BindingBlocker::WrongFamily {
                        key: key.clone(),
                        family: decision.family(),
                    })
                } else {
                    read_animation_index(&mut context, decision, carrier_bytes).map_err(|error| {
                        BindingBlocker::CarrierRefused {
                            key: key.clone(),
                            code: error.code(),
                        }
                    })
                }
            }
            Err(error) => Err(BindingBlocker::CarrierRefused {
                key: key.clone(),
                code: error.code(),
            }),
        };

    let payload = match &index {
        Ok(index) => match index.payload() {
            Ok(payload) => Some(PayloadFacts {
                span: payload.span(),
                declared_record_count: payload.header().declared_record_count,
                gravity: payload.header().gravity,
                record_table_offset: payload.record_table_offset(),
                first_record_name: payload.first_record_name().to_vec(),
                records_not_decoded_reason: payload.records_not_decoded_reason(),
            }),
            Err(error) => {
                blockers.push(BindingBlocker::PayloadRefused {
                    key: key.clone(),
                    code: error.code(),
                });
                None
            }
        },
        Err(blocker) => {
            blockers.push(blocker.clone());
            None
        }
    };

    if let Ok(index) = &index {
        collect_rows(index, &mut externals, &mut members);
    }

    let mut document = None;
    let mut startup = None;
    let reader_bytes = match reader {
        SiblingReader::Bytes(bytes) => Some(bytes),
        SiblingReader::Unreadable(reason) => {
            blockers.push(BindingBlocker::ReaderUnreadable {
                key: reader_key.to_owned(),
                reason,
            });
            None
        }
        SiblingReader::Absent => {
            blockers.push(BindingBlocker::MissingReader {
                expected_key: reader_key.to_owned(),
            });
            None
        }
    };
    if let Some(bytes) = reader_bytes {
        match read_document(reader_key, bytes, document_member(kind)) {
            Ok(decoded) => document = Some(decoded),
            Err(blocker) => blockers.push(blocker),
        }
        match read_startup_identities(reader_key, bytes) {
            // A scope whose reader has no `startanims.zrd` is content, not a
            // failure: the field stays absent and nothing is reported.
            Ok(Some(identities)) => startup = Some(identities),
            Ok(None) => {}
            Err(blocker) => blockers.push(blocker),
        }
    }

    if let (Ok(_), Some(document)) = (&index, &document) {
        bind_references(document, &mut members, &mut references, &mut unresolved);
    }

    CarrierBinding {
        container_key: key,
        kind,
        basis,
        version,
        size_bytes: carrier_bytes.len() as u64,
        externals,
        members,
        payload,
        document,
        references,
        unresolved,
        startup,
        blockers,
    }
}

/// The bytes the documented header rule evaluates: the signature and version
/// words, or the whole container when it is shorter than that.
fn header_prefix(bytes: &[u8]) -> &[u8] {
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    &bytes[..needed]
}

/// Copies the index rows into owned rows with no reference bound yet.
fn collect_rows(
    index: &AnimationIndex<'_>,
    externals: &mut Vec<CarrierMember>,
    members: &mut Vec<CarrierMember>,
) {
    externals.extend(index.externals().iter().map(owned_row));
    members.extend(index.members().iter().map(owned_row));
}

/// One index row as an owned [`CarrierMember`].
fn owned_row(row: &AnimationRow<'_>) -> CarrierMember {
    CarrierMember {
        index: row.index(),
        external: row.is_external(),
        path: String::from_utf8_lossy(row.path()).into_owned(),
        stamp: row.stamp(),
        span: row.record_span(),
        references: Vec::new(),
    }
}

/// Finds a member of a reader archive by name and decodes it as an
/// `ANIMATION_DEFINITIONS` record.
fn read_document(
    reader_key: &str,
    reader_bytes: &[u8],
    member: &'static str,
) -> Result<AnimationDocument, BindingBlocker> {
    let content = read_reader_member(reader_key, reader_bytes, member)?;
    let (span, bytes) = content;
    let value = decode_zrd(&bytes).map_err(|error| BindingBlocker::DocumentRefused {
        reader_key: reader_key.to_owned(),
        code: error.code(),
    })?;
    decode_document(reader_key, member, span, &value)
        .map_err(|(reader_key, reason)| BindingBlocker::DocumentShapeRefused { reader_key, reason })
}

/// Reads the scope's `startanims.zrd` startup table.
///
/// `Ok(None)` is measured content, not a failure: the 53 mission-scoped
/// readers carry this member and the world-group and content-root readers do
/// not, so an absent one leaves the field empty and reports nothing.
fn read_startup_identities(
    reader_key: &str,
    reader_bytes: &[u8],
) -> Result<Option<StartupIdentities>, BindingBlocker> {
    let member = STARTUP_MEMBER;
    let (_, bytes) = match read_reader_member(reader_key, reader_bytes, member) {
        Ok(found) => found,
        Err(BindingBlocker::DocumentAbsent { .. }) => return Ok(None),
        Err(blocker) => return Err(blocker),
    };
    let value = decode_zrd(&bytes).map_err(|error| BindingBlocker::DocumentRefused {
        reader_key: reader_key.to_owned(),
        code: error.code(),
    })?;
    let groups = startup_groups(&value);
    Ok(Some(StartupIdentities {
        reader_key: reader_key.to_owned(),
        member: member.to_owned(),
        groups,
        reason: UNRESOLVED_REASON_NO_RECORD_NAMES,
    }))
}

/// The startup groups a decoded `startanims.zrd` states.
///
/// Measured shape of all 53 records: one record of two keys —
/// `NEW_GAME_START` and `LOAD_GAME_START`, in that order — each with a list of
/// animation identities, one identity per child list. The identities are read;
/// nothing here maps one onto a carrier member.
fn startup_groups(value: &ZrdValue) -> Vec<StartupGroup> {
    let Some(children) = value.as_list() else {
        return Vec::new();
    };
    let Some(record) = children.first() else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    for (key, value) in zrd_flat_fields(record) {
        let identities = value
            .as_list()
            .unwrap_or_default()
            .iter()
            .filter_map(first_text)
            .map(ToOwned::to_owned)
            .collect();
        groups.push(StartupGroup {
            key: key.to_owned(),
            identities,
        });
    }
    groups
}

/// Reads one named member out of a reader archive with the production
/// dispatch, trailer and listing readers.
///
/// The member's bytes are copied out: a `.zrd` record is a few kilobytes, and
/// the archive's own borrows (its dispatch's spelling, its member table) cannot
/// outlive this function, so handing out the bytes is what lets the decoded
/// document outlive them.
fn read_reader_member(
    reader_key: &str,
    reader_bytes: &[u8],
    member: &'static str,
) -> Result<(SourceSpan, Vec<u8>), BindingBlocker> {
    let mut context = ParseContext::with_defaults(reader_key);
    let path = path_for(reader_key);
    let decision = dispatch(ZbdProbe::new(reader_key, &path, reader_bytes)).map_err(|error| {
        BindingBlocker::ReaderRefused {
            key: reader_key.to_owned(),
            code: error.code(),
        }
    })?;
    let index = read_version_one_index(&mut context, decision, reader_bytes).map_err(|error| {
        BindingBlocker::ReaderRefused {
            key: reader_key.to_owned(),
            code: error.code(),
        }
    })?;
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).map_err(|error| {
        BindingBlocker::ReaderRefused {
            key: reader_key.to_owned(),
            code: error.code(),
        }
    })?;
    archive
        .entries()
        .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
        .map(|entry| (entry.span(), entry.content().to_vec()))
        .ok_or_else(|| BindingBlocker::DocumentAbsent {
            reader_key: reader_key.to_owned(),
            member,
        })
}

/// The relative spelling of a logical key, for the role half of a dispatch.
///
/// A key that does not parse as a relative path cannot match a role rule; the
/// header half still decides, and this keeps the caller from inventing a path.
fn path_for(key: &str) -> RelativePath {
    RelativePath::new(key).unwrap_or_else(|_| {
        RelativePath::new("zbd").expect("the static fallback spelling is relative")
    })
}

/// Turns one decoded `.zrd` record into an [`AnimationDocument`].
///
/// The measured shape of all 61 retail records is
/// `root -> [ ANIMATION_DEFINITIONS, { GRAVITY, ANIMATION_PATH?, ANIMATION_LIST } ]`,
/// where `ANIMATION_LIST` alternates `ANIMATION_DEFINITION_FILE` with a
/// one-element list holding the path. Anything else is refused by name.
fn decode_document(
    reader_key: &str,
    member: &'static str,
    span: SourceSpan,
    value: &ZrdValue,
) -> Result<AnimationDocument, (String, &'static str)> {
    let record = value
        .as_list()
        .and_then(|children| children.first())
        .ok_or_else(|| (reader_key.to_owned(), DOC_SHAPE_NO_RECORD))?;
    let fields = zrd_flat_fields(record);
    let (_, body) = fields
        .iter()
        .find(|(key, _)| *key == ANIMATION_DEFINITIONS_KEY)
        .ok_or_else(|| (reader_key.to_owned(), DOC_SHAPE_NO_DEFINITIONS))?;
    let body = zrd_flat_fields(body);
    let gravity = body
        .iter()
        .find(|(key, _)| *key == GRAVITY_KEY)
        .and_then(|(_, value)| first_float(value));
    let roots = body
        .iter()
        .find(|(key, _)| *key == ANIMATION_PATH_KEY)
        .map_or_else(Vec::new, |(_, value)| path_roots(value));
    let list = body
        .iter()
        .find(|(key, _)| *key == ANIMATION_LIST_KEY)
        .ok_or_else(|| (reader_key.to_owned(), DOC_SHAPE_NO_LIST))?;
    let mut references = Vec::new();
    for (key, value) in zrd_flat_fields(list.1) {
        if key != ANIMATION_DEFINITION_FILE_KEY {
            continue;
        }
        match first_text(value) {
            Some(path) => references.push(path.to_owned()),
            None => {
                return Err((reader_key.to_owned(), DOC_SHAPE_DEFINITION_NOT_TEXT));
            }
        }
    }
    Ok(AnimationDocument {
        reader_key: reader_key.to_owned(),
        member: member.to_owned(),
        span,
        gravity,
        roots,
        references,
    })
}

/// The record decoded, but it has no child node at all.
const DOC_SHAPE_NO_RECORD: &str = "the decoded member has no child node";
/// The record decoded, but no `ANIMATION_DEFINITIONS` key sits in it.
const DOC_SHAPE_NO_DEFINITIONS: &str = "no ANIMATION_DEFINITIONS key in the decoded record";
/// The record decoded, but its definitions carry no `ANIMATION_LIST`.
const DOC_SHAPE_NO_LIST: &str = "no ANIMATION_LIST key in the ANIMATION_DEFINITIONS body";
/// The record decoded, but one `ANIMATION_DEFINITION_FILE` is not a text path.
const DOC_SHAPE_DEFINITION_NOT_TEXT: &str =
    "an ANIMATION_DEFINITION_FILE entry is not a single text path";

/// The text of a one-element list, which is how the records spell a scalar.
fn first_text(value: &ZrdValue) -> Option<&str> {
    value
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_text)
}

/// The float of a one-element list.
fn first_float(value: &ZrdValue) -> Option<f32> {
    match value.as_list().and_then(|children| children.first()) {
        Some(ZrdValue::Float(value)) => Some(*value),
        _ => None,
    }
}

/// The roots of one `ANIMATION_PATH` value: its text entries, each split on
/// [`PATH_ROOT_SEPARATOR`] because 19 of the 61 retail records store two roots
/// in one string.
fn path_roots(value: &ZrdValue) -> Vec<String> {
    let mut roots = Vec::new();
    for child in value.as_list().unwrap_or_default() {
        let Some(text) = child.as_text() else {
            continue;
        };
        for part in text.split(PATH_ROOT_SEPARATOR) {
            if !part.is_empty() {
                roots.push(part.to_owned());
            }
        }
    }
    roots
}

/// Joins the document's references to the carrier's member rows.
///
/// A reference that carries a separator is compared as it stands; a bare name
/// is joined to each root in order, and the first candidate that names a member
/// wins. Nothing is normalized, lower-cased or matched by basename.
fn bind_references(
    document: &AnimationDocument,
    members: &mut [CarrierMember],
    references: &mut Vec<AnimationReference>,
    unresolved: &mut Vec<UnresolvedReference>,
) {
    let mut by_path: BTreeMap<String, usize> = BTreeMap::new();
    for row in members.iter() {
        by_path.entry(row.path.clone()).or_insert(row.index);
    }

    for raw in &document.references {
        let candidates: Vec<String> = if raw.contains(PATH_COMPONENT_SEPARATOR) {
            vec![raw.clone()]
        } else {
            document
                .roots
                .iter()
                .map(|root| format!("{root}{PATH_COMPONENT_SEPARATOR}{raw}"))
                .collect()
        };
        let ordinal = references.len();
        let found = candidates
            .iter()
            .find_map(|candidate| by_path.get(candidate).copied());
        match found {
            Some(member) => {
                if let Some(row) = members.iter_mut().find(|row| row.index == member) {
                    row.references.push(ordinal);
                }
                references.push(AnimationReference {
                    raw: raw.clone(),
                    candidates: Vec::new(),
                    member: Some(member),
                });
            }
            None => {
                unresolved.push(UnresolvedReference {
                    raw: raw.clone(),
                    candidates: candidates.clone(),
                    reason: UNRESOLVED_REASON_NO_MEMBER,
                });
                references.push(AnimationReference {
                    raw: raw.clone(),
                    candidates: Vec::new(),
                    member: None,
                });
            }
        }
    }
}

/// Binds every animation carrier the installation declares.
///
/// One production discovery pass, then per carrier: its own header through
/// [`dispatch`], its index through [`read_animation_index`], its sibling
/// reader's paired records through the production `.zrd` reader, and the join
/// between them.
pub fn survey_animation_bindings(
    install_root: &Path,
) -> Result<AnimationBindingSurvey, AnimationBindingError> {
    let found = install::discover(install_root).map_err(AnimationBindingError::Discovery)?;
    Ok(bind_installation(&found, install_root))
}

/// Binds every animation carrier of an installation the caller already
/// discovered.
///
/// The discovery pass is the expensive half of the survey, so a caller that
/// has one — a census over several scopes, say — hands it in instead of paying
/// for it twice.
#[must_use]
pub fn bind_installation(found: &Discovery, install_root: &Path) -> AnimationBindingSurvey {
    let mut carriers = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        let kind = if key.ends_with(&format!("/{MISSION_CARRIER}")) {
            CarrierKind::Mission
        } else if key.ends_with(&format!("/{CAMERA_CARRIER}")) {
            CarrierKind::Camera
        } else {
            continue;
        };
        let Some((scope, _)) = key.rsplit_once('/') else {
            continue;
        };
        let reader_key = format!("{scope}/{READER_ARCHIVE}");
        let carrier_host = install_root.join(record.relative_spelling.as_str());
        let reader = found
            .manifest
            .files
            .iter()
            .find(|candidate| candidate.relative_spelling.logical_key() == reader_key)
            .map(|candidate| {
                std::fs::read(install_root.join(candidate.relative_spelling.as_str()))
            });
        let binding = match (std::fs::read(&carrier_host), reader) {
            (Ok(carrier_bytes), Some(Ok(reader_bytes))) => bind_animation_carrier(
                &record.relative_spelling,
                &reader_key,
                kind,
                &carrier_bytes,
                SiblingReader::Bytes(&reader_bytes),
            ),
            (Ok(carrier_bytes), Some(Err(error))) => bind_animation_carrier(
                &record.relative_spelling,
                &reader_key,
                kind,
                &carrier_bytes,
                SiblingReader::Unreadable(error.to_string()),
            ),
            (Ok(carrier_bytes), None) => bind_animation_carrier(
                &record.relative_spelling,
                &reader_key,
                kind,
                &carrier_bytes,
                SiblingReader::Absent,
            ),
            (Err(error), _) => {
                let mut binding = bind_animation_carrier(
                    &record.relative_spelling,
                    &reader_key,
                    kind,
                    &[],
                    SiblingReader::Absent,
                );
                binding.size_bytes = record.size_bytes;
                binding.blockers.retain(|blocker| {
                    !matches!(
                        blocker,
                        BindingBlocker::MissingReader { .. }
                            | BindingBlocker::CarrierRefused { .. }
                            | BindingBlocker::WrongFamily { .. }
                    )
                });
                binding.blockers.push(BindingBlocker::CarrierUnreadable {
                    key,
                    reason: error.to_string(),
                });
                binding
            }
        };
        carriers.push(binding);
    }
    carriers.sort_by(|left, right| left.container_key.cmp(&right.container_key));
    AnimationBindingSurvey { carriers }
}
