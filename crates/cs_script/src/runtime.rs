//! Mutable mission state, stable event ordering, the bounded evaluator, the
//! pending-work queue and the save record (F37-A, F37-B, F37-C, F37-D).
//!
//! Program data is [`crate::ir`]; this module holds the *execution* state and
//! the pure per-tick resolution that the simulation host drives. The bounded
//! work budget and the deferred work queue are F37-B; the versioned
//! [`MissionStateSnapshot`] that preserves a pending timer's exact remaining
//! ticks across save/restore is F37-C. F37-D ran the adversarial corpus and the
//! reference ordering probes: the work budget's floor ([`MIN_WORK_PER_TICK`])
//! and two more save-record refusals come from it. Host effect application is
//! the simulation side (`cs_sim::mission`).
//!
//! One input precedes every phase below: the **mission-countdown
//! pre-emption** (F37-D-FU4, measured — owner note on Rally #589). When the
//! countdown expired on this tick and neither measured exclusion applies
//! ([`MissionCountdown::preempts`]), the tick ends *there*: the session
//! records [`TerminalState::Failed`] and phases 1-4 do not run at all, so no
//! objective of that tick completes, no reward of that tick is granted and no
//! queued item due that tick runs.
//!
//! Phases of one tick (`docs/contracts/SCRIPT-MISSION.md`, "Objective event
//! ordering"; "actions do not directly recurse into callbacks"):
//! 1. **Observe** — every condition is evaluated against the state as it was
//!    at the start of the tick; nothing evaluated in this tick sees a write
//!    made in this tick.
//! 2. **Queue + resolve objectives** — firing objectives are taken in program
//!    order and their actions run. `Schedule`/`Reschedule` never call back
//!    into evaluation: they append to the *pending* queue, which is drained
//!    separately.
//! 3. **Drain pending work** — items whose `due` tick is now or past run in
//!    (due, enqueue) order, one action at a time. A zero-delay item appends to
//!    the end of the queue being drained, so it cannot starve earlier work.
//! 4. **Apply** — `State` writes become visible, `Terminal` requests are
//!    resolved by the [`PrecedencePolicy`], `Host` effects are emitted.
//!
//! Two orders come out of one tick and they are not the same order. Execution
//! follows phase 2 then 3, so two objectives writing one variable on one tick
//! leave the later *declaration*'s write standing. Observation follows the
//! [`EventKey`] total order, which is by source symbol and does not depend on
//! declaration order at all. F37-D pinned both with its corpus.
//!
//! Both gameplay rules carry an explicit **source label** ([`RuleSource`],
//! F37-D-FU2): the terminal precedence
//! ([`TERMINAL_PRECEDENCE_RULE`]) and the per-tick order
//! ([`TICK_ORDERING_RULE`]) are `measured-from-original` — static code
//! evidence from the owner-supplied decrypted executable, never an original
//! run and never `verified_original` — while the observation order
//! ([`EVENT_OBSERVATION_ORDER_RULE`]) and
//! [`PrecedencePolicy::SyntheticConservative`] are `designed-and-unmeasured`.
//! Where this runtime does not follow a measured fact, the fact names the
//! [`RULE_LIMITATIONS`] entry that gates the affected fidelity claim; nothing
//! is recorded as measured that has not been read at the addresses cited.
//!
//! The terminal check's **presentation half** is
//! [`MissionEndPresentation`] (F37-D-FU5): the same tick that records the
//! result selects the branch (LOST before WON), the end delay, the
//! `OBJECTIVES_*_SOUND`/`MISSION_*_SOUND` slots and the win/loss animation,
//! and the three reads stay independent — the loss branch can run while the
//! recorded result is the success. A selection is not a playback: which of
//! those slots a mission actually fills with a handle is mission content, and
//! playing, showing or timing them on screen belongs to the audio/presentation
//! stages, not to this layer.
//!
//! Bounds (contract: "each tick has an instruction/action budget and
//! recursion/stack limits"):
//! - every objective firing, pending dequeue and action execution spends one
//!   unit of the per-tick [`WorkLimits::max_work_per_tick`] budget, which is
//!   bounded below by [`MIN_WORK_PER_TICK`] so a tick can always execute the
//!   first action of the item it admits;
//! - the pending queue holds at most
//!   [`WorkLimits::max_pending_items`] scheduled items (memory cap);
//! - `Schedule` nesting is bounded at validation ([`crate::ir::MAX_ACTION_NESTING`]).
//!
//! When the budget runs out the tick does **not** error: the interrupted list
//! is re-queued at its next action, everything already executed stays
//! committed, and the result carries [`StopReason`] with the mission id,
//! program locator and the trace — the contract's budget diagnostic. Nothing
//! is skipped and nothing is repeated.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::time::Duration;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::evidence::ClaimStatus;
use cs_types::random::SplitMix64;

use crate::ir::{
    Action, ActorId, ActorState, CompareOp, Condition, DirectiveOperation,
    MAX_ACTIONS_PER_OBJECTIVE, MemberName, Outcome, ProgramLocator, SymbolId, TravelersAnchor,
    ValidatedProgram, ValidationError, Value, ValueType,
};

/// SplitMix64 domain separating the mission evaluator's stream from every
/// other consumer of a run seed (`"MSN_EVAL"`). See
/// `cs_types::random::SplitMix64::for_domain`.
const MISSION_EVALUATOR_DOMAIN: u64 = 0x4D53_4E5F_4556_414C;

/// Default per-tick work budget: units of "one firing, one dequeue or one
/// action". The value is a design bound, not a measured original limit.
pub const MAX_WORK_PER_TICK: u64 = 4096;

/// Smallest work budget that can still make progress.
///
/// Admitting a work item costs one unit — an objective firing or a pending
/// dequeue — and every action it runs costs another. A budget of one therefore
/// admits items and never executes any of their actions: the mission latches
/// its objectives, reports `StopReason::WorkBudget` on every tick forever and
/// never grants, never finishes and never clears its queue. That is a silent
/// stall, not a bound, so two units is the floor:
/// [`MissionState::set_limits`] raises a smaller budget to it, and a save record
/// claiming less is refused ([`RestoreError::WorkBudgetTooSmall`]).
pub const MIN_WORK_PER_TICK: u64 = 2;

/// Default cap on stored scheduled items (contract: "cap memory/time").
/// A design bound, not a measured original limit.
pub const MAX_PENDING_ITEMS: usize = 4096;

/// Version of the [`MissionStateSnapshot`] record this crate writes. It is
/// separate from [`crate::ir::IR_VERSION`] because a save outlives the program
/// it was taken from: an older record must be refused, never reinterpreted.
///
/// Version 2 carries the mission-end presentation
/// ([`MissionEndPresentation`], F37-D-FU5): a record written before it cannot
/// say which branch, delay, sounds and animation the tick that ended the
/// session selected, so it is refused rather than read as if it carried one.
pub const SNAPSHOT_VERSION: u32 = 2;

/// Most `Draw`s a restore will replay to rewind the mission's RNG stream.
///
/// `SplitMix64` exposes no state getter (`cs_types::random` is outside this
/// stage's owner paths), so [`MissionStateSnapshot`] stores how many draws the
/// session took and a restore re-seeds the same domain-separated stream and
/// replays them. A design bound on restore work — contract: "cap memory/time"
/// — and not a measured original limit.
pub const MAX_RNG_REPLAY_DRAWS: u64 = 1 << 22;

/// The per-tick and queue bounds the evaluator runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkLimits {
    /// Most work units one tick may spend. A caller may ask for anything in
    /// `[MIN_WORK_PER_TICK, MAX_WORK_PER_TICK]`; a smaller budget is raised to
    /// the floor rather than stalling the mission.
    pub max_work_per_tick: u64,
    /// Most scheduled items the pending queue may hold.
    pub max_pending_items: usize,
}

impl Default for WorkLimits {
    fn default() -> Self {
        Self {
            max_work_per_tick: MAX_WORK_PER_TICK,
            max_pending_items: MAX_PENDING_ITEMS,
        }
    }
}

/// Generation of one mission session; a restarted mission gets a new one so
/// stale events can never be confused with current ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionGeneration(pub u32);

/// Total order of emitted events: session, tick, source objective, then the
/// program sequence inside that objective (0 = the completion itself, then
/// action index + 1). Never hash or entity order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventKey {
    pub session: SessionGeneration,
    pub tick: Tick,
    pub source: SymbolId,
    pub sequence: u32,
}

impl EventKey {
    /// The key without the tick: identifies *what* was consumed so a retry on
    /// a later tick cannot repeat a reward or a capture.
    pub fn execution_key(&self) -> ExecutionKey {
        ExecutionKey {
            session: self.session,
            source: self.source,
            sequence: self.sequence,
        }
    }
}

/// Exactly-once identity of one event or action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExecutionKey {
    pub session: SessionGeneration,
    pub source: SymbolId,
    pub sequence: u32,
}

/// The mission's terminal state (contract: exactly one of these).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalState {
    Running,
    Succeeded,
    Failed,
    Aborted,
    /// Reached only through launch refusal ([`crate::ir::ValidationError`]),
    /// never from evaluation.
    Unsupported,
}

impl From<Outcome> for TerminalState {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Succeeded => Self::Succeeded,
            Outcome::Failed => Self::Failed,
            Outcome::Aborted => Self::Aborted,
        }
    }
}

/// Where a mission rule's evidence comes from — the source label F37-D-FU2
/// puts on the terminal-precedence rule and on the tick-ordering rule.
///
/// Neither variant can ever be [`ClaimStatus::VerifiedOriginal`]:
/// [`Self::MeasuredFromOriginalStatic`] is *static code evidence* read from
/// the owner-supplied decrypted executable at the addresses the owner's note
/// on Rally #589 cites (2026-10-05) — never a run of the original — and
/// [`Self::DesignedAndUnmeasured`] is project design the original has not been
/// measured on. [`RuleSource::claim`] maps the two to `inferred` and
/// `designed` structurally, so no record can upgrade either by assertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleSource {
    /// **measured-from-original**: settled by static analysis of the
    /// owner-supplied decrypted executable, cited per fact.
    MeasuredFromOriginalStatic,
    /// **designed-and-unmeasured**: a project design the original has not been
    /// measured on (contract: "A designed conservative policy can be used for
    /// synthetic tests only until verified").
    DesignedAndUnmeasured,
}

impl RuleSource {
    /// The spec-vocabulary label of the source.
    pub const fn label(self) -> &'static str {
        match self {
            Self::MeasuredFromOriginalStatic => "measured-from-original",
            Self::DesignedAndUnmeasured => "designed-and-unmeasured",
        }
    }

    /// The claim this source supports: static code evidence is `inferred`,
    /// design is `designed`. No arm can produce `verified_original`.
    pub const fn claim(self) -> ClaimStatus {
        match self {
            Self::MeasuredFromOriginalStatic => ClaimStatus::Inferred,
            Self::DesignedAndUnmeasured => ClaimStatus::Designed,
        }
    }
}

/// sha256 of the owner-supplied decrypted image the measured rules below were
/// read from (`$CS_GAME_DIR/crimson.decrypted.exe`, the owner's decryption of
/// `crimson.icd` `0e3b4724…9833b`) — the same image F16-F records in
/// `cs_sim::time`.
///
/// A hash of decrypted bytes, never the bytes: no executable, disassembly
/// listing or decompiled code is committed anywhere in this tree, and the
/// addresses on each fact are documentation, not code.
pub const ORIGINAL_IMAGE_SHA256: &str =
    "43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75";

/// The findings entry that records both rules: provenance, the addresses, how
/// this runtime differs from the original and every limitation below.
pub const TERMINAL_RULE_FINDINGS: &str =
    "docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md";

/// The findings entry that records the mission-end presentation this runtime
/// applies ([`MissionEndPresentation`], F37-D-FU5): which of the measured
/// facts it now follows, how the mission IR's single terminal action maps onto
/// the original's instant-outcome marker, and what closed
/// `f37.d.limit.terminal_branch_delay_and_sound`.
pub const TERMINAL_PRESENTATION_FINDINGS: &str =
    "docs/findings/2026-10-07-f37-d-fu5-mission-end-presentation.md";

/// One `f37.d.limit.*` claim: a place where this runtime does not follow a
/// measured rule, or where the evidence cannot settle a mapping at all.
///
/// Every entry names the content it affects and the task that resolves it
/// (or why nothing can), and it travels with the rule that produced it, so a
/// fidelity claim gated by one cannot be dropped silently — the F27-E.1
/// precedent, on the mission terminal rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleLimitation {
    /// Stable claim id, always `f37.d.limit.*`.
    pub id: &'static str,
    /// What is open, and about which rule.
    pub open: &'static str,
    /// The content the open part affects: what a claim must not cover.
    pub affected_content: &'static str,
    /// The Rally task (or the reason no task can) that closes it.
    pub resolving_task: &'static str,
}

/// Every limitation F37-D-FU2 records for the terminal-precedence and
/// tick-ordering rules.
///
/// The list shrinks only with its evidence: F37-D-FU5 (#731) removed
/// `f37.d.limit.terminal_branch_delay_and_sound` once
/// [`MissionEndPresentation`] implemented the branch, the end delay and the
/// sound/animation selection it named, pinned by the `accept_f37_d_fu5_*`
/// tests together with the findings update — the closure and what stays open
/// are recorded in [`TERMINAL_PRESENTATION_FINDINGS`].
pub const RULE_LIMITATIONS: &[RuleLimitation] = &[
    RuleLimitation {
        id: "f37.d.limit.one_completion_per_tick",
        open: "The original completes at most one objective per tick (the lowest \
               index whose condition holds); `MissionState::step` completes every \
               satisfied objective, in declaration order, on the same tick.",
        affected_content: "Any mission program run through `MissionState::step` in \
                           which two or more objectives satisfy on one tick — today \
                           the F37-D corpus programs and the AC01 two-objective probe, \
                           and from F38/F39 on every campaign mission lowered into \
                           this IR (starting with M01): its per-tick completions, \
                           event sequence and same-tick write conflicts differ from \
                           the original by one tick per extra completion.",
        resolving_task: "F37-D-FU3 (#729)",
    },
    RuleLimitation {
        id: "f37.d.limit.mission_countdown_tick_dt",
        open: "The countdown producer (`cs_sim::mission::Countdown`) decrements \
               by the session's declared fixed tick dt — one mission tick is one \
               `CountdownSpec::rate` dt — while the original decrements `[+4]` by \
               the *variable* per-frame game dt (`0x46c5f0`), which is clamped at \
               0.125 s, doubled under the 2x speed-up and frozen while paused. In \
               game-time seconds both count the same interval; in wall-clock time \
               the tick on which a countdown expires can differ — the standing \
               F16-F divergences, now reached through the countdown.",
        affected_content: "The wall-clock time (and so, at a rate the host did not \
                           match to the original's frame dt, the tick) at which a \
                           mission countdown expires, for every `MISSION_TIMER`-armed \
                           mission run under the project's designed fixed rate versus \
                           the original's variable frame dt — under frame pacing, the \
                           125 ms cap and the speed-up.",
        resolving_task: "F16-F-CAP (#721) and F16-F-SPEEDUP (#722) hold the dt \
                         divergences; VS-M01-RUNTIME (#359) is where a wired \
                         session's real rate is sourced",
    },
    RuleLimitation {
        id: "f37.d.limit.mission_countdown_end_guards",
        open: "Two guards of the original's expiry path stay unmodelled: \
               `0x440ad0()`'s global game-state byte, whose meaning is unknown and \
               which nothing in this tree can source, and the `remaining < -1.0f` \
               skip. The second cannot be reached in this layering — the producer \
               reports expiry the first tick `remaining <= 0.0`, the consumer ends \
               the mission on that same report and nothing defers it — but it is \
               recorded so a future deferring end path re-checks it rather than \
               inheriting a wrong poll.",
        affected_content: "Every mission countdown's expiry: whichever game states \
                           the byte gates may suppress or allow expiry in the \
                           original with no equivalent here, and any future end \
                           path that defers the report past one tick (the 3.0 s \
                           end delay F37-D-FU7 adds) must re-derive the -1.0f skip.",
        resolving_task: "F37-D-FU8 (#740)",
    },
    RuleLimitation {
        id: "f37.d.limit.mission_countdown_spec_sourcing",
        open: "The producer exists but its inputs are caller-declared, not yet \
               sourced: nothing lowers a control record's `MISSION_TIMER` field \
               — its seconds and its exact 7-byte `NOLOSS` second child — into \
               `cs_sim::mission::CountdownSpec`, so `no_loss` is as spelled rather \
               than read from a record, and no networked mission session exists \
               to source `network_game`. The timer directives are consumed by the \
               countdown, but no source adapter has yet been shown to spell \
               `RESET_TIMER`/`TIMER_ADJUST`/`END_TIMER`/`ADJUST_TIMER_WHEN_I_COMPLETE` \
               into `Action::Directive` emissions.",
        affected_content: "Every campaign mission whose control record spells \
                           `MISSION_TIMER` or `NOLOSS` (the record fields of every \
                           timed mission) and every networked mission session: \
                           until the lowering and the mode wiring exist, their \
                           countdowns run only where a caller declares the spec.",
        resolving_task: "the M01-LC lowering line (control-record fields into the \
                         IR; #726 and its stages) for the record field, \
                         VS-M01-RUNTIME (#359) for the session wiring, and the \
                         F56 mode line for a networked mission session",
    },
    RuleLimitation {
        id: "f37.d.limit.aborted_outcome",
        open: "The original has no Aborted outcome, so no original observation of its \
               precedence can exist. `PrecedencePolicy::MeasuredOriginal` keeps the \
               designed conservative ordering — an abort request beats the measured \
               results — because a mission torn down by the host must not be recorded \
               as a result a program asked for.",
        affected_content: "Any program or teardown that requests `Outcome::Aborted` \
                           together with a WON/LOST result on one tick (`Finish(Aborted)` \
                           sites and `MissionState::abort`): the recorded result of such \
                           a tick is designed, never measured, and gates any fidelity \
                           claim about an aborted or torn-down mission.",
        resolving_task: "none possible — owner decision only; the original has no such \
                         state, so no measurement can resolve it",
    },
    RuleLimitation {
        id: "f37.d.limit.frame_phase_and_player_down",
        open: "The original runs the world/node update first and the mission update \
               near the end of the frame, and skips the mission update entirely while \
               the player-down flag is set. This runtime has no frame: whoever calls \
               `MissionState::step` decides where the mission tick sits and whether it \
               runs at all.",
        affected_content: "Every mission tick while the player is down (respawn and \
                           downed windows) and every mission's position in the frame, \
                           for the wired mission path that drives this session — a \
                           caller that advances the mission every frame regardless \
                           diverges from the original's skip.",
        resolving_task: "VS-M01-RUNTIME (#359), which wires the mission session into \
                         the application frame",
    },
];

/// One fact about a rule the owner note settles, with the virtual addresses in
/// the analysed image that settle it and the limitations where this runtime
/// does not follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredFact {
    /// Stable id, e.g. `f37.rule.terminal_precedence.result_iff_won`.
    pub id: &'static str,
    /// What the original does.
    pub statement: &'static str,
    /// The virtual addresses that settle it (file offset = VA − 0x400000 for
    /// `.text`, `.rdata` and `.data` below VA 0x643000).
    pub addresses: &'static str,
    /// The [`RULE_LIMITATIONS`] ids where this runtime diverges; empty when
    /// this runtime follows the fact.
    pub limitations: &'static [&'static str],
}

/// One mission rule with its source label, the facts behind it and where the
/// evidence comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleLabel {
    /// Stable id for evidence records and fidelity gates.
    pub id: &'static str,
    /// `measured-from-original` or `designed-and-unmeasured`.
    pub source: RuleSource,
    /// The rule in one sentence.
    pub statement: &'static str,
    /// The findings entry, the image sha256 and the method behind the label.
    pub evidence: &'static str,
    /// The facts behind `statement`, in the order the original applies them.
    pub facts: &'static [MeasuredFact],
}

/// The terminal-precedence rule: what decides a mission's recorded result when
/// objectives, a countdown and the terminal check collide on one tick.
///
/// `measured-from-original` static code evidence (owner note on Rally #589,
/// 2026-10-05) — `inferred`, never `verified_original`.
pub const TERMINAL_PRECEDENCE_RULE: RuleLabel = RuleLabel {
    id: "f37.rule.terminal_precedence",
    source: RuleSource::MeasuredFromOriginalStatic,
    statement: "A mission countdown that expires ends the mission at once as a failure \
                and pre-empts any objective result of the same tick; at most one \
                objective completes per tick, the lowest index whose condition holds, \
                so through objectives success and failure cannot be requested on one \
                tick; the loss branch is tested before the win branch, and the recorded \
                result is success if and only if the WON flag is set. The original has \
                no Aborted outcome.",
    evidence: "owner note on Rally #589 (2026-10-05), static analysis of the \
               owner-supplied decrypted executable; see \
               docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md; \
               image sha256 \
               43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75; \
               static code evidence, never an original run and never verified_original",
    facts: &[
        MeasuredFact {
            id: "f37.rule.terminal_precedence.countdown_preempts",
            statement: "A mission countdown that expires ends the mission immediately \
                        with neither WON nor LOST (message 0x1772), before the \
                        objective passes of that tick, so it pre-empts any objective \
                        result in the same tick; the recorded result is failure.",
            addresses: "countdown object 0x71b468; expiry check 0x46c640 (skipped with \
                        NOLOSS and in network games); end call 0x463c30(1, 3.0); result \
                        0x4194e0",
            limitations: &[
                "f37.d.limit.mission_countdown_tick_dt",
                "f37.d.limit.mission_countdown_end_guards",
                "f37.d.limit.mission_countdown_spec_sourcing",
            ],
        },
        MeasuredFact {
            id: "f37.rule.terminal_precedence.one_completion_per_tick",
            statement: "At most one objective completes per tick — the lowest-indexed \
                        one whose condition holds. Later satisfied objectives wait for \
                        later ticks while their condition checks still run this tick, \
                        so through objectives success and failure can never be \
                        requested on one tick.",
            addresses: "CZMission::Update 0x46a490; scan from index 0, cursor 0x71c128 \
                        only normalised; the completed-this-tick flag is tested at \
                        0x46a94c after the condition checks",
            limitations: &["f37.d.limit.one_completion_per_tick"],
        },
        MeasuredFact {
            id: "f37.rule.terminal_precedence.loss_branch_before_win",
            statement: "The terminal check tests LOST before WON: the loss branch \
                        chooses the end delay (0.1 s when an INSTANTLOSS fired that \
                        tick, else 3.0 s) and OBJECTIVES_LOST_SOUND, the win branch the \
                        same for OBJECTIVES_WON_SOUND. Which branch ran does not decide \
                        the recorded result.",
            addresses: "LOST 0x46af7a then WON 0x46afad; OBJECTIVES_LOST_SOUND +0xc74, \
                        OBJECTIVES_WON_SOUND +0xc70",
            limitations: &[],
        },
        MeasuredFact {
            id: "f37.rule.terminal_precedence.result_iff_won",
            statement: "The recorded mission result is success if and only if the WON \
                        flag is set; the LOST flag has no other reader. When both flags \
                        are set together the loss branch still plays, but success is \
                        what is recorded, what the end animation shows and which \
                        mission sound the end call picks.",
            addresses: "result 0x4194e0; flags [mission+0xc58] WON / [mission+0xc5c] \
                        LOST; end call 0x463c30 picks MISSION_WON_SOUND +0xc78 when WON \
                        is set, else MISSION_LOST_SOUND +0xc7c; end animation 0x46ba10 \
                        (WIN_ANIM +0x6e4 when WON, else LOSS_ANIM +0x6e8)",
            limitations: &[],
        },
        MeasuredFact {
            id: "f37.rule.terminal_precedence.no_aborted",
            statement: "The original has no Aborted outcome: an objective's outcome \
                        kind is LOST=1, WON=2, INSTANTWIN=3, INSTANTLOSS=4 or none=0.",
            addresses: "outcome kind at [obj+0x554], objective records 0x5e4 bytes at \
                        [mission+0xc4c] with the count at +0xc48",
            limitations: &["f37.d.limit.aborted_outcome"],
        },
    ],
};

/// The tick-ordering rule: what runs inside one tick and in what order.
///
/// `measured-from-original` static code evidence (owner note on Rally #589,
/// 2026-10-05) — `inferred`, never `verified_original`. It is a different
/// question from the *observation* order of the recreated event stream, which
/// is [`EVENT_OBSERVATION_ORDER_RULE`].
pub const TICK_ORDERING_RULE: RuleLabel = RuleLabel {
    id: "f37.rule.tick_ordering",
    source: RuleSource::MeasuredFromOriginalStatic,
    statement: "One tick is one rendered frame: the world/node update runs first \
                (planes, AI, weapons, damage, effects) and the mission update runs \
                near the end of the frame, skipped entirely while the player is down. \
                Inside the mission update: helper updates, the countdown, mission \
                time, the objective lifecycle timers in index order, then the \
                completion scan in index order — at most one completion, lowest index, \
                with its effects in a fixed order — then the WON/LOST recount, then \
                the terminal check with loss before win.",
    evidence: "owner note on Rally #589 (2026-10-05), static analysis of the \
               owner-supplied decrypted executable; see \
               docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md; \
               image sha256 \
               43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75; \
               static code evidence, never an original run and never verified_original",
    facts: &[
        MeasuredFact {
            id: "f37.rule.tick_ordering.mission_update_position",
            statement: "The flight frame runs the world/node update first and calls \
                        the mission update near its end; the mission update does not \
                        run at all while the player-down flag is set.",
            addresses: "flight frame 0x4a0220; world/node update 0x4d0010; mission \
                        update call 0x4a09c3; player-down flag [player+0x91d]",
            limitations: &["f37.d.limit.frame_phase_and_player_down"],
        },
        MeasuredFact {
            id: "f37.rule.tick_ordering.per_tick_order",
            statement: "Inside the mission update: helper updates, then the countdown, \
                        then mission time, then the objective lifecycle timers in index \
                        order, then the completion scan in index order with at most one \
                        completion and its effects in a fixed order, then the WON/LOST \
                        recount over all objectives, then the terminal check with loss \
                        before win.",
            addresses: "CZMission::Update 0x46a490; helpers 0x46cdf0, 0x46c870; \
                        countdown 0x46c640; mission time [mission+0x6f0]; lifecycle \
                        timers (dormant [obj+0x5d0], awake 0x5d4, nap 0x5d8, end 0x5dc, \
                        gate [obj+0x10]); completion scan cursor 0x71c128 with the flag \
                        at 0x46a94c; completion effects 0x46a630; terminal check \
                        0x46af7a then 0x46afad",
            limitations: &[
                "f37.d.limit.mission_countdown_tick_dt",
                "f37.d.limit.mission_countdown_end_guards",
                "f37.d.limit.mission_countdown_spec_sourcing",
                "f37.d.limit.one_completion_per_tick",
            ],
        },
    ],
};

/// The *observation* order of the recreated event stream — the question the
/// original cannot answer, because it emits no such stream.
///
/// `designed-and-unmeasured`: the key is fixed by the shared contract
/// ("stable ordering keys use session/tick/source/program sequence, not hash
/// map or entity iteration order"), not by anything measured from the
/// original, and it is the order `EventKey` sorts by.
pub const EVENT_OBSERVATION_ORDER_RULE: RuleLabel = RuleLabel {
    id: "f37.rule.event_observation_order",
    source: RuleSource::DesignedAndUnmeasured,
    statement: "The recreated runtime observes one tick's events in `EventKey` order — \
                session, tick, source symbol, program sequence — while execution \
                follows declaration order and then the deferred queue; neither order is \
                a claim about the original.",
    evidence: "docs/contracts/SCRIPT-MISSION.md, \"Objective event ordering\"; \
               docs/findings/2026-10-03-f37-d-adversarial-corpus-and-ordering-probes.md; \
               no addresses: nothing about this order was measured from the original, \
               which emits no event stream",
    facts: &[],
};

/// How simultaneous terminal requests on one tick are resolved.
///
/// Two policies, each carrying its own source label ([`RuleSource`]):
///
/// * [`Self::MeasuredOriginal`] is the default: the terminal precedence the
///   owner measured in the original's mission runtime — the recorded result is
///   **success if and only if the WON flag is set**, so when both a success
///   and a failure are requested on one tick the success stands, and an abort
///   request keeps the designed ordering because the original has no such
///   outcome ([`TERMINAL_PRECEDENCE_RULE`]).
/// * [`Self::SyntheticConservative`] is the designed `Aborted` > `Failed` >
///   `Succeeded` policy F37-A wrote. The contract allows a designed policy
///   "for synthetic tests only until verified"; the rule has been measured
///   (static code evidence, `inferred`), so the runtime no longer selects it
///   by default, and it stays selectable for the synthetic tests that want the
///   conservative answer.
///
/// Both are labelled and neither can be `verified_original`: see
/// [`RuleSource::claim`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecedencePolicy {
    /// `Aborted` beats `Failed` beats `Succeeded`. Designed and unmeasured —
    /// synthetic tests only.
    SyntheticConservative,
    /// The measured original precedence: success iff WON, an abort request
    /// ordered above the measured results because the original has no abort.
    MeasuredOriginal,
}

impl PrecedencePolicy {
    /// The source label of the rule this policy applies.
    pub const fn source(self) -> RuleSource {
        match self {
            Self::SyntheticConservative => RuleSource::DesignedAndUnmeasured,
            Self::MeasuredOriginal => RuleSource::MeasuredFromOriginalStatic,
        }
    }

    fn pick(self, requested: &BTreeSet<Outcome>) -> Option<Outcome> {
        match self {
            // `Outcome`'s order is Succeeded < Failed < Aborted.
            Self::SyntheticConservative => requested.iter().next_back().copied(),
            Self::MeasuredOriginal => {
                if requested.contains(&Outcome::Aborted) {
                    // The original has no Aborted outcome, so no measurement
                    // can order it: the designed conservative answer stands
                    // (a torn-down mission must not be recorded as a result a
                    // program asked for). `f37.d.limit.aborted_outcome`.
                    return Some(Outcome::Aborted);
                }
                // The recorded result is success iff the WON flag is set: when
                // both were requested, the success is the one that is recorded
                // even though the loss branch is the one that plays.
                if requested.contains(&Outcome::Succeeded) {
                    return Some(Outcome::Succeeded);
                }
                // Otherwise the single failure, or nothing was requested.
                requested.iter().next_back().copied()
            }
        }
    }
}

/// One tick's mission-countdown input: the measured expiry decision's
/// inputs, supplied by whoever runs the countdown (F37-D-FU4).
///
/// The original's countdown is a global object at `0x71b468`: while it runs
/// (`[+0x10]`) `[+4]` holds remaining **seconds** and is decremented by the
/// frame's dt (`0x46c5f0`), `[+0x14]` is the `NOLOSS` flag, and the poll
/// `0x46c640` reports expiry iff remaining `<= 0.0f`. The producer is
/// `cs_sim::mission::Countdown` — it keeps the seconds counter `[+4]` and
/// decrements by the session's declared fixed tick dt, never a guessed
/// conversion (`f37.d.limit.mission_countdown_tick_dt`); this layer owns
/// only the *decision* the poll feeds: an expiry ends the mission at once
/// as a failure, before that tick's objective passes
/// (`f37.rule.terminal_precedence.countdown_preempts`, owner note on Rally
/// #589 — static code evidence, never an original run).
///
/// `Default` is [`Self::NONE`], "no countdown expired", which is what
/// [`MissionState::step`] passes: a session nobody feeds an expiry into can
/// never end by timeout, exactly as before this input existed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MissionCountdown {
    /// The countdown reached zero on this tick — the poll's
    /// `remaining <= 0` while the timer is running (`0x46c640`).
    pub expired: bool,
    /// The mission record's `NOLOSS` flag: the exact 7-byte `NOLOSS` child of
    /// `MISSION_TIMER` sets it (`0x46c540`, field `[+0x14]`), and the poll
    /// then zeroes the remaining time and reports **no** expiry (`0x46c640`).
    pub no_loss: bool,
    /// A network game: the original skips the expiry check in network games
    /// (owner note on Rally #589). Session-constant, carried here so the
    /// measured decision and both of its exclusions live in one place.
    pub network_game: bool,
}

impl MissionCountdown {
    /// No countdown expired this tick: what [`MissionState::step`] passes and
    /// what every caller that has no countdown should pass.
    pub const NONE: Self = Self {
        expired: false,
        no_loss: false,
        network_game: false,
    };

    /// The measured decision: expiry ends the mission **unless** one of the
    /// two measured exclusions holds — NOLOSS (the poll itself reports no
    /// expiry, `0x46c640`) and the network-game skip (owner note on Rally
    /// #589). `expired == false` is the stopped or still-running timer, which
    /// the poll also reports as no expiry.
    pub const fn preempts(self) -> bool {
        self.expired && !self.no_loss && !self.network_game
    }
}

/// Measured end-screen delay for a mission whose terminal tick fired an
/// `INSTANTWIN`/`INSTANTLOSS`: **0.1 s**.
///
/// `measured-from-original` static code evidence (owner note on Rally #589,
/// 2026-10-05) — the instant-outcome completion sets the byte the terminal
/// check reads, and the terminal check (`LOST 0x46af7a` then `WON 0x46afad`)
/// passes 0.1 s when it is set. `inferred`, never `verified_original`.
pub const INSTANT_END_DELAY: Duration = Duration::from_millis(100);

/// Measured end-screen delay in every other case: **3.0 s** — the terminal
/// check's answer when no instant outcome fired that tick, and the value the
/// countdown-expiry path passes to the end call `0x463c30(1, 3.0)` outright.
///
/// Same evidence and same claim status as [`INSTANT_END_DELAY`].
pub const STANDARD_END_DELAY: Duration = Duration::from_secs(3);

/// Which branch of the original's terminal check selected the mission-end
/// presentation. The check tests **LOST before WON** (`0x46af7a` then
/// `0x46afad`), and which branch ran never decides the recorded result
/// ([`MissionEndPresentation::result`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalBranch {
    /// The LOST flag was set — even when WON is set too, the loss branch is
    /// the one tested first and therefore the one that runs.
    Loss,
    /// The LOST flag was clear and the WON flag set: the win branch runs.
    Win,
}

/// The `OBJECTIVES_LOST_SOUND` (`+0xc74`) or `OBJECTIVES_WON_SOUND` (`+0xc70`)
/// slot the branch selects. Whether the mission fills that slot with a handle
/// is mission content (a null handle plays nothing), not this layer's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectivesCue {
    ObjectivesLostSound,
    ObjectivesWonSound,
}

/// The `MISSION_WON_SOUND` (`+0xc78`) or `MISSION_LOST_SOUND` (`+0xc7c`) slot
/// the end call `0x463c30` selects from the **WON flag** — from the flag, not
/// from the recorded result, so the two reads stay independent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissionCue {
    MissionWonSound,
    MissionLostSound,
}

/// The end animation `0x46ba10` selects from the WON flag: `WIN_ANIM`
/// (`+0x6e4`) when WON is set, `LOSS_ANIM` (`+0x6e8`) otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndAnimation {
    WinAnim,
    LossAnim,
}

/// How long the end screen waits: [`EndDelay::Instant`] when an
/// `INSTANTWIN`/`INSTANTLOSS` fired on the terminal tick, [`EndDelay::Standard`]
/// otherwise (measured pair, see [`INSTANT_END_DELAY`]/[`STANDARD_END_DELAY`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndDelay {
    Instant,
    Standard,
}

impl EndDelay {
    /// The measured wait this delay names: 0.1 s or 3.0 s.
    #[must_use]
    pub const fn seconds(self) -> Duration {
        match self {
            Self::Instant => INSTANT_END_DELAY,
            Self::Standard => STANDARD_END_DELAY,
        }
    }
}

/// What one terminal tick selected for mission-end presentation (F37-D-FU5):
/// the branch, the end delay, the two sound slots and the end animation,
/// beside the recorded result.
///
/// Built from three **independent** reads of the same tick, exactly as the
/// original reads them, so a caller cannot collapse them into one:
///
/// * `lost` — the branch is chosen by testing **LOST before WON**
///   (`0x46af7a` then `0x46afad`): [`Self::branch`] is [`TerminalBranch::Loss`]
///   whenever the LOST flag is set, even when the WON flag is set too, and
///   `None` when neither flag is set (the original then plays no
///   `OBJECTIVES_*_SOUND` at all — that is the countdown-expiry shape);
/// * `won` — the mission sound and the animation follow the **WON flag** alone
///   (`0x463c30` picks `MISSION_WON_SOUND` when WON is set, `0x46ba10` picks
///   `WIN_ANIM`), never the branch and never the result;
/// * `instant` — the end delay is 0.1 s when an `INSTANTWIN`/`INSTANTLOSS`
///   fired that tick, else 3.0 s;
/// * `result` — what the session recorded, success iff WON (`0x4194e0`), so
///   **the loss branch may run while the recorded result is the success**.
///
/// This is a *selection*, not a playback: it says which of the mission's
/// `*_SOUND` slots and which animation the original would reach for, and how
/// long the end screen would wait. Filling those slots with handles is mission
/// content, and playing or showing anything needs the audio/presentation
/// stages and their own capabilities (`docs/00-SCOPE.md`); nothing here claims
/// an audible or visual result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissionEndPresentation {
    /// The branch the terminal check took, `None` when neither flag was set.
    pub branch: Option<TerminalBranch>,
    /// The mission-level sound slot the end call selects from the WON flag.
    pub mission_cue: MissionCue,
    /// The end animation the WON flag selects.
    pub animation: EndAnimation,
    /// How long the end screen waits.
    pub end_delay: EndDelay,
    /// The recorded result: success iff the WON flag is set (`0x4194e0`).
    pub result: TerminalState,
}

impl MissionEndPresentation {
    /// Builds the presentation of one terminal tick from its four measured
    /// inputs: the LOST flag, the WON flag, whether an instant outcome fired
    /// that tick, and the result the session recorded.
    ///
    /// The branch test is written LOST first, mirroring `0x46af7a` then
    /// `0x46afad`, so the order cannot be reversed by accident.
    #[must_use]
    pub fn new(lost: bool, won: bool, instant: bool, result: TerminalState) -> Self {
        let branch = if lost {
            Some(TerminalBranch::Loss)
        } else if won {
            Some(TerminalBranch::Win)
        } else {
            None
        };
        Self {
            branch,
            mission_cue: if won {
                MissionCue::MissionWonSound
            } else {
                MissionCue::MissionLostSound
            },
            animation: if won {
                EndAnimation::WinAnim
            } else {
                EndAnimation::LossAnim
            },
            end_delay: if instant {
                EndDelay::Instant
            } else {
                EndDelay::Standard
            },
            result,
        }
    }

    /// The `OBJECTIVES_*_SOUND` slot the branch selects, `None` when neither
    /// flag was set — the branch's own half of the presentation.
    #[must_use]
    pub const fn objectives_cue(&self) -> Option<ObjectivesCue> {
        match self.branch {
            Some(TerminalBranch::Loss) => Some(ObjectivesCue::ObjectivesLostSound),
            Some(TerminalBranch::Win) => Some(ObjectivesCue::ObjectivesWonSound),
            None => None,
        }
    }
}

/// What the simulation tells the mission about actors this tick. An actor
/// absent from the map matches no [`Condition::ActorIs`] — so an actor that
/// left mission accounting is absent, not defaulted to another state.
///
/// The evaluator never invents these facts: the simulation's authoritative
/// actor record writes them, which is `cs_sim`'s actor-fact table —
/// registration writes [`ActorState::Alive`], a measured lifecycle
/// transition writes `Dead`, `Captured` or `Despawned`, and a state with no
/// measured producer is refused by name rather than written.
///
/// The world-shaped conditions
/// ([`Condition::InactiveMembers`], [`Condition::EnemyGroupDepletion`],
/// [`Condition::Travelers`], [`Condition::AnimationStates`]) read the other
/// maps here, each keyed exactly as the record spells its operands:
///
/// * `objectives` — the numbered block's measured lifecycle state
///   ([`ObjectiveLifecycle`]), written by `cs_sim`'s block-lifecycle table
///   from the record's own `BEGIN_DORMANT` spelling and the mission clock;
/// * `members` — a named member chain ([`crate::ir::MemberName`]) with its
///   in-play presence and world position;
/// * `groups` / `generators` — the living count of an AI group and the
///   pending-spawn count a named generator still owes;
/// * `animations` — an animation name's current state byte, the measured
///   `UNDEFINED` 0 … `INVALID_AND_RUNNING` 6 table (finding C).
///
/// Absence is always "not observed", never a default: an unpopulated map
/// makes its conditions answer `false`, so a session whose facts nobody
/// populated completes nothing instead of completing everything.
///
/// `PartialEq` only, not `Eq`: [`MemberFact`] carries a world position, and
/// a float position is not an equivalence relation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MissionFacts {
    pub actors: BTreeMap<ActorId, ActorState>,
    /// Each numbered block's lifecycle state, keyed by its zero-based record
    /// index — the index `TICK_DEPENDS_ON_OBJ` stores.
    pub objectives: BTreeMap<u32, ObjectiveLifecycle>,
    /// The named world members the record's operands resolve to, keyed by
    /// the chain exactly as spelled.
    pub members: BTreeMap<MemberName, MemberFact>,
    /// Group id → how many of its members are still in play (not despawned).
    /// A group nobody recorded is unknown, not empty.
    pub groups: BTreeMap<i32, u32>,
    /// Generator name → its pending-spawn count. A name that does not
    /// resolve owes **0** — measured: the original logs `Cannot find
    /// generator %s` and adds nothing.
    pub generators: BTreeMap<String, u32>,
    /// Animation name → its current state byte (measured table: `UNDEFINED`
    /// 0, `DORMANT` 1, `RUNNING` 2, `EXECUTED` 3, `INVALID` 4, `CORRUPT` 5,
    /// `INVALID_AND_RUNNING` 6).
    pub animations: BTreeMap<String, u32>,
}

impl MissionFacts {
    /// Folds one fact table into another, field by field: `other` wins on a
    /// key both carry, since it is the later observation.
    ///
    /// This is how several writers compose — the actor-fact table, the
    /// block-lifecycle table and the world-side member/group/animation
    /// tables each build their own [`MissionFacts`] and a caller folds them
    /// before advancing a tick. The key spaces are distinct, so a collision
    /// means two writers disagree about one key and the later one stands.
    pub fn absorb(&mut self, other: MissionFacts) {
        self.actors.extend(other.actors);
        self.objectives.extend(other.objectives);
        self.members.extend(other.members);
        self.groups.extend(other.groups);
        self.generators.extend(other.generators);
        self.animations.extend(other.animations);
    }
}

/// The measured lifecycle state of one numbered block — the original's
/// `+0x5c8` vocabulary, all four values (finding B): 0 dormant, 1 awake,
/// 2 napping, 3 done.
///
/// Only `Awake` is what [`Condition::ObjectiveAwake`] reads: the pass-2 gate
/// admits a block exactly while it is awake.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectiveLifecycle {
    Dormant,
    Awake,
    Napping,
    Done,
}

/// Whether a named world member exists and carries the in-play bit.
///
/// The original tests one flag for both questions: an `INACTIVE<n>` row is
/// counted when its object **exists** (`!= 0`) and its `+0x24` bit 4 is
/// **clear**, and a `TRAVELERS` subject is tested only when its object
/// exists and the bit is **set** (finding B). Three states keep "the name did
/// not resolve" apart from "it resolved and is out of play", which is the
/// distinction both evaluators turn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberPresence {
    /// The name did not resolve to an object.
    Missing,
    /// The object exists and is in play (bit set).
    InPlay,
    /// The object exists and is no longer in play (bit clear).
    OutOfPlay,
}

/// One named world member's observed row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemberFact {
    /// Whether the name resolved and whether it is in play.
    pub presence: MemberPresence,
    /// The member's world position, for the radius comparisons. Only
    /// [`Condition::Travelers`] reads it; a member recorded without a
    /// meaningful position should be recorded with `[0.0; 3]`, which is
    /// what the original's zeroed record holds for an absent anchor.
    pub position: [f64; 3],
}

/// What an emitted event is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    ObjectiveCompleted,
    RewardGranted(ContentId),
    TerminalRequested(Outcome),
}

/// One ordered event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionEvent {
    pub key: EventKey,
    pub kind: EventKind,
}

/// One measured directive operation the program asked the host to perform —
/// the documented host effect of [`Action::Directive`].
///
/// `EventKind` is F37's fixed vocabulary of mission-state *observations* —
/// an objective completing, a reward intent, a terminal request — and a
/// measured directive is none of those. The simulation matches the enum
/// exhaustively (`cs_sim::mission` applies each kind), so this crate cannot
/// grow it without changing a crate this stage does not own. A directive is
/// instead emitted to the session's **directive log**
/// ([`MissionState::directives`]): exactly once per execution key under the
/// same `consumed` guard [`MissionEvent`]s use, in `EventKey` order, with the
/// bound call's arguments carried field for field — nested lists stay
/// nested. It is a real emission, not a no-op: the host applies each entry
/// it has not already applied, deduplicating by execution key exactly as it
/// does for events.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectiveEmission {
    /// When and where in the program the emission was produced; its
    /// [`EventKey::execution_key`] is the exactly-once identity the host
    /// deduplicates on.
    pub key: EventKey,
    /// The measured operation the binding declared for the call.
    pub operation: DirectiveOperation,
    /// The call's own arguments as the site spelled them, nested structure
    /// intact — never flattened into a positional order the site did not
    /// write.
    pub args: Vec<Value>,
}

/// Result of one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickResult {
    /// The session generation that produced this result, stamped by
    /// [`MissionState::step`]. The host refuses a result whose session is not
    /// its own before touching any of it, so a result with no events still
    /// carries its provenance.
    pub session: SessionGeneration,
    pub tick: Tick,
    /// Sorted by [`EventKey`].
    pub events: Vec<MissionEvent>,
    pub terminal: TerminalState,
    /// Set when a bound stopped the tick early: the contract's budget
    /// diagnostic (mission id, program locator, trace) plus `events`, the
    /// short event trace of everything that ran before the stop.
    pub stop: Option<StopReason>,
}

/// Why one tick stopped before its work was done. Never a mission failure:
/// the interrupted list is re-queued at its next action, the work spent is
/// committed, and a later tick resumes it. `Running` keeps flowing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The per-tick work budget ran out.
    WorkBudget { at: ProgramLocator, spent: u64 },
    /// A `Schedule`/`Reschedule` could not enqueue because the pending queue
    /// already holds `WorkLimits::max_pending_items` items; the enqueueing
    /// action is retried on the next tick.
    PendingLimit { at: ProgramLocator, queued: usize },
    /// The session's work-item ordinal space ran out (~66 million scheduled
    /// items): the locator names the schedule that could not take a key.
    SequenceExhausted { at: ProgramLocator },
}

/// Why a tick was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickError {
    /// The tick is not after the last evaluated one (a replay or retry).
    NotAdvancing { last: Tick, given: Tick },
}

/// One scheduled work item as the save record sees it.
///
/// The record keeps the item's action list, not a reference into the program:
/// a budget stop defers the *unexecuted suffix* of an action list, so the
/// resume point cannot be re-derived from program data alone. Program data is
/// immutable and shared, so carrying the deferred text in the record costs
/// space and never a second source of truth.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledWork {
    /// The program symbol the item's events and diagnostics attribute to.
    pub source: SymbolId,
    /// Session-unique ordinal that keeps this item's event keys distinct.
    pub ordinal: u32,
    /// First tick the item is eligible on.
    pub due: Tick,
    /// First action not yet executed.
    pub next: usize,
    pub actions: Vec<Action>,
}

/// One queued item as an observer sees it: when it fires and how much of it is
/// left. The same view is available on live state
/// ([`MissionState::pending_timers`]) and on a save record
/// ([`MissionStateSnapshot::pending_timers`]), so a caller can compare what was
/// pending before and after a restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTimer {
    /// The program symbol the item's events attribute to.
    pub source: SymbolId,
    /// The tick the item becomes eligible on.
    pub due: Tick,
    /// Ticks from the session's last evaluated tick to [`PendingTimer::due`];
    /// `0` means "eligible on the tick evaluated last", which is what a
    /// work-budget stop leaves behind.
    pub remaining: u64,
    /// First action not yet executed.
    pub next: usize,
    /// How many actions the item holds in total.
    pub actions_total: usize,
}

impl PendingTimer {
    /// Actions still to run: a resumed item's tail.
    pub fn actions_remaining(&self) -> usize {
        self.actions_total - self.next
    }
}

/// Builds one [`PendingTimer`] view from a queue entry and the session's last
/// evaluated tick.
fn pending_timer(
    source: SymbolId,
    due: Tick,
    next: usize,
    actions_total: usize,
    last_tick: Option<Tick>,
) -> PendingTimer {
    let remaining = last_tick.map_or(due.0, |last| due.0.saturating_sub(last.0));
    PendingTimer {
        source,
        due,
        remaining,
        next,
        actions_total,
    }
}

/// The gameplay-relevant execution state of one mission session, as one
/// versioned record (contract, "IR requirements": "state snapshot/restore
/// must preserve all gameplay-relevant pieces or declare mid-mission save
/// unsupported").
///
/// It carries every piece that can change a later observation: variable
/// values, the latched objectives, the consumed execution keys, the terminal
/// state, the last evaluated tick, the whole pending queue in drain order with
/// its eligibility ticks, the item-ordinal counter, the RNG draw count and
/// the directive emissions already produced. Mid-mission save is therefore
/// supported; nothing is declared unsupported.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionStateSnapshot {
    pub version: u32,
    /// The mission this state belongs to; a record may only be restored into
    /// the program that produced it.
    pub mission: ContentId,
    pub session: SessionGeneration,
    /// In `SymbolId` order, which is the live map's own order.
    pub variables: Vec<(SymbolId, Value)>,
    /// Latched objectives in `SymbolId` order.
    pub completed: Vec<SymbolId>,
    /// Every execution key already emitted, in key order.
    pub consumed: Vec<ExecutionKey>,
    /// The session's directive emissions so far, in `EventKey` order — the
    /// record of host effects already produced, so a restore keeps it and
    /// the host never applies one twice.
    pub directives: Vec<DirectiveEmission>,
    pub terminal: TerminalState,
    /// The mission-end presentation of the tick that ended the session, or
    /// `None` while it runs. It travels in the record because it is state the
    /// session produced — which branch, delay, sound slots and animation the
    /// terminal tick selected — and a restore must hand the host the same
    /// answer a session that never stopped would (F37-D-FU5).
    pub presentation: Option<MissionEndPresentation>,
    /// The last evaluated tick; a restored session still refuses to re-evaluate
    /// it ([`TickError::NotAdvancing`]).
    pub last_tick: Option<Tick>,
    /// The precedence policy this session resolves terminal requests with. It
    /// travels in the record because the rule decides the result: a restore
    /// must not swap a session from the measured policy to the designed one
    /// (or back) half-way through.
    pub policy: PrecedencePolicy,
    /// The whole pending queue in `(due, enqueue)` order — the exact order a
    /// drain would rebuild it in.
    pub pending: Vec<ScheduledWork>,
    /// Ordinal the next scheduled item will take.
    pub next_item_ordinal: u32,
    pub limits: WorkLimits,
    /// Draws taken from the session's RNG stream.
    pub rng_draws: u64,
}

impl MissionStateSnapshot {
    /// The pending queue as [`PendingTimer`]s, in the same `(due, enqueue)`
    /// order and relative to the record's own `last_tick`.
    pub fn pending_timers(&self) -> Vec<PendingTimer> {
        self.pending
            .iter()
            .map(|item| {
                pending_timer(
                    item.source,
                    item.due,
                    item.next,
                    item.actions.len(),
                    self.last_tick,
                )
            })
            .collect()
    }
}

/// An internal inconsistency in a save record. The record is data from outside
/// the process, so every field is checked instead of trusted: restoring a
/// record that broke these invariants would silently change which events
/// fire, which is the one thing an execution key exists to prevent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreDefect {
    /// Two queued items share one ordinal, so their event keys can collide and
    /// the exactly-once guard would swallow one item's events.
    DuplicateOrdinal { ordinal: u32 },
    /// An item's ordinal is at or above `next_item_ordinal`, so this session
    /// never allocated it.
    OrdinalNotAllocated { ordinal: u32 },
    /// An item's resume cursor points outside its action list.
    ActionCursor { next: usize, actions: usize },
    /// The queue is not in `(due, enqueue)` order, so draining it would not
    /// reproduce the order the record claims.
    PendingOrder { previous: Tick, given: Tick },
    /// A consumed execution key — or a directive emission's key — belongs to
    /// another session.
    ForeignExecutionKey { session: SessionGeneration },
    /// The directive log is not in `EventKey` order, or repeats an execution
    /// key: no live session produced it that way.
    DirectiveOrder,
    /// A directive emission's execution key was never consumed, so no live
    /// session emitted it — the emit path inserts the key and the emission
    /// together.
    DirectiveNotConsumed { key: ExecutionKey },
    /// The record drops a variable the program declares. Every state of this
    /// program holds all of them, and a condition on a missing variable is
    /// false, so an absent one would silently disarm the mission.
    MissingVariable { symbol: SymbolId },
    /// The record carries two values for one variable, so which one a restore
    /// installed would be an accident of order.
    DuplicateVariable { symbol: SymbolId },
    /// The record carries more queued items than the session's queue may hold:
    /// the live path enforces that cap and a restore must not route around it.
    PendingQueueTooLong { count: usize, allowed: usize },
    /// The record's terminal state and its mission-end presentation disagree
    /// (F37-D-FU5): a session holds a presentation exactly while it is
    /// terminal, and that presentation's recorded result is the record's own
    /// terminal state. A record that breaks either half is not one a live
    /// session wrote, and restoring it would decide the end screen from data
    /// the tick that ended the mission never selected.
    PresentationMismatch,
}

/// Why a save record could not be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// The record was written by another snapshot version.
    SnapshotVersion { found: u32 },
    /// The record belongs to another mission.
    MissionMismatch {
        expected: ContentId,
        found: ContentId,
    },
    /// A variable in the record is not declared by the program.
    UnknownVariable { symbol: SymbolId },
    /// A latched objective in the record is not declared by the program.
    UnknownObjective { symbol: SymbolId },
    /// A record value's type differs from the program's declaration, so the
    /// restore would install a value no condition can compare.
    TypeMismatch {
        symbol: SymbolId,
        expected: ValueType,
        found: ValueType,
    },
    /// The record is internally inconsistent.
    Corrupt { defect: RestoreDefect },
    /// The session took more draws than [`MAX_RNG_REPLAY_DRAWS`], so rewinding
    /// its RNG stream would exceed the restore work bound.
    RngReplayTooLong { draws: u64 },
    /// The record's per-tick work budget is below [`MIN_WORK_PER_TICK`], so the
    /// restored session could never execute an action: every tick would admit
    /// work items and run none of them, so no reward is granted, no terminal
    /// request is made and the deferred queue only grows. A record may tighten
    /// the engine's bounds, but not to a session that cannot progress.
    WorkBudgetTooSmall { found: u64 },
    /// The record's item-ordinal counter is past the sequence space, further
    /// than [`FIRST_EXHAUSTED_ITEM_ORDINAL`] — the value the live allocator
    /// stops *at*. No live session writes such a counter, so it is record data
    /// from nowhere; every later `Schedule` would stop with
    /// [`StopReason::SequenceExhausted`] and no deferred work would ever run
    /// again. The exhausted marker itself restores: that is the state a session
    /// is in once its space runs out, and it must stay saveable.
    SequenceSpaceExhausted { ordinal: u32 },
    /// A queued item's action list is one this program would refuse: an
    /// undecodable instruction, an empty `Draw` range or a write to a variable
    /// the program does not declare. Restoring it would hand the evaluator an
    /// action validation guarantees cannot occur.
    DeferredActions {
        ordinal: u32,
        error: ValidationError,
    },
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotVersion { found } => {
                write!(
                    f,
                    "snapshot version {found} unsupported (expected {SNAPSHOT_VERSION})"
                )
            }
            Self::MissionMismatch { expected, found } => {
                write!(f, "snapshot is for {found}, not {expected}")
            }
            Self::UnknownVariable { symbol } => {
                write!(
                    f,
                    "snapshot variable #{} is not declared by the program",
                    symbol.0
                )
            }
            Self::UnknownObjective { symbol } => {
                write!(
                    f,
                    "snapshot objective #{} is not declared by the program",
                    symbol.0
                )
            }
            Self::TypeMismatch {
                symbol,
                expected,
                found,
            } => write!(
                f,
                "snapshot variable #{} holds {found:?}, program declares {expected:?}",
                symbol.0
            ),
            Self::Corrupt { defect } => match defect {
                RestoreDefect::DuplicateOrdinal { ordinal } => {
                    write!(f, "two pending items share ordinal {ordinal}")
                }
                RestoreDefect::OrdinalNotAllocated { ordinal } => {
                    write!(f, "pending item ordinal {ordinal} was never allocated")
                }
                RestoreDefect::ActionCursor { next, actions } => {
                    write!(f, "pending item resumes at action {next} of {actions}")
                }
                RestoreDefect::PendingOrder { previous, given } => write!(
                    f,
                    "pending queue is not due-ordered: tick {} follows tick {}",
                    given.0, previous.0
                ),
                RestoreDefect::ForeignExecutionKey { session } => {
                    write!(f, "execution key belongs to session {}", session.0)
                }
                RestoreDefect::DirectiveOrder => {
                    write!(
                        f,
                        "directive log is not key-ordered or repeats an execution key"
                    )
                }
                RestoreDefect::DirectiveNotConsumed { key } => {
                    write!(
                        f,
                        "directive emission for objective #{} seq {} was never consumed",
                        key.source.0, key.sequence
                    )
                }
                RestoreDefect::MissingVariable { symbol } => {
                    write!(
                        f,
                        "snapshot has no value for declared variable #{}",
                        symbol.0
                    )
                }
                RestoreDefect::DuplicateVariable { symbol } => {
                    write!(f, "snapshot holds variable #{} twice", symbol.0)
                }
                RestoreDefect::PendingQueueTooLong { count, allowed } => write!(
                    f,
                    "snapshot carries {count} pending items, more than the queue's {allowed}"
                ),
                RestoreDefect::PresentationMismatch => write!(
                    f,
                    "snapshot's terminal state and mission-end presentation disagree"
                ),
            },
            Self::WorkBudgetTooSmall { found } => write!(
                f,
                "snapshot spends {found} work per tick, below the floor {MIN_WORK_PER_TICK}"
            ),
            Self::SequenceSpaceExhausted { ordinal } => write!(
                f,
                "snapshot item ordinal {ordinal} is past the allocatable sequence space"
            ),
            Self::RngReplayTooLong { draws } => write!(
                f,
                "{draws} RNG draws exceed the restore replay bound {MAX_RNG_REPLAY_DRAWS}"
            ),
            Self::DeferredActions { ordinal, error } => {
                write!(f, "pending item {ordinal} carries refused actions: {error}")
            }
        }
    }
}

impl std::error::Error for RestoreError {}

/// Event-key sequence space one pending work item owns. An objective's own
/// events use `0..=MAX_ACTIONS_PER_OBJECTIVE`; an item's events live above
/// that, packed by the item's session-unique ordinal, so two items from one
/// source can never collide and a restore-replay re-emits identical keys.
const SEQS_PER_ITEM: u32 = MAX_ACTIONS_PER_OBJECTIVE as u32 + 1;

fn item_sequence(ordinal: u32, action_index: usize) -> Option<u32> {
    ordinal
        .checked_add(1)?
        .checked_mul(SEQS_PER_ITEM)?
        .checked_add(action_index as u32 + 1)
}

/// The first item ordinal the sequence space cannot address: the largest action
/// index an item may carry no longer fits beside it in a `u32`. It is where
/// [`MissionState::alloc_ordinal`] stops — it refuses to hand this ordinal out
/// and never counts past it — so it is both the largest counter a live save
/// record can carry and the smallest one that means "exhausted".
pub const FIRST_EXHAUSTED_ITEM_ORDINAL: u32 = (u32::MAX - (SEQS_PER_ITEM - 1)) / SEQS_PER_ITEM;

/// Can this session still hand out `ordinal` to a scheduled item? The largest
/// action index an item may carry must still fit in its sequence space, which
/// is what keeps two items from one source addressable.
fn ordinal_allocatable(ordinal: u32) -> bool {
    ordinal < FIRST_EXHAUSTED_ITEM_ORDINAL
}

/// One queued work item: a validated action list eligible from `due`,
/// resumable after a bound stop (`next` is the first action not yet run).
#[derive(Clone, Debug, PartialEq)]
struct PendingWork {
    /// The program symbol the item's events and diagnostics attribute to —
    /// the objective (or item) that scheduled it.
    source: SymbolId,
    /// Unique within the session; separates this item's event keys from every
    /// other item sharing `source`.
    ordinal: u32,
    /// First tick the item is eligible on; a later tick still drains it.
    due: Tick,
    /// First action not yet executed (`0` for a fresh item).
    next: usize,
    actions: Vec<Action>,
}

/// The work one tick is in the middle of: buffered state writes, terminal
/// requests and the due-item queue being drained.
struct TickRun {
    writes: Vec<(SymbolId, Value)>,
    requested: BTreeSet<Outcome>,
    ready: VecDeque<PendingWork>,
    work: u64,
}

/// Mutable execution state, kept apart from the program.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionState {
    session: SessionGeneration,
    variables: BTreeMap<SymbolId, Value>,
    completed: BTreeSet<SymbolId>,
    consumed: BTreeSet<ExecutionKey>,
    /// The session's directive emissions so far, in `EventKey` order.
    directives: Vec<DirectiveEmission>,
    terminal: TerminalState,
    last_tick: Option<Tick>,
    policy: PrecedencePolicy,
    /// Scheduled work, keyed by eligibility tick.
    pending: BTreeMap<Tick, VecDeque<PendingWork>>,
    /// Count of items stored in `pending` (the memory cap's account).
    pending_len: usize,
    /// The presentation of the tick that ended this session, `None` while it
    /// runs ([`MissionEndPresentation`]); set together with `terminal`, so the
    /// two can never disagree.
    presentation: Option<MissionEndPresentation>,
    /// Session-unique ordinal for the next scheduled item.
    next_item_ordinal: u32,
    limits: WorkLimits,
    /// The mission's explicit RNG stream, seeded from the session so a replay
    /// of one session reproduces every draw bit-for-bit.
    rng: SplitMix64,
    /// Draws taken from `rng`; a restore replays them (see
    /// [`MAX_RNG_REPLAY_DRAWS`]).
    rng_draws: u64,
}

impl MissionState {
    /// Initial state of a validated program.
    pub fn new(program: &ValidatedProgram, session: SessionGeneration) -> Self {
        Self {
            session,
            variables: program
                .program()
                .variables
                .iter()
                .map(|v| (v.id, v.initial.clone()))
                .collect(),
            completed: BTreeSet::new(),
            consumed: BTreeSet::new(),
            directives: Vec::new(),
            terminal: TerminalState::Running,
            last_tick: None,
            // The measured original precedence (static code evidence, owner
            // note on Rally #589). The designed conservative policy stays
            // selectable for synthetic tests; the contract only allowed it
            // "until verified".
            policy: PrecedencePolicy::MeasuredOriginal,
            pending: BTreeMap::new(),
            pending_len: 0,
            presentation: None,
            next_item_ordinal: 0,
            limits: WorkLimits::default(),
            rng: SplitMix64::for_domain(session.0 as u64, MISSION_EVALUATOR_DOMAIN),
            rng_draws: 0,
        }
    }

    pub fn terminal(&self) -> TerminalState {
        self.terminal
    }

    /// The measured mission-end presentation once the session is terminal
    /// ([`MissionEndPresentation`]), `None` while it runs.
    ///
    /// It is the same value on every later read: the tick that ended the
    /// mission selected it, a latched terminal state never re-selects it, and
    /// a restored session carries the presentation its record held.
    #[must_use]
    pub fn terminal_presentation(&self) -> Option<MissionEndPresentation> {
        self.presentation
    }

    /// Overrides the work/queue bounds; the default is [`WorkLimits::default`].
    ///
    /// A work budget below [`MIN_WORK_PER_TICK`] is raised to it: such a budget
    /// could admit work items without ever executing one of their actions, which
    /// stalls the mission instead of bounding it. A queue cap above
    /// [`MAX_PENDING_ITEMS`] is lowered to it, the same way a save record's
    /// bounds are: the caller tightens the engine's bounds, never lifts them.
    pub fn set_limits(&mut self, limits: WorkLimits) {
        self.limits = WorkLimits {
            max_work_per_tick: limits
                .max_work_per_tick
                .clamp(MIN_WORK_PER_TICK, MAX_WORK_PER_TICK),
            max_pending_items: limits.max_pending_items.min(MAX_PENDING_ITEMS),
        };
    }

    /// Scheduled items still waiting for their eligibility tick.
    pub fn queued_items(&self) -> usize {
        self.pending_len
    }

    pub fn variable(&self, id: SymbolId) -> Option<&Value> {
        self.variables.get(&id)
    }

    pub fn is_completed(&self, objective: SymbolId) -> bool {
        self.completed.contains(&objective)
    }

    /// The last evaluated tick, or `None` before the session's first step.
    pub fn last_tick(&self) -> Option<Tick> {
        self.last_tick
    }

    /// The session's directive emissions so far, in [`EventKey`] order — the
    /// documented host effect of an [`Action::Directive`]. Each carries its
    /// execution key (emitted exactly once, under the same `consumed` guard
    /// the events use), the measured operation and the bound call's own
    /// arguments.
    pub fn directives(&self) -> &[DirectiveEmission] {
        &self.directives
    }

    /// The pending queue as [`PendingTimer`]s, in `(due, enqueue)` order.
    /// Each carries the exact remaining ticks of one scheduled item.
    pub fn pending_timers(&self) -> Vec<PendingTimer> {
        self.pending
            .values()
            .flatten()
            .map(|item| {
                pending_timer(
                    item.source,
                    item.due,
                    item.next,
                    item.actions.len(),
                    self.last_tick,
                )
            })
            .collect()
    }

    /// The versioned save record of this state (F37-C). Everything that can
    /// change a later observation is in it, so
    /// [`MissionState::restore`] reproduces the session exactly.
    pub fn snapshot(&self, program: &ValidatedProgram) -> MissionStateSnapshot {
        let pending = self
            .pending
            .values()
            .flatten()
            .map(|item| ScheduledWork {
                source: item.source,
                ordinal: item.ordinal,
                due: item.due,
                next: item.next,
                actions: item.actions.clone(),
            })
            .collect();
        MissionStateSnapshot {
            version: SNAPSHOT_VERSION,
            mission: program.program().mission.clone(),
            session: self.session,
            variables: self
                .variables
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            completed: self.completed.iter().copied().collect(),
            consumed: self.consumed.iter().copied().collect(),
            directives: self.directives.clone(),
            terminal: self.terminal,
            presentation: self.presentation,
            last_tick: self.last_tick,
            policy: self.policy,
            pending,
            next_item_ordinal: self.next_item_ordinal,
            limits: self.limits,
            rng_draws: self.rng_draws,
        }
    }

    /// Rebuilds execution state from a [`MissionStateSnapshot`].
    ///
    /// # Errors
    ///
    /// [`RestoreError`]: a foreign, older or internally inconsistent record is
    /// refused with the precise defect, never partially applied.
    pub fn restore(
        program: &ValidatedProgram,
        snapshot: MissionStateSnapshot,
    ) -> Result<Self, RestoreError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(RestoreError::SnapshotVersion {
                found: snapshot.version,
            });
        }
        let expected = &program.program().mission;
        if &snapshot.mission != expected {
            return Err(RestoreError::MissionMismatch {
                expected: expected.clone(),
                found: snapshot.mission.clone(),
            });
        }
        // The terminal state and its mission-end presentation are written
        // together, so they must arrive together: a session holds a
        // presentation exactly while it is terminal, and the presentation's
        // own result is the record's terminal state (F37-D-FU5).
        let presentation_consistent = snapshot.presentation.is_some()
            == (snapshot.terminal != TerminalState::Running)
            && snapshot
                .presentation
                .is_none_or(|presentation| presentation.result == snapshot.terminal);
        if !presentation_consistent {
            return Err(RestoreError::Corrupt {
                defect: RestoreDefect::PresentationMismatch,
            });
        }
        let declared = |symbol: SymbolId| {
            program
                .program()
                .variables
                .iter()
                .find(|v| v.id == symbol)
                .map(|v| v.initial.value_type())
        };
        let mut variables = BTreeMap::new();
        for (symbol, value) in &snapshot.variables {
            let Some(ty) = declared(*symbol) else {
                return Err(RestoreError::UnknownVariable { symbol: *symbol });
            };
            if value.value_type() != ty {
                return Err(RestoreError::TypeMismatch {
                    symbol: *symbol,
                    expected: ty,
                    found: value.value_type(),
                });
            }
            if variables.insert(*symbol, value.clone()).is_some() {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::DuplicateVariable { symbol: *symbol },
                });
            }
        }
        // Every state of this program holds every declared variable, and a
        // condition on an absent one is false: a record that dropped one would
        // restore a mission that can no longer reach its own objectives.
        for variable in program.program().variables.iter().map(|v| v.id) {
            if !variables.contains_key(&variable) {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::MissingVariable { symbol: variable },
                });
            }
        }
        for symbol in &snapshot.completed {
            if !program.program().objectives.iter().any(|o| o.id == *symbol) {
                return Err(RestoreError::UnknownObjective { symbol: *symbol });
            }
        }
        let mut consumed = BTreeSet::new();
        for key in &snapshot.consumed {
            if key.session != snapshot.session {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::ForeignExecutionKey {
                        session: key.session,
                    },
                });
            }
            consumed.insert(*key);
        }
        // The directive log is host-effect state, checked rather than
        // trusted like every other record field: every emission belongs to
        // this session, the log is in `EventKey` order with distinct
        // execution keys, and every emission's execution key was consumed —
        // the emit path inserts the two together, so a record that names an
        // emission without its consumption could not come from a live
        // session.
        let mut previous_directive: Option<EventKey> = None;
        let mut directive_keys = BTreeSet::new();
        for emission in &snapshot.directives {
            if emission.key.session != snapshot.session {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::ForeignExecutionKey {
                        session: emission.key.session,
                    },
                });
            }
            if previous_directive.is_some_and(|before| before >= emission.key)
                || !directive_keys.insert(emission.key.execution_key())
            {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::DirectiveOrder,
                });
            }
            previous_directive = Some(emission.key);
            if !consumed.contains(&emission.key.execution_key()) {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::DirectiveNotConsumed {
                        key: emission.key.execution_key(),
                    },
                });
            }
        }
        if snapshot.rng_draws > MAX_RNG_REPLAY_DRAWS {
            return Err(RestoreError::RngReplayTooLong {
                draws: snapshot.rng_draws,
            });
        }
        // The queue cap is an engine bound, so a record cannot install a longer
        // one: the restored session may be *tighter* than the record claims, but
        // never looser than [`MAX_PENDING_ITEMS`].
        let limits = WorkLimits {
            max_work_per_tick: snapshot.limits.max_work_per_tick.min(MAX_WORK_PER_TICK),
            max_pending_items: snapshot.limits.max_pending_items.min(MAX_PENDING_ITEMS),
        };
        if snapshot.limits.max_work_per_tick < MIN_WORK_PER_TICK {
            // A budget this small cannot execute an action at all: every tick
            // would admit work items and run none of them. It is a tighter bound
            // than the clamp above allows, because it is tighter than a session
            // that progresses at all.
            return Err(RestoreError::WorkBudgetTooSmall {
                found: snapshot.limits.max_work_per_tick,
            });
        }
        if snapshot.next_item_ordinal > FIRST_EXHAUSTED_ITEM_ORDINAL {
            // `alloc_ordinal` stops *at* the exhausted marker — it refuses to
            // hand it out and never counts past it — so that value is the
            // largest a live record can carry and it restores. A counter past
            // it can only come from a record no live session wrote, and it
            // would leave every `Schedule` the program still has permanently
            // stopped with `SequenceExhausted`.
            return Err(RestoreError::SequenceSpaceExhausted {
                ordinal: snapshot.next_item_ordinal,
            });
        }
        if snapshot.pending.len() > limits.max_pending_items {
            return Err(RestoreError::Corrupt {
                defect: RestoreDefect::PendingQueueTooLong {
                    count: snapshot.pending.len(),
                    allowed: limits.max_pending_items,
                },
            });
        }
        let mut pending = BTreeMap::new();
        let mut pending_len = 0usize;
        let mut seen_ordinals = BTreeSet::new();
        let mut previous = None;
        for item in &snapshot.pending {
            if item.next > item.actions.len() {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::ActionCursor {
                        next: item.next,
                        actions: item.actions.len(),
                    },
                });
            }
            if !seen_ordinals.insert(item.ordinal) {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::DuplicateOrdinal {
                        ordinal: item.ordinal,
                    },
                });
            }
            if item.ordinal >= snapshot.next_item_ordinal {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::OrdinalNotAllocated {
                        ordinal: item.ordinal,
                    },
                });
            }
            if let Some(before) = previous
                && before > item.due
            {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::PendingOrder {
                        previous: before,
                        given: item.due,
                    },
                });
            }
            previous = Some(item.due);
            // The evaluator runs an item's actions on the assumption that
            // pre-launch validation already refused an `Unknown` node, an empty
            // `Draw` range and a write to an undeclared variable. A record
            // carries its deferred text verbatim, so that assumption is checked
            // here instead of asserted.
            if let Err(error) = program.validate_actions(&item.actions) {
                return Err(RestoreError::DeferredActions {
                    ordinal: item.ordinal,
                    error,
                });
            }
            pending
                .entry(item.due)
                .or_insert_with(VecDeque::new)
                .push_back(PendingWork {
                    source: item.source,
                    ordinal: item.ordinal,
                    due: item.due,
                    next: item.next,
                    actions: item.actions.clone(),
                });
            pending_len += 1;
        }
        let mut rng = SplitMix64::for_domain(snapshot.session.0 as u64, MISSION_EVALUATOR_DOMAIN);
        for _ in 0..snapshot.rng_draws {
            rng.next_u64();
        }
        Ok(Self {
            session: snapshot.session,
            variables,
            completed: snapshot.completed.iter().copied().collect(),
            consumed,
            directives: snapshot.directives,
            terminal: snapshot.terminal,
            presentation: snapshot.presentation,
            last_tick: snapshot.last_tick,
            policy: snapshot.policy,
            pending,
            pending_len,
            next_item_ordinal: snapshot.next_item_ordinal,
            limits,
            rng,
            rng_draws: snapshot.rng_draws,
        })
    }

    /// Drops every queued work item and returns how many were dropped.
    ///
    /// Teardown: once the mission is over, deferred program work must not fire
    /// on a later tick, and leaving it queued would keep it alive in memory and
    /// in the save record. Returns the count so a caller can report it.
    pub fn teardown(&mut self) -> usize {
        let dropped = self.pending_len;
        self.pending.clear();
        self.pending_len = 0;
        dropped
    }

    /// Ends the session as [`TerminalState::Aborted`] and drops its queued
    /// work. Idempotent, and a session that already resolved an outcome keeps
    /// it: the evaluator's answer is the answer.
    ///
    /// A teardown sets neither mission flag and fires no instant outcome, so
    /// the presentation it records is the original's **no-flag** shape
    /// ([`MissionEndPresentation::new`] with all three flags clear): no
    /// `OBJECTIVES_*_SOUND`, the standard 3.0 s end delay, and the loss side
    /// of the mission sound and animation because WON is not set. The
    /// *result* of this path stays designed — the original has no Aborted
    /// state to observe (`f37.d.limit.aborted_outcome`).
    pub fn abort(&mut self) -> usize {
        if self.terminal == TerminalState::Running {
            self.terminal = TerminalState::Aborted;
            self.presentation = Some(MissionEndPresentation::new(
                false,
                false,
                false,
                TerminalState::Aborted,
            ));
        }
        self.teardown()
    }

    /// Resolves one tick with no countdown input
    /// ([`MissionCountdown::NONE`]): nothing can end this session by
    /// timeout. The countdown-aware path is
    /// [`MissionState::step_with_countdown`].
    ///
    /// # Errors
    ///
    /// [`TickError::NotAdvancing`] when `tick` is not after the last one.
    /// Bound violations are not errors: they stop the tick early and are
    /// reported in [`TickResult::stop`].
    pub fn step(
        &mut self,
        program: &ValidatedProgram,
        facts: &MissionFacts,
        tick: Tick,
    ) -> Result<TickResult, TickError> {
        self.step_with_countdown(program, facts, tick, MissionCountdown::NONE)
    }

    /// Resolves one tick with a mission-countdown input. See the module docs
    /// for the phases and bounds.
    ///
    /// The countdown is polled **first**, before the observe phase: when
    /// [`MissionCountdown::preempts`] says the countdown expired on this tick,
    /// the session records [`TerminalState::Failed`] and returns without
    /// observing a single condition, so no objective of this tick completes,
    /// no reward of this tick is granted and no queued item due this tick
    /// runs. That is the measured order (owner note on Rally #589, 2026-10-05:
    /// the countdown is polled inside `CZMission::Update` `0x46a490` at
    /// `0x46c640`, before the objective passes, and the mission ends at once
    /// through `0x463c30(1, 3.0)` with neither WON nor LOST — static code
    /// evidence, never an original run). The result is a failure because the
    /// recorded result is success iff the WON flag is set (`0x4194e0`), and
    /// the expiry sets no flag at all. Nothing else in the tick changes: the
    /// precedence policy is not consulted, because the original does not end
    /// through an objective's outcome kind here.
    ///
    /// Work already due on this tick is left in the pending queue — it belongs
    /// to a mission that has just ended, and it is dropped by the consumer's
    /// teardown of a terminal tick (`cs_sim::mission::MissionSession::advance`
    /// finishes the session) or by [`MissionState::teardown`]; it is never
    /// executed.
    ///
    /// # Errors
    ///
    /// [`TickError::NotAdvancing`] when `tick` is not after the last one.
    /// Bound violations are not errors: they stop the tick early and are
    /// reported in [`TickResult::stop`].
    pub fn step_with_countdown(
        &mut self,
        program: &ValidatedProgram,
        facts: &MissionFacts,
        tick: Tick,
        countdown: MissionCountdown,
    ) -> Result<TickResult, TickError> {
        if let Some(last) = self.last_tick
            && tick <= last
        {
            return Err(TickError::NotAdvancing { last, given: tick });
        }
        self.last_tick = Some(tick);
        let mut result = TickResult {
            session: self.session,
            tick,
            events: Vec::new(),
            terminal: self.terminal,
            stop: None,
        };
        if self.terminal != TerminalState::Running {
            return Ok(result);
        }
        // Phase 0 — the measured countdown pre-emption: an expiry ends the
        // mission here, before any condition is observed and before any
        // queued work runs, so this tick completes no objective and grants no
        // reward. Both measured exclusions (NOLOSS, network game) are part of
        // the input, not of this branch: see `MissionCountdown::preempts`.
        if countdown.preempts() {
            self.terminal = TerminalState::Failed;
            // The presentation half of the same tick (F37-D-FU5), built from
            // the shape this path really has: the expiry sets **no** flag
            // (neither WON nor LOST), and no `INSTANTWIN`/`INSTANTLOSS`
            // fired, so the terminal check takes its no-flag branch — no
            // `OBJECTIVES_*_SOUND`, the standard 3.0 s delay `0x463c30(1,
            // 3.0)` receives, and the loss side of the mission sound and the
            // animation because WON is clear. Set with `terminal`, so a save
            // taken after an expiry still restores
            // ([`RestoreDefect::PresentationMismatch`]).
            self.presentation = Some(MissionEndPresentation::new(
                false,
                false,
                false,
                self.terminal,
            ));
            result.terminal = self.terminal;
            return Ok(result);
        }
        let mut run = TickRun {
            writes: Vec::new(),
            requested: BTreeSet::new(),
            // Every item already due — in (due, enqueue) order.
            ready: self.take_due(tick),
            work: 0,
        };
        let budget = self.limits.max_work_per_tick;

        // Observe: all conditions against the start-of-tick state.
        let firing: Vec<_> = program
            .program()
            .objectives
            .iter()
            .filter(|o| !self.completed.contains(&o.id) && self.holds(&o.condition, facts))
            .collect();

        // Queue + resolve objective actions, in program order. A budget stop
        // latches the interrupted objective (it *did* fire), defers its
        // unexecuted actions as pending work and stops the tick; objectives
        // after it never fired and stay unfired for a later tick.
        for o in firing {
            if run.work >= budget {
                self.completed.insert(o.id);
                self.emit(
                    &mut result,
                    EventKey {
                        session: self.session,
                        tick,
                        source: o.id,
                        sequence: 0,
                    },
                    EventKind::ObjectiveCompleted,
                );
                self.defer(tick, &mut run, o.id, o.actions.to_vec());
                result.stop = Some(StopReason::WorkBudget {
                    at: self.locator(program, o.id, &["objective fire"]),
                    spent: run.work,
                });
                break;
            }
            run.work += 1;
            self.completed.insert(o.id);
            let session = self.session;
            let key = |sequence| EventKey {
                session,
                tick,
                source: o.id,
                sequence,
            };
            self.emit(&mut result, key(0), EventKind::ObjectiveCompleted);
            for (i, action) in o.actions.iter().enumerate() {
                if run.work >= budget {
                    self.defer(tick, &mut run, o.id, o.actions[i..].to_vec());
                    result.stop = Some(StopReason::WorkBudget {
                        at: self.locator(program, o.id, &[&format!("action {i}")]),
                        spent: run.work,
                    });
                    break;
                }
                run.work += 1;
                if let Some(stop) = self.run_action(
                    program,
                    action,
                    key(i as u32 + 1),
                    tick,
                    o.actions.as_slice(),
                    i,
                    &mut run,
                    &mut result,
                ) {
                    // The action did not complete (e.g. its `Schedule` could
                    // not enqueue); defer the list from that action on so it
                    // is retried, never skipped.
                    if !matches!(stop, StopReason::WorkBudget { .. }) {
                        self.defer(tick, &mut run, o.id, o.actions[i..].to_vec());
                    }
                    result.stop = Some(stop);
                    break;
                }
            }
            if result.stop.is_some() {
                break;
            }
        }

        // Drain pending work: FIFO, one action at a time. Zero-delay items
        // scheduled in this tick append to `run.ready`'s back, so they still
        // run — but never before work already queued, and never unbounded:
        // the per-tick budget applies here too.
        while result.stop.is_none()
            && let Some(mut item) = run.ready.pop_front()
        {
            let item_source = item.source;
            if run.work >= budget {
                run.ready.push_front(item);
                result.stop = Some(StopReason::WorkBudget {
                    at: self.locator(program, item_source, &["pending dequeue"]),
                    spent: run.work,
                });
                break;
            }
            run.work += 1;
            while item.next < item.actions.len() {
                let action_index = item.next;
                if run.work >= budget {
                    run.ready.push_front(item);
                    result.stop = Some(StopReason::WorkBudget {
                        at: self.locator(
                            program,
                            item_source,
                            &[&format!("pending action {action_index}")],
                        ),
                        spent: run.work,
                    });
                    break;
                }
                run.work += 1;
                let Some(sequence) = item_sequence(item.ordinal, action_index) else {
                    run.ready.push_front(item);
                    result.stop = Some(StopReason::SequenceExhausted {
                        at: self.locator(
                            program,
                            item_source,
                            &[&format!("pending action {action_index}")],
                        ),
                    });
                    break;
                };
                item.next = action_index + 1;
                let key = EventKey {
                    session: self.session,
                    tick,
                    source: item_source,
                    sequence,
                };
                if let Some(stop) = self.run_action(
                    program,
                    &item.actions[action_index],
                    key,
                    tick,
                    item.actions.as_slice(),
                    action_index,
                    &mut run,
                    &mut result,
                ) {
                    // Every `run_action` stop means the action did not
                    // complete (its `Schedule`/`Reschedule` did not enqueue);
                    // resume at that action so it is retried, never skipped.
                    // On `PendingLimit` the item goes to the back so the
                    // items still queued for this tick run first and can
                    // free the cap.
                    item.next = action_index;
                    if matches!(stop, StopReason::PendingLimit { .. }) {
                        run.ready.push_back(item);
                    } else {
                        run.ready.push_front(item);
                    }
                    result.stop = Some(stop);
                    break;
                }
            }
        }

        // Anything not reached this tick stays pending; it is already due, so
        // the next tick drains it first.
        if !run.ready.is_empty() {
            let remaining = run.ready.len();
            self.pending.entry(tick).or_default().extend(run.ready);
            self.pending_len += remaining;
        }
        for (variable, value) in run.writes {
            self.variables.insert(variable, value);
        }
        if let Some(outcome) = self.policy.pick(&run.requested) {
            self.terminal = outcome.into();
            // The presentation half of the same tick (F37-D-FU5), built from
            // three independent reads: the LOST flag chooses the branch (loss
            // before win), the WON flag chooses the mission sound, the
            // animation and — through the policy — the recorded result, and
            // an instant outcome fired this tick exactly when the program
            // requested the win or the loss. `Action::Finish` is the mission
            // IR's terminal action and the measured lowering vocabulary
            // reaches it through `INSTANTWIN`/`INSTANTLOSS`
            // (`cs_content::mission_control::terminal_outcome_of`), so a
            // program request that resolves a mission *is* the original's
            // instant-outcome marker; a transition with no such request (only
            // a host teardown reaches one today) carries the standard delay.
            let lost = run.requested.contains(&Outcome::Failed);
            let won = run.requested.contains(&Outcome::Succeeded);
            let instant = lost || won;
            self.presentation = Some(MissionEndPresentation::new(
                lost,
                won,
                instant,
                self.terminal,
            ));
        }
        result.events.sort_by_key(|e| e.key);
        result.terminal = self.terminal;
        Ok(result)
    }

    /// Moves every item eligible at `tick` out of `pending`, preserving
    /// (due, enqueue) order.
    fn take_due(&mut self, tick: Tick) -> VecDeque<PendingWork> {
        let keys: Vec<Tick> = self.pending.range(..=tick).map(|(t, _)| *t).collect();
        let mut ready = VecDeque::new();
        for key in keys {
            if let Some(items) = self.pending.remove(&key) {
                self.pending_len -= items.len();
                ready.extend(items);
            }
        }
        ready
    }

    /// Enqueues a scheduled work item. `delay == 0` appends to the queue being
    /// drained this tick — never stored, so exempt from the pending cap and
    /// bounded by the work budget instead; a positive delay stores it in
    /// `pending` and counts against the cap. Both paths are bounded: the
    /// pending cap and the session's ordinal space.
    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        program: &ValidatedProgram,
        run: &mut TickRun,
        tick: Tick,
        source: SymbolId,
        delay: u64,
        actions: Vec<Action>,
        action_index: usize,
    ) -> Option<StopReason> {
        if delay > 0 && self.pending_len >= self.limits.max_pending_items {
            return Some(StopReason::PendingLimit {
                at: self.locator(
                    program,
                    source,
                    &[&format!("schedule at action {action_index}")],
                ),
                queued: self.pending_len,
            });
        }
        let Some(ordinal) = self.alloc_ordinal() else {
            return Some(StopReason::SequenceExhausted {
                at: self.locator(
                    program,
                    source,
                    &[&format!("schedule at action {action_index}")],
                ),
            });
        };
        let item = PendingWork {
            source,
            ordinal,
            due: Tick(tick.0.saturating_add(delay)),
            next: 0,
            actions,
        };
        if delay == 0 {
            run.ready.push_back(item);
        } else {
            self.pending.entry(item.due).or_default().push_back(item);
            self.pending_len += 1;
        }
        None
    }

    /// The session-unique ordinal for the next scheduled item. `None` from
    /// [`FIRST_EXHAUSTED_ITEM_ORDINAL`] on (~66 million items), where the check
    /// guarantees every action index still fits in [`item_sequence`]. The
    /// counter stops there and never advances past it, which is what makes that
    /// value the largest a save record may carry.
    fn alloc_ordinal(&mut self) -> Option<u32> {
        let ordinal = self.next_item_ordinal;
        if !ordinal_allocatable(ordinal) {
            return None;
        }
        self.next_item_ordinal += 1;
        Some(ordinal)
    }

    /// Turns the unexecuted rest of a stopped action list into pending work
    /// due this tick: it drains first on the next tick, so nothing already
    /// executed repeats and nothing not yet executed is skipped. Remainder
    /// items are exempt from the pending cap — their number is bounded by the
    /// work the tick already spent, and dropping one would lose program
    /// instructions.
    fn defer(&mut self, tick: Tick, run: &mut TickRun, source: SymbolId, actions: Vec<Action>) {
        // `alloc_ordinal` failing is a pathological session; the fallback
        // ordinal overflows `item_sequence`, so the item stops its tick with
        // `SequenceExhausted` instead of emitting under a colliding key.
        let ordinal = self.alloc_ordinal().unwrap_or(u32::MAX);
        run.ready.push_back(PendingWork {
            source,
            ordinal,
            due: tick,
            next: 0,
            actions,
        });
    }

    /// Emits one event unless its execution key was already consumed — the
    /// exactly-once guard for save/restore and retry (non-negotiable 3).
    fn emit(&mut self, result: &mut TickResult, key: EventKey, kind: EventKind) {
        if self.consumed.insert(key.execution_key()) {
            result.events.push(MissionEvent { key, kind });
        }
    }

    /// Emits one directive emission under the same exactly-once guard
    /// [`emit`] applies to events: the execution key is consumed first, so a
    /// retried or replayed execution cannot produce the effect twice.
    ///
    /// The log is kept in [`EventKey`] order. Execution order is not key
    /// order — objectives resolve in declaration order while keys order by
    /// source — so the emission is inserted at its sorted position, the same
    /// order `TickResult.events` is sorted into before the tick returns.
    fn emit_directive(&mut self, key: EventKey, operation: DirectiveOperation, args: &[Value]) {
        if !self.consumed.insert(key.execution_key()) {
            return;
        }
        let emission = DirectiveEmission {
            key,
            operation,
            args: args.to_vec(),
        };
        let Err(at) = self.directives.binary_search_by_key(&key, |e| e.key) else {
            // The full key is already logged: impossible through `consumed`,
            // which admits an execution key only once, but fail closed rather
            // than record a host effect twice.
            return;
        };
        self.directives.insert(at, emission);
    }

    /// Runs one action of `enclosing` (the objective's list or the pending
    /// item's list — `Reschedule` re-queues whichever list the action lives
    /// in). `key` is the event key this action's emissions use.
    ///
    /// Returns `Some(stop)` when a bound refuses: `PendingLimit` means the
    /// action did not enqueue and must be retried by its owner next tick.
    #[allow(clippy::too_many_arguments)]
    fn run_action(
        &mut self,
        program: &ValidatedProgram,
        action: &Action,
        key: EventKey,
        tick: Tick,
        enclosing: &[Action],
        action_index: usize,
        run: &mut TickRun,
        result: &mut TickResult,
    ) -> Option<StopReason> {
        match action {
            Action::SetVariable { variable, value } => {
                run.writes.push((*variable, value.clone()));
            }
            Action::Draw { variable, min, max } => {
                // Validation proved `min <= max`, so the span fits u64 and
                // the drawn value is in `[min, max]`, hence in `i32`.
                let span = (*max as i64 - *min as i64 + 1) as u64;
                let drawn = (*min as i64 + (self.rng.next_u64() % span) as i64) as i32;
                self.rng_draws += 1;
                run.writes.push((*variable, Value::Int(drawn)));
            }
            Action::Finish(outcome) => {
                run.requested.insert(*outcome);
                self.emit(result, key, EventKind::TerminalRequested(*outcome));
            }
            Action::GrantReward { reward } => {
                self.emit(result, key, EventKind::RewardGranted(reward.clone()));
            }
            Action::Directive { operation, args } => {
                self.emit_directive(key, *operation, args);
            }
            Action::Schedule {
                delay_ticks,
                actions,
            } => {
                return self.enqueue(
                    program,
                    run,
                    tick,
                    key.source,
                    *delay_ticks,
                    actions.clone(),
                    action_index,
                );
            }
            Action::Reschedule { delay_ticks } => {
                return self.enqueue(
                    program,
                    run,
                    tick,
                    key.source,
                    *delay_ticks,
                    enclosing.to_vec(),
                    action_index,
                );
            }
            // Validation rejects Unknown; reaching it here is a program that
            // bypassed `validate`, which is impossible through
            // `ValidatedProgram`.
            Action::Unknown { .. } => unreachable!("validated program"),
        }
        None
    }

    /// The contract's budget diagnostic: mission id, the owning symbol and a
    /// short trace of where work stopped.
    fn locator(
        &self,
        program: &ValidatedProgram,
        source: SymbolId,
        trace: &[&str],
    ) -> ProgramLocator {
        ProgramLocator {
            mission: program.program().mission.to_string(),
            objective: Some(source),
            trace: trace.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// Evaluates one condition against `facts`: the whole of the mission's
    /// per-tick predicate evaluation.
    ///
    /// **Side-effect-free by construction** (contract, "Objective event
    /// ordering"; task `M01-LC-DIRECTIVE-LOWERING.02`, AC2): the receiver is
    /// `&self` and the facts are `&MissionFacts`, so evaluation cannot write
    /// program state, cannot write world state and cannot run an action. The
    /// original's `DEDG` evaluation-time member-field rewrites therefore do
    /// not live here — they are a host effect carrying their own named
    /// residual unknown ([`crate::ir::DEDG_MEMBER_FIELD_REWRITES`]), never
    /// part of the predicate.
    ///
    /// A fact nobody populated is **not** a default: every world-shaped arm
    /// answers `false` when the map it reads does not hold the key, so an
    /// unpopulated [`MissionFacts`] completes nothing.
    pub fn holds(&self, condition: &Condition, facts: &MissionFacts) -> bool {
        match condition {
            Condition::Const(b) => *b,
            Condition::ActorIs { actor, state } => facts.actors.get(actor) == Some(state),
            Condition::ObjectiveAwake { index } => {
                facts.objectives.get(index) == Some(&ObjectiveLifecycle::Awake)
            }
            Condition::InactiveMembers { members, threshold } => {
                // Measured: the unarmed evaluator (`+0x560 == 0`, no member
                // row at all) never fires, so an empty list answers `false`
                // rather than the vacuous `0 >= threshold`.
                if members.is_empty() {
                    return false;
                }
                let cleared = members
                    .iter()
                    .filter(|chain| {
                        facts
                            .members
                            .get(*chain)
                            .is_some_and(|row| row.presence == MemberPresence::OutOfPlay)
                    })
                    .count();
                cleared as u32 >= *threshold
            }
            Condition::EnemyGroupDepletion {
                group,
                remaining,
                generator,
            } => {
                let Some(living) = facts.groups.get(group) else {
                    // An unrecorded group is unknown, not an empty one.
                    return false;
                };
                // A generator the name does not resolve owes nothing —
                // measured: the original logs and adds no pending spawns.
                let pending = generator
                    .as_ref()
                    .map_or(0, |name| facts.generators.get(name).copied().unwrap_or(0));
                let Ok(remaining) = u32::try_from(*remaining) else {
                    // A negative remaining threshold is the record's
                    // "unarmed" spelling; nothing living can satisfy it.
                    return false;
                };
                u64::from(*living) + u64::from(pending) <= u64::from(remaining)
            }
            Condition::Travelers {
                subject,
                anchor,
                radius,
                approaching,
            } => {
                let Some(subject_row) = facts.members.get(subject) else {
                    return false;
                };
                if subject_row.presence != MemberPresence::InPlay {
                    // Subject absent or inactive: the original falls through
                    // to its counting path, which with a string subject has
                    // no armed group and returns false forever.
                    return false;
                }
                let anchor_point = match anchor {
                    TravelersAnchor::Object(chain) => match facts.members.get(chain) {
                        Some(row) if row.presence != MemberPresence::Missing => Some(row.position),
                        // Measured (finding B): the anchor name is resolved
                        // lazily and, while it never resolves, the record's
                        // explicit point is never written — so the original
                        // measures the distance from the **zeroed point**.
                        // A chain the facts do not carry at all is a
                        // different question: nobody observed it, so no
                        // distance may be taken from a world nobody described.
                        Some(_) => Some([0.0; 3]),
                        None => None,
                    },
                    TravelersAnchor::Point(point) => Some(*point),
                };
                let Some(anchor_point) = anchor_point else {
                    // No anchor the facts carry: no distance may be computed
                    // from a world nobody described.
                    return false;
                };
                let distance_squared: f64 = subject_row
                    .position
                    .iter()
                    .zip(anchor_point)
                    .map(|(here, there)| (here - there) * (here - there))
                    .sum();
                let limit = radius * radius;
                // Strict both ways: equality never fires (finding B).
                if *approaching {
                    distance_squared < limit
                } else {
                    distance_squared > limit
                }
            }
            Condition::AnimationStates {
                required,
                animations,
            } => {
                animations
                    .iter()
                    .filter(|(name, state)| {
                        facts
                            .animations
                            .get(name)
                            .is_some_and(|current| *current == state.code())
                    })
                    .count() as u32
                    >= *required
            }
            Condition::Not(c) => !self.holds(c, facts),
            Condition::All(cs) => cs.iter().all(|c| self.holds(c, facts)),
            Condition::Any(cs) => cs.iter().any(|c| self.holds(c, facts)),
            Condition::Compare {
                variable,
                op,
                value,
            } => self
                .variables
                .get(variable)
                .is_some_and(|v| compare(v, *op, value)),
            Condition::Unknown { .. } => unreachable!("validated program"),
        }
    }
}

fn compare(left: &Value, op: CompareOp, right: &Value) -> bool {
    use std::cmp::Ordering;
    let ordering = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => match a.partial_cmp(b) {
            Some(o) => o,
            None => return false,
        },
        _ => {
            return match op {
                CompareOp::Eq => left == right,
                CompareOp::Ne => left != right,
                _ => false,
            };
        }
    };
    match op {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::Ne => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::Le => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::Ge => ordering != Ordering::Less,
    }
}
