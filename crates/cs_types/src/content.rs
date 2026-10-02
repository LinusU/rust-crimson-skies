//! Stable content identity and provenance-bearing catalog schema (F14-A).
//!
//! Spec F14, "Deliverable and interfaces": a [`CatalogElement`] carries a
//! stable [`ContentId`], its [`ContentKind`], an [`Origin`], its
//! dependencies, its parse and normalize states, its runtime consumers, its
//! readiness and the reasons it is unsupported, plus an optional content
//! fingerprint. Parsing, normalization, dependency validation and runtime
//! readiness stay **separate** states (non-negotiable behavior 1): a row can
//! be visible and still unavailable, and an unavailable row must say why.
//!
//! [`ContentId`] derives from a semantic source key plus a namespace (the
//! [`ContentKind`]) — never from enumeration order — and is normalized to
//! lowercase `[a-z0-9._-]` with at least one alphanumeric at construction, so
//! the same content yields the same id on every run and a key can never
//! smuggle a path separator, a `.` or `..` component or an absolute/drive
//! spelling into a lookup (non-negotiable behavior 4 and the
//! `IDENTITY-CONTENT` contract's `ContentId` rule: "validate at
//! construction; no unchecked path join"). Original display names are
//! carried outside the identity in [`CatalogElement::display_name`].
//!
//! Every normalized value travels as a [`Known`] or a [`Resolved`], so a
//! field is either a value with [`Provenance`] or an explicit
//! [`Resolved::Unknown`] with a claim id and a reason; there is no
//! `Default::default()` path that silently invents a critical value
//! (non-negotiable behavior 3). The `IDENTITY-CONTENT` contract's
//! `EvidenceClass` is the existing seven-state
//! [`ClaimStatus`](crate::evidence::ClaimStatus) vocabulary from F01; this
//! module reuses it instead of defining a second copy that could drift.
//!
//! The catalog collection that consumes these records (insertion, duplicate
//! identity refusal, the declared launchable baseline and readiness
//! accounting) lives in `cs_content::catalog`. F14-B adds the canonical
//! scalar [`Unit`] and [`PermittedRange`] vocabulary used by that crate's
//! normalizer, and the `cs_content::catalog::closure` walk implements the
//! transitive dependency closure over these records. Nothing in this module
//! is derived from original game data.

use std::fmt;

use crate::asset_id::SourceSpan;
use crate::evidence::{ClaimId, ClaimStatus, Fingerprint};
use crate::install::ParseState;

/// Maximum byte length of a [`ContentId`] key.
pub const MAX_CONTENT_KEY_LEN: usize = 128;

/// The separator between the namespace and the key of a [`ContentId`]. The
/// key grammar excludes it, so the split is unambiguous.
pub const CONTENT_ID_SEPARATOR: char = '/';

/// One namespace of the content catalog: the kind of thing an element is.
///
/// The set is the union of the catalog collections the `IDENTITY-CONTENT`
/// contract requires ("Install files and container members; world groups and
/// variants; …") and the collections spec F14 names in its deliverable
/// ("worlds, missions, airframes, loadouts, factions, weapons, sounds,
/// dialogue, media, stunts, scrapbook items, IA scenarios and multiplayer
/// rules"). A kind is engine-authored vocabulary, not an observed retail
/// label: which original file a kind maps to is discovered by the format
/// and mission tasks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContentKind {
    /// An installation file or one of its container members.
    InstallFile,
    /// A world group or one variant of a world.
    World,
    /// A scene root or a node in the node hierarchy.
    SceneNode,
    /// A render mesh.
    Mesh,
    /// A render material.
    Material,
    /// An image or texture resource.
    Image,
    /// A collision surface.
    CollisionSurface,
    /// An airframe.
    Airframe,
    /// An engine.
    Engine,
    /// An armor component.
    Armor,
    /// A gun.
    Gun,
    /// Ammunition.
    Ammo,
    /// A hardpoint or other equipment slot.
    HardpointEquipment,
    /// A blueprint.
    Blueprint,
    /// A faction paint mask.
    PaintMask,
    /// A pilot.
    Pilot,
    /// A voice.
    Voice,
    /// A faction.
    Faction,
    /// A campaign mission.
    Mission,
    /// A script resource.
    Script,
    /// A script instruction or opcode.
    Instruction,
    /// A native binding or handler.
    NativeBinding,
    /// An animation track.
    AnimationTrack,
    /// A camera track.
    CameraTrack,
    /// A mission objective.
    Objective,
    /// A trigger.
    Trigger,
    /// A route.
    Route,
    /// A sound effect.
    Sound,
    /// Music.
    Music,
    /// Spoken dialogue.
    Dialogue,
    /// A video.
    Video,
    /// A font.
    Font,
    /// A string-table resource.
    StringResource,
    /// A user-interface resource.
    UiResource,
    /// A stunt.
    Stunt,
    /// A scrapbook item or reward.
    ScrapbookItem,
    /// An instant-action scenario.
    IaScenario,
    /// An instant-action preset or option.
    IaPreset,
    /// A multiplayer scenario.
    MultiplayerScenario,
    /// A multiplayer rule set.
    MultiplayerRules,
    /// A legacy custom-plane resource.
    CustomPlane,
    /// A player-loadable weapon and equipment loadout.
    Loadout,
    /// A weapon available to a loadout.
    Weapon,
}

impl ContentKind {
    /// Every kind, in a stable order. [`ContentKind::from_label`] scans this
    /// table, so `label` and `from_label` cannot disagree about one kind.
    pub const ALL: &'static [ContentKind] = &[
        Self::InstallFile,
        Self::World,
        Self::SceneNode,
        Self::Mesh,
        Self::Material,
        Self::Image,
        Self::CollisionSurface,
        Self::Airframe,
        Self::Engine,
        Self::Armor,
        Self::Gun,
        Self::Ammo,
        Self::HardpointEquipment,
        Self::Blueprint,
        Self::PaintMask,
        Self::Pilot,
        Self::Voice,
        Self::Faction,
        Self::Mission,
        Self::Script,
        Self::Instruction,
        Self::NativeBinding,
        Self::AnimationTrack,
        Self::CameraTrack,
        Self::Objective,
        Self::Trigger,
        Self::Route,
        Self::Sound,
        Self::Music,
        Self::Dialogue,
        Self::Video,
        Self::Font,
        Self::StringResource,
        Self::UiResource,
        Self::Stunt,
        Self::ScrapbookItem,
        Self::IaScenario,
        Self::IaPreset,
        Self::MultiplayerScenario,
        Self::MultiplayerRules,
        Self::CustomPlane,
        Self::Loadout,
        Self::Weapon,
    ];

    /// The stable label used in ids and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::InstallFile => "install_file",
            Self::World => "world",
            Self::SceneNode => "scene_node",
            Self::Mesh => "mesh",
            Self::Material => "material",
            Self::Image => "image",
            Self::CollisionSurface => "collision_surface",
            Self::Airframe => "airframe",
            Self::Engine => "engine",
            Self::Armor => "armor",
            Self::Gun => "gun",
            Self::Ammo => "ammo",
            Self::HardpointEquipment => "hardpoint_equipment",
            Self::Blueprint => "blueprint",
            Self::PaintMask => "paint_mask",
            Self::Pilot => "pilot",
            Self::Voice => "voice",
            Self::Faction => "faction",
            Self::Mission => "mission",
            Self::Script => "script",
            Self::Instruction => "instruction",
            Self::NativeBinding => "native_binding",
            Self::AnimationTrack => "animation_track",
            Self::CameraTrack => "camera_track",
            Self::Objective => "objective",
            Self::Trigger => "trigger",
            Self::Route => "route",
            Self::Sound => "sound",
            Self::Music => "music",
            Self::Dialogue => "dialogue",
            Self::Video => "video",
            Self::Font => "font",
            Self::StringResource => "string_resource",
            Self::UiResource => "ui_resource",
            Self::Stunt => "stunt",
            Self::ScrapbookItem => "scrapbook_item",
            Self::IaScenario => "ia_scenario",
            Self::IaPreset => "ia_preset",
            Self::MultiplayerScenario => "multiplayer_scenario",
            Self::MultiplayerRules => "multiplayer_rules",
            Self::CustomPlane => "custom_plane",
            Self::Loadout => "loadout",
            Self::Weapon => "weapon",
        }
    }

    /// Looks a kind up by its label; `None` for an unknown namespace.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.label() == label)
    }

    /// Whether an element of this kind can be launched as playable content.
    ///
    /// The declared launchable baseline is what readiness is measured over
    /// (spec F14 non-negotiable behavior 4): a campaign mission, an
    /// instant-action scenario and a multiplayer scenario are launchable; a
    /// texture or a sound is a dependency of one, never a launchable row
    /// itself.
    pub const fn is_launchable(self) -> bool {
        matches!(
            self,
            Self::Mission | Self::IaScenario | Self::MultiplayerScenario
        )
    }
}

impl fmt::Display for ContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a [`ContentId`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentIdError {
    /// The id text was empty.
    Empty,
    /// The id text carried no namespace/key separator.
    MissingSeparator,
    /// The namespace was not a known [`ContentKind`] label.
    UnknownNamespace {
        /// The unrecognized namespace text.
        namespace: String,
    },
    /// The key was empty.
    EmptyKey,
    /// The key exceeded [`MAX_CONTENT_KEY_LEN`] bytes.
    KeyTooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` (after ASCII
    /// lowercasing), including any path separator.
    BadKeyCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character: it was only separators
    /// (`.`/`-`/`_`) and could spell a `.` or `..` path component.
    NoAlphanumeric,
}

impl fmt::Display for ContentIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "content id must not be empty"),
            Self::MissingSeparator => write!(
                f,
                "content id must be <namespace>{CONTENT_ID_SEPARATOR}<key>"
            ),
            Self::UnknownNamespace { namespace } => {
                write!(f, "content id namespace {namespace:?} is not a known kind")
            }
            Self::EmptyKey => write!(f, "content id key must not be empty"),
            Self::KeyTooLong { len } => write!(
                f,
                "content id key is {len} bytes, max is {MAX_CONTENT_KEY_LEN}"
            ),
            Self::BadKeyCharacter { ch } => {
                write!(f, "content id key contains disallowed character {ch:?}")
            }
            Self::NoAlphanumeric => write!(
                f,
                "content id key must contain at least one ASCII alphanumeric character"
            ),
        }
    }
}

impl std::error::Error for ContentIdError {}

/// Maximum byte length of a graph-local [`DamageNodeKey`].
///
/// The graph-local node identity uses the same bound as a [`ContentId`]
/// key, and this constant is that one bound: the two public names cannot
/// drift apart.
pub const MAX_NODE_KEY_LEN: usize = MAX_CONTENT_KEY_LEN;

/// The internal outcome of the one key-grammar implementation.
enum KeyGrammarError {
    /// The key was empty.
    Empty,
    /// The key exceeded the caller's byte bound.
    TooLong { len: usize },
    /// The key contained a character outside `[a-z0-9._-]`.
    BadCharacter { ch: char },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

/// Validates and normalizes a content-style key: lowercased ASCII
/// alphanumerics plus `.`, `_` and `-`, at least one of them alphanumeric.
///
/// This is the single implementation of the `IDENTITY-CONTENT` key grammar.
/// [`ContentId`] applies it to a namespaced catalog key and
/// [`DamageNodeKey`] to a graph-local identity; neither re-implements it.
/// The grammar deliberately excludes `/`, `\`, `:`, requires at least one
/// alphanumeric character and so refuses the `.`/`..`/blank path components,
/// meaning a key can never be joined to a filesystem path or escape a
/// namespace (`IDENTITY-CONTENT`: "validate at construction; no unchecked
/// path join"). Uppercase input is folded rather than rejected: a semantic
/// source key such as `M01` and `m01` name the same content, and identity is
/// normalized centrally while the original display name stays outside it.
fn normalize_grammar(source_key: &str, max_len: usize) -> Result<String, KeyGrammarError> {
    if source_key.is_empty() {
        return Err(KeyGrammarError::Empty);
    }
    let key = source_key.to_ascii_lowercase();
    if key.len() > max_len {
        return Err(KeyGrammarError::TooLong { len: key.len() });
    }
    for ch in key.chars() {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
            return Err(KeyGrammarError::BadCharacter { ch });
        }
    }
    if !key.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
        return Err(KeyGrammarError::NoAlphanumeric);
    }
    Ok(key)
}

fn normalize_key(source_key: &str) -> Result<String, ContentIdError> {
    normalize_grammar(source_key, MAX_CONTENT_KEY_LEN).map_err(|error| match error {
        KeyGrammarError::Empty => ContentIdError::EmptyKey,
        KeyGrammarError::TooLong { len } => ContentIdError::KeyTooLong { len },
        KeyGrammarError::BadCharacter { ch } => ContentIdError::BadKeyCharacter { ch },
        KeyGrammarError::NoAlphanumeric => ContentIdError::NoAlphanumeric,
    })
}

/// Why a graph-local [`DamageNodeKey`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageNodeKeyError {
    /// The key was empty.
    Empty,
    /// The key exceeded [`MAX_NODE_KEY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` (after ASCII
    /// lowercasing).
    BadCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

impl fmt::Display for DamageNodeKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a damage node key must not be empty"),
            Self::TooLong { len } => {
                write!(
                    f,
                    "a damage node key is {len} bytes, max is {MAX_NODE_KEY_LEN}"
                )
            }
            Self::BadCharacter { ch } => {
                write!(f, "a damage node key contains disallowed character {ch:?}")
            }
            Self::NoAlphanumeric => {
                write!(f, "a damage node key must contain an ASCII alphanumeric")
            }
        }
    }
}

impl std::error::Error for DamageNodeKeyError {}

/// The stable identity of one node inside one damage graph.
///
/// Keys are graph-local: `engine_1` inside one airframe's graph and
/// `engine_1` inside another's are different nodes because the owning actor
/// differs. The key is semantic — an authored part name — never the node's
/// position in any list.
///
/// The grammar is the [`ContentId`] key grammar applied to a graph-local
/// identity instead of a catalog id: the "same identity discipline" F29
/// requires for every damage graph. One definition serves `cs_sim` and
/// `cs_content` (task #442), so the two crates can no longer apply the
/// grammar independently and drift; a declared key lowers to a runtime key
/// unchanged.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DamageNodeKey(String);

impl DamageNodeKey {
    /// Validates and wraps a node key. Uppercase input is folded, matching
    /// [`ContentId`] normalization.
    ///
    /// # Errors
    ///
    /// [`DamageNodeKeyError`] when the key is empty, too long, carries a
    /// character outside `[a-z0-9._-]` or has no alphanumeric.
    pub fn new(key: &str) -> Result<Self, DamageNodeKeyError> {
        normalize_grammar(key, MAX_NODE_KEY_LEN)
            .map(Self)
            .map_err(|error| match error {
                KeyGrammarError::Empty => DamageNodeKeyError::Empty,
                KeyGrammarError::TooLong { len } => DamageNodeKeyError::TooLong { len },
                KeyGrammarError::BadCharacter { ch } => DamageNodeKeyError::BadCharacter { ch },
                KeyGrammarError::NoAlphanumeric => DamageNodeKeyError::NoAlphanumeric,
            })
    }

    /// The normalized key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DamageNodeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The stable identity of one catalog element: a namespace plus a semantic
/// source key.
///
/// The canonical text form is `namespace/key` and is what equality, hashing
/// and ordering compare, so an element keeps one identity regardless of
/// insertion order (spec F14 AC02). The id is built by [`ContentId::from_source`]
/// from a *semantic* source key — a mission id, an airframe name — never
/// from an enumeration index; original display names live on the element.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentId {
    /// The canonical `namespace/key` text.
    id: String,
    /// The parsed namespace.
    kind: ContentKind,
    /// The normalized key.
    key: String,
}

impl ContentId {
    /// Builds an id from a kind and a semantic source key.
    ///
    /// # Errors
    ///
    /// Returns [`ContentIdError`] when the key is empty, too long or
    /// contains a character outside the id grammar.
    pub fn from_source(kind: ContentKind, source_key: &str) -> Result<Self, ContentIdError> {
        let key = normalize_key(source_key)?;
        let id = format!("{}{CONTENT_ID_SEPARATOR}{key}", kind.label());
        Ok(Self { id, kind, key })
    }

    /// Parses a canonical `namespace/key` id.
    ///
    /// # Errors
    ///
    /// Returns [`ContentIdError`] when the text is empty, has no separator,
    /// names an unknown namespace or carries an invalid key.
    pub fn parse(text: &str) -> Result<Self, ContentIdError> {
        if text.is_empty() {
            return Err(ContentIdError::Empty);
        }
        let Some((namespace, key)) = text.split_once(CONTENT_ID_SEPARATOR) else {
            return Err(ContentIdError::MissingSeparator);
        };
        let Some(kind) = ContentKind::from_label(namespace) else {
            return Err(ContentIdError::UnknownNamespace {
                namespace: namespace.to_owned(),
            });
        };
        Self::from_source(kind, key)
    }

    /// The element's namespace.
    pub fn kind(&self) -> ContentKind {
        self.kind
    }

    /// The normalized semantic key, without the namespace.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The canonical `namespace/key` text.
    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

/// Where a catalog element came from.
///
/// The distinction is load-bearing (spec F14 AC04): a newly authored
/// synthetic fixture row must never be mistaken for a retail catalog entry.
/// Only [`Origin::Installation`] names bytes of the owner's installation;
/// [`Origin::is_original`] answers that question, and the readiness report
/// counts synthetic and original launchable rows separately.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Derived from bytes of the owner's original installation, located by
    /// a checked span.
    Installation {
        /// Where the element's bytes live.
        source: SourceSpan,
    },
    /// Newly authored synthetic fixture content. It can never stand in for
    /// missing original data.
    SyntheticFixture,
    /// Newly authored engine design with no original counterpart.
    Designed,
}

impl Origin {
    /// Whether this origin is original installation data.
    pub fn is_original(&self) -> bool {
        matches!(self, Self::Installation { .. })
    }

    /// The stable label used in reports.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Installation { .. } => "installation",
            Self::SyntheticFixture => "synthetic_fixture",
            Self::Designed => "designed",
        }
    }

    /// The installation span, when the element came from original bytes.
    pub fn source(&self) -> Option<&SourceSpan> {
        match self {
            Self::Installation { source } => Some(source),
            Self::SyntheticFixture | Self::Designed => None,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where one normalized value or catalog claim came from.
///
/// The `class` is the contract's `EvidenceClass`, which is the canonical
/// seven-state [`ClaimStatus`] from F01. `source` locates the bytes the
/// claim was read from; it is required for a
/// [`ClaimStatus::VerifiedOriginal`] claim, because such a claim means
/// "someone looked at fingerprinted original bytes" (F01-A).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// The claim this provenance backs.
    pub claim_id: ClaimId,
    /// How strong the evidence is.
    pub class: ClaimStatus,
    /// Where the evidence was observed, when it has a location.
    pub source: Option<SourceSpan>,
}

/// Why a [`Provenance`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvenanceError {
    /// A `verified_original` provenance named no source span, so it locates
    /// nothing it could have observed.
    VerifiedOriginalWithoutSource,
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VerifiedOriginalWithoutSource => write!(
                f,
                "a verified_original provenance must name the source span it observed"
            ),
        }
    }
}

impl std::error::Error for ProvenanceError {}

impl Provenance {
    /// Builds provenance, refusing a `verified_original` claim with no
    /// source span.
    ///
    /// # Errors
    ///
    /// [`ProvenanceError::VerifiedOriginalWithoutSource`].
    pub fn new(
        claim_id: ClaimId,
        class: ClaimStatus,
        source: Option<SourceSpan>,
    ) -> Result<Self, ProvenanceError> {
        if class == ClaimStatus::VerifiedOriginal && source.is_none() {
            return Err(ProvenanceError::VerifiedOriginalWithoutSource);
        }
        Ok(Self {
            claim_id,
            class,
            source,
        })
    }

    /// The designed-default marker for an engine-authored value: explicit
    /// design, with no observed source.
    pub fn designed(claim_id: ClaimId) -> Self {
        Self {
            claim_id,
            class: ClaimStatus::Designed,
            source: None,
        }
    }

    /// The marker for a value whose origin is explicitly unknown.
    pub fn unknown(claim_id: ClaimId) -> Self {
        Self {
            claim_id,
            class: ClaimStatus::Unknown,
            source: None,
        }
    }
}

/// A value together with where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known<T> {
    /// The value.
    pub value: T,
    /// Its provenance.
    pub provenance: Provenance,
}

impl<T> Known<T> {
    /// Pairs a value with its provenance.
    pub fn new(value: T, provenance: Provenance) -> Self {
        Self { value, provenance }
    }
}

/// Why a [`Resolved`] was rejected when built from raw parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedError {
    /// An unknown value carried no reason.
    EmptyReason,
}

impl fmt::Display for ResolvedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyReason => write!(f, "an unknown value must carry a reason"),
        }
    }
}

impl std::error::Error for ResolvedError {}

/// A normalized value that is either known with provenance or explicitly
/// unknown.
///
/// This is the `IDENTITY-CONTENT` `Resolved<T>` contract: a missing critical
/// value is an explicit unknown with a reason, never a zero or a
/// `Default::default()` (spec F14 non-negotiable behavior 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved<T> {
    /// The value and its provenance.
    Known(Known<T>),
    /// The value is not known; nothing is invented in its place.
    Unknown {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
}

impl<T> Resolved<T> {
    /// An explicit unknown, refusing an empty reason.
    ///
    /// # Errors
    ///
    /// [`ResolvedError::EmptyReason`] when `reason` is empty or whitespace.
    pub fn unknown(claim_id: ClaimId, reason: &str) -> Result<Self, ResolvedError> {
        if reason.trim().is_empty() {
            return Err(ResolvedError::EmptyReason);
        }
        Ok(Self::Unknown {
            claim_id,
            reason: reason.to_owned(),
        })
    }

    /// Whether this is a known value.
    pub fn is_known(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    /// The known value, consuming the resolution.
    pub fn known(self) -> Option<T> {
        match self {
            Self::Known(known) => Some(known.value),
            Self::Unknown { .. } => None,
        }
    }

    /// The provenance of a known value.
    pub fn provenance(&self) -> Option<&Provenance> {
        match self {
            Self::Known(known) => Some(&known.provenance),
            Self::Unknown { .. } => None,
        }
    }
}

/// A canonical unit a normalized quantity is expressed in.
///
/// [`Unit::ALL`] is the closed vocabulary the `IDENTITY-CONTENT` numeric
/// contract names: "Normalization creates meters, seconds, radians,
/// kilograms or explicitly documented game-weight units." Original UI units
/// (miles per hour, feet, degrees) are conversion *inputs*, never these
/// simulation units: [`Unit::from_label`] answers `None` for any spelling it
/// does not know, and F14-B's normalizer turns that into an explicit unknown
/// rather than assuming SI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Unit {
    /// Metres.
    Meters,
    /// Seconds.
    Seconds,
    /// Radians.
    Radians,
    /// Kilograms.
    Kilograms,
    /// A documented game-weight unit with no SI counterpart. Its conversion
    /// is declared per source, never assumed to be kilograms.
    GameWeight,
}

impl Unit {
    /// Every unit, in a stable order. [`Unit::from_label`] scans this table,
    /// so `label` and `from_label` cannot disagree about one unit.
    pub const ALL: &'static [Unit] = &[
        Self::Meters,
        Self::Seconds,
        Self::Radians,
        Self::Kilograms,
        Self::GameWeight,
    ];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Meters => "meters",
            Self::Seconds => "seconds",
            Self::Radians => "radians",
            Self::Kilograms => "kilograms",
            Self::GameWeight => "game_weight",
        }
    }

    /// The unit's symbol, when it has an unambiguous one.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Meters => "m",
            Self::Seconds => "s",
            Self::Radians => "rad",
            Self::Kilograms => "kg",
            // A game-weight unit is project vocabulary; inventing an SI
            // symbol for it would imply a conversion nobody measured.
            Self::GameWeight => "game_weight",
        }
    }

    /// Looks a unit up by its label; `None` for an unknown unit.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|unit| unit.label() == label)
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a [`PermittedRange`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeError {
    /// A bound was NaN or infinite.
    NonFinite {
        /// Which bound: `"min"` or `"max"`.
        which: &'static str,
    },
    /// The lower bound was greater than the upper bound.
    Reversed {
        /// The lower bound.
        min: f64,
        /// The upper bound.
        max: f64,
    },
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { which } => {
                write!(f, "the {which} bound of a permitted range must be finite")
            }
            Self::Reversed { min, max } => {
                write!(
                    f,
                    "the permitted range {min}..={max} has its bounds reversed"
                )
            }
        }
    }
}

impl std::error::Error for RangeError {}

/// The approved inclusive range of one normalized quantity.
///
/// The numeric contract requires a tuning value to be "finite and within
/// measured/approved ranges"; a range is declared by the consumer, validated
/// once (`PermittedRange::new`) and then bounds every value F14-B's
/// normalizer accepts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PermittedRange {
    min: f64,
    max: f64,
}

impl PermittedRange {
    /// A range from an inclusive `min` to an inclusive `max`.
    ///
    /// # Errors
    ///
    /// [`RangeError::NonFinite`] when a bound is NaN or infinite, and
    /// [`RangeError::Reversed`] when `min > max`.
    pub fn new(min: f64, max: f64) -> Result<Self, RangeError> {
        if !min.is_finite() {
            return Err(RangeError::NonFinite { which: "min" });
        }
        if !max.is_finite() {
            return Err(RangeError::NonFinite { which: "max" });
        }
        if min > max {
            return Err(RangeError::Reversed { min, max });
        }
        Ok(Self { min, max })
    }

    /// The inclusive lower bound.
    pub const fn min(self) -> f64 {
        self.min
    }

    /// The inclusive upper bound.
    pub const fn max(self) -> f64 {
        self.max
    }

    /// Whether `value` is finite and inside the inclusive range.
    pub fn contains(self, value: f64) -> bool {
        value.is_finite() && self.min <= value && value <= self.max
    }
}

impl fmt::Display for PermittedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}]", self.min, self.max)
    }
}

/// What consumes a catalog element at runtime.
///
/// The dependency closure verifies "resources, parsers, instructions,
/// native handlers, sockets, strings, media and gameplay consumers"
/// (`IDENTITY-CONTENT`). An element with no consumer of any kind cannot be
/// ready; which consumer an element needs is F14-B's closure rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConsumerKind {
    /// A format parser.
    Parser,
    /// A resource loader.
    ResourceLoader,
    /// A script instruction handler.
    InstructionHandler,
    /// A native binding or handler.
    NativeBinding,
    /// A network socket.
    Socket,
    /// A string table.
    StringTable,
    /// A media player (sound, music, dialogue or video).
    MediaPlayer,
    /// Simulation or UI gameplay code.
    Gameplay,
}

impl ConsumerKind {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Parser => "parser",
            Self::ResourceLoader => "resource_loader",
            Self::InstructionHandler => "instruction_handler",
            Self::NativeBinding => "native_binding",
            Self::Socket => "socket",
            Self::StringTable => "string_table",
            Self::MediaPlayer => "media_player",
            Self::Gameplay => "gameplay",
        }
    }
}

impl fmt::Display for ConsumerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One runtime consumer of an element, with its provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeConsumer {
    /// What kind of consumer this is.
    pub kind: ConsumerKind,
    /// Where the claim that this consumer reads the element comes from.
    pub provenance: Provenance,
}

/// Which part of a graph an edge belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DependencyKind {
    /// A reference the bytes name directly.
    Static,
    /// A candidate discovered conservatively by the script adapter; a
    /// dynamic lookup that cannot be bounded is an unresolved dependency,
    /// never proof of no dependency (`IDENTITY-CONTENT`). Following this
    /// edge is what keeps an unbounded lookup from looking like "no
    /// dependencies".
    DynamicCandidate,
    /// A parent/child hierarchy edge: the target is owned by the source.
    ///
    /// Unlike a reference edge, an ownership **cycle** is invalid
    /// (`IDENTITY-CONTENT`: "cycles in ownership/parent hierarchies are
    /// invalid"); a reference cycle is a legitimate graph and is allowed.
    Ownership,
}

impl DependencyKind {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::DynamicCandidate => "dynamic_candidate",
            Self::Ownership => "ownership",
        }
    }

    /// Whether this edge expresses ownership rather than a reference.
    ///
    /// Ownership edges take part in the closure like any other edge, but
    /// only they are subject to the acyclic rule (F14-B).
    pub const fn is_ownership(self) -> bool {
        matches!(self, Self::Ownership)
    }
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One reference from an element to another catalog element, with per-edge
/// provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    /// The referenced element.
    pub target: ContentId,
    /// Whether the reference was read directly or discovered as a
    /// conservative dynamic candidate.
    pub kind: DependencyKind,
    /// Where the reference was read from.
    pub provenance: Provenance,
}

/// How far an element's normalization got.
///
/// Separate from parsing and from readiness (non-negotiable behavior 1): an
/// element can parse and still fail normalization, and either failure keeps
/// its diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizeState {
    /// Not normalized yet (deliberately or not).
    NotNormalized,
    /// Normalized into canonical units and checked value ranges.
    Normalized,
    /// Normalization was attempted and failed; the diagnostic is mandatory.
    Failed {
        /// Why normalization failed.
        diagnostic: String,
    },
}

/// Whether an element is usable at runtime.
///
/// A [`Readiness::Unavailable`] element must carry at least one
/// [`UnsupportedReason`]; a [`Readiness::Ready`] element must carry none, so
/// "ready" can never be a silent default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Readiness {
    /// Every critical requirement for this element is supported.
    Ready,
    /// The element is visible but cannot be used.
    Unavailable,
}

impl Readiness {
    /// Whether the element is ready.
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Unavailable => "unavailable",
        }
    }
}

impl fmt::Display for Readiness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a catalog element is not ready.
///
/// Reasons stay typed and named so a report can count, group and quote them;
/// an unsupported element is never dropped from the catalog (spec F14
/// non-negotiable behavior 4 and the `IDENTITY-CONTENT` rule that
/// collections cannot exclude failed entries).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsupportedReason {
    /// No parser covers the element's source family.
    MissingParser,
    /// The element has not been parsed yet, so nothing about it is validated.
    NotParsed,
    /// The element's bytes failed to parse.
    ParseFailed {
        /// The parser's diagnostic.
        diagnostic: String,
    },
    /// The element has not been normalized yet: parsing, normalization and
    /// readiness are separate states, and an unnormalized element is not
    /// validated (F14-B).
    NotNormalized,
    /// Normalization refused a field.
    NormalizeFailed {
        /// Why normalization failed.
        diagnostic: String,
    },
    /// No runtime consumer claims the element.
    MissingRuntimeConsumer,
    /// A referenced element is itself unsupported.
    UnsupportedDependency {
        /// The unsupported target.
        target: ContentId,
    },
    /// A conservative dynamic lookup could not be bounded, so its
    /// dependencies are unknown rather than absent (`IDENTITY-CONTENT`).
    UnboundedDynamicDependency {
        /// What could not be bounded.
        detail: String,
    },
    /// The element depends on something explicitly recorded as unknown.
    Unknown {
        /// The claim the unknown belongs to.
        claim_id: ClaimId,
        /// What is not known.
        reason: String,
    },
}

impl UnsupportedReason {
    /// The stable, machine-matchable reason code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingParser => "missing_parser",
            Self::NotParsed => "not_parsed",
            Self::ParseFailed { .. } => "parse_failed",
            Self::NotNormalized => "not_normalized",
            Self::NormalizeFailed { .. } => "normalize_failed",
            Self::MissingRuntimeConsumer => "missing_runtime_consumer",
            Self::UnsupportedDependency { .. } => "unsupported_dependency",
            Self::UnboundedDynamicDependency { .. } => "unbounded_dynamic_dependency",
            Self::Unknown { .. } => "unknown",
        }
    }

    /// The reason's free-text detail, when it carries one.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::ParseFailed { diagnostic } | Self::NormalizeFailed { diagnostic } => {
                Some(diagnostic)
            }
            Self::UnboundedDynamicDependency { detail } => Some(detail),
            Self::Unknown { reason, .. } => Some(reason),
            Self::MissingParser
            | Self::NotParsed
            | Self::NotNormalized
            | Self::MissingRuntimeConsumer
            | Self::UnsupportedDependency { .. } => None,
        }
    }

    /// Whether the reason carries the required non-empty detail.
    fn validate(&self) -> Result<(), ElementError> {
        if self.detail().is_some_and(|detail| detail.trim().is_empty()) {
            return Err(ElementError::EmptyUnsupportedDetail { code: self.code() });
        }
        Ok(())
    }
}

impl fmt::Display for UnsupportedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self.detail() {
            Some(detail) => write!(f, "{code}: {detail}"),
            None => write!(f, "{code}"),
        }
    }
}

/// Why a [`CatalogElement`] failed its per-record rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementError {
    /// The element's `kind` field disagreed with the namespace of its id.
    KindMismatch {
        /// The kind the id names.
        id_kind: ContentKind,
        /// The kind the element declares.
        kind: ContentKind,
    },
    /// A ready element carried unsupported reasons.
    ReadyWithUnsupportedReasons {
        /// How many reasons it carried.
        count: usize,
    },
    /// An unavailable element carried no reason.
    UnavailableWithoutReason,
    /// A failed parse state carried no diagnostic.
    EmptyParseDiagnostic,
    /// A failed normalize state carried no diagnostic.
    EmptyNormalizeDiagnostic,
    /// An unsupported reason carried an empty detail.
    EmptyUnsupportedDetail {
        /// The reason code.
        code: &'static str,
    },
}

impl fmt::Display for ElementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindMismatch { id_kind, kind } => write!(
                f,
                "element kind {kind} disagrees with its id namespace {id_kind}"
            ),
            Self::ReadyWithUnsupportedReasons { count } => write!(
                f,
                "a ready element must not carry unsupported reasons (found {count})"
            ),
            Self::UnavailableWithoutReason => {
                write!(f, "an unavailable element must carry at least one reason")
            }
            Self::EmptyParseDiagnostic => write!(f, "a failed parse state must carry a diagnostic"),
            Self::EmptyNormalizeDiagnostic => {
                write!(f, "a failed normalize state must carry a diagnostic")
            }
            Self::EmptyUnsupportedDetail { code } => {
                write!(f, "unsupported reason {code} must carry non-empty detail")
            }
        }
    }
}

impl std::error::Error for ElementError {}

/// One row of the canonical content catalog.
///
/// The ten fields are the contract's element record — `id`, `kind`,
/// `origin`, `dependencies`, `parse_state`, `normalize_state`,
/// `runtime_consumers`, `readiness`, `unsupported_reasons` and
/// `fingerprint` — plus the original display name that identity deliberately
/// excludes. Parsing, normalization and readiness are independent; a failed
/// or unknown row stays in the collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogElement {
    /// The stable identity.
    pub id: ContentId,
    /// The kind; must equal `id.kind()`.
    pub kind: ContentKind,
    /// The original or authored display name, outside identity.
    pub display_name: Option<String>,
    /// Where the element came from.
    pub origin: Origin,
    /// Its outgoing references, each with provenance.
    pub dependencies: Vec<Dependency>,
    /// How far parsing got.
    pub parse_state: ParseState,
    /// How far normalization got.
    pub normalize_state: NormalizeState,
    /// The consumers that read it at runtime.
    pub runtime_consumers: Vec<RuntimeConsumer>,
    /// Whether it is ready.
    pub readiness: Readiness,
    /// Why it is not ready; empty iff ready.
    pub unsupported_reasons: Vec<UnsupportedReason>,
    /// The content fingerprint, when the element's bytes were hashed.
    pub fingerprint: Option<Fingerprint>,
}

impl CatalogElement {
    /// The per-record admission rules.
    ///
    /// The first violation encountered is returned. Set-level rules —
    /// duplicate identities — belong to the catalog and are not checked
    /// here.
    ///
    /// # Errors
    ///
    /// [`ElementError`] for a kind/id mismatch, an unavailable element with
    /// no reason, a ready element with reasons, or an empty mandatory
    /// diagnostic.
    pub fn validate(&self) -> Result<(), ElementError> {
        if self.id.kind() != self.kind {
            return Err(ElementError::KindMismatch {
                id_kind: self.id.kind(),
                kind: self.kind,
            });
        }
        match self.readiness {
            Readiness::Ready if !self.unsupported_reasons.is_empty() => {
                return Err(ElementError::ReadyWithUnsupportedReasons {
                    count: self.unsupported_reasons.len(),
                });
            }
            Readiness::Unavailable if self.unsupported_reasons.is_empty() => {
                return Err(ElementError::UnavailableWithoutReason);
            }
            Readiness::Ready | Readiness::Unavailable => {}
        }
        if let ParseState::Failed { diagnostic } = &self.parse_state
            && diagnostic.trim().is_empty()
        {
            return Err(ElementError::EmptyParseDiagnostic);
        }
        if let NormalizeState::Failed { diagnostic } = &self.normalize_state
            && diagnostic.trim().is_empty()
        {
            return Err(ElementError::EmptyNormalizeDiagnostic);
        }
        for reason in &self.unsupported_reasons {
            reason.validate()?;
        }
        Ok(())
    }

    /// Whether the element is ready.
    pub fn is_ready(&self) -> bool {
        self.readiness.is_ready()
    }

    /// The reason codes, in order.
    pub fn unsupported_codes(&self) -> Vec<&'static str> {
        self.unsupported_reasons
            .iter()
            .map(UnsupportedReason::code)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::ContentHash;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("test claim id is valid")
    }

    fn span() -> SourceSpan {
        SourceSpan::new(
            ContentHash::from_bytes([0x2a; 32]),
            "gosdata/assets/crimson.rof",
            Some("ASSETS/SYNTHETIC.TGA"),
            0,
            4,
            None,
        )
        .expect("test span is valid")
    }

    fn ready_element(id: ContentId) -> CatalogElement {
        CatalogElement {
            kind: id.kind(),
            id,
            display_name: Some("Synthetic Mission".to_owned()),
            origin: Origin::SyntheticFixture,
            dependencies: Vec::new(),
            parse_state: ParseState::Parsed,
            normalize_state: NormalizeState::Normalized,
            runtime_consumers: vec![RuntimeConsumer {
                kind: ConsumerKind::Gameplay,
                provenance: Provenance::designed(claim("f14.synthetic.consumer")),
            }],
            readiness: Readiness::Ready,
            unsupported_reasons: Vec::new(),
            fingerprint: Some(Fingerprint {
                kind: crate::evidence::FingerprintKind::Artifact,
                sha256: ContentHash::from_bytes([0x3b; 32]),
            }),
        }
    }

    /// AC02 at the identity level: ids derive from a semantic source key
    /// plus namespace, normalize spelling, and never depend on order or
    /// carry a path separator.
    #[test]
    fn accept_f14_a_content_id_is_namespaced_normalized_and_path_safe() {
        let from_upper =
            ContentId::from_source(ContentKind::Mission, "M01").expect("uppercase key folds");
        let from_lower =
            ContentId::from_source(ContentKind::Mission, "m01").expect("lowercase key is valid");
        assert_eq!(from_upper, from_lower, "spelling does not change identity");
        assert_eq!(from_upper.as_str(), "mission/m01");
        assert_eq!(from_upper.kind(), ContentKind::Mission);
        assert_eq!(from_upper.key(), "m01");

        assert_eq!(
            ContentId::parse("mission/m01").expect("round trip"),
            from_upper,
            "the canonical text parses back to the same id"
        );

        assert_eq!(
            ContentId::from_source(ContentKind::World, "c1").expect("world id"),
            ContentId::parse("world/c1").expect("world id"),
        );
        assert_ne!(
            ContentId::from_source(ContentKind::World, "c1").expect("world id"),
            ContentId::from_source(ContentKind::World, "c2").expect("world id"),
            "the key is part of identity"
        );

        for (key, error) in [
            ("", ContentIdError::EmptyKey),
            ("c1/../planes", ContentIdError::BadKeyCharacter { ch: '/' }),
            ("c1\\planes", ContentIdError::BadKeyCharacter { ch: '\\' }),
            ("c:1", ContentIdError::BadKeyCharacter { ch: ':' }),
            ("c1 planes", ContentIdError::BadKeyCharacter { ch: ' ' }),
            (".", ContentIdError::NoAlphanumeric),
            ("..", ContentIdError::NoAlphanumeric),
            ("---", ContentIdError::NoAlphanumeric),
        ] {
            assert_eq!(
                ContentId::from_source(ContentKind::World, key),
                Err(error),
                "path-like key {key:?} is refused"
            );
        }

        assert_eq!(
            ContentId::from_source(ContentKind::Mission, "m01.intro").expect("dotted key"),
            ContentId::parse("mission/m01.intro").expect("dotted key round-trips"),
            "an interior dot is allowed; only a dot-only key is refused"
        );

        assert_eq!(ContentId::parse(""), Err(ContentIdError::Empty));
        assert_eq!(
            ContentId::parse("mission"),
            Err(ContentIdError::MissingSeparator)
        );
        assert_eq!(
            ContentId::parse("nonsense/m01"),
            Err(ContentIdError::UnknownNamespace {
                namespace: "nonsense".to_owned()
            })
        );

        let long = "a".repeat(MAX_CONTENT_KEY_LEN + 1);
        assert_eq!(
            ContentId::from_source(ContentKind::Mission, &long),
            Err(ContentIdError::KeyTooLong {
                len: MAX_CONTENT_KEY_LEN + 1
            })
        );

        let labels: Vec<&str> = ContentKind::ALL.iter().map(|kind| kind.label()).collect();
        let mut unique = labels.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "kind labels are unique");
        for kind in ContentKind::ALL {
            assert_eq!(ContentKind::from_label(kind.label()), Some(*kind));
        }
        assert_eq!(ContentKind::from_label("not_a_kind"), None);
        assert!(ContentKind::Mission.is_launchable());
        assert!(ContentKind::IaScenario.is_launchable());
        assert!(ContentKind::MultiplayerScenario.is_launchable());
        assert!(!ContentKind::Airframe.is_launchable());
    }

    /// The contract's numeric rule: a value is known with provenance or an
    /// explicit unknown with a reason; nothing defaults.
    #[test]
    fn accept_f14_a_values_are_known_with_provenance_or_explicitly_unknown() {
        let designed = Provenance::designed(claim("f14.synthetic.design"));
        assert_eq!(designed.class, ClaimStatus::Designed);
        assert_eq!(designed.source, None);

        let observed = Provenance::new(
            claim("f14.observed.texture"),
            ClaimStatus::ObservedTool,
            Some(span()),
        )
        .expect("an observed claim carries its span");
        let known: Resolved<u32> = Resolved::Known(Known::new(512, observed));
        assert!(known.is_known());
        assert_eq!(known.clone().known(), Some(512));
        assert_eq!(
            known.provenance().map(|p| p.class),
            Some(ClaimStatus::ObservedTool)
        );

        let unknown: Resolved<u32> =
            Resolved::unknown(claim("f14.unknown.dimension"), "no observed width")
                .expect("a reason is present");
        assert!(!unknown.is_known());
        assert_eq!(unknown.known(), None);
        assert_eq!(
            Resolved::<u32>::unknown(claim("f14.unknown.dimension"), "   "),
            Err(ResolvedError::EmptyReason),
            "an unknown without a reason is refused"
        );

        assert_eq!(
            Provenance::new(
                claim("f14.verified.no_source"),
                ClaimStatus::VerifiedOriginal,
                None
            ),
            Err(ProvenanceError::VerifiedOriginalWithoutSource),
            "verified_original without a located source is refused"
        );
    }

    /// AC04 at the schema level: only an installation origin is original; a
    /// synthetic fixture row never claims to be a retail entry.
    #[test]
    fn accept_f14_a_origin_distinguishes_synthetic_from_installation() {
        let installation = Origin::Installation { source: span() };
        assert!(installation.is_original());
        assert_eq!(installation.label(), "installation");
        assert_eq!(installation.source(), Some(&span()));
        assert!(!Origin::SyntheticFixture.is_original());
        assert!(!Origin::Designed.is_original());
        assert_eq!(Origin::SyntheticFixture.source(), None);
    }

    /// Parsing, normalization and readiness are independent states, and an
    /// element's per-record rules refuse silent readiness.
    #[test]
    fn accept_f14_a_element_states_are_independent_and_reasons_are_mandatory() {
        let mission = ContentId::from_source(ContentKind::Mission, "m01").expect("mission id");
        assert_eq!(ready_element(mission.clone()).validate(), Ok(()));

        let mut mismatch = ready_element(mission.clone());
        mismatch.kind = ContentKind::World;
        assert_eq!(
            mismatch.validate(),
            Err(ElementError::KindMismatch {
                id_kind: ContentKind::Mission,
                kind: ContentKind::World,
            })
        );

        let mut unavailable = ready_element(mission.clone());
        unavailable.readiness = Readiness::Unavailable;
        assert_eq!(
            unavailable.validate(),
            Err(ElementError::UnavailableWithoutReason),
            "an unavailable row must say why"
        );

        let mut ready_with_reason = ready_element(mission.clone());
        ready_with_reason
            .unsupported_reasons
            .push(UnsupportedReason::MissingParser);
        assert_eq!(
            ready_with_reason.validate(),
            Err(ElementError::ReadyWithUnsupportedReasons { count: 1 })
        );

        let mut unsupported = ready_element(mission);
        unsupported.readiness = Readiness::Unavailable;
        unsupported.parse_state = ParseState::Failed {
            diagnostic: "synthetic parse failure".to_owned(),
        };
        unsupported.normalize_state = NormalizeState::NotNormalized;
        unsupported
            .unsupported_reasons
            .push(UnsupportedReason::ParseFailed {
                diagnostic: String::new(),
            });
        assert_eq!(
            unsupported.validate(),
            Err(ElementError::EmptyUnsupportedDetail {
                code: "parse_failed"
            })
        );
        unsupported.unsupported_reasons = vec![UnsupportedReason::ParseFailed {
            diagnostic: "synthetic parse failure".to_owned(),
        }];
        assert_eq!(unsupported.validate(), Ok(()));
        assert!(!unsupported.is_ready());
        assert_eq!(unsupported.unsupported_codes(), vec!["parse_failed"]);
    }
}
