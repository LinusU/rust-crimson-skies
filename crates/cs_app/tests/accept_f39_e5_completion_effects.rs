//! Acceptance suite F39-E5: the declared objective schema carries a
//! completion-effect vocabulary, the runtime applies it, the one conflict the
//! corpus contains is decided by the engine's measured order, and the shape
//! that stays unmeasured is refused by name.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (the sheet has no `### F39-E5` section, so the stage takes the `F39-E` plan
//! the sheet's stage list was resequenced into and the prefix is stated here as
//! the sheet states a prefix per stage). Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md` — "Objective event ordering": *"Actions do
//! not directly recurse into callbacks. Maintain ordered queues and define when
//! a new event is eligible for observation. Stable ordering keys use
//! session/tick/source/program sequence."*
//!
//! # What the original declares, and what this suite may claim
//!
//! F39-D counted the declaration sites of `WAKE_OBJECTIVE_WHEN_I_COMPLETE`
//! (412), `NAP_OBJECTIVE_WHEN_I_COMPLETE` (417), `KILL_OBJECTIVE_WHEN_I_COMPLETE`
//! (225) and `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` (2) and left what they do
//! unmeasured; F39-E2 read them **per block** and measured that all 1706 targets
//! name an objective of the *same* record, that every `NAP` site carries one
//! extra number whose unit is unmeasured, and that exactly **one** block in 1338
//! declares two different effects for the same objective
//! (`zbd/c3/m05` `OBJECTIVE8`, naming objective 68) without anything in the
//! **files** saying which of them wins — the engine image later supplied that
//! answer (its fixed order decides), and the schema now accepts the shape.
//!
//! So the *spellings*, the closure over the record and the isolated conflict
//! were measured; **that a spelling means "wakes that objective when this one
//! completes" was an inference from the spelling** — and M01-B-OBJ-1 then read
//! the engine image and measured what each effect does (fixed `WAKE` → `NAP`
//! → `KILL` → `SLEEP` order, a guarded wake, a nap a timed pause, a kill a
//! removal). This suite's runtime assertions now pin those measured moves
//! through
//! [`cs_sim::objectives::runtime::CompletionEffectKind::moves_to`]; the
//! alignment and its boundaries are recorded in
//! `docs/findings/2026-10-12-m01-b-obj-1-fu1-runtime-effect-alignment.md`.
//! Nothing here is an original-fidelity claim, and no original executable has
//! been run: the gate test at the end is the one that keeps that true.
//!
//! # What each test drives
//!
//! * `accept_f39_e5_a_completion_wakes_the_objectives_it_names` — the production
//!   path end to end: a declared program lowers, an ordinary deadline expiry
//!   completes one objective, and the four effects it declares apply in the
//!   engine's measured order on the same tick, under the completing
//!   objective's own event key: the wake and the wakeup move their targets,
//!   the nap moves its target to `Napping`, and the kill removes its target
//!   without moving its state.
//! * `accept_f39_e5_effects_apply_on_the_completion_tick_and_only_once` — the
//!   eligibility rule and the latch: the moves land on the completion's tick (not
//!   the next one), and a deadline cannot fire its effects a second time.
//! * `accept_f39_e5_a_napped_objective_is_set_aside_whatever_the_number_says` —
//!   the measured nap time with an unmeasured unit: two programs whose nap
//!   numbers are `0.5` and `170` produce identical streams, and the value
//!   crosses the lowering boundary as the declared time data, never converted
//!   into this engine's ticks.
//! * `accept_f39_e5_an_effect_on_a_hidden_objective_is_reported_not_dropped` —
//!   reveal rules still govern visibility (F39 non-negotiable behavior 5): an
//!   effect naming an objective the player has not been shown is refused *and
//!   reported*, so "the program asked" stays distinguishable from "nothing
//!   happened".
//! * `accept_f39_e5_one_declaration_two_effects_on_one_target_is_accepted` — the
//!   corpus's one measured conflict in declared form: one declaration carrying
//!   a `WAKE` and a `NAP` on one target is accepted in **both** authored
//!   orders, kept as spelled, because the engine's measured fixed order decides
//!   it — and the runtime registers the shape and applies the engine's order.
//! * `accept_f39_e5_two_completions_naming_one_objective_are_refused_by_name` —
//!   the schema-level refusal that stays, in **both** authored orders — two
//!   *different* declaring objectives naming different effects for one target,
//!   which the engine's one-completion-per-pass rule keeps undecided — with the
//!   neighbour that stays legal (two objectives agreeing on the same effect) so
//!   the refusal cannot widen into "no two effects at all"; and the runtime's
//!   own refusal of the same shape built without the declared schema, in either
//!   registration order.
//! * `accept_f39_e5_dead_completion_effects_are_refused_by_name` — the four dead
//!   shapes and the number-shape rule, each with its own named error, plus the
//!   declaration the rule must *not* refuse: an objective born finished that
//!   declares no effects at all.
//! * `accept_f39_e5_a_tick_over_its_event_budget_is_stopped_before_any_effect_applies`
//!   — the effects a tick is about to queue are counted against
//!   `max_events_per_tick`, so a tick that would exceed it is refused whole
//!   instead of applying the completion and leaving four moves to arrive later.
//! * `accept_f39_e5_the_effect_vocabulary_is_the_measured_one` — the four
//!   spellings are the measured ones, `WAKE` and `WAKEUP` never collapse, only a
//!   nap carries a number, and no effect kind completes an objective (which is
//!   what makes the effect queue non-cascading).
//! * `accept_f39_e5_an_original_record_with_effects_is_never_played` — the gate.
//!   The same declarations over an installation origin stay unplayable and are
//!   refused by `lower_program` with the record's own reason: a way to *say*
//!   what a record declares is not recovering what it means.
//!
//! Every value the non-retail tests use is newly authored synthetic fixture
//! data, never original game data.

use cs_app::objectives::{ObjectiveSession, ProgramLowerError, lower_program};
use cs_content::objectives::{
    BRANCH_EFFECT_KEY_VOCABULARY, BRANCH_KEY_VOCABULARY, BranchEffectKind,
    DeclaredCompletionEffect, DeclaredObjective, DeclaredObjectiveProgram, DeclaredObjectiveState,
    DeclaredRevealRule, DeclaredSupport, ObjectivesSchemaError, ProgramSymbol,
    SYNTHETIC_E5_COMPLETING, SYNTHETIC_E5_NAPPED, SYNTHETIC_E5_WOKEN,
    UNMEASURED_BLOCK_PRECEDENCE, UNMEASURED_NAP_ARGUMENT, UNMEASURED_OBJECTIVE_SEMANTICS,
    declared_synthetic_completion_effects,
};
use cs_script::ir::SymbolId;
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::runtime::{
    CompletionEffect, CompletionEffectKind, NapTime, ObjectiveCompletion, ObjectiveEventKind,
    ObjectiveRuntime, ObjectiveSpec, RevealRule, RuntimeError, RuntimeLimits, StopReason,
    TickInput,
};
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::TerminalPrecedence;
use cs_sim::objectives::timer::TimerRequest;
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Origin};
use cs_types::evidence::ContentHash;

const GEN: SessionGeneration = SessionGeneration(7);

fn input<'a>(tick: u64, committed: u64) -> TickInput<'a> {
    TickInput {
        tick: Tick(tick),
        committed_ticks: committed,
        lifecycles: &[],
        movements: &[],
        signals: &[],
        timer_requests: &[],
        objective_requests: &[],
        terminal_requests: &[],
    }
}

/// Arms the program's deadline on `tick` and commits one whole tick, which is
/// the ordinary declared way the fixture's completing objective completes.
fn arm_deadline(session: &mut ObjectiveSession, tick: u64) -> cs_app::objectives::SessionTick {
    let requests = [TimerRequest::Arm(SymbolId(
        declared_synthetic_completion_effects().timers()[0].symbol.0,
    ))];
    let mut facts = input(tick, 1);
    facts.timer_requests = &requests;
    session.step(&facts).expect("a legal tick")
}

fn launch(program: &DeclaredObjectiveProgram) -> ObjectiveSession {
    ObjectiveSession::launch(lower_program(program).expect("the program lowers"), GEN)
        .expect("the lowered program launches")
}

fn symbol(symbol: ProgramSymbol) -> SymbolId {
    SymbolId(symbol.0)
}

/// The `ObjectiveChanged` events of one tick, as `(source, objective, from, to,
/// sequence)` — everything a consumer needs to read a completion effect's move
/// and the key it reported under.
fn changes(
    tick: &cs_app::objectives::SessionTick,
) -> Vec<(SymbolId, SymbolId, ObjectiveState, ObjectiveState, u32)> {
    tick.tick
        .events
        .iter()
        .filter_map(|event| match event.kind {
            ObjectiveEventKind::ObjectiveChanged {
                objective,
                from,
                to,
            } => Some((event.key.source, objective, from, to, event.key.sequence)),
            _ => None,
        })
        .collect()
}

/// The same program with one nap number replaced, everything else untouched.
fn with_nap_number(value: f64) -> DeclaredObjectiveProgram {
    let base = declared_synthetic_completion_effects();
    let mut objectives = base.objectives().to_vec();
    let completing = objectives
        .iter_mut()
        .find(|objective| !objective.completion_effects.is_empty())
        .expect("the fixture's completing objective declares effects");
    let nap = completing
        .completion_effects
        .iter_mut()
        .find(|effect| effect.kind == BranchEffectKind::Nap)
        .expect("the fixture naps one objective");
    *nap = DeclaredCompletionEffect::new(BranchEffectKind::Nap, nap.objective, Some(value))
        .expect("a finite declared number");
    DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        objectives,
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("the rebuilt program is still valid")
}

// ---------------------------------------------------------------------------
// The production path: a completion moves the objectives it names
// ---------------------------------------------------------------------------

#[test]
fn accept_f39_e5_a_completion_wakes_the_objectives_it_names() {
    let program = declared_synthetic_completion_effects();
    let mut session = launch(&program);

    let completed = arm_deadline(&mut session, 1);
    assert!(
        completed.refusals.is_empty(),
        "a legal completion effect is not a refusal: {:?}",
        completed.refusals
    );
    assert_eq!(
        session.outcome(),
        None,
        "the fixture does not end the mission"
    );

    // The deadline drove the completion, so *its* event reports under the timer…
    let (deadline, completed_objective, from, to, _) = changes(&completed)
        .into_iter()
        .find(|(_, objective, _, _, _)| *objective == symbol(program.objectives()[0].symbol))
        .expect("the deadline completed the completing objective");
    assert_eq!(deadline, symbol(program.timers()[0].symbol));
    assert_eq!(completed_objective, symbol(program.objectives()[0].symbol));
    assert_eq!(
        (from, to),
        (ObjectiveState::Active, ObjectiveState::Succeeded)
    );

    // …and each declared effect applies under the objective whose declaration
    // carries it — not under the timer that happened to complete it — in the
    // engine's fixed order (M01-B-OBJ-1): the wake and the wakeup move their
    // targets, the nap moves its target to `Napping`, and the kill is a
    // removal that moves no state at all.
    let moved = changes(&completed);
    let completing = symbol(program.objectives()[0].symbol);
    let effect_moves: Vec<_> = moved
        .into_iter()
        .filter(|(source, _, _, _, _)| *source == completing)
        .collect();
    assert_eq!(
        effect_moves.len(),
        3,
        "one move per state-moving effect (the kill moves none): {effect_moves:?}"
    );
    for (index, expected) in [
        (
            program.objectives()[1].symbol,
            ObjectiveState::Pending,
            ObjectiveState::Active,
        ),
        (
            program.objectives()[3].symbol,
            ObjectiveState::Optional,
            ObjectiveState::Active,
        ),
        (
            program.objectives()[2].symbol,
            ObjectiveState::Active,
            ObjectiveState::Napping,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (source, objective, from, to, sequence) = effect_moves[index];
        assert_eq!(
            source, completing,
            "an effect reports under its declaration"
        );
        assert_eq!(objective, symbol(expected.0));
        assert_eq!(
            (from, to),
            (expected.1, expected.2),
            "the effect moved the state it declares"
        );
        if index > 0 {
            assert!(
                effect_moves[index - 1].4 < sequence,
                "engine effect order is kept: sequence {sequence} follows {}",
                effect_moves[index - 1].4
            );
        }
    }

    // The kill is the fourth effect: it removed its target instead of moving
    // it, and the removal is on the same ordered stream.
    let removed: Vec<_> = completed
        .tick
        .events
        .iter()
        .filter_map(|event| match event.kind {
            ObjectiveEventKind::ObjectiveRemoved { objective, from } => {
                Some((event.key.source, objective, from))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        removed,
        vec![(
            completing,
            symbol(program.objectives()[4].symbol),
            ObjectiveState::Active
        )],
        "the kill removes its target and reports the state it kept"
    );

    // The consumer side needs nothing new: the objective display moved from the
    // same ordered stream.
    for (index, state) in [
        ObjectiveState::Succeeded,
        ObjectiveState::Active,
        ObjectiveState::Napping,
        ObjectiveState::Active,
        ObjectiveState::Active,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            session
                .display()
                .row(symbol(program.objectives()[index].symbol))
                .expect("tracked")
                .state,
            state,
            "objective {index} shows the state the stream reported"
        );
    }
    assert!(
        session
            .display()
            .row(symbol(program.objectives()[4].symbol))
            .expect("tracked")
            .revealed,
        "a killed objective is still shown to the player; it was removed from play, it did not vanish"
    );

    // The stream is still ordered by (session, tick, source, sequence).
    let keys: Vec<_> = completed
        .tick
        .events
        .iter()
        .map(|event| event.key)
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "the stream is sorted by EventKey");
    assert!(
        keys.iter()
            .all(|key| key.session == GEN && key.tick == Tick(1))
    );
}

#[test]
fn accept_f39_e5_effects_apply_on_the_completion_tick_and_only_once() {
    let program = declared_synthetic_completion_effects();
    let mut session = launch(&program);

    let completed = arm_deadline(&mut session, 1);
    // The eligibility rule: the queue is drained in the tick that filled it, so a
    // completion's effects are observable on the *same* stream as the completion
    // instead of appearing a tick later with nothing to explain the delay.
    let moved_on_completion = changes(&completed).len();
    assert_eq!(
        moved_on_completion, 4,
        "one completion plus the three state-moving effects; the kill's removal moves nothing"
    );

    // A later tick with no facts moves nothing: the objectives have already been
    // moved, and an objective's completion latches, so its effects cannot replay.
    let idle = session.step(&input(2, 1)).expect("a legal tick");
    assert!(
        changes(&idle).is_empty(),
        "a completion effect fires once: {:?}",
        changes(&idle)
    );
    assert!(idle.refusals.is_empty());

    // An explicit re-arm does run the deadline again — that is F39-B's rule for a
    // repeating deadline — but the objective it completes has latched, so the
    // completion is refused by name and the completion effects that belong to it
    // cannot replay. The effects are the objective's, not the timer's.
    let deadline = program.timers()[0].symbol;
    let requests = [TimerRequest::Arm(symbol(deadline))];
    let mut facts = input(3, 1);
    facts.timer_requests = &requests;
    let rearmed = session.step(&facts).expect("a legal tick");
    assert!(
        rearmed.tick.events.iter().any(|event| matches!(
            event.kind,
            ObjectiveEventKind::TimerExpired { timer } if timer == symbol(deadline)
        )),
        "an explicit re-arm really did run the deadline again"
    );
    assert!(
        rearmed.refusals.iter().any(|refusal| matches!(
            refusal,
            cs_app::objectives::SessionRefusal::ObjectiveChange { objective, from, to }
                if *objective == symbol(program.objectives()[0].symbol)
                    && *from == ObjectiveState::Succeeded
                    && *to == ObjectiveState::Succeeded
        )),
        "the repeated completion is refused by name: {:?}",
        rearmed.refusals
    );
    let replayed: Vec<_> = changes(&rearmed)
        .into_iter()
        .filter(|(_, objective, _, _, _)| *objective != symbol(program.objectives()[0].symbol))
        .collect();
    assert!(
        replayed.is_empty(),
        "and no completion effect replayed: {replayed:?}"
    );
}

#[test]
fn accept_f39_e5_a_napped_objective_is_set_aside_whatever_the_number_says() {
    // 0.5 and 170 are the two ends of the range F39-E2 measured across the 417
    // nap sites. M01-B-OBJ-1 measured that the number is the target's *nap
    // time* in the engine's own unit — but that unit's correspondence to this
    // runtime's integer ticks is unmeasured, so nothing may schedule on it
    // either: two programs differing only in the number still behave
    // identically.
    let short = with_nap_number(0.5);
    let long = with_nap_number(170.0);

    let mut a = launch(&short);
    let mut b = launch(&long);
    let tick_a = arm_deadline(&mut a, 1);
    let tick_b = arm_deadline(&mut b, 1);
    assert_eq!(
        changes(&tick_a),
        changes(&tick_b),
        "the declared number may not change what the nap does"
    );
    for (program, session) in [(&short, &a), (&long, &b)] {
        assert_eq!(
            session
                .display()
                .row(symbol(program.objectives()[2].symbol))
                .expect("tracked")
                .state,
            ObjectiveState::Napping,
            "the napped objective is put aside in the engine's state 2"
        );
    }

    // The number does cross the boundary, as the declared nap time: it is on
    // the lowered effect, unchanged, with no conversion into ticks attached
    // to it.
    let lowered = lower_program(&long).expect("the program lowers");
    let nap = lowered.objectives[0]
        .completion_effects
        .iter()
        .find(|effect| effect.kind == CompletionEffectKind::Nap)
        .expect("the nap lowered");
    assert_eq!(
        nap.argument.map(NapTime::value),
        Some(170.0),
        "the declared nap time arrives verbatim, in the engine's own unit"
    );
}

#[test]
fn accept_f39_e5_an_effect_on_a_hidden_objective_is_reported_not_dropped() {
    // Visibility stays governed by the reveal rule (F39 non-negotiable
    // behavior 5), so an effect naming an objective the player has not been
    // shown cannot reveal it. It must not be silent either: the runtime's rule is
    // that a named reference is never dropped without a report.
    let base = declared_synthetic_completion_effects();
    let mut objectives = base.objectives().to_vec();
    objectives[1].initial = DeclaredObjectiveState::Hidden;
    objectives[1].reveal = DeclaredRevealRule::OnSignal(ProgramSymbol(90));
    let program = DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        objectives,
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("naming a hidden objective is a live declaration: its reveal may come first");
    let woken = program.objectives()[1].symbol;
    let mut session = launch(&program);

    let completed = arm_deadline(&mut session, 1);
    assert!(
        completed.refusals.iter().any(|refusal| matches!(
            refusal,
            cs_app::objectives::SessionRefusal::ObjectiveChange { objective, to, .. }
                if *objective == symbol(woken) && *to == ObjectiveState::Active
        )),
        "the refused move is reported, not dropped: {:?}",
        completed.refusals
    );
    let row = session.display().row(symbol(woken)).expect("tracked");
    assert_eq!(row.state, ObjectiveState::Hidden);
    assert!(
        !row.revealed,
        "a completion effect never reveals an objective"
    );
}

// ---------------------------------------------------------------------------
// The one accepted conflict, and the refusal that stays
// ---------------------------------------------------------------------------

/// Rebuilds the fixture with `edit` applied to its objective list, and
/// answers the schema's verdict on the result.
fn with_objectives(
    edit: impl FnOnce(&mut Vec<DeclaredObjective>),
) -> Result<DeclaredObjectiveProgram, ObjectivesSchemaError> {
    let base = declared_synthetic_completion_effects();
    let mut objectives = base.objectives().to_vec();
    edit(&mut objectives);
    DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        objectives,
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
}

#[test]
fn accept_f39_e5_one_declaration_two_effects_on_one_target_is_accepted() {
    // The isolated condition F39-E2 measured, in the shape a declared program can
    // express: one completing objective, two different effects, one target. This
    // is `zbd/c3/m05` `OBJECTIVE8`, which names objective 68 in both a wake and
    // a nap — and M01-B-OBJ-1 measured the engine resolving exactly that shape
    // with its fixed order (`WAKE` then `NAP`, so the target ends napping). The
    // schema therefore accepts both authored orders; nothing ranks by the
    // authored order, because the runtime applies the engine's own.
    for reversed in [false, true] {
        let program = with_objectives(|objectives| {
            let nap_on_woken =
                DeclaredCompletionEffect::new(BranchEffectKind::Nap, SYNTHETIC_E5_WOKEN, Some(2.0))
                    .expect("a nap with its number");
            let completing = objectives
                .iter_mut()
                .find(|objective| !objective.completion_effects.is_empty())
                .expect("the fixture's completing objective declares effects");
            if reversed {
                completing.completion_effects.insert(0, nap_on_woken);
            } else {
                completing.completion_effects.push(nap_on_woken);
            }
        })
        .expect(
            "one declaration carrying two effects on one target is decided by the \
             engine's fixed order, not refused",
        );
        let declared = program
            .objectives()
            .iter()
            .find(|objective| objective.symbol == SYNTHETIC_E5_COMPLETING)
            .expect("the rebuilt program keeps the declaring objective");
        let kinds: Vec<BranchEffectKind> = declared
            .completion_effects
            .iter()
            .map(|effect| effect.kind)
            .collect();
        let (wake_first, nap_first) = (
            vec![
                BranchEffectKind::Wake,
                BranchEffectKind::Nap,
                BranchEffectKind::Wakeup,
                BranchEffectKind::Kill,
                BranchEffectKind::Nap,
            ],
            vec![
                BranchEffectKind::Nap,
                BranchEffectKind::Wake,
                BranchEffectKind::Nap,
                BranchEffectKind::Wakeup,
                BranchEffectKind::Kill,
            ],
        );
        assert_eq!(
            kinds,
            if reversed { nap_first } else { wake_first },
            "both authored orders are accepted and carried as spelled"
        );
    }

    // The runtime applies one declaration's two effects on one target in the
    // engine's fixed order (`WAKE` then `KILL` here) whatever the authored
    // order — the retail block F39-E2 measured spells exactly this shape.
    let mut runtime = ObjectiveRuntime::new(
        GEN,
        TerminalPrecedence::SyntheticConservative,
        Default::default(),
    );
    let both = ObjectiveSpec {
        id: SymbolId(1),
        content: ContentId::from_source(ContentKind::Objective, "synthetic.f39e.both")
            .expect("an objective id"),
        initial: ObjectiveState::Active,
        reveal: RevealRule::Immediate,
        on_complete: ObjectiveCompletion::Continue,
        completion_effects: vec![
            CompletionEffect {
                kind: CompletionEffectKind::Wake,
                target: SymbolId(3),
                argument: None,
            },
            CompletionEffect {
                kind: CompletionEffectKind::Kill,
                target: SymbolId(3),
                argument: None,
            },
        ],
    };
    assert!(
        runtime.add_objective(both).is_ok(),
        "one declaration carrying two effects on one target is ordered by the engine's fixed order"
    );
}

#[test]
fn accept_f39_e5_two_completions_naming_one_objective_are_refused_by_name() {
    // What stays refused after the within-declaration acceptance: two
    // *different* declaring objectives naming different effects for one
    // target. The engine completes one objective per pass, so those two
    // completions never collide there; which of two same-tick completions
    // applies last depends on the pass-origin rule, which stays unmeasured.
    // Both authored assignments of the two kinds are refused by name.
    for (first, second) in [
        (BranchEffectKind::Wake, BranchEffectKind::Nap),
        (BranchEffectKind::Nap, BranchEffectKind::Wake),
    ] {
        let refused = with_objectives(|objectives| {
            let effect = |kind| {
                DeclaredCompletionEffect::new(
                    kind,
                    SYNTHETIC_E5_WOKEN,
                    kind.carries_argument().then_some(2.0),
                )
                .expect("a finite declared effect")
            };
            objectives[0].completion_effects = vec![effect(first)];
            objectives[2].completion_effects = vec![effect(second)];
        })
        .expect_err("two completions naming one target differently are refused");
        assert_eq!(
            refused,
            ObjectivesSchemaError::AmbiguousCompletionEffect {
                by: SYNTHETIC_E5_NAPPED,
                objective: SYNTHETIC_E5_WOKEN,
                first,
                second,
            },
            "the authored order decides nothing: {first} then {second} is refused the \
             same way round"
        );
        let message = refused.to_string();
        assert!(
            message.contains("symbol(3)") && message.contains("symbol(2)"),
            "the refusal names who declared what and on whom: {message}"
        );
        assert!(
            message.contains("no measured rule says which of the two completions applies last"),
            "the refusal states the rule: {message}"
        );
    }

    // The runtime refuses the same shape across two *different* completions,
    // in either registration order, because a caller may build a program
    // without the declared schema at all — and because the engine completes
    // one objective per pass, so which of two same-tick completions should
    // apply last depends on the unmeasured pass-origin rule.
    for order in [0, 1] {
        let mut runtime = ObjectiveRuntime::new(
            GEN,
            TerminalPrecedence::SyntheticConservative,
            Default::default(),
        );
        let spec = |id: u32, target: u32, kind: CompletionEffectKind| ObjectiveSpec {
            id: SymbolId(id),
            content: ContentId::from_source(
                ContentKind::Objective,
                &format!("synthetic.f39e.{id}"),
            )
            .expect("an objective id"),
            initial: ObjectiveState::Active,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Continue,
            completion_effects: vec![CompletionEffect {
                kind,
                target: SymbolId(target),
                argument: None,
            }],
        };
        let (first, second) = if order == 0 {
            (
                spec(1, 3, CompletionEffectKind::Wake),
                spec(2, 3, CompletionEffectKind::Kill),
            )
        } else {
            (
                spec(2, 3, CompletionEffectKind::Kill),
                spec(1, 3, CompletionEffectKind::Wake),
            )
        };
        runtime
            .add_objective(first)
            .expect("the first declaration registers");
        assert_eq!(
            runtime.add_objective(second),
            Err(RuntimeError::AmbiguousCompletionEffect {
                source: SymbolId(if order == 0 { 2 } else { 1 }),
                target: SymbolId(3),
            }),
            "registration order decides nothing either"
        );
    }

    // And the refusal is not "no two effects at all": two objectives that *agree*
    // about what happens to one objective declare no order to decide.
    assert!(
        with_objectives(|objectives| {
            objectives[2].completion_effects = vec![
                DeclaredCompletionEffect::new(BranchEffectKind::Kill, ProgramSymbol(5), None)
                    .expect("a kill"),
            ];
        })
        .is_ok(),
        "two declarations of the same effect on one objective agree and are legal"
    );
}

#[test]
fn accept_f39_e5_dead_completion_effects_are_refused_by_name() {
    let base = declared_synthetic_completion_effects();
    let effect = |kind, objective, argument| {
        DeclaredCompletionEffect::new(kind, objective, argument).expect("a finite declared effect")
    };
    let rebuild = |objectives: Vec<DeclaredObjective>| {
        DeclaredObjectiveProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            base.precedence().clone(),
            objectives,
            base.conditions().to_vec(),
            base.timers().to_vec(),
            base.triggers().to_vec(),
            base.spawn_groups().to_vec(),
        )
        .expect_err("a dead completion effect is refused at declaration")
    };

    // An effect on the objective that fires it: it applies only once that
    // objective holds `Succeeded`, and nothing moves a target out of a final
    // state.
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects = vec![effect(BranchEffectKind::Wake, ProgramSymbol(1), None)];
    assert_eq!(
        rebuild(objectives),
        ObjectivesSchemaError::SelfCompletionEffect {
            objective: ProgramSymbol(1)
        }
    );

    // An effect naming an objective born finished: no row leaves a final state.
    for born in [
        DeclaredObjectiveState::Succeeded,
        DeclaredObjectiveState::Failed,
        DeclaredObjectiveState::Superseded,
    ] {
        let mut objectives = base.objectives().to_vec();
        objectives[2].initial = born;
        objectives[0].completion_effects =
            vec![effect(BranchEffectKind::Wake, ProgramSymbol(3), None)];
        assert_eq!(
            rebuild(objectives),
            ObjectivesSchemaError::DeadCompletionEffect {
                by: ProgramSymbol(1),
                objective: ProgramSymbol(3),
                state: born,
            }
        );
    }

    // Effects declared by an objective born finished: it never completes, so they
    // never fire. Nothing else names it here, so the refusal is about the
    // declaring objective rather than about a target born finished.
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects = Vec::new();
    objectives[4].initial = DeclaredObjectiveState::Succeeded;
    objectives[4].completion_effects = vec![effect(BranchEffectKind::Wake, ProgramSymbol(2), None)];
    assert_eq!(
        rebuild(objectives),
        ObjectivesSchemaError::UnfiredCompletionEffects {
            by: ProgramSymbol(5),
            state: DeclaredObjectiveState::Succeeded
        }
    );

    // …and the refusal is about the *effects*, not about the state. An objective
    // born finished that declares none is an ordinary declaration this stage has
    // no rule about — nothing was measured about an objective that never
    // completes, because it can never complete — so the schema must accept it
    // rather than refuse it under a message claiming effects it never declared.
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects = Vec::new();
    objectives[4].initial = DeclaredObjectiveState::Succeeded;
    assert!(
        DeclaredObjectiveProgram::try_new(
            base.subject().clone(),
            base.origin().clone(),
            base.provenance().clone(),
            base.precedence().clone(),
            objectives,
            base.conditions().to_vec(),
            base.timers().to_vec(),
            base.triggers().to_vec(),
            base.spawn_groups().to_vec(),
        )
        .is_ok(),
        "an objective born finished that declares no completion effects is not this refusal"
    );

    // A nap without the number every measured nap carries, and a number on an
    // effect that never had one.
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects = vec![effect(BranchEffectKind::Nap, ProgramSymbol(3), None)];
    assert_eq!(
        rebuild(objectives.clone()),
        ObjectivesSchemaError::EffectArgumentShape {
            by: ProgramSymbol(1),
            objective: ProgramSymbol(3),
            kind: BranchEffectKind::Nap,
        }
    );
    let mut with_number = objectives;
    with_number[0].completion_effects =
        vec![effect(BranchEffectKind::Wake, ProgramSymbol(3), Some(2.0))];
    assert_eq!(
        rebuild(with_number),
        ObjectivesSchemaError::EffectArgumentShape {
            by: ProgramSymbol(1),
            objective: ProgramSymbol(3),
            kind: BranchEffectKind::Wake,
        }
    );

    // A target this program does not declare is a dangling reference, refused at
    // declaration for the same reason every other one is: F39-E2 measured that
    // every retail target names an objective of the same record, so there is no
    // cross-record name to resolve here either.
    let mut objectives = base.objectives().to_vec();
    objectives[0].completion_effects =
        vec![effect(BranchEffectKind::Wake, ProgramSymbol(99), None)];
    assert_eq!(
        rebuild(objectives),
        ObjectivesSchemaError::UnknownObjective {
            by: ProgramSymbol(1),
            objective: ProgramSymbol(99)
        }
    );

    // The refusals say why, by name.
    assert!(
        ObjectivesSchemaError::SelfCompletionEffect {
            objective: ProgramSymbol(1)
        }
        .to_string()
        .contains("can only apply once it has completed"),
        "the self-effect names its rule"
    );
    assert!(
        ObjectivesSchemaError::UnfiredCompletionEffects {
            by: ProgramSymbol(5),
            state: DeclaredObjectiveState::Succeeded,
        }
        .to_string()
        .contains("never completes"),
        "the unfired effects name their rule"
    );
}

#[test]
fn accept_f39_e5_a_tick_over_its_event_budget_is_stopped_before_any_effect_applies() {
    // The bound is decided before anything applies, and it has to *count* the
    // completion effects a tick is about to queue — otherwise a program whose
    // objectives each declare effects could emit more events in one tick than
    // `max_events_per_tick` allows without the bound ever noticing. 8 is exactly
    // what this fixture's tick declares without its four effects (1 timer request,
    // 2 timer events, 5 objective events), so a tick stopped at 8 was stopped by
    // the effects and by nothing else.
    let budget = 8;
    let program = declared_synthetic_completion_effects();
    let mut session = ObjectiveSession::launch(
        lower_program(&program)
            .expect("the program lowers")
            .with_limits(RuntimeLimits {
                max_events_per_tick: budget,
                max_timers: 1024,
            }),
        GEN,
    )
    .expect("the lowered program launches");

    let stopped = arm_deadline(&mut session, 1);
    assert_eq!(
        stopped.stop,
        Some(StopReason::EventBudget {
            at_tick: Tick(1),
            limit: budget,
            declared: budget + 4,
        }),
        "the four declared effects are counted against the bound: {:?}",
        stopped.stop
    );
    assert!(
        stopped.tick.events.is_empty(),
        "a stopped tick applies nothing at all: {:?}",
        stopped.tick.events
    );
    assert!(!stopped.display_changed, "so no row moved");

    // Nothing is left half-applied: not the completion, not one effect, and no
    // effect waiting for a later tick to apply it late.
    for (index, state) in [
        ObjectiveState::Active,
        ObjectiveState::Pending,
        ObjectiveState::Active,
        ObjectiveState::Optional,
        ObjectiveState::Active,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            session
                .display()
                .row(symbol(program.objectives()[index].symbol))
                .expect("tracked")
                .state,
            state,
            "objective {index} still holds the state it was born in"
        );
    }
}

#[test]
fn accept_f39_e5_the_effect_vocabulary_is_the_measured_one() {
    // The declared form is written over F39-E2's measured vocabulary, so this
    // stage declares no second spelling list: the four keys are the measured
    // ones, each a member of F39-D's branching vocabulary, and they round-trip.
    assert_eq!(BRANCH_EFFECT_KEY_VOCABULARY.len(), 4);
    for key in BRANCH_EFFECT_KEY_VOCABULARY {
        assert!(
            BRANCH_KEY_VOCABULARY.contains(&key),
            "{key} is a measured branching key"
        );
        assert_eq!(
            BranchEffectKind::from_measured_key(key)
                .expect("a measured key names an effect")
                .measured_key(),
            key,
            "the vocabulary round-trips"
        );
    }
    // `WAKE` and `WAKEUP` are two spellings in one corpus and nothing measured
    // says they are the same effect, so they never collapse into each other.
    assert_ne!(
        BranchEffectKind::from_measured_key("WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        BranchEffectKind::from_measured_key("WAKEUP_OBJECTIVE_WHEN_I_COMPLETE")
    );
    // A spelling nobody measured names nothing, rather than reading as the
    // closest effect it shares a prefix with.
    for unknown in [
        "WAKE_OBJECTIVE",
        "WAKEUP",
        "KILL_OBJECTIVE_WHEN_I_AM_SICK",
        "TICK_DEPENDS_ON_OBJ",
        "",
    ] {
        assert_eq!(
            BranchEffectKind::from_measured_key(unknown),
            None,
            "{unknown} is not a measured completion effect"
        );
    }
    // Only a nap carries the measured number.
    assert!(BranchEffectKind::Nap.carries_argument());
    for kind in [
        BranchEffectKind::Wake,
        BranchEffectKind::Kill,
        BranchEffectKind::Wakeup,
    ] {
        assert!(!kind.carries_argument());
    }

    // The two named verdicts this stage leans on still state the measured
    // conditions behind the two rules it enforces: the isolated block it refuses
    // to order, and the number it refuses to interpret. A verdict that stopped
    // naming its condition would leave the rules asserting an absence, so both are
    // pinned here.
    assert!(
        UNMEASURED_BLOCK_PRECEDENCE.contains("exactly one of its blocks")
            && UNMEASURED_BLOCK_PRECEDENCE.contains("must not apply an authored field order"),
        "the precedence verdict still names the isolated condition and the rule it forbids: {UNMEASURED_BLOCK_PRECEDENCE}"
    );
    assert!(
        UNMEASURED_NAP_ARGUMENT.contains("no other completion-effect site carries one")
            && UNMEASURED_NAP_ARGUMENT
                .contains("nothing may schedule, compare or weigh anything on it"),
        "the nap verdict still names the measured shape and the use it forbids: {UNMEASURED_NAP_ARGUMENT}"
    );

    // The lowering keeps the kinds apart, and no kind moves its target to
    // `Succeeded` — the structural reason an applied effect can never queue
    // another one, so the effect queue cannot cascade.
    let lowered = lower_program(&declared_synthetic_completion_effects()).expect("it lowers");
    let kinds: Vec<_> = lowered.objectives[0]
        .completion_effects
        .iter()
        .map(|effect| effect.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            CompletionEffectKind::Wake,
            CompletionEffectKind::Nap,
            CompletionEffectKind::Wakeup,
            CompletionEffectKind::Kill,
        ],
        "authored order and the four distinct kinds survive the boundary"
    );
    for kind in [
        CompletionEffectKind::Wake,
        CompletionEffectKind::Nap,
        CompletionEffectKind::Kill,
        CompletionEffectKind::Wakeup,
        CompletionEffectKind::Sleep,
    ] {
        assert_ne!(
            kind.moves_to(),
            Some(ObjectiveState::Succeeded),
            "{kind:?} must not complete its target"
        );
    }
    // And the moves are the engine's measured ones (M01-B-OBJ-1): a wake to
    // `Active`, a nap to the timed-pause state, a sleep to `Asleep`, and a
    // kill that moves no state at all — it removes instead.
    assert_eq!(
        CompletionEffectKind::Wake.moves_to(),
        Some(ObjectiveState::Active)
    );
    assert_eq!(
        CompletionEffectKind::Nap.moves_to(),
        Some(ObjectiveState::Napping)
    );
    assert_eq!(
        CompletionEffectKind::Sleep.moves_to(),
        Some(ObjectiveState::Asleep)
    );
    assert_eq!(CompletionEffectKind::Kill.moves_to(), None);
}

#[test]
fn accept_f39_e5_an_original_record_with_effects_is_never_played() {
    // The gate is not weakened by this stage. The same declarations over an
    // installation origin name objectives whose rules were never recovered, so
    // they stay Unsupported and `lower_program` refuses by name with the
    // record's own reason: a declared vocabulary for what a record *says* is not
    // a recovery of what it *means*.
    let base = declared_synthetic_completion_effects();
    let span = SourceSpan::new(
        ContentHash::from_hex(&"0".repeat(64)).expect("a 64-nibble hash"),
        "ZBD/C3/M05/zrdr.zbd",
        Some("objectives.zrd"),
        15718,
        13998,
        None,
    )
    .expect("a valid source span");
    let original = DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        Origin::Installation { source: span },
        base.provenance().clone(),
        base.precedence().clone(),
        base.objectives().to_vec(),
        base.conditions().to_vec(),
        base.timers().to_vec(),
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("an original-origin record validates its declarations");
    assert!(matches!(
        original.support(),
        DeclaredSupport::Original { .. }
    ));
    assert!(!original.is_playable());
    assert_eq!(
        original.support().refusal(),
        Some(UNMEASURED_OBJECTIVE_SEMANTICS)
    );
    match lower_program(&original).unwrap_err() {
        ProgramLowerError::UnsupportedProgram { reason, .. } => {
            assert_eq!(reason, UNMEASURED_OBJECTIVE_SEMANTICS);
        }
        other => panic!("an unrecovered record must be refused by name, got {other:?}"),
    }

    // The authored twin of the very same declarations lowers and plays, so the
    // gate is about provenance and not about the new vocabulary.
    assert!(base.is_playable());
    let mut session = launch(&base);
    assert!(arm_deadline(&mut session, 1).refusals.is_empty());
}
