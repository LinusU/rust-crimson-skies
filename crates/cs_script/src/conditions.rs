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
//!   holds one slot per kind and overwrites it at parse — except
//!   `ANIM_STATE`, whose header appends every pair and counts them all into
//!   `required`, which [`lower_block_condition`] accumulates into one
//!   condition;
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

/// The two measured spec keys inside an `ANIM_STATE` descriptor (finding C).
const ANIM_STATE_SPEC_KEYS: [&str; 2] = ["NAME", "STATE"];

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
    // disjunction the original cannot produce. `ANIM_STATE` is the measured
    // exception — its header *appends* every pair and counts them into
    // `required` — so its pairs accumulate below.
    let mut dedg_spelled = false;
    let mut travelers_spelled = false;
    let mut animations: Vec<(String, AnimationState)> = Vec::new();

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
            // Measured (finding C): every `ANIM_STATE` directive appends its
            // pairs to the block's single `{required, count, records}` header
            // and increments `required` once per appended pair, so several
            // directives are **one** evaluator whose required count is the
            // total pair count — not a disjunction of per-directive tests.
            "ANIM_STATE" => animations.extend(anim_state(block, directive)?),
            "DANGER_ZONES_COMPLETED" | "DANGER_ZONES_COMPLETION_COUNT" => {
                return Err(ConditionRefusal::NoConditionFor {
                    block: block.to_owned(),
                    key: key.to_owned(),
                    detail: "the danger-zones flag evaluator is measured but this build lowers no \
                             condition for it, and offering one would be a guess at its predicate"
                        .to_owned(),
                });
            }
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
            "COMPLETION_COUNT" => {
                return Err(ConditionRefusal::UnknownChangesPredicate {
                    block: block.to_owned(),
                    key: key.to_owned(),
                    detail: "ANIM_STATE's required count is overwritten by a sibling \
                             COMPLETION_COUNT (finding C), so the count this block would test is \
                             not the one the record spells"
                        .to_owned(),
                });
            }
            // Everything else is a completion, wake or transition effect, an
            // outcome class or presentation data: measured stages of the
            // pipeline, and none of them is a predicate — finding B's pass-2
            // evaluator list is the complete one.
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
    if !animations.is_empty() {
        // Measured (finding C): `required` is the number of appended pairs,
        // counted across every `ANIM_STATE` directive of the block.
        evaluators.push(Condition::AnimationStates {
            required: animations.len() as u32,
            animations,
        });
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

/// `ANIM_STATE ["ANIM", ["NAME", [name], "STATE", [token]]]` → the animation
/// pairs this directive **appends** to the block's one header.
///
/// Measured (finding C): the descriptor is a tag-3 `ANIM` followed by a spec
/// record; `NAME` and `STATE` are read out of it, the state token maps
/// `RUNNING`/`EXECUTED`/`INVALID` to 2/3/4, and the header's `required` is
/// incremented once per appended pair — across every `ANIM_STATE` directive of
/// the block. Every M01 site spells exactly one pair; the
/// `COMPLETION_COUNT` sibling that could overwrite `required` is refused by
/// its own key.
fn anim_state(
    block: &str,
    directive: &BlockDirective,
) -> Result<Vec<(String, AnimationState)>, ConditionRefusal> {
    let args = directive.operands(block)?;
    let refuse = |detail: String| ConditionRefusal::BadOperands {
        block: block.to_owned(),
        key: directive.key.clone(),
        detail,
    };
    if args.len() != 2 {
        return Err(arity_error(
            block,
            directive,
            args,
            "the tag `ANIM` and one spec record",
        ));
    }
    match &args[0] {
        Value::Str(tag) if tag == ANIM_STATE_TAG => {}
        Value::Str(tag) => {
            return Err(refuse(format!(
                "the only measured descriptor tag is `{ANIM_STATE_TAG}` and this site spells {tag:?}"
            )));
        }
        other => {
            return Err(refuse(format!(
                "the descriptor tag is the text `{ANIM_STATE_TAG}` and this site spells {}",
                value_label(other)
            )));
        }
    }
    let Value::List(spec) = &args[1] else {
        return Err(refuse(format!(
            "the spec is a list and this site spells {}",
            value_label(&args[1])
        )));
    };
    if spec.is_empty() || spec.len() % 2 != 0 {
        return Err(refuse(format!(
            "the spec holds {} element(s); the measured shape is key/value pairs",
            spec.len()
        )));
    }
    let mut name: Option<String> = None;
    let mut state: Option<AnimationState> = None;
    for pair in spec.chunks(2) {
        let Some(Value::Str(spec_key)) = pair.first() else {
            return Err(refuse("the spec key is not text".to_owned()));
        };
        let Some(value) = pair.get(1) else {
            return Err(refuse("the spec ends mid-pair".to_owned()));
        };
        let Value::List(wrapped) = value else {
            return Err(refuse(format!(
                "the spec value is a one-element list and this site spells {}",
                value_label(value)
            )));
        };
        let Some(Value::Str(text)) = wrapped.first() else {
            return Err(refuse("the spec value holds no text".to_owned()));
        };
        if wrapped.len() != 1 {
            return Err(refuse(format!(
                "the spec value holds {} elements; the measured shape is one",
                wrapped.len()
            )));
        }
        if spec_key == "NAME" {
            if name.is_some() {
                return Err(refuse("the spec spells NAME twice".to_owned()));
            }
            name = Some(text.clone());
        } else if spec_key == "STATE" {
            if state.is_some() {
                return Err(refuse("the spec spells STATE twice".to_owned()));
            }
            let Some(mapped) = AnimationState::from_token(text) else {
                return Err(ConditionRefusal::UnknownChangesPredicate {
                    block: block.to_owned(),
                    key: directive.key.clone(),
                    detail: "the parser maps only RUNNING, EXECUTED and INVALID to a state and \
                             drops every other token; a token outside that vocabulary has no \
                             measured meaning here"
                        .to_owned(),
                });
            };
            state = Some(mapped);
        } else {
            return Err(refuse(format!(
                "the spec key {spec_key:?} is outside the measured {:?} vocabulary",
                ANIM_STATE_SPEC_KEYS
            )));
        }
    }
    let (Some(name), Some(state)) = (name, state) else {
        return Err(refuse(
            "the spec does not spell both a NAME and a STATE".to_owned(),
        ));
    };
    Ok(vec![(name, state)])
}
