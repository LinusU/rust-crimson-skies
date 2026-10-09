//! The `M01-LC-DIRECTIVE-LOWERING` adapter: the crossing between a measured
//! control record and `cs_script::bindings::RawProgram`.
//!
//! Task: `M01-LC-DIRECTIVE-LOWERING` (#717), stage `.03`. Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` ("Source adapter acceptance"). The
//! measurement vocabulary lives in [`cs_content::mission_control`]; this module
//! is the adapter the `call_arguments` and `objective_condition` lowering rows
//! are derived from.
//!
//! # What the adapter does
//!
//! [`lower_control_record`] walks the record's own decoded document the same
//! way [`cs_content::mission_control::measure_control_record`] did — the same
//! asymmetric directive grammar, so a site is a key plus the list beside it and
//! a bare key is the measured no-argument spelling — and produces, per numbered
//! `OBJECTIVE<N>` block:
//!
//! * one [`cs_script::conditions::RawBlock`] for the condition lowering, whose
//!   [`cs_script::conditions::lower_record`] verdict the attempt reports; and
//! * one [`RawObjective`] carrying that condition — `Lowered` conditions go in
//!   verbatim, a refusal or unreadable block goes in as
//!   `Condition::Unknown`, which `MissionProgram::validate` refuses rather than
//!   guesses — and every directive site as one [`RawCall`], args carried field
//!   for field inside `Value::List` (nested lists stay nested; nothing is
//!   flattened). A site of a **list-taking** directive (the measured
//!   operations that take one list of objective indices) carries its spelled
//!   list as *one* `Value::List` argument instead of a positional row, so the
//!   list's length is never read as an arity and a long list stays inside the
//!   registry's per-signature argument bound (`MAX_CALL_ARGS` is not raised).
//!
//! Calls are bound through a [`HostBindingRegistry`] built from the record's
//! own key dispositions: a `Measured` key registers one [`BindingSpec`] whose
//! signatures are exactly the shapes its sites were measured to spell (a key
//! whose sites disagree gets one signature per shape — none is chosen), a
//! `TerminalOutcome` key registers `Lowering::Finish` with the empty measured
//! signature, and an `Unmeasured` key registers **nothing**, so its sites
//! refuse `unknown host call` rather than bind to a convenient operation.
//!
//! # Fail-closed edges
//!
//! * A site beside a scalar (`not_a_list`), an argument the IR cannot carry
//!   (`u32` beyond `i32`, a non-finite float, a list deeper or wider than the
//!   `Value` bounds) produces no `RawCall` and is refused by block, key and
//!   child. A block holding such a site is *damaged*: its condition verdict is
//!   overridden to a refusal naming the site, because a predicate lowered over
//!   a partially-represented directive list is not the record's predicate.
//! * A block the walk cannot read stays `RawBlock::Unreadable` and its
//!   `RawObjective` carries `Condition::Unknown`, so validation refuses it.
//! * A record whose mission id did not resolve produces no `RawProgram` at
//!   all: `mission_identity` and `objective_identity` report the reason, and
//!   `call_arguments` still names every per-site verdict.
//! * `lower_program` produces no `MissionProgram` while any call fails to
//!   bind, and the result only counts as lowered when
//!   `MissionProgram::validate` also accepts it.

use cs_content::mission_control::{
    CallOutcome, ConditionOutcome, DirectiveDisposition, DirectiveOperation as MeasuredOperation,
    DirectiveShape, LoweringAttempt, MeasuredArg, MeasuredControlRecord, MeasuredDirectiveKey,
    TerminalOutcome,
};
use cs_content::objectives::objective_block_number;
use cs_content::stunts::{ZrdValue, objective_record, zrd_flat_fields};
use cs_script::bindings::{
    ArgDomain, BindingError, BindingProvenance, BindingSpec, CallSite, HostBindingRegistry,
    HostFamily, Lowering, RawCall, RawObjective, RawProgram, Repeatability, lower_program,
};
use cs_script::conditions::{
    BlockCondition, BlockDirective, BlockRefusal, DirectiveArguments,
    OBJECTIVE_CONDITION_REQUIREMENT, RawBlock, lower_record,
};
use cs_script::ir::{
    Condition, DirectiveOperation, MAX_VALUE_DEPTH, MAX_VALUE_ITEMS, MissionProgram, Outcome,
    SymbolId, Value,
};
use cs_types::content::ContentId;

/// What one attempt to lower a measured control record produced.
///
/// The [`LoweringAttempt`] inside is what [`MeasuredControlRecord::lowering`]
/// and [`MeasuredControlRecord::is_complete`] read — the rows are derived from
/// this attempt, so nothing here can mark a record complete that the lowering
/// refused. The assembled [`RawProgram`] and the bound [`MissionProgram`] are
/// kept beside it so a caller can inspect what the attempt emitted, not only
/// that it refused.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredControlRecord {
    /// The attempt as `cs_content` sees it: plain data the accounting reads.
    attempt: LoweringAttempt,
    /// The assembled program before binding — `Some` only when the record
    /// spelled a program the adapter could assemble (a mission id resolved and
    /// the blocks walked).
    raw: Option<RawProgram>,
    /// The bound program — `Some` only when every call bound. It is not yet
    /// validated; `attempt.validation` carries that verdict.
    program: Option<MissionProgram>,
    /// The registry the record's own key dispositions declared — kept so a
    /// caller can inspect which signatures a measured key was registered with.
    registry: HostBindingRegistry,
    /// Every per-site binding error the registry returned, in record order.
    binding_errors: Vec<BindingError>,
    /// How many bindings the registry carried for this record's vocabulary.
    bindings: usize,
}

impl LoweredControlRecord {
    /// The attempt the lowering rows are derived from.
    #[must_use]
    pub fn attempt(&self) -> &LoweringAttempt {
        &self.attempt
    }

    /// The assembled `RawProgram`, or `None` when the attempt assembled none.
    #[must_use]
    pub fn raw_program(&self) -> Option<&RawProgram> {
        self.raw.as_ref()
    }

    /// The bound `MissionProgram`, or `None` when any call refused. Validation
    /// is the attempt's own field (`attempt.validation`), not implied here.
    #[must_use]
    pub fn program(&self) -> Option<&MissionProgram> {
        self.program.as_ref()
    }

    /// The registry the attempt bound through: one spec per dispositioned
    /// key, each carrying one signature per measured shape.
    #[must_use]
    pub fn registry(&self) -> &HostBindingRegistry {
        &self.registry
    }

    /// The per-site binding errors, in record order — empty when every carried
    /// site bound.
    #[must_use]
    pub fn binding_errors(&self) -> &[BindingError] {
        &self.binding_errors
    }

    /// How many `BindingSpec`s the registry carried.
    #[must_use]
    pub fn bindings(&self) -> usize {
        self.bindings
    }
}

/// Lowers one measured control record: walks the numbered blocks of
/// `document` into `RawBlock`s, lowers each block's completion condition,
/// carries every directive site to a `RawCall`, binds the calls through a
/// registry built from `record`'s key dispositions, and assembles the
/// `RawProgram` — when `mission` resolved — for `lower_program`.
///
/// `mission` is the canonical mission `ContentId` the census derives from the
/// campaign layout, or the reason it could not; `mission_label` is the row's
/// `zbd/<group>/<mission>` label, used as the diagnostic mission name for
/// per-site refusals when no id resolved.
///
/// The record is the census's own measurement of `document`: the adapter
/// trusts `record`'s counts for accounting and re-walks `document` for the
/// values, because the record retains shapes rather than the spelled values.
/// The two walks share the measured grammar, and the acceptance suite pins
/// that the emitted counts equal `record.blocks()`/`record.sites()`.
#[must_use]
pub fn lower_control_record(
    mission: Result<ContentId, String>,
    mission_label: &str,
    document: &ZrdValue,
    record: &MeasuredControlRecord,
) -> LoweredControlRecord {
    let call_mission = mission
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|_| mission_label.to_owned());

    // The keys whose measured operation takes one list of indices: their
    // spelled list is one `Value::List` argument, not a positional row.
    let index_list_keys: Vec<&str> = record
        .keys()
        .iter()
        .filter(|key| takes_index_list(key))
        .map(|key| key.key.as_str())
        .collect();

    // ---- Walk the numbered blocks, exactly the measure grammar.
    let mut raw_blocks: Vec<RawBlock> = Vec::new();
    let mut block_calls: Vec<Vec<RawCall>> = Vec::new();
    let mut block_numbers: Vec<u32> = Vec::new();
    // One verdict slot per directive site, in record order: `Some(Refused)`
    // where the walk itself refused the site (a scalar follower or a value the
    // IR cannot carry), `None` where the emitted `RawCall` still owes a bind
    // verdict, which `site_map` locates.
    let mut site_outcomes: Vec<Option<CallOutcome>> = Vec::new();
    let mut site_map: Vec<(usize, u32, usize)> = Vec::new();
    // The first carry refusal per block, if any: a block whose site could not
    // be represented faithfully has no trustworthy lowered condition.
    let mut block_damage: Vec<Option<String>> = Vec::new();

    for (key, value) in zrd_flat_fields(objective_record(document)) {
        let Some(number) = objective_block_number(key) else {
            continue;
        };
        let block_index = raw_blocks.len() as u32;
        let Some(children) = value.as_list() else {
            raw_blocks.push(RawBlock::Unreadable(BlockRefusal::BlockNotAList {
                block: key.to_owned(),
            }));
            block_calls.push(Vec::new());
            block_numbers.push(number);
            block_damage.push(None);
            continue;
        };
        let mut directives = Vec::new();
        let mut calls = Vec::new();
        let mut damage: Option<String> = None;
        let mut unreadable = None;
        let mut index = 0usize;
        while index < children.len() {
            let Some(name) = children[index].as_text() else {
                unreadable = Some(BlockRefusal::KeyNotText {
                    block: key.to_owned(),
                    index,
                });
                break;
            };
            match children.get(index + 1) {
                Some(next) if next.as_list().is_some() => {
                    let items = next.as_list().unwrap_or_default();
                    match convert_args(items).map(|args| {
                        if index_list_keys.contains(&name) {
                            vec![Value::List(args)]
                        } else {
                            args
                        }
                    }) {
                        Ok(args) => {
                            site_map.push((site_outcomes.len(), block_index, calls.len()));
                            site_outcomes.push(None);
                            calls.push(RawCall {
                                name: name.to_owned(),
                                args: args.clone(),
                                span: None,
                            });
                            directives.push(BlockDirective {
                                key: name.to_owned(),
                                args: DirectiveArguments::List(args),
                            });
                        }
                        Err(reason) => {
                            let field = format!(
                                "{OBJECTIVE_CONDITION_REQUIREMENT}: `{key}` `{name}`: {reason}"
                            );
                            damage.get_or_insert(field.clone());
                            site_outcomes.push(Some(CallOutcome::Refused(field)));
                        }
                    }
                    index += 2;
                }
                None | Some(ZrdValue::Text(_)) => {
                    // The measured bare spelling: a text follower is the next
                    // directive's key, and no follower ends the block.
                    site_map.push((site_outcomes.len(), block_index, calls.len()));
                    site_outcomes.push(None);
                    calls.push(RawCall {
                        name: name.to_owned(),
                        args: Vec::new(),
                        span: None,
                    });
                    directives.push(BlockDirective {
                        key: name.to_owned(),
                        args: DirectiveArguments::Bare,
                    });
                    index += 1;
                }
                Some(scalar) => {
                    // `not_a_list`: a scalar sits beside the key. It is not an
                    // argument list, so no `RawCall` can carry it — the site is
                    // refused by name rather than flattened into one, and the
                    // block is damaged: its directive list is only partially
                    // represented, so no lowered condition is trusted either.
                    match zrd_to_value(scalar, 0) {
                        Ok(value) => {
                            directives.push(BlockDirective {
                                key: name.to_owned(),
                                args: DirectiveArguments::NotAList(value),
                            });
                            damage.get_or_insert(format!(
                                "{OBJECTIVE_CONDITION_REQUIREMENT}: `{key}` `{name}`: the site \
                                 spells a scalar beside its key (`not_a_list`), so the block's \
                                 directive list is only partially represented"
                            ));
                        }
                        Err(reason) => {
                            damage.get_or_insert(format!(
                                "{OBJECTIVE_CONDITION_REQUIREMENT}: `{key}` `{name}`: {reason}"
                            ));
                        }
                    }
                    site_outcomes.push(Some(CallOutcome::Refused(format!(
                        "{call_mission} objective#{block_index} `{name}`: the site spells a \
                         scalar beside its key (`not_a_list`), which no `RawCall` can carry"
                    ))));
                    index += 2;
                }
            }
        }
        let raw = match unreadable {
            Some(refusal) => RawBlock::Unreadable(refusal),
            None => RawBlock::Read {
                block: key.to_owned(),
                index: block_index,
                directives,
            },
        };
        raw_blocks.push(raw);
        block_calls.push(calls);
        block_numbers.push(number);
        block_damage.push(damage);
    }

    // ---- The registry: one spec per dispositioned key.
    let (registry, unbound_keys) = registry_for(record);

    // ---- Bind every carried site, in record order.
    let mut binding_errors = Vec::new();
    for (site, block_index, call_index) in &site_map {
        let call = &block_calls[*block_index as usize][*call_index];
        let at = CallSite {
            mission: call_mission.clone(),
            objective: SymbolId(*block_index),
            call: *call_index,
            span: call.span,
        };
        match registry.bind(call, &at) {
            Ok(_) => {
                site_outcomes[*site] = Some(CallOutcome::Bound);
            }
            Err(error) => {
                site_outcomes[*site] = Some(CallOutcome::Refused(error.to_string()));
                binding_errors.push(error);
            }
        }
    }
    let calls: Vec<CallOutcome> = site_outcomes
        .into_iter()
        .map(|outcome| outcome.expect("every spelled site got a verdict"))
        .collect();

    // ---- The per-block conditions.
    let lowered = lower_record(&raw_blocks);
    let mut conditions = Vec::with_capacity(raw_blocks.len());
    let mut objective_conditions: Vec<Condition> = Vec::with_capacity(raw_blocks.len());
    for (i, block_condition) in lowered.iter().enumerate() {
        let (outcome, condition) = match block_condition {
            BlockCondition::Lowered(condition) => match &block_damage[i] {
                None => (ConditionOutcome::Lowered, condition.clone()),
                Some(field) => (
                    ConditionOutcome::Refused(field.clone()),
                    Condition::Unknown {
                        instruction: field.clone(),
                    },
                ),
            },
            BlockCondition::Refused(refusal) => (
                ConditionOutcome::Refused(refusal.field()),
                Condition::Unknown {
                    instruction: refusal.field(),
                },
            ),
            BlockCondition::Unreadable(refusal) => (
                ConditionOutcome::Unreadable(format!(
                    "{OBJECTIVE_CONDITION_REQUIREMENT}: {refusal}"
                )),
                Condition::Unknown {
                    instruction: refusal.to_string(),
                },
            ),
        };
        conditions.push(outcome);
        objective_conditions.push(condition);
    }

    // ---- Assemble and lower the program, when a mission id resolved.
    let (raw, program, validation, program_refusal) = match &mission {
        Ok(id) => {
            let mut objectives = Vec::with_capacity(raw_blocks.len());
            let mut refusal = None;
            for (i, number) in block_numbers.iter().enumerate() {
                let content = match ContentId::from_source(
                    cs_types::content::ContentKind::Objective,
                    &format!("{}.objective{}", id.key(), number),
                ) {
                    Ok(content) => content,
                    // The key grammar is fixed and the parts are a campaign
                    // key plus digits, so a refusal here is a boundary bug —
                    // but it stays a refusal, never a fabricated id, and the
                    // program never assembles.
                    Err(error) => {
                        refusal = Some(format!(
                            "the objective id for `OBJECTIVE{number}` of `{id}` is not a valid \
                             `ContentId` key: {error}"
                        ));
                        break;
                    }
                };
                objectives.push(RawObjective {
                    id: SymbolId(i as u32),
                    content,
                    condition: objective_conditions[i].clone(),
                    calls: std::mem::take(&mut block_calls[i]),
                    span: None,
                });
            }
            if let Some(reason) = refusal {
                (None, None, None, Some(reason))
            } else {
                let raw = RawProgram {
                    mission: id.clone(),
                    variables: Vec::new(),
                    objectives,
                };
                let program = lower_program(&registry, raw.clone()).ok();
                let validation = program.as_ref().map(|program| {
                    program
                        .clone()
                        .validate()
                        .map(|_| Vec::new())
                        .unwrap_or_else(|error| vec![error.to_string()])
                });
                (Some(raw), program, validation, None)
            }
        }
        Err(_) => (None, None, None, None),
    };

    LoweredControlRecord {
        attempt: LoweringAttempt {
            mission: match (mission, program_refusal) {
                (Ok(id), None) => Ok(id.to_string()),
                (Ok(_), Some(reason)) | (Err(reason), _) => Err(reason),
            },
            objectives: raw.as_ref().map_or(0, |raw| raw.objectives.len() as u32),
            conditions,
            calls,
            unbound_keys,
            validation,
        },
        raw,
        program,
        bindings: registry.len(),
        registry,
        binding_errors,
    }
}

/// The `ArgDomain` one measured argument shape accepts: the shape's own type,
/// unbounded within it (the measurement records no range — a bound here would
/// be invented).
fn arg_domain(arg: &MeasuredArg) -> ArgDomain {
    match arg {
        MeasuredArg::Int => ArgDomain::IntRange {
            min: i32::MIN,
            max: i32::MAX,
        },
        MeasuredArg::Float => ArgDomain::FloatRange {
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
        },
        MeasuredArg::Text => ArgDomain::Str {
            max_bytes: usize::MAX,
        },
        MeasuredArg::Empty => ArgDomain::List(Vec::new()),
        MeasuredArg::List(children) => ArgDomain::List(children.iter().map(arg_domain).collect()),
    }
}

/// Whether a key's measured operation takes **one list of objective indices**
/// (`DirectiveOperation::WakeObjectives`, `SleepObjectives`, `KillObjectives`,
/// `WakeObjectivesOnTransition`): the list spelled beside the key is that one
/// argument, so its length is the list's, not an arity. Carrying it as one
/// `Value::List` keeps a long list (M02's nine-index kill sites) inside the
/// host-call bound without raising it. A bare site has no list and carries
/// none.
fn takes_index_list(key: &MeasuredDirectiveKey) -> bool {
    matches!(
        key.disposition(),
        DirectiveDisposition::Measured(directive) if matches!(
            directive.operation,
            MeasuredOperation::WakeObjectives
                | MeasuredOperation::SleepObjectives
                | MeasuredOperation::KillObjectives
                | MeasuredOperation::WakeObjectivesOnTransition
        )
    )
}

/// The signatures one key's measured shapes accept: one per distinct spelled
/// shape — a `Bare` site accepts no arguments, a `not_a_list` site no
/// signature covers, and equal shapes contribute one signature.
fn signatures_for(key: &MeasuredDirectiveKey) -> Vec<Vec<ArgDomain>> {
    let mut signatures: Vec<Vec<ArgDomain>> = Vec::new();
    for (shape, _) in &key.shapes {
        let signature = match shape {
            DirectiveShape::Bare => Vec::new(),
            DirectiveShape::Arguments(args) if takes_index_list(key) => {
                vec![ArgDomain::List(args.iter().map(arg_domain).collect())]
            }
            DirectiveShape::Arguments(args) => args.iter().map(arg_domain).collect(),
            DirectiveShape::NotAList => continue,
        };
        if !signatures.contains(&signature) {
            signatures.push(signature);
        }
    }
    signatures
}

/// The registry the record's own key dispositions declare.
///
/// A `Measured` key registers a `Lowering::Directive` spec whose signatures
/// are exactly the shapes the census measured its sites spelling; a
/// `TerminalOutcome` key registers `Lowering::Finish` with the measured empty
/// signature; an `Unmeasured` key registers nothing — its sites refuse
/// `unknown host call` rather than bind to a convenient operation (contract
/// "Host interface").
///
/// The cs_content operation reaches `cs_script::ir::DirectiveOperation`
/// through the shared operation codes — the wire vocabulary both sides
/// publish, which the acceptance suite cross-checks — so a measured operation
/// this build does not mirror is refused by name rather than bound to a
/// different one.
///
/// Returns the registry and the keys registration refused, each with its
/// reason.
fn registry_for(record: &MeasuredControlRecord) -> (HostBindingRegistry, Vec<String>) {
    let mut registry = HostBindingRegistry::new();
    let mut unbound = Vec::new();
    for key in record.keys() {
        let spec = match key.disposition() {
            DirectiveDisposition::TerminalOutcome { outcome } => {
                let outcome = match outcome {
                    TerminalOutcome::Succeeded => Outcome::Succeeded,
                    TerminalOutcome::Failed => Outcome::Failed,
                };
                BindingSpec {
                    name: key.key.clone(),
                    family: HostFamily::MissionState,
                    // The measured spelling is bare; a site that wrote
                    // arguments beside the key fits no signature and refuses.
                    signatures: vec![Vec::new()],
                    lowering: Lowering::Finish(outcome),
                    repeatability: Repeatability::Once,
                    provenance: BindingProvenance::Observed {
                        evidence: "the record's own bare spelling, read by \
                                   cs_content::mission_control::terminal_outcome_of"
                            .to_owned(),
                    },
                }
            }
            DirectiveDisposition::Measured(directive) => {
                let Some(operation) = DirectiveOperation::from_code(directive.operation.code())
                else {
                    unbound.push(format!(
                        "`{}`: the measured operation `{}` has no \
                         `cs_script::ir::DirectiveOperation` counterpart",
                        key.key,
                        directive.operation.code()
                    ));
                    continue;
                };
                let signatures = signatures_for(key);
                if signatures.is_empty() {
                    unbound.push(format!(
                        "`{}`: no measured site shape produces a call signature",
                        key.key
                    ));
                    continue;
                }
                BindingSpec {
                    name: key.key.clone(),
                    family: HostFamily::MissionState,
                    signatures,
                    lowering: Lowering::Directive(operation),
                    repeatability: Repeatability::Once,
                    provenance: BindingProvenance::Observed {
                        evidence: directive.evidence.join(", "),
                    },
                }
            }
            DirectiveDisposition::Unmeasured { .. } => continue,
        };
        if let Err(error) = registry.register(spec) {
            unbound.push(format!("`{}`: {error}", key.key));
        }
    }
    (registry, unbound)
}

/// One `.zrd` argument node as the IR `Value` it carries.
///
/// `depth` counts the enclosing `Value::List`s, matching `Value::check`'s
/// convention: a top-level argument converts at depth 0. `Err` is a refusal —
/// the reason names what the node spelled — never a coerced value.
fn zrd_to_value(value: &ZrdValue, depth: usize) -> Result<Value, String> {
    match value {
        ZrdValue::Int(v) => i32::try_from(*v)
            .map(Value::Int)
            .map_err(|_| format!("int {v} does not fit the checked 32-bit value")),
        ZrdValue::Float(f) => {
            let f = f64::from(*f);
            if f.is_finite() {
                Ok(Value::Float(f))
            } else {
                Err(format!("float {f} is not finite"))
            }
        }
        ZrdValue::Text(text) => Ok(Value::Str(text.clone())),
        ZrdValue::List(children) => {
            if depth >= MAX_VALUE_DEPTH {
                return Err(format!("a list nested deeper than {MAX_VALUE_DEPTH}"));
            }
            if children.len() > MAX_VALUE_ITEMS {
                return Err(format!(
                    "a list of {} items exceeds {MAX_VALUE_ITEMS}",
                    children.len()
                ));
            }
            children
                .iter()
                .map(|child| zrd_to_value(child, depth + 1))
                .collect::<Result<Vec<Value>, String>>()
                .map(Value::List)
        }
    }
}

/// A site's argument list as IR `Value`s: the `Value::List` items in order,
/// bounded like one argument node per item.
fn convert_args(children: &[ZrdValue]) -> Result<Vec<Value>, String> {
    if children.len() > MAX_VALUE_ITEMS {
        return Err(format!(
            "an argument list of {} items exceeds {MAX_VALUE_ITEMS}",
            children.len()
        ));
    }
    children
        .iter()
        .map(|child| zrd_to_value(child, 0))
        .collect()
}
