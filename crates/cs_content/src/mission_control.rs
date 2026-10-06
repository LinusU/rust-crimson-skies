//! The measured **mission control program**: which reader member carries it, the
//! directive vocabulary its numbered objective blocks spell, and what each
//! directive's measured signature does and does not license the engine to do.
//!
//! Task: `M01-LC-MISSION-PROGRAM` (#630). Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("Source adapter acceptance", "Host
//! interface", "IR requirements"); sibling finding
//! `docs/findings/2026-10-04-m01-lc-mission-program.md`.
//!
//! # What this module settles, and what it deliberately does not
//!
//! F13-B located 1452 "programs" in the installation by name and path and was
//! careful to say it had located **none** of them: no instruction unit, no
//! opcode table, an empty ledger, 0 of 1452 resolved. The corpus search never
//! found the mission language because for a mission-scoped reader the mission
//! language is not the byte layout — it is a **typed keyed list**. A
//! `zrdr.zbd` member decodes through [`crate::stunts::decode_zrd`] into
//! `int`/`float`/`text`/`list` nodes, and the member that carries a mission's
//! control program is the one whose record declares numbered `OBJECTIVE<N>`
//! blocks. That rule is checked here, per member, rather than assumed by name:
//! [`control_member`] answers `Some` for exactly the member whose decoded record
//! declares such a block, and a mission with zero or two such members is a
//! [`ControlMemberError`], never a guess at a filename.
//!
//! Once the member is located, this module measures **every** directive its
//! blocks spell — the key, the argument shape beside it and how many blocks and
//! sites carry it — and gives each distinct key a
//! [`DirectiveDisposition`]. The disposition vocabulary is three answers and no
//! fourth:
//!
//! * [`DirectiveDisposition::TerminalOutcome`] — the mission IR has an action for
//!   it ([`crate::objectives::FAILURE_KEY_VOCABULARY`], whose two spellings the
//!   installation writes beside no argument list);
//! * [`DirectiveDisposition::Measured`] — a stage B/C/D finding measured the
//!   key's **effect**: the operation the original performs is named, with the
//!   evidence for the measurement and the residual unknowns it leaves. Measured
//!   is not support: the key still has no engine operation, and the lowering
//!   accounting names the argument shapes and bindings it still lacks;
//! * [`DirectiveDisposition::Unmeasured`] with a named
//!   [`UnmeasuredReason`] — the spelling is measured and its **effect** is not,
//!   either because no original observation states what the key does
//!   ([`UnmeasuredReason::MeaningNotMeasured`]), because its own sites disagree
//!   about their argument shape
//!   ([`UnmeasuredReason::DisagreeingArgumentShape`]), or because the shape it
//!   spells has no value in the mission IR ([`UnmeasuredReason::ArgumentShapeHasNoValue`]).
//!
//! There is deliberately **no** "partially understood" and **no** "probably this"
//! variant. A directive the engine cannot honour is counted and named, never
//! lowered to a no-op and never silently dropped: [`MeasuredControlRecord::sites`]
//! is the sum over every key, and [`MeasuredControlRecord::sites`] is checked
//! against the walk that produced it by the acceptance suite.
//!
//! # Why the record does not lower to a [`cs_script::ir::MissionProgram`] yet
//!
//! [`MeasuredControlRecord::lowering`] is the honest accounting. `lower_program`
//! (`cs_script::bindings`) takes a `RawProgram` whose objectives each carry a
//! `ContentId` and a `Condition`, and whose calls carry a flat
//! `Vec<cs_script::ir::Value>`. The control record meets **two** of those four
//! requirements and fails the other two on measured grounds:
//!
//! | What `lower_program` needs | What the record spells | Verdict |
//! | --- | --- | --- |
//! | the mission's `ContentId` | nothing; the member is mission-scoped by *path* | supplied outside the member (`missions/bindings/M01.json`, M01-A) |
//! | an objective `ContentId` per block | each block is authored under its own `OBJECTIVE<N>` key, and every cross-objective directive addresses a block by its zero-based index (measured); `IDENTITY` is measured to supply the presentation class and HUD ordinal, not the identity | [`LoweringRequirement::ObjectiveIdentity`], **met** |
//! | a `Condition` per block | measured evaluators that read live world state — member handles, registry bytes, animation states, the named counters — several with side effects during evaluation; none is the side-effect-free `Condition` the field needs | [`LoweringRequirement::ObjectiveCondition`], unmeasured |
//! | a flat `Vec<Value>` per call | measured operations — lifecycle writes, evaluator arming, ordered pipeline effects — none of which is a `cs_script::bindings::Lowering` variant; plus sites that disagree about shape and nested shapes the IR cannot carry | [`LoweringRequirement::CallArguments`], unmeasured |
//!
//! Each row is reported as a [`LoweringRequirement`] carrying the measured
//! numbers behind it and the fields that remain unknown, so the reader can see
//! *why* the member is Unsupported rather than only that it is.
//! [`LoweringRequirement::is_met`] is false for a retail record today, and
//! [`ControlLowering::complete`] is false with it — nothing downstream may call
//! a mission playable off this measurement (AGENTS.md rule 4, contract "If the
//! actual program is unavailable or cannot be decoded, the mission remains
//! Unsupported").
//!
//! # What is measured and what is not
//!
//! Every spelling in this module was measured over the owner's installation on
//! 2026-10-04 by reading each mission's reader archive through production
//! discovery, decoding it with the production `.zrd` reader and walking the
//! numbered blocks. What any spelling **does** is unmeasured: no original
//! executable has been run, and `INSTANTWIN` naming a win is a reading of a
//! name, not an observation of behaviour. Nothing here is `verified_original`
//! (AGENTS.md rule 8), and no `content` field below is a decoded meaning.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::objectives::{
    OBJECTIVE_DORMANT_KEY, OBJECTIVE_IDENTITY_KEY, OBJECTIVE_INACTIVE_COUNT_KEY,
    OBJECTIVE_INACTIVE_STAGE_PREFIX, is_objective_inactive_stage, objective_block_number,
};
use crate::stunts::{ZrdValue, objective_record, zrd_flat_fields};

/// `OBJECTIVE17` → `Some(17)`; anything else → `None`.
///
/// The same block rule the rest of the project counts with
/// ([`objective_block_number`]), so this module cannot disagree with F13-B's or
/// F39-D's about which keys are blocks.
fn is_objective_block(key: &str) -> bool {
    objective_block_number(key).is_some()
}

/// The terminal outcome one measured outcome key names.
///
/// **A reading of a spelling.** Measured: the installation writes
/// [`crate::objectives::FAILURE_KEY_VOCABULARY`] and neither spelling ever carries an
/// argument list (M01: `INSTANTWIN` and `INSTANTLOSS`, one site each, both bare).
/// That `INSTANTWIN` asks for success is an inference from the name; no original
/// executable has been run, so the reading is recorded as a disposition and
/// stays one. The acceptance suite pins that this function answers for every key
/// in [`crate::objectives::FAILURE_KEY_VOCABULARY`] and for nothing else, so the
/// two vocabularies cannot drift apart.
#[must_use]
pub fn terminal_outcome_of(key: &str) -> Option<TerminalOutcome> {
    match key {
        "INSTANTWIN" => Some(TerminalOutcome::Succeeded),
        "INSTANTLOSS" => Some(TerminalOutcome::Failed),
        _ => None,
    }
}

/// The reader-archive member a mission's control program lives in.
///
/// **Measured**, not assumed: over M01's twelve members exactly one —
/// `objectives.zrd` — decodes to a record declaring numbered `OBJECTIVE<N>`
/// blocks, and the same is true of every mission-scoped reader the census
/// measured. The constant is the *name* that measurement settles on, kept for
/// diagnostics and cross-checks; [`control_member`] is what actually decides, so
/// a member the original ever renames is still found and a member that stops
/// carrying blocks stops being accepted under this name.
pub const CONTROL_MEMBER: &str = crate::stunts::SCENARIO_OBJECTIVES_MEMBER;

/// The measured record keys a mission's control record carries **outside** its
/// numbered objective blocks.
///
/// Measured over M01 (`zbd/c1c/m01`), whose record holds exactly these five:
/// `MISSION_TIMER` and `PLAYER_INIT` plus the three animation lists
/// `RESTORE_ANIMS`, `EXECUTE_ANIMS` and `INVALIDATE_ANIMS`. Their values are
/// counted by [`MeasuredControlRecord::record_fields`] and read by
/// [`crate::objectives`], never interpreted here: what `MISSION_TIMER`'s single
/// number or `PLAYER_INIT`'s five is stays **unmeasured**, and
/// [`ControlRecordField::support`] says so for each.
pub const CONTROL_RECORD_KEY_VOCABULARY: [&str; 5] = [
    "MISSION_TIMER",
    "PLAYER_INIT",
    "RESTORE_ANIMS",
    "EXECUTE_ANIMS",
    "INVALIDATE_ANIMS",
];

/// Why a reader archive's control member could not be established.
///
/// The archive is **refused**, never guessed: a reader with no member that
/// declares objective blocks, or one with two, has no single control program and
/// picking either would be a guess about which half drives the mission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlMemberError {
    /// No member of the archive decodes to a record declaring numbered
    /// `OBJECTIVE<N>` blocks.
    NoControlMember {
        /// The archive's logical key, as `zbd/<group>/<mission>/zrdr.zbd`.
        container: String,
        /// How many members the archive declares and therefore offered.
        members: usize,
    },
    /// More than one member declares numbered blocks, so the archive does not
    /// have one control program.
    AmbiguousControlMember {
        /// The archive's logical key.
        container: String,
        /// The members that declare blocks, sorted.
        members: Vec<String>,
    },
}

impl fmt::Display for ControlMemberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoControlMember { container, members } => write!(
                f,
                "{container}: none of its {members} member(s) declares a numbered objective block"
            ),
            Self::AmbiguousControlMember { container, members } => write!(
                f,
                "{container}: {} members declare numbered objective blocks ({})",
                members.len(),
                members.join(", ")
            ),
        }
    }
}

impl std::error::Error for ControlMemberError {}

/// One decoded member offered to [`control_member`], with what the walk found in
/// it.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedMember {
    /// The member's name inside its archive, as production discovery spells it.
    pub name: String,
    /// Its decoded document.
    pub document: ZrdValue,
}

impl DecodedMember {
    /// Builds one entry.
    #[must_use]
    pub fn new(name: impl Into<String>, document: ZrdValue) -> Self {
        Self {
            name: name.into(),
            document,
        }
    }
}

/// How many numbered `OBJECTIVE<N>` blocks one member's decoded record declares.
#[must_use]
pub fn objective_blocks_of(member: &DecodedMember) -> u32 {
    zrd_flat_fields(objective_record(&member.document))
        .into_iter()
        .filter(|(key, _)| is_objective_block(key))
        .count() as u32
}

/// The one member of `members` that carries a mission's control program.
///
/// **The measurement rule, in one place.** A member is the control program
/// exactly when its decoded record declares at least one numbered
/// `OBJECTIVE<N>` block. Nothing else about a member counts: not its name, not
/// its length, not its position in the archive. That is what makes the rule
/// falsifiable — M01's `wv_tailhook.zrd` is 38639 bytes beside its 24012-byte
/// `objectives.zrd` (**1.61x**, measured) and carries no block at all, so the
/// size-and-name heuristic the task description started from
/// ([`CONTROL_MEMBER`]'s doc) is measured to be wrong, and this function is what
/// replaces it.
///
/// # Errors
///
/// [`ControlMemberError::NoControlMember`] when no member qualifies and
/// [`ControlMemberError::AmbiguousControlMember`] when more than one does.
pub fn control_member<'a>(
    container: &str,
    members: &'a [DecodedMember],
) -> Result<&'a DecodedMember, ControlMemberError> {
    let found: Vec<&DecodedMember> = members
        .iter()
        .filter(|member| objective_blocks_of(member) > 0)
        .collect();
    match found.as_slice() {
        [only] => Ok(only),
        [] => Err(ControlMemberError::NoControlMember {
            container: container.to_owned(),
            members: members.len(),
        }),
        many => Err(ControlMemberError::AmbiguousControlMember {
            container: container.to_owned(),
            members: many.iter().map(|m| m.name.clone()).collect(),
        }),
    }
}

// ---------------------------------------------------------------------------
// The measured directive signature
// ---------------------------------------------------------------------------

/// The measured shape of one argument of a control directive.
///
/// Ordered and **total**: `[int, float]` and `[float, int]` are different
/// shapes, and a list is described by its own children's shapes rather than by
/// its length, because two lists of the same length can mean different things
/// (`[text,text]` is a `SET_HELP_LABEL`'s node and label, `[[text,text]]` is an
/// `ADD_OBJECTIVE_TARGET`'s node pair).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeasuredArg {
    /// A `.zrd` int node.
    Int,
    /// A `.zrd` float node.
    Float,
    /// A `.zrd` text node.
    Text,
    /// A `.zrd` list node with no children.
    Empty,
    /// A `.zrd` list node, described by its children.
    List(Vec<MeasuredArg>),
}

impl MeasuredArg {
    /// The stable label a report carries: `int`, `float`, `text`, `[]` or
    /// `[a,b,c]`.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Int => "int".to_owned(),
            Self::Float => "float".to_owned(),
            Self::Text => "text".to_owned(),
            Self::Empty => "[]".to_owned(),
            Self::List(children) => {
                let inner: Vec<String> = children.iter().map(Self::label).collect();
                format!("[{}]", inner.join(","))
            }
        }
    }

    /// Whether this shape can be written into a `cs_script::ir::Value`.
    ///
    /// **No.** The IR's value types are `bool`, checked integer, finite float,
    /// string, content id, actor, vector and optional actor — a *list* is not
    /// one of them. So a directive whose argument list holds a list has a shape
    /// the IR cannot carry, which is what
    /// [`UnmeasuredReason::ArgumentShapeHasNoValue`] reports rather than
    /// flattening it into an invented order.
    #[must_use]
    pub const fn is_ir_carriable(&self) -> bool {
        matches!(self, Self::Int | Self::Float | Self::Text)
    }
}

/// What one directive site spells beside its key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirectiveShape {
    /// **No argument list is written at all.** Measured: the two outcome keys
    /// (`INSTANTWIN`, `INSTANTLOSS`) appear in M01 with the next key straight
    /// after them, which is how the original spells "this directive takes no
    /// argument" — a `.zrd` list of two children, not an empty list.
    Bare,
    /// The value beside the key is a **scalar** (an int or a float node), not a
    /// list.
    ///
    /// Reachable and separate from [`DirectiveShape::Bare`] because the grammar
    /// decides it: a `.zrd` argument list is always a list node, so a scalar
    /// follower cannot be the next directive's key — keys are text. A **text**
    /// follower is the ambiguous case, and it is read as the next key; see
    /// [`DirectiveShape::Bare`].
    NotAList,
    /// A list, described by its arguments' shapes in order.
    Arguments(Vec<MeasuredArg>),
}

impl DirectiveShape {
    /// The stable label a report carries.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Bare => "bare".to_owned(),
            Self::NotAList => "not_a_list".to_owned(),
            Self::Arguments(args) => {
                let inner: Vec<String> = args.iter().map(MeasuredArg::label).collect();
                format!("[{}]", inner.join(","))
            }
        }
    }

    /// Whether this shape can be written into a `cs_script::ir::Value`.
    ///
    /// A bare directive needs no value at all, so it is carriable; see
    /// [`MeasuredArg::is_ir_carriable`] for why a list is not.
    #[must_use]
    pub fn is_ir_carriable(&self) -> bool {
        match self {
            Self::Bare => true,
            Self::NotAList => false,
            Self::Arguments(args) => args.iter().all(MeasuredArg::is_ir_carriable),
        }
    }

    /// How many top-level arguments this shape carries, counting a bare key as
    /// zero.
    #[must_use]
    pub fn arity(&self) -> usize {
        match self {
            Self::Bare => 0,
            Self::NotAList => 1,
            Self::Arguments(args) => args.len(),
        }
    }
}

impl fmt::Display for DirectiveShape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// The measured shape of one `.zrd` value, for the recursive [`MeasuredArg`].
fn arg_shape(value: &ZrdValue) -> MeasuredArg {
    match value {
        ZrdValue::Int(_) => MeasuredArg::Int,
        ZrdValue::Float(_) => MeasuredArg::Float,
        ZrdValue::Text(_) => MeasuredArg::Text,
        ZrdValue::List(children) if children.is_empty() => MeasuredArg::Empty,
        ZrdValue::List(children) => MeasuredArg::List(children.iter().map(arg_shape).collect()),
    }
}

/// One key the record spells, with everything measured about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredDirectiveKey {
    /// The key as the original spells it.
    pub key: String,
    /// How many numbered blocks spell it.
    pub blocks: u32,
    /// How many sites it has: one per block that spells it, plus one per extra
    /// occurrence inside a block (measured: M01 spells a key at most once per
    /// block, so `sites == blocks` today and the census reports both rather than
    /// assuming they are equal).
    pub sites: u32,
    /// Every measured shape beside this key, with the sites carrying it, sorted
    /// by shape label so the report is stable.
    pub shapes: Vec<(DirectiveShape, u32)>,
}

impl MeasuredDirectiveKey {
    /// The dominant shape, or `None` when the sites disagree — which is exactly
    /// the case [`UnmeasuredReason::DisagreeingArgumentShape`] refuses, so the
    /// two can never disagree about what "dominant" means here.
    #[must_use]
    pub fn agreed_shape(&self) -> Option<&DirectiveShape> {
        match self.shapes.as_slice() {
            [(shape, _)] => Some(shape),
            _ => None,
        }
    }

    /// What the engine may do with this key.
    ///
    /// The effect question is asked **before** the shape question: a key whose
    /// semantics a finding measured is [`DirectiveDisposition::Measured`] even
    /// when its own sites disagree about their shape (`INACTIVE1`) or spell a
    /// shape the IR cannot carry (`ANIM_STATE`) — the shape defect belongs to
    /// the lowering accounting, which names it per site, not to the
    /// disposition, which would otherwise report a measured key as *unknown*.
    /// Shape refusals remain, for the keys no finding covers.
    #[must_use]
    pub fn disposition(&self) -> DirectiveDisposition {
        if let Some(outcome) = terminal_outcome_of(&self.key) {
            return DirectiveDisposition::TerminalOutcome { outcome };
        }
        if let Some(measured) = measured_directive(&self.key) {
            return DirectiveDisposition::Measured(measured);
        }
        if self.shapes.len() > 1 {
            return DirectiveDisposition::Unmeasured {
                reason: UnmeasuredReason::DisagreeingArgumentShape {
                    shapes: self.shapes.len(),
                },
            };
        }
        match self.agreed_shape() {
            Some(shape) if !shape.is_ir_carriable() => DirectiveDisposition::Unmeasured {
                reason: UnmeasuredReason::ArgumentShapeHasNoValue {
                    shape: shape.clone(),
                },
            },
            _ => DirectiveDisposition::Unmeasured {
                reason: UnmeasuredReason::MeaningNotMeasured,
            },
        }
    }
}

/// The measured terminal outcome an outcome key requests.
///
/// A **reading of a spelling**: measured that the installation writes the two
/// keys and that neither carries an argument list, and that
/// `cs_script::ir::Outcome` has an action for a terminal request. That
/// `INSTANTWIN` requests `Succeeded` is an inference from the name and stays one
/// (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TerminalOutcome {
    /// A success request.
    Succeeded,
    /// A failure request.
    Failed,
}

impl TerminalOutcome {
    /// The stable label a report carries.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for TerminalOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why one measured key's effect is not implemented.
///
/// Three reasons and no fourth, each naming what is missing. A fourth kind —
/// "probably this" — is the thing this vocabulary exists to prevent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnmeasuredReason {
    /// No original observation states what the key does.
    ///
    /// The spelling is measured; its effect is not. This is the reason **most**
    /// of a retail record's keys carry, and it cannot be discharged by reading
    /// the name harder.
    MeaningNotMeasured,
    /// The key's own sites disagree about their argument shape.
    ///
    /// Measured in M01 for `INACTIVE1`: ten sites spell
    /// `[text,text,text]` — a node, a part and a part-state — and two spell
    /// `[text]` — a node alone. Both are kept; neither is declared the real one,
    /// because a reader that picked the majority would be inventing a rule the
    /// original does not state.
    DisagreeingArgumentShape {
        /// How many distinct shapes the sites spell.
        shapes: usize,
    },
    /// Every site agrees, but the agreed shape has no value in the mission IR.
    ///
    /// Measured for `ADD_OBJECTIVE_TARGET`, `REMOVE_OBJECTIVE_TARGET`,
    /// `ADD_OTHER_TARGET`, `SET_AI_NET`, `SET_HELP_LABEL`, `ANIM_STATE` and
    /// `COMPLETED_STOPPOINT`, whose argument lists nest one or more lists
    /// (`[[text,text]]`, `[text,[text,[text],text,[text]]]`). `cs_script::ir::Value`
    /// has no list variant, so carrying these would mean flattening a nested
    /// original structure into a positional one — a format change, not a
    /// binding.
    ArgumentShapeHasNoValue {
        /// The shape every site agrees on.
        shape: DirectiveShape,
    },
}

impl UnmeasuredReason {
    /// The stable identifier a report and a log carry.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MeaningNotMeasured => "meaning_not_measured",
            Self::DisagreeingArgumentShape { .. } => "disagreeing_argument_shape",
            Self::ArgumentShapeHasNoValue { .. } => "argument_shape_has_no_value",
        }
    }

    /// The measured numbers behind the reason, rendered for a report.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::MeaningNotMeasured => {
                "no original observation states what this key does".to_owned()
            }
            Self::DisagreeingArgumentShape { shapes } => {
                format!("{shapes} distinct argument shapes across its sites")
            }
            Self::ArgumentShapeHasNoValue { shape } => {
                format!("every site spells {shape}, which the mission IR cannot carry")
            }
        }
    }
}

impl fmt::Display for UnmeasuredReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.code(), self.detail())
    }
}

// ---------------------------------------------------------------------------
// The measured dispositions
// ---------------------------------------------------------------------------

/// The findings documents the measured dispositions come from.
const FINDING_A: &str = "2026-10-04-m01-lc-mission-program";
const FINDING_B: &str = "2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics";
const FINDING_C: &str = "2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives";
const FINDING_D: &str = "2026-10-06-m01-lc-directive-d-sound-help-timer-directives";

/// Which stage of an objective's life a measured directive feeds — where the
/// original consumes the fields the directive's arguments are parsed into.
///
/// The role is measured, not a guess: each variant is the pipeline stage a
/// findings document located the fields' consumer at. A directive's role
/// decides which lowering requirement its residual unknowns count against —
/// evaluators against the objective's condition, everything else against its
/// calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirectiveRole {
    /// The block's completion predicate: evaluated every tick while the block
    /// is awake, and its truth is what completes the objective.
    CompletionCondition,
    /// A field of the objective record the lifecycle itself reads — initial
    /// dormant state, the awake gate, the presentation class and ordinal.
    ObjectiveRecord,
    /// Runs inside the completion pipeline, in its measured order, when the
    /// objective completes.
    CompletionEffect,
    /// Runs inside the wake transition.
    WakeEffect,
    /// Runs on a nap or done state transition of the same objective.
    TransitionEffect,
    /// Feeds the mission-outcome aggregation rather than firing a call.
    OutcomeAggregation,
}

impl DirectiveRole {
    /// The stable identifier a report carries.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::CompletionCondition => "completion_condition",
            Self::ObjectiveRecord => "objective_record",
            Self::CompletionEffect => "completion_effect",
            Self::WakeEffect => "wake_effect",
            Self::TransitionEffect => "transition_effect",
            Self::OutcomeAggregation => "outcome_aggregation",
        }
    }
}

/// The measured operation one directive key performs — what a findings document
/// established the parser writes and the consumer reads, stated as an operation
/// rather than a name.
///
/// One variant per measured mechanism, not one per key: keys that share a
/// handler (`WAKE_OBJECTIVE` and `WAKE_OBJECTIVE_WHEN_I_COMPLETE`; the hundred
/// `INACTIVE<n>` spellings) share a variant, and the key-level table is
/// [`measured_directive`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirectiveOperation {
    /// `INACTIVE<n>`: each listed name resolves through a chained member lookup
    /// into a handle; the block completes when at least the threshold of them
    /// no longer carry the in-play bit.
    InactiveMembers,
    /// `INACTIVE_COMPLETION_COUNT`: the count of cleared members the
    /// inactive-members evaluator needs.
    InactiveThreshold,
    /// `DANGER_ZONES_COMPLETED`: completes when at least the stored threshold
    /// of the recorded zone flag bytes are nonzero.
    DangerZoneFlags,
    /// `DANGER_ZONES_COMPLETION_COUNT`: that threshold.
    DangerZoneThreshold,
    /// `DEDG`: completes when the named enemy group has at most the spelled
    /// count of members still in play, plus whatever its generator still owes;
    /// evaluating it also rewrites three member fields.
    EnemyGroupDepletion,
    /// `ANIM_STATE`: completes when at least the required count of the listed
    /// animations are in the named state.
    AnimationStates,
    /// `TRAVELERS`: completes when the named subject crosses the radius about
    /// the anchor — or, in the counting mode, when the cumulative group count
    /// reaches the required number.
    Travelers,
    /// `COUNTER`: the block's named-counter triples over the global integer
    /// registry — wake and complete writes plus a per-tick test.
    NamedCounters,
    /// `BEGIN_DORMANT`: the block starts dormant; its first child is the
    /// mission-clock second at which it wakes itself, and the next children
    /// arm the awake, nap and done timers.
    DormantStart,
    /// `TICK_DEPENDS_ON_OBJ`: the dependent runs no timers and evaluates no
    /// conditions while the objective at the stored index is not awake.
    DependencyGate,
    /// `IDENTITY`: the class spelling selects the completion sound channel and
    /// HUD class; the integer is the HUD slot ordinal.
    PresentationIdentity,
    /// `WAKE_OBJECTIVE` / `WAKE_OBJECTIVE_WHEN_I_COMPLETE`: at completion, wake
    /// each listed index in order — killed and already-completed records are
    /// skipped, an already-awake one ends the list, at most 15 entries are
    /// processed.
    WakeObjectives,
    /// `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`: at completion, put each listed index
    /// to sleep through the shared transition.
    SleepObjectives,
    /// `KILL_OBJECTIVE_WHEN_I_COMPLETE`: at completion, kill each listed index —
    /// it stops ticking and never counts in the outcome aggregation.
    KillObjectives,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE`: at completion, put the target to nap
    /// and re-wake it after the spelled seconds, clearing its completed flag.
    NapObjective,
    /// `ADD`/`REMOVE` `_OBJECTIVE`/`_OTHER` `_TARGET`: at completion, resolve
    /// each name chain to one object and set or clear its target flag.
    SetTargetFlag {
        /// `true` for the objective-target flag, `false` for the other-target
        /// flag.
        objective: bool,
        /// `true` to set the flag, `false` to clear it.
        set: bool,
    },
    /// `COMPLETED_STOPPOINT`: forward the `{int, bool}` pair to the named
    /// stoppoint's two-step advance/select handler.
    AdvanceStopPoint,
    /// `COMPLETED_ZEPCANNONS`: store the byte at the resolved zeppelin's field.
    ZeppelinCannons,
    /// `SET_AI_NET`: point the named vehicle or zeppelin at the named entry of
    /// the global node list.
    AssignNet,
    /// `SET_AI_TEAM`: write the named actor's team field.
    AssignTeam,
    /// `SET_AI_ATTACK_RADIUS`: write the vehicle's radius triple
    /// `r²`, `-r`, `r`.
    SetAttackRadius,
    /// `START_TAXI`: clear the vehicle's AI hold-off byte.
    ReleaseTaxi,
    /// `SET_HELP_LABEL`: give the resolved object the localized label id and
    /// text.
    SetHelpLabel,
    /// `STOP_QUEUED_SOUNDS`: flag each matching queued-sound entry and schedule
    /// its removal a fixed time later.
    StopQueuedSounds,
    /// `COMPLETED_SOUND_GROUP`: play the sound-group handle through the
    /// completed channel.
    CompletedSoundGroup,
    /// `TIMER_ADJUST` / `ADJUST_TIMER_WHEN_I_COMPLETE`: set or adjust the
    /// mission timer by the spelled seconds at completion.
    AdjustMissionTimer,
    /// `END_TIMER`: stop the mission timer at completion.
    EndMissionTimer,
    /// `WARP_VEHICLE`: teleport the vehicle to a randomly chosen listed point
    /// and add the shared-scalar velocity unless AI-driven.
    WarpVehicle,
    /// `WAKEUP_ENEMIES`: on wake, wake only the named actors that are asleep.
    WakeEnemies,
    /// `WAKEUP_TURRETS`: on wake, set the live byte of every turret entry whose
    /// name matches, `*` consuming exactly one digit.
    WakeTurrets,
    /// `WAKEUP_ZEP_TURRETS`: on wake, activate the named zeppelin-turret node
    /// and all its children.
    WakeZeppelinTurrets,
    /// `WAKEUP_GENERATOR`: on wake, add the spelled count to the named
    /// generator's pending-spawn counter.
    FeedGenerator,
    /// `WAKE_ANIM`: on wake, execute the named animation on the named or
    /// defaulted target.
    WakeAnimation,
    /// `WAKEUP_SOUND_GROUP`: on wake, play the sound-group handle through the
    /// woken channel.
    WakeSoundGroup,
    /// `RESET_TIMER`: on a dormant wake, set and start the mission timer at the
    /// spelled seconds.
    ResetMissionTimer,
    /// `HIDE_OBJ`: on a dormant wake, mark the named objective completed with
    /// no outcome class.
    HideObjective,
    /// `SLEEP_ANIM`: on a nap or done transition, execute the named animation
    /// on the named target.
    TransitionAnimation,
    /// `WAKE_OBJECTIVE_WHEN_I_SLEEP`: wake the listed indices when this
    /// objective auto-naps or auto-dones on its own timers.
    WakeObjectivesOnTransition,
    /// `WON` / `LOST`: the block's outcome class — the mission resolves when
    /// every block of a class completes; the class block itself does not fire
    /// on its own completion.
    OutcomeClass {
        /// `true` for `WON`, `false` for `LOST`.
        won: bool,
    },
}

impl DirectiveOperation {
    /// Which stage of an objective's life this operation feeds.
    #[must_use]
    pub const fn role(self) -> DirectiveRole {
        match self {
            Self::InactiveMembers
            | Self::InactiveThreshold
            | Self::DangerZoneFlags
            | Self::DangerZoneThreshold
            | Self::EnemyGroupDepletion
            | Self::AnimationStates
            | Self::Travelers
            | Self::NamedCounters => DirectiveRole::CompletionCondition,
            Self::DormantStart | Self::DependencyGate | Self::PresentationIdentity => {
                DirectiveRole::ObjectiveRecord
            }
            Self::WakeObjectives
            | Self::SleepObjectives
            | Self::KillObjectives
            | Self::NapObjective
            | Self::SetTargetFlag { .. }
            | Self::AdvanceStopPoint
            | Self::ZeppelinCannons
            | Self::AssignNet
            | Self::AssignTeam
            | Self::SetAttackRadius
            | Self::ReleaseTaxi
            | Self::SetHelpLabel
            | Self::StopQueuedSounds
            | Self::CompletedSoundGroup
            | Self::AdjustMissionTimer
            | Self::EndMissionTimer
            | Self::WarpVehicle => DirectiveRole::CompletionEffect,
            Self::WakeEnemies
            | Self::WakeTurrets
            | Self::WakeZeppelinTurrets
            | Self::FeedGenerator
            | Self::WakeAnimation
            | Self::WakeSoundGroup
            | Self::ResetMissionTimer
            | Self::HideObjective => DirectiveRole::WakeEffect,
            Self::TransitionAnimation | Self::WakeObjectivesOnTransition => {
                DirectiveRole::TransitionEffect
            }
            Self::OutcomeClass { .. } => DirectiveRole::OutcomeAggregation,
        }
    }

    /// The stable identifier a report carries.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InactiveMembers => "inactive_members",
            Self::InactiveThreshold => "inactive_threshold",
            Self::DangerZoneFlags => "danger_zone_flags",
            Self::DangerZoneThreshold => "danger_zone_threshold",
            Self::EnemyGroupDepletion => "enemy_group_depletion",
            Self::AnimationStates => "animation_states",
            Self::Travelers => "travelers",
            Self::NamedCounters => "named_counters",
            Self::DormantStart => "dormant_start",
            Self::DependencyGate => "dependency_gate",
            Self::PresentationIdentity => "presentation_identity",
            Self::WakeObjectives => "wake_objectives",
            Self::SleepObjectives => "sleep_objectives",
            Self::KillObjectives => "kill_objectives",
            Self::NapObjective => "nap_objective",
            Self::SetTargetFlag {
                objective: true,
                set: true,
            } => "add_objective_target",
            Self::SetTargetFlag {
                objective: true,
                set: false,
            } => "remove_objective_target",
            Self::SetTargetFlag {
                objective: false,
                set: true,
            } => "add_other_target",
            Self::SetTargetFlag {
                objective: false,
                set: false,
            } => "remove_other_target",
            Self::AdvanceStopPoint => "advance_stop_point",
            Self::ZeppelinCannons => "zeppelin_cannons",
            Self::AssignNet => "assign_net",
            Self::AssignTeam => "assign_team",
            Self::SetAttackRadius => "set_attack_radius",
            Self::ReleaseTaxi => "release_taxi",
            Self::SetHelpLabel => "set_help_label",
            Self::StopQueuedSounds => "stop_queued_sounds",
            Self::CompletedSoundGroup => "completed_sound_group",
            Self::AdjustMissionTimer => "adjust_mission_timer",
            Self::EndMissionTimer => "end_mission_timer",
            Self::WarpVehicle => "warp_vehicle",
            Self::WakeEnemies => "wake_enemies",
            Self::WakeTurrets => "wake_turrets",
            Self::WakeZeppelinTurrets => "wake_zeppelin_turrets",
            Self::FeedGenerator => "feed_generator",
            Self::WakeAnimation => "wake_animation",
            Self::WakeSoundGroup => "wake_sound_group",
            Self::ResetMissionTimer => "reset_mission_timer",
            Self::HideObjective => "hide_objective",
            Self::TransitionAnimation => "transition_animation",
            Self::WakeObjectivesOnTransition => "wake_objectives_on_transition",
            Self::OutcomeClass { won: true } => "outcome_won",
            Self::OutcomeClass { won: false } => "outcome_lost",
        }
    }
}

/// The measured effect of one directive key, as a stage A/B/C/D findings
/// document established it.
///
/// A measured disposition is **not** support: it names the operation the
/// original performs, with the evidence for the measurement and the residual
/// unknowns the measurement leaves. The engine still cannot honour it —
/// [`DirectiveDisposition::is_implemented`] stays `false` — and
/// [`ControlLowering`]'s rows name the argument shapes the IR cannot carry and
/// the bindings the operation still lacks, per key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredDirective {
    /// The measured operation.
    pub operation: DirectiveOperation,
    /// The measured effect in one line — what the parser stores and which
    /// consumer reads it.
    pub summary: &'static str,
    /// The findings documents the measurement comes from.
    pub evidence: &'static [&'static str],
    /// What the measurement still leaves unknown about this key's own arguments
    /// or its world-side consumption. Carried, never dropped: *measured* is not
    /// *fully known*.
    pub unknowns: &'static [&'static str],
}

/// Whether `key` spells one of the measured `INACTIVE<n>` member-count keys —
/// the parser's `sprintf("INACTIVE%d")` loop, `d` in `1..=100`
/// ([`FINDING_A`]/[`FINDING_B`]).
fn measured_inactive_stage(key: &str) -> bool {
    let Some(digits) = key.strip_prefix(OBJECTIVE_INACTIVE_STAGE_PREFIX) else {
        return false;
    };
    let Ok(stage) = digits.parse::<u32>() else {
        return false;
    };
    (1..=100).contains(&stage) && stage.to_string() == digits
}

/// The measured disposition of a directive key, or `None` when no findings
/// document states what the key does.
///
/// The table is exactly the vocabulary the findings measure — every entry is a
/// handler whose argument consumption and consumer were located in the
/// original, and every key the findings do **not** cover stays `None` here:
/// `SET_AI_`, `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`, `OBJECTIVE_HD_a`,
/// `OBJECTIVE_HD_b`, `TEST_COMPLETE`, `COMPLETION_COUNT`, `WIN_ANIM`,
/// `LOSS_ANIM`, `DELETE_ON_SUCCESS`, the stray English words the corpus spells
/// (`Change`, `to`, `mobile`, `net`) and the record-level keys
/// (`MISSION_TIMER`, `PLAYER_INIT`, the animation lists) are each refused
/// [`UnmeasuredReason::MeaningNotMeasured`] — a name alone is not evidence.
#[must_use]
pub fn measured_directive(key: &str) -> Option<MeasuredDirective> {
    let directive = match key {
        // Objective-record fields (finding B).
        "BEGIN_DORMANT" => MeasuredDirective {
            operation: DirectiveOperation::DormantStart,
            summary: "presence starts the block dormant (its awake flag and active flag cleared); \
                      child0 is the mission-clock second at which the objective wakes itself — \
                      below zero disables the timed wake — and children 1..3 arm the awake, nap \
                      and done timers",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "TICK_DEPENDS_ON_OBJ" => MeasuredDirective {
            operation: DirectiveOperation::DependencyGate,
            summary: "the objective runs no timers and evaluates no conditions while the block at \
                      child0−1 is not currently awake; a completed dependency freezes it \
                      permanently",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "IDENTITY" => MeasuredDirective {
            operation: DirectiveOperation::PresentationIdentity,
            summary: "child0's class spelling (PRIMARY/SECONDARY/TERTIARY → 1/2/3) picks the \
                      completion sound channel and the HUD class; child1 is the HUD slot ordinal \
                      the completion and save/load posts carry",
            evidence: &[FINDING_B],
            unknowns: &[
                "whether `IDENTITY` child2 — the `MSG_*` text at 4 of 5 M01 sites — is ever \
                 re-read from the retained document: the measured parse never reads it and no \
                 consumer was found",
            ],
        },

        // Completion-condition evaluators and their thresholds.
        "INACTIVE_COMPLETION_COUNT" => MeasuredDirective {
            operation: DirectiveOperation::InactiveThreshold,
            summary: "the count of cleared members the inactive-members evaluator needs; absent, \
                      the threshold defaults to all the block's listed members",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "DANGER_ZONES_COMPLETED" => MeasuredDirective {
            operation: DirectiveOperation::DangerZoneFlags,
            summary: "completes when at least the completion-count threshold of the stored \
                      danger-zone flag bytes are nonzero",
            evidence: &[FINDING_A, FINDING_B],
            unknowns: &[
                "the +0x570 field beside the count and the flag array — parser-side storage \
                 whose use is untraced",
            ],
        },
        "DANGER_ZONES_COMPLETION_COUNT" => MeasuredDirective {
            operation: DirectiveOperation::DangerZoneThreshold,
            summary: "the flag-count threshold the danger-zones evaluator compares the nonzero \
                      flag bytes against",
            evidence: &[FINDING_A],
            unknowns: &[],
        },
        "DEDG" => MeasuredDirective {
            operation: DirectiveOperation::EnemyGroupDepletion,
            summary: "completes when the group at child0 has child1 or fewer members still in \
                      play, plus whatever the optionally-named generator still owes; evaluating \
                      it also rewrites three member fields on every counted member",
            evidence: &[FINDING_B],
            unknowns: &[
                "what the three member fields the `DEDG` evaluator rewrites on every counted \
                 member (+0x318/+0x31c/+0x320) feed — untraced world state",
            ],
        },
        "TRAVELERS" => MeasuredDirective {
            operation: DirectiveOperation::Travelers,
            summary: "completes when the named subject is inside the radius about the anchor when \
                      child1 spells `APPROACHING` and outside it otherwise (exact equality never \
                      fires) — or, with a non-string child0, when the cumulative count of \
                      matching group members reaches the required number; `DELETE_ON_SUCCESS` \
                      deletes the subject on firing",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[
                "the token naming `TRAVELERS`' unspelled polarity — only `APPROACHING` is a \
                 measured spelling; the outside case's word is unknown",
                "which of the two modes M01's site takes is a runtime property of the world \
                 build, not of the spelling",
                "what group id 0 means in the counting mode",
            ],
        },
        "ANIM_STATE" => MeasuredDirective {
            operation: DirectiveOperation::AnimationStates,
            summary: "completes when at least `required` listed animations are in the wanted state \
                      (RUNNING/EXECUTED/INVALID → 2/3/4); `required` defaults to the listed pair \
                      count and a sibling COMPLETION_COUNT overrides it",
            evidence: &[FINDING_C],
            unknowns: &[
                "the animation-state enum above 6 indexes past the engine's name table — \
                 unexercised by M01, unexplored",
            ],
        },
        "COUNTER" => MeasuredDirective {
            operation: DirectiveOperation::NamedCounters,
            summary: "the block's ON_WAKEUP/ON_COMPLETE/TEST_COMPLETE counter triples over the \
                      named global integer registry — writes run at wake and complete, tests run \
                      every tick (`TEST_LE` measured to evaluate as `TEST_GE`)",
            evidence: &[FINDING_B],
            unknowns: &[],
        },

        // Completion effects (findings B, C, D).
        "WAKE_OBJECTIVE" | "WAKE_OBJECTIVE_WHEN_I_COMPLETE" => MeasuredDirective {
            operation: DirectiveOperation::WakeObjectives,
            summary: "at this objective's completion the listed zero-based block indices are woken \
                      in order — killed and already-completed records are skipped, an \
                      already-awake one ends the list, at most 15 processed entries",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "SLEEP_OBJECTIVE_WHEN_I_COMPLETE" => MeasuredDirective {
            operation: DirectiveOperation::SleepObjectives,
            summary: "at this objective's completion each listed index is put to state 3 through \
                      the shared transition, which also plays the target's `SLEEP_ANIM`",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "KILL_OBJECTIVE_WHEN_I_COMPLETE" => MeasuredDirective {
            operation: DirectiveOperation::KillObjectives,
            summary: "at this objective's completion each listed index is killed — it stops \
                      ticking and never counts in the outcome aggregation, irreversibly",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "NAP_OBJECTIVE_WHEN_I_COMPLETE" => MeasuredDirective {
            operation: DirectiveOperation::NapObjective,
            summary: "at this objective's completion the listed index is put to state 2 and \
                      re-wakes child1 seconds later (a missing child1 measures ≈0.3s and logs); \
                      the nap clears the target's completed flag so it can complete again",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "WAKE_OBJECTIVE_WHEN_I_SLEEP" => MeasuredDirective {
            operation: DirectiveOperation::WakeObjectivesOnTransition,
            summary: "the listed indices are woken when this objective auto-naps or auto-dones on \
                      its own timers — not when another objective sleeps it and not at its \
                      completion",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "ADD_OBJECTIVE_TARGET" => MeasuredDirective {
            operation: DirectiveOperation::SetTargetFlag {
                objective: true,
                set: true,
            },
            summary: "at completion each spelled name resolves lazily through the vehicle, turret \
                      and object registries and the leaf object's +0x4d objective-target flag is \
                      set",
            evidence: &[FINDING_B],
            unknowns: &[
                "which icon or label the target-info layer draws from the target flag — \
                 presentation code, untraced",
            ],
        },
        "REMOVE_OBJECTIVE_TARGET" => MeasuredDirective {
            operation: DirectiveOperation::SetTargetFlag {
                objective: true,
                set: false,
            },
            summary: "at completion each spelled name resolves lazily through the vehicle, turret \
                      and object registries and the leaf object's +0x4d objective-target flag is \
                      cleared",
            evidence: &[FINDING_B],
            unknowns: &[
                "which icon or label the target-info layer draws from the target flag — \
                 presentation code, untraced",
            ],
        },
        "ADD_OTHER_TARGET" => MeasuredDirective {
            operation: DirectiveOperation::SetTargetFlag {
                objective: false,
                set: true,
            },
            summary: "at completion each spelled name resolves lazily through the vehicle, turret \
                      and object registries and the leaf object's +0x4c other-target flag is set",
            evidence: &[FINDING_B],
            unknowns: &[
                "which icon or label the target-info layer draws from the target flag — \
                 presentation code, untraced",
            ],
        },
        "REMOVE_OTHER_TARGET" => MeasuredDirective {
            operation: DirectiveOperation::SetTargetFlag {
                objective: false,
                set: false,
            },
            summary: "at completion each spelled name resolves lazily through the vehicle, turret \
                      and object registries and the leaf object's +0x4c other-target flag is \
                      cleared",
            evidence: &[FINDING_B],
            unknowns: &[
                "which icon or label the target-info layer draws from the target flag — \
                 presentation code, untraced",
            ],
        },
        "COMPLETED_ZEPCANNONS" => MeasuredDirective {
            operation: DirectiveOperation::ZeppelinCannons,
            summary: "at completion each `{name, byte}` record resolves the name in the zeppelin \
                      registry and stores the byte at the zeppelin's +0xc",
            evidence: &[FINDING_B],
            unknowns: &[
                "what the zeppelin's +0xc byte drives — the write is measured, the field's \
                 consumers are not traced",
            ],
        },
        "COMPLETED_STOPPOINT" => MeasuredDirective {
            operation: DirectiveOperation::AdvanceStopPoint,
            summary: "at completion each `{name, int, bool}` record forwards the pair to the named \
                      stoppoint's two-step advance/select handler when the int is positive",
            evidence: &[FINDING_B],
            unknowns: &[
                "what the forwarded pair means to a stoppoint — its code is outside the measured \
                 bound",
            ],
        },
        "SET_AI_NET" => MeasuredDirective {
            operation: DirectiveOperation::AssignNet,
            summary: "at completion each `{actor, net}` pair resolves the named vehicle or \
                      zeppelin and points it at the named entry of the global node list — the \
                      same field the vehicle wake path restores a position from",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[
                "what a re-seed copies out of the node-list entry — the setter's internals are \
                 not traced",
            ],
        },
        "SET_AI_TEAM" => MeasuredDirective {
            operation: DirectiveOperation::AssignTeam,
            summary: "at completion each `{name, team}` pair writes the named actor's team — a \
                      vehicle's base team field through its vtable setter, a zeppelin's \
                      +0xdc/+0xe0 pair",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[
                "the object a vehicle holds at +0x948, released through a virtual call on the \
                 write — unidentified",
            ],
        },
        "SET_AI_ATTACK_RADIUS" => MeasuredDirective {
            operation: DirectiveOperation::SetAttackRadius,
            summary: "at completion each `{name, radius}` record writes the vehicle's radius \
                      triple +0x328=r², +0x32c=−r, +0x330=r — vehicles only",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[
                "which of the three written fields is the original's attack_rad, attack_u or \
                 attack_l — its own comment gives the order, not the semantics",
            ],
        },
        "START_TAXI" => MeasuredDirective {
            operation: DirectiveOperation::ReleaseTaxi,
            summary: "at completion each name clears the vehicle's AI hold-off byte — the flag \
                      the AI think reads first and the spawn path sets on a parked vehicle",
            evidence: &[FINDING_B, FINDING_C, FINDING_D],
            unknowns: &[
                "the original name of the vehicle's +0xd4 hold-off flag — measured as a flag; \
                 the AI-node comment's `taxiPath` is consistent, not proof",
            ],
        },
        "SET_HELP_LABEL" => MeasuredDirective {
            operation: DirectiveOperation::SetHelpLabel,
            summary: "at completion the node names resolve to one world object, which receives \
                      the localized label id and its label text; M01's two sites both label a \
                      single space",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[
                "which HUD element displays the label — the assignment is measured, the UI \
                 consumer untraced",
            ],
        },
        "STOP_QUEUED_SOUNDS" => MeasuredDirective {
            operation: DirectiveOperation::StopQueuedSounds,
            summary: "at completion each name — at most ten — flags the matching queued-sound \
                      entry and schedules its removal 10.0 time units later",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[
                "no reader of the queued entry's flag was traced — the measured effect is a \
                 scheduled removal, not a measured audible stop",
            ],
        },
        "COMPLETED_SOUND_GROUP" => MeasuredDirective {
            operation: DirectiveOperation::CompletedSoundGroup,
            summary: "at completion the sound-group handle resolved at parse is played through \
                      the completed channel; the built-in `music_*_sg` names are the engine's \
                      music state requests",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[
                "what sound a group name resolves to — the handle's identity is runtime state, \
                 not shipped data",
                "which groups occupy the sound manager's four routing slots at runtime",
            ],
        },
        "TIMER_ADJUST" => MeasuredDirective {
            operation: DirectiveOperation::AdjustMissionTimer,
            summary: "at completion the mission timer is adjusted by the spelled seconds",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[],
        },
        "ADJUST_TIMER_WHEN_I_COMPLETE" => MeasuredDirective {
            operation: DirectiveOperation::AdjustMissionTimer,
            summary: "at completion the mission timer is set to the spelled seconds when child0 \
                      spells `SET`, adjusted by them when it spells `ADJUST`",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[],
        },
        "END_TIMER" => MeasuredDirective {
            operation: DirectiveOperation::EndMissionTimer,
            summary: "at completion the mission timer is stopped",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[],
        },
        "WARP_VEHICLE" => MeasuredDirective {
            operation: DirectiveOperation::WarpVehicle,
            summary: "at completion the named vehicle teleports to one of its listed warp points \
                      chosen at random — the point's angle is spelled in degrees — and, unless \
                      AI-driven, gains a velocity of one shared scalar times three per-vehicle \
                      factors",
            evidence: &[FINDING_C],
            unknowns: &[
                "the three per-axis factors and the world field the shared scalar is taken \
                 from — what they are is unknown",
            ],
        },
        "HIDE_OBJ" => MeasuredDirective {
            operation: DirectiveOperation::HideObjective,
            summary: "when this objective wakes from dormant, the named objective is marked \
                      completed with no outcome class, hiding it from the outcome aggregation — \
                      the effect does not fire on a nap or done wake",
            evidence: &[FINDING_B],
            unknowns: &[],
        },

        // Wake and transition effects (findings B, C, D).
        "WAKEUP_ENEMIES" => MeasuredDirective {
            operation: DirectiveOperation::WakeEnemies,
            summary: "on wake each name resolves as a vehicle — woken only if its asleep byte is \
                      set — else as a zeppelin, woken only if its dormant byte is set; a name \
                      for something already awake does nothing",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[],
        },
        "WAKEUP_TURRETS" => MeasuredDirective {
            operation: DirectiveOperation::WakeTurrets,
            summary: "on wake each name sets the live byte of every turret-registry entry whose \
                      name matches, a `*` in the spelled name consuming exactly one digit of the \
                      entry's name",
            evidence: &[FINDING_C],
            unknowns: &[
                "the `*` wildcard's intent — the one-digit consuming rule is measured; no M01 \
                 site exercises it",
            ],
        },
        "WAKEUP_ZEP_TURRETS" => MeasuredDirective {
            operation: DirectiveOperation::WakeZeppelinTurrets,
            summary: "on wake each name resolves a zeppelin-turret node and sets the live byte of \
                      its registry entry, recursing over the node's children",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[],
        },
        "WAKEUP_GENERATOR" => MeasuredDirective {
            operation: DirectiveOperation::FeedGenerator,
            summary: "on wake the named generator's pending-spawn counter increases by the \
                      spelled count — the units it may still produce",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[],
        },
        "WAKE_ANIM" => MeasuredDirective {
            operation: DirectiveOperation::WakeAnimation,
            summary: "on wake the named animation executes on the named or defaulted target \
                      object",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[
                "the animation call's three trailing arguments — the mission always passes \
                 0,0,0 and other callers' defaults are unmeasured",
            ],
        },
        "WAKEUP_SOUND_GROUP" => MeasuredDirective {
            operation: DirectiveOperation::WakeSoundGroup,
            summary: "on wake the sound-group handle resolved at parse is played through the \
                      woken channel; the built-in `music_*_sg` names are music state requests",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[
                "what sound a group name resolves to — the handle's identity is runtime state, \
                 not shipped data",
                "which groups occupy the sound manager's four routing slots at runtime",
            ],
        },
        "RESET_TIMER" => MeasuredDirective {
            operation: DirectiveOperation::ResetMissionTimer,
            summary: "on a dormant wake the mission timer is set to the spelled seconds and \
                      started — the effect does not fire on a nap or done wake",
            evidence: &[FINDING_B, FINDING_D],
            unknowns: &[],
        },
        "SLEEP_ANIM" => MeasuredDirective {
            operation: DirectiveOperation::TransitionAnimation,
            summary: "when this objective is put to nap or done — by its own timers or another \
                      objective's `SLEEP` list — the named animation executes on the named \
                      target; it is not a completion effect",
            evidence: &[FINDING_B, FINDING_C],
            unknowns: &[
                "the animation call's three trailing arguments — the mission always passes \
                 0,0,0 and other callers' defaults are unmeasured",
            ],
        },

        // The outcome classes (finding B).
        "WON" => MeasuredDirective {
            operation: DirectiveOperation::OutcomeClass { won: true },
            summary: "the block's outcome class: the mission resolves to won when every WON-class \
                      block completes; a class block does not fire on its own completion",
            evidence: &[FINDING_B],
            unknowns: &[],
        },
        "LOST" => MeasuredDirective {
            operation: DirectiveOperation::OutcomeClass { won: false },
            summary: "the block's outcome class: the mission resolves to lost when every \
                      LOST-class block completes; a class block does not fire on its own \
                      completion",
            evidence: &[FINDING_B],
            unknowns: &[],
        },

        // The hundred INACTIVE<n> spellings are one measured mechanism.
        _ if measured_inactive_stage(key) => MeasuredDirective {
            operation: DirectiveOperation::InactiveMembers,
            summary: "each listed name resolves through a chained member lookup into a handle; \
                      the block completes when at least `INACTIVE_COMPLETION_COUNT` of them no \
                      longer carry the in-play bit",
            evidence: &[FINDING_B],
            unknowns: &[
                "the world code that sets and clears a member's in-play bit (+0x24 bit 4): every \
                 consumer of it is measured, its spawn and despawn writers are not traced",
            ],
        },
        _ => return None,
    };
    Some(directive)
}

/// What became of one measured directive key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectiveDisposition {
    /// The mission IR has an action for it: a terminal outcome request.
    TerminalOutcome {
        /// The outcome the key's **spelling** names.
        outcome: TerminalOutcome,
    },
    /// A stage A/B/C/D finding measured the key's **effect**: the operation the
    /// original performs is named, with the evidence for the measurement and
    /// the residual unknowns it leaves.
    ///
    /// **Measured is not support.** The disposition says what the key does, not
    /// that the engine can do it: the key still has no
    /// `cs_script::bindings` operation — [`Self::is_implemented`] stays
    /// `false` — and the lowering accounting ([`ControlLowering`]) names the
    /// per-site argument shapes the mission IR cannot carry and every residual
    /// unknown the finding recorded, so a measured key can never be silently
    /// read as either *implemented* or *unknown*.
    Measured(MeasuredDirective),
    /// Measured, counted and refused before flight. Never replaced by a stub
    /// that reports success (contract "Host interface": a binding with no
    /// measured meaning is not bound to a convenient operation).
    Unmeasured {
        /// Why it is not implemented.
        reason: UnmeasuredReason,
    },
}

impl DirectiveDisposition {
    /// Whether the engine may act on this key.
    ///
    /// Only a terminal outcome reaches an engine operation today: a
    /// [`Self::Measured`] key's semantics are known but there is no host
    /// binding that runs them, so it is **not** implemented — measured is a
    /// statement about the original, not a license.
    #[must_use]
    pub const fn is_implemented(&self) -> bool {
        matches!(self, Self::TerminalOutcome { .. })
    }

    /// The refusal, or `None` for a disposition that carries none — an
    /// implemented outcome, or a measured key (whose remaining gaps are the
    /// lowering accounting's to name, not this refusal's).
    #[must_use]
    pub const fn refusal(&self) -> Option<&UnmeasuredReason> {
        match self {
            Self::TerminalOutcome { .. } | Self::Measured(_) => None,
            Self::Unmeasured { reason } => Some(reason),
        }
    }
}

// ---------------------------------------------------------------------------
// The record-level fields
// ---------------------------------------------------------------------------

/// One of the measured keys a control record carries outside its numbered
/// blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ControlRecordField {
    /// `MISSION_TIMER` — a list holding one number. Measured in M01 as `[0.0]`.
    MissionTimer,
    /// `PLAYER_INIT` — a list of five values: an integer, a three-number vector,
    /// another three-number vector, a number and a number. Measured in M01 as
    /// `[int, [float x3], [float x3], float, float]`.
    PlayerInit,
    /// `RESTORE_ANIMS`, `EXECUTE_ANIMS` and `INVALIDATE_ANIMS` — measured in M01
    /// as three **empty** lists, so a mission that restores, runs and
    /// invalidates no animation by name at mission start.
    AnimList(AnimList),
}

/// Which of the three animation lists a [`ControlRecordField::AnimList`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnimList {
    /// `RESTORE_ANIMS`.
    Restore,
    /// `EXECUTE_ANIMS`.
    Execute,
    /// `INVALIDATE_ANIMS`.
    Invalidate,
}

impl AnimList {
    /// The key as the original spells it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Restore => "RESTORE_ANIMS",
            Self::Execute => "EXECUTE_ANIMS",
            Self::Invalidate => "INVALIDATE_ANIMS",
        }
    }
}

impl ControlRecordField {
    /// The key as the original spells it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::MissionTimer => "MISSION_TIMER",
            Self::PlayerInit => "PLAYER_INIT",
            Self::AnimList(list) => list.key(),
        }
    }

    /// The field a key names, or `None` for a key outside
    /// [`CONTROL_RECORD_KEY_VOCABULARY`].
    ///
    /// A **name match**, not a reading: matching the key tells a caller which
    /// documented field the value belongs to and nothing about what the value
    /// does. [`Self::support`] is where that limit is stated.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "MISSION_TIMER" => Some(Self::MissionTimer),
            "PLAYER_INIT" => Some(Self::PlayerInit),
            "RESTORE_ANIMS" => Some(Self::AnimList(AnimList::Restore)),
            "EXECUTE_ANIMS" => Some(Self::AnimList(AnimList::Execute)),
            "INVALIDATE_ANIMS" => Some(Self::AnimList(AnimList::Invalidate)),
            _ => None,
        }
    }
}

/// What a record field's **effect** is measured to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSupport {
    /// The value's shape is measured; what it does is not. No original
    /// executable has been run, so neither `MISSION_TIMER`'s number nor
    /// `PLAYER_INIT`'s five may be read as a duration, a position, a heading or a
    /// radius.
    ShapeMeasured,
}

impl FieldSupport {
    /// The stable label a report carries.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ShapeMeasured => "shape_measured",
        }
    }

    /// The refusal text this support level implies, for a report that has to say
    /// what is missing.
    #[must_use]
    pub const fn refusal(self) -> &'static str {
        match self {
            Self::ShapeMeasured => {
                "the field's value shape is measured; no original observation states what the value \
                 does, so no duration, unit or coordinate may be read from it"
            }
        }
    }
}

impl ControlRecordField {
    /// The measured support level of this field's effect.
    #[must_use]
    pub const fn support(self) -> FieldSupport {
        FieldSupport::ShapeMeasured
    }
}

// ---------------------------------------------------------------------------
// The measurement
// ---------------------------------------------------------------------------

/// Why one block of a control record could not be read as a directive list.
///
/// A refusal, never a skip: a block this walk cannot parse is a block whose
/// directives would otherwise vanish from the census, which is the failure
/// SCRIPT-MISSION's "Source adapter acceptance" counts against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockRefusal {
    /// A block whose value is not a list, so it holds no directives at all.
    BlockNotAList {
        /// The block key as the record spells it (`OBJECTIVE7`).
        block: String,
    },
    /// A child of a block is not a text key, so the directive it starts cannot be
    /// read.
    KeyNotText {
        /// The block key.
        block: String,
        /// The zero-based child position inside the block.
        index: usize,
    },
}

impl BlockRefusal {
    /// The stable identifier a report and a log carry.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::BlockNotAList { .. } => "block_not_a_list",
            Self::KeyNotText { .. } => "key_not_text",
        }
    }

    /// The block the refusal sits in.
    #[must_use]
    pub fn block(&self) -> &str {
        match self {
            Self::BlockNotAList { block } | Self::KeyNotText { block, .. } => block,
        }
    }
}

impl fmt::Display for BlockRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlockNotAList { block } => {
                write!(
                    f,
                    "{block}: {}: the block is not a directive list",
                    self.code()
                )
            }
            Self::KeyNotText { block, index } => write!(
                f,
                "{block}: {}: child {index} is not a directive key",
                self.code()
            ),
        }
    }
}

/// The measured control record of one mission.
///
/// Every number here comes from walking the decoded member: [`blocks`] is the
/// counted numbered blocks, [`sites`] the counted directive sites and
/// [`keys`] the complete key vocabulary with the sites each carries. A key the
/// walk could not classify is still counted, so the vocabulary can never be
/// smaller than what the record spells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasuredControlRecord {
    blocks: u32,
    sites: u32,
    keys: Vec<MeasuredDirectiveKey>,
    record_fields: Vec<(ControlRecordField, u32)>,
    record_field_shapes: Vec<(String, DirectiveShape)>,
    unclassified_record_keys: Vec<String>,
    refusals: Vec<BlockRefusal>,
}

impl MeasuredControlRecord {
    /// How many numbered `OBJECTIVE<N>` blocks the record declares.
    #[must_use]
    pub const fn blocks(&self) -> u32 {
        self.blocks
    }

    /// How many directive sites the numbered blocks carry in total.
    ///
    /// This is the sum over [`Self::keys`], and the acceptance suite checks it
    /// against an independent walk: a census whose total is smaller than the sum
    /// of its rows is a census that dropped a directive.
    #[must_use]
    pub const fn sites(&self) -> u32 {
        self.sites
    }

    /// The complete directive key vocabulary, sorted by key.
    #[must_use]
    pub fn keys(&self) -> &[MeasuredDirectiveKey] {
        &self.keys
    }

    /// The measured key, or `None` when the record does not spell it.
    #[must_use]
    pub fn key(&self, key: &str) -> Option<&MeasuredDirectiveKey> {
        self.keys.iter().find(|measured| measured.key == key)
    }

    /// The record keys outside the numbered blocks that this crate classifies,
    /// with the sites each carries.
    #[must_use]
    pub fn record_fields(&self) -> &[(ControlRecordField, u32)] {
        &self.record_fields
    }

    /// The measured shape of each classified record field's value, in
    /// [`Self::record_fields`] order. A shape a field carries at several sites
    /// with different shapes would appear once per distinct shape, which is why
    /// this is a pair list rather than a map keyed by field.
    #[must_use]
    pub fn record_field_shapes(&self) -> &[(String, DirectiveShape)] {
        &self.record_field_shapes
    }

    /// Record keys outside the numbered blocks that
    /// [`ControlRecordField::from_key`] does not name.
    ///
    /// Measured in M01: none. Carried so a mission that grows a sixth record key
    /// shows up in the census as an **unclassified** key rather than being
    /// absorbed into a documented field or dropped.
    #[must_use]
    pub fn unclassified_record_keys(&self) -> &[String] {
        &self.unclassified_record_keys
    }

    /// The blocks this walk could not read as a directive list.
    #[must_use]
    pub fn refusals(&self) -> &[BlockRefusal] {
        &self.refusals
    }

    /// The keys whose **effect** is unmeasured — no finding states what they
    /// do — each with its refusal, in key order.
    ///
    /// A *measured* key is not here: [`Self::measured`] lists the keys a
    /// finding measures the effect of, and [`Self::implemented`] the ones the
    /// engine can act on. The three sets partition the vocabulary.
    #[must_use]
    pub fn unmeasured(&self) -> Vec<(&MeasuredDirectiveKey, DirectiveDisposition)> {
        self.keys
            .iter()
            .filter(|measured| {
                matches!(
                    measured.disposition(),
                    DirectiveDisposition::Unmeasured { .. }
                )
            })
            .map(|measured| (measured, measured.disposition()))
            .collect()
    }

    /// The keys a finding measures the **effect** of, in key order.
    ///
    /// *Measured is not support*: none of these is implemented, and the
    /// lowering accounting names the argument shapes and residual unknowns
    /// each still carries. This list is what a census reports as *understood
    /// but not runnable*.
    #[must_use]
    pub fn measured(&self) -> Vec<(&MeasuredDirectiveKey, MeasuredDirective)> {
        self.keys
            .iter()
            .filter_map(|key| match key.disposition() {
                DirectiveDisposition::Measured(directive) => Some((key, directive)),
                _ => None,
            })
            .collect()
    }

    /// The measured keys the engine may act on, in key order.
    #[must_use]
    pub fn implemented(&self) -> Vec<(&MeasuredDirectiveKey, TerminalOutcome)> {
        self.keys
            .iter()
            .filter_map(|measured| match measured.disposition() {
                DirectiveDisposition::TerminalOutcome { outcome } => Some((measured, outcome)),
                DirectiveDisposition::Measured(_) | DirectiveDisposition::Unmeasured { .. } => None,
            })
            .collect()
    }

    /// How many distinct keys the record spells.
    #[must_use]
    pub fn vocabulary(&self) -> u32 {
        self.keys.len() as u32
    }

    /// Whether every directive of the record is measured, every block was
    /// read, and every lowering requirement is met — the whole of what
    /// "Supported" means.
    ///
    /// `false` for every measured retail record, and that is the correct reading
    /// rather than a missing one: a mission may not be launched off a record whose
    /// directives the engine cannot honour (contract "Source adapter acceptance").
    /// An empty record also answers `false`, so a member nobody read can never
    /// report itself complete.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.keys.is_empty()
            && self.refusals.is_empty()
            && self.unmeasured().is_empty()
            && self.lowering().complete()
    }

    /// The honest accounting of what this record would still need before
    /// `lower_program` could produce a `MissionProgram`.
    #[must_use]
    pub fn lowering(&self) -> ControlLowering {
        ControlLowering::measure(self)
    }

    /// The refusal text this record raises: the unmet requirements and the fields
    /// they leave unmeasured, one per line.
    ///
    /// The one-line-per-reason form of [`Self::lowering`], for a diagnostic that
    /// has room for a message and not for a table.
    #[must_use]
    pub fn to_lowering_refusal(&self) -> String {
        lowering_refusal(self)
    }
}

/// Walks one decoded control member and measures every directive its numbered
/// blocks spell.
///
/// The directive grammar is **measured**, not assumed, and it is asymmetric on
/// purpose: a directive is a text key followed, *if and only if* it has one, by
/// its argument list. A key followed immediately by another text key takes no
/// argument — that is how the installation spells the two outcome directives, and
/// reading the pair as `key, value` would attribute the next key's name to the
/// previous key's argument list.
///
/// Anything else is a [`BlockRefusal`] with the block and the child position, so
/// a record that stops matching this grammar is visible instead of quietly
/// shorter.
#[must_use]
pub fn measure_control_record(document: &ZrdValue) -> MeasuredControlRecord {
    let mut record = MeasuredControlRecord::default();
    let mut keys: BTreeMap<String, (u32, u32, BTreeMap<DirectiveShape, u32>)> = BTreeMap::new();
    let mut fields: BTreeMap<ControlRecordField, u32> = BTreeMap::new();
    let mut field_shapes: BTreeMap<(ControlRecordField, DirectiveShape), ()> = BTreeMap::new();
    let mut unclassified: BTreeMap<String, ()> = BTreeMap::new();

    for (key, value) in zrd_flat_fields(objective_record(document)) {
        if is_objective_block(key) {
            record.blocks += 1;
            let Some(children) = value.as_list() else {
                record.refusals.push(BlockRefusal::BlockNotAList {
                    block: key.to_owned(),
                });
                continue;
            };
            // The keys this block has already contributed, so a key spelled
            // twice in one block counts **one** block and two sites while a key
            // spelled in two blocks counts two. Counting blocks from the key's
            // first appearance anywhere in the record would under-report every
            // key after its own first block.
            let mut in_this_block: BTreeSet<String> = BTreeSet::new();
            let mut index = 0;
            while index < children.len() {
                let Some(name) = children[index].as_text() else {
                    record.refusals.push(BlockRefusal::KeyNotText {
                        block: key.to_owned(),
                        index,
                    });
                    break;
                };
                let shape = match children.get(index + 1) {
                    Some(next) if next.as_list().is_some() => DirectiveShape::Arguments(
                        next.as_list()
                            .unwrap_or_default()
                            .iter()
                            .map(arg_shape)
                            .collect(),
                    ),
                    // A text follower is the next directive's key: the original
                    // spells a no-argument directive by leaving the next key
                    // beside it, and this is the only reading the corpus
                    // supports (measured: every no-argument site in M01 is
                    // followed by a key, and no site is followed by a bare
                    // string that is not a key). A scalar follower cannot be a
                    // key — keys are text — so it is this directive's value.
                    None => DirectiveShape::Bare,
                    Some(ZrdValue::Text(_)) => DirectiveShape::Bare,
                    Some(_) => DirectiveShape::NotAList,
                };
                let advance = match shape {
                    // A text follower is the **next** directive's key, so this
                    // directive takes no argument and the walk advances by one
                    // child. A scalar or a list is this directive's argument and
                    // the walk advances by two.
                    DirectiveShape::Bare => 1,
                    _ => 2,
                };
                let entry = keys
                    .entry(name.to_owned())
                    .or_insert((0, 0, BTreeMap::new()));
                if in_this_block.insert(name.to_owned()) {
                    entry.0 += 1;
                }
                entry.1 += 1;
                *entry.2.entry(shape).or_insert(0) += 1;
                record.sites += 1;
                index += advance;
            }
            continue;
        }
        if let Some(field) = ControlRecordField::from_key(key) {
            *fields.entry(field).or_insert(0) += 1;
            let shape = match value.as_list() {
                Some(children) => {
                    DirectiveShape::Arguments(children.iter().map(arg_shape).collect())
                }
                None => DirectiveShape::NotAList,
            };
            field_shapes.insert((field, shape), ());
        } else {
            unclassified.insert(key.to_owned(), ());
        }
    }

    record.keys = keys
        .into_iter()
        .map(|(key, (blocks, sites, shapes))| MeasuredDirectiveKey {
            key,
            blocks,
            sites,
            shapes: shapes.into_iter().collect(),
        })
        .collect();
    record.record_fields = fields.into_iter().collect();
    record.record_field_shapes = field_shapes
        .into_iter()
        .map(|((field, shape), ())| (field.key().to_owned(), shape))
        .collect();
    record.unclassified_record_keys = unclassified.into_keys().collect();
    record
}

// ---------------------------------------------------------------------------
// The lowering accounting
// ---------------------------------------------------------------------------

/// Which requirement of `lower_program` a [`LoweringRequirement`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoweringRequirementKind {
    /// `RawProgram::mission`: the program's stable mission `ContentId`.
    MissionIdentity,
    /// `RawObjective::content`: one stable objective `ContentId` per block.
    ObjectiveIdentity,
    /// `RawObjective::condition`: the side-effect-free predicate that latches a
    /// block.
    ObjectiveCondition,
    /// `RawCall`: one host call per directive, with its arguments in the IR's
    /// value types.
    CallArguments,
}

impl LoweringRequirementKind {
    /// Every requirement, in the order `lower_program` needs them.
    pub const ALL: [LoweringRequirementKind; 4] = [
        Self::MissionIdentity,
        Self::ObjectiveIdentity,
        Self::ObjectiveCondition,
        Self::CallArguments,
    ];

    /// The stable identifier a report and a log carry.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissionIdentity => "mission_identity",
            Self::ObjectiveIdentity => "objective_identity",
            Self::ObjectiveCondition => "objective_condition",
            Self::CallArguments => "call_arguments",
        }
    }

    /// What the requirement is, in one line, for a report.
    #[must_use]
    pub const fn need(self) -> &'static str {
        match self {
            Self::MissionIdentity => "a stable mission ContentId",
            Self::ObjectiveIdentity => "one stable objective ContentId per numbered block",
            Self::ObjectiveCondition => "one side-effect-free condition per numbered block",
            Self::CallArguments => "one host call per directive with IR-carriable arguments",
        }
    }
}

impl fmt::Display for LoweringRequirementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.need())
    }
}

/// One row of the lowering accounting: what `lower_program` needs, what the
/// record spells, and the fields that stay unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoweringRequirement {
    /// Which requirement this row is.
    pub kind: LoweringRequirementKind,
    /// Whether the record meets it.
    pub met: bool,
    /// The measured numbers behind the answer, rendered for a report. Empty when
    /// the requirement is met and needs no explanation.
    pub measurement: String,
    /// The fields that remain unmeasured, each named. Never a defaulted empty
    /// list on an unmet row: an unmet requirement with nothing named is the
    /// failure this accounting exists to prevent.
    pub unmeasured_fields: Vec<String>,
}

impl LoweringRequirement {
    /// A met row, with no measurement text and nothing unmeasured.
    #[must_use]
    pub fn met(kind: LoweringRequirementKind, measurement: impl Into<String>) -> Self {
        Self {
            kind,
            met: true,
            measurement: measurement.into(),
            unmeasured_fields: Vec::new(),
        }
    }

    /// An unmet row, naming every field that stays unknown.
    #[must_use]
    pub fn unmet(
        kind: LoweringRequirementKind,
        measurement: impl Into<String>,
        unmeasured_fields: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            kind,
            met: false,
            measurement: measurement.into(),
            unmeasured_fields: unmeasured_fields.into_iter().collect(),
        }
    }

    /// The stable label a report carries.
    #[must_use]
    pub fn label(&self) -> String {
        if self.met {
            format!("{}: met", self.kind.code())
        } else {
            format!("{}: unmet", self.kind.code())
        }
    }
}

/// The record's lowering accounting, row per requirement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlLowering {
    requirements: Vec<LoweringRequirement>,
}

impl ControlLowering {
    /// The rows, in [`LoweringRequirementKind::ALL`] order.
    #[must_use]
    pub fn requirements(&self) -> &[LoweringRequirement] {
        &self.requirements
    }

    /// The unmet rows, in order.
    pub fn unmet(&self) -> impl Iterator<Item = &LoweringRequirement> {
        self.requirements.iter().filter(|row| !row.met)
    }

    /// Every field that stays unmeasured across the unmet rows, deduplicated and
    /// sorted.
    #[must_use]
    pub fn unmeasured_fields(&self) -> Vec<String> {
        let mut fields: BTreeMap<String, ()> = BTreeMap::new();
        for row in self.unmet() {
            for field in &row.unmeasured_fields {
                fields.insert(field.clone(), ());
            }
        }
        fields.into_keys().collect()
    }

    /// Whether every requirement is met.
    ///
    /// `false` for every measured retail record today. It is the gate a
    /// `MissionProgram` would have to pass before a mission could be launched,
    /// and it fails closed: a record with no measured keys at all has unmet rows
    /// because its rows say so, not because it is empty.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.requirements.iter().all(|row| row.met)
    }

    /// Builds the accounting for one measured record.
    #[must_use]
    pub fn measure(record: &MeasuredControlRecord) -> Self {
        let blocks = record.blocks();
        let keys = record.keys();

        // The mission id is not a member field: the archive is mission-scoped by
        // its path (F13-B's rule) and the canonical id comes from the campaign
        // binding record M01-A derived. That is a real answer, so the row is met
        // — and it says where the answer comes from, so nobody later reads it as
        // something the control member spells.
        let mission = LoweringRequirement::met(
            LoweringRequirementKind::MissionIdentity,
            "the member spells no mission identity; the reader archive is mission-scoped by path \
             and the canonical id comes from the campaign binding record (M01-A)",
        );

        // The per-block identity is derivable now that the stage-B measurement
        // is in: every block is authored under its own `OBJECTIVE<N>` key at a
        // distinct position in the record, every cross-objective directive
        // addresses a block by its zero-based index, and `IDENTITY` is
        // measured to be presentation data — class channel plus HUD ordinal —
        // not an identity. A record with no blocks spells no program at all,
        // so the row stays unmet there rather than vacuously met.
        let identity_sites = keys
            .iter()
            .filter(|key| key.key == OBJECTIVE_IDENTITY_KEY)
            .map(|key| key.sites)
            .sum::<u32>();
        let objective_identity = if blocks > 0 {
            LoweringRequirement::met(
                LoweringRequirementKind::ObjectiveIdentity,
                format!(
                    "{blocks} numbered block(s); each is authored under its own `OBJECTIVE<N>` key \
                     at a distinct position in the record, and every cross-objective directive \
                     addresses a block by its zero-based index (measured), so one stable objective \
                     ContentId per block is derivable; {identity_sites} `{OBJECTIVE_IDENTITY_KEY}` \
                     site(s) supply the presentation class and HUD ordinal the completion and \
                     save/load paths read, which is measured presentation data rather than identity"
                ),
            )
        } else {
            LoweringRequirement::unmet(
                LoweringRequirementKind::ObjectiveIdentity,
                "0 numbered block(s); the record declares no objective program at all",
                vec!["one numbered block authored under an `OBJECTIVE<N>` key".to_owned()],
            )
        };

        // The completion-condition evaluators: measured where the findings
        // cover them — they read live world state (member handles, registry
        // bytes, animation states, the named counters) and several perform
        // measured side effects during evaluation — so none is the
        // side-effect-free `Condition` `RawObjective::condition` requires,
        // whatever its shape. A block with no evaluator completes when awake:
        // a lifecycle state, not a predicate.
        let condition_keys: Vec<&MeasuredDirectiveKey> = keys
            .iter()
            .filter(|key| {
                matches!(key.disposition(), DirectiveDisposition::Measured(directive)
                    if directive.operation.role() == DirectiveRole::CompletionCondition)
            })
            .collect();
        let evaluator_sites: u32 = condition_keys.iter().map(|key| key.sites).sum();
        let stage_sites: u32 = keys
            .iter()
            .filter(|key| is_objective_inactive_stage(&key.key))
            .map(|key| key.sites)
            .sum();
        let threshold_sites: u32 = keys
            .iter()
            .filter(|key| key.key == OBJECTIVE_INACTIVE_COUNT_KEY)
            .map(|key| key.sites)
            .sum();
        let dormant_sites: u32 = keys
            .iter()
            .filter(|key| key.key == OBJECTIVE_DORMANT_KEY)
            .map(|key| key.sites)
            .sum();
        let condition = if blocks == 0 {
            LoweringRequirement::unmet(
                LoweringRequirementKind::ObjectiveCondition,
                "0 numbered block(s); the record declares no objective program at all",
                vec!["one numbered block whose condition could be lowered".to_owned()],
            )
        } else {
            let mut fields = residual_unknowns(&condition_keys);
            let gaps = shape_gaps(&condition_keys);
            let condition_gaps = gaps.len();
            fields.extend(gaps);
            fields.push(format!(
                "one side-effect-free `Condition` for each of the {blocks} block(s)"
            ));
            LoweringRequirement::unmet(
                LoweringRequirementKind::ObjectiveCondition,
                format!(
                    "{evaluator_sites} completion-evaluator site(s) over {} measured key(s) — \
                     {stage_sites} inactive-stage site(s) beside {threshold_sites} \
                     completion-count threshold(s) and {dormant_sites} dormant marker(s); \
                     {condition_gaps} evaluator key(s) spell no single `Value`-carriable \
                     signature; the evaluators are measured to read live world state and several \
                     write during evaluation, so none is the side-effect-free `Condition` \
                     `RawObjective::condition` requires",
                    condition_keys.len(),
                ),
                fields,
            )
        };

        // The calls: every non-condition directive needs a host call, and none
        // has one — the measured operations are lifecycle writes, evaluator
        // arming and ordered pipeline effects, none of which is a
        // `cs_script::bindings::Lowering` variant. Measured keys still name
        // their residual unknowns and their shape gaps; unmeasured keys name
        // their reason.
        let call_keys: Vec<&MeasuredDirectiveKey> = keys
            .iter()
            .filter(|key| {
                !matches!(
                    key.disposition(),
                    DirectiveDisposition::TerminalOutcome { .. }
                ) && !matches!(key.disposition(), DirectiveDisposition::Measured(directive)
                    if directive.operation.role() == DirectiveRole::CompletionCondition)
            })
            .collect();
        let measured_calls = call_keys
            .iter()
            .filter(|key| matches!(key.disposition(), DirectiveDisposition::Measured(_)))
            .count();
        let unmeasured_calls: Vec<&MeasuredDirectiveKey> = call_keys
            .iter()
            .copied()
            .filter(|key| matches!(key.disposition(), DirectiveDisposition::Unmeasured { .. }))
            .collect();
        let shape_blocked = call_keys
            .iter()
            .filter(|key| {
                key.shapes.len() > 1
                    || key
                        .agreed_shape()
                        .is_some_and(|shape| !shape.is_ir_carriable())
            })
            .count();
        let widest = keys
            .iter()
            .filter_map(|key| key.agreed_shape())
            .map(DirectiveShape::arity)
            .max()
            .unwrap_or(0);
        let calls = if keys.is_empty() {
            LoweringRequirement::unmet(
                LoweringRequirementKind::CallArguments,
                "0 directive key(s); the record declares no directive to lower",
                vec!["a directive whose measured operation could be bound".to_owned()],
            )
        } else {
            let mut fields: Vec<String> = unmeasured_calls
                .iter()
                .map(|key| {
                    let disposition = key.disposition();
                    let reason = disposition
                        .refusal()
                        .expect("an unmeasured disposition carries a reason");
                    format!("`{}`: {}", key.key, reason)
                })
                .collect();
            fields.extend(residual_unknowns(&call_keys));
            fields.extend(shape_gaps(&call_keys));
            fields.push(
                "one bound host call per directive site — no `cs_script::bindings::Lowering` \
                 variant carries the measured directive operations"
                    .to_owned(),
            );
            LoweringRequirement::unmet(
                LoweringRequirementKind::CallArguments,
                format!(
                    "{} directive key(s) over {} site(s); {measured_calls} measured non-condition \
                     key(s) and {} unmeasured one(s) would each need a host call; \
                     {shape_blocked} of them spell no single `Value`-carriable signature; widest \
                     agreed arity {widest}; the measured dispositions are lifecycle writes, \
                     evaluator arming and ordered pipeline effects — no \
                     `cs_script::bindings::Lowering` variant exists for them",
                    record.vocabulary(),
                    record.sites(),
                    unmeasured_calls.len(),
                ),
                fields,
            )
        };

        Self {
            requirements: vec![mission, objective_identity, condition, calls],
        }
    }
}

/// The residual unknowns the measured keys in `keys` still carry, grouped by
/// the unknown so each names every key it applies to once.
fn residual_unknowns(keys: &[&MeasuredDirectiveKey]) -> Vec<String> {
    let mut by_unknown: BTreeMap<&'static str, Vec<&str>> = BTreeMap::new();
    for key in keys {
        if let DirectiveDisposition::Measured(directive) = key.disposition() {
            for unknown in directive.unknowns {
                by_unknown
                    .entry(unknown)
                    .or_default()
                    .push(key.key.as_str());
            }
        }
    }
    by_unknown
        .into_iter()
        .map(|(unknown, spelled)| format!("{unknown} (spelled by {})", spelled.join(", ")))
        .collect()
}

/// The argument-shape gaps `lower_program` would hit for `keys`, one line per
/// spelled defect — the refusal a per-key disposition no longer carries now
/// that the key's *effect* may be measured.
fn shape_gaps(keys: &[&MeasuredDirectiveKey]) -> Vec<String> {
    let mut gaps = Vec::new();
    for key in keys {
        match key.shapes.len() {
            0 => {}
            1 => {
                if let Some(shape) = key.agreed_shape()
                    && !shape.is_ir_carriable()
                {
                    gaps.push(format!(
                        "`{}`: every site spells {}, which `cs_script::ir::Value` cannot carry",
                        key.key,
                        shape.label()
                    ));
                }
            }
            count => gaps.push(format!(
                "`{}`: {count} distinct argument shapes across its sites — no single call signature",
                key.key
            )),
        }
    }
    gaps
}

impl fmt::Display for ControlLowering {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in &self.requirements {
            writeln!(f, "{}: {}", row.label(), row.measurement)?;
            for field in &row.unmeasured_fields {
                writeln!(f, "    unmeasured: {field}")?;
            }
        }
        Ok(())
    }
}

/// The refusal a mission's control program raises when the record cannot be
/// lowered: the unmet rows, the fields they name and any unreadable block.
///
/// An unreadable block is named here too, because it is the one case where the
/// record's own directives are **unknown rather than unmeasured** — a block this
/// walk cannot read might hold a directive no other part of the census mentions.
#[must_use]
pub fn lowering_refusal(record: &MeasuredControlRecord) -> String {
    let lowering = record.lowering();
    let mut out = if lowering.complete() && record.refusals.is_empty() {
        String::new()
    } else {
        format!(
            "the control record spells {} of the {} things lower_program needs",
            LoweringRequirementKind::ALL.len() - lowering.unmet().count(),
            LoweringRequirementKind::ALL.len()
        )
    };
    for field in lowering.unmeasured_fields() {
        out.push_str(&format!("\n  unmeasured: {field}"));
    }
    for refusal in &record.refusals {
        out.push_str(&format!("\n  unreadable: {refusal}"));
    }
    out
}
