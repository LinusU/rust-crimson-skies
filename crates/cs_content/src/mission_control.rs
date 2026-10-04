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
//! `Vec<cs_script::ir::Value>`. The control record spells **none** of those four
//! things, and the reasons are measured, not stylistic:
//!
//! | What `lower_program` needs | What the record spells | Verdict |
//! | --- | --- | --- |
//! | the mission's `ContentId` | nothing; the member is mission-scoped by *path* | supplied outside the member (`missions/bindings/M01.json`, M01-A) |
//! | an objective `ContentId` per block | `IDENTITY` writes a role spelling, a bare integer and an optional briefing label | [`LoweringRequirement::ObjectiveIdentity`], unmeasured |
//! | a `Condition` per block | nothing that reads as a predicate; the closest keys are the `INACTIVE<n>` stages beside `INACTIVE_COMPLETION_COUNT` (F39-E4 measured their *names*, not their meaning) and `BEGIN_DORMANT`'s unnamed number | [`LoweringRequirement::ObjectiveCondition`], unmeasured |
//! | a flat `Vec<Value>` per call | 43 measured keys whose argument lists are frequently **nested** (`[[text,text]]`, `[text,[text,[text],text,[text]]]`) and whose `KILL_/WAKE_OBJECTIVE_WHEN_I_COMPLETE` sites carry up to 12 integers corpus-wide | [`LoweringRequirement::CallArguments`], unmeasured |
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
    is_objective_inactive_stage, objective_block_number,
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
/// falsifiable — M01's `wv_tailhook.zrd` is three times longer than its
/// `objectives.zrd` and carries no block at all, so the size-and-name heuristic
/// the task description started from ([`CONTROL_MEMBER`]'s doc) is measured to be
/// wrong, and this function is what replaces it.
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
    #[must_use]
    pub fn disposition(&self) -> DirectiveDisposition {
        if let Some(outcome) = terminal_outcome_of(&self.key) {
            return DirectiveDisposition::TerminalOutcome { outcome };
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

/// What became of one measured directive key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectiveDisposition {
    /// The mission IR has an action for it: a terminal outcome request.
    TerminalOutcome {
        /// The outcome the key's **spelling** names.
        outcome: TerminalOutcome,
    },
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
    #[must_use]
    pub const fn is_implemented(&self) -> bool {
        matches!(self, Self::TerminalOutcome { .. })
    }

    /// The refusal, or `None` for an implemented disposition.
    #[must_use]
    pub const fn refusal(&self) -> Option<&UnmeasuredReason> {
        match self {
            Self::TerminalOutcome { .. } => None,
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

    /// The measured keys whose effect is not implemented, in key order.
    #[must_use]
    pub fn unmeasured(&self) -> Vec<(&MeasuredDirectiveKey, DirectiveDisposition)> {
        self.keys
            .iter()
            .filter(|measured| !measured.disposition().is_implemented())
            .map(|measured| (measured, measured.disposition()))
            .collect()
    }

    /// The measured keys the engine may act on, in key order.
    #[must_use]
    pub fn implemented(&self) -> Vec<(&MeasuredDirectiveKey, TerminalOutcome)> {
        self.keys
            .iter()
            .filter_map(|measured| match measured.disposition() {
                DirectiveDisposition::TerminalOutcome { outcome } => Some((measured, outcome)),
                DirectiveDisposition::Unmeasured { .. } => None,
            })
            .collect()
    }

    /// How many distinct keys the record spells.
    #[must_use]
    pub fn vocabulary(&self) -> u32 {
        self.keys.len() as u32
    }

    /// Whether every directive of the record has an implemented disposition and
    /// every block was read.
    ///
    /// `false` for every measured retail record, and that is the correct reading
    /// rather than a missing one: a mission may not be launched off a record whose
    /// directives the engine cannot honour (contract "Source adapter acceptance").
    /// An empty record also answers `false`, so a member nobody read can never
    /// report itself complete.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.keys.is_empty() && self.refusals.is_empty() && self.unmeasured().is_empty()
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

        // The per-block identity the record *does* spell: `IDENTITY` sites.
        let identity_sites = keys
            .iter()
            .filter(|key| key.key == OBJECTIVE_IDENTITY_KEY)
            .map(|key| key.sites)
            .sum::<u32>();
        let objective_identity = LoweringRequirement::unmet(
            LoweringRequirementKind::ObjectiveIdentity,
            format!(
                "{blocks} numbered block(s); {identity_sites} `{OBJECTIVE_IDENTITY_KEY}` site(s) \
                 spell a role spelling, a bare integer and an optional briefing label, and no block \
                 spells a content id"
            ),
            vec![
                "the meaning of the integer an IDENTITY site carries".to_owned(),
                "whether the role spelling indexes an objective or a label".to_owned(),
                format!("a stable objective ContentId for each of the {blocks} block(s)"),
            ],
        );

        // The closest thing to a predicate, counted rather than described.
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
        let condition = LoweringRequirement::unmet(
            LoweringRequirementKind::ObjectiveCondition,
            format!(
                "{stage_sites} inactive-stage site(s) beside {threshold_sites} completion-count \
                 threshold(s) and {dormant_sites} dormant marker(s); F39-E4 measured the stages' \
                 names, not the rule they state, and no block spells a predicate"
            ),
            vec![
                "the rule an INACTIVE stage states when its count is met".to_owned(),
                "what an INACTIVE_COMPLETION_COUNT threshold is counted over".to_owned(),
                "what the number a BEGIN_DORMANT site carries means".to_owned(),
                format!("one side-effect-free condition for each of the {blocks} block(s)"),
            ],
        );

        // The calls: how many keys, how many sites, and which of them have an
        // argument shape the IR cannot carry.
        let nested = keys
            .iter()
            .filter(|key| {
                key.agreed_shape()
                    .is_some_and(|shape| !shape.is_ir_carriable())
            })
            .count();
        let widest = keys
            .iter()
            .filter_map(|key| key.agreed_shape())
            .map(DirectiveShape::arity)
            .max()
            .unwrap_or(0);
        let calls = LoweringRequirement::unmet(
            LoweringRequirementKind::CallArguments,
            format!(
                "{} directive key(s) over {} site(s); {nested} key(s) whose agreed shape nests a \
                 list, which cs_script::ir::Value cannot carry; widest agreed arity {widest}",
                record.vocabulary(),
                record.sites()
            ),
            vec![
                "the effect of every directive key that is not an outcome key".to_owned(),
                "the meaning of every number a directive carries".to_owned(),
                "the order and the meaning of the parts of every nested argument list".to_owned(),
            ],
        );

        Self {
            requirements: vec![mission, objective_identity, condition, calls],
        }
    }
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
