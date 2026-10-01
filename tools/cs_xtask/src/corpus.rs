//! The F62-A corpus contract: synthetic/private separation and the per
//! container oracle the corpus runs against
//! (`specs/F62-differential-corpus-fuzzing-and-regression-closure.md`,
//! stage `### F62-A`).
//!
//! Three different inputs share one corpus and must never be confused:
//!
//! * **Synthetic** inputs are authored bytes, redistributable, produced by
//!   named byte-builders in `crates/cs_formats/tests/corpus/` or by the
//!   committed text fixtures under `fixtures/synthetic/`. Nothing in this
//!   class is derived from the original installation.
//! * **Private** inputs are members of the read-only original installation
//!   (`$CS_GAME_DIR`, resolved through `--private-root`). They are selected
//!   by an install-relative selector, counted and fingerprinted at audit
//!   time, and never committed — the audit gate checks the tracked file
//!   list to prove none landed in Git.
//! * **Regression** inputs are minimized reproductions of fixed bugs. They
//!   may only use a synthetic-clean source (a builder or a committed
//!   fixture): when shrinking a crashing input cannot remove protected
//!   bytes the seed stays private, per spec non-negotiable #2.
//!
//! The **oracle contract** records, per container entrypoint, what a parse
//! of each input class is allowed to do. For truncation (spec AC01) the
//! contract is [`TruncationOracle`]: prefix-refusing containers must refuse
//! every prefix that cuts inside a required span with a structured,
//! bounded diagnostic — never a panic, never a silent success past the
//! cut. The binding between a manifest entry and the production parser it
//! names lives in `crates/cs_formats/tests/corpus/`, which resolves the
//! declared boundary kinds to real byte offsets and runs the oracle.
//!
//! This module is the single source of truth for the manifest: the
//! `cs_xtask corpus` commands report it and audit the separation rules,
//! and the `accept_f62_a_*` tests bind every declared container to its
//! production parser. It carries no game logic and no parser dependency.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where a corpus input is allowed to live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CorpusClass {
    /// Authored, redistributable bytes: byte-builders in the corpus test
    /// registry or committed files under `fixtures/synthetic/`.
    Synthetic,
    /// Members of the read-only original installation. Resolved against a
    /// private root at audit time; never committed, never copied.
    Private,
    /// A minimized reproduction of a fixed bug. Synthetic-clean sources
    /// only; a reproducer that still carries protected bytes stays
    /// `Private` instead.
    Regression,
}

impl CorpusClass {
    /// Stable lowercase identifier, for logs and the manifest report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Synthetic => "synthetic",
            Self::Private => "private",
            Self::Regression => "regression",
        }
    }
}

impl fmt::Display for CorpusClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A named byte boundary of a container layout, per spec AC01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BoundaryKind {
    /// The fixed-size leading header every truncation corpus starts with.
    FixedHeader,
    /// A count- or offset-driven table whose extent depends on the
    /// content (record tables, index entries, mip chains, sections).
    VariableTable,
    /// A fixed-size structure read from the end of the container.
    Trailer,
    /// One element of a fixed-width repeated sequence (opcode words, PCM
    /// frames): a cut strictly inside the element must be refused, while a
    /// cut on an element boundary is a shorter valid input and may parse.
    Frame,
    /// Bytes the entrypoint is allowed to leave unread (tolerated tails,
    /// member payloads a header-only dispatch does not open). Cuts here
    /// may still parse; the oracle only demands a bounded outcome.
    Slack,
}

impl BoundaryKind {
    /// Stable lowercase identifier, for logs and the manifest report.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FixedHeader => "fixed_header",
            Self::VariableTable => "variable_table",
            Self::Trailer => "trailer",
            Self::Frame => "frame",
            Self::Slack => "slack",
        }
    }
}

impl fmt::Display for BoundaryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a truncation of the container is allowed to make its parser do.
///
/// The corpus binding resolves the abstract contract to real byte offsets
/// against its synthetic fixture: every [`BoundaryKind`] the spec lists
/// must appear in the fixture's span map, and required (non-`Slack`) spans
/// must actually be load-bearing — a cut inside any of them is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruncationOracle {
    /// Every prefix that cuts inside a required span must be refused with
    /// a structured error; the complete buffer must parse; a cut inside a
    /// `Slack` span may do either but must stay a bounded outcome.
    PrefixRefusal,
    /// The container is not self-framed: its extents are described by an
    /// input the bytes do not carry (a member table, an index), so a cut
    /// cannot be refused at the top level. The oracle is that the parse
    /// stays bounded and reports the loss in its structured status — e.g.
    /// a member extent that no longer fits degrades its member row and the
    /// container status — never a panic and never a silent read past the
    /// end.
    ExtentStatus {
        /// Why a hard refusal is impossible here.
        rationale: &'static str,
    },
    /// The input is not a prefix-closed container at all (a line scan
    /// accepts a shorter document, an inventory absorbs anything).
    /// Truncation is not a refusal oracle for it; the rationale is recorded
    /// so a later stage cannot silently drop the entry.
    NotApplicable {
        /// Why truncation cannot be a refusal oracle here.
        rationale: &'static str,
    },
}

/// One known container surface: a production parse entrypoint plus the
/// boundary kinds its layout has.
pub struct ContainerSpec {
    /// Stable corpus identifier, e.g. `"rof.directory"`.
    pub id: &'static str,
    /// The production entrypoint the corpus binds, as a fully qualified
    /// name, e.g. `"cs_formats::read_directory"`. The binding asserts the
    /// symbol exists by calling it; the string is for reports.
    pub entrypoint: &'static str,
    /// The truncation contract for this entrypoint.
    pub truncation: TruncationOracle,
    /// Every [`BoundaryKind`] this container's layout has. The synthetic
    /// fixture must produce at least one resolved span of each kind.
    pub boundaries: &'static [BoundaryKind],
    /// The fuzz target under `crates/cs_formats/fuzz/fuzz_targets/` that
    /// feeds this entrypoint arbitrary bytes, without the `.rs` suffix.
    /// `None` for `NotApplicable` entrypoints that take no byte slice.
    pub fuzz_target: Option<&'static str>,
}

/// Where the bytes of a [`CorpusEntry`] come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusSource {
    /// Produced by the byte-builder of this name in the corpus test
    /// registry (`crates/cs_formats/tests/corpus/`). The bytes never touch
    /// disk and carry no original content.
    Builder {
        /// Builder id the test registry resolves, e.g. `"rof.directory.minimal"`.
        builder: &'static str,
    },
    /// A committed, redistributable fixture file, repo-relative. The audit
    /// requires it to be tracked and to live under `fixtures/synthetic/`.
    CommittedFixture {
        /// Repo-relative path, e.g. `"fixtures/synthetic/rectangular.bm"`.
        path: &'static str,
    },
    /// Members of the read-only original installation, selected by file
    /// extension at audit time. Never committed; the audit counts and
    /// fingerprints the matched members under the private root.
    PrivateInstall {
        /// Lowercase extensions (no dot) an install member must have, e.g.
        /// `&["rof"]`. Matching is case-insensitive on the file name.
        extensions: &'static [&'static str],
    },
    /// A corpus input kept only under the private corpus directory because
    /// it still carries original bytes — e.g. a crash seed that could not
    /// be minimized into clean bytes (spec non-negotiable #2). The path is
    /// relative to the private root and is never resolved inside the repo.
    PrivateSeed {
        /// Path relative to the private corpus root.
        path: &'static str,
        /// SHA-256 the member must fingerprint to, hex-free: the audit
        /// computes it; `None` until a stage records the first seed.
        sha256: Option<[u8; 32]>,
    },
}

/// What a parse of the entry's bytes as-is must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectedOutcome {
    /// The entrypoint must accept the input.
    Accept,
    /// The entrypoint must refuse the input with a structured, bounded
    /// diagnostic (a deliberately invalid fixture, e.g. `bad-version.interp`).
    Refuse,
}

/// One corpus input.
pub struct CorpusEntry {
    /// Stable corpus identifier, e.g. `"synthetic/committed/rectangular.bm"`.
    pub id: &'static str,
    /// The [`ContainerSpec::id`] this entry feeds.
    pub container: &'static str,
    /// Which corpus class it belongs to.
    pub class: CorpusClass,
    /// Where its bytes come from.
    pub source: CorpusSource,
    /// What parsing it as-is must do.
    pub expected: ExpectedOutcome,
    /// Provenance note: for `Regression` entries this names the resolved
    /// bug/report it minimizes; for private selectors it names the role the
    /// members play. Empty is a manifest error.
    pub note: &'static str,
}

// ---------------------------------------------------------------------------
// The manifest
// ---------------------------------------------------------------------------

const REFUSAL: TruncationOracle = TruncationOracle::PrefixRefusal;

const HEADER_TABLE: &[BoundaryKind] = &[BoundaryKind::FixedHeader, BoundaryKind::VariableTable];
const HEADER_TABLE_SLACK: &[BoundaryKind] = &[
    BoundaryKind::FixedHeader,
    BoundaryKind::VariableTable,
    BoundaryKind::Slack,
];
const HEADER_SLACK: &[BoundaryKind] = &[BoundaryKind::FixedHeader, BoundaryKind::Slack];
const TABLE_SLACK: &[BoundaryKind] = &[BoundaryKind::VariableTable, BoundaryKind::Slack];
const TABLE_TRAILER: &[BoundaryKind] = &[BoundaryKind::VariableTable, BoundaryKind::Trailer];
const FRAMES: &[BoundaryKind] = &[BoundaryKind::Frame];
const SLACK_ONLY: &[BoundaryKind] = &[BoundaryKind::Slack];

/// The canonical list of known container entrypoints.
///
/// One spec per public `cs_formats` parser that consumes a container's
/// bytes — or, where the spec says so, a structured member/listing input
/// derived from those bytes. Second-stage transforms that take already
/// parsed values (`decode_interp` records, `ImageDescriptor` inputs,
/// `read_placeholders`, `decode_strip`) are deliberately not containers;
/// the findings document records them as covered by their own stage's
/// tests. A spec whose id no corpus binding resolves — or a binding that
/// names no declared container — fails `accept_f62_a_registry_*`, so the
/// manifest and the test registry cannot drift apart.
pub fn containers() -> &'static [ContainerSpec] {
    CONTAINERS
}

static CONTAINERS: &[ContainerSpec] = &[
    ContainerSpec {
        id: "rof.directory",
        entrypoint: "cs_formats::read_directory",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("rof_directory"),
    },
    ContainerSpec {
        id: "rof.tree",
        entrypoint: "cs_formats::read_tree",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("rof_tree"),
    },
    ContainerSpec {
        id: "rof.member",
        entrypoint: "cs_formats::read_member",
        truncation: REFUSAL,
        boundaries: TABLE_SLACK,
        fuzz_target: Some("rof_member"),
    },
    ContainerSpec {
        id: "zbd.dispatch",
        entrypoint: "cs_formats::zbd::dispatch",
        truncation: REFUSAL,
        boundaries: HEADER_SLACK,
        fuzz_target: Some("zbd_dispatch"),
    },
    ContainerSpec {
        id: "zbd.reader_archive",
        entrypoint: "cs_formats::zbd::read_reader_archive",
        truncation: TruncationOracle::ExtentStatus {
            rationale: "the container carries no self-describing extent; the \
                        member table is an input, so a cut can only degrade the \
                        member rows it no longer covers",
        },
        boundaries: TABLE_SLACK,
        fuzz_target: Some("zbd_reader_archive"),
    },
    ContainerSpec {
        id: "zbd.list_members",
        entrypoint: "cs_formats::zbd::list_members",
        truncation: TruncationOracle::ExtentStatus {
            rationale: "the shared member-listing layer the family readers \
                        wrap: extents arrive via the MemberTable input, so a \
                        cut can only degrade the member rows it no longer \
                        covers",
        },
        boundaries: TABLE_SLACK,
        fuzz_target: Some("zbd_list_members"),
    },
    ContainerSpec {
        id: "zbd.sound_archive",
        entrypoint: "cs_formats::zbd::read_sound_archive",
        truncation: TruncationOracle::ExtentStatus {
            rationale: "same as zbd.reader_archive: extents come from the \
                        member table input, not from the bytes",
        },
        boundaries: TABLE_SLACK,
        fuzz_target: Some("zbd_sound_archive"),
    },
    ContainerSpec {
        id: "zbd.trailer_index",
        entrypoint: "cs_formats::zbd::trailer::read_version_one_index",
        truncation: REFUSAL,
        boundaries: TABLE_TRAILER,
        fuzz_target: Some("zbd_trailer_index"),
    },
    ContainerSpec {
        id: "zbd.wave_header",
        entrypoint: "cs_formats::zbd::read_wave_header",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("zbd_wave_header"),
    },
    ContainerSpec {
        id: "interp.container",
        entrypoint: "cs_formats::read_interp",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("interp_container"),
    },
    ContainerSpec {
        id: "interp.decode",
        entrypoint: "cs_formats::decode_interp",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("interp_decode"),
    },
    ContainerSpec {
        id: "bm.image",
        entrypoint: "cs_formats::read_bm",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE_SLACK,
        fuzz_target: Some("bm_image"),
    },
    ContainerSpec {
        id: "texture.bmp",
        entrypoint: "cs_formats::read_bmp",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("texture_bmp"),
    },
    ContainerSpec {
        id: "texture.tga",
        entrypoint: "cs_formats::read_tga",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("texture_tga"),
    },
    ContainerSpec {
        id: "texture.zbd_package",
        entrypoint: "cs_formats::read_zbd_textures",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE,
        fuzz_target: Some("texture_zbd_package"),
    },
    ContainerSpec {
        id: "gamez.meshes",
        entrypoint: "cs_formats::read_gamez_meshes",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE_SLACK,
        fuzz_target: Some("gamez_meshes"),
    },
    ContainerSpec {
        id: "gamez.materials",
        entrypoint: "cs_formats::read_gamez_materials",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE_SLACK,
        fuzz_target: Some("gamez_materials"),
    },
    ContainerSpec {
        id: "pe.layout",
        entrypoint: "cs_formats::read_pe_layout",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE_SLACK,
        fuzz_target: Some("pe_layout"),
    },
    ContainerSpec {
        id: "pe.resources",
        entrypoint: "cs_formats::read_pe_resources",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE_SLACK,
        fuzz_target: Some("pe_resources"),
    },
    ContainerSpec {
        id: "legacy.profile",
        entrypoint: "cs_formats::legacy_profile::read_legacy_profile",
        truncation: REFUSAL,
        boundaries: HEADER_TABLE_SLACK,
        fuzz_target: Some("legacy_profile"),
    },
    ContainerSpec {
        id: "script.program",
        entrypoint: "cs_formats::script_raw::walk_program",
        truncation: REFUSAL,
        boundaries: FRAMES,
        fuzz_target: Some("script_program"),
    },
    ContainerSpec {
        id: "text.lines",
        entrypoint: "cs_formats::text::scan_lines",
        truncation: TruncationOracle::NotApplicable {
            rationale: "a line scan is total over any byte slice: a prefix \
                        is a shorter valid document, never a refusal",
        },
        boundaries: SLACK_ONLY,
        fuzz_target: Some("text_lines"),
    },
    ContainerSpec {
        id: "text.keyed_list",
        entrypoint: "cs_formats::text::read_keyed_list",
        truncation: TruncationOracle::NotApplicable {
            rationale: "the keyed-field-list reader never fails on content; \
                        only an allocation budget can refuse it",
        },
        boundaries: SLACK_ONLY,
        fuzz_target: Some("text_keyed_list"),
    },
    ContainerSpec {
        id: "text.resource_header",
        entrypoint: "cs_formats::text::read_resource_header",
        truncation: TruncationOracle::NotApplicable {
            rationale: "no fixed header: line classifications accept a prefix \
                        as a shorter header",
        },
        boundaries: SLACK_ONLY,
        fuzz_target: Some("text_resource_header"),
    },
    ContainerSpec {
        id: "script.discovery",
        entrypoint: "cs_formats::script_raw::discover_container",
        truncation: TruncationOracle::NotApplicable {
            rationale: "discovery absorbs any bytes: a refused dispatch is a \
                        finding and an opaque record, not an error",
        },
        boundaries: SLACK_ONLY,
        fuzz_target: Some("script_discovery"),
    },
    ContainerSpec {
        id: "script.inventory",
        entrypoint: "cs_formats::script_raw::inventory_scripts",
        truncation: TruncationOracle::NotApplicable {
            rationale: "the inventory maps every source to candidate records; \
                        malformed input degrades to opaque, never to refusal",
        },
        boundaries: SLACK_ONLY,
        fuzz_target: Some("script_inventory"),
    },
];

/// The canonical corpus entry list.
///
/// Synthetic builders are resolved by name in the corpus test registry;
/// committed fixtures are the redistributable files under
/// `fixtures/synthetic/`; private selectors name original-installation
/// member sets the private corpus exercises.
pub fn entries() -> &'static [CorpusEntry] {
    ENTRIES
}

/// One `Builder` entry per declared container: `builder` is the registry
/// id and `note` the fixture description. Generated so the manifest and
/// the entry list cannot drift apart.
const fn synthetic(container: &'static str, note: &'static str) -> CorpusEntry {
    CorpusEntry {
        id: container,
        container,
        class: CorpusClass::Synthetic,
        source: CorpusSource::Builder { builder: container },
        expected: ExpectedOutcome::Accept,
        note,
    }
}

static ENTRIES: &[CorpusEntry] = &[
    // Synthetic minimal fixtures, one per container. The builders live in
    // crates/cs_formats/tests/corpus/ under the container id and produce
    // their boundary span maps alongside the bytes.
    synthetic("rof.directory", "smallest valid ROF directory block"),
    synthetic(
        "rof.tree",
        "smallest valid ROF tree: root block plus one file member",
    ),
    synthetic(
        "rof.member",
        "the tree fixture read through its member record",
    ),
    synthetic(
        "zbd.dispatch",
        "signature-family header a probe dispatch accepts",
    ),
    synthetic("zbd.reader_archive", "one-member reader archive"),
    synthetic(
        "zbd.list_members",
        "the same member payload through the shared listing layer",
    ),
    synthetic("zbd.sound_archive", "one-member sound archive"),
    synthetic(
        "zbd.trailer_index",
        "version-one trailer index with one member entry",
    ),
    synthetic(
        "zbd.wave_header",
        "smallest RIFF/WAVE header the member reader accepts",
    ),
    synthetic("interp.container", "one-script INTERP container"),
    synthetic(
        "interp.decode",
        "the same container through the token decoder",
    ),
    synthetic("bm.image", "1x1 BM: header plus five planes"),
    synthetic("texture.bmp", "smallest BMP in the observed subset"),
    synthetic("texture.tga", "smallest TGA in the observed subset"),
    synthetic("texture.zbd_package", "one-texture ZBD texture package"),
    synthetic("gamez.meshes", "one-stub-mesh GameZ container"),
    synthetic(
        "gamez.materials",
        "one-texture GameZ container with a full material section",
    ),
    synthetic(
        "pe.layout",
        "smallest DOS+PE image the layout reader accepts",
    ),
    synthetic(
        "pe.resources",
        "smallest PE carrying one resource string block",
    ),
    synthetic(
        "legacy.profile",
        "one-record document in the designed fixture layout",
    ),
    synthetic("script.program", "a three-word program of known opcodes"),
    synthetic("text.lines", "a small multi-line document"),
    synthetic("text.keyed_list", "a small keyed-field-list document"),
    synthetic("text.resource_header", "a small resource-header document"),
    synthetic("script.discovery", "a container handed to discovery"),
    synthetic("script.inventory", "a two-source script inventory"),
    // Committed redistributable fixtures (authored by
    // tools/make_synthetic_fixtures.py; never original bytes).
    CorpusEntry {
        id: "synthetic/committed/flat-uncompressed.rof",
        container: "rof.directory",
        class: CorpusClass::Synthetic,
        source: CorpusSource::CommittedFixture {
            path: "fixtures/synthetic/flat-uncompressed.rof",
        },
        expected: ExpectedOutcome::Accept,
        note: "shared golden ROF directory block",
    },
    CorpusEntry {
        id: "synthetic/committed/rectangular.bm",
        container: "bm.image",
        class: CorpusClass::Synthetic,
        source: CorpusSource::CommittedFixture {
            path: "fixtures/synthetic/rectangular.bm",
        },
        expected: ExpectedOutcome::Accept,
        note: "shared golden BM image",
    },
    CorpusEntry {
        id: "synthetic/committed/truncated.bm",
        container: "bm.image",
        class: CorpusClass::Synthetic,
        source: CorpusSource::CommittedFixture {
            path: "fixtures/synthetic/truncated.bm",
        },
        expected: ExpectedOutcome::Refuse,
        note: "deliberately one byte short; exercises the refusal oracle",
    },
    CorpusEntry {
        id: "synthetic/committed/synthetic.interp",
        container: "interp.container",
        class: CorpusClass::Synthetic,
        source: CorpusSource::CommittedFixture {
            path: "fixtures/synthetic/synthetic.interp",
        },
        expected: ExpectedOutcome::Accept,
        note: "shared golden INTERP container",
    },
    CorpusEntry {
        id: "synthetic/committed/bad-version.interp",
        container: "interp.container",
        class: CorpusClass::Synthetic,
        source: CorpusSource::CommittedFixture {
            path: "fixtures/synthetic/bad-version.interp",
        },
        expected: ExpectedOutcome::Refuse,
        note: "version 7 fails dispatch; exercises the refusal oracle",
    },
    // Private selectors: members of the read-only original installation
    // the private corpus exercises. Top-level members only here; containers
    // nested inside archives are reached through the container readers by
    // the runner stage (F62-B).
    CorpusEntry {
        id: "private/install/rof-archives",
        container: "rof.tree",
        class: CorpusClass::Private,
        source: CorpusSource::PrivateInstall {
            extensions: &["rof"],
        },
        expected: ExpectedOutcome::Accept,
        note: "the installation's asset archives (crimson.rof, crimptch.rof)",
    },
    CorpusEntry {
        id: "private/install/zbd-containers",
        container: "zbd.dispatch",
        class: CorpusClass::Private,
        source: CorpusSource::PrivateInstall {
            extensions: &["zbd"],
        },
        expected: ExpectedOutcome::Accept,
        note: "every top-level ZBD family container in the installation",
    },
    CorpusEntry {
        id: "private/install/tga-images",
        container: "texture.tga",
        class: CorpusClass::Private,
        source: CorpusSource::PrivateInstall {
            extensions: &["tga"],
        },
        expected: ExpectedOutcome::Accept,
        note: "loose TGA images at install root",
    },
    CorpusEntry {
        id: "private/install/pe-images",
        container: "pe.layout",
        class: CorpusClass::Private,
        source: CorpusSource::PrivateInstall {
            extensions: &["exe", "dll"],
        },
        expected: ExpectedOutcome::Accept,
        note: "executables/DLLs the PE layout and resource readers cover",
    },
];

/// Looks up a container spec by [`ContainerSpec::id`].
pub fn find_container(id: &str) -> Option<&'static ContainerSpec> {
    containers().iter().find(|spec| spec.id == id)
}

// ---------------------------------------------------------------------------
// Manifest verification
// ---------------------------------------------------------------------------

/// A way the declared manifest contradicts the separation contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// Two containers share an id.
    DuplicateContainerId(String),
    /// Two entries share an id.
    DuplicateEntryId(String),
    /// An entry names a container the manifest does not declare.
    UnknownContainer { entry: String, container: String },
    /// An entry's provenance note is empty.
    MissingNote(String),
    /// A `Synthetic`/`Regression` entry uses a private source, or the
    /// reverse: classes and sources must not cross.
    ClassSourceMismatch { entry: String },
    /// A `CommittedFixture` path is not under the redistributable root.
    FixtureOutsideSyntheticRoot { entry: String, path: String },
    /// A `Regression` entry does not name the bug it minimizes.
    RegressionWithoutBugRef { entry: String },
    /// A `PrivateInstall` selector carries an empty extension list or an
    /// extension that is not lowercase alnum.
    BadPrivateSelector { entry: String },
    /// A `PrefixRefusal` container lists `Slack` as its only boundary kind:
    /// nothing would ever be required.
    NoRequiredBoundary { container: String },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateContainerId(id) => write!(f, "duplicate container id {id:?}"),
            Self::DuplicateEntryId(id) => write!(f, "duplicate corpus entry id {id:?}"),
            Self::UnknownContainer { entry, container } => {
                write!(f, "entry {entry:?} names unknown container {container:?}")
            }
            Self::MissingNote(entry) => write!(f, "entry {entry:?} has an empty note"),
            Self::ClassSourceMismatch { entry } => write!(
                f,
                "entry {entry:?}: corpus class and source disagree about privacy"
            ),
            Self::FixtureOutsideSyntheticRoot { entry, path } => write!(
                f,
                "entry {entry:?}: committed fixture {path:?} is outside fixtures/synthetic/"
            ),
            Self::RegressionWithoutBugRef { entry } => write!(
                f,
                "regression entry {entry:?} does not name the bug it minimizes"
            ),
            Self::BadPrivateSelector { entry } => write!(
                f,
                "entry {entry:?}: private selector has an empty or malformed extension list"
            ),
            Self::NoRequiredBoundary { container } => write!(
                f,
                "container {container:?}: only Slack boundaries declared, nothing required"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Repo-relative prefix every committed synthetic fixture must live under.
pub const SYNTHETIC_FIXTURE_ROOT: &str = "fixtures/synthetic/";

/// Checks the manifest against the separation contract.
///
/// Returns every violation found, so a report lists them all rather than
/// the first.
pub fn verify_manifest() -> Vec<ManifestError> {
    let mut errors = Vec::new();

    let mut container_ids = BTreeSet::new();
    for spec in containers() {
        if !container_ids.insert(spec.id) {
            errors.push(ManifestError::DuplicateContainerId(spec.id.to_owned()));
        }
        if spec.truncation == TruncationOracle::PrefixRefusal
            && spec
                .boundaries
                .iter()
                .all(|kind| *kind == BoundaryKind::Slack)
        {
            errors.push(ManifestError::NoRequiredBoundary {
                container: spec.id.to_owned(),
            });
        }
    }

    let mut entry_ids = BTreeSet::new();
    for entry in entries() {
        if !entry_ids.insert(entry.id) {
            errors.push(ManifestError::DuplicateEntryId(entry.id.to_owned()));
        }
        if !container_ids.contains(entry.container) {
            errors.push(ManifestError::UnknownContainer {
                entry: entry.id.to_owned(),
                container: entry.container.to_owned(),
            });
        }
        if entry.note.is_empty() {
            errors.push(ManifestError::MissingNote(entry.id.to_owned()));
        }
        match entry.class {
            CorpusClass::Synthetic | CorpusClass::Regression => match entry.source {
                CorpusSource::Builder { .. } => {}
                CorpusSource::CommittedFixture { path } => {
                    if !path.starts_with(SYNTHETIC_FIXTURE_ROOT) {
                        errors.push(ManifestError::FixtureOutsideSyntheticRoot {
                            entry: entry.id.to_owned(),
                            path: path.to_owned(),
                        });
                    }
                }
                CorpusSource::PrivateInstall { .. } | CorpusSource::PrivateSeed { .. } => {
                    errors.push(ManifestError::ClassSourceMismatch {
                        entry: entry.id.to_owned(),
                    });
                }
            },
            CorpusClass::Private => match entry.source {
                CorpusSource::PrivateInstall { extensions } => {
                    let malformed = extensions.is_empty()
                        || extensions.iter().any(|ext| {
                            ext.is_empty()
                                || !ext
                                    .chars()
                                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                        });
                    if malformed {
                        errors.push(ManifestError::BadPrivateSelector {
                            entry: entry.id.to_owned(),
                        });
                    }
                }
                CorpusSource::PrivateSeed { .. } => {}
                CorpusSource::Builder { .. } | CorpusSource::CommittedFixture { .. } => {
                    errors.push(ManifestError::ClassSourceMismatch {
                        entry: entry.id.to_owned(),
                    });
                }
            },
        }
        if entry.class == CorpusClass::Regression
            && !(entry.note.starts_with('#') || entry.note.contains("bug"))
        {
            errors.push(ManifestError::RegressionWithoutBugRef {
                entry: entry.id.to_owned(),
            });
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

/// Aggregate counts the report and audit share.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CorpusCounts {
    /// Declared container entrypoints.
    pub containers: usize,
    /// Containers whose truncation oracle is `PrefixRefusal`.
    pub prefix_refusing: usize,
    /// Corpus entries by class.
    pub synthetic: usize,
    /// Private selector entries.
    pub private: usize,
    /// Regression entries.
    pub regression: usize,
    /// Declared fuzz targets.
    pub fuzz_targets: usize,
}

/// Counts the manifest's own declaration, for reports that must state real
/// numbers rather than aspirational ones (spec non-negotiable #5).
pub fn manifest_counts() -> CorpusCounts {
    let mut counts = CorpusCounts {
        containers: containers().len(),
        ..CorpusCounts::default()
    };
    for spec in containers() {
        if spec.truncation == TruncationOracle::PrefixRefusal {
            counts.prefix_refusing += 1;
        }
        if spec.fuzz_target.is_some() {
            counts.fuzz_targets += 1;
        }
    }
    for entry in entries() {
        match entry.class {
            CorpusClass::Synthetic => counts.synthetic += 1,
            CorpusClass::Private => counts.private += 1,
            CorpusClass::Regression => counts.regression += 1,
        }
    }
    counts
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The manifest as a deterministic JSON document, for reports that need a
/// machine-readable corpus declaration (F62-C completeness reports consume
/// this rather than re-deriving it).
pub fn manifest_json() -> String {
    let mut out = String::from("{\n  \"containers\": [");
    for (index, spec) in containers().iter().enumerate() {
        let boundaries: Vec<String> = spec
            .boundaries
            .iter()
            .map(|kind| json_string(kind.as_str()))
            .collect();
        let truncation = match spec.truncation {
            TruncationOracle::PrefixRefusal => json_string("prefix_refusal"),
            TruncationOracle::ExtentStatus { rationale } => format!(
                "{{\"kind\": \"extent_status\", \"rationale\": {}}}",
                json_string(rationale)
            ),
            TruncationOracle::NotApplicable { rationale } => format!(
                "{{\"kind\": \"not_applicable\", \"rationale\": {}}}",
                json_string(rationale)
            ),
        };
        let fuzz = match spec.fuzz_target {
            Some(target) => json_string(target),
            None => "null".to_owned(),
        };
        out.push_str(&format!(
            "{}\n    {{\"id\": {}, \"entrypoint\": {}, \"truncation\": {}, \
             \"boundaries\": [{}], \"fuzz_target\": {}}}",
            if index == 0 { "" } else { "," },
            json_string(spec.id),
            json_string(spec.entrypoint),
            truncation,
            boundaries.join(", "),
            fuzz,
        ));
    }
    out.push_str("\n  ],\n  \"entries\": [");
    for (index, entry) in entries().iter().enumerate() {
        let source = match &entry.source {
            CorpusSource::Builder { builder } => {
                format!(
                    "{{\"kind\": \"builder\", \"builder\": {}}}",
                    json_string(builder)
                )
            }
            CorpusSource::CommittedFixture { path } => {
                format!(
                    "{{\"kind\": \"committed_fixture\", \"path\": {}}}",
                    json_string(path)
                )
            }
            CorpusSource::PrivateInstall { extensions } => {
                let exts: Vec<String> = extensions.iter().map(|e| json_string(e)).collect();
                format!(
                    "{{\"kind\": \"private_install\", \"extensions\": [{}]}}",
                    exts.join(", ")
                )
            }
            CorpusSource::PrivateSeed { path, sha256 } => {
                let hash = match sha256 {
                    Some(hash) => json_string(&hex(hash)),
                    None => "null".to_owned(),
                };
                format!(
                    "{{\"kind\": \"private_seed\", \"path\": {}, \"sha256\": {}}}",
                    json_string(path),
                    hash,
                )
            }
        };
        let expected = match entry.expected {
            ExpectedOutcome::Accept => "accept",
            ExpectedOutcome::Refuse => "refuse",
        };
        out.push_str(&format!(
            "{}\n    {{\"id\": {}, \"container\": {}, \"class\": {}, \
             \"source\": {}, \"expected\": {}, \"note\": {}}}",
            if index == 0 { "" } else { "," },
            json_string(entry.id),
            json_string(entry.container),
            json_string(entry.class.as_str()),
            source,
            json_string(expected),
            json_string(entry.note),
        ));
    }
    let counts = manifest_counts();
    out.push_str(&format!(
        "\n  ],\n  \"counts\": {{\"containers\": {}, \"prefix_refusing\": {}, \
         \"synthetic\": {}, \"private\": {}, \"regression\": {}, \
         \"fuzz_targets\": {}}}\n}}\n",
        counts.containers,
        counts.prefix_refusing,
        counts.synthetic,
        counts.private,
        counts.regression,
        counts.fuzz_targets,
    ));
    out
}

// ---------------------------------------------------------------------------
// Audit
// ---------------------------------------------------------------------------

/// Repo prefixes that must never hold a tracked file: the gitignored homes
/// of private corpus material. A tracked file here is original bytes (or a
/// private seed) committed by mistake.
pub const PRIVATE_ONLY_PREFIXES: &[&str] = &[
    "private/",
    "original/",
    "assets/original/",
    "research-output/",
    "cache/",
    "captures/",
];

/// Whether the private corpus could be resolved at audit time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivateAvailability {
    /// No private root was given: the private suite did not run. Reported,
    /// never counted as pass or fail — spec non-negotiable #5.
    Unavailable,
    /// A private root was resolved and every selector was enumerated.
    Available,
}

/// The audit result for one private selector entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateSelectorReport {
    /// The entry's id.
    pub entry: &'static str,
    /// Members the selector matched under the private root.
    pub members: usize,
    /// Total bytes across matched members.
    pub total_bytes: u64,
    /// SHA-256 over every member's digest concatenated in path order — one
    /// fingerprint for the whole selected set (spec: "record the actual
    /// input fingerprint").
    pub fingerprint: [u8; 32],
}

/// What `audit` found.
#[derive(Clone, Debug)]
pub struct AuditReport {
    /// Tracked files under a private-only prefix — must be empty.
    pub tracked_private_paths: Vec<String>,
    /// Committed fixtures that are not tracked in the repo.
    pub missing_committed_fixtures: Vec<String>,
    /// Manifest violations, if any.
    pub manifest_errors: Vec<ManifestError>,
    /// Whether the private corpus was resolved.
    pub private_availability: PrivateAvailability,
    /// Per-selector results (empty when the private corpus is unavailable).
    pub private_selectors: Vec<PrivateSelectorReport>,
}

/// Ways the audit itself can fail to run.
#[derive(Clone, Debug)]
pub enum AuditError {
    /// `git ls-files` could not run in the workspace.
    Git(String),
    /// The private root was given but is not a readable directory.
    PrivateRootUnreadable(PathBuf),
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git(message) => write!(f, "git ls-files failed: {message}"),
            Self::PrivateRootUnreadable(path) => {
                write!(
                    f,
                    "private root {} is not a readable directory",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for AuditError {}

/// Runs the separation audit: manifest consistency, no tracked file under a
/// private-only prefix, every committed fixture tracked, and — when a
/// private root is given — each private selector enumerated and
/// fingerprinted.
///
/// `private_root` is the read-only original installation (or a private
/// corpus directory); `None` reports the private suite as unavailable
/// without failing the audit.
pub fn audit(
    workspace_root: &Path,
    private_root: Option<&Path>,
) -> Result<AuditReport, AuditError> {
    let tracked = git_ls_files(workspace_root)?;
    let tracked_set: BTreeSet<&str> = tracked.iter().map(String::as_str).collect();

    let tracked_private_paths: Vec<String> = tracked
        .iter()
        .filter(|path| {
            PRIVATE_ONLY_PREFIXES
                .iter()
                .any(|prefix| path.starts_with(prefix))
        })
        .cloned()
        .collect();

    let mut missing_committed_fixtures = Vec::new();
    for entry in entries() {
        if let CorpusSource::CommittedFixture { path } = entry.source
            && !tracked_set.contains(path)
        {
            missing_committed_fixtures.push(path.to_owned());
        }
    }

    let mut private_selectors = Vec::new();
    let private_availability = match private_root {
        None => PrivateAvailability::Unavailable,
        Some(root) => {
            if !root.is_dir() {
                return Err(AuditError::PrivateRootUnreadable(root.to_path_buf()));
            }
            let mut members = Vec::new();
            collect_members(root, root, &mut members);
            members.sort();
            for entry in entries() {
                let CorpusSource::PrivateInstall { extensions } = entry.source else {
                    continue;
                };
                let matched: Vec<&PathBuf> = members
                    .iter()
                    .filter(|path| {
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| {
                                extensions
                                    .iter()
                                    .any(|wanted| ext.eq_ignore_ascii_case(wanted))
                            })
                    })
                    .collect();
                private_selectors.push(fingerprint_selector(entry.id, root, &matched));
            }
            PrivateAvailability::Available
        }
    };

    Ok(AuditReport {
        tracked_private_paths,
        missing_committed_fixtures,
        manifest_errors: verify_manifest(),
        private_availability,
        private_selectors,
    })
}

/// Whether the report describes a clean audit: no tracked private paths, no
/// missing fixtures, no manifest violations.
pub fn audit_is_clean(report: &AuditReport) -> bool {
    report.tracked_private_paths.is_empty()
        && report.missing_committed_fixtures.is_empty()
        && report.manifest_errors.is_empty()
}

fn git_ls_files(root: &Path) -> Result<Vec<String>, AuditError> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|error| AuditError::Git(error.to_string()))?;
    if !output.status.success() {
        return Err(AuditError::Git(format!(
            "exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn collect_members(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_members(root, &path, out);
        } else if path.is_file() {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
}

fn fingerprint_selector(
    entry: &'static str,
    root: &Path,
    members: &[&PathBuf],
) -> PrivateSelectorReport {
    let mut total_bytes = 0u64;
    let mut digests = Vec::new();
    for member in members {
        if let Ok(bytes) = fs::read(root.join(member)) {
            total_bytes += bytes.len() as u64;
            digests.push(sha256(&bytes));
        }
    }
    let mut fingerprint_input = Vec::with_capacity(digests.len() * 32);
    for digest in &digests {
        fingerprint_input.extend_from_slice(digest);
    }
    PrivateSelectorReport {
        entry,
        members: digests.len(),
        total_bytes,
        fingerprint: sha256(&fingerprint_input),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Lowercase hex form of a digest, for reports.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&sha256(bytes))
}

// ---------------------------------------------------------------------------
// SHA-256 (hand-rolled: the workspace carries no hashing dependency)
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 of `bytes`, as the fingerprint private corpus members carry.
///
/// Hand-rolled so `cs_xtask` keeps its zero-dependency manifest: the
/// algorithm is FIPS 180-4 over standard constants, and the corpus tests
/// pin it against published vectors.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut padded = bytes.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 64];
    for chunk in padded.as_chunks::<64>().0 {
        for (i, word) in w.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (acc, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *acc = acc.wrapping_add(v);
        }
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}
