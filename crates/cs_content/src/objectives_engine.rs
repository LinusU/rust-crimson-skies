//! What the original engine does with an objective's completion effects, its
//! tick dependency (M01-B-OBJ-1), its dormancy timers, its completion count,
//! its display identity and its sound-group cues (M01-B-OBJ-2), measured from
//! the owner-supplied decrypted executable (`CS_ENGINE_IMAGE`) and nothing
//! else.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`.
//! Findings: `docs/findings/2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`
//! and `docs/findings/2026-10-12-m01-b-obj-2-objective-lifecycle-count-identity-cues.md`
//! hold the addresses, the measurements and what stays unknown. This module is
//! the **content half**: pure data and pure functions, no Bevy and no renderer.
//!
//! # What was measured
//!
//! * **Reading.** The mission loader reads `OBJECTIVE1`, `OBJECTIVE2`, … and
//!   stops at the first number the record does not declare, so objective number
//!   *N* is index *N − 1* of the mission's objective array. Every integer after
//!   `WAKE_OBJECTIVE_WHEN_I_COMPLETE`, `KILL_OBJECTIVE_WHEN_I_COMPLETE`,
//!   `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`, `NAP_OBJECTIVE_WHEN_I_COMPLETE`,
//!   `WAKE_OBJECTIVE_WHEN_I_SLEEP` and `TICK_DEPENDS_ON_OBJ` is stored as
//!   *number − 1*; a stored `-1` ends a list. The bare key `WAKE_OBJECTIVE` is
//!   read as an alias of `WAKE_OBJECTIVE_WHEN_I_COMPLETE`.
//! * **Completion.** An objective that completes becomes completed and inactive
//!   and enters state [`EngineState::Asleep`]; then, **in this fixed order and
//!   whatever order the record spells them in**, its `WAKE` list, its `NAP`
//!   target, its `KILL` list and its `SLEEP` list are applied
//!   ([`EngineObjectives::complete`]). A completed objective emits its
//!   `COMPLETED_SOUND_GROUP`, then the mission-level jingle for its role code,
//!   then — once, when the mission has not ended — the HUD notification pair
//!   (role code, ordinal).
//! * **Effects.** `WAKE` wakes a target ([`EngineObjectives::wake`]); `NAP`
//!   puts one target to [`EngineState::Napping`] for the declared time and
//!   clears its completed flag; `KILL` removes the target for good; `SLEEP`
//!   puts targets to [`EngineState::Asleep`]. Every wake of an objective that
//!   declares `WAKEUP_SOUND_GROUP` emits that group.
//! * **Tick dependency.** An objective with `TICK_DEPENDS_ON_OBJ` is evaluated
//!   only while the named objective is [`EngineState::Awake`]
//!   ([`EngineObjectives::is_ticked`]); the same gate holds its timers.
//! * **Dormancy.** `BEGIN_DORMANT` holds up to four seconds-valued timers
//!   ([`DormantTimers`]) and makes the objective load [`EngineState::Dormant`]
//!   and inactive. [`EngineObjectives::advance`] is the measured per-pass timer
//!   dispatch: a dormant objective wakes when the mission clock reaches its
//!   wake time, an awake one naps after its awake time and sleeps when the
//!   clock reaches its sleep time, and a napping one re-wakes after its nap
//!   time. The auto-nap and auto-sleep transitions wake the objective's
//!   `WAKE_OBJECTIVE_WHEN_I_SLEEP` list.
//! * **Completion count.** `INACTIVE_COMPLETION_COUNT` is the threshold the
//!   measured `INACTIVE<n>`-stage completion check compares the number of
//!   flag-clear resolved stage targets against; absent, it defaults to the
//!   resolved stage count ([`EngineDeclaration::declared_inactive_stages`]
//!   documents the difference between declared and resolved).
//! * **Identity.** `IDENTITY`'s role text and ordinal are resolved by the
//!   loader into the objective's role code and ordinal, used by the
//!   completion-time HUD notification; the third element — the message name —
//!   is never read by the objective runtime (only by the briefing table
//!   builder, a different code path).
//!
//! What is **not** here: how an objective *becomes* complete beyond the count
//! check (its other conditions are other field families), what a stage's
//! cleared flag means, the `RESET_TIMER`, `HIDE_OBJ`, score-adjustment and
//! actor-activation fields `wake` also walks (other families), the rule that
//! picks which of several simultaneously complete objectives completes first
//! in a pass, and the sound manager's internal handling of an emitted group.

use std::fmt;

/// The number of target slots the engine's list readers walk, terminator
/// included. A list longer than [`ENGINE_LIST_CAPACITY`]` - 1` has no room for
/// its terminator and overruns into the next field of the engine's own record,
/// so it is refused here rather than modelled.
pub const ENGINE_LIST_CAPACITY: usize = 15;

/// The nap time the engine substitutes when `NAP_OBJECTIVE_WHEN_I_COMPLETE`
/// names a target but no time. The engine logs "has no nap_time specified" and
/// stores this value; it is `0x3e99999a` as a 32-bit float.
pub const ENGINE_DEFAULT_NAP_TIME: f32 = f32::from_bits(0x3e99_999a);

/// The measured keys the engine reads for the effects and the dependency.
pub mod keys {
    /// Wakes the listed objectives when this one completes.
    pub const WAKE: &str = "WAKE_OBJECTIVE_WHEN_I_COMPLETE";
    /// The engine reads this bare spelling as a `WAKE` alias.
    pub const WAKE_ALIAS: &str = "WAKE_OBJECTIVE";
    /// Napping: one target and a time.
    pub const NAP: &str = "NAP_OBJECTIVE_WHEN_I_COMPLETE";
    /// Removes the listed objectives when this one completes.
    pub const KILL: &str = "KILL_OBJECTIVE_WHEN_I_COMPLETE";
    /// Sleeps the listed objectives. The engine reads it; no retail record
    /// spells it.
    pub const SLEEP: &str = "SLEEP_OBJECTIVE_WHEN_I_COMPLETE";
    /// The objective that must be awake for this one to be evaluated.
    pub const TICK_DEPENDS_ON_OBJ: &str = "TICK_DEPENDS_ON_OBJ";
    /// The objective loads dormant; its elements are the four timer slots.
    pub const BEGIN_DORMANT: &str = "BEGIN_DORMANT";
    /// Wakes the listed objectives when this one naps or sleeps itself.
    ///
    /// The engine reads it like [`WAKE`](Self::WAKE); the list is walked on
    /// the auto-nap and auto-sleep timer transitions, not on completion.
    pub const WAKE_WHEN_I_SLEEP: &str = "WAKE_OBJECTIVE_WHEN_I_SLEEP";
    /// How many of the resolved `INACTIVE<n>` stage targets must be cleared;
    /// absent, the resolved stage count.
    pub const INACTIVE_COMPLETION_COUNT: &str = "INACTIVE_COMPLETION_COUNT";
    /// The objective's display identity: a role text, an ordinal and an
    /// optional message name.
    pub const IDENTITY: &str = "IDENTITY";
    /// The sound group emitted when the objective is woken.
    pub const WAKEUP_SOUND_GROUP: &str = "WAKEUP_SOUND_GROUP";
    /// The sound group emitted when the objective completes.
    pub const COMPLETED_SOUND_GROUP: &str = "COMPLETED_SOUND_GROUP";
}

/// The engine's four objective states, named by the value the engine stores.
///
/// The names for 2 and 3 follow the `NAP` and `SLEEP` spellings of the effects
/// that set them; completion also sets [`Asleep`](Self::Asleep).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EngineState {
    /// State 0: set by `BEGIN_DORMANT`, left by a wake.
    Dormant,
    /// State 1: the state in which an objective is evaluated and counts as
    /// awake for the tick dependency.
    Awake,
    /// State 2: set by `NAP`, left when the nap time has elapsed.
    Napping,
    /// State 3: set when the objective completes and by `SLEEP`.
    Asleep,
}

/// One objective's list of targets, as the engine stores it.
///
/// Targets are indices (*number − 1*). The list ends at the first stored `-1`,
/// i.e. at the first record integer `0`, which the engine treats as the
/// terminator rather than a target.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TargetList(Vec<usize>);

impl TargetList {
    /// Reads the integers of a record value the way the engine does.
    ///
    /// # Errors
    ///
    /// [`EngineEffectError::ListTooLong`] when the list leaves no room for the
    /// engine's terminator.
    pub fn from_record(numbers: &[u32]) -> Result<Self, EngineEffectError> {
        if numbers.len() >= ENGINE_LIST_CAPACITY {
            return Err(EngineEffectError::ListTooLong {
                len: numbers.len(),
                capacity: ENGINE_LIST_CAPACITY - 1,
            });
        }
        Ok(Self(
            numbers
                .iter()
                .map_while(|number| number.checked_sub(1))
                .map(|index| index as usize)
                .collect(),
        ))
    }

    /// The target indices, in the order the record spells them.
    #[must_use]
    pub fn targets(&self) -> &[usize] {
        &self.0
    }
}

/// The `NAP` declaration: at most one target and its time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NapDeclaration {
    /// The target's index, or `None` when the stored target is `-1` (the engine
    /// then does nothing).
    pub target: Option<usize>,
    /// The nap time, in the unit of the engine's per-pass time step.
    pub time: f32,
}

impl NapDeclaration {
    /// Reads the integer target and the optional time of a `NAP` record value.
    ///
    /// `time` is the second value of the key, already converted to a float (the
    /// engine accepts an integer there and converts it); `None` means the
    /// record gave none, and the engine then logs an error and uses
    /// [`ENGINE_DEFAULT_NAP_TIME`].
    #[must_use]
    pub fn from_record(target: u32, time: Option<f32>) -> Self {
        Self {
            target: target.checked_sub(1).map(|index| index as usize),
            time: time.unwrap_or(ENGINE_DEFAULT_NAP_TIME),
        }
    }
}

/// How many `BEGIN_DORMANT` elements the engine reads: one per timer slot.
pub const DORMANT_SLOT_COUNT: usize = 4;

/// The four timer slots the engine keeps per objective (`+0x5d0..+0x5dc`),
/// filled by `BEGIN_DORMANT`'s elements.
///
/// **Every value is seconds.** The mission clock the wake and sleep slots are
/// compared against accumulates the engine's frame delta — a millisecond
/// counter scaled by `0.001` — and the elapsed timer the nap slots are compared
/// against accumulates the same delta while the objective is awake or napping.
///
/// A slot's *armed* test is measured per slot and is **not** the same for all
/// four: the nap slot arms only on a strictly positive value while the other
/// three arm on `>= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DormantTimers {
    /// Slot `+0x5d0` (element 0): a dormant objective wakes itself once the
    /// mission clock reaches this many seconds. Armed on `>= 0`; the default
    /// is `0.0`, so a dormant objective with no element wakes as soon as the
    /// first pass runs.
    pub wake_at: f32,
    /// Slot `+0x5d4` (element 1): an awake objective naps itself once it has
    /// been awake for this many seconds. Armed on `> 0` — the one slot for
    /// which `0.0` is **not** armed. The default is `-1.0`.
    pub nap_after: f32,
    /// Slot `+0x5d8` (element 2): a napping objective re-wakes once it has been
    /// napping for this many seconds. Armed on `>= 0`; the default is `-1.0`.
    /// The `NAP` completion effect writes the same slot on its target.
    pub rewake_after: f32,
    /// Slot `+0x5dc` (element 3): an awake objective sleeps itself once the
    /// mission clock reaches this many seconds. Armed on `>= 0`; the default
    /// is `-1.0`.
    pub sleep_at: f32,
}

impl DormantTimers {
    /// The values the loader writes before it reads `BEGIN_DORMANT`: the wake
    /// slot armed at `0.0`, the other three disarmed at `-1.0`.
    pub const DEFAULT: Self = Self {
        wake_at: 0.0,
        nap_after: -1.0,
        rewake_after: -1.0,
        sleep_at: -1.0,
    };

    /// The slots as the loader stores them: element *i* fills slot *i* over
    /// [`DEFAULT`](Self::DEFAULT), at most [`DORMANT_SLOT_COUNT`] elements are
    /// read and the rest are never looked at.
    ///
    /// `elements` is the record's elements already converted to `f32` the way
    /// the loader converts them (an integer element is accepted and converted).
    #[must_use]
    pub fn from_elements(elements: &[f32]) -> Self {
        let mut timers = Self::DEFAULT;
        if let Some(&wake_at) = elements.first() {
            timers.wake_at = wake_at;
        }
        if let Some(&nap_after) = elements.get(1) {
            timers.nap_after = nap_after;
        }
        if let Some(&rewake_after) = elements.get(2) {
            timers.rewake_after = rewake_after;
        }
        if let Some(&sleep_at) = elements.get(3) {
            timers.sleep_at = sleep_at;
        }
        timers
    }

    /// Whether the wake slot is armed (measured `>= 0`).
    #[must_use]
    pub const fn wake_armed(&self) -> bool {
        self.wake_at >= 0.0
    }

    /// Whether the nap slot is armed (measured `> 0` — `0.0` is disarmed).
    #[must_use]
    pub const fn nap_armed(&self) -> bool {
        self.nap_after > 0.0
    }

    /// Whether the re-wake slot is armed (measured `>= 0`).
    #[must_use]
    pub const fn rewake_armed(&self) -> bool {
        self.rewake_after >= 0.0
    }

    /// Whether the sleep slot is armed (measured `>= 0`).
    #[must_use]
    pub const fn sleep_armed(&self) -> bool {
        self.sleep_at >= 0.0
    }
}

impl Default for DormantTimers {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the engine's loader resolves of an `IDENTITY` declaration.
///
/// The loader reads at least two elements: element 0 is the role text and
/// element 1 the ordinal integer. Element 2 — the message name — is **never
/// read on this path**; the briefing-table builder reads it separately. The
/// role code is `_stricmp`'d against the three spellings and stays `0` for
/// anything else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineIdentity {
    /// The role text as written (`PRIMARY`, `SECONDARY`, `TERTIARY` in the
    /// installation's spellings).
    pub role: String,
    /// The loader's code for [`role`](Self::role): `primary` → 1,
    /// `secondary` → 2, `tertiary` → 3, case-insensitively; `0` otherwise.
    pub role_code: u32,
    /// The ordinal inside the role, element 1 verbatim.
    pub ordinal: u32,
    /// The message name when the declaration carries one. The objective
    /// runtime never reads it; it is kept verbatim because the briefing-table
    /// builder is its measured consumer.
    pub message: Option<String>,
}

impl EngineIdentity {
    /// The loader's role-code mapping (case-insensitive).
    #[must_use]
    pub fn role_code_of(role: &str) -> u32 {
        if role.eq_ignore_ascii_case("primary") {
            1
        } else if role.eq_ignore_ascii_case("secondary") {
            2
        } else if role.eq_ignore_ascii_case("tertiary") {
            3
        } else {
            0
        }
    }

    /// An identity as the loader resolves it: the role code from the text,
    /// the ordinal verbatim and the unread message element kept verbatim.
    #[must_use]
    pub fn from_record(role: &str, ordinal: u32, message: Option<String>) -> Self {
        Self {
            role: role.to_owned(),
            role_code: Self::role_code_of(role),
            ordinal,
            message,
        }
    }
}

/// Which group a [`CueEmission`] came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueKind {
    /// The objective's own `WAKEUP_SOUND_GROUP`, emitted inside `wake` after
    /// the objective's flags are written.
    Wakeup,
    /// The objective's own `COMPLETED_SOUND_GROUP`, emitted inside `complete`
    /// right after the completion flags are written — before the role jingle
    /// and the completion effects.
    Completed,
    /// The mission-level jingle dispatched on the completed objective's role
    /// code, emitted right after `Completed`. The engine keeps one global
    /// group per role code and emits it only for codes 1 through 3; the
    /// record does not name it.
    RoleJingle,
}

/// One sound-group cue the objective runtime emitted.
///
/// Emission is the measured fact — "the runtime handed this group to the sound
/// manager here". What the manager does with it (whether the name resolves to
/// an audible clip, which channel class it takes) is the sound engine's, not
/// the objective semantics', and stays outside this model.
#[derive(Clone, Debug, PartialEq)]
pub struct CueEmission {
    /// The objective index whose declaration named the group.
    pub objective: usize,
    /// Which cue this is.
    pub kind: CueKind,
    /// The declared group name. `None` for [`CueKind::RoleJingle`], which is a
    /// mission-level group no record names; the role code is on
    /// [`EngineObjective::role_code`].
    pub group: Option<String>,
}

/// The pair the engine hands the HUD when a completed objective has a role.
///
/// Emitted at most once per objective — the engine's `+0x558` pending flag —
/// and only while the mission has not ended (`+0xc54`, written by the outcome
/// family, which this model exposes as [`EngineObjectives::mission_ended`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HudNotification {
    /// The completed objective's index.
    pub objective: usize,
    /// Its role code (`+0x00`).
    pub role_code: u32,
    /// Its ordinal (`+0x04`).
    pub ordinal: u32,
}

/// What one objective declares for its completion and its evaluation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineDeclaration {
    /// `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (or its bare alias).
    pub wake: TargetList,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE`.
    pub nap: Option<NapDeclaration>,
    /// `KILL_OBJECTIVE_WHEN_I_COMPLETE`.
    pub kill: TargetList,
    /// `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`.
    pub sleep: TargetList,
    /// `TICK_DEPENDS_ON_OBJ`: the index of the objective that must be awake.
    pub tick_depends_on: Option<usize>,
    /// `BEGIN_DORMANT`'s timer slots. `Some` also means the objective loads
    /// [`EngineState::Dormant`] and inactive; an element list may hold fewer
    /// than four values, the rest keep [`DormantTimers::DEFAULT`].
    pub begin_dormant: Option<DormantTimers>,
    /// `WAKE_OBJECTIVE_WHEN_I_SLEEP`: the list the auto-nap and auto-sleep
    /// transitions wake.
    pub wake_when_i_sleep: TargetList,
    /// `INACTIVE_COMPLETION_COUNT` verbatim; `None` means the engine default,
    /// the resolved stage count.
    pub inactive_completion_count: Option<u32>,
    /// How many `INACTIVE<n>` conditions the block declares (distinct stage
    /// numbers the engine scans, `1..=100`).
    ///
    /// The engine's own count — `+0x560`, the `INACTIVE_COMPLETION_COUNT`
    /// default — is the number of declarations it **resolves**; an
    /// unresolvable condition is dropped rather than counted. Whether a
    /// declaration resolves belongs to the stage-condition family, which is
    /// unmeasured, so this is the *declared* count: an upper bound the default
    /// reaches exactly when every stage resolves.
    pub declared_inactive_stages: u32,
    /// `IDENTITY` as the objective loader resolves it.
    pub identity: Option<EngineIdentity>,
    /// `WAKEUP_SOUND_GROUP`'s group name.
    pub wakeup_sound_group: Option<String>,
    /// `COMPLETED_SOUND_GROUP`'s group name.
    pub completed_sound_group: Option<String>,
}

/// Why a record value cannot be read into an engine declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineEffectError {
    /// A list has no room for the engine's terminator.
    ListTooLong {
        /// How many integers the record spells.
        len: usize,
        /// How many the engine's list can hold.
        capacity: usize,
    },
    /// A target names an objective the record does not declare.
    TargetOutOfRange {
        /// The declaring objective's index.
        from: usize,
        /// The target index.
        target: usize,
        /// How many objectives the record declares.
        count: usize,
    },
}

impl fmt::Display for EngineEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ListTooLong { len, capacity } => write!(
                formatter,
                "a target list of {len} integers overruns the engine's {capacity}-target list"
            ),
            Self::TargetOutOfRange {
                from,
                target,
                count,
            } => write!(
                formatter,
                "objective index {from} names objective index {target}, but only {count} are declared"
            ),
        }
    }
}

impl std::error::Error for EngineEffectError {}

/// One objective's engine flags and timers.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineObjective {
    /// Cleared by `KILL`; a wake skips an objective that is not present.
    pub present: bool,
    /// Cleared on completion and by every state change.
    pub active: bool,
    /// Set on completion; cleared again by `NAP`.
    pub completed: bool,
    /// The engine state.
    pub state: EngineState,
    /// Seconds accumulated in the current state — reset by `wake` and by every
    /// state-setter transition, advanced only while the armed timer of the
    /// awake or napping case runs.
    pub elapsed: f32,
    /// The live timer slots. `rewake_after` doubles as the slot a `NAP`
    /// completion effect writes on its target.
    pub timers: DormantTimers,
    /// The loader-resolved role code (`+0x00`): 0 when the block carries no
    /// usable `IDENTITY`.
    pub role_code: u32,
    /// The loader-resolved ordinal (`+0x04`).
    pub ordinal: u32,
    /// `+0x558`: the pending completion HUD notification — set at load and
    /// consumed on the objective's first completion that runs while the
    /// mission has not ended.
    pub hud_notify_pending: bool,
    /// What this objective declares.
    pub declaration: EngineDeclaration,
}

impl EngineObjective {
    /// An objective in `state` with nothing pending.
    #[must_use]
    pub fn new(state: EngineState, declaration: EngineDeclaration) -> Self {
        Self {
            present: true,
            active: state == EngineState::Awake,
            completed: false,
            state,
            elapsed: 0.0,
            timers: declaration.begin_dormant.unwrap_or_default(),
            role_code: declaration
                .identity
                .as_ref()
                .map_or(0, |identity| identity.role_code),
            ordinal: declaration
                .identity
                .as_ref()
                .map_or(0, |identity| identity.ordinal),
            hud_notify_pending: true,
            declaration,
        }
    }

    /// The loader's construction: `BEGIN_DORMANT` present makes the objective
    /// dormant and inactive, otherwise it is awake and active.
    #[must_use]
    pub fn loaded(declaration: EngineDeclaration) -> Self {
        let state = if declaration.begin_dormant.is_some() {
            EngineState::Dormant
        } else {
            EngineState::Awake
        };
        Self::new(state, declaration)
    }
}

/// All of one mission's objectives, in objective-number order.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineObjectives {
    objectives: Vec<EngineObjective>,
    /// `+0x6f0`: the mission clock in seconds, advanced once per
    /// [`advance`](Self::advance) before the timer dispatch runs.
    mission_clock: f32,
    /// `+0xc54`: the mission-ended flag the outcome family sets. Its measured
    /// use here is the completion-time HUD gate (a `0x463c00` call reads it
    /// before the `+0x558` block runs); whether the caller keeps running the
    /// whole objective pass once it is set is outside this function.
    /// Nothing in this model sets it; it is public so an owner of the outcome
    /// family can drive it.
    pub mission_ended: bool,
    /// The sound-group cues emitted, in emission order.
    cues: Vec<CueEmission>,
    /// The HUD notifications emitted, in emission order.
    hud_notifications: Vec<HudNotification>,
}

impl EngineObjectives {
    /// Builds the set, refusing a declaration whose target is out of range.
    ///
    /// # Errors
    ///
    /// [`EngineEffectError::TargetOutOfRange`] for the first such target.
    pub fn new(objectives: Vec<EngineObjective>) -> Result<Self, EngineEffectError> {
        let count = objectives.len();
        for (from, objective) in objectives.iter().enumerate() {
            let declaration = &objective.declaration;
            let nap = declaration.nap.and_then(|nap| nap.target);
            let named = declaration
                .wake
                .targets()
                .iter()
                .chain(declaration.kill.targets())
                .chain(declaration.sleep.targets())
                .chain(declaration.wake_when_i_sleep.targets())
                .copied()
                .chain(nap)
                .chain(declaration.tick_depends_on);
            for target in named {
                if target >= count {
                    return Err(EngineEffectError::TargetOutOfRange {
                        from,
                        target,
                        count,
                    });
                }
            }
        }
        Ok(Self {
            objectives,
            mission_clock: 0.0,
            mission_ended: false,
            cues: Vec::new(),
            hud_notifications: Vec::new(),
        })
    }

    /// The objective at `index`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&EngineObjective> {
        self.objectives.get(index)
    }

    /// Whether `index` is evaluated this pass as far as the tick dependency
    /// goes: it has none, or the objective it names is awake.
    #[must_use]
    pub fn is_ticked(&self, index: usize) -> bool {
        let Some(objective) = self.objectives.get(index) else {
            return false;
        };
        objective
            .declaration
            .tick_depends_on
            .is_none_or(|dependency| self.objectives[dependency].state == EngineState::Awake)
    }

    /// Whether the completion conditions of `index` are evaluated: the tick
    /// dependency holds, it is active and it has not completed.
    #[must_use]
    pub fn evaluates_completion(&self, index: usize) -> bool {
        self.is_ticked(index) && self.objectives[index].active && !self.objectives[index].completed
    }

    /// The mission clock (`+0x6f0`), in seconds.
    #[must_use]
    pub const fn mission_clock(&self) -> f32 {
        self.mission_clock
    }

    /// The cues emitted so far, in the order the engine emitted them.
    ///
    /// [`wake`](Self::wake) and [`complete`](Self::complete) push here; nothing
    /// drains it, so the log is the mission's whole cue history unless the
    /// caller clears it.
    #[must_use]
    pub fn cues(&self) -> &[CueEmission] {
        &self.cues
    }

    /// Takes the cues emitted since the last call, in emission order, leaving
    /// the log empty for the next pass.
    ///
    /// This is the host's read: one pass takes each emitted cue exactly once,
    /// so a cue is handed to the sound consumer once and can never be offered
    /// twice — while [`Self::cues`] stays the whole history for a caller that
    /// wants it.
    pub fn take_cues(&mut self) -> Vec<CueEmission> {
        std::mem::take(&mut self.cues)
    }

    /// Drops every recorded cue and HUD notification.
    pub fn clear_outputs(&mut self) {
        self.cues.clear();
        self.hud_notifications.clear();
    }

    /// The HUD notifications emitted so far, in emission order.
    #[must_use]
    pub fn hud_notifications(&self) -> &[HudNotification] {
        &self.hud_notifications
    }

    /// The threshold the measured `INACTIVE<n>`-stage completion check compares
    /// against: `INACTIVE_COMPLETION_COUNT` when the block declares one, else
    /// the stage count — the engine's `+0x560`, which counts *resolved*
    /// conditions; [`EngineDeclaration::declared_inactive_stages`] is the
    /// declared count, an upper bound it reaches when every stage resolves.
    ///
    /// `index` is the zero-based objective index.
    #[must_use]
    pub fn inactive_count_threshold(&self, index: usize) -> Option<u32> {
        let declaration = &self.objectives.get(index)?.declaration;
        Some(
            declaration
                .inactive_completion_count
                .unwrap_or(declaration.declared_inactive_stages),
        )
    }

    /// Whether the objective could still fire the count check — the threshold
    /// the engine stored is at or under its stage count.
    ///
    /// Reachability in the *structural* sense only: with no stages the engine's
    /// checker never fires at all, and a threshold above the stage count can
    /// never be reached however the conditions resolve. Whether enough stages
    /// can actually clear in ordinary play is the condition family's meaning —
    /// unmeasured — so this is the necessary condition, not a promise.
    #[must_use]
    pub fn count_threshold_reachable(&self, index: usize) -> Option<bool> {
        let declaration = &self.objectives.get(index)?.declaration;
        if declaration.declared_inactive_stages == 0 {
            return Some(false);
        }
        Some(self.inactive_count_threshold(index)? <= declaration.declared_inactive_stages)
    }

    /// The measured `INACTIVE<n>`-stage completion check itself: it never
    /// fires on an objective with no resolved stage targets, and otherwise
    /// fires once at least `threshold` resolved targets report the flag it
    /// counts.
    ///
    /// `flag_clear_stages` is the count of resolved stage targets whose
    /// `+0x24` bit `0x4` is **clear** — exactly what the engine's checker
    /// tallies. What sets or clears that bit is the condition family's
    /// meaning, unmeasured here, so the count is the caller's input and never
    /// invented.
    #[must_use]
    pub fn inactive_count_met(&self, index: usize, flag_clear_stages: u32) -> bool {
        let Some(objective) = self.objectives.get(index) else {
            return false;
        };
        objective.declaration.declared_inactive_stages > 0
            && flag_clear_stages >= self.inactive_count_threshold(index).unwrap_or(u32::MAX)
    }

    /// Wakes each target of `targets` in order, as the engine's wake does.
    ///
    /// A target that is not present or has completed is skipped. A target that
    /// is **already awake ends the whole list** (its elapsed time is reset
    /// first): later targets are not woken, and that awake target is what the
    /// engine returns. Every woken target emits its `WAKEUP_SOUND_GROUP` when
    /// it declares one, after the state writes.
    ///
    /// The engine's return value is measured and matters to the auto-nap path:
    /// the already-awake early-out returns that target's index; a list so long
    /// it fills the engine's 15-entry window returns the last target woken;
    /// and a list that ends on its terminator returns `-1` — modelled here as
    /// `None`, even when targets were woken on the way.
    ///
    /// Waking a target also runs the engine's dormant-only extras — the
    /// `RESET_TIMER` HUD timer (`+0x5e0`, consumed once) and a `+0xd8` link
    /// that marks another objective completed — **only when the target's
    /// state before this wake was dormant**. The record keys that fill those
    /// fields belong to other families and are not in this model, so the
    /// extras are documented but not reproduced.
    pub fn wake(&mut self, targets: &[usize]) -> Option<usize> {
        let mut last_woken = None;
        for &target in targets.iter().take(ENGINE_LIST_CAPACITY) {
            let objective = &mut self.objectives[target];
            if !objective.present || objective.completed {
                continue;
            }
            if objective.state == EngineState::Awake {
                objective.elapsed = 0.0;
                return Some(target);
            }
            last_woken = Some(target);
            objective.active = true;
            objective.state = EngineState::Awake;
            objective.elapsed = 0.0;
            if let Some(group) = objective.declaration.wakeup_sound_group.clone() {
                self.cues.push(CueEmission {
                    objective: target,
                    kind: CueKind::Wakeup,
                    group: Some(group),
                });
            }
        }
        // The stored list holds at most `ENGINE_LIST_CAPACITY - 1` targets
        // plus its terminator, so a parsed list always ends on the `-1` the
        // engine returns. Only a raw slice long enough to fill the whole
        // window can reach the cap exit, which returns the last woken index.
        (targets.len() >= ENGINE_LIST_CAPACITY)
            .then_some(last_woken)
            .flatten()
    }

    /// Sets `target` to `state` the way the engine's state setter does: inactive,
    /// elapsed time zero.
    fn set_state(&mut self, target: usize, state: EngineState) {
        let objective = &mut self.objectives[target];
        objective.active = false;
        objective.state = state;
        objective.elapsed = 0.0;
    }

    /// Completes `index` and applies its effects in the engine's order: the
    /// objective becomes completed, inactive and asleep; its
    /// `COMPLETED_SOUND_GROUP` and the mission-level jingle for its role code
    /// are emitted; its pending HUD notification is delivered once when the
    /// mission has not ended; then `WAKE`, `NAP`, `KILL`, `SLEEP`.
    ///
    /// The order is the engine's, not the record's, so two effects on one
    /// objective always resolve the same way: `WAKE` then `NAP` leaves the
    /// target napping and inactive.
    pub fn complete(&mut self, index: usize) {
        let declaration = self.objectives[index].declaration.clone();
        {
            let objective = &mut self.objectives[index];
            objective.completed = true;
            objective.active = false;
            objective.state = EngineState::Asleep;
        }
        if let Some(group) = declaration.completed_sound_group.clone() {
            self.cues.push(CueEmission {
                objective: index,
                kind: CueKind::Completed,
                group: Some(group),
            });
        }
        let role_code = self.objectives[index].role_code;
        if (1..=3).contains(&role_code) {
            self.cues.push(CueEmission {
                objective: index,
                kind: CueKind::RoleJingle,
                group: None,
            });
        }
        if !self.mission_ended && self.objectives[index].hud_notify_pending {
            if role_code != 0 {
                self.hud_notifications.push(HudNotification {
                    objective: index,
                    role_code,
                    ordinal: self.objectives[index].ordinal,
                });
            }
            self.objectives[index].hud_notify_pending = false;
        }
        self.wake(declaration.wake.targets());
        if let Some(NapDeclaration {
            target: Some(target),
            time,
        }) = declaration.nap
        {
            self.set_state(target, EngineState::Napping);
            self.objectives[target].timers.rewake_after = time;
            self.objectives[target].completed = false;
        }
        for &target in declaration.kill.targets() {
            let objective = &mut self.objectives[target];
            objective.present = false;
            objective.active = false;
        }
        for &target in declaration.sleep.targets() {
            self.set_state(target, EngineState::Asleep);
        }
    }

    /// One pass of the engine's per-frame objective step, `dt` seconds long.
    ///
    /// The measured order is: the mission clock advances by `dt` once; then
    /// every present objective whose tick dependency holds runs its state's
    /// timer case, in objective order:
    ///
    /// * **Dormant** — when its wake slot is armed, it is not completed and
    ///   not active, and the mission clock has reached the slot: `wake` itself.
    /// * **Awake** — when its nap slot is armed its elapsed grows by `dt`;
    ///   on reaching the slot the state becomes
    ///   [`Napping`](EngineState::Napping) *before* its
    ///   `WAKE_OBJECTIVE_WHEN_I_SLEEP` list is woken, and unless that wake
    ///   returns the objective itself — the measured quirk that leaves it
    ///   awake — the state setter finishes the transition.
    /// * **Napping** — when its re-wake slot is armed its elapsed grows by
    ///   `dt`; on reaching the slot it `wake`s itself.
    /// * **Asleep** — no case body.
    ///
    /// After the case, the shared tail runs for whatever state the objective
    /// is now in: still [`Awake`](EngineState::Awake), with the sleep slot
    /// armed and the clock past it, it sleeps through the state setter **and
    /// then** wakes its `WAKE_OBJECTIVE_WHEN_I_SLEEP` list.
    ///
    /// Whether the engine still runs this pass once
    /// [`mission_ended`](Self::mission_ended) is set is outside the measured
    /// loop, so it is not gated here.
    ///
    /// The completion pass is a second loop the engine runs separately; it is
    /// driven here by [`complete`](Self::complete) because the conditions that
    /// fire it are other field families' measurements.
    pub fn advance(&mut self, dt: f32) -> TickOutcome {
        self.mission_clock += dt;
        let mut outcome = TickOutcome::default();
        for index in 0..self.objectives.len() {
            if !self.objectives[index].present || !self.is_ticked(index) {
                continue;
            }
            match self.objectives[index].state {
                EngineState::Dormant => {
                    let wake_at = self.objectives[index].timers.wake_at;
                    if wake_at >= 0.0
                        && !self.objectives[index].completed
                        && !self.objectives[index].active
                        && self.mission_clock >= wake_at
                    {
                        self.wake(&[index]);
                        outcome.woken.push(index);
                    }
                }
                EngineState::Awake => {
                    let nap_after = self.objectives[index].timers.nap_after;
                    if nap_after > 0.0 {
                        self.objectives[index].elapsed += dt;
                        if self.objectives[index].elapsed >= nap_after {
                            // The engine writes the state before the wake, so
                            // a list entry that wakes this objective itself
                            // does not take the already-awake early-out.
                            self.objectives[index].state = EngineState::Napping;
                            let list = self.objectives[index]
                                .declaration
                                .wake_when_i_sleep
                                .targets()
                                .to_vec();
                            if self.wake(&list) != Some(index) {
                                self.set_state(index, EngineState::Napping);
                            }
                            outcome.napped.push(index);
                        }
                    }
                }
                EngineState::Napping => {
                    let rewake_after = self.objectives[index].timers.rewake_after;
                    if rewake_after >= 0.0 {
                        self.objectives[index].elapsed += dt;
                        if self.objectives[index].elapsed >= rewake_after {
                            self.wake(&[index]);
                            outcome.woken.push(index);
                        }
                    }
                }
                EngineState::Asleep => {}
            }
            // The shared tail: whatever the objective's state is *now*, the
            // sleep timer only looks at an awake one.
            if self.objectives[index].state == EngineState::Awake {
                let sleep_at = self.objectives[index].timers.sleep_at;
                if sleep_at >= 0.0 && self.mission_clock >= sleep_at {
                    self.set_state(index, EngineState::Asleep);
                    let list = self.objectives[index]
                        .declaration
                        .wake_when_i_sleep
                        .targets()
                        .to_vec();
                    if !list.is_empty() {
                        self.wake(&list);
                    }
                    outcome.slept.push(index);
                }
            }
        }
        outcome
    }
}

/// What one [`EngineObjectives::advance`] pass did, in engine order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickOutcome {
    /// Objectives woken by a dormant or napping timer — the wake time or the
    /// re-wake time firing.
    pub woken: Vec<usize>,
    /// Objectives whose awake timer napped them.
    pub napped: Vec<usize>,
    /// Objectives whose sleep timer slept them.
    pub slept: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(declaration: EngineDeclaration, state: EngineState) -> EngineObjective {
        EngineObjective::new(state, declaration)
    }

    fn list(numbers: &[u32]) -> TargetList {
        TargetList::from_record(numbers).expect("list fits")
    }

    #[test]
    fn a_zero_ends_a_list_and_numbers_are_one_based() {
        assert_eq!(list(&[3, 1, 0, 5]).targets(), &[2, 0]);
        assert!(list(&[]).targets().is_empty());
        assert!(TargetList::from_record(&[1; ENGINE_LIST_CAPACITY]).is_err());
        assert!(TargetList::from_record(&[1; ENGINE_LIST_CAPACITY - 1]).is_ok());
    }

    #[test]
    fn wake_then_nap_on_one_target_leaves_it_napping() {
        let completing = EngineDeclaration {
            wake: list(&[2]),
            nap: Some(NapDeclaration::from_record(2, Some(2.0))),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(completing, EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Dormant),
        ])
        .expect("in range");
        set.complete(0);
        let target = set.get(1).expect("target");
        assert_eq!(target.state, EngineState::Napping);
        assert!(!target.active);
        assert!((target.timers.rewake_after - 2.0).abs() < f32::EPSILON);
        let source = set.get(0).expect("source");
        assert!(source.completed && !source.active);
        assert_eq!(source.state, EngineState::Asleep);
    }

    #[test]
    fn nap_expiry_wakes_the_target() {
        let completing = EngineDeclaration {
            nap: Some(NapDeclaration::from_record(2, Some(1.0))),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(completing, EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Awake),
        ])
        .expect("in range");
        set.complete(0);
        set.advance(0.5);
        assert_eq!(set.get(1).expect("target").state, EngineState::Napping);
        set.advance(0.5);
        let target = set.get(1).expect("target");
        assert_eq!(target.state, EngineState::Awake);
        assert!(target.active);
    }

    #[test]
    fn nap_clears_the_completed_flag_and_kill_blocks_a_later_wake() {
        let completing = EngineDeclaration {
            nap: Some(NapDeclaration::from_record(2, Some(1.0))),
            kill: list(&[3]),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(completing, EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Asleep),
            declared(EngineDeclaration::default(), EngineState::Dormant),
        ])
        .expect("in range");
        set.objectives[1].completed = true;
        set.complete(0);
        assert!(!set.get(1).expect("napper").completed);
        assert!(!set.get(2).expect("killed").present);
        assert_eq!(set.wake(&[2]), None);
        assert_eq!(set.get(2).expect("killed").state, EngineState::Dormant);
    }

    #[test]
    fn an_awake_target_ends_the_wake_list() {
        let mut set = EngineObjectives::new(vec![
            declared(EngineDeclaration::default(), EngineState::Dormant),
            declared(EngineDeclaration::default(), EngineState::Awake),
            declared(EngineDeclaration::default(), EngineState::Dormant),
        ])
        .expect("in range");
        assert_eq!(set.wake(&[0, 1, 2]), Some(1));
        assert_eq!(set.get(0).expect("first").state, EngineState::Awake);
        assert_eq!(set.get(2).expect("third").state, EngineState::Dormant);
    }

    #[test]
    fn the_tick_dependency_needs_the_named_objective_awake() {
        let dependent = EngineDeclaration {
            tick_depends_on: Some(0),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            declared(EngineDeclaration::default(), EngineState::Dormant),
            declared(dependent, EngineState::Awake),
        ])
        .expect("in range");
        assert!(!set.is_ticked(1));
        assert!(!set.evaluates_completion(1));
        set.wake(&[0]);
        assert!(set.is_ticked(1));
        assert!(set.evaluates_completion(1));
        set.complete(0);
        assert!(!set.is_ticked(1), "a completed dependency is asleep");
    }

    #[test]
    fn a_target_out_of_range_is_refused() {
        let bad = EngineDeclaration {
            kill: list(&[5]),
            ..EngineDeclaration::default()
        };
        let error = EngineObjectives::new(vec![declared(bad, EngineState::Awake)])
            .expect_err("index 4 of 1");
        assert_eq!(
            error,
            EngineEffectError::TargetOutOfRange {
                from: 0,
                target: 4,
                count: 1
            }
        );
    }

    // ------------------------------------------------------ M01-B-OBJ-2 ---

    fn dormant(elements: &[f32]) -> EngineDeclaration {
        EngineDeclaration {
            begin_dormant: Some(DormantTimers::from_elements(elements)),
            ..EngineDeclaration::default()
        }
    }

    #[test]
    fn a_begin_dormant_loads_dormant_and_inactive_its_absence_loads_awake() {
        let dormant = EngineObjective::loaded(EngineDeclaration {
            begin_dormant: Some(DormantTimers::from_elements(&[])),
            ..EngineDeclaration::default()
        });
        assert_eq!(dormant.state, EngineState::Dormant);
        assert!(!dormant.active);
        let awake = EngineObjective::loaded(EngineDeclaration::default());
        assert_eq!(awake.state, EngineState::Awake);
        assert!(awake.active);
    }

    #[test]
    fn dormant_slots_default_and_fill_from_elements() {
        let timers = DormantTimers::from_elements(&[2.0]);
        assert_eq!(timers.wake_at, 2.0);
        assert_eq!(timers.nap_after, -1.0);
        assert_eq!(timers.rewake_after, -1.0);
        assert_eq!(timers.sleep_at, -1.0);
        // Elements past the fourth are never looked at.
        let full = DormantTimers::from_elements(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(full.sleep_at, 4.0);
    }

    #[test]
    fn the_armed_tests_are_per_slot_and_asymmetric() {
        assert!(
            DormantTimers::from_elements(&[0.0]).wake_armed(),
            "wake arms at >= 0"
        );
        assert!(
            !DormantTimers::from_elements(&[0.0, 0.0]).nap_armed(),
            "nap arms only at > 0"
        );
        assert!(DormantTimers::from_elements(&[0.0, 0.0, 0.0]).rewake_armed());
        assert!(DormantTimers::from_elements(&[0.0, 0.0, 0.0, 0.0]).sleep_armed());
    }

    #[test]
    fn a_dormant_objective_wakes_when_the_clock_reaches_its_slot() {
        let mut set = EngineObjectives::new(vec![EngineObjective::loaded(dormant(&[2.0]))])
            .expect("in range");
        let first = set.advance(1.0);
        assert_eq!(set.mission_clock(), 1.0);
        assert!(first.woken.is_empty());
        assert_eq!(set.get(0).expect("objective").state, EngineState::Dormant);
        let second = set.advance(1.0);
        assert_eq!(second.woken, vec![0]);
        let objective = set.get(0).expect("objective");
        assert_eq!(objective.state, EngineState::Awake);
        assert!(objective.active);
    }

    #[test]
    fn the_minus_one_sentinel_never_self_wakes_but_an_external_wake_does() {
        let mut set = EngineObjectives::new(vec![EngineObjective::loaded(dormant(&[-1.0]))])
            .expect("in range");
        for _ in 0..5 {
            assert!(set.advance(10.0).woken.is_empty());
        }
        assert_eq!(set.get(0).expect("objective").state, EngineState::Dormant);
        set.wake(&[0]);
        assert_eq!(set.get(0).expect("objective").state, EngineState::Awake);
    }

    #[test]
    fn an_awake_objective_naps_after_its_awake_time_then_rewakes() {
        // BEGIN_DORMANT [-1, 4, 1]: never self-wake, nap 4s after waking,
        // re-wake 1s after napping.
        let mut set =
            EngineObjectives::new(vec![EngineObjective::loaded(dormant(&[-1.0, 4.0, 1.0]))])
                .expect("in range");
        set.wake(&[0]);
        let outcome = set.advance(2.0);
        assert!(
            outcome.napped.is_empty(),
            "the nap timer needs 4 awake seconds"
        );
        let outcome = set.advance(2.0);
        assert_eq!(outcome.napped, vec![0]);
        assert_eq!(set.get(0).expect("objective").state, EngineState::Napping);
        let outcome = set.advance(1.0);
        assert_eq!(outcome.woken, vec![0]);
        assert_eq!(set.get(0).expect("objective").state, EngineState::Awake);
    }

    #[test]
    fn an_awake_objective_sleeps_when_the_clock_reaches_its_slot() {
        // BEGIN_DORMANT [-1, -1, -1, 3]: never self-wake, sleep at clock 3.
        let mut set = EngineObjectives::new(vec![EngineObjective::loaded(dormant(&[
            -1.0, -1.0, -1.0, 3.0,
        ]))])
        .expect("in range");
        set.wake(&[0]);
        assert!(set.advance(2.0).slept.is_empty());
        let outcome = set.advance(2.0);
        assert_eq!(outcome.slept, vec![0]);
        assert_eq!(set.get(0).expect("objective").state, EngineState::Asleep);
        assert!(!set.get(0).expect("objective").active);
    }

    #[test]
    fn the_when_i_sleep_list_is_woken_on_the_auto_transitions() {
        let napping = EngineDeclaration {
            begin_dormant: Some(DormantTimers::from_elements(&[-1.0, 1.0])),
            wake_when_i_sleep: list(&[2]),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            EngineObjective::loaded(napping),
            EngineObjective::loaded(dormant(&[-1.0])),
        ])
        .expect("in range");
        set.wake(&[0]);
        let outcome = set.advance(1.0);
        assert_eq!(outcome.napped, vec![0]);
        assert_eq!(
            outcome.woken,
            Vec::<usize>::new(),
            "the when-i-sleep wake is a wake(), not a timer wake"
        );
        assert_eq!(set.get(1).expect("list target").state, EngineState::Awake);
    }

    #[test]
    fn a_when_i_sleep_list_naming_the_objective_once_still_naps_it() {
        // The engine writes state=2, then wakes the list: the self entry is
        // woken back awake (cue and elapsed reset included), but the list
        // ends on its terminator, which returns -1 — not the objective — so
        // the state setter runs anyway and the objective ends napping.
        let quirky = EngineDeclaration {
            begin_dormant: Some(DormantTimers::from_elements(&[-1.0, 1.0])),
            wake_when_i_sleep: list(&[1]),
            wakeup_sound_group: Some("snd_self".to_owned()),
            ..EngineDeclaration::default()
        };
        let mut set =
            EngineObjectives::new(vec![EngineObjective::loaded(quirky)]).expect("in range");
        set.wake(&[0]);
        set.clear_outputs();
        let outcome = set.advance(1.0);
        assert_eq!(outcome.napped, vec![0], "the timer fired");
        let objective = set.get(0).expect("objective");
        assert_eq!(objective.state, EngineState::Napping);
        assert!(!objective.active);
        assert_eq!(objective.elapsed, 0.0);
        assert_eq!(set.cues().len(), 1, "the self-wake still emitted its cue");
        assert_eq!(set.cues()[0].kind, CueKind::Wakeup);
    }

    #[test]
    fn a_when_i_sleep_list_naming_the_objective_twice_leaves_it_awake() {
        // The second self entry meets the first one's awake state, so the
        // wake takes its already-awake early-out and returns the objective —
        // the one return value that skips the state setter.
        let quirky = EngineDeclaration {
            begin_dormant: Some(DormantTimers::from_elements(&[-1.0, 1.0])),
            wake_when_i_sleep: list(&[1, 1]),
            ..EngineDeclaration::default()
        };
        let mut set =
            EngineObjectives::new(vec![EngineObjective::loaded(quirky)]).expect("in range");
        set.wake(&[0]);
        let outcome = set.advance(1.0);
        assert_eq!(outcome.napped, vec![0], "the timer fired");
        assert_eq!(
            set.get(0).expect("objective").state,
            EngineState::Awake,
            "the engine's self-wake quirk skips the state setter"
        );
    }

    #[test]
    fn waking_emits_the_wakeup_cue_and_completing_emits_its_pair() {
        let declaration = EngineDeclaration {
            begin_dormant: Some(DormantTimers::from_elements(&[0.5])),
            wakeup_sound_group: Some("snd_wake".to_owned()),
            completed_sound_group: Some("snd_done".to_owned()),
            identity: Some(EngineIdentity::from_record("SECONDARY", 7, None)),
            ..EngineDeclaration::default()
        };
        let mut set =
            EngineObjectives::new(vec![EngineObjective::loaded(declaration)]).expect("in range");
        set.advance(1.0);
        assert_eq!(set.cues().len(), 1);
        assert_eq!(set.cues()[0].kind, CueKind::Wakeup);
        assert_eq!(set.cues()[0].group.as_deref(), Some("snd_wake"));

        set.complete(0);
        assert_eq!(set.cues().len(), 3);
        assert_eq!(set.cues()[1].kind, CueKind::Completed);
        assert_eq!(set.cues()[1].group.as_deref(), Some("snd_done"));
        assert_eq!(set.cues()[2].kind, CueKind::RoleJingle);
        assert_eq!(set.cues()[2].group, None);
        assert_eq!(
            set.hud_notifications(),
            &[HudNotification {
                objective: 0,
                role_code: 2,
                ordinal: 7,
            }]
        );
    }

    #[test]
    fn the_hud_notification_is_once_and_gated_on_the_mission_not_ending() {
        let declaration = EngineDeclaration {
            identity: Some(EngineIdentity::from_record(
                "PRIMARY",
                1,
                Some("msg".to_owned()),
            )),
            ..EngineDeclaration::default()
        };
        let mut set = EngineObjectives::new(vec![
            EngineObjective::loaded(declaration.clone()),
            EngineObjective::loaded(declaration),
        ])
        .expect("in range");
        set.mission_ended = true;
        set.complete(0);
        assert!(
            set.hud_notifications().is_empty(),
            "mission ended: no notify"
        );
        assert!(
            set.get(0).expect("objective").hud_notify_pending,
            "stays pending"
        );
        set.mission_ended = false;
        set.complete(1);
        assert_eq!(set.hud_notifications().len(), 1);
        // An objective with no role emits the jingle's siblings but no pair.
    }

    #[test]
    fn the_count_check_needs_stages_and_the_threshold() {
        let declaration = EngineDeclaration {
            inactive_completion_count: Some(5),
            declared_inactive_stages: 18,
            ..EngineDeclaration::default()
        };
        let set =
            EngineObjectives::new(vec![EngineObjective::loaded(declaration)]).expect("in range");
        assert_eq!(set.inactive_count_threshold(0), Some(5));
        assert!(!set.inactive_count_met(0, 4));
        assert!(set.inactive_count_met(0, 5));
        assert_eq!(set.count_threshold_reachable(0), Some(true));
    }

    #[test]
    fn the_count_defaults_to_the_stage_count_and_zero_stages_never_met() {
        let staged = EngineDeclaration {
            declared_inactive_stages: 9,
            ..EngineDeclaration::default()
        };
        let unstaged = EngineDeclaration {
            inactive_completion_count: Some(1),
            ..EngineDeclaration::default()
        };
        let set = EngineObjectives::new(vec![
            EngineObjective::loaded(staged),
            EngineObjective::loaded(unstaged),
        ])
        .expect("in range");
        assert_eq!(
            set.inactive_count_threshold(0),
            Some(9),
            "absent count defaults to the stage count"
        );
        assert!(!set.inactive_count_met(0, 8));
        assert!(set.inactive_count_met(0, 9));
        assert_eq!(set.count_threshold_reachable(1), Some(false));
        assert!(
            !set.inactive_count_met(1, 100),
            "no stages: the checker never fires"
        );
    }

    #[test]
    fn the_identity_resolves_role_codes_case_insensitively() {
        assert_eq!(EngineIdentity::from_record("PRIMARY", 1, None).role_code, 1);
        assert_eq!(
            EngineIdentity::from_record("secondary", 2, None).role_code,
            2
        );
        assert_eq!(
            EngineIdentity::from_record("Tertiary", 3, None).role_code,
            3
        );
        assert_eq!(EngineIdentity::from_record("bonus", 4, None).role_code, 0);
        let objective = EngineObjective::loaded(EngineDeclaration {
            identity: Some(EngineIdentity::from_record("SECONDARY", 11, None)),
            ..EngineDeclaration::default()
        });
        assert_eq!(objective.role_code, 2);
        assert_eq!(objective.ordinal, 11);
    }
}
