//! Locating and classifying the campaign's programs (spec F13, stage F13-B;
//! shared contract `docs/contracts/SCRIPT-MISSION.md`).
//!
//! F13-B has to say **where the campaign's programs are and what each one is**
//! before anyone picks a VM, without pretending a name is a decoder. This
//! module is that half:
//!
//! * [`discover_container`] takes one candidate container (provenance label,
//!   installation-relative path, bytes), routes it through the F06 two-key
//!   [`dispatch`], and hands back a [`ContainerDiscovery`]: the programs it
//!   could locate, each with the [`ProgramLocator`] that names its exact
//!   bytes, the [`ProgramKind`] a name or a location supports and the
//!   [`Confidence`] of that claim.
//! * The INTERP loading container is split by the F07 reader into one program
//!   per script body. The role is [`ProgramKind::Loading`] at
//!   [`Confidence::Documented`], because F07 / FORMAT-NOTES document that
//!   container.
//! * A reader archive is split by its own version-one trailer index into one
//!   program per readable member. The member's kind comes from its **name**
//!   and, for an otherwise unnamed member of a mission's archive, from the
//!   mission the path puts it in; every such classification is at
//!   [`Confidence::Inferred`] and its reason says exactly which rule produced
//!   it.
//! * An animation container becomes one program covering the bytes after its
//!   validated 8-byte header, with the kind its basename names
//!   (`cam_anim.zbd` / `mis_anim.zbd`) and no decoded instruction meaning.
//! * A container nothing routes, a family excluded from the script search and
//!   a reader that refuses its own bytes are still reported: as a
//!   [`DiscoveryFinding`], never dropped.
//!
//! Nothing here decodes an instruction. The located program is the input to
//! [`crate::script_raw::ledger::walk_program`]: F13-B's empty ledger refuses
//! the first reached program counter with the mission and the
//! [`ProgramLocator`], and a later stage (F13-C) supplies measured opcodes.
//! Classification is a claim about a name or a path — never that the bytes
//! *are* instructions.
//!
//! Every fixture exercised by `crates/cs_formats/tests/script_raw/` is newly
//! authored synthetic bytes; nothing here is derived from original game data.

use std::fmt;

use cs_types::install::RelativePath;

use super::evidence::{ByteSpan, Confidence};
use super::inventory::EXCLUDED_FAMILY_REASON;
use super::ledger::{OpcodeLedger, ProgramError, ProgramLocator, ReachedOpcode, walk_program};
use crate::interp::read_interp;
use crate::io::ParseContext;
use crate::zbd::{
    ANIMATION_VERSION_OFFSET, HeaderStatus, ZbdDispatch, ZbdFamily, ZbdProbe, dispatch,
    read_reader_archive, read_version_one_index,
};

/// Bytes of an animation container's validated header: the signature word at
/// offset 0 and the version word at offset 4 (`crate::zbd::header`). The bytes
/// after it are the located program; the header itself is container structure.
pub const ANIMATION_HEADER_BYTES: u64 = (ANIMATION_VERSION_OFFSET + 4) as u64;

/// Why a member of a mission's reader archive is classified as a mission
/// program: its location is the only support, and the corpus has not
/// separated a mission program from an animation event yet.
pub const MISSION_MEMBER_REASON: &str = "a member of a mission reader archive; the mission comes from the path and the member's role \
     is not established";

/// Why a reader member at a world-group or content-root archive is retained as
/// a reader entry: no name or location rule names its role.
pub const READER_MEMBER_REASON: &str =
    "a reader-archive member whose basename and location name no role";

/// The name rule for a camera-animation record.
pub const CAM_ANIM_MEMBER: &str = "cam_anim.zrd";

/// The name rule for a mission-animation record.
pub const MIS_ANIM_MEMBER: &str = "mis_anim.zrd";

/// The member names a mission's own reader archive is observed to carry for
/// its control data. Cited from the retail listing in
/// `docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md`; a name
/// rule, not a decode.
pub const MISSION_CONTROL_MEMBERS: [&str; 3] = ["aiv.zrd", "objectives.zrd", "targets.zrd"];

/// Which program family a located program belongs to.
///
/// This is the program's *role*, the weakest claim F13-B can make: it comes
/// from a documented container, an observed member name or a mission path,
/// never from decoding the bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProgramKind {
    /// An INTERP loading-script body (F07 / FORMAT-NOTES document the
    /// container).
    Loading,
    /// A program scoped to a mission: a control member or another member of a
    /// mission's reader archive.
    Mission,
    /// A camera-animation record or container.
    CameraAnimation,
    /// A mission-animation record or container.
    MissionAnimation,
    /// A reader-archive member no rule classifies further.
    ReaderEntry,
    /// Nothing identifies the program.
    Unknown,
}

impl ProgramKind {
    /// Every kind, in report order.
    pub const ALL: [ProgramKind; 6] = [
        Self::Loading,
        Self::Mission,
        Self::CameraAnimation,
        Self::MissionAnimation,
        Self::ReaderEntry,
        Self::Unknown,
    ];

    /// Stable lowercase label for reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Mission => "mission",
            Self::CameraAnimation => "camera_animation",
            Self::MissionAnimation => "mission_animation",
            Self::ReaderEntry => "reader_entry",
            Self::Unknown => "unknown",
        }
    }

    /// Whether the kind is one of the two animation families, which the spec
    /// keeps distinct from each other and from mission programs.
    pub const fn is_animation(self) -> bool {
        matches!(self, Self::CameraAnimation | Self::MissionAnimation)
    }
}

/// One program the discovery located: where its bytes are, which mission it
/// belongs to and the role a name or a path supports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedProgram<'a> {
    mission: Option<String>,
    locator: ProgramLocator,
    kind: ProgramKind,
    confidence: Confidence,
    reason: &'static str,
    bytes: &'a [u8],
}

impl<'a> LocatedProgram<'a> {
    /// The mission the program belongs to (`zbd/<group>/<mission>`), when the
    /// path puts it in one.
    pub fn mission(&self) -> Option<&str> {
        self.mission.as_deref()
    }

    /// Where the program lives: its container, its archive member when it is
    /// one and its byte range. This is the "source location" a failed walk
    /// carries.
    pub const fn locator(&self) -> &ProgramLocator {
        &self.locator
    }

    /// Provenance label of the container the program is in.
    pub fn container(&self) -> &str {
        self.locator.container()
    }

    /// The role a name or a path supports.
    pub const fn kind(&self) -> ProgramKind {
        self.kind
    }

    /// How far the role is supported: `documented` for an INTERP body,
    /// `inferred` for a name or path rule, `unknown` when nothing names it.
    pub const fn confidence(&self) -> Confidence {
        self.confidence
    }

    /// The rule that produced [`Self::kind`], quoted in reports.
    pub const fn reason(&self) -> &'static str {
        self.reason
    }

    /// The program's bytes, borrowed from the container.
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The mission the walk reports: the program's mission when it has one,
    /// otherwise the container label, so a failure always names a scope.
    pub fn mission_label(&self) -> &str {
        self.mission
            .as_deref()
            .unwrap_or_else(|| self.locator.container())
    }

    /// Walks this program with `ledger`, refusing the first reached program
    /// counter the ledger does not name.
    ///
    /// Thin wrapper over [`walk_program`]: the mission, the locator and the
    /// bytes all come from the located program, so a caller cannot pair a
    /// program with the wrong locator.
    ///
    /// # Errors
    ///
    /// Every [`ProgramError`] [`walk_program`] raises; the F13-B minimum
    /// scenario is [`ProgramError::UnknownOpcode`], which carries the mission
    /// and the [`ProgramLocator`] of the offending word.
    pub fn walk(
        &self,
        ledger: &OpcodeLedger,
        word_bytes: u32,
        budget: u32,
    ) -> Result<Vec<ReachedOpcode>, ProgramError> {
        walk_program(
            self.mission_label(),
            &self.locator,
            self.bytes,
            word_bytes,
            ledger,
            budget,
        )
    }
}

/// Something the discovery could not locate or classify, kept rather than
/// dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryFinding {
    /// The F06 two-key dispatch refused the container; the code is
    /// `ZbdDispatchError::code`.
    DispatchRefused {
        /// The refusal code.
        code: &'static str,
    },
    /// The container's family is excluded from the script search.
    Excluded {
        /// The dispatched family.
        family: ZbdFamily,
        /// Why it is excluded.
        reason: &'static str,
    },
    /// The F07 INTERP reader refused the container.
    InterpRefused {
        /// The reader error code.
        code: &'static str,
        /// The offset the reader reported.
        offset: u64,
    },
    /// The container's own member index could not be read.
    IndexRefused {
        /// The index error code.
        code: &'static str,
    },
    /// The reader archive reader refused the container's member table.
    ReaderRefused {
        /// The reader error code.
        code: &'static str,
    },
    /// A declared reader member failed its bounds check and was not located.
    MemberRefused {
        /// How many declared members were unreadable.
        count: usize,
    },
}

impl DiscoveryFinding {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DispatchRefused { .. } => "dispatch_refused",
            Self::Excluded { .. } => "excluded",
            Self::InterpRefused { .. } => "interp_refused",
            Self::IndexRefused { .. } => "index_refused",
            Self::ReaderRefused { .. } => "reader_refused",
            Self::MemberRefused { .. } => "member_refused",
        }
    }
}

impl fmt::Display for DiscoveryFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DispatchRefused { code } => write!(f, "dispatch refused ({code})"),
            Self::Excluded { family, reason } => {
                write!(f, "family `{}` is excluded: {reason}", family.as_str())
            }
            Self::InterpRefused { code, offset } => {
                write!(
                    f,
                    "the interp reader refused the container at {offset} ({code})"
                )
            }
            Self::IndexRefused { code } => write!(f, "the member index was refused ({code})"),
            Self::ReaderRefused { code } => {
                write!(
                    f,
                    "the reader-archive reader refused the container ({code})"
                )
            }
            Self::MemberRefused { count } => {
                write!(
                    f,
                    "{count} declared reader members failed their bounds check"
                )
            }
        }
    }
}

/// One container's located programs and what the discovery could not do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerDiscovery<'a> {
    container: String,
    path: RelativePath,
    family: Option<ZbdFamily>,
    header_status: Option<HeaderStatus>,
    programs: Vec<LocatedProgram<'a>>,
    findings: Vec<DiscoveryFinding>,
}

impl<'a> ContainerDiscovery<'a> {
    /// Provenance label of the container.
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Installation-relative path of the container.
    pub const fn path(&self) -> &RelativePath {
        &self.path
    }

    /// The dispatched family, when the container routed.
    pub const fn family(&self) -> Option<ZbdFamily> {
        self.family
    }

    /// What dispatch established about the container's header, when it routed.
    pub const fn header_status(&self) -> Option<HeaderStatus> {
        self.header_status
    }

    /// The programs the discovery located, in container order.
    pub fn programs(&self) -> &[LocatedProgram<'a>] {
        &self.programs
    }

    /// What the discovery could not locate or classify.
    pub fn findings(&self) -> &[DiscoveryFinding] {
        &self.findings
    }

    /// Whether any program was located.
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// Number of located programs.
    pub fn len(&self) -> usize {
        self.programs.len()
    }
}

/// The mission id a path is scoped to, or `None` when it is not in one.
///
/// The logical key is matched against the observed layout
/// `zbd/<group>/<mission>/<archive>`: exactly four key components. A deeper or
/// shallower path names no mission.
pub fn mission_scope(path: &RelativePath) -> Option<String> {
    let key = path.logical_key();
    let mut components = key.split('/');
    let root = components.next()?;
    let group = components.next()?;
    let mission = components.next()?;
    let archive = components.next()?;
    if root != "zbd"
        || group.is_empty()
        || mission.is_empty()
        || archive.is_empty()
        || components.next().is_some()
    {
        return None;
    }
    Some(format!("{root}/{group}/{mission}"))
}

/// Locates and classifies the programs of one candidate container.
///
/// Never fails: a container dispatch refuses, a family excluded from the
/// script search or a reader that rejects its own bytes is reported through
/// [`ContainerDiscovery::findings`] with an empty program list, never
/// dropped.
///
/// `label` is the provenance string every result and error carries; `path` is
/// the installation-relative path the role rules and the mission scope are
/// matched against; `bytes` is the whole container.
pub fn discover_container<'a>(
    label: &str,
    path: &RelativePath,
    bytes: &'a [u8],
) -> ContainerDiscovery<'a> {
    let mission = mission_scope(path);
    let probe = ZbdProbe::new(label, path, bytes);
    let decision = match dispatch(probe) {
        Ok(decision) => decision,
        Err(error) => {
            return ContainerDiscovery {
                container: label.to_owned(),
                path: path.clone(),
                family: None,
                header_status: None,
                programs: Vec::new(),
                findings: vec![DiscoveryFinding::DispatchRefused { code: error.code() }],
            };
        }
    };
    let family = decision.family();
    let header_status = Some(decision.header_status());
    let mut findings = Vec::new();
    let programs = match family {
        ZbdFamily::Interp => interp_programs(label, bytes, mission.as_deref(), &mut findings),
        ZbdFamily::Reader => {
            reader_programs(label, decision, bytes, mission.as_deref(), &mut findings)
        }
        ZbdFamily::Animation => animation_program(label, path, mission.as_deref(), bytes),
        ZbdFamily::Texture | ZbdFamily::Sound | ZbdFamily::GameZ => {
            findings.push(DiscoveryFinding::Excluded {
                family,
                reason: EXCLUDED_FAMILY_REASON,
            });
            Vec::new()
        }
    };
    ContainerDiscovery {
        container: label.to_owned(),
        path: path.clone(),
        family: Some(family),
        header_status,
        programs,
        findings,
    }
}

/// One program per INTERP script body, each at [`Confidence::Documented`].
fn interp_programs<'a>(
    label: &str,
    bytes: &'a [u8],
    mission: Option<&str>,
    findings: &mut Vec<DiscoveryFinding>,
) -> Vec<LocatedProgram<'a>> {
    let mut context = ParseContext::with_defaults(label);
    let file = match read_interp(&mut context, bytes) {
        Ok(file) => file,
        Err(error) => {
            findings.push(DiscoveryFinding::InterpRefused {
                code: error.code(),
                offset: error.offset(),
            });
            return Vec::new();
        }
    };
    file.scripts()
        .iter()
        .filter_map(|script| {
            let start = u64::from(script.entry.script_offset);
            let end = script.end();
            let span = ByteSpan::from_range(start, end)?;
            let body = bytes.get(start as usize..end as usize)?;
            Some(located(
                mission,
                ProgramLocator::new(label, None, span),
                ProgramKind::Loading,
                Confidence::Documented,
                "the F07 / FORMAT-NOTES INTERP loading container decodes this script body",
                body,
            ))
        })
        .collect()
}

/// One program per readable reader-archive member.
///
/// The container's own version-one trailer is the index (F06), and the reader
/// archive reader validates the member extents. The program bytes are then
/// sliced straight out of `bytes` by member span, so the result borrows the
/// container and not the (local) index.
fn reader_programs<'a>(
    label: &str,
    decision: ZbdDispatch<'_>,
    bytes: &'a [u8],
    mission: Option<&str>,
    findings: &mut Vec<DiscoveryFinding>,
) -> Vec<LocatedProgram<'a>> {
    let mut context = ParseContext::with_defaults(label);
    let index = match read_version_one_index(&mut context, decision, bytes) {
        Ok(index) => index,
        Err(error) => {
            findings.push(DiscoveryFinding::IndexRefused { code: error.code() });
            return Vec::new();
        }
    };
    let table = index.member_table();
    let archive = match read_reader_archive(&mut context, &table, index.data()) {
        Ok(archive) => archive,
        Err(error) => {
            findings.push(DiscoveryFinding::ReaderRefused { code: error.code() });
            return Vec::new();
        }
    };
    if archive.failures() > 0 {
        findings.push(DiscoveryFinding::MemberRefused {
            count: archive.failures(),
        });
    }
    let mut programs = Vec::new();
    for entry in archive.entries() {
        let span = ByteSpan::new(entry.span().offset, entry.span().length);
        let Some(body) = bytes.get(span.offset as usize..span.end() as usize) else {
            findings.push(DiscoveryFinding::MemberRefused { count: 1 });
            continue;
        };
        let (kind, confidence, reason) = classify_reader_member(entry.name(), mission);
        programs.push(located(
            mission,
            ProgramLocator::new(
                label,
                Some(String::from_utf8_lossy(entry.name()).into_owned()),
                span,
            ),
            kind,
            confidence,
            reason,
            body,
        ));
    }
    programs
}

/// One program covering the bytes after an animation container's header.
fn animation_program<'a>(
    label: &str,
    path: &RelativePath,
    mission: Option<&str>,
    bytes: &'a [u8],
) -> Vec<LocatedProgram<'a>> {
    // Dispatch validated the signature, so the header is present; the clamp
    // keeps a hostile length from indexing past the container.
    let start = usize::try_from(ANIMATION_HEADER_BYTES)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let span = ByteSpan::new(start as u64, (bytes.len() - start) as u64);
    let key = path.logical_key();
    let basename = key.rsplit('/').next().unwrap_or(&key);
    let (kind, confidence, reason) = match basename {
        "cam_anim.zbd" => (
            ProgramKind::CameraAnimation,
            Confidence::Inferred,
            "the observed camera-animation archive name",
        ),
        "mis_anim.zbd" => (
            ProgramKind::MissionAnimation,
            Confidence::Inferred,
            "the observed mission-animation archive name",
        ),
        _ => (
            ProgramKind::Unknown,
            Confidence::Unknown,
            "an animation-family container whose name matches no observed role",
        ),
    };
    vec![located(
        mission,
        ProgramLocator::new(label, None, span),
        kind,
        confidence,
        reason,
        &bytes[start..],
    )]
}

/// Classifies one reader-archive member by its name and its mission scope.
fn classify_reader_member(
    name: &[u8],
    mission: Option<&str>,
) -> (ProgramKind, Confidence, &'static str) {
    let lower: Vec<u8> = name.iter().map(u8::to_ascii_lowercase).collect();
    if lower == CAM_ANIM_MEMBER.as_bytes() {
        (
            ProgramKind::CameraAnimation,
            Confidence::Inferred,
            "the observed camera-animation member name",
        )
    } else if lower == MIS_ANIM_MEMBER.as_bytes() {
        (
            ProgramKind::MissionAnimation,
            Confidence::Inferred,
            "the observed mission-animation member name",
        )
    } else if MISSION_CONTROL_MEMBERS
        .iter()
        .any(|known| known.as_bytes() == lower.as_slice())
    {
        (
            ProgramKind::Mission,
            Confidence::Inferred,
            "an observed mission-control member name",
        )
    } else if mission.is_some() {
        (
            ProgramKind::Mission,
            Confidence::Inferred,
            MISSION_MEMBER_REASON,
        )
    } else {
        (
            ProgramKind::ReaderEntry,
            Confidence::Inferred,
            READER_MEMBER_REASON,
        )
    }
}

/// Builds one located program.
fn located<'a>(
    mission: Option<&str>,
    locator: ProgramLocator,
    kind: ProgramKind,
    confidence: Confidence,
    reason: &'static str,
    bytes: &'a [u8],
) -> LocatedProgram<'a> {
    LocatedProgram {
        mission: mission.map(str::to_owned),
        locator,
        kind,
        confidence,
        reason,
        bytes,
    }
}
