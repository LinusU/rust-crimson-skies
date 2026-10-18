//! The executor for the wake/sleep/kill/nap directive family (tasks
//! `M02-B-FU4`, Rally #807, and `M02-B-FU5`, Rally #1586).
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md` ("Objective event
//! ordering": queued, no reentrancy). The measurement is
//! `docs/findings/2026-10-09-m02-b-fu3-out-of-range-wake-address.md` (the
//! address rule and the wake walk's 15-entry window) and
//! `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`
//! (the completion pipeline's order). Nothing here is `verified_original`.
//!
//! A [`DirectiveEmission`](cs_script::runtime::DirectiveEmission) of the family
//! carries addresses *as the record spelled them*. This module decodes each one
//! the way the original's parse stores it and resolves every address through
//! [`resolve_objective_address`], so [`OUT_OF_RANGE_OBJECTIVE_ADDRESS`] has a
//! live call site:
//!
//! * **Lists** (`WakeObjectives`, `SleepObjectives`, `KillObjectives`,
//!   `WakeObjectivesOnTransition`) arrive as the one `Value::List` the lowering
//!   carries. The parse's store loop decrements and writes every spelled
//!   child's payload into the record's 15-dword list field and appends a
//!   `−1` terminator one slot past the last, so an authored `0` stores the
//!   `−1` the walk stops on: it ends the walk and the children after it
//!   are never reached, which is not a refusal — but they are still
//!   *stored*, so [`ENGINE_LIST_WINDOW`] or more spelled children leave
//!   the appended terminator no slot, the shape `TargetList::from_record`
//!   refuses, and the whole list gets [`RuntimeError::DirectiveListTooLong`],
//!   never a truncated walk.
//!   Any other address outside `[1, objectives]` — including a negative
//!   one, which decodes below the terminator — is refused by name.
//! * **`NapObjective`** spells `[address, seconds]`; a missing or non-numeric
//!   `seconds` is refused (the original logs "has no nap_time specified").
//! * **`WakeObjectivesOnTransition`, `DependencyGate`, `HideObjective`** feed
//!   the same index arrays, so their addresses are checked by the same rule,
//!   but the lifecycle has no consumer for them yet. Each is reported as
//!   [`RuntimeError::UnexecutedDirective`] and moves nothing — never dropped
//!   in silence, never approximated by another effect — because its measured
//!   trigger is machinery this runtime deliberately does not carry:
//!   `WakeObjectivesOnTransition` (`WAKE_OBJECTIVE_WHEN_I_SLEEP`, `+0xdc`)
//!   fires only on the source's *timer-driven* nap and sleep transitions,
//!   never on `NAP`/`SLEEP` completion effects, and this runtime keeps no
//!   dormant/nap timers; `DependencyGate` (`TICK_DEPENDS_ON_OBJ`, `+0x10`)
//!   gates the *dependent's own* evaluation — no timers, no conditions —
//!   on the dependency being awake, and this runtime evaluates no
//!   per-objective conditions a gate could hold; and `HideObjective`
//!   (`HIDE_OBJ`, `+0xd8`) marks the named objective completed only when
//!   the woken block's prior state was **dormant**, a state
//!   [`ObjectiveState`](super::state::ObjectiveState) deliberately does not
//!   have, so the guard cannot be evaluated here and firing it on every
//!   wake the emission reports would complete objectives the engine never
//!   touches. The record world runs all three through
//!   `cs_content::objectives_engine`, which holds the dormant timers and
//!   the gate.
//!
//! Each outcome is a [`Planned`]: an effect the runtime queues for its phase 7,
//! or the refusal it reports there as
//! [`ObjectiveEventKind::RequestRefused`](super::runtime::ObjectiveEventKind).

use cs_script::ir::{DirectiveOperation, SymbolId, Value};

use super::address::resolve_objective_address;
use super::runtime::{CompletionEffect, CompletionEffectKind, NapTime, RuntimeError};

/// The engine's per-objective list window: the slots its `−1`-terminated
/// target arrays hold, **terminator included** — the `cmpl $0xf` cap the
/// wake walk at `0x469af0` carries (`cs_content::objectives_engine`
/// publishes the same number as `ENGINE_LIST_CAPACITY`). A spelled list
/// of this many children leaves no slot for its terminator and is
/// refused whole rather than walked.
const ENGINE_LIST_WINDOW: usize = 15;

/// One decoded outcome of a family directive, in list order.
pub type Planned = Result<CompletionEffect, RuntimeError>;

/// Whether `operation` belongs to the family this executor owns.
#[must_use]
pub const fn is_family(operation: DirectiveOperation) -> bool {
    matches!(
        operation,
        DirectiveOperation::WakeObjectives
            | DirectiveOperation::SleepObjectives
            | DirectiveOperation::KillObjectives
            | DirectiveOperation::NapObjective
            | DirectiveOperation::WakeObjectivesOnTransition
            | DirectiveOperation::DependencyGate
            | DirectiveOperation::HideObjective
    )
}

/// The engine's fixed application rank of a family operation: the completion
/// pipeline applies `WAKE`, `NAP`, `KILL`, `SLEEP` in that order
/// ([`CompletionEffectKind::engine_order`]); the operations with no consumer
/// follow.
#[must_use]
pub const fn rank(operation: DirectiveOperation) -> u8 {
    match operation {
        DirectiveOperation::WakeObjectives => CompletionEffectKind::Wake.engine_order(),
        DirectiveOperation::NapObjective => CompletionEffectKind::Nap.engine_order(),
        DirectiveOperation::KillObjectives => CompletionEffectKind::Kill.engine_order(),
        DirectiveOperation::SleepObjectives => CompletionEffectKind::Sleep.engine_order(),
        _ => 4,
    }
}

/// Decodes one family emission against a record of `objectives` blocks.
///
/// Returns nothing for an operation outside the family. Every address goes
/// through [`resolve_objective_address`]; nothing is clamped.
#[must_use]
pub fn plan(operation: DirectiveOperation, args: &[Value], objectives: u32) -> Vec<Planned> {
    let kind = match operation {
        DirectiveOperation::WakeObjectives => Some(CompletionEffectKind::Wake),
        DirectiveOperation::SleepObjectives => Some(CompletionEffectKind::Sleep),
        DirectiveOperation::KillObjectives => Some(CompletionEffectKind::Kill),
        _ => None,
    };
    if let Some(kind) = kind {
        return plan_list(operation, args, objectives, |target| {
            CompletionEffect::new(kind, target, None)
        });
    }
    match operation {
        DirectiveOperation::NapObjective => plan_nap(args, objectives),
        DirectiveOperation::WakeObjectivesOnTransition => {
            // Only the address refusals are kept: a resolved address would
            // have woken something, and this operation wakes nothing yet.
            let mut out = plan_list(operation, args, objectives, |target| {
                CompletionEffect::new(CompletionEffectKind::Wake, target, None)
            });
            out.retain(Result::is_err);
            out.push(Err(RuntimeError::UnexecutedDirective { operation }));
            out
        }
        DirectiveOperation::DependencyGate | DirectiveOperation::HideObjective => {
            let mut out = Vec::new();
            for value in args {
                match value {
                    Value::Int(address) => {
                        if let Err(refusal) =
                            resolve_objective_address(i64::from(*address), objectives)
                        {
                            out.push(Err(refusal.into()));
                        }
                    }
                    _ => out.push(Err(RuntimeError::MalformedDirective { operation })),
                }
            }
            out.push(Err(RuntimeError::UnexecutedDirective { operation }));
            out
        }
        _ => Vec::new(),
    }
}

/// A `−1`-terminated list of addresses: the lowering carries the spelled list
/// as the call's one `Value::List`.
fn plan_list(
    operation: DirectiveOperation,
    args: &[Value],
    objectives: u32,
    make: impl Fn(SymbolId) -> Planned,
) -> Vec<Planned> {
    let [Value::List(children)] = args else {
        return vec![Err(RuntimeError::MalformedDirective { operation })];
    };
    // The parse's store loop is tag-blind: it decrements and writes
    // **every** spelled child's payload into the record's 15-dword list
    // field — a spelled `0` stores the `−1` the walk later stops on but
    // still occupies its slot, and the children after it are stored too,
    // however dead they are to the walk — then appends its own `−1`
    // terminator one slot past the last child. [`ENGINE_LIST_WINDOW`]
    // stored children leave that terminator nowhere (it lands in the
    // next field's first slot), and the record reader refuses the same
    // shape (`EngineEffectError::ListTooLong` counts the whole spelled
    // list, not the walked prefix). The whole list is refused — the
    // walk's `cmpl $0xf` cap is never reached with a stored list, so
    // modelling a truncated walk would invent behaviour no record can
    // produce.
    if children.len() >= ENGINE_LIST_WINDOW {
        return vec![Err(RuntimeError::DirectiveListTooLong {
            operation,
            len: children.len(),
        })];
    }
    let mut out = Vec::new();
    for child in children {
        let Value::Int(address) = child else {
            out.push(Err(RuntimeError::MalformedDirective { operation }));
            break;
        };
        // The parse stores `address − 1` and terminates the array with `−1`, so
        // an authored `0` ends the walk before the children that follow it.
        if *address == 0 {
            break;
        }
        out.push(
            match resolve_objective_address(i64::from(*address), objectives) {
                Ok(target) => make(target),
                Err(refusal) => Err(refusal.into()),
            },
        );
    }
    out
}

/// `NAP_OBJECTIVE_WHEN_I_COMPLETE [address, seconds]`.
fn plan_nap(args: &[Value], objectives: u32) -> Vec<Planned> {
    let operation = DirectiveOperation::NapObjective;
    let [Value::Int(address), rest @ ..] = args else {
        return vec![Err(RuntimeError::MalformedDirective { operation })];
    };
    let target = match resolve_objective_address(i64::from(*address), objectives) {
        Ok(target) => target,
        Err(refusal) => return vec![Err(refusal.into())],
    };
    let seconds = match rest {
        [Value::Int(seconds)] => Some(f64::from(*seconds)),
        [Value::Float(seconds)] => Some(*seconds),
        _ => None,
    };
    let time = seconds.and_then(NapTime::new);
    vec![CompletionEffect::new(
        CompletionEffectKind::Nap,
        target,
        time,
    )]
}
