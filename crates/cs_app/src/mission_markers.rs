//! The mission/objective consumer of the animation playback's gameplay
//! markers (F20-C, task #507).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`, non-negotiable behaviors 1 and 5. Shared contracts:
//! `docs/contracts/IDENTITY-CONTENT.md` (session generations, `EventId`) and
//! `docs/contracts/SCRIPT-MISSION.md` ("Mission state" and "Objective event
//! ordering").
//!
//! # What this closes
//!
//! F20-C's integration step made markers reach
//! [`AnimationLog`](crate::animation::AnimationLog) from a real session and
//! recorded, in
//! `docs/findings/2026-10-02-f20-c-wired-session-integration.md` finding 7,
//! that *nothing consumed them*: "the layer that turns them into gameplay is
//! the mission/objective layer (F37/F39), which does not exist. Inventing a
//! parallel 'marker registry' here would be a guess about a layer this task
//! does not own."
//!
//! That layer exists now — F39-C's [`ObjectiveSession`] — so this module is the
//! consumer it was waiting for, and it is the *mission* half of it:
//!
//! | end | what it is |
//! | --- | --- |
//! | producer | [`AnimationLog`](crate::animation::AnimationLog): the fixed-tick playback publishes every marker crossing into the log and nothing reads it |
//! | hand-off | [`MissionMarkerConsumer::drain`]: takes the marker records through [`AnimationLog::take_markers`](crate::animation::AnimationLog::take_markers) and leaves the other kinds for their consumers |
//! | binding | [`MissionMarkerBindings`]: the mission host's **declared** cue → mission-signal table, supplied by the caller |
//! | consumer | [`step_mission_with_markers`]: raises the delivered signals into a real [`ObjectiveSession`] step, where the program's own declared rules decide what a signal means |
//! | trace | [`MarkerDelivery`]: every marker applied, every refusal named, every presentation cue counted and not applied |
//!
//! # A gameplay marker becomes a declared mission signal, and nothing else
//!
//! A fired marker names an authored **cue label**
//! ([`MarkerEffect::Gameplay { cue }`](cs_sim::animated_object::MarkerEffect)),
//! not a mission symbol. Which mission signal a cue raises is therefore a
//! **content binding**, and this module ships none of it: a cue the host did
//! not bind is refused by name ([`MarkerRefusal::UnboundCue`]) and its gameplay
//! transition does not happen, exactly like a marker whose effect is
//! [`Resolved::Unknown`](cs_types::content::Resolved) is blocked rather than
//! guessed. Inventing a symbol for a cue would be a guess about a mission's
//! program, and a guessed symbol in a mission trigger table is worse than a
//! missing one — the same boundary
//! `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md` drew when it
//! refused to invent an objective trigger symbol for a collider.
//!
//! The signal is also the *only* request this consumer makes. It does not set,
//! reveal, complete or fail an objective, spawn a wave, arm a timer or grant a
//! reward: F39 non-negotiable behavior 3 requires that "reaching a waypoint can
//! unlock or reset objectives only through a specific program action", and the
//! declared program action is reached through the signal the program's own
//! rules listen for. A marker therefore reaches the mission layer the way any
//! other producer reaches it — as a fact in [`TickInput::signals`].
//!
//! # "Exactly once per activation" is this layer's rule, not the evaluator's
//!
//! F20 non-negotiable behavior 5 says reversing or skipping a cinematic must
//! not duplicate pickups, ammo or mission events. The evaluator already keeps
//! a one-shot gameplay marker to one firing per activation
//! ([`fired_gameplay`](cs_sim::animated_object)), and the playback already
//! holds a head that went backwards, so a loop pass and a skip each offer a
//! marker once. But *this* layer is where a duplicate would become a duplicate
//! mission event, so it keeps the guarantee itself: every applied marker
//! records its [`MarkerActivation`] — `(session, producer serial, marker key)`
//! — and a second presentation of the same activation is refused
//! ([`MarkerRefusal::RepeatedActivation`]) instead of raising the signal
//! again.
//!
//! The producer serial is what makes that identity right rather than merely
//! conservative:
//!
//! * the serial is allocated per started instance, so **two instances of one
//!   clip are two activations** — two engines starting are two mission events;
//! * a rebind after a teardown gets a **fresh** serial, so a re-opened door is
//!   a new activation and its gameplay effect applies again, which is what
//!   `accept_f20_c_stopping_an_instance_releases_it_and_a_rebind_plays_again`
//!   already asserts on the producer side;
//! * a loop pass, a skip and a re-drained batch keep the same serial, so none of
//!   them can raise the signal twice.
//!
//! # A refused tick hands its batch back
//!
//! [`step_mission_with_markers`] takes the markers first and steps the objective
//! session second, because the log is drained whether or not the step is
//! accepted. [`ObjectiveSession::step`] refuses a non-advancing tick and a
//! refused movement *after* the input is built, so a refusal loses nothing and
//! repeats nothing: [`MissionStepRefusal`] carries the delivery it could not
//! apply, and the caller retries the tick with those signals. Calling
//! `step_mission_with_markers` again instead would find the log empty — the
//! activation was consumed, which is what makes the layer idempotent.
//!
//! # What the host must still get right
//!
//! This layer refuses the cue nobody declared, the reserved source, a stale
//! generation and a repeated activation. Two things stay the host's contract,
//! because this module cannot decide either from what it is given:
//!
//! * **A binding must not name a symbol the program declares.** The F39
//!   runtime treats a host-injected signal as a mission signal and emits
//!   `SignalRaised` under it; a cue bound to an objective, condition, timer or
//!   trigger symbol the program already owns would alias that declaration's own
//!   event, exactly as the reserved source would. [`MissionMarkerBindings`]
//!   refuses [`RESERVED_ACTOR_EVENT_SOURCE`] because that one is a constant it
//!   *can* decide; the rest needs the lowered program, which the mission host
//!   owns beside this table.
//! * **A retry is a generation change.** [`MissionMarkerConsumer::retry`]
//!   refuses the generation it already serves ([`MarkerTeardownError::SameSession`]),
//!   but it cannot tell a caller that meant a fresh generation from one that
//!   meant to clear the ledger of the live mission.
//!
//! # What is **not** claimed
//!
//! * **No original marker semantics.** The original `mis_anim.zbd` /
//!   `cam_anim.zbd` marker encoding is undecoded (F13), `MarkerEffect` is a
//!   **designed** vocabulary and MechWarrior semantics do not transfer (F20
//!   non-negotiable behavior 2). Nothing here says the original raised a
//!   mission signal on `door_opened`, or that an original cue label exists at
//!   all; F20-D keeps the original-family validation gate.
//! * **No pickup or ammo effect.** Behavior 5 names pickups and ammo next to
//!   mission events. What this layer guarantees is that a gameplay marker
//!   reaches the mission layer exactly once per activation; whether a raised
//!   signal is spent on a pickup, an ammo grant or an objective is the
//!   declared program's decision (F27/F36 own those effects), and this module
//!   neither names nor counts them.
//! * **No production mission host.** [`ObjectiveSession`] is host-owned, not a
//!   resource, and no production composition drives it yet (the animation
//!   composition is filed as #509). The entries here are therefore plain
//!   functions the host calls per committed tick, the same shape
//!   [`bind_animated_node`](crate::animation::bind_animated_node) and
//!   [`apply_attachment_transitions`](crate::animation::apply_attachment_transitions)
//!   already use.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use bevy::ecs::world::World;
use cs_script::ir::SymbolId;
use cs_sim::animated_object::{AnimationEvent, AnimationEventId};
use cs_sim::objectives::runtime::{RuntimeError, TickInput};
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

use crate::animation::{AnimationInstance, AnimationLog, AnimationRefusal};
use crate::objectives::{ObjectiveSession, SessionTick};

/// The mission signal the F39 runtime reserves for actor-keyed events.
///
/// A cue bound to it would make the marker's `SignalRaised` event carry the
/// reserved source and alias every counted actor's own event, so
/// [`MissionMarkerBindings`] refuses the binding instead of accepting it.
pub const RESERVED_ACTOR_EVENT_SOURCE: SymbolId = cs_sim::objectives::runtime::ACTOR_EVENT_SOURCE;

/// One declared binding: an authored gameplay cue label and the mission signal
/// it raises.
///
/// This is **input**, the way a spawn group's subject or a mission program is:
/// this module ships no table, because the original marker encoding is
/// undecoded (F13) and a cue → symbol mapping that nobody declared would be a
/// guess about a mission's program. The mission host that authors the program
/// builds the table; see the module doc.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissionMarkerBinding {
    /// The authored cue label a gameplay marker carries.
    pub cue: String,
    /// The mission signal the cue raises.
    pub signal: SymbolId,
}

impl MissionMarkerBinding {
    /// A binding of `cue` to `signal`.
    pub fn new(cue: impl Into<String>, signal: SymbolId) -> Self {
        Self {
            cue: cue.into(),
            signal,
        }
    }
}

/// Why a declared binding table was refused.
///
/// Each variant names what was asked for; nothing is defaulted, because a
/// defaulted cue → symbol mapping would raise a signal nobody declared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerBindingError {
    /// A binding carries an empty cue label. An empty label is not a name a
    /// marker could ever carry, so the row is an authoring defect rather than a
    /// binding.
    EmptyCue,
    /// Two bindings claim the same cue. Which row a marker would resolve to is
    /// then decided by registration order rather than by the host's
    /// declaration, so the table is refused instead — including when both rows
    /// name the same signal, where a repeated row is an authoring defect rather
    /// than a second declaration of anything.
    DuplicateCue {
        /// The cue both rows name.
        cue: String,
        /// The signal the first row bound.
        first: SymbolId,
        /// The signal the second row bound.
        second: SymbolId,
    },
    /// A binding names the reserved actor-event source
    /// ([`RESERVED_ACTOR_EVENT_SOURCE`]).
    ReservedSignal {
        /// The cue whose row names the reserved source.
        cue: String,
        /// The signal the row named.
        signal: SymbolId,
    },
}

impl fmt::Display for MarkerBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCue => write!(f, "a marker binding carries no cue label"),
            Self::DuplicateCue { cue, first, second } => write!(
                f,
                "the cue `{cue}` is bound to both {first:?} and {second:?}, so the signal a \
                 marker would raise is undecided"
            ),
            Self::ReservedSignal { cue, signal } => write!(
                f,
                "the cue `{cue}` is bound to {signal:?}, the source the runtime reserves for \
                 actor-keyed events"
            ),
        }
    }
}

impl std::error::Error for MarkerBindingError {}

/// The mission host's declared cue → mission-signal table.
///
/// A lookup by cue label and nothing else: the table never guesses, never
/// falls back to a default signal and never resolves an ambiguous row (those
/// are refused by [`MissionMarkerBindings::new`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionMarkerBindings {
    signals: BTreeMap<String, SymbolId>,
}

impl MissionMarkerBindings {
    /// Validates and assembles a declared table.
    ///
    /// # Errors
    ///
    /// [`MarkerBindingError::EmptyCue`],
    /// [`MarkerBindingError::DuplicateCue`] naming the two signals the same
    /// cue claims, and [`MarkerBindingError::ReservedSignal`] for a row naming
    /// the reserved actor-event source.
    pub fn new(
        bindings: impl IntoIterator<Item = MissionMarkerBinding>,
    ) -> Result<Self, MarkerBindingError> {
        let mut signals: BTreeMap<String, SymbolId> = BTreeMap::new();
        for binding in bindings {
            if binding.cue.is_empty() {
                return Err(MarkerBindingError::EmptyCue);
            }
            if binding.signal == RESERVED_ACTOR_EVENT_SOURCE {
                return Err(MarkerBindingError::ReservedSignal {
                    cue: binding.cue,
                    signal: binding.signal,
                });
            }
            if let Some(first) = signals.get(binding.cue.as_str()).copied() {
                return Err(MarkerBindingError::DuplicateCue {
                    cue: binding.cue,
                    first,
                    second: binding.signal,
                });
            }
            signals.insert(binding.cue, binding.signal);
        }
        Ok(Self { signals })
    }

    /// The signal `cue` is bound to, when the host bound it.
    #[must_use]
    pub fn signal_for(&self, cue: &str) -> Option<SymbolId> {
        self.signals.get(cue).copied()
    }

    /// How many cues are bound.
    #[must_use]
    pub fn len(&self) -> usize {
        self.signals.len()
    }

    /// Whether no cue is bound.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.signals.is_empty()
    }

    /// The bound cues, in stable order.
    pub fn cues(&self) -> impl Iterator<Item = &str> {
        self.signals.keys().map(String::as_str)
    }
}

/// The identity of **one activation of one marker on one live instance**.
///
/// `(session, producer serial, marker key)` is the whole key: the playback
/// allocates a producer serial per started instance and never recycles it, so
/// two instances of one clip, one instance across many loop passes, a skip and
/// a rebind after teardown all resolve to the activation they really are. See
/// the module doc for why the tick and the event sequence are deliberately
/// **not** part of it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarkerActivation {
    /// The session generation the firing was stamped with.
    session: SessionId,
    /// The playback instance's producer serial.
    producer: u32,
    /// The stable marker key inside its clip.
    marker: String,
}

impl MarkerActivation {
    /// The activation of `marker` on the instance stamping `producer` in
    /// `session`.
    pub fn new(session: SessionId, producer: u32, marker: impl Into<String>) -> Self {
        Self {
            session,
            producer,
            marker: marker.into(),
        }
    }

    /// The session generation this activation belongs to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The producer serial of the live instance that fired the marker.
    #[must_use]
    pub const fn producer(&self) -> u32 {
        self.producer
    }

    /// The stable marker key.
    #[must_use]
    pub fn marker(&self) -> &str {
        &self.marker
    }
}

impl fmt::Display for MarkerActivation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "session {} producer {} marker `{}`",
            self.session.get(),
            self.producer,
            self.marker
        )
    }
}

/// One gameplay marker the consumer turned into a declared mission signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaisedMarker {
    /// The fired event's identity, so a caller can trace the exact firing.
    pub event: AnimationEventId,
    /// The activation this raise belongs to.
    pub activation: MarkerActivation,
    /// The clip the marker came from.
    pub clip: ContentId,
    /// The stable marker key inside the clip.
    pub marker: String,
    /// The authored cue label the declared table resolved.
    pub cue: String,
    /// The mission signal the cue was bound to.
    pub signal: SymbolId,
    /// The loop pass the firing belonged to (`0` for a one-shot gameplay
    /// marker, which never fires on a later pass).
    pub pass: u64,
    /// The simulation tick the firing was stamped with.
    ///
    /// This is the tick the publishing advance ran at, so a **skip** stamps the
    /// tick it jumped to rather than the marker's clip tick. The crossing's
    /// position inside the clip is not on the event at all
    /// ([`AnimationEvent`](cs_sim::animated_object::AnimationEvent) carries no
    /// clip time), so this layer cannot report it; recovering it would be a
    /// producer-side addition.
    pub at: Tick,
}

/// Why a gameplay marker did not become a mission signal.
///
/// Every variant names the marker and the reason, and none of them records an
/// activation: a refused marker applies nothing and may be offered again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerRefusal {
    /// The marker's cue is bound to no mission signal, so the gameplay
    /// transition it asks for is not declared and does not happen.
    UnboundCue {
        /// The clip the marker came from.
        clip: ContentId,
        /// The stable marker key.
        marker: String,
        /// The authored cue label nothing bound.
        cue: String,
        /// The loop pass the firing belonged to.
        pass: u64,
    },
    /// The activation already raised its signal once, so this presentation of it
    /// is refused rather than applied again (F20 non-negotiable behavior 5).
    RepeatedActivation {
        /// The clip the marker came from.
        clip: ContentId,
        /// The stable marker key.
        marker: String,
        /// The activation that already applied.
        activation: MarkerActivation,
    },
    /// The firing was published by another session generation, so it belongs to
    /// a mission that is no longer live and must not reach this one.
    StaleSession {
        /// The clip the marker came from.
        clip: ContentId,
        /// The stable marker key.
        marker: String,
        /// The session generation the event carried.
        session: SessionId,
        /// The session generation this consumer serves.
        served: SessionId,
    },
    /// The marker's effect is [`Resolved::Unknown`](cs_types::content::Resolved),
    /// so the gameplay transition it gates is blocked and reported with the
    /// unknown's own claim (F20 non-negotiable behavior 2).
    BlockedEffect {
        /// The clip the marker belongs to.
        clip: ContentId,
        /// The stable marker key.
        marker: String,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the effect is unknown.
        reason: String,
        /// The loop pass the crossing belonged to.
        pass: u64,
    },
    /// The playback held an advance that went backwards, so the instance
    /// published no marker at all.
    ///
    /// It is named here because the consumer drained the record that says so:
    /// a reversal that silently vanished would leave a mission unable to tell
    /// "the cinematic was rewound" from "the marker never fired" (F20
    /// non-negotiable behavior 5). `from` is the clip time the instance sits
    /// at, `to` the one the session tick asked for.
    HeldAdvance {
        /// The clip whose head was held.
        clip: ContentId,
        /// The live instance of that clip whose head was held.
        instance: AnimationInstance,
        /// The clip time the instance sits at.
        from: u64,
        /// The clip time the session tick asked for.
        to: u64,
    },
}

impl fmt::Display for MarkerRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnboundCue {
                clip,
                marker,
                cue,
                pass,
            } => write!(
                f,
                "the gameplay marker `{marker}` of {clip} (pass {pass}) asks for the cue \
                 `{cue}`, which no declared mission signal is bound to"
            ),
            Self::RepeatedActivation {
                clip,
                marker,
                activation,
            } => write!(
                f,
                "the gameplay marker `{marker}` of {clip} already applied for {activation}"
            ),
            Self::StaleSession {
                clip,
                marker,
                session,
                served,
            } => write!(
                f,
                "the gameplay marker `{marker}` of {clip} was published in {session}, not in \
                 the {served} this consumer serves"
            ),
            Self::BlockedEffect {
                clip,
                marker,
                claim_id,
                reason,
                pass,
            } => write!(
                f,
                "the gameplay marker `{marker}` of {clip} (pass {pass}) is blocked by {claim_id}: \
                 {reason}"
            ),
            Self::HeldAdvance {
                clip,
                instance,
                from,
                to,
            } => write!(
                f,
                "{clip} as {instance} was held at clip time {from} instead of {to}: the advance \
                 went backwards, so no marker was offered"
            ),
        }
    }
}

/// What one fired event did to this layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerAdmission {
    /// A gameplay marker raised its declared mission signal.
    Raised(RaisedMarker),
    /// A presentation cue: it is not gameplay, so this layer neither applied it
    /// nor recorded it as a refusal.
    Presentation,
    /// A gameplay marker was refused, and why.
    Refused(MarkerRefusal),
}

/// What one drain of the animation log handed to the mission layer.
///
/// This is the trace of a pass, not the layer's state: the authoritative record
/// of what has been applied is
/// [`MissionMarkerConsumer::applied`].
///
/// The playback's log holds more than markers — blocked *track* references and
/// attachment transitions are in it too — but they belong to the render and
/// collision consumers, so this layer takes only the marker records
/// ([`AnimationLog::take_markers`](crate::animation::AnimationLog::take_markers))
/// and leaves those in the log. The decision is recorded in
/// `docs/findings/2026-10-06-f20-c-animation-log-multi-consumer-seam.md`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkerDelivery {
    raised: Vec<RaisedMarker>,
    refusals: Vec<MarkerRefusal>,
    presentation: usize,
    drained: usize,
}

impl MarkerDelivery {
    /// The gameplay markers applied, in publication order.
    #[must_use]
    pub fn raised(&self) -> &[RaisedMarker] {
        &self.raised
    }

    /// The refusals: the blocked effects first, then the held advances, then the
    /// per-event refusals in publication order.
    #[must_use]
    pub fn refusals(&self) -> &[MarkerRefusal] {
        &self.refusals
    }

    /// The mission signals this delivery raises, in the order their markers
    /// fired — the values the caller puts into [`TickInput::signals`].
    #[must_use]
    pub fn signals(&self) -> Vec<SymbolId> {
        self.raised.iter().map(|raised| raised.signal).collect()
    }

    /// How many presentation cues the batch carried and this layer did not
    /// apply.
    #[must_use]
    pub const fn presentation_ignored(&self) -> usize {
        self.presentation
    }

    /// How many marker records the taken batch held in total: the raised and
    /// refused markers, the ignored presentation cues, the blocked effects and
    /// the held advances.
    #[must_use]
    pub const fn drained(&self) -> usize {
        self.drained
    }
}

/// What a generation change released from this layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkerTeardown {
    /// The session generation that is now served.
    pub served: SessionId,
    /// How many activations the previous generation had consumed.
    pub released: usize,
}

/// Why a generation change was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerTeardownError {
    /// `served` is the generation this consumer already serves.
    ///
    /// Clearing the ledger for the generation that is **live** would re-arm
    /// every activation of a running mission, which is the duplicate F20
    /// non-negotiable behavior 5 forbids. A generation change is the only thing
    /// that may release an activation, so this is refused here rather than left
    /// to the caller — the same rule
    /// [`ObjectiveSession::retry`](crate::objectives::ObjectiveSession::retry)
    /// applies to the runtime it feeds.
    SameSession {
        /// The generation that is already served.
        served: SessionId,
    },
}

impl fmt::Display for MarkerTeardownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameSession { served } => write!(
                f,
                "{served} is the generation already served, so a retry would re-arm the \
                 activations of a live mission"
            ),
        }
    }
}

impl std::error::Error for MarkerTeardownError {}

/// The mission layer's consumer of fired animation gameplay markers.
///
/// One consumer serves one session generation and owns the two pieces of state
/// that make the layer correct: the declared cue table, and the set of
/// [`MarkerActivation`]s it has already applied. It is a plain struct owned by
/// the mission host beside the [`ObjectiveSession`] it feeds, not a Bevy
/// resource, because the session it drives is host-owned too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionMarkerConsumer {
    served: SessionId,
    bindings: MissionMarkerBindings,
    applied: BTreeSet<MarkerActivation>,
}

impl MissionMarkerConsumer {
    /// A consumer for `served` with the host's declared cue table.
    pub fn new(served: SessionId, bindings: MissionMarkerBindings) -> Self {
        Self {
            served,
            bindings,
            applied: BTreeSet::new(),
        }
    }

    /// The session generation this consumer serves; a firing published by any
    /// other one is refused.
    #[must_use]
    pub const fn served(&self) -> SessionId {
        self.served
    }

    /// The declared cue table this consumer resolves markers through.
    #[must_use]
    pub const fn bindings(&self) -> &MissionMarkerBindings {
        &self.bindings
    }

    /// Every activation this generation has applied, in stable order.
    #[must_use]
    pub const fn applied(&self) -> &BTreeSet<MarkerActivation> {
        &self.applied
    }

    /// Whether `activation` has already been applied in this generation.
    #[must_use]
    pub fn has_applied(&self, activation: &MarkerActivation) -> bool {
        self.applied.contains(activation)
    }

    /// Starts a new session generation with an empty ledger.
    ///
    /// The F39-C retry, for this layer: the activations the failed generation
    /// consumed are reported and dropped, so a marker of the next generation
    /// cannot be refused as a repeat of the last one's.
    ///
    /// Note what a generation change does **not** restore: the evaluator fires
    /// a one-shot gameplay marker once per activation, so a mission that needs
    /// the same marker's effect again must restart the animation (which yields a
    /// fresh producer serial, and therefore a new activation) rather than expect
    /// this layer to re-raise a signal.
    ///
    /// # Errors
    ///
    /// [`MarkerTeardownError::SameSession`] for the generation already served.
    /// The ledger is left untouched, so a refused retry cannot re-arm a live
    /// mission.
    pub fn retry(&mut self, served: SessionId) -> Result<MarkerTeardown, MarkerTeardownError> {
        if served == self.served {
            return Err(MarkerTeardownError::SameSession { served });
        }
        let released = self.applied.len();
        self.applied.clear();
        self.served = served;
        Ok(MarkerTeardown { served, released })
    }

    /// Decides what one fired event does, recording the activation when it
    /// applies.
    ///
    /// The checks run in the order a caller can act on them: the event's own
    /// session first (a stale firing belongs to another mission), then whether
    /// the effect is gameplay at all, then the per-activation ledger, and only
    /// then the declared binding — so an unbound cue is a refusal about the
    /// mission's program and never consumes an activation.
    pub fn admit(&mut self, event: &AnimationEvent) -> MarkerAdmission {
        let activation = MarkerActivation::new(event.id.session, event.id.producer, &event.marker);
        if event.id.session != self.served {
            return MarkerAdmission::Refused(MarkerRefusal::StaleSession {
                clip: event.clip.clone(),
                marker: event.marker.clone(),
                session: event.id.session,
                served: self.served,
            });
        }
        if !event.effect.is_gameplay() {
            return MarkerAdmission::Presentation;
        }
        if self.applied.contains(&activation) {
            return MarkerAdmission::Refused(MarkerRefusal::RepeatedActivation {
                clip: event.clip.clone(),
                marker: event.marker.clone(),
                activation,
            });
        }
        let cue = event.effect.cue();
        let Some(signal) = self.bindings.signal_for(cue) else {
            return MarkerAdmission::Refused(MarkerRefusal::UnboundCue {
                clip: event.clip.clone(),
                marker: event.marker.clone(),
                cue: cue.to_owned(),
                pass: event.pass,
            });
        };
        self.applied.insert(activation.clone());
        MarkerAdmission::Raised(RaisedMarker {
            event: event.id,
            activation,
            clip: event.clip.clone(),
            marker: event.marker.clone(),
            cue: cue.to_owned(),
            signal,
            pass: event.pass,
            at: event.id.tick,
        })
    }

    /// Takes everything the playback published and applies it to this layer.
    ///
    /// The marker records are handed over through
    /// [`AnimationLog::take_markers`](crate::animation::AnimationLog::take_markers),
    /// so a long session accumulates no markers: a world with no
    /// [`AnimationLog`](crate::animation::AnimationLog) resource drains
    /// nothing and reports nothing, which is the same "no producer, no
    /// consumer" rule the rest of the playback follows.
    ///
    /// Blocked effects become refusals before the events are dispatched, and a
    /// held advance is named after them, so a mission can see that a transition
    /// it expected was gated by an unknown — or that a rewind published nothing
    /// at all — rather than finding a silence where the marker should have been.
    ///
    /// The blocked track references and attachment transitions stay in the log
    /// for the consumers that own them; this layer neither reads nor counts them.
    pub fn drain(&mut self, world: &mut World) -> MarkerDelivery {
        let batch = match world.get_resource_mut::<AnimationLog>() {
            Some(mut log) => log.take_markers(),
            None => AnimationLog::new(),
        };
        let mut delivery = MarkerDelivery {
            drained: batch.len(),
            ..MarkerDelivery::default()
        };
        for blocked in batch.blocked_markers() {
            delivery.refusals.push(MarkerRefusal::BlockedEffect {
                clip: blocked.clip.clone(),
                marker: blocked.marker.clone(),
                claim_id: blocked.claim_id.clone(),
                reason: blocked.reason.clone(),
                pass: blocked.pass,
            });
        }
        for refused in batch.refusals() {
            let AnimationRefusal::Held {
                clip,
                instance,
                from,
                to,
            } = refused;
            delivery.refusals.push(MarkerRefusal::HeldAdvance {
                clip: clip.clone(),
                instance: *instance,
                from: *from,
                to: *to,
            });
        }
        for event in batch.events() {
            match self.admit(event) {
                MarkerAdmission::Raised(raised) => delivery.raised.push(raised),
                MarkerAdmission::Presentation => delivery.presentation += 1,
                MarkerAdmission::Refused(refusal) => delivery.refusals.push(refusal),
            }
        }
        delivery
    }
}

/// What one composed pass did: the markers it drained and the objective tick it
/// applied them to.
#[derive(Debug)]
pub struct MissionStep {
    /// The drain this pass applied.
    pub markers: MarkerDelivery,
    /// The objective session's answer.
    pub tick: SessionTick,
}

/// An objective session that refused the tick, with the markers it refused to
/// apply.
///
/// The markers are named here rather than dropped because the log has already
/// been drained: the caller retries the tick with
/// [`MarkerDelivery::signals`], and nothing is lost and nothing is applied
/// twice.
#[derive(Debug)]
pub struct MissionStepRefusal {
    /// What the runtime refused.
    pub error: RuntimeError,
    /// The drain that could not be applied.
    pub markers: MarkerDelivery,
}

impl fmt::Display for MissionStepRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the objective session refused the tick, so {} gameplay marker(s) were not applied: \
             {}",
            self.markers.raised().len(),
            self.error
        )
    }
}

impl std::error::Error for MissionStepRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// The composed mission-marker step: drain the animation log, raise the
/// delivered signals, and step the objective session with them.
///
/// This is the producer → runtime → consumer path in one call: the host's own
/// `facts` (the tick, the committed ticks, its lifecycle transitions,
/// movement segments and declared requests) are the mission host's, and the
/// signals the markers declared are **added** to them, never substituted for
/// them. The objective session then applies both through the same
/// [`ObjectiveSession::step`], so a marker's signal and the host's own facts
/// are one ordered tick and one event stream.
///
/// # Errors
///
/// [`MissionStepRefusal`] when the objective session refuses the tick, carrying
/// the delivery so the caller can retry with it.
pub fn step_mission_with_markers(
    world: &mut World,
    consumer: &mut MissionMarkerConsumer,
    objectives: &mut ObjectiveSession,
    facts: &TickInput<'_>,
) -> Result<MissionStep, MissionStepRefusal> {
    let markers = consumer.drain(world);
    let mut signals = facts.signals.to_vec();
    signals.extend(markers.signals());
    let mut input = facts.clone();
    input.signals = &signals;
    match objectives.step(&input) {
        Ok(tick) => Ok(MissionStep { tick, markers }),
        Err(error) => Err(MissionStepRefusal { error, markers }),
    }
}
