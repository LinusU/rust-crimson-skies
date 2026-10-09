//! Lowering one numbered block's measured directives into a side-effect-free
//! completion [`Condition`] (`M01-LC-DIRECTIVE-LOWERING.02`).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` — "IR requirements" and
//! "Objective event ordering" ("Conditions distinguish disabled, dead,
//! captured, escaped, detached and despawned"; *measure rather than assume*).
//! The measured semantics come from
//! `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
//! and `…-m01-lc-directive-c-ai-world-and-animation-directives.md`, plus the
//! record itself.
//!
//! # What this module decides
//!
//! The original completes a block in `CZMission::Update`'s pass 2: a gate
//! (the block itself awake, its `TICK_DEPENDS_ON_OBJ` dependency awake, not
//! already completed), then the first **armed** evaluator that is true, and
//! — only when no evaluator fired — a fallthrough AND-gate that admits a
//! block with nothing armed. [`lower_block_condition`] reproduces exactly
//! that shape as one predicate:
//!
//! * every block carries [`Condition::ObjectiveAwake`] for itself, so a block
//!   that starts dormant cannot latch at tick 0 and a completed dependency
//!   freezes its dependent (the measured `TICK_DEPENDS_ON_OBJ` gate);
//! * an armed evaluator becomes the block's predicate; two **different
//!   kinds** would be [`Condition::Any`] (the original's "first true wins");
//!   a second spelling of the *same* kind refuses instead, because the record
//!   holds one slot per kind and overwrites it at parse. `ANIM_STATE` is
//!   slotted the same way, silently: the parse runs its helper once per
//!   block and the helper's lookup takes the **first** `ANIM_STATE` text in
//!   the record's depth-first order, so a second site is never reached
//!   rather than refused;
//! * a block with **no** armed evaluator is the gate alone — measured: "an
//!   objective with no armed condition completes on the first tick it is
//!   awake". That is a lifecycle state, never a `Condition::Const(_)`.
//!
//! # What it refuses
//!
//! A residual unknown that would change what "complete" means never becomes a
//! predicate: the block is refused **by block and key**, and the refusal
//! carries the field a caller reports in `objective_condition`'s unmet row
//! ([`ConditionRefusal::field`]). Unknowns that do *not* change the predicate
//! stay on the condition as [`crate::ir::Condition::residual_unknowns`],
//! named rather than dropped.
//!
//! Effect keys — everything the original runs inside the completion, wake or
//! transition pipeline — are **not** part of a predicate and are deliberately
//! skipped here: their host calls belong to `lower_program`'s
//! `call_arguments` row, which another task owns. The pass-2 evaluator list
//! in finding B is complete, so a key outside it is measured not to be a
//! predicate.

use std::fmt;

use crate::ir::{AnimationState, Condition, MAX_VALUE_ITEMS, MemberName, TravelersAnchor, Value};

/// The measured polarity token `TRAVELERS` child1 spells for the *inside*
/// pole. Only this token is a spelling the original stores as `1`; the
/// outside pole's own word was never measured (finding C, unknown 6), so a
/// site that spells anything else is refused rather than read as one pole or
/// the other.
pub const TRAVELERS_APPROACHING: &str = "APPROACHING";

/// The measured descriptor tag `ANIM_STATE` child0 spells — a five-byte
/// compare before the spec record (finding C).
pub const ANIM_STATE_TAG: &str = "ANIM";

/// The `ANIM_STATE` key text itself: the string the parse's single
/// depth-first lookup matches anywhere in the block's record (finding C).
const ANIM_STATE_KEY: &str = "ANIM_STATE";

/// The `COMPLETION_COUNT` key text: read recursively inside the found
/// `ANIM_STATE` operand list only — the parse never looks it up on the
/// block, so a top-level spelling of the same key is inert (finding C:
/// `0x57a1b0` is called on the found list, not on the record).
const ANIM_STATE_COUNT_KEY: &str = "COMPLETION_COUNT";

/// The measured zone-name key `0x465ec0` looks up once per block: its
/// follower is the zone-name list, its count the flag array's length.
const DANGER_ZONES_KEY: &str = "DANGER_ZONES_COMPLETED";

/// The measured flag-count key `0x465ec0` looks up once per block, after the
/// zone-name list: its first child payload is the threshold the evaluator
/// compares the nonzero flag bytes against.
const DANGER_ZONES_COUNT_KEY: &str = "DANGER_ZONES_COMPLETION_COUNT";

/// The stable code of the lowering requirement a refusal blocks —
/// `LoweringRequirementKind::ObjectiveCondition::code()` on the measurement
/// side, mirrored here so [`ConditionRefusal::field`] renders a row the
/// accounting can place without a second source of truth (the acceptance
/// suite pins the two codes together).
pub const OBJECTIVE_CONDITION_REQUIREMENT: &str = "objective_condition";

/// The measured `INACTIVE<n>` prefix: `INACTIVE` followed by a non-empty run
/// of ASCII digits, exactly the rule `cs_content::objectives` counts with
/// (cross-checked by the acceptance suite so the two copies cannot drift).
const INACTIVE_STAGE_PREFIX: &str = "INACTIVE";

/// `INACTIVE<n>`'s stage number, or `None` for any other spelling.
///
/// `INACTIVE_COMPLETION_COUNT` is not one (its rest is not digits) and
/// neither is a bare `INACTIVE`.
fn inactive_stage(key: &str) -> Option<u32> {
    let rest = key.strip_prefix(INACTIVE_STAGE_PREFIX)?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// One directive of one numbered block, as the record's directive grammar
/// yields it.
///
/// The grammar is asymmetric on purpose and comes from the production walk
/// (`cs_content::stunts::zrd_directive_fields`, the same rule
/// `mission_control::measure_control_record` applies): a text key is followed
/// by its argument list **if and only if** it has one, so a key standing
/// beside the next key takes no argument.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDirective {
    /// The key as the record spells it.
    pub key: String,
    /// What the site wrote beside the key.
    pub args: DirectiveArguments,
}

/// What one site wrote beside its key — the three cases
/// `cs_content::mission_control::DirectiveShape` measures.
#[derive(Clone, Debug, PartialEq)]
pub enum DirectiveArguments {
    /// The key stood alone: no argument list was written at all.
    Bare,
    /// A scalar where the grammar expects a list — `not_a_list`, a shape no
    /// key this module reads ever spells.
    NotAList(Value),
    /// The site's argument list, in order.
    List(Vec<Value>),
}

impl BlockDirective {
    /// Builds one directive from a key and its argument list.
    #[must_use]
    pub fn new(key: impl Into<String>, args: Vec<Value>) -> Self {
        Self {
            key: key.into(),
            args: DirectiveArguments::List(args),
        }
    }

    /// Builds one directive whose key stood alone.
    #[must_use]
    pub fn bare(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            args: DirectiveArguments::Bare,
        }
    }

    /// The site's argument list, or the refusal that the site spelled no list
    /// where every measured site of this key spells one.
    fn operands(&self, block: &str) -> Result<&[Value], ConditionRefusal> {
        match &self.args {
            DirectiveArguments::List(args) => Ok(args),
            DirectiveArguments::Bare => Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: self.key.clone(),
                detail: "the site spelled no argument list, and every measured site of this key \
                         spells one"
                    .to_owned(),
            }),
            DirectiveArguments::NotAList(value) => Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: self.key.clone(),
                detail: format!(
                    "the site spelled {} where the measured shape is a list",
                    value_label(value)
                ),
            }),
        }
    }
}

/// A short label for one argument, for a refusal's detail.
fn value_label(value: &Value) -> String {
    match value {
        Value::Int(n) => format!("the int {n}"),
        Value::Float(f) => format!("the float {f}"),
        Value::Str(text) => format!("the text {text:?}"),
        Value::List(items) => format!("a {}-element list", items.len()),
        other => format!("a {} value", value_kind(other)),
    }
}

/// The kind name of an argument, for a refusal's detail.
fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "text",
        Value::Content(_) => "content",
        Value::Actor(_) => "actor",
        Value::Vector(_) => "vector",
        Value::OptActor(_) => "opt-actor",
        Value::List(_) => "list",
    }
}

/// Why one numbered block has no completion condition.
///
/// Every variant names the **block** and the **key** that refused it
/// ([`Self::block`], [`Self::key`]), and [`Self::field`] renders the pair the
/// way the lowering accounting reports an unmet `objective_condition` row —
/// so a caller can name the field without parsing a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConditionRefusal {
    /// The block's evaluator carries a residual unknown that changes what
    /// "complete" means, so this build offers no predicate for it.
    UnknownChangesPredicate {
        /// The block key (`OBJECTIVE3`).
        block: String,
        /// The directive key that refused.
        key: String,
        /// The unknown, named.
        detail: String,
    },
    /// The block's evaluator is one this build lowers no condition for, so
    /// offering one would be a guess at its predicate.
    NoConditionFor {
        /// The block key.
        block: String,
        /// The directive key that refused.
        key: String,
        /// What is missing, named.
        detail: String,
    },
    /// The site's operands are outside every shape the findings measured.
    BadOperands {
        /// The block key.
        block: String,
        /// The directive key that refused.
        key: String,
        /// What the site spelled and why it is not a measured shape.
        detail: String,
    },
}

impl ConditionRefusal {
    /// The block the refusal sits in.
    #[must_use]
    pub fn block(&self) -> &str {
        match self {
            Self::UnknownChangesPredicate { block, .. }
            | Self::NoConditionFor { block, .. }
            | Self::BadOperands { block, .. } => block,
        }
    }

    /// The directive key that refused.
    #[must_use]
    pub fn key(&self) -> &str {
        match self {
            Self::UnknownChangesPredicate { key, .. }
            | Self::NoConditionFor { key, .. }
            | Self::BadOperands { key, .. } => key,
        }
    }

    /// What is missing, named.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            Self::UnknownChangesPredicate { detail, .. }
            | Self::NoConditionFor { detail, .. }
            | Self::BadOperands { detail, .. } => detail,
        }
    }

    /// The field a caller reports in the unmet `objective_condition` row:
    /// the block, the key and what is missing, rendered once.
    ///
    /// This is the "visible to a caller" half of AC4 — a report can push it
    /// into `LoweringRequirement::unmet`'s `unmeasured_fields` and then name
    /// the block and the key rather than only the requirement.
    #[must_use]
    pub fn field(&self) -> String {
        format!(
            "{}: `{}` `{}`: {}",
            OBJECTIVE_CONDITION_REQUIREMENT,
            self.block(),
            self.key(),
            self.detail()
        )
    }
}

impl fmt::Display for ConditionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.field())
    }
}

impl std::error::Error for ConditionRefusal {}

/// Why the directive walk could not read a block.
///
/// The two refusals `cs_content::mission_control::BlockRefusal` records,
/// mirrored here because `cs_script` may depend on `cs_types` only
/// (`docs/01-ARCHITECTURE.md`) and cannot name that type. The acceptance
/// suite pins [`Self::code`] to the measurement side's codes, so the two
/// copies cannot drift — the same edge `DirectiveOperation`'s mirror uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockRefusal {
    /// The block's value is not a list, so it holds no directives at all.
    BlockNotAList {
        /// The block key as the record spells it.
        block: String,
    },
    /// A child of the block is not a text key, so the directive it starts
    /// cannot be read.
    KeyNotText {
        /// The block key.
        block: String,
        /// The zero-based child position inside the block.
        index: usize,
    },
}

impl BlockRefusal {
    /// The stable identifier the measurement side publishes for the same
    /// refusal: `block_not_a_list`, `key_not_text`.
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
            Self::BlockNotAList { block } => write!(
                f,
                "{block}: {}: the block is not a directive list",
                self.code()
            ),
            Self::KeyNotText { block, index } => {
                write!(
                    f,
                    "{block}: {}: child {index} is not a directive key",
                    self.code()
                )
            }
        }
    }
}

/// One numbered block as the record's directive walk yields it.
#[derive(Clone, Debug, PartialEq)]
pub enum RawBlock {
    /// The walk read every directive of the block.
    Read {
        /// The block key (`OBJECTIVE7`).
        block: String,
        /// The block's zero-based position in the record — the index every
        /// cross-objective directive spells (measured).
        index: u32,
        /// The block's directives, in record order.
        directives: Vec<BlockDirective>,
    },
    /// The walk could not read the block. The refusal is carried into the
    /// lowered output instead of the block silently vanishing from it (AC5: a
    /// block the directive walk cannot read stays a `BlockRefusal`).
    Unreadable(BlockRefusal),
}

/// The completion condition of one numbered block, or why it has none.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockCondition {
    /// The block lowered to a predicate `MissionProgram::validate` accepts.
    Lowered(Condition),
    /// The block's evaluator refused it **by block and key**.
    Refused(ConditionRefusal),
    /// The directive walk could not read the block; the refusal stands.
    Unreadable(BlockRefusal),
}

impl BlockCondition {
    /// The lowered condition, or `None` when the block has none.
    #[must_use]
    pub fn condition(&self) -> Option<&Condition> {
        match self {
            Self::Lowered(condition) => Some(condition),
            Self::Refused(_) | Self::Unreadable(_) => None,
        }
    }
}

impl fmt::Display for BlockCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lowered(_) => f.write_str("lowered"),
            Self::Refused(refusal) => write!(f, "refused: {refusal}"),
            Self::Unreadable(refusal) => write!(f, "unreadable: {refusal}"),
        }
    }
}

/// Lowers one record's blocks, in record order.
///
/// An empty record produces no conditions (AC5): nothing is invented for a
/// block nobody read.
#[must_use]
pub fn lower_record(blocks: &[RawBlock]) -> Vec<BlockCondition> {
    blocks.iter().map(lower_block).collect()
}

/// Lowers one block the walk yields, preserving a walk refusal.
#[must_use]
pub fn lower_block(raw: &RawBlock) -> BlockCondition {
    match raw {
        RawBlock::Unreadable(refusal) => BlockCondition::Unreadable(refusal.clone()),
        RawBlock::Read {
            block,
            index,
            directives,
        } => match lower_block_condition(block, *index, directives) {
            Ok(condition) => BlockCondition::Lowered(condition),
            Err(refusal) => BlockCondition::Refused(refusal),
        },
    }
}

/// Lowers one read block's directives into its side-effect-free completion
/// predicate.
///
/// `index` is the block's zero-based position in the record: it is both the
/// key [`Condition::ObjectiveAwake`] reads and the index
/// `TICK_DEPENDS_ON_OBJ`'s `child0 - 1` names (measured), so the two resolve
/// in one key space with no mapping a caller could get wrong.
///
/// # Errors
///
/// A [`ConditionRefusal`] naming the block and the key, never a guessed
/// predicate: an evaluator this build does not lower, a second spelling of an
/// evaluator kind the record stores once, a polarity or state token outside
/// the measured vocabulary, or operands outside every measured shape.
pub fn lower_block_condition(
    block: &str,
    index: u32,
    directives: &[BlockDirective],
) -> Result<Condition, ConditionRefusal> {
    let mut gate = vec![Condition::ObjectiveAwake { index }];
    let mut dependency: Option<u32> = None;
    let mut members: Vec<MemberName> = Vec::new();
    let mut threshold: Option<u32> = None;
    let mut evaluators: Vec<Condition> = Vec::new();
    // The record holds **one slot per evaluator kind**: a second `DEDG` or
    // `TRAVELERS` spelling overwrites the first's fields at parse (finding B),
    // so the original evaluates one evaluator of each kind, never both. A
    // block that spells either key twice is refused rather than OR'd into a
    // disjunction the original cannot produce.
    let mut dedg_spelled = false;
    let mut travelers_spelled = false;
    // `ANIM_STATE` is slotted the same way — one evaluator per block — but by
    // a different measured route: the parse helper runs once (finding C,
    // `0x4691d0`) and its `0x57a090` lookup takes the FIRST `ANIM_STATE` text
    // in the record's depth-first order — a top-level directive key or a
    // text nested inside an earlier directive's operand list — and reads the
    // list right after it. A second `ANIM_STATE` site is never reached, and
    // a first text followed by no list arms no evaluator at all.
    let anim_evaluator = anim_state_site(directives).map(|operands| {
        let (animations, override_count) = anim_state(operands);
        let required = override_count.unwrap_or(animations.len() as u32);
        Condition::AnimationStates {
            required,
            animations,
        }
    });
    // `DANGER_ZONES_COMPLETED` is selected the same measured way, by a helper
    // the parse runs **once per block** (`0x465ec0`): both of its `0x57a090`
    // lookups — the zone-name list and the `DANGER_ZONES_COMPLETION_COUNT`
    // threshold beside it — take the first occurrence of their key in the
    // block's own depth-first order, so the whole evaluator is computed here,
    // before the loop, and a later spelling of either key is never reached:
    // inert, exactly as a second `ANIM_STATE` site is.
    let danger_evaluator = danger_zones(block, directives)?;

    for directive in directives {
        let key = directive.key.as_str();
        if let Some(stage) = inactive_stage(key) {
            // The parser measures the stage range `1..=100` (finding B); a
            // stage-shaped key outside it is a spelling this build cannot
            // read as a member row, refused rather than skipped — skipping it
            // would silently drop an evaluator from the predicate.
            if !(1..=100).contains(&stage) {
                return Err(ConditionRefusal::BadOperands {
                    block: block.to_owned(),
                    key: key.to_owned(),
                    detail: format!(
                        "the parser measures the stage range 1..=100 and this site spells \
                         INACTIVE{stage}"
                    ),
                });
            }
            members.push(member_chain(block, directive)?);
            continue;
        }
        match key {
            "INACTIVE_COMPLETION_COUNT" => {
                if threshold.is_some() {
                    return Err(ConditionRefusal::BadOperands {
                        block: block.to_owned(),
                        key: key.to_owned(),
                        detail: "the block spells the threshold twice, and one block carries one \
                                 threshold field"
                            .to_owned(),
                    });
                }
                let args = directive.operands(block)?;
                let [Value::Int(count)] = args else {
                    return Err(arity_error(block, directive, args, "one int"));
                };
                if *count < 0 {
                    return Err(ConditionRefusal::BadOperands {
                        block: block.to_owned(),
                        key: key.to_owned(),
                        detail: format!(
                            "the measured threshold is a count and this site spells {count}"
                        ),
                    });
                }
                threshold = Some(*count as u32);
            }
            "BEGIN_DORMANT" => {
                // Presence is the whole predicate contribution: it is what
                // makes the block start dormant, so it cannot latch at tick 0.
                // Child0 is the mission-clock second the block wakes itself
                // at, and the wake advance is the lifecycle table's job — a
                // clock comparison never enters the condition. The shape is
                // still checked, so a declaration this build cannot read is
                // refused instead of silently treated as "dormant, never
                // waking".
                let args = directive.operands(block)?;
                let [number] = args else {
                    return Err(arity_error(block, directive, args, "one number"));
                };
                match number {
                    Value::Float(f) if !f.is_finite() => {
                        return Err(ConditionRefusal::BadOperands {
                            block: block.to_owned(),
                            key: key.to_owned(),
                            detail: format!("the wake time {f} is not finite"),
                        });
                    }
                    Value::Float(_) | Value::Int(_) => {}
                    other => {
                        return Err(ConditionRefusal::BadOperands {
                            block: block.to_owned(),
                            key: key.to_owned(),
                            detail: format!(
                                "the measured shape is one finite number and this site spells {}",
                                value_label(other)
                            ),
                        });
                    }
                }
            }
            "TICK_DEPENDS_ON_OBJ" => {
                let args = directive.operands(block)?;
                let [Value::Int(target)] = args else {
                    return Err(arity_error(block, directive, args, "one int"));
                };
                // Measured: the parse stores `child0 - 1` unconditionally, so
                // a spelled `0` lands on the record's "no dependency" sentinel
                // `-1` and no gate exists — the same as not spelling the key.
                let spelled = i64::from(*target) - 1;
                if spelled >= 0 {
                    dependency = Some(spelled as u32);
                }
            }
            "DEDG" => {
                if dedg_spelled {
                    return Err(ConditionRefusal::NoConditionFor {
                        block: block.to_owned(),
                        key: key.to_owned(),
                        detail: "the record holds one DEDG slot: a second spelling overwrites \
                                 +0x580/+0x584 at parse, and the optional generator name at \
                                 +0x588 is aliased with TICK_DEPENDS_ON_OBJ, so only one DEDG \
                                 evaluator exists and OR-ing two spellings would offer a \
                                 predicate that evaluator cannot produce"
                            .to_owned(),
                    });
                }
                dedg_spelled = true;
                if let Some(condition) = dedg(block, directive)? {
                    evaluators.push(condition);
                }
            }
            "TRAVELERS" => {
                if travelers_spelled {
                    return Err(ConditionRefusal::NoConditionFor {
                        block: block.to_owned(),
                        key: key.to_owned(),
                        detail: "the record holds one TRAVELERS slot: a second spelling \
                                 overwrites the subject, the polarity, the anchor, the radius \
                                 and the required count at parse, so only one TRAVELERS \
                                 evaluator exists and OR-ing two spellings would offer a \
                                 predicate that evaluator cannot produce"
                            .to_owned(),
                    });
                }
                travelers_spelled = true;
                if let Some(condition) = travelers(block, directive)? {
                    evaluators.push(condition);
                }
            }
            // `ANIM_STATE` and both danger-zones keys are consumed by the
            // once-per-block site searches above (`anim_evaluator`,
            // `danger_evaluator`): the parse's own lookups take the FIRST
            // occurrence of each key in the block's depth-first order, so a
            // later spelling is never reached — inert, not a second
            // evaluator and not a refusal.
            "ANIM_STATE" | "DANGER_ZONES_COMPLETED" | "DANGER_ZONES_COMPLETION_COUNT" => {}
            "COUNTER" | "TEST_COMPLETE" => {
                return Err(ConditionRefusal::NoConditionFor {
                    block: block.to_owned(),
                    key: key.to_owned(),
                    detail: "the named-counter test is measured but this build lowers no \
                             condition for it: `TEST_LE` evaluates as `TEST_GE` in the original \
                             and no predicate may be offered before that is lowered"
                        .to_owned(),
                });
            }
            // Everything else is a completion, wake or transition effect, an
            // outcome class or presentation data: measured stages of the
            // pipeline, and none of them is a predicate — finding B's pass-2
            // evaluator list is the complete one. A top-level
            // `COMPLETION_COUNT` belongs here too: the parse only looks that
            // key up *inside* the ANIM_STATE operand list (`0x57a1b0` on the
            // found list), so a block-level spelling is inert and the call
            // accounting, not this module, owns its verdict.
            _ => {}
        }
    }

    if let Some(dependency) = dependency {
        gate.push(Condition::ObjectiveAwake { index: dependency });
    }
    if !members.is_empty() {
        // Measured: the threshold defaults to the block's own member count
        // when the key is absent (`+0x55c` defaults to `+0x560`).
        let threshold = threshold.unwrap_or(members.len() as u32);
        evaluators.push(Condition::InactiveMembers { members, threshold });
    }
    // Pass-2 order in the image is INACTIVE, DANGER_ZONES, ANIM_STATE
    // (`0x46a872`…`0x46a895`); the disjunction below is order-insensitive, so
    // the evaluator list keeps that reading order for a reader comparing the
    // two.
    if let Some(evaluator) = danger_evaluator {
        evaluators.push(evaluator);
    }
    if let Some(evaluator) = anim_evaluator {
        evaluators.push(evaluator);
    }

    Ok(match evaluators.len() {
        // No armed evaluator: the gate alone. Measured — "an objective with
        // no armed condition completes on the first tick it is awake", which
        // is a lifecycle state and never a `Const`.
        0 if gate.len() == 1 => gate.pop().expect("one gate"),
        0 => Condition::All(gate),
        // One evaluator: gate AND evaluator.
        1 => {
            gate.push(evaluators.pop().expect("one evaluator"));
            Condition::All(gate)
        }
        // Several: the original's "first true wins" is a disjunction behind
        // the same gate.
        _ => {
            gate.push(Condition::Any(evaluators));
            Condition::All(gate)
        }
    })
}

/// A refusal for a site whose argument list is not the shape the key spells.
fn arity_error(
    block: &str,
    directive: &BlockDirective,
    args: &[Value],
    wanted: &str,
) -> ConditionRefusal {
    ConditionRefusal::BadOperands {
        block: block.to_owned(),
        key: directive.key.clone(),
        detail: format!(
            "the measured shape is {wanted} and this site spells {}",
            if args.is_empty() {
                "an empty argument list".to_owned()
            } else {
                args.iter().map(value_label).collect::<Vec<_>>().join(", ")
            }
        ),
    }
}

/// The block's `DANGER_ZONES_COMPLETED` evaluator, armed the way the parse
/// arms it (`0x465ec0`, run **once per block**).
///
/// Both lookups that helper performs — the zone-name list and the
/// `DANGER_ZONES_COMPLETION_COUNT` beside it — are the record lookup
/// `0x57a090`, so each selects the **first** occurrence of its key in the
/// block's depth-first order and reads the record right after it. That gives
/// three outcomes:
///
/// * `None` when the block spells no `DANGER_ZONES_COMPLETED` text, and
///   `None` again when the selected site's own operand list is empty: either
///   way `+0x56c == 0`, the unarmed evaluator the fallthrough gate admits;
/// * a **refusal** when the selected site's follower is not a value list, a
///   listed name is not text, or — with zones armed — the selected threshold
///   site does not read as one int. The original then reads whatever follows
///   the matched text as the zone vector or as the flag-count payload, so no
///   predicate this build can state is measured for that record;
/// * `Some(condition)` otherwise: one flag byte per listed name, zeroed at
///   parse, counted nonzero against a threshold that defaults to the listed
///   count (measured in the engine image — see
///   [`crate::ir::Condition::DangerZoneFlags`]).
///
/// M07 spells three sites (blocks 37, 39, 41); the installation spells the
/// key in 31 blocks across seven readers (T463/T464).
fn danger_zones(
    block: &str,
    directives: &[BlockDirective],
) -> Result<Option<Condition>, ConditionRefusal> {
    let Some(site) = key_site(directives, DANGER_ZONES_KEY) else {
        return Ok(None);
    };
    let Some(operands) = site else {
        return Err(ConditionRefusal::NoConditionFor {
            block: block.to_owned(),
            key: DANGER_ZONES_KEY.to_owned(),
            detail: "the parse reads the record right after the key as this site's zone-name list \
                     and this site spelled no list after the key, so that read would take the \
                     record that follows as the flag array's names — a spelling no measured site \
                     uses, and offering one would be a guess at its predicate"
                .to_owned(),
        });
    };
    let mut zones = Vec::new();
    for operand in operands {
        let Value::Str(name) = operand else {
            return Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: DANGER_ZONES_KEY.to_owned(),
                detail: format!(
                    "the measured shape is a list of zone names, one per flag byte the parse \
                     strdups, and this site spells {}",
                    value_label(operand)
                ),
            });
        };
        zones.push(name.clone());
    }
    if zones.is_empty() {
        // `+0x56c == 0`: the evaluator answers false before it reads a flag
        // byte or the threshold, so no evaluator is armed.
        return Ok(None);
    }
    let required = match key_site(directives, DANGER_ZONES_COUNT_KEY) {
        // The measured default: `+0x568` falls back to the listed count.
        None => zones.len() as u32,
        Some(None) => {
            return Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: DANGER_ZONES_COUNT_KEY.to_owned(),
                detail: "the parse reads the record right after the key as the flag-count list \
                         and this site spelled no list after it, so what the evaluator would \
                         compare against is unreadable"
                    .to_owned(),
            });
        }
        Some(Some(counts)) => {
            let [Value::Int(count)] = counts else {
                return Err(ConditionRefusal::BadOperands {
                    block: block.to_owned(),
                    key: DANGER_ZONES_COUNT_KEY.to_owned(),
                    detail: format!(
                        "the measured shape is one int in the key's own list and this site \
                         spells {}",
                        if counts.is_empty() {
                            "an empty argument list".to_owned()
                        } else {
                            counts
                                .iter()
                                .map(value_label)
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    ),
                });
            };
            // Measured: the evaluator compares the nonzero flag count with
            // `setge`, so every threshold at or below 0 fires on the first
            // armed tick — which is exactly what a threshold of 0 says, and
            // a negative count is not a count this build stores.
            (*count).max(0) as u32
        }
    };
    Ok(Some(Condition::DangerZoneFlags { zones, required }))
}

/// The first `key` text in the block's depth-first order — the selection the
/// record lookup `0x57a090` applies — and the record immediately after it.
///
/// `None` when the block spells no such text; `Some(None)` when the text it
/// found is followed by no value list (the lookup hands the parse whatever
/// record comes next, list or not); `Some(Some(operands))` for the measured
/// `Text, List` pair.
fn key_site<'a>(directives: &'a [BlockDirective], key: &str) -> Option<Option<&'a [Value]>> {
    for directive in directives {
        if directive.key == key {
            return Some(match &directive.args {
                DirectiveArguments::List(operands) => Some(operands.as_slice()),
                DirectiveArguments::Bare | DirectiveArguments::NotAList(_) => None,
            });
        }
        if let DirectiveArguments::List(args) = &directive.args
            && let Some(site) = nested_key_site(args, key)
        {
            return Some(site);
        }
    }
    None
}

/// The first `key` text inside an operand list, searched depth-first — the
/// same first match the recursive half of `0x57a090` applies to a list child.
/// `Some(Some(operands))` when the text's own follower is a list (the
/// selected site); `Some(None)` when it is not; `None` when the list holds no
/// such text at all.
fn nested_key_site<'a>(items: &'a [Value], key: &str) -> Option<Option<&'a [Value]>> {
    for (index, item) in items.iter().enumerate() {
        match item {
            Value::Str(text) if text == key => {
                return Some(match items.get(index + 1) {
                    Some(Value::List(operands)) => Some(operands.as_slice()),
                    _ => None,
                });
            }
            Value::List(children) => {
                if let Some(found) = nested_key_site(children, key) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// One member chain: every argument is text, so `[base]` and
/// `[base, part, sub]` both stay the hierarchy the site spelled — the
/// node/part/part-state triple F39-E4 measured.
fn member_chain(block: &str, directive: &BlockDirective) -> Result<MemberName, ConditionRefusal> {
    let args = directive.operands(block)?;
    if args.is_empty() {
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: "the site spelled no name, and a member row names at least the base object"
                .to_owned(),
        });
    }
    if args.len() > MAX_VALUE_ITEMS {
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: format!(
                "the chain holds {} names, more than the IR's {}-operand bound",
                args.len(),
                MAX_VALUE_ITEMS
            ),
        });
    }
    let mut chain = MemberName::new();
    for arg in args {
        let Value::Str(name) = arg else {
            return Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: directive.key.clone(),
                detail: format!(
                    "the measured name chain is text and this site spells {}",
                    value_label(arg)
                ),
            });
        };
        chain.push(name.clone());
    }
    Ok(chain)
}

/// `DEDG [group, remaining, generator?]` → the group-depletion predicate, or
/// `None` when the site spells the record's **unarmed** spelling.
///
/// Measured: the evaluator runs only while `group > 0 && remaining >= 0`, and
/// an unarmed `DEDG` is exactly what the fallthrough gate's "no armed DEDG"
/// test admits — so such a block behaves as one with no evaluator and
/// completes when awake.
fn dedg(block: &str, directive: &BlockDirective) -> Result<Option<Condition>, ConditionRefusal> {
    let args = directive.operands(block)?;
    if !(2..=3).contains(&args.len()) {
        return Err(arity_error(
            block,
            directive,
            args,
            "two ints and an optional generator name",
        ));
    }
    let (Value::Int(group), Value::Int(remaining)) = (&args[0], &args[1]) else {
        return Err(arity_error(
            block,
            directive,
            args,
            "two ints and an optional generator name",
        ));
    };
    let generator = match args.get(2) {
        None => None,
        Some(Value::Str(name)) => Some(name.clone()),
        Some(other) => {
            return Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: directive.key.clone(),
                detail: format!(
                    "the optional third argument is the generator's name and this site spells {}",
                    value_label(other)
                ),
            });
        }
    };
    if *group <= 0 || *remaining < 0 {
        return Ok(None);
    }
    Ok(Some(Condition::EnemyGroupDepletion {
        group: *group,
        remaining: *remaining,
        generator,
    }))
}

/// `TRAVELERS [subject, polarity, anchor, radius, count?, delete?]` → the
/// radius predicate, or a refusal for a shape or a token no site spells.
///
/// Which of the original's two modes a site takes is decided by the site's
/// own spelling, not by the world: a string child0 resolves a **subject**, a
/// non-string child0 arms the counting mode. M01 spells child0 as text, so
/// its one site takes subject mode (measured against the installation). The
/// "subject carries the in-play bit" question the findings leave to the world
/// build is not part of the mode — it is a fact this predicate reads.
fn travelers(
    block: &str,
    directive: &BlockDirective,
) -> Result<Option<Condition>, ConditionRefusal> {
    let args = directive.operands(block)?;
    if !(4..=6).contains(&args.len()) {
        return Err(arity_error(
            block,
            directive,
            args,
            "`[text, text, text|list, float]` with an optional count and DELETE_ON_SUCCESS",
        ));
    }
    let Value::Str(subject) = &args[0] else {
        // A non-string subject arms the counting mode, whose cumulative
        // matching-member count is written into `+0x5b8` **during**
        // evaluation — a predicate with a write inside it, refused by name.
        return Err(ConditionRefusal::UnknownChangesPredicate {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: "a non-string subject arms TRAVELERS' counting mode, which accumulates its \
                     matching-member count into +0x5b8 during evaluation — a write, so no \
                     side-effect-free predicate can carry it"
                .to_owned(),
        });
    };
    let Value::Str(polarity) = &args[1] else {
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: format!(
                "the polarity token is text and this site spells {}",
                value_label(&args[1])
            ),
        });
    };
    if polarity != TRAVELERS_APPROACHING {
        return Err(ConditionRefusal::UnknownChangesPredicate {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: "the only measured polarity token is APPROACHING; the word for the other pole \
                     was never measured, so this site's inside/outside test is unknown"
                .to_owned(),
        });
    }
    let anchor = match &args[2] {
        Value::Str(name) => TravelersAnchor::Object(MemberName::from([name.clone()])),
        Value::List(point) => TravelersAnchor::Point(point_of(block, directive, point)?),
        other => {
            return Err(ConditionRefusal::BadOperands {
                block: block.to_owned(),
                key: directive.key.clone(),
                detail: format!(
                    "the anchor is a name or a three-number point and this site spells {}",
                    value_label(other)
                ),
            });
        }
    };
    let Value::Float(radius) = &args[3] else {
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: format!(
                "the radius is a float and this site spells {}",
                value_label(&args[3])
            ),
        });
    };
    if !radius.is_finite() {
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: format!("the radius {radius} is not finite"),
        });
    }
    if let Some(count) = args.get(4)
        && !matches!(count, Value::Int(_))
    {
        // Measured: child4 feeds the counting mode's required count and is
        // never read in subject mode — a spelled count does not change this
        // predicate, but a non-int one is still a shape no site spells.
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: format!(
                "the optional count is an int and this site spells {}",
                value_label(count)
            ),
        });
    }
    if let Some(delete) = args.get(5)
        && delete != &Value::Str("DELETE_ON_SUCCESS".to_owned())
    {
        return Err(ConditionRefusal::BadOperands {
            block: block.to_owned(),
            key: directive.key.clone(),
            detail: format!(
                "the only measured child5 spelling is DELETE_ON_SUCCESS and this site spells {}",
                value_label(delete)
            ),
        });
    }
    Ok(Some(Condition::Travelers {
        subject: MemberName::from([subject.clone()]),
        anchor,
        radius: *radius,
        // The token check above has already refused every spelling that is
        // not the measured inside pole, so this is never the unnamed
        // direction.
        approaching: true,
    }))
}

/// The three reals of an explicit `TRAVELERS` anchor point.
fn point_of(
    block: &str,
    directive: &BlockDirective,
    point: &[Value],
) -> Result<[f64; 3], ConditionRefusal> {
    let refuse = |detail: String| ConditionRefusal::BadOperands {
        block: block.to_owned(),
        key: directive.key.clone(),
        detail,
    };
    if point.len() != 3 {
        return Err(refuse(format!(
            "the measured point holds three numbers and this site spells {}",
            point.len()
        )));
    }
    let mut components = [0.0; 3];
    for (slot, value) in components.iter_mut().zip(point) {
        let number = match value {
            Value::Float(f) if f.is_finite() => *f,
            Value::Int(n) => f64::from(*n),
            Value::Float(f) => {
                return Err(refuse(format!("the point component {f} is not finite")));
            }
            other => return Err(refuse(format!("the point holds {}", value_label(other)))),
        };
        *slot = number;
    }
    Ok(components)
}

/// The operand list the block's one `ANIM_STATE` evaluator reads, or `None`
/// when the block's first `ANIM_STATE` text is followed by no list — the
/// measured lookup then finds no evaluator and the key contributes nothing.
///
/// Measured (finding C): the parse calls the helper `0x4691d0` once per
/// block, so the **first** `ANIM_STATE` text in the record's depth-first
/// order is the site — a top-level directive key or a text nested inside an
/// earlier directive's operand list — and the record immediately after it is
/// the operand list the descriptor walk consumes. A site after the first is
/// never reached.
fn anim_state_site(directives: &[BlockDirective]) -> Option<&[Value]> {
    // `Some(None)` — the first `ANIM_STATE` text is followed by no list —
    // collapses to "no evaluator armed", the measured reading; the danger-zones
    // site search keeps the same tri-state because its own reading refuses.
    key_site(directives, ANIM_STATE_KEY).flatten()
}

/// The `ANIM_STATE` operand list, walked the measured way (`0x4691d0`).
///
/// The walk reads the list's own children in order: a tag-3 `ANIM` followed
/// by a tag-4 spec record appends one `{name, state}` pair — `NAME` and
/// `STATE` are read out of the spec by the flat lookup `0x57a0f0` (first
/// match wins) — and the header's `required` counts one per appended pair.
/// The original appends only when the resolved handle **and** the state are
/// non-zero: a spec that yields no name text or a state token outside the
/// measured `RUNNING`/`EXECUTED`/`INVALID` vocabulary is dropped the same
/// way here, while a spelled name that resolves to no animation is kept —
/// the handle lookup is a runtime property of the world build, and the
/// evaluator treats an animation the facts do not carry as not in its wanted
/// state rather than guessing which pairs the original dropped. Every other
/// child is skipped.
///
/// After the walk the first `COMPLETION_COUNT` text inside the same list —
/// searched recursively — overwrites `required` when its follower resolves
/// an integer. Returns the appended pairs and that override; the caller
/// defaults `required` to the pair count.
fn anim_state(operands: &[Value]) -> (Vec<(String, AnimationState)>, Option<u32>) {
    let mut animations = Vec::new();
    let mut index = 0;
    while index < operands.len() {
        if matches!(&operands[index], Value::Str(tag) if tag == ANIM_STATE_TAG)
            && let Some(Value::List(spec)) = operands.get(index + 1)
        {
            if let (Some(name), Some(state)) = (spec_value(spec, "NAME"), spec_value(spec, "STATE"))
                && let Some(state) = AnimationState::from_token(state)
            {
                animations.push((name.to_owned(), state));
            }
            index += 2;
            continue;
        }
        index += 1;
    }
    (animations, completion_count(operands))
}

/// A spec record's `key` value the way the flat lookup `0x57a0f0` resolves
/// it: the first `key` text's follower — the string itself for a text
/// follower, or the list's first text child for a list follower. `None` for
/// any other spelling, which drops the pair the spec belongs to.
fn spec_value<'a>(spec: &'a [Value], key: &str) -> Option<&'a str> {
    let position = spec
        .iter()
        .position(|item| matches!(item, Value::Str(text) if text == key))?;
    match spec.get(position + 1)? {
        Value::Str(text) => Some(text),
        Value::List(items) => match items.first() {
            Some(Value::Str(text)) => Some(text),
            _ => None,
        },
        _ => None,
    }
}

/// The `COMPLETION_COUNT` override inside one operand list: the integer the
/// first `COMPLETION_COUNT` text's follower resolves, in the list's own
/// depth-first order. Returns `None` when the list holds no such text or the
/// first one's follower resolves no integer — the lookup `0x57a1b0` stops at
/// the first match either way, so no later occurrence is searched.
fn completion_count(operands: &[Value]) -> Option<u32> {
    completion_count_in(operands).flatten()
}

/// The tri-state `COMPLETION_COUNT` search: `Some(Some(count))` — a text
/// whose follower resolved `count`; `Some(None)` — a text whose follower
/// resolved no integer (the search stops at this first match);
/// `None` — no `COMPLETION_COUNT` text inside at all.
fn completion_count_in(items: &[Value]) -> Option<Option<u32>> {
    for (index, item) in items.iter().enumerate() {
        match item {
            Value::Str(text) if text == ANIM_STATE_COUNT_KEY => {
                return Some(items.get(index + 1).and_then(count_int));
            }
            Value::List(children) => {
                if let Some(found) = completion_count_in(children) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// The integer a `COMPLETION_COUNT` follower resolves: the int itself, or a
/// list's first int child — the two shapes `0x57a1b0` writes. A negative
/// count clamps to 0: the original's `matches >= required` test is then
/// always true, and a `required` of 0 carries the same truth.
fn count_int(value: &Value) -> Option<u32> {
    let int = match value {
        Value::Int(n) => *n,
        Value::List(items) => match items.first() {
            Some(Value::Int(n)) => *n,
            _ => return None,
        },
        _ => return None,
    };
    Some(int.max(0) as u32)
}
