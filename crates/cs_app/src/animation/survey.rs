//! The mission-critical animation-family survey: every carrier the original
//! installation declares, validated and fingerprinted
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-D`; shared contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! # What an "animation family" is here
//!
//! F06's family inventory ties two observed archive names to the animation
//! reader ([`ZbdFamily::Animation`]): `mis_anim.zbd` in a **mission**
//! directory and `cam_anim.zbd` in a **world group** directory
//! (`cs_formats::zbd::family`, task #340 findings). Each sits beside the
//! scope's own `zrdr.zbd` reader archive, and that archive carries the paired
//! record member (`mis_anim.zrd`, `cam_anim.zrd`) plus any other
//! animation-named records — `startanims.zrd` rides in every mission reader,
//! and individual missions carry their own extras (F13-B member census).
//!
//! So one *carrier* is one of those containers plus the record members its
//! sibling reader holds, and *the* mission-critical families are the two
//! carriers every launchable scope declares. A survey that did not check both
//! directions would quietly miss a scope whose animation data is absent, so
//! the walk is built from the **expected** set:
//!
//! * every discovered world group (`zbd/<group>`) must carry `cam_anim.zbd`
//!   beside its `zrdr.zbd`;
//! * every directory under a world group that carries a `zrdr.zbd` — a
//!   launchable mission, instant-action or multiplayer scope — must carry
//!   `mis_anim.zbd`;
//! * every mission directory the campaign layout declares
//!   (`cs_content::campaign_bindings::campaign_layout`) must appear among
//!   those mission scopes, whether or not its reader is present.
//!
//! # What is measured, and what stays unknown
//!
//! For each carrier the survey runs the production two-key
//! [`dispatch`] on the container's own header bytes, records the validated
//! signature/version, the manifest's SHA-256 and size, and reads the sibling
//! reader archive's version-one trailer through
//! [`read_version_one_index`]/[`read_reader_archive`] to account for the
//! paired record and every other `*anim*` member it holds. Nothing here
//! decodes a payload: the animation container bodies are still an
//! **undecoded layout** (F13, F20-C findings), so a carrier is *validated*,
//! never *interpreted*, and the digest families the survey groups them into
//! are a byte-level fact, not a semantic one.
//!
//! # Fail-closed
//!
//! A scope with no carrier, a carrier that dispatches to anything but the
//! animation family or fails its signature probe, a sibling reader that
//! cannot be read, or a reader that lacks the paired member is a
//! [`CarrierBlocker`] on that scope's row — the row is never dropped. An
//! animation-named `.zbd` at a position the layout does not declare is
//! recorded in [`AnimationFamilySurvey::strays`] rather than ignored. The
//! verdict a caller asserts is [`AnimationFamilySurvey::is_clean`], which is
//! false as long as any blocker or stray exists.

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::Path;

use cs_assets::install::{self, Discovery};
use cs_content::campaign_bindings::campaign_layout;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{
    ANIMATION_SIGNATURE, DispatchBasis, HeaderStatus, ZbdFamily, ZbdProbe, dispatch, family_record,
    read_reader_archive, read_version_one_index,
};
use cs_types::evidence::{ContentHash, SourceSpan};
use cs_types::install::InstallFileRecord;

/// The carrier file every mission-scoped directory declares.
pub const MISSION_CARRIER: &str = "mis_anim.zbd";
/// The paired record a mission scope's reader archive carries for it.
pub const MISSION_MEMBER: &str = "mis_anim.zrd";
/// The carrier file every world-group directory declares.
pub const CAMERA_CARRIER: &str = "cam_anim.zbd";
/// The paired record a world group's reader archive carries for it.
pub const CAMERA_MEMBER: &str = "cam_anim.zrd";
/// The reader archive sibling every scope's carrier pairs with.
pub const READER_ARCHIVE: &str = "zrdr.zbd";

/// Which of the two mission-critical animation families a scope's carrier
/// belongs to.
///
/// The names are the F06 role rules' (`cam_anim.zbd` at world-group level,
/// `mis_anim.zbd` at mission level); the pairing is the observed layout
/// F13-B's member census recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CarrierKind {
    /// `mis_anim.zbd` in `zbd/<group>/<mission>/`, paired with the
    /// `mis_anim.zrd` member of that directory's `zrdr.zbd`.
    Mission,
    /// `cam_anim.zbd` in `zbd/<group>/`, paired with the `cam_anim.zrd`
    /// member of that group's `zrdr.zbd`.
    Camera,
}

impl CarrierKind {
    /// The carrier's file name inside its scope directory.
    #[must_use]
    pub const fn container_name(self) -> &'static str {
        match self {
            Self::Mission => MISSION_CARRIER,
            Self::Camera => CAMERA_CARRIER,
        }
    }

    /// The paired record's member name inside the scope's `zrdr.zbd`.
    #[must_use]
    pub const fn member_name(self) -> &'static str {
        match self {
            Self::Mission => MISSION_MEMBER,
            Self::Camera => CAMERA_MEMBER,
        }
    }

    /// Stable lowercase label for reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mission => "mission_animation",
            Self::Camera => "camera_animation",
        }
    }
}

/// One record member of a scope's sibling reader archive, measured.
///
/// The record's bytes are never retained or returned — the reader entry
/// hands out the member's span and this digest of exactly those bytes, so a
/// reviewer can match the member against another installation without the
/// payload leaving the installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberRecord {
    /// The member's name, as the index stores it.
    pub name: String,
    /// Where the member's bytes live inside the archive.
    pub span: SourceSpan,
    /// SHA-256 of the member's bytes.
    pub sha256: ContentHash,
}

/// What the survey measured for one carrier, all through production readers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarrierRecord {
    /// The carrier container's logical key (`zbd/<group>/cam_anim.zbd` or
    /// `zbd/<group>/<mission>/mis_anim.zbd`).
    pub container_key: String,
    /// The scope directory's logical key the carrier belongs to.
    pub scope: String,
    /// Which family this carrier is.
    pub kind: CarrierKind,
    /// How dispatch identified the family — `header+role` when both keys
    /// agree, `header` alone when the carrier sits at a position no role
    /// rule declares.
    pub basis: DispatchBasis,
    /// The version word the documented signature probe validated (53 on the
    /// retail installation family).
    pub version: u32,
    /// The carrier's size, from production discovery.
    pub size_bytes: u64,
    /// SHA-256 of the carrier, from production discovery.
    pub sha256: ContentHash,
    /// The sibling reader archive's logical key the members came from.
    pub reader_key: String,
    /// Every `*anim*` member of the scope's reader archive, in declared
    /// order — the paired record plus extras like `startanims.zrd`, so the
    /// family census is complete rather than just the name this stage
    /// already knew.
    pub animation_members: Vec<MemberRecord>,
}

impl CarrierRecord {
    /// The paired record member the family name declares, when the sibling
    /// reader carried it.
    #[must_use]
    pub fn paired_member(&self) -> Option<&MemberRecord> {
        self.animation_members
            .iter()
            .find(|member| member.name.eq_ignore_ascii_case(self.kind.member_name()))
    }
}

/// Why one scope's animation carrier could not be validated.
///
/// Every variant names the logical key it is about; a blocker is a fact about
/// that scope, never a reason to drop the row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarrierBlocker {
    /// The scope's carrier file does not exist in the manifest at all.
    MissingCarrier {
        /// The logical key the layout expects the carrier at.
        expected_key: String,
    },
    /// The carrier's file exists but its bytes could not be read.
    Unreadable {
        /// The carrier's logical key.
        key: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
    /// Two-key dispatch refused the carrier's own header bytes: the
    /// signature did not match, the version is not the documented one, or
    /// the bytes claim another family's signature.
    DispatchRefused {
        /// The carrier's logical key.
        key: String,
        /// `ZbdDispatchError::code`, verbatim.
        code: &'static str,
    },
    /// The carrier dispatched, but not to the animation family — impossible
    /// for a header-and-role match, recorded because the check must exist
    /// rather than be assumed unreachable.
    WrongFamily {
        /// The carrier's logical key.
        key: String,
        /// The family dispatch decided.
        family: ZbdFamily,
    },
    /// The carrier validated but its header bytes could not be confirmed
    /// (`HeaderStatus::Unvalidated`) — unreachable for the animation family,
    /// which has a documented signature rule, recorded for the same reason
    /// as [`Self::WrongFamily`].
    UnvalidatedHeader {
        /// The carrier's logical key.
        key: String,
    },
    /// The scope's sibling `zrdr.zbd` does not exist, so the paired record
    /// cannot be accounted for.
    MissingReader {
        /// The logical key the reader is expected at.
        expected_key: String,
    },
    /// The sibling reader exists but its bytes could not be read.
    ReaderUnreadable {
        /// The reader's logical key.
        key: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
    /// The sibling reader's bytes refused dispatch, index or listing: the
    /// code of whichever production reader refused, verbatim.
    ReaderRefused {
        /// The reader's logical key.
        key: String,
        /// The refusal code of the reader that rejected it.
        code: &'static str,
    },
    /// The sibling reader parsed but carries no member named for the
    /// carrier's paired record.
    MemberAbsent {
        /// The reader's logical key.
        reader_key: String,
        /// The paired member name that was not found.
        member: &'static str,
    },
}

impl CarrierBlocker {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::MissingCarrier { .. } => "missing_carrier",
            Self::Unreadable { .. } => "unreadable",
            Self::DispatchRefused { .. } => "dispatch_refused",
            Self::WrongFamily { .. } => "wrong_family",
            Self::UnvalidatedHeader { .. } => "unvalidated_header",
            Self::MissingReader { .. } => "missing_reader",
            Self::ReaderUnreadable { .. } => "reader_unreadable",
            Self::ReaderRefused { .. } => "reader_refused",
            Self::MemberAbsent { .. } => "member_absent",
        }
    }
}

impl fmt::Display for CarrierBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCarrier { expected_key } => {
                write!(f, "no animation carrier at {expected_key}")
            }
            Self::Unreadable { key, reason } => {
                write!(f, "the animation carrier {key} could not be read: {reason}")
            }
            Self::DispatchRefused { key, code } => {
                write!(f, "the animation carrier {key} refused dispatch ({code})")
            }
            Self::WrongFamily { key, family } => write!(
                f,
                "the animation carrier {key} dispatched to the `{}` family",
                family.as_str()
            ),
            Self::UnvalidatedHeader { key } => {
                write!(f, "the animation carrier {key}'s header did not validate")
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
            Self::MemberAbsent { reader_key, member } => {
                write!(
                    f,
                    "the reader archive {reader_key} carries no {member} member"
                )
            }
        }
    }
}

impl std::error::Error for CarrierBlocker {}

/// One scope the layout expects to carry animation data: its directory, the
/// family that makes it mission-critical and what the survey found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurveyedScope {
    /// The scope directory's logical key (`zbd/<group>` or
    /// `zbd/<group>/<mission>`).
    pub scope: String,
    /// Which family this scope must carry.
    pub kind: CarrierKind,
    /// The measured carrier, or the blocker that stands in for it. The row
    /// exists either way — a blocked scope is reported, not dropped.
    pub state: Result<CarrierRecord, CarrierBlocker>,
}

/// A set of carriers that share one payload: their digests are equal, so the
/// containers hold identical bytes. The group is a byte-level fact — whether
/// the identical payloads are also identical *animations* is undecoded and
/// stays unclaimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadFamily {
    /// The SHA-256 every member carrier shares.
    pub sha256: ContentHash,
    /// The container keys holding this payload, sorted.
    pub carriers: Vec<String>,
}

/// An animation-named member of a reader archive that is not a carrier's
/// paired record: the content-root `zbd/zrdr.zbd` holds `anim.zrd` and
/// `map_anims.zrd` on the retail installation, which are animation-family
/// records with no `*.zbd` carrier beside them. Recorded so the family
/// census is complete; whether the original consumes them is unmeasured and
/// stays unclaimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnpairedMember {
    /// The reader archive's logical key the member lives in.
    pub reader_key: String,
    /// The member record.
    pub member: MemberRecord,
}

/// The whole survey: one row per expected scope, plus the animation-named
/// containers the layout does not declare and the animation-named members
/// no carrier pairs with.
#[derive(Clone, Debug)]
pub struct AnimationFamilySurvey {
    /// Every expected scope, in scope-key order.
    pub scopes: Vec<SurveyedScope>,
    /// `.zbd` files named `mis_anim`/`cam_anim` at positions the layout does
    /// not declare. Reported rather than ignored: an unaccounted animation
    /// container is a fact about the installation, and a clean survey has
    /// none.
    pub strays: Vec<String>,
    /// `*anim*` members of the content-root `zbd/zrdr.zbd`, which is no
    /// scope's sibling reader. Empty when the installation has no root
    /// reader — a reader archive at group or mission level is always a
    /// scope's sibling, so only the root can hold unpaired members.
    pub unpaired_members: Vec<UnpairedMember>,
}

impl AnimationFamilySurvey {
    /// The scopes whose carrier was measured.
    pub fn carriers(&self) -> impl Iterator<Item = &CarrierRecord> {
        self.scopes
            .iter()
            .filter_map(|scope| scope.state.as_ref().ok())
    }

    /// The scopes whose carrier could not be validated, with the blocker.
    pub fn blockers(&self) -> impl Iterator<Item = (&str, &CarrierBlocker)> {
        self.scopes.iter().filter_map(|scope| {
            scope
                .state
                .as_ref()
                .err()
                .map(|blocker| (scope.scope.as_str(), blocker))
        })
    }

    /// The scopes of one family.
    pub fn scopes_of(&self, kind: CarrierKind) -> impl Iterator<Item = &SurveyedScope> {
        self.scopes.iter().filter(move |scope| scope.kind == kind)
    }

    /// One scope's row by its directory key.
    #[must_use]
    pub fn scope(&self, key: &str) -> Option<&SurveyedScope> {
        self.scopes.iter().find(|scope| scope.scope == key)
    }

    /// The payload families the carriers group into: one row per distinct
    /// digest, largest family first then by digest for a stable order.
    #[must_use]
    pub fn payload_families(&self) -> Vec<PayloadFamily> {
        let mut by_digest: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut digests: BTreeMap<String, ContentHash> = BTreeMap::new();
        for carrier in self.carriers() {
            let hex = carrier.sha256.to_hex();
            digests.insert(hex.clone(), carrier.sha256);
            by_digest
                .entry(hex)
                .or_default()
                .push(carrier.container_key.clone());
        }
        let mut families: Vec<PayloadFamily> = by_digest
            .into_iter()
            .map(|(hex, carriers)| PayloadFamily {
                sha256: digests.remove(&hex).expect("the digest is keyed by itself"),
                carriers,
            })
            .collect();
        families.sort_by(|left, right| {
            right
                .carriers
                .len()
                .cmp(&left.carriers.len())
                .then_with(|| left.sha256.to_hex().cmp(&right.sha256.to_hex()))
        });
        families
    }

    /// Whether every expected scope validated and nothing stray exists.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.strays.is_empty() && self.scopes.iter().all(|scope| scope.state.is_ok())
    }
}

/// Why a survey could not be produced at all.
#[derive(Debug)]
pub enum AnimationSurveyError {
    /// Production discovery could not read the installation.
    Discovery(install::DiscoveryError),
    /// The campaign layout could not be walked.
    Layout(String),
    /// The installation declares no world group under its zbd root.
    NoWorldGroups,
}

impl fmt::Display for AnimationSurveyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => {
                write!(f, "the installation could not be discovered: {error}")
            }
            Self::Layout(reason) => {
                write!(f, "the campaign layout could not be walked: {reason}")
            }
            Self::NoWorldGroups => {
                f.write_str("the installation declares no world group under its zbd root")
            }
        }
    }
}

impl std::error::Error for AnimationSurveyError {}

/// Surveys every mission-critical animation carrier the installation at
/// `install_root` declares.
///
/// One production discovery and one campaign-layout walk, then per scope:
/// the carrier's header through [`dispatch`], its fingerprint from the
/// manifest, and the sibling reader's `*anim*` members through
/// [`read_version_one_index`] + [`read_reader_archive`]. See the module doc
/// for what is expected, what is measured and what stays unknown.
///
/// # Errors
///
/// [`AnimationSurveyError::Discovery`] or [`AnimationSurveyError::Layout`]
/// when the installation itself cannot be walked, and
/// [`AnimationSurveyError::NoWorldGroups`] when there is nothing to survey.
/// Per-scope failures are **not** errors here — they are
/// [`CarrierBlocker`]s on that scope's row.
pub fn survey_animation_families(
    install_root: &Path,
) -> Result<AnimationFamilySurvey, AnimationSurveyError> {
    let found = install::discover(install_root).map_err(AnimationSurveyError::Discovery)?;
    if found.diagnosis.world_groups.is_empty() {
        return Err(AnimationSurveyError::NoWorldGroups);
    }

    // The expected set, in two halves: every world group is a camera scope,
    // and every launchable directory under one — a directory carrying its
    // own `zrdr.zbd` — plus every campaign-declared mission directory is a
    // mission scope. Keyed by scope so a directory that qualifies twice is
    // one row.
    let mut expected: BTreeMap<String, CarrierKind> = BTreeMap::new();
    for group in &found.diagnosis.world_groups {
        expected.insert(group.logical_key(), CarrierKind::Camera);
    }
    let reader_keys: std::collections::BTreeSet<String> = found
        .manifest
        .files
        .iter()
        .map(|record| record.relative_spelling.logical_key())
        .filter(|key| key.ends_with("/zrdr.zbd"))
        .collect();
    for directory in &found.diagnosis.directories {
        let key = directory.logical_key();
        // A launchable scope is `zbd/<group>/<dir>`: exactly three key
        // components below nothing else.
        let components: Vec<&str> = key.split('/').collect();
        if components.len() == 3 && components[0] == "zbd" {
            let reader = format!("{key}/{READER_ARCHIVE}");
            if reader_keys.contains(&reader) {
                expected.insert(key, CarrierKind::Mission);
            }
        }
    }
    let campaign = campaign_layout(install_root)
        .map_err(|error| AnimationSurveyError::Layout(error.to_string()))?;
    for entry in &campaign {
        // `program_asset` is the mission dir's `zrdr.zbd` as spelled; its
        // parent directory is the scope, whether or not the archive exists.
        let dir = entry
            .mission
            .program_asset
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_ascii_lowercase())
            .unwrap_or_default();
        if !dir.is_empty() {
            expected.entry(dir).or_insert(CarrierKind::Mission);
        }
    }

    // Animation-named containers, found by basename over the manifest so the
    // position check and the validation are one pass.
    let mut carriers: BTreeMap<String, &InstallFileRecord> = BTreeMap::new();
    let mut strays = Vec::new();
    for record in &found.manifest.files {
        let key = record.relative_spelling.logical_key();
        let Some((scope, basename)) = key.rsplit_once('/') else {
            continue;
        };
        let kind = if basename == MISSION_CARRIER {
            CarrierKind::Mission
        } else if basename == CAMERA_CARRIER {
            CarrierKind::Camera
        } else {
            continue;
        };
        // A carrier's position must match the family's observed rule:
        // `mis_anim.zbd` three components deep, `cam_anim.zbd` two — both
        // under `zbd/`. A container anywhere else is still measured (it is a
        // row under its own scope) but is reported as a stray rather than
        // standing in for a scope's carrier.
        let depth = key.split('/').count();
        let declared = (kind == CarrierKind::Mission && depth == 4)
            || (kind == CarrierKind::Camera && depth == 3);
        if !declared || !key.starts_with("zbd/") {
            strays.push(key.clone());
        }
        if !expected.contains_key(scope) {
            // A carrier the expectations did not produce: e.g. a mission
            // directory with `mis_anim.zbd` but no `zrdr.zbd`. It is still
            // measured and still becomes a scope row — a mission dir
            // without its reader is exactly the failure this stage must
            // report, and the member pairing names the missing reader.
            expected.insert(scope.to_owned(), kind);
        }
        carriers.insert(key.clone(), record);
    }

    let mut scopes = Vec::with_capacity(expected.len());
    for (scope, kind) in &expected {
        let container_key = format!("{scope}/{}", kind.container_name());
        let state = match carriers.get(&container_key) {
            Some(record) => measure_carrier(install_root, &found, scope, *kind, record),
            None => Err(CarrierBlocker::MissingCarrier {
                expected_key: container_key.clone(),
            }),
        };
        scopes.push(SurveyedScope {
            scope: scope.clone(),
            kind: *kind,
            state,
        });
    }

    // The content-root reader is no scope's sibling, so its animation-named
    // members pair with no carrier; they are recorded rather than skipped.
    let unpaired_members =
        read_sibling_reader(install_root, &found, "zbd/zrdr.zbd", "zbd/zrdr.zbd")
            .map(|members| {
                members
                    .into_iter()
                    .map(|member| UnpairedMember {
                        reader_key: "zbd/zrdr.zbd".to_owned(),
                        member,
                    })
                    .collect()
            })
            .unwrap_or_default();

    Ok(AnimationFamilySurvey {
        scopes,
        strays,
        unpaired_members,
    })
}

/// Measures one carrier: the header dispatch, the fingerprint, and the
/// sibling reader's `*anim*` members.
fn measure_carrier(
    install_root: &Path,
    found: &Discovery,
    scope: &str,
    kind: CarrierKind,
    record: &InstallFileRecord,
) -> Result<CarrierRecord, CarrierBlocker> {
    let key = record.relative_spelling.logical_key();
    let host = install_root.join(record.relative_spelling.as_str());
    let rule = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .expect("the animation family has a documented signature rule");
    let header =
        read_prefix(&host, rule.required_bytes()).map_err(|error| CarrierBlocker::Unreadable {
            key: key.clone(),
            reason: error.to_string(),
        })?;
    let decision =
        dispatch(ZbdProbe::new(&key, &record.relative_spelling, &header)).map_err(|error| {
            CarrierBlocker::DispatchRefused {
                key: key.clone(),
                code: error.code(),
            }
        })?;
    if decision.family() != ZbdFamily::Animation {
        return Err(CarrierBlocker::WrongFamily {
            key: key.clone(),
            family: decision.family(),
        });
    }
    let version = match decision.header_status() {
        HeaderStatus::Validated { signature, version } => {
            debug_assert_eq!(signature, ANIMATION_SIGNATURE);
            version
        }
        HeaderStatus::Unvalidated { .. } => {
            return Err(CarrierBlocker::UnvalidatedHeader { key: key.clone() });
        }
    };
    let basis = decision.basis();
    let reader_key = format!("{scope}/{READER_ARCHIVE}");
    let animation_members = read_sibling_reader(install_root, found, &reader_key, &key)?;
    if !animation_members
        .iter()
        .any(|member| member.name.eq_ignore_ascii_case(kind.member_name()))
    {
        return Err(CarrierBlocker::MemberAbsent {
            reader_key,
            member: kind.member_name(),
        });
    }

    Ok(CarrierRecord {
        container_key: key,
        scope: scope.to_owned(),
        kind,
        basis,
        version,
        size_bytes: record.size_bytes,
        sha256: record.sha256,
        reader_key,
        animation_members,
    })
}

/// Reads the scope's sibling `zrdr.zbd` and returns every member whose name
/// carries `anim`, paired record first or not — the family's whole census in
/// that archive, not just the name this stage already knew.
fn read_sibling_reader(
    install_root: &Path,
    found: &Discovery,
    reader_key: &str,
    carrier_key: &str,
) -> Result<Vec<MemberRecord>, CarrierBlocker> {
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == reader_key)
        .ok_or_else(|| CarrierBlocker::MissingReader {
            expected_key: reader_key.to_owned(),
        })?;
    let host = install_root.join(record.relative_spelling.as_str());
    let bytes = std::fs::read(&host).map_err(|error| CarrierBlocker::ReaderUnreadable {
        key: reader_key.to_owned(),
        reason: error.to_string(),
    })?;
    let mut context = ParseContext::with_defaults(carrier_key);
    let decision = dispatch(ZbdProbe::new(reader_key, &record.relative_spelling, &bytes)).map_err(
        |error| CarrierBlocker::ReaderRefused {
            key: reader_key.to_owned(),
            code: error.code(),
        },
    )?;
    let index = read_version_one_index(&mut context, decision, &bytes).map_err(|error| {
        CarrierBlocker::ReaderRefused {
            key: reader_key.to_owned(),
            code: error.code(),
        }
    })?;
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).map_err(|error| {
        CarrierBlocker::ReaderRefused {
            key: reader_key.to_owned(),
            code: error.code(),
        }
    })?;
    Ok(archive
        .entries()
        .filter(|entry| {
            String::from_utf8_lossy(entry.name())
                .to_ascii_lowercase()
                .contains("anim")
        })
        .map(|entry| MemberRecord {
            name: String::from_utf8_lossy(entry.name()).into_owned(),
            span: entry.span(),
            sha256: cs_assets::install::sha256(entry.content()),
        })
        .collect())
}

/// The carrier's first `needed` bytes, which is all the documented header
/// rule evaluates.
fn read_prefix(path: &Path, needed: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut header = vec![0_u8; needed];
    file.read_exact(&mut header)?;
    Ok(header)
}
