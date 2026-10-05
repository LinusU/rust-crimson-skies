//! The pilot-bailout mission transition: **distinct from destruction**, and
//! carried by no counted category.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (task F29-C.4), acceptance case **AC04** — *"Bailout and
//! ordinary death trigger the correct distinct mission transitions"* — and
//! non-negotiable behaviors 3 and 4. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md` ("Results, previous targets and
//! delayed callbacks are always generation-qualified").
//!
//! # What F29-C left open
//!
//! The damage domain has the vocabulary and the authority:
//! [`LifecycleKind::PilotBailout`] is a record of its own, written through
//! [`crate::damage::DamageResolver::record_lifecycle`], and the objective layer
//! already *refuses to count* it —
//! [`CountKind::from_lifecycle`](super::counters::CountKind::from_lifecycle)
//! answers `None`, the mission layer writes
//! [`ActorState::Escaped`] never
//! ([`PILOT_BAILOUT_WRITES_NO_STATE`]), and the targeting store keeps a
//! bailed-out airframe eligible. What did **not** exist was a consumer that
//! turns the record into a *mission transition*. The F29-C finding named this
//! as the open follow-up
//! (`docs/findings/2026-10-02-f29-c-damage-consumers.md`), and this module is
//! it: an observed bailout becomes a transition a mission program can read,
//! and it is not the destruction transition.
//!
//! [`ActorState::Escaped`]: cs_script::ir::ActorState::Escaped
//! [`PILOT_BAILOUT_WRITES_NO_STATE`]: crate::mission::PILOT_BAILOUT_WRITES_NO_STATE
//!
//! # The four things made structural
//!
//! 1. **Two transitions, one enum.** [`MissionTransition`] has exactly two
//!    variants the sheet separates, and [`MissionTransition::from_lifecycle`] is
//!    the only place a [`LifecycleKind`] becomes one. The other three lifecycle
//!    kinds are deliberately *not* mission transitions here: capture and despawn
//!    are counted categories F39-E4 measured, and mission removal is accounting
//!    (F37-E1), so mapping them here would invent a second answer to a question
//!    another module already owns.
//! 2. **A confirmation before a transition.** A bailout needs a
//!    [`BailoutConfirmation`]: the declared eject edge delivered by the modern
//!    binding layer in a context that accepts flight commands, or a mission
//!    program's own request. A rendered parachute is not a confirmation, so an
//!    unconfirmed [`LifecycleKind::PilotBailout`] is **refused by name** and
//!    produces no transition at all — non-negotiable 4's "do not grant survival
//!    or success just because a parachute renders" as a gate rather than a
//!    comment. The context gate is [`InputContext::accepts`], the same
//!    production predicate F22 uses, so a cinematic or a text-entry context
//!    cannot confirm an ejection.
//! 3. **A declared mission-result policy that grants nothing.**
//!    [`BailoutResultPolicy`] has one variant, [`BailoutResultPolicy::Unmeasured`],
//!    and it requests **no** terminal outcome and credits **no** survival. It
//!    is the declaration, not a guess: the runtime asks the policy what a
//!    bailout does to the mission and gets "nothing measured", so the seam
//!    exists for a measured rule to fill without touching the phase.
//! 4. **The first transition for an actor is the one kept.**
//!    [`MissionTransitions`] latches per actor, so a destruction report that
//!    arrives *after* a bailout cannot turn it into a kill, and a bailout
//!    reported after a destruction cannot undo it. This is the same
//!    first-terminal-report rule [`crate::net_state`] applies to the network
//!    summary, and it is why the ledger is a per-actor latch rather than a set:
//!    "not counted" alone would let the second report through.
//!
//! # What is measured, and what is not
//!
//! **Measured (vocabulary only).** The owner's installation names an ejection
//! as authored content, and names it *in several voices*:
//!
//! | what | where (read-only census, `strings` over the archives) |
//! | --- | --- |
//! | the ejected pilot object | `zbd/zrdr.zbd` → `..\data\common\zrdr\objects\pilot_eject.zrd` |
//! | a parachute beside it | `..\data\common\zrdr\objects\chuteman.zrd`, with nodes `chuteman`, `chutemanparent` |
//! | the animation the pilot plays | `cpilot_eject.zan` / `cpilot_eject2.zan` under cue names `cpeject1`, `cpeject2`, `cpejectstop`, driving `OBJECT_MOTION_SI_SCRIPT` on a `cpilot` child of `player`/`pilot_pos` |
//! | sounds | `snd_pilot_eject1` → `pilot_eject1.wav`, `snd_chuteopen` → `chuteopen.wav` |
//! | voice lines | `VO_id1_DA-Bail-A/B`, `VO_id1_DA-NoBail-A/B` and per-wingman `vo_*_DE-Bail-A.wav`, the latter spelled **No**Bail |
//! | mission-side node names | `zbd/C2/MP2/mis_anim.zbd` and its siblings carry a bare `eject` node name |
//! | camera binding | every `zbd/C*/cam_anim.zbd` references `pilot_eject.zrd` and `cpilot_eject.zan` |
//!
//! Two things follow, and only two. **An ejection exists, with a parachute and a
//! camera treatment** — so it is authored content, not a guess. And the game
//! distinguishes *bailing out* from *not* bailing out in its own radio script,
//! which is why the mission layer here must keep a bailout and an ordinary
//! death apart rather than folding both into one "gone" flag.
//!
//! **Not measured, and therefore not claimed.** Nothing in that census says what
//! a bailout **does to a mission**. No `OBJECTIVE<N>` key in any reader
//! archive's `objectives.zrd` spells an ejected-pilot category or a
//! bailout outcome (F39-E4's census measured the five counted categories and
//! found none for a pilot who is alive), the compiled program behind those
//! records is still undecoded (F13-B/C, F38), and no original run exists in this
//! project. A name is not a rule, and a `.zan` file is not a mission result.
//! The policy here is therefore `Unmeasured`: it withholds every mission result
//! and credits no survival, and the original's rule is **not** inverted into a
//! plausible one. **Affected content:** every airframe a mission can lose a
//! pilot from, and every mission that would score such a loss. **Resolving
//! task:** F29-D, with `retail` for the records and `human_review` from the owner
//! for the result question itself — an agent can measure what a mission declares
//! and cannot measure what the game did with it.
//!
//! Nothing in this module is an original-fidelity claim; the vocabulary above is
//! a file-content measurement and every behaviour here is this engine's
//! **designed** reading.

use std::collections::BTreeMap;
use std::fmt;

use cs_script::ir::{ActorId, SymbolId};
use cs_types::input::{Action, FlightCommand, InputContext};

use crate::damage::LifecycleKind;

use super::terminal::TerminalOutcome;

/// The named reason a bailout's mission result is withheld.
///
/// F29 non-negotiable 4 asks for a "data/evidence-backed mission result
/// policy". No such data was recovered (see the module docs), so the policy
/// names the absence instead of filling it: this string is what
/// [`BailoutResultPolicy::reason`] returns, and it is the text a report or an
/// evidence record carries.
pub const BAILOUT_RESULT_POLICY_UNMEASURED: &str = "pilot_bailout_mission_result_unmeasured";

/// What a pilot bailout means for the mission's **result**, as a declaration.
///
/// The enum carries one variant, and that is the honest answer rather than a
/// placeholder: the original's rule is unmeasured, so the only policy this
/// engine may declare is the one that **withholds**. A measured rule becomes a
/// new named variant — the same way
/// [`TerminalPrecedence::SyntheticConservative`](super::terminal::TerminalPrecedence::SyntheticConservative)
/// is a *designed* policy that a measurement replaces rather than overwrites —
/// so every existing caller keeps its meaning and the runtime's policy seam
/// needs no edit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BailoutResultPolicy {
    /// Nothing measured says what the original did, so nothing is granted: the
    /// transition is recorded and the mission continues.
    ///
    /// It deliberately does **not** default to a failure or a success. Both
    /// would be the original's rule, and neither is known; what *is* known is
    /// that a mission must not end because a parachute rendered.
    #[default]
    Unmeasured,
}

impl BailoutResultPolicy {
    /// The stable label used in reports and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unmeasured => "unmeasured",
        }
    }

    /// Whether this policy came from a measurement. Always `false`: no
    /// measured bailout rule exists in this project.
    #[must_use]
    pub const fn is_measured(self) -> bool {
        match self {
            Self::Unmeasured => false,
        }
    }

    /// Why this policy grants what it grants, by name.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Unmeasured => BAILOUT_RESULT_POLICY_UNMEASURED,
        }
    }

    /// The terminal outcome a confirmed bailout requests. `None` under the only
    /// policy: a bailout is not a mission ending.
    ///
    /// This is the seam the objective runtime asks once per applied bailout
    /// transition, so a measured policy plugs in without the phase changing.
    #[must_use]
    pub const fn terminal_outcome(self) -> Option<TerminalOutcome> {
        match self {
            Self::Unmeasured => None,
        }
    }

    /// Whether this policy credits the pilot's survival. `false` under the only
    /// policy, and it stays a separate question from
    /// [`Self::terminal_outcome`] so "the mission carries on" can never be read
    /// as "the pilot lived".
    #[must_use]
    pub const fn grants_survival(self) -> bool {
        match self {
            Self::Unmeasured => false,
        }
    }
}

impl fmt::Display for BailoutResultPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The two mission transitions F29 keeps apart.
///
/// Destruction is a kill; a bailout is a live pilot leaving an airframe. They
/// are one enum rather than two booleans so no consumer can read one and infer
/// the other, and [`MissionTransition::is_kill`] is the single place that says
/// which one scores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissionTransition {
    /// The airframe was destroyed: the kill path, the one
    /// [`CountKind::Destroyed`](super::counters::CountKind::Destroyed) counts.
    Destroyed,
    /// The pilot left a live airframe: **not** a kill, not a count, and by
    /// itself not a mission ending.
    PilotBailedOut,
}

impl MissionTransition {
    /// Every transition, in declaration order.
    pub const ALL: &'static [MissionTransition] = &[Self::Destroyed, Self::PilotBailedOut];

    /// The stable label used in reports and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::PilotBailedOut => "pilot_bailed_out",
        }
    }

    /// The lifecycle transition this mission transition is recorded for, or
    /// `None` for one this ledger does not own.
    #[must_use]
    pub const fn lifecycle(self) -> LifecycleKind {
        match self {
            Self::Destroyed => LifecycleKind::Destroyed,
            Self::PilotBailedOut => LifecycleKind::PilotBailout,
        }
    }

    /// The mission transition a lifecycle transition causes, or `None` for the
    /// three kinds this ledger deliberately does not answer.
    ///
    /// `OwnershipCaptured` and `Despawned` are the counted categories F39-E4
    /// measured and the actor states F37-E1 writes; `MissionRemoved` is the
    /// accounting removal. Giving them a mission transition here would be a
    /// second, unmeasured answer to a question those modules already own — the
    /// refusal F39-E4 recorded for a category with no producer.
    #[must_use]
    pub const fn from_lifecycle(kind: LifecycleKind) -> Option<Self> {
        match kind {
            LifecycleKind::Destroyed => Some(Self::Destroyed),
            LifecycleKind::PilotBailout => Some(Self::PilotBailedOut),
            LifecycleKind::OwnershipCaptured
            | LifecycleKind::Despawned
            | LifecycleKind::MissionRemoved => None,
        }
    }

    /// Whether this transition awards a kill. Only destruction does.
    #[must_use]
    pub const fn is_kill(self) -> bool {
        match self {
            Self::Destroyed => true,
            Self::PilotBailedOut => false,
        }
    }
}

impl fmt::Display for MissionTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// That a pilot really left the airframe, in a form the mission layer accepts.
///
/// A lifecycle transition alone is not enough (F29 non-negotiable 4): the
/// mission needs to know the departure was *asked for* — by the player's own
/// eject edge under a binding, or by a mission program — before it records a
/// transition. There is deliberately no third variant for "the chute opened":
/// that is a presentation fact and non-negotiable 1 already forbids gameplay
/// state being inferred from one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BailoutConfirmation {
    /// The local player pressed the declared eject command while the flight
    /// context owned the devices.
    ///
    /// `FlightCommand::Eject` is this project's modern binding vocabulary
    /// (F22-A) and it is delivered as a once-per-press edge by
    /// [`crate::control::ControlBuffer`]; the context is carried so the gate can
    /// be checked where the confirmation is recorded rather than trusted from
    /// the caller.
    PlayerEject { context: InputContext },
    /// A mission program asked for the pilot to leave. No local input is
    /// involved, so there is no context to gate.
    Scripted { requester: SymbolId },
}

impl BailoutConfirmation {
    /// The confirmation one input edge carries.
    ///
    /// # Errors
    ///
    /// [`BailoutRefusal::NotAnEjectCommand`] when the edge is a flight command
    /// that is not the declared eject command, and
    /// [`BailoutRefusal::InputContextRefused`] when the context that owned the
    /// devices accepts no flight command — the F22 gate that stops a cinematic
    /// or a text-entry context from ejecting.
    pub fn from_action(action: Action, context: InputContext) -> Result<Self, BailoutRefusal> {
        let Action::Flight(command) = action else {
            return Err(BailoutRefusal::NotAnEjectEdge { action, context });
        };
        if command != FlightCommand::Eject {
            return Err(BailoutRefusal::NotAnEjectCommand { command });
        }
        let confirmation = Self::PlayerEject { context };
        confirmation.admits()?;
        Ok(confirmation)
    }

    /// The confirmation a mission program's own request carries.
    #[must_use]
    pub const fn scripted(requester: SymbolId) -> Self {
        Self::Scripted { requester }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PlayerEject { .. } => "player_eject",
            Self::Scripted { .. } => "scripted",
        }
    }

    /// Whether this confirmation may record a bailout transition.
    ///
    /// # Errors
    ///
    /// [`BailoutRefusal::InputContextRefused`] for a player eject whose context
    /// accepts no flight command. The check runs again where the confirmation is
    /// recorded rather than only at construction, so a struct literal cannot
    /// carry an ungated context past the gate.
    pub fn admits(self) -> Result<(), BailoutRefusal> {
        match self {
            Self::PlayerEject { context } => {
                if context.accepts(Action::Flight(FlightCommand::Eject)) {
                    Ok(())
                } else {
                    Err(BailoutRefusal::InputContextRefused { context })
                }
            }
            Self::Scripted { .. } => Ok(()),
        }
    }
}

impl fmt::Display for BailoutConfirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why no mission transition was recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BailoutRefusal {
    /// The input edge was not a flight command at all, so there is no eject to
    /// confirm.
    NotAnEjectEdge {
        /// The action the edge carried.
        action: Action,
        /// The context that owned the devices.
        context: InputContext,
    },
    /// The edge was a flight command, but not the declared eject command.
    NotAnEjectCommand { command: FlightCommand },
    /// The input context that owned the devices accepts no flight command, so it
    /// cannot confirm an ejection.
    InputContextRefused { context: InputContext },
    /// The actor already holds a confirmation this session.
    AlreadyConfirmed { actor: ActorId },
    /// A pilot bailout was reported with no confirmation beside it. A rendering
    /// parachute is not a pilot leaving the airframe, so nothing is recorded.
    Unconfirmed { actor: ActorId },
    /// The actor's mission transition is already latched. The *first* one is
    /// kept, so a later destruction report cannot turn a bailout into a kill and
    /// a later bailout cannot undo a kill.
    AlreadyTransitioned {
        /// The actor whose record is closed.
        actor: ActorId,
        /// The transition the caller asked for.
        requested: MissionTransition,
        /// The transition the record holds.
        kept: MissionTransition,
    },
}

impl BailoutRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NotAnEjectEdge { .. } => "not_an_eject_edge",
            Self::NotAnEjectCommand { .. } => "not_an_eject_command",
            Self::InputContextRefused { .. } => "input_context_refused",
            Self::AlreadyConfirmed { .. } => "already_confirmed",
            Self::Unconfirmed { .. } => "unconfirmed_bailout",
            Self::AlreadyTransitioned { .. } => "already_transitioned",
        }
    }
}

impl fmt::Display for BailoutRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnEjectEdge { action, context } => {
                write!(f, "{action} in the {context} context is not an eject edge")
            }
            Self::NotAnEjectCommand { command } => {
                write!(f, "{command} is not the declared eject command")
            }
            Self::InputContextRefused { context } => {
                write!(
                    f,
                    "the {context} context accepts no flight command, so it cannot eject"
                )
            }
            Self::AlreadyConfirmed { actor } => {
                write!(f, "actor {actor:?} already holds a bailout confirmation")
            }
            Self::Unconfirmed { actor } => write!(
                f,
                "actor {actor:?} reported a pilot bailout with no confirmed input, so none is recorded"
            ),
            Self::AlreadyTransitioned {
                actor,
                requested,
                kept,
            } => write!(
                f,
                "actor {actor:?} already ended as {kept}, so the later {requested} is refused"
            ),
        }
    }
}

impl std::error::Error for BailoutRefusal {}

/// One mission transition that was recorded.
///
/// `confirmation` is also the **discriminator** between the two transitions,
/// which is what lets the objective runtime branch on it rather than on
/// `transition`: a bailout is only ever applied with one, because
/// [`MissionTransitions::observe`] refuses it otherwise, and a destruction needs
/// none. Nothing here ever substitutes a default confirmation for a missing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppliedTransition {
    /// The actor that transitioned.
    pub actor: ActorId,
    /// Which transition.
    pub transition: MissionTransition,
    /// The confirmation a bailout transition carried. `None` for destruction,
    /// which needs none.
    pub confirmation: Option<BailoutConfirmation>,
}

/// What one reported lifecycle transition did to the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransitionOutcome {
    /// First transition for this actor; recorded.
    Applied(AppliedTransition),
    /// The actor already holds this exact transition. Idempotent, and nothing
    /// new to report — the same shape the once-per-category counters already
    /// have.
    Repeated(MissionTransition),
    /// Refused by name; the actor's record is unchanged.
    Refused(BailoutRefusal),
}

/// The per-actor mission-transition ledger of one session generation.
///
/// Owned by one [`ObjectiveRuntime`](super::runtime::ObjectiveRuntime), so it is
/// generation-qualified the way the shared contract requires: a retry builds a
/// fresh runtime and a fresh ledger, and nothing an earlier generation recorded
/// is reachable from the new one.
///
/// It holds two things and nothing else: the **first** mission transition per
/// actor, and the confirmations handed to it before that transition arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionTransitions {
    /// The first mission transition each actor reached.
    kept: BTreeMap<ActorId, MissionTransition>,
    /// Confirmations recorded ahead of the transition they confirm.
    confirmations: BTreeMap<ActorId, BailoutConfirmation>,
    /// The declared mission-result policy, asked once per applied bailout.
    policy: BailoutResultPolicy,
}

impl Default for MissionTransitions {
    fn default() -> Self {
        Self::new(BailoutResultPolicy::default())
    }
}

impl MissionTransitions {
    /// An empty ledger under `policy`.
    #[must_use]
    pub const fn new(policy: BailoutResultPolicy) -> Self {
        Self {
            kept: BTreeMap::new(),
            confirmations: BTreeMap::new(),
            policy,
        }
    }

    /// This ledger under another policy. A measured rule replaces the
    /// unmeasured one here without any caller changing.
    #[must_use]
    pub fn with_policy(mut self, policy: BailoutResultPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// The declared mission-result policy.
    #[must_use]
    pub const fn policy(&self) -> BailoutResultPolicy {
        self.policy
    }

    /// Records the confirmation for `actor`, so the transition it confirms is
    /// not refused as unconfirmed.
    ///
    /// Validation happens before anything is written, so a refused confirmation
    /// mutates nothing. A second confirmation for the same actor is refused: the
    /// first edge is the one the transition belongs to.
    ///
    /// # Errors
    ///
    /// [`BailoutRefusal::InputContextRefused`] for a confirmation the input
    /// context gate rejects, [`BailoutRefusal::AlreadyConfirmed`] for a repeated
    /// confirmation, and [`BailoutRefusal::AlreadyTransitioned`] when the actor's
    /// transition is already latched — a pilot cannot leave an airframe that is
    /// already a wreck.
    pub fn confirm(
        &mut self,
        actor: ActorId,
        confirmation: BailoutConfirmation,
    ) -> Result<(), BailoutRefusal> {
        confirmation.admits()?;
        if let Some(kept) = self.kept.get(&actor) {
            return Err(BailoutRefusal::AlreadyTransitioned {
                actor,
                requested: MissionTransition::PilotBailedOut,
                kept: *kept,
            });
        }
        if self.confirmations.contains_key(&actor) {
            return Err(BailoutRefusal::AlreadyConfirmed { actor });
        }
        self.confirmations.insert(actor, confirmation);
        Ok(())
    }

    /// The confirmation recorded for `actor` and not yet consumed, for
    /// inspection. It is consumed by the transition it confirms.
    #[must_use]
    pub fn pending_confirmation(&self, actor: ActorId) -> Option<BailoutConfirmation> {
        self.confirmations.get(&actor).copied()
    }

    /// The transition `actor` reached, or `None` when it reached none.
    #[must_use]
    pub fn transition(&self, actor: ActorId) -> Option<MissionTransition> {
        self.kept.get(&actor).copied()
    }

    /// The actors that reached a transition, in id order.
    pub fn transitioned(&self) -> impl Iterator<Item = ActorId> + '_ {
        self.kept.keys().copied()
    }

    /// Folds one reported lifecycle transition in.
    ///
    /// * a transition the actor already holds is [`TransitionOutcome::Repeated`]
    ///   and changes nothing;
    /// * the *other* transition for an actor that already holds one is
    ///   [`TransitionOutcome::Refused`] and names both — the first report is the
    ///   one kept, so a destruction arriving after a bailout cannot award a kill
    ///   and a bailout arriving after a destruction cannot undo it;
    /// * a bailout with no confirmation is
    ///   [`TransitionOutcome::Refused`] as unconfirmed, and records nothing.
    pub fn observe(&mut self, actor: ActorId, transition: MissionTransition) -> TransitionOutcome {
        if let Some(kept) = self.kept.get(&actor) {
            return if *kept == transition {
                TransitionOutcome::Repeated(*kept)
            } else {
                TransitionOutcome::Refused(BailoutRefusal::AlreadyTransitioned {
                    actor,
                    requested: transition,
                    kept: *kept,
                })
            };
        }
        let confirmation = if transition == MissionTransition::PilotBailedOut {
            match self.confirmations.remove(&actor) {
                // The gate is asked again here, not only in `confirm`, because
                // every field is public and a struct literal can carry a context
                // that never owned the devices.
                Some(confirmation) => match confirmation.admits() {
                    Ok(()) => Some(confirmation),
                    Err(refusal) => return TransitionOutcome::Refused(refusal),
                },
                None => return TransitionOutcome::Refused(BailoutRefusal::Unconfirmed { actor }),
            }
        } else {
            Option::None
        };
        self.kept.insert(actor, transition);
        TransitionOutcome::Applied(AppliedTransition {
            actor,
            transition,
            confirmation,
        })
    }
}
