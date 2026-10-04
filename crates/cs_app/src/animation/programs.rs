//! Which reader-archive member drives which actor: the measured binding from a
//! mission's `startanims.zrd` startup event table to the `ANIMATION_DEFINITIONS`
//! members that declare the named animations, and from a definition's object
//! names to the scene nodes of the world container they select
//! (`M01-LC-WORLD-ACTORS`, task #632).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`
//! (stage `### F20-D`, the original-animation-family evidence stage) and
//! `specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`
//! (`### F34-D`, "verify each mission-required non-aircraft actor family").
//! Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # The gap this closes
//!
//! F20-D's [`super::survey`] proved every launchable scope **carries** an
//! animation carrier and fingerprinted it; F13-B located every reader-archive
//! member as a program and classified it only from its name and path. Between
//! those two, nothing read the members' bodies, so the question
//! `VS-M01-RUNTIME` needs answered — *which original member drives which
//! actor* — had no answer at all: the `.zrd` members of `zrdr.zbd` all decoded
//! as documents and none had a consumer.
//!
//! This module is that consumer, and it is deliberately a **binding**, not a
//! runtime. Four measured shapes carry it (every number in
//! `docs/findings/2026-10-04-m01-lc-world-actors.md` came out of the readers
//! below over the installation):
//!
//! 1. **The startup event table.** Every one of the 53 mission reader archives
//!    carries a `startanims.zrd` whose root is one record with exactly two
//!    fields, `NEW_GAME_START` and `LOAD_GAME_START`, each a list of
//!    one-element lists naming an animation. So a mission does **not** name a
//!    member at startup; it names an *animation*, and the animation name is
//!    resolved to a member afterwards.
//! 2. **The definition record.** An `ANIMATION_DEFINITIONS` member carries an
//!    `ANIMATION_LIST` of `ANIMATION_DEFINITION` entries (or, in the shared
//!    index, `ANIMATION_DEFINITION_FILE` entries naming the authoring paths the
//!    original built them from). A definition names the animation it implements
//!    (`ANIMATION_NAME`), the objects it drives, when it activates (`ACTIVATION`)
//!    and its statement sequence (`SEQUENCE_DEFINITION`).
//! 3. **The two object fields.** A definition names its objects one of two
//!    measured ways. `NAME` is a flat list of node names — `wv_tailhook`,
//!    `piratezep`. `NAME1` is a list of **pairs**: a state name bound to the node
//!    path that state drives, e.g. the state `wv_hookup_lights` bound to
//!    `workersvoyagezep/hookup_lights`. That pairing is the closest thing these
//!    records come to saying "this animation moves that object", and it is
//!    measured, not inferred.
//! 4. **The object selector.** An object name resolves against the scene nodes
//!    of a world container. Every literal name of the measured M01 closure
//!    resolves to exactly one node of `ZBD/C1C/gamez.zbd`; a name carrying `*`
//!    is a **prefix wildcard** and matches the numbered siblings the same family
//!    authors (`g_engine*` over `g_engine1..8`), which is why a wildcard resolves
//!    in one world container and in no other.
//!
//! # What this module does **not** claim
//!
//! * **Not `verified_original`.** Every fact below was read out of the owner's
//!   bytes by the production readers. `retail` is read access, not an original
//!   run: nothing here says what the 2000 engine did with these members.
//! * **Not a world-actor program.** A
//!   [`cs_content::world_actors::DeclaredWorldActorProgram`] needs a tick rate,
//!   a kind, a motion, sockets and pickups. A definition record states none of
//!   them: its `SEQUENCE_DEFINITION` entries name 37 statement kinds, none of
//!   which this module interprets. So the members resolve to a **measured
//!   binding** — an archive, a member, a byte span and the object selectors —
//!   and every field family that would be needed to lower one is named by
//!   [`UnmeasuredFieldFamily`] rather than invented.
//! * **Not a spawn/pose/route reading.** The mission members that *do* carry
//!   placement (`zeppelins.zrd`'s `node`/`position`/`yaw`/`pitch`/`max_speed`/
//!   `max_accel`) stay undecoded here: their `position` unit is task #436's
//!   measurement and it has not been made. The families are named.
//!
//! # The rule this module adds
//!
//! Exactly one is a **rule** rather than a measurement, and it carries its own
//! claim id so a reader can see which half is which:
//!
//! * [`OBJECT_SELECTOR_CLAIM`] — a stored object name containing `*` is a prefix
//!   wildcard over a node name. The bytes establish that `*` occurs and that no
//!   stored node name contains it; the **matching** rule is this project's, and
//!   the corpus is what supports it.
//!
//! Everything else is a measured vocabulary: an activation outside the three
//! measured values is [`Activation::Other`] and keeps its spelling, and a field
//! this module does not interpret is listed by name on the definition it sits
//! on, so "no record is silently dropped" is checkable by a caller.

use std::collections::BTreeSet;
use std::fmt;

use cs_content::stunts::{ZrdValue, decode_zrd, zrd_flat_fields};
use cs_formats::gamez::GameZNodes;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, Origin, Provenance};
use cs_types::evidence::ClaimId;

/// The reader-archive member every mission scope declares for its startup
/// animations (measured: all 53 mission scopes that declare a reader archive).
pub const STARTUP_MEMBER: &str = "startanims.zrd";

/// The record name an animation-definition member declares.
pub const ANIMATION_DEFINITIONS_RECORD: &str = "ANIMATION_DEFINITIONS";

/// The field an `ANIMATION_DEFINITIONS` body keeps its entries under.
pub const ANIMATION_LIST_FIELD: &str = "ANIMATION_LIST";

/// The entry that carries one definition inline.
pub const ANIMATION_DEFINITION_FIELD: &str = "ANIMATION_DEFINITION";

/// The entry that names another file the original built a definition set into.
pub const ANIMATION_DEFINITION_FILE_FIELD: &str = "ANIMATION_DEFINITION_FILE";

/// The field a definition uses for the animation it implements.
pub const ANIMATION_NAME_FIELD: &str = "ANIMATION_NAME";

/// The field a definition uses for a flat list of the node names it drives.
pub const NAME_FIELD: &str = "NAME";

/// The field a definition uses for state-name/node-path pairs.
pub const NAME_ALTERNATE_FIELD: &str = "NAME1";

/// The field that says when a definition activates.
pub const ACTIVATION_FIELD: &str = "ACTIVATION";

/// The field that says what another definition must already hold.
pub const ACTIVATION_PREREQUISITE_FIELD: &str = "ACTIVATION_PREREQUISITE";

/// The repeatable field holding one definition's statements.
pub const SEQUENCE_FIELD: &str = "SEQUENCE_DEFINITION";

/// The field a `SEQUENCE_DEFINITION` uses for its own name.
pub const SEQUENCE_NAME_FIELD: &str = "NAME";

/// The startup event a mission's fresh game fires.
pub const NEW_GAME_START: &str = "NEW_GAME_START";

/// The startup event a mission's loaded save fires.
pub const LOAD_GAME_START: &str = "LOAD_GAME_START";

/// The character that makes a stored object name a prefix wildcard.
///
/// Measured: `*` occurs in stored object names (for example `g_engine*` and
/// `deploy_bwzep_rbroad1*`) and in **no** stored scene-node name of any of the
/// nine measured GameZ containers, which is the observation
/// [`OBJECT_SELECTOR_CLAIM`] rests on.
pub const WILDCARD_CHAR: char = '*';

/// The character separating two node names inside one stored path.
pub const NODE_PATH_SEPARATOR: char = '/';

/// The claim filed under the one matching rule this module adds.
pub const OBJECT_SELECTOR_CLAIM: &str = "f20-anim.object-selector-wildcard-is-a-node-name-prefix";

/// The claim filed under the measured activation vocabulary.
pub const ACTIVATION_VOCABULARY_CLAIM: &str =
    "f20-anim.activation-vocabulary-measured-three-values";

/// The claim filed under the measured statement-kind census.
pub const SEQUENCE_KINDS_CLAIM: &str = "f20-anim.sequence-entry-kinds-measured-not-interpreted";

/// The prerequisite requirement that says every named condition must hold.
pub const REQUIRED_PREREQUISITE: &str = "REQUIRED";

/// The prerequisite requirement that says some named conditions must hold.
pub const OPTIONS_PREREQUISITE: &str = "OPTIONS";

/// The segment an `OPTIONS` prerequisite states its count in.
pub const MINIMUM_TO_SATISFY: &str = "MINIMUM_TO_SATISFY";

/// Why a member could not be read as the record it claims to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnimationProgramError {
    /// The member's bytes are not a decodable `.zrd` document.
    Decode {
        /// The reader's own stable code.
        code: &'static str,
        /// Byte offset inside the member the refusal was found at.
        offset: u64,
    },
    /// The document decoded but is not the measured shape for this record.
    Shape {
        /// The shape the reader expected.
        expected: &'static str,
        /// What the document held instead.
        reason: String,
    },
}

impl AnimationProgramError {
    /// The stable refusal code, for callers that match rather than read.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Decode { code, .. } => code,
            Self::Shape { .. } => "unexpected_shape",
        }
    }
}

impl fmt::Display for AnimationProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode { code, offset } => {
                write!(f, "the .zrd member is not decodable at {offset} ({code})")
            }
            Self::Shape { expected, reason } => {
                write!(f, "the .zrd member is not the {expected} shape: {reason}")
            }
        }
    }
}

impl std::error::Error for AnimationProgramError {}

/// One stored node name, read as a selector over a container's records.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SelectorSegment {
    /// The name is exactly this node name.
    Literal(String),
    /// The name is any node name with this prefix.
    PrefixWildcard(String),
}

impl SelectorSegment {
    /// The segment's literal spelling, or the wildcard's prefix.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Literal(name) | Self::PrefixWildcard(name) => name,
        }
    }

    /// Whether this segment is a wildcard.
    #[must_use]
    pub const fn is_wildcard(&self) -> bool {
        matches!(self, Self::PrefixWildcard(_))
    }

    /// Whether `name` is selected by this segment.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        match self {
            Self::Literal(expected) => name == expected,
            Self::PrefixWildcard(prefix) => name.starts_with(prefix),
        }
    }
}

/// One stored object name: a node name, possibly wildcarded.
///
/// The **stored** spelling is always kept, so a caller can report exactly what
/// the member said; the segment is this module's reading of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectSelector {
    stored: String,
    segment: SelectorSegment,
}

impl ObjectSelector {
    /// Reads one stored object name.
    ///
    /// # Errors
    ///
    /// [`AnimationProgramError::Shape`] when the spelling is empty or its
    /// wildcard has no prefix. An empty selector would select every record and a
    /// prefix-less wildcard would too; neither is a name the original can have
    /// authored, so both are refused instead of quietly matching everything.
    pub fn parse(stored: &str) -> Result<Self, AnimationProgramError> {
        let segment = match stored.split_once(WILDCARD_CHAR) {
            Some((prefix, _)) => SelectorSegment::PrefixWildcard(prefix.to_owned()),
            None => SelectorSegment::Literal(stored.to_owned()),
        };
        if segment.text().is_empty() {
            return Err(AnimationProgramError::Shape {
                expected: "an object selector",
                reason: format!("the stored name {stored:?} selects every record"),
            });
        }
        Ok(Self {
            stored: stored.to_owned(),
            segment,
        })
    }

    /// The name exactly as the member stored it.
    #[must_use]
    pub fn stored(&self) -> &str {
        &self.stored
    }

    /// The name's reading as a selector segment.
    #[must_use]
    pub const fn segment(&self) -> &SelectorSegment {
        &self.segment
    }

    /// Whether the stored name carries a wildcard.
    #[must_use]
    pub const fn has_wildcard(&self) -> bool {
        self.segment.is_wildcard()
    }

    /// The literal node name of a non-wildcard selector.
    #[must_use]
    pub fn literal_name(&self) -> Option<&str> {
        match &self.segment {
            SelectorSegment::Literal(name) => Some(name),
            SelectorSegment::PrefixWildcard(_) => None,
        }
    }

    /// The provenance of the **matching rule**, which is designed, not measured.
    ///
    /// The stored spelling and the presence of `*` are measurements; the
    /// decision that `*` means "any name with this prefix" is this project's
    /// rule, filed under [`OBJECT_SELECTOR_CLAIM`].
    ///
    /// # Panics
    ///
    /// Panics if [`OBJECT_SELECTOR_CLAIM`] is not a valid claim id. It is a
    /// module constant, so a malformed one is an authoring error rather than a
    /// runtime condition.
    #[must_use]
    pub fn rule_provenance(&self) -> Provenance {
        Provenance::designed(ClaimId::new(OBJECT_SELECTOR_CLAIM).expect("constant claim id"))
    }

    /// The measured presence of a wildcard, as a claim about the spelling.
    ///
    /// `None` when the stored name carries no wildcard; otherwise the spelling
    /// and the claim id the reading rests on.
    #[must_use]
    pub fn wildcard_evidence(&self) -> Option<(String, &'static str)> {
        self.has_wildcard()
            .then(|| (self.stored.clone(), OBJECT_SELECTOR_CLAIM))
    }
}

impl fmt::Display for ObjectSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.stored)
    }
}

/// One `/`-separated node path: the shape a `NAME1` pair and a prerequisite both
/// use.
///
/// Measured: the steps are node names and a step may carry the same `*`
/// wildcard a bare name does (`beowulfzep` / `rbroad1*`). This module keeps the
/// steps and resolves none of them: a path needs the container's parent/child
/// hierarchy, which a node-name table does not carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodePathSelector {
    stored: String,
    steps: Vec<ObjectSelector>,
}

impl NodePathSelector {
    /// Reads one stored node path.
    ///
    /// # Errors
    ///
    /// [`AnimationProgramError::Shape`] when a step is not a usable selector.
    pub fn parse(stored: &str) -> Result<Self, AnimationProgramError> {
        let steps = stored
            .split(NODE_PATH_SEPARATOR)
            .map(ObjectSelector::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            stored: stored.to_owned(),
            steps,
        })
    }

    /// The path exactly as the member stored it.
    #[must_use]
    pub fn stored(&self) -> &str {
        &self.stored
    }

    /// The path's steps, outermost first.
    #[must_use]
    pub fn steps(&self) -> &[ObjectSelector] {
        &self.steps
    }

    /// The path's last step, which a hierarchy would walk down to.
    #[must_use]
    pub fn terminal(&self) -> Option<&ObjectSelector> {
        self.steps.last()
    }

    /// Whether any step carries a wildcard.
    #[must_use]
    pub fn has_wildcard(&self) -> bool {
        self.steps.iter().any(ObjectSelector::has_wildcard)
    }
}

impl fmt::Display for NodePathSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.stored)
    }
}

/// One `NAME1` pair: a state name and the node path that state drives.
///
/// This is the strongest measured statement these records make about which
/// object an animation moves: the state `wv_hookup_lights` is bound to
/// `workersvoyagezep/hookup_lights`. Whether the engine resolves that binding by
/// name at load time or by a build-time reference is **unmeasured**, and nothing
/// here depends on the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateBinding {
    state: String,
    path: NodePathSelector,
}

impl StateBinding {
    /// Records one state binding.
    #[must_use]
    pub fn new(state: impl Into<String>, path: NodePathSelector) -> Self {
        Self {
            state: state.into(),
            path,
        }
    }

    /// The state name, exactly as the member stored it.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Whether the state name carries a wildcard.
    #[must_use]
    pub fn state_is_wildcarded(&self) -> bool {
        self.state.contains(WILDCARD_CHAR)
    }

    /// The node path this state drives.
    #[must_use]
    pub const fn path(&self) -> &NodePathSelector {
        &self.path
    }
}

/// The three activation values the installation states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeasuredActivation {
    /// `ON_CALL`: the animation runs when something calls it by name.
    OnCall,
    /// `ON_STARTUP`: the animation runs when the scope starts.
    OnStartup,
    /// `WEAPON_OR_COLLIDE_HIT`: the animation runs on a hit event.
    WeaponOrCollideHit,
}

impl MeasuredActivation {
    /// The stored spelling of this value.
    #[must_use]
    pub const fn stored(self) -> &'static str {
        match self {
            Self::OnCall => "ON_CALL",
            Self::OnStartup => "ON_STARTUP",
            Self::WeaponOrCollideHit => "WEAPON_OR_COLLIDE_HIT",
        }
    }
}

/// What a definition's `ACTIVATION` field says.
///
/// A value outside the measured three is [`Self::Other`] with its spelling
/// intact: the census is a measurement of what this installation uses, never a
/// closed enumeration, so an unseen value must not be refused or coerced.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Activation {
    /// One of the three measured values.
    Measured(MeasuredActivation),
    /// A value this installation does not use, retained verbatim.
    Other(String),
}

impl Activation {
    /// Reads one stored activation value.
    #[must_use]
    pub fn measured(stored: &str) -> Self {
        match stored {
            "ON_CALL" => Self::Measured(MeasuredActivation::OnCall),
            "ON_STARTUP" => Self::Measured(MeasuredActivation::OnStartup),
            "WEAPON_OR_COLLIDE_HIT" => Self::Measured(MeasuredActivation::WeaponOrCollideHit),
            other => Self::Other(other.to_owned()),
        }
    }

    /// The stored spelling, whether measured or not.
    #[must_use]
    pub fn stored(&self) -> &str {
        match self {
            Self::Measured(value) => value.stored(),
            Self::Other(value) => value,
        }
    }

    /// Whether this activation is the startup one.
    #[must_use]
    pub const fn is_startup(&self) -> bool {
        matches!(self, Self::Measured(MeasuredActivation::OnStartup))
    }

    /// Whether the stored value is one of the three measured ones.
    #[must_use]
    pub const fn is_measured(&self) -> bool {
        matches!(self, Self::Measured(_))
    }
}

/// The three kinds of thing a prerequisite names, plus anything else retained.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PrerequisiteCondition {
    /// `OBJECT_ACTIVE_LIST`: node paths that must be active.
    ObjectActiveList,
    /// `OBJECT_INACTIVE_LIST`: node paths that must be inactive.
    ObjectInactiveList,
    /// `ANIMATION_LIST`: animations that must have finished.
    AnimationList,
    /// A condition this installation does not use, retained verbatim.
    Other(String),
}

impl PrerequisiteCondition {
    /// Reads one stored condition name.
    #[must_use]
    pub fn measured(stored: &str) -> Self {
        match stored {
            "OBJECT_ACTIVE_LIST" => Self::ObjectActiveList,
            "OBJECT_INACTIVE_LIST" => Self::ObjectInactiveList,
            "ANIMATION_LIST" => Self::AnimationList,
            other => Self::Other(other.to_owned()),
        }
    }

    /// The stored spelling, whether measured or not.
    #[must_use]
    pub fn stored(&self) -> &str {
        match self {
            Self::ObjectActiveList => "OBJECT_ACTIVE_LIST",
            Self::ObjectInactiveList => "OBJECT_INACTIVE_LIST",
            Self::AnimationList => "ANIMATION_LIST",
            Self::Other(value) => value,
        }
    }

    /// Whether this condition names node paths rather than animations.
    #[must_use]
    pub const fn names_node_paths(&self) -> bool {
        matches!(self, Self::ObjectActiveList | Self::ObjectInactiveList)
    }
}

/// A definition's `ACTIVATION_PREREQUISITE`: what must already hold.
///
/// Measured shape: `[REQUIRED | OPTIONS, [CONDITION, …]]`, where an `OPTIONS`
/// body leads with `MINIMUM_TO_SATISFY` and its count. This module reads that
/// structure and does **not** evaluate it — see
/// [`UnmeasuredFieldFamily::PrerequisiteEvaluation`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationPrerequisite {
    requirement: String,
    minimum_to_satisfy: Option<u32>,
    condition: PrerequisiteCondition,
    animation_names: Vec<String>,
    paths: Vec<NodePathSelector>,
}

impl ActivationPrerequisite {
    /// The stored requirement: `REQUIRED` or `OPTIONS`.
    #[must_use]
    pub fn requirement(&self) -> &str {
        &self.requirement
    }

    /// The count an `OPTIONS` prerequisite states, when it states one.
    #[must_use]
    pub const fn minimum_to_satisfy(&self) -> Option<u32> {
        self.minimum_to_satisfy
    }

    /// What kind of thing the prerequisite names.
    #[must_use]
    pub const fn condition(&self) -> &PrerequisiteCondition {
        &self.condition
    }

    /// The animation names an `ANIMATION_LIST` prerequisite requires.
    #[must_use]
    pub fn animation_names(&self) -> &[String] {
        &self.animation_names
    }

    /// The node paths an object-list prerequisite requires.
    #[must_use]
    pub fn paths(&self) -> &[NodePathSelector] {
        &self.paths
    }
}

/// One statement inside a `SEQUENCE_DEFINITION`.
///
/// The statement's kind and its own field names are kept; none of its payload is
/// interpreted. That is the honest reading of a record whose statements the
/// original executed through code this project has not measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceEntry {
    kind: String,
    fields: Vec<String>,
}

impl SequenceEntry {
    /// The statement kind, for example `OBJECT_MOTION_FROM_TO`.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The statement's own field names, in stored order.
    #[must_use]
    pub fn fields(&self) -> &[String] {
        &self.fields
    }
}

/// One definition's `SEQUENCE_DEFINITION`: its statements, in stored order.
///
/// The record's own `NAME` field is the sequence's name — every measured
/// sequence states one, and it is the same `callback_sequence` in most of them —
/// so it is read as [`Self::name`] and is **not** counted as a statement. A
/// reader that walked it as one would report a `NAME` "statement kind" that no
/// animation system executes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationSequence {
    name: Option<String>,
    entries: Vec<SequenceEntry>,
}

impl AnimationSequence {
    /// The sequence's own stored name, when it states one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The statements, in stored order.
    #[must_use]
    pub fn entries(&self) -> &[SequenceEntry] {
        &self.entries
    }

    /// The distinct statement kinds this sequence uses, sorted.
    #[must_use]
    pub fn kinds(&self) -> BTreeSet<&str> {
        self.entries.iter().map(SequenceEntry::kind).collect()
    }
}

/// How a definition names the objects it drives, in one of the two measured
/// shapes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefinitionObjects {
    /// `NAME`: a flat list of node names.
    Names(Vec<ObjectSelector>),
    /// `NAME1`: alternating pairs of a state name and the node path it drives.
    StateBindings(Vec<StateBinding>),
    /// The definition carries neither field, so it names no object at all.
    Absent,
}

impl DefinitionObjects {
    /// The field this shape came from, or `None` when the definition has none.
    #[must_use]
    pub const fn field(&self) -> Option<&'static str> {
        match self {
            Self::Names(_) => Some(NAME_FIELD),
            Self::StateBindings(_) => Some(NAME_ALTERNATE_FIELD),
            Self::Absent => None,
        }
    }

    /// The node names, over both shapes: a `NAME` entry and a `NAME1` path's
    /// terminal step are both node names a caller may resolve.
    pub fn node_names(&self) -> Box<dyn Iterator<Item = &ObjectSelector> + '_> {
        match self {
            Self::Names(names) => Box::new(names.iter()),
            Self::StateBindings(bindings) => Box::new(
                bindings
                    .iter()
                    .filter_map(|binding| binding.path().terminal()),
            ),
            Self::Absent => Box::new(std::iter::empty()),
        }
    }

    /// Every stored selector over both shapes: a `NAME` entry, or every step of
    /// a `NAME1` path.
    pub fn selectors(&self) -> Box<dyn Iterator<Item = &ObjectSelector> + '_> {
        match self {
            Self::Names(names) => Box::new(names.iter()),
            Self::StateBindings(bindings) => {
                Box::new(bindings.iter().flat_map(|binding| binding.path().steps()))
            }
            Self::Absent => Box::new(std::iter::empty()),
        }
    }

    /// Whether this shape carries a wildcard anywhere.
    #[must_use]
    pub fn has_wildcard(&self) -> bool {
        self.selectors().any(ObjectSelector::has_wildcard)
    }
}

/// One `ANIMATION_DEFINITION` of an `ANIMATION_DEFINITIONS` member.
///
/// Every field the record stores is either read into a field of this record or
/// listed by name in [`Self::uninterpreted_fields`]. There is no third case: a
/// definition cannot carry a field this record silently dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredAnimationDefinition {
    index: usize,
    animation_name: Option<String>,
    objects: DefinitionObjects,
    activation: Option<Activation>,
    prerequisite: Option<ActivationPrerequisite>,
    sequences: Vec<AnimationSequence>,
    uninterpreted_fields: Vec<String>,
}

impl DeclaredAnimationDefinition {
    /// The definition's position inside its member, in stored order.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The animation this definition implements, when it declares one.
    #[must_use]
    pub fn animation_name(&self) -> Option<&str> {
        self.animation_name.as_deref()
    }

    /// How the definition names the objects it drives.
    #[must_use]
    pub const fn objects(&self) -> &DefinitionObjects {
        &self.objects
    }

    /// When this definition activates.
    #[must_use]
    pub fn activation(&self) -> Option<&Activation> {
        self.activation.as_ref()
    }

    /// What must already hold before this definition activates.
    #[must_use]
    pub fn prerequisite(&self) -> Option<&ActivationPrerequisite> {
        self.prerequisite.as_ref()
    }

    /// The definition's statement sequences, in stored order.
    #[must_use]
    pub fn sequences(&self) -> &[AnimationSequence] {
        &self.sequences
    }

    /// The fields this record stores and does not interpret, sorted.
    #[must_use]
    pub fn uninterpreted_fields(&self) -> &[String] {
        &self.uninterpreted_fields
    }

    /// Every distinct statement kind this definition uses, sorted.
    #[must_use]
    pub fn sequence_kinds(&self) -> BTreeSet<&str> {
        self.sequences
            .iter()
            .flat_map(AnimationSequence::kinds)
            .collect()
    }
}

/// One `ANIMATION_DEFINITIONS` member, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationDefinitionMember {
    member: String,
    record: String,
    fields: Vec<String>,
    definitions: Vec<DeclaredAnimationDefinition>,
    definition_files: Vec<String>,
}

impl AnimationDefinitionMember {
    /// The member's name, as the archive's index spells it.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// The record name the member declared (`ANIMATION_DEFINITIONS`).
    #[must_use]
    pub fn record(&self) -> &str {
        &self.record
    }

    /// The record body's own field names, in stored order.
    #[must_use]
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// Every definition, in stored order.
    #[must_use]
    pub fn definitions(&self) -> &[DeclaredAnimationDefinition] {
        &self.definitions
    }

    /// The authoring paths the member names through
    /// [`ANIMATION_DEFINITION_FILE`], in stored order.
    ///
    /// These are **not** installable paths: they are the paths the original's
    /// authoring tree used (`..\data\common\zrdr\planes\player.zrd`). Their
    /// basenames match member names, which is measured; the directory they came
    /// from is not, so nothing here resolves one to a container.
    #[must_use]
    pub fn definition_files(&self) -> &[String] {
        &self.definition_files
    }

    /// Every definition that activates at startup, in stored order.
    pub fn startup_definitions(&self) -> impl Iterator<Item = &DeclaredAnimationDefinition> {
        self.definitions
            .iter()
            .filter(|definition| definition.activation().is_some_and(Activation::is_startup))
    }

    /// Every distinct statement kind this member's definitions use, sorted.
    #[must_use]
    pub fn sequence_kinds(&self) -> BTreeSet<&str> {
        self.definitions
            .iter()
            .flat_map(DeclaredAnimationDefinition::sequence_kinds)
            .collect()
    }

    /// Every definition implementing `animation`, in stored order.
    pub fn definitions_of(
        &self,
        animation: &str,
    ) -> impl Iterator<Item = &DeclaredAnimationDefinition> {
        self.definitions
            .iter()
            .filter(move |definition| definition.animation_name() == Some(animation))
    }
}

/// Reads one `ANIMATION_DEFINITIONS` member.
///
/// The member's root is a one-element list holding the record body, which is a
/// flat `KEY, value` list. The record body is read as far as the fields this
/// module names and **every** other field is recorded by name on the definition
/// it sits on, so a caller can enumerate what was not read.
///
/// # Errors
///
/// [`AnimationProgramError::Decode`] when the bytes are not a decodable `.zrd`
/// document, and [`AnimationProgramError::Shape`] when the document is not the
/// measured record shape or names an object this module cannot read as a
/// selector.
pub fn read_animation_definition_member(
    member: &str,
    bytes: &[u8],
) -> Result<AnimationDefinitionMember, AnimationProgramError> {
    let document = decode_zrd(bytes).map_err(|error| AnimationProgramError::Decode {
        code: error.code(),
        offset: error.offset(),
    })?;
    let record = zrd_record(&document, ANIMATION_DEFINITIONS_RECORD)?;
    let Some(body) = field(record, ANIMATION_DEFINITIONS_RECORD) else {
        return Err(AnimationProgramError::Shape {
            expected: "an ANIMATION_DEFINITIONS record body",
            reason: "the record carries no body of its own name".to_owned(),
        });
    };
    let list = list_of(body, ANIMATION_LIST_FIELD)?;
    let mut fields = Vec::new();
    let mut definitions = Vec::new();
    let mut definition_files = Vec::new();
    for (name, value) in zrd_flat_fields(list) {
        match name {
            ANIMATION_DEFINITION_FIELD => {
                definitions.push(read_definition(definitions.len(), value)?);
            }
            ANIMATION_DEFINITION_FILE_FIELD => definition_files.push(single_text(value)),
            other => fields.push(other.to_owned()),
        }
    }
    Ok(AnimationDefinitionMember {
        member: member.to_owned(),
        record: ANIMATION_DEFINITIONS_RECORD.to_owned(),
        fields,
        definitions,
        definition_files,
    })
}

/// Reads one `startanims.zrd` startup event table.
///
/// # Errors
///
/// [`AnimationProgramError::Decode`] when the bytes are not a decodable `.zrd`
/// document, and [`AnimationProgramError::Shape`] when a field's value is not a
/// list of names.
pub fn read_startup_animations(
    bytes: &[u8],
) -> Result<StartupAnimationTable, AnimationProgramError> {
    let document = decode_zrd(bytes).map_err(|error| AnimationProgramError::Decode {
        code: error.code(),
        offset: error.offset(),
    })?;
    let record = zrd_root_record(&document, "a startup event table")?;
    let mut events = Vec::new();
    for (name, value) in zrd_flat_fields(record) {
        let children = value
            .as_list()
            .ok_or_else(|| AnimationProgramError::Shape {
                expected: "a startup event's animation list",
                reason: format!("the {name} field is not a list"),
            })?;
        events.push(StartupEvent {
            event: name.to_owned(),
            animation_names: children.iter().map(single_text).collect(),
        });
    }
    Ok(StartupAnimationTable { events })
}

/// One startup event and the animations it fires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupEvent {
    event: String,
    animation_names: Vec<String>,
}

impl StartupEvent {
    /// The stored event name, for example [`NEW_GAME_START`].
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The animation names the event fires, in stored order.
    #[must_use]
    pub fn animation_names(&self) -> &[String] {
        &self.animation_names
    }
}

/// A `startanims.zrd` member's startup event table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupAnimationTable {
    events: Vec<StartupEvent>,
}

impl StartupAnimationTable {
    /// Every event, in stored order.
    #[must_use]
    pub fn events(&self) -> &[StartupEvent] {
        &self.events
    }

    /// The animations one event fires, or `None` when the member declares no
    /// such event — a measured absence, not an empty list.
    #[must_use]
    pub fn event(&self, event: &str) -> Option<&[String]> {
        self.events
            .iter()
            .find(|entry| entry.event() == event)
            .map(StartupEvent::animation_names)
    }

    /// Every animation the table names, in stored order across every event.
    pub fn animation_names(&self) -> impl Iterator<Item = &str> {
        self.events
            .iter()
            .flat_map(|entry| entry.animation_names.iter().map(String::as_str))
    }

    /// Whether the table names no animation at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.animation_names().next().is_none()
    }
}

/// One definition, located in the archive that declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationDefinitionSite {
    archive: String,
    member: String,
    span: SourceSpan,
    definition: DeclaredAnimationDefinition,
}

impl AnimationDefinitionSite {
    /// Records a located definition.
    #[must_use]
    pub fn new(
        archive: impl Into<String>,
        member: impl Into<String>,
        span: SourceSpan,
        definition: DeclaredAnimationDefinition,
    ) -> Self {
        Self {
            archive: archive.into(),
            member: member.into(),
            span,
            definition,
        }
    }

    /// The archive's logical key, for example `zbd/c1c/m01/zrdr.zbd`.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// The member's name, as the archive's index spells it.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// Where the declaring member's bytes live.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// The definition itself.
    #[must_use]
    pub const fn definition(&self) -> &DeclaredAnimationDefinition {
        &self.definition
    }

    /// The animation this definition implements.
    #[must_use]
    pub fn animation_name(&self) -> Option<&str> {
        self.definition.animation_name()
    }

    /// How the definition names the objects it drives.
    #[must_use]
    pub const fn objects(&self) -> &DefinitionObjects {
        self.definition.objects()
    }
}

/// How one startup animation name resolved against the scopes read.
///
/// Three outcomes, and the middle one is the point: a name two members declare
/// is **ambiguous**, never resolved by preferring the mission archive or the
/// first hit. Nothing measured here says which one the original ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingResolution {
    /// Exactly one definition declares the animation.
    Single(Box<AnimationDefinitionSite>),
    /// More than one declares it; none is privileged.
    Ambiguous(Vec<AnimationDefinitionSite>),
    /// No definition in the scopes read declares it.
    Unresolved,
}

impl BindingResolution {
    /// The one declaring definition, when exactly one declares it.
    #[must_use]
    pub fn single(&self) -> Option<&AnimationDefinitionSite> {
        match self {
            Self::Single(site) => Some(site),
            Self::Ambiguous(_) | Self::Unresolved => None,
        }
    }

    /// Every declaring definition, when there is more than one.
    #[must_use]
    pub fn ambiguous(&self) -> Option<&[AnimationDefinitionSite]> {
        match self {
            Self::Ambiguous(sites) => Some(sites),
            Self::Single(_) | Self::Unresolved => None,
        }
    }

    /// Whether the name resolved to exactly one definition.
    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Single(_))
    }

    /// Whether two or more definitions declare the name.
    #[must_use]
    pub const fn is_ambiguous(&self) -> bool {
        matches!(self, Self::Ambiguous(_))
    }

    /// Whether no definition in the scopes read declares the name.
    #[must_use]
    pub const fn is_unresolved(&self) -> bool {
        matches!(self, Self::Unresolved)
    }

    /// How many definitions declare the name.
    #[must_use]
    pub fn declaring_sites(&self) -> usize {
        match self {
            Self::Single(_) => 1,
            Self::Ambiguous(sites) => sites.len(),
            Self::Unresolved => 0,
        }
    }
}

/// One startup event's animation and where it was declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartupAnimationBinding {
    event: String,
    animation_name: String,
    resolution: BindingResolution,
}

impl StartupAnimationBinding {
    /// Binds one startup animation name.
    #[must_use]
    pub fn new(
        event: impl Into<String>,
        animation_name: impl Into<String>,
        resolution: BindingResolution,
    ) -> Self {
        Self {
            event: event.into(),
            animation_name: animation_name.into(),
            resolution,
        }
    }

    /// The startup event that fires this animation.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// The animation name the event fires.
    #[must_use]
    pub fn animation_name(&self) -> &str {
        &self.animation_name
    }

    /// Where the animation was declared.
    #[must_use]
    pub const fn resolution(&self) -> &BindingResolution {
        &self.resolution
    }
}

/// Resolves one animation name against every declaration read, in search order.
///
/// Zero hits is [`BindingResolution::Unresolved`], one hit is
/// [`BindingResolution::Single`] and two or more is
/// [`BindingResolution::Ambiguous`]. No hit order decides a winner: the search
/// order is the *reporting* order, and preferring the mission archive over the
/// shared one would be a rule this measurement does not support.
fn resolve_animation(
    animation: &str,
    declarations: &[AnimationDefinitionSite],
) -> BindingResolution {
    let mut hits = declarations
        .iter()
        .filter(|site| site.animation_name() == Some(animation))
        .cloned();
    let Some(first) = hits.next() else {
        return BindingResolution::Unresolved;
    };
    let Some(second) = hits.next() else {
        return BindingResolution::Single(Box::new(first));
    };
    let mut sites = vec![first, second];
    sites.extend(hits);
    BindingResolution::Ambiguous(sites)
}

/// What one mission scope's reader archives bind, measured.
///
/// This is the task's answer for a mission: which members its startup fires,
/// which definitions those names land on, and which definitions the scope itself
/// activates at startup. It is **not** a
/// [`cs_content::world_actors::DeclaredWorldActorProgram`]: the records say
/// nothing about motion, sockets, pickups or a tick rate, and
/// [`UnmeasuredFieldFamily`] names every field family that would be needed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldActorProgramBinding {
    scope: String,
    archives: Vec<String>,
    members: Vec<String>,
    startup: Vec<StartupAnimationBinding>,
    startup_activations: Vec<AnimationDefinitionSite>,
}

impl WorldActorProgramBinding {
    /// Assembles a scope's binding from the scopes read for it.
    ///
    /// `startup` is the mission's [`STARTUP_MEMBER`] event table read through
    /// `read_startup_animations`; `declarations` is every definition site the
    /// scope's own archive, its world group and the shared root declare, in that
    /// search order. The order decides **which** sites an ambiguous name carries,
    /// never which one wins.
    #[must_use]
    pub fn new(
        scope: impl Into<String>,
        archives: Vec<String>,
        members: Vec<String>,
        startup: StartupAnimationTable,
        declarations: &[AnimationDefinitionSite],
    ) -> Self {
        let bindings = startup
            .events
            .iter()
            .flat_map(|entry| {
                entry.animation_names.iter().map(move |name| {
                    StartupAnimationBinding::new(
                        entry.event(),
                        name,
                        resolve_animation(name, declarations),
                    )
                })
            })
            .collect();
        Self {
            scope: scope.into(),
            archives,
            members,
            startup: bindings,
            startup_activations: declarations
                .iter()
                .filter(|site| {
                    site.definition
                        .activation()
                        .is_some_and(Activation::is_startup)
                })
                .cloned()
                .collect(),
        }
    }

    /// The scope's own archive key, for example `zbd/c1c/m01/zrdr.zbd`.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// Every archive read for the scope, in search order.
    #[must_use]
    pub fn archives(&self) -> &[String] {
        &self.archives
    }

    /// Every member of the scope's own archive, in stored order.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }

    /// Every startup binding, in event order then stored order.
    #[must_use]
    pub fn startup(&self) -> &[StartupAnimationBinding] {
        &self.startup
    }

    /// Every definition that activates at startup, in search order.
    #[must_use]
    pub fn startup_activations(&self) -> &[AnimationDefinitionSite] {
        &self.startup_activations
    }

    /// The bindings of one event, in stored order.
    pub fn startup_of(&self, event: &str) -> impl Iterator<Item = &StartupAnimationBinding> {
        self.startup
            .iter()
            .filter(move |binding| binding.event() == event)
    }

    /// The bindings that resolved to exactly one definition.
    pub fn resolved(&self) -> impl Iterator<Item = &StartupAnimationBinding> {
        self.startup
            .iter()
            .filter(|binding| binding.resolution().is_resolved())
    }

    /// The bindings two or more members declare.
    ///
    /// A distinct list from [`Self::unresolved`] on purpose: "nobody declares
    /// this animation" and "two members declare it and this measurement cannot
    /// say which one ran" are different failures, and a caller that collapsed
    /// them would report a missing actor where the data is contradictory.
    pub fn ambiguous(&self) -> impl Iterator<Item = &StartupAnimationBinding> {
        self.startup
            .iter()
            .filter(|binding| binding.resolution().is_ambiguous())
    }

    /// The bindings no member of the scopes read declares.
    pub fn unresolved(&self) -> impl Iterator<Item = &StartupAnimationBinding> {
        self.startup
            .iter()
            .filter(|binding| binding.resolution().is_unresolved())
    }

    /// The node names a resolved startup animation drives, in binding order,
    /// each with the site that declared it.
    ///
    /// A definition naming its objects through [`DefinitionObjects::StateBindings`]
    /// contributes each path's terminal step, because that is the node the state
    /// ends up moving; a path's outer steps need the container hierarchy and are
    /// not resolved here.
    pub fn selected_objects(
        &self,
    ) -> impl Iterator<Item = (&AnimationDefinitionSite, &ObjectSelector)> {
        self.startup.iter().filter_map(|binding| {
            let site = binding.resolution().single()?;
            Some((site, site.objects().node_names().next()?))
        })
    }
}

/// One GameZ container's node names, as a resolution target.
///
/// Only the **stored display name** of each record is used. A
/// [`NodePathSelector`] needs the container's parent/child hierarchy, and this
/// table deliberately does not carry one: [`SelectorMatch::Path`] is reported
/// instead of a guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldNodeNames {
    container: String,
    names: Vec<String>,
}

impl WorldNodeNames {
    /// Reads the node names of a container the production node reader decoded.
    #[must_use]
    pub fn from_gamez(container: impl Into<String>, nodes: &GameZNodes) -> Self {
        Self {
            container: container.into(),
            names: nodes.nodes.iter().map(|node| node.name.clone()).collect(),
        }
    }

    /// The container's logical key.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// How many records the container holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the container holds no record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Every stored record name, in stored order.
    ///
    /// The names are the records' stored display names, verbatim: no case
    /// folding, no escaping and no de-duplication, so two records that share a
    /// name are both here and a count over them is the count the container
    /// holds. This is the table [`Self::resolve`] searches.
    pub fn node_names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// Resolves one object name against this container's records.
    ///
    /// A literal name and a prefix wildcard are counted against the stored names,
    /// which is the measurement: how many records of this container the name
    /// selects. A **zero** is a measurement too — the container holds no such
    /// record, which is exactly what says a family is not placed in this world.
    #[must_use]
    pub fn resolve(&self, selector: &ObjectSelector) -> SelectorMatch {
        match selector.segment() {
            SelectorSegment::Literal(name) => SelectorMatch::Node {
                name: name.clone(),
                occurrences: self.names.iter().filter(|stored| *stored == name).count(),
            },
            SelectorSegment::PrefixWildcard(prefix) => SelectorMatch::Family {
                prefix: prefix.clone(),
                occurrences: self
                    .names
                    .iter()
                    .filter(|stored| stored.starts_with(prefix))
                    .count(),
            },
        }
    }

    /// Reports one node path as unmeasured by this table.
    ///
    /// A path's steps are not resolved, not even its terminal step: the terminal
    /// name is only meaningful *under* its parent, so matching it on its own
    /// would report a hit in the wrong world. The returned match carries the
    /// path so a caller can report which one it could not answer.
    ///
    /// The container is named in the result, so an unmeasured path still says
    /// which container could not answer it.
    #[must_use]
    pub fn resolve_path(&self, path: &NodePathSelector) -> SelectorMatch {
        SelectorMatch::Path {
            path: path.stored().to_owned(),
        }
    }
}

/// How one object name met one container's records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectorMatch {
    /// The name is one literal node name.
    Node {
        /// The node name the selector names.
        name: String,
        /// How many records of the container carry it.
        occurrences: usize,
    },
    /// The name is one prefix wildcard.
    Family {
        /// The wildcard's stored prefix.
        prefix: String,
        /// How many records of the container carry a name with that prefix.
        occurrences: usize,
    },
    /// The selector is a node path; a node-name table has no hierarchy, so how
    /// many records it selects is unmeasured here.
    Path {
        /// The stored path.
        path: String,
    },
}

impl SelectorMatch {
    /// How many records the selector selects.
    ///
    /// **`Some(0)` is a measurement**, not a gap: the container was searched and
    /// holds no record with that name or prefix, which is exactly what says the
    /// family is not placed in this world. `None` means this table could not
    /// answer at all, which only a node path does.
    #[must_use]
    pub const fn occurrences(&self) -> Option<usize> {
        match self {
            Self::Node { occurrences, .. } | Self::Family { occurrences, .. } => Some(*occurrences),
            Self::Path { .. } => None,
        }
    }

    /// Whether this table could not answer.
    #[must_use]
    pub const fn is_unmeasured(&self) -> bool {
        matches!(self, Self::Path { .. })
    }
}

/// A field family this measurement reached and deliberately did not interpret.
///
/// Each family names its own claim id and what it blocks, so a consumer can
/// report "the member said X and the workspace cannot use it yet" instead of
/// reading a default out of it. The families are the answer to the task's second
/// acceptance branch: the members resolve to a measured binding, and the field
/// families that stop them becoming a world-actor program are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnmeasuredFieldFamily {
    /// A `SEQUENCE_DEFINITION` statement's meaning.
    StatementSemantics,
    /// Whether `ACTIVATION` has values beyond the three measured.
    ActivationVocabulary,
    /// Whether `*` really is a prefix wildcard, and whether other wildcards
    /// exist.
    SelectorWildcardSemantics,
    /// A `NAME1` path or a prerequisite's path: which node under which parent.
    NodePathResolution,
    /// Whether the engine resolves a `NAME1` state binding at load time or by a
    /// build-time reference, and whether the state name is itself addressable.
    StateBindingResolution,
    /// `RESET_TIME` and `RESET_STATE`: what they reset and after how long.
    ResetFields,
    /// `EXECUTION_PRIORITY`, `EXECUTION_BY_RENDER`, `EXECUTION_BY_RANGE`.
    ExecutionOrder,
    /// `AUTO_ADD_TO_WORLD` and `LOCAL_NODES_ONLY`: what is added, and when.
    WorldMembership,
    /// `ANIMATION_DEFINITION_FILE` authoring paths: how a member resolves to
    /// another file, and whether the directories mean anything.
    DefinitionFilePaths,
    /// `HEALTH`, `DAMAGE_SEQUENCE`, `PROXIMITY_DAMAGE`, `COPY_NODE_DATA`: the
    /// damage coupling an authored destruction chain states.
    DamageCoupling,
    /// `SAVE_LOG`, `NETWORK_LOG`, `PERSIST_LOG`: the replay/network records.
    PersistenceFlags,
    /// `ACTIVATION_PREREQUISITE`: how a prerequisite is evaluated.
    PrerequisiteEvaluation,
    /// The placement members (`node`/`position`/`yaw`/`pitch`/`max_speed`/
    /// `max_accel`): spawn and route for a placed actor.
    PlacementRecords,
    /// The stored-vertex unit every length in these records is stated in.
    StoredUnit,
    /// The tick rate an animation program runs at.
    TickRate,
}

impl UnmeasuredFieldFamily {
    /// Every family, in a stable report order.
    pub const ALL: [Self; 15] = [
        Self::StatementSemantics,
        Self::ActivationVocabulary,
        Self::SelectorWildcardSemantics,
        Self::NodePathResolution,
        Self::StateBindingResolution,
        Self::ResetFields,
        Self::ExecutionOrder,
        Self::WorldMembership,
        Self::DefinitionFilePaths,
        Self::DamageCoupling,
        Self::PersistenceFlags,
        Self::PrerequisiteEvaluation,
        Self::PlacementRecords,
        Self::StoredUnit,
        Self::TickRate,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::StatementSemantics => "statement_semantics",
            Self::ActivationVocabulary => "activation_vocabulary",
            Self::SelectorWildcardSemantics => "selector_wildcard_semantics",
            Self::NodePathResolution => "node_path_resolution",
            Self::StateBindingResolution => "state_binding_resolution",
            Self::ResetFields => "reset_fields",
            Self::ExecutionOrder => "execution_order",
            Self::WorldMembership => "world_membership",
            Self::DefinitionFilePaths => "definition_file_paths",
            Self::DamageCoupling => "damage_coupling",
            Self::PersistenceFlags => "persistence_flags",
            Self::PrerequisiteEvaluation => "prerequisite_evaluation",
            Self::PlacementRecords => "placement_records",
            Self::StoredUnit => "stored_unit",
            Self::TickRate => "tick_rate",
        }
    }

    /// The claim id this family is filed under.
    #[must_use]
    pub const fn claim_id(self) -> &'static str {
        match self {
            Self::StatementSemantics => SEQUENCE_KINDS_CLAIM,
            Self::ActivationVocabulary => ACTIVATION_VOCABULARY_CLAIM,
            Self::SelectorWildcardSemantics => OBJECT_SELECTOR_CLAIM,
            Self::NodePathResolution => "f20-anim.node-path-resolution-unmeasured",
            Self::StateBindingResolution => "f20-anim.name1-state-binding-resolution-unmeasured",
            Self::ResetFields => "f20-anim.reset-fields-unmeasured",
            Self::ExecutionOrder => "f20-anim.execution-order-fields-unmeasured",
            Self::WorldMembership => "f20-anim.world-membership-fields-unmeasured",
            Self::DefinitionFilePaths => "f20-anim.definition-file-paths-unresolved",
            Self::DamageCoupling => "f20-anim.damage-coupling-unmeasured",
            Self::PersistenceFlags => "f20-anim.persistence-flags-unmeasured",
            Self::PrerequisiteEvaluation => "f20-anim.activation-prerequisite-unevaluated",
            Self::PlacementRecords => "f34-world.placement-records-unmeasured",
            Self::StoredUnit => "f18-world.stored-vertex-unit-unmeasured",
            Self::TickRate => "f20-anim.tick-rate-unmeasured",
        }
    }

    /// The claim id as a validated [`ClaimId`].
    ///
    /// # Panics
    ///
    /// Panics if a claim id above is malformed. Every value is a module
    /// constant, so a malformed one is an authoring error rather than a
    /// runtime condition.
    #[must_use]
    pub fn claim(self) -> ClaimId {
        ClaimId::new(self.claim_id()).expect("constant claim id")
    }

    /// What is unmeasured, and what it blocks.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::StatementSemantics => {
                "the record names each statement kind (OBJECT_MOTION_FROM_TO, CALL_ANIMATION, IF, ...) \
                 and this module reads no statement's payload, so no pose, route, spawn or \
                 gameplay transition is recovered"
            }
            Self::ActivationVocabulary => {
                "the installation states ON_CALL, ON_STARTUP and WEAPON_OR_COLLIDE_HIT; nothing \
                 establishes that this is the closed set, so a value outside it is retained by \
                 spelling instead of refused"
            }
            Self::SelectorWildcardSemantics => {
                "a stored object name may carry '*' and no stored scene-node name does; treating \
                 '*' as a node-name prefix is this project's rule, corroborated by the corpus \
                 (g_engine* over g_engine1..8) rather than read from the bytes"
            }
            Self::NodePathResolution => {
                "a NAME1 pair and a prerequisite both use '/'-separated node paths \
                 (workersvoyagezep/hookup_lights); a node-name table carries no hierarchy, so such \
                 a path is reported unmeasured rather than matched on its last step"
            }
            Self::StateBindingResolution => {
                "NAME1 binds a state name to a node path, which is the strongest statement these \
                 records make about which object an animation moves; whether the engine resolved \
                 that binding by name at load time or by a build-time reference is unmeasured"
            }
            Self::ResetFields => {
                "RESET_TIME and RESET_STATE are stored (eleven and twenty distinct shapes \
                 respectively) and neither the unit, the two-word meaning nor the reset target is \
                 measured"
            }
            Self::ExecutionOrder => {
                "EXECUTION_PRIORITY, EXECUTION_BY_RENDER and EXECUTION_BY_RANGE are stored with \
                 values and no measured meaning; ordering two definitions cannot be recovered"
            }
            Self::WorldMembership => {
                "AUTO_ADD_TO_WORLD states only OFF in this installation and LOCAL_NODES_ONLY states \
                 an empty list in 457 definitions; what is added to the world, and when, is \
                 unmeasured"
            }
            Self::DefinitionFilePaths => {
                "ANIMATION_DEFINITION_FILE names the original's authoring paths (for example \
                 ..\\data\\common\\zrdr\\planes\\player.zrd); their basenames match member names \
                 but the directories are not installation-relative, so a path resolves to no \
                 container here"
            }
            Self::DamageCoupling => {
                "HEALTH, DAMAGE_SEQUENCE, PROXIMITY_DAMAGE and COPY_NODE_DATA state an authored \
                 destruction chain; the damage model's thresholds and the sequences' roles are \
                 F29's measurement and have not been made"
            }
            Self::PersistenceFlags => {
                "SAVE_LOG, NETWORK_LOG and PERSIST_LOG are stored as ON/OFF and the log they write \
                 is not located, so nothing can be replayed or reproduced from them yet"
            }
            Self::PrerequisiteEvaluation => {
                "ACTIVATION_PREREQUISITE is read as its measured requirement/condition structure \
                 and is never evaluated: whether a named node is active, inactive or has finished \
                 another animation is a runtime predicate this module cannot decide"
            }
            Self::PlacementRecords => {
                "the members that place an actor (node/position/yaw/pitch/max_speed/max_accel) are \
                 not read here: F33-D recorded the carrier's presence and no task has decoded its \
                 encoding, so a spawn or a route has no measured source"
            }
            Self::StoredUnit => {
                "every length, position and speed in these records is in the original's stored unit, \
                 which task #436 has not measured; no value may be read as metres or as knots"
            }
            Self::TickRate => {
                "no member of these archives states a tick rate or a frame time, so no sequence \
                 duration can be turned into simulation ticks"
            }
        }
    }

    /// The provenance of the **absence**: the reason is a statement about what
    /// has not been measured, never a claim about the original's behaviour.
    #[must_use]
    pub fn absence_provenance(self) -> Provenance {
        Provenance::designed(self.claim())
    }

    /// The measured content id of this family's spelling, for a caller that
    /// files the gap under a stable content key.
    ///
    /// # Errors
    ///
    /// Returns the id error when the derived key is not a valid
    /// [`ContentId`]; the label is a module constant, so a malformed one is an
    /// authoring error.
    pub fn content_id(&self) -> Result<ContentId, cs_types::content::ContentIdError> {
        ContentId::from_source(cs_types::content::ContentKind::AnimationTrack, self.label())
    }

    /// Whether a family is a measurement of the original bytes.
    ///
    /// Always `false` for these families: each one is a statement that something
    /// has **not** been established, so it can never be reported as
    /// installation-derived.
    #[must_use]
    pub const fn is_measured(&self) -> bool {
        false
    }

    /// The origin every one of these families carries.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        Origin::Designed
    }
}

// --------------------------------- document helpers -------------------------

/// The single record a `.zrd` member's root holds.
fn zrd_root_record<'a>(
    document: &'a ZrdValue,
    expected: &'static str,
) -> Result<&'a ZrdValue, AnimationProgramError> {
    let children = document
        .as_list()
        .ok_or_else(|| AnimationProgramError::Shape {
            expected,
            reason: "the member's root is not a list".to_owned(),
        })?;
    let record = children
        .first()
        .ok_or_else(|| AnimationProgramError::Shape {
            expected,
            reason: "the member's root holds no record".to_owned(),
        })?;
    if record.as_list().is_none() {
        return Err(AnimationProgramError::Shape {
            expected,
            reason: "the member's root does not hold a record list".to_owned(),
        });
    }
    Ok(record)
}

/// The record of `name` inside a member's root, or a refusal.
///
/// The record name is checked because a member's *first* text is not
/// necessarily its record: `startanims.zrd`'s is `NEW_GAME_START`, so a reader
/// that trusted the first key would file the startup table under a record it
/// never declared.
fn zrd_record<'a>(
    document: &'a ZrdValue,
    name: &'static str,
) -> Result<&'a ZrdValue, AnimationProgramError> {
    let record = zrd_root_record(document, name)?;
    let first = record
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_text);
    if first != Some(name) {
        return Err(AnimationProgramError::Shape {
            expected: name,
            reason: match first {
                Some(other) => format!("the member declares the record {other:?}"),
                None => "the member's first field is not text".to_owned(),
            },
        });
    }
    Ok(record)
}

/// The value of one field of a flat record.
fn field<'a>(record: &'a ZrdValue, name: &str) -> Option<&'a ZrdValue> {
    zrd_flat_fields(record)
        .into_iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// A list-valued field of a flat record.
fn list_of<'a>(
    record: &'a ZrdValue,
    name: &'static str,
) -> Result<&'a ZrdValue, AnimationProgramError> {
    let value = field(record, name).ok_or_else(|| AnimationProgramError::Shape {
        expected: name,
        reason: format!("the record carries no {name} field"),
    })?;
    if value.as_list().is_none() {
        return Err(AnimationProgramError::Shape {
            expected: name,
            reason: format!("the {name} field is not a list"),
        });
    }
    Ok(value)
}

/// The children of a list-valued field.
fn children(value: &ZrdValue) -> &[ZrdValue] {
    value.as_list().unwrap_or_default()
}

/// One stored name, refusing a value that is not exactly one text.
fn single_text(value: &ZrdValue) -> String {
    let mut found = texts(value).into_iter();
    match (found.next(), found.next()) {
        (Some(text), None) => text,
        // A value that is not a name at all is reported as the empty string: it
        // can never match a node, so a caller reading the names sees the miss
        // instead of a plausible actor.
        _ => String::new(),
    }
}

/// Every text a value holds, flattening nested lists.
fn texts(value: &ZrdValue) -> Vec<String> {
    match value {
        ZrdValue::Text(text) => vec![text.clone()],
        ZrdValue::List(kids) => kids.iter().flat_map(texts).collect(),
        ZrdValue::Int(_) | ZrdValue::Float(_) => Vec::new(),
    }
}

/// The `/`-joined spelling of one stored node path.
///
/// Measured: a path is stored as a list of steps — `["wv_tailhook",
/// "pickup_node"]` — except when it has a single step, which the records store
/// both as a bare text (`phut_healthy`) and as a one-element list
/// (`["panelleftb1"]`). Both shapes are read here, because the two spellings are
/// measurements and neither is the reader's to normalize away. The spelling this
/// returns is therefore **assembled**, so a caller that needs the original
/// nesting keeps [`NodePathSelector::steps`].
fn path_text(value: &ZrdValue) -> String {
    let steps: Vec<String> = match value {
        ZrdValue::List(_) => children(value).iter().map(single_text).collect(),
        // A single-step path stored as a bare text.
        _ => vec![single_text(value)],
    };
    steps.join(&NODE_PATH_SEPARATOR.to_string())
}

/// One definition, read from an `ANIMATION_DEFINITION` value.
fn read_definition(
    index: usize,
    value: &ZrdValue,
) -> Result<DeclaredAnimationDefinition, AnimationProgramError> {
    let fields = zrd_flat_fields(value);
    let mut animation_name = None;
    let mut objects = DefinitionObjects::Absent;
    let mut activation = None;
    let mut prerequisite = None;
    let mut sequences = Vec::new();
    let mut interpreted: BTreeSet<&str> = BTreeSet::new();
    for (name, field) in &fields {
        match *name {
            ANIMATION_NAME_FIELD => {
                interpreted.insert(ANIMATION_NAME_FIELD);
                animation_name = texts(field).into_iter().next();
            }
            NAME_FIELD => {
                interpreted.insert(NAME_FIELD);
                objects = DefinitionObjects::Names(read_selectors(field)?);
            }
            NAME_ALTERNATE_FIELD => {
                interpreted.insert(NAME_ALTERNATE_FIELD);
                objects = DefinitionObjects::StateBindings(read_state_bindings(field)?);
            }
            ACTIVATION_FIELD => {
                interpreted.insert(ACTIVATION_FIELD);
                activation = texts(field)
                    .into_iter()
                    .next()
                    .map(|stored| Activation::measured(&stored));
            }
            ACTIVATION_PREREQUISITE_FIELD => {
                interpreted.insert(ACTIVATION_PREREQUISITE_FIELD);
                prerequisite = read_prerequisite(field)?;
            }
            SEQUENCE_FIELD => {
                interpreted.insert(SEQUENCE_FIELD);
                sequences.push(read_sequence(field));
            }
            _ => {}
        }
    }
    let mut uninterpreted_fields: Vec<String> = fields
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .filter(|name| !interpreted.contains(name.as_str()))
        .collect();
    uninterpreted_fields.sort();
    uninterpreted_fields.dedup();
    Ok(DeclaredAnimationDefinition {
        index,
        animation_name,
        objects,
        activation,
        prerequisite,
        sequences,
        uninterpreted_fields,
    })
}

/// A `NAME` field's node names, in stored order.
fn read_selectors(value: &ZrdValue) -> Result<Vec<ObjectSelector>, AnimationProgramError> {
    children(value)
        .iter()
        .map(|child| ObjectSelector::parse(&single_text(child)))
        .collect()
}

/// A `NAME1` field's state/path pairs, in stored order.
///
/// Measured: the field alternates a state name with the path list that follows
/// it, so the count is always even. An odd count means the record is not the
/// measured shape and is refused rather than paired up with nothing.
fn read_state_bindings(value: &ZrdValue) -> Result<Vec<StateBinding>, AnimationProgramError> {
    let kids = children(value);
    if !kids.len().is_multiple_of(2) {
        return Err(AnimationProgramError::Shape {
            expected: "a NAME1 state/path pair list",
            reason: format!(
                "the field holds {} entries, which is not an even number of pairs",
                kids.len()
            ),
        });
    }
    kids.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            Ok(StateBinding::new(
                single_text(&pair[0]),
                NodePathSelector::parse(&path_text(&pair[1]))?,
            ))
        })
        .collect()
}

/// An `ACTIVATION_PREREQUISITE` field, read as its measured structure.
///
/// Measured: `[REQUIRED | OPTIONS, body]`, where `body` is
/// `[CONDITION, …]` and an `OPTIONS` body leads with `MINIMUM_TO_SATISFY` and its
/// count. The condition's payload is a list of animation names for
/// `ANIMATION_LIST` and a list of node paths for the two object lists.
fn read_prerequisite(
    value: &ZrdValue,
) -> Result<Option<ActivationPrerequisite>, AnimationProgramError> {
    let root = children(value);
    let requirement = single_text(root.first().ok_or_else(|| AnimationProgramError::Shape {
        expected: "an activation prerequisite",
        reason: "the field is empty".to_owned(),
    })?);
    let body = children(root.get(1).ok_or_else(|| AnimationProgramError::Shape {
        expected: "an activation prerequisite",
        reason: format!("the {requirement:?} requirement carries no condition"),
    })?);
    let mut cursor = 0;
    let minimum_to_satisfy = if requirement == OPTIONS_PREREQUISITE {
        if single_text(body.first().ok_or_else(|| AnimationProgramError::Shape {
            expected: "an OPTIONS prerequisite",
            reason: "the body is empty".to_owned(),
        })?) != MINIMUM_TO_SATISFY
        {
            return Err(AnimationProgramError::Shape {
                expected: "an OPTIONS prerequisite",
                reason: format!(
                    "the body does not lead with {MINIMUM_TO_SATISFY}, so the requirement count is \
                     not stated where this reading expects it"
                ),
            });
        }
        let count = body
            .get(1)
            .and_then(ZrdValue::as_list)
            .and_then(|value| value.first())
            .and_then(ZrdValue::as_int);
        cursor = 2;
        count
    } else {
        None
    };
    let condition =
        PrerequisiteCondition::measured(&single_text(body.get(cursor).ok_or_else(|| {
            AnimationProgramError::Shape {
                expected: "an activation prerequisite",
                reason: format!("the {requirement:?} requirement names no condition"),
            }
        })?));
    let empty = ZrdValue::List(Vec::new());
    let payload = children(body.get(cursor + 1).unwrap_or(&empty));
    let (animation_names, paths) = if condition.names_node_paths() {
        let mut paths = Vec::new();
        for path in payload {
            paths.push(NodePathSelector::parse(&path_text(path))?);
        }
        (Vec::new(), paths)
    } else {
        (payload.iter().map(single_text).collect(), Vec::new())
    };
    Ok(Some(ActivationPrerequisite {
        requirement,
        minimum_to_satisfy,
        condition,
        animation_names,
        paths,
    }))
}

/// One `SEQUENCE_DEFINITION`, read as its statement kinds and field names.
fn read_sequence(value: &ZrdValue) -> AnimationSequence {
    let mut name = None;
    let mut entries = Vec::new();
    for (kind, field) in zrd_flat_fields(value) {
        if kind == SEQUENCE_NAME_FIELD {
            name = texts(field).into_iter().next();
            continue;
        }
        entries.push(SequenceEntry {
            kind: kind.to_owned(),
            fields: zrd_flat_fields(field)
                .into_iter()
                .map(|(name, _)| name.to_owned())
                .collect(),
        });
    }
    AnimationSequence { name, entries }
}
