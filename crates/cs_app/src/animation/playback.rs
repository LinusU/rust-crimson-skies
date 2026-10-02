//! The fixed-tick animation playback: verified transform, material and
//! attachment tracks (F20-B), with per-instance identity and teardown
//! (F20-C.02).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stages `### F20-B` and `### F20-C`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F20-A defined the declared IR
//! ([`cs_content::animation`](cs_content::animation)) and the fixed-tick
//! evaluator ([`cs_sim::animated_object`](cs_sim::animated_object)) but left
//! them unplugged: nothing played a clip inside a session and no consumer saw
//! a track's value. This module is that production path:
//!
//! * [`play_animation`] takes a validated declared
//!   [`AnimationClip`](cs_content::animation::AnimationClip), lowers it
//!   through [`lower::lower_clip`] and starts **one live instance** of it in
//!   the [`AnimationPlayback`] resource — refused when the world has no
//!   session, when that `(track, instance)` identity already plays, or when
//!   the clip does not survive the boundary;
//! * [`advance_animation`] is the fixed-tick entry: once per committed
//!   session tick it advances every playing instance, publishes the markers
//!   it crossed into the [`AnimationLog`], and applies the four track kinds
//!   this stage owns to the entities bound to them. F20-C.02 places it on the
//!   schedule: see [`super::schedule`]; the visibility channel's LOD/damage
//!   ownership is [`super::visibility`];
//! * [`stop_animation`] ends one instance **and releases what that instance
//!   applied** — its animation-managed hierarchy links first, then its applied
//!   components and its bindings, for that instance's entities only.
//!
//! # Instance identity
//!
//! One `animation_track` can have several live instances: two aircraft spin
//! their propellers with the same authored clip, so the live map is keyed by
//! [`InstanceKey`] — the `(track, [`AnimationInstance`])` pair every
//! [`AnimatedNodeBinding`] names. Each instance owns its own evaluator (so
//! each fires its one-shot gameplay marker once, independently), its own
//! producer serial (so two instances' events of one tick never share an
//! [`AnimationEventId`](cs_sim::animated_object::AnimationEventId)) and its
//! own applied state. `play_animation` refuses a second instance of the
//! *same identity* with [`AnimationPlayError::AlreadyPlaying`] and never
//! silently replaces a live one.
//!
//! # Verified application
//!
//! A track value reaches an entity only when the binding verifies:
//!
//! * the entity's [`AnimatedNodeBinding`] names a **playing** instance,
//! * and the **scene generation** that instance serves — a binding stamped by
//!   a superseded scene load is never driven (F11/F20 session generation
//!   ownership), and
//! * the binding's node is one the clip actually drives, with an aspect that
//!   has a reached key.
//!
//! Every other entity is left exactly as it is. Nothing is guessed for a
//! missing entity: the state is derived from the clip position every tick, so
//! application is idempotent — running two advances over the same span writes
//! the same components, and a loop pass that repeats a pose writes it again
//! without re-firing anything (F20 non-negotiable behavior 3).
//!
//! # Unknowns block one track, not the node
//!
//! A material or attachment reference that is [`Resolved::Unknown`] never
//! becomes a component: the previously applied value stays and the refusal is
//! published once as a [`BlockedTrack`] carrying the unknown's claim id and
//! reason (F20 non-negotiable behavior 2 — an unknown retains its locator and
//! blocks the transition it gates). The report is a property of the playing
//! instance, so it is published whether or not an entity happens to be bound
//! to that node — the same way a blocked marker is — while the binding decides
//! only whether a *value* is applied. The *other* tracks of the same node keep
//! applying, and marker effects keep firing, so one undecoded reference
//! cannot silently stop a whole clip — it is visible in the log instead.
//!
//! # What this stage does not do
//!
//! The transform track is published as [`NodeAnimatedPose`], one component
//! per node: the pose a render and a collision consumer both read, so neither
//! can diverge (the F20-A `mesh_pose`/`collider_pose` rule carried into the
//! ECS). Recomposing descendants' world poses from it, reparenting entities
//! and inherited detach velocity are the consumers' work; the hierarchy half is
//! [`super::attachment`], the schedule placement of the advance is
//! [`super::schedule`], and the visibility channel's ownership against LOD and
//! damage is [`super::visibility`]. See
//! `docs/findings/2026-09-30-f20-b-transform-material-attachment-tracks.md`,
//! `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md`
//! and
//! `docs/findings/2026-10-02-f20-c-03-visibility-lod-damage-ownership.md`.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use bevy::ecs::component::Component;
use bevy::ecs::world::World;
use bevy::prelude::{Entity, Resource};
use cs_content::animation::AnimationClip;
use cs_sim::animated_object::{
    AnimatedNodeState, AnimatedObject, AnimationEvent, AttachmentState, BlockedMarker, PoseSample,
    Visibility,
};
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

use crate::scene::SceneGeneration;

use super::AnimatedNodeBinding;
use super::AnimationInstance;
use super::attachment::AttachmentRecord;
use super::lower::{LowerError, lower_clip};
use super::visibility::NodeAnimatedVisibility;

// --------------------------------------------------------------- tracks ---

/// Component: the pose the playing clip's transform track applied to this
/// node.
///
/// This is the node's one animated pose — the F20-A rule carried into the
/// ECS: [`Self::mesh`] and [`Self::collider`] are two names for the same
/// stored value, so a transform track cannot reach a render consumer without
/// reaching the collision consumer in the same tick (AC01's coherence, stated
/// structurally). An entity without the component has not been driven by a
/// transform track; the component is written only for a verified binding.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct NodeAnimatedPose(pub PoseSample);

impl NodeAnimatedPose {
    /// The pose the mesh presents.
    #[must_use]
    pub const fn mesh(&self) -> PoseSample {
        self.0
    }

    /// The pose the collider evaluates — the same value as [`Self::mesh`].
    #[must_use]
    pub const fn collider(&self) -> PoseSample {
        self.0
    }

    /// The stored pose.
    #[must_use]
    pub const fn pose(&self) -> PoseSample {
        self.0
    }
}

/// Component: the material the playing clip's material track applied to this
/// node.
///
/// Only a **known**, kind-validated `material` id ever reaches this
/// component: a [`Resolved::Unknown`] material is blocked and reported in the
/// [`AnimationLog`] instead, so a consumer never sees a material nobody
/// resolved and never guesses one.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct NodeAnimatedMaterial(pub ContentId);

impl NodeAnimatedMaterial {
    /// The applied material (`material` kind, validated when the clip was
    /// assembled).
    #[must_use]
    pub fn material(&self) -> &ContentId {
        &self.0
    }
}

/// Component: the attachment the playing clip's attachment track applied to
/// this node.
///
/// The record is the explicit parent change with its authored
/// [`PosePolicy`](cs_sim::animated_object::PosePolicy) — `parent: None` is a
/// detach, `Some(known)` an attachment under that node. Reparenting the ECS
/// hierarchy and recomputing the world/local pose belongs to F20-C; an
/// unknown parent never reaches this component (it is blocked and reported).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct NodeAnimatedAttachment(pub AttachmentState);

impl NodeAnimatedAttachment {
    /// The applied attachment state.
    #[must_use]
    pub fn attachment(&self) -> &AttachmentState {
        &self.0
    }
}

/// Which `Resolved` track kind an application refused.
///
/// The visibility channel is deliberately absent: a visibility key carries a
/// `NodeVisibility`, not a `Resolved<_>`, so it has no unknown to block. Its
/// blocking rules are the ones every aspect shares — an unreached key writes
/// nothing, and an unverified binding is never written (see
/// [`super::visibility`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrackKind {
    /// A material-swap key that resolved to nothing.
    Material,
    /// An attachment key whose parent resolved to nothing.
    Attachment,
}

impl TrackKind {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Material => "material",
            Self::Attachment => "attachment",
        }
    }
}

/// A track application that was blocked by an unknown reference.
///
/// Published once per (clip, node, track) per playing instance, from the
/// clip's evaluated state alone — an entity does not have to be bound for a
/// gap in the clip to be visible. An unknown that stays unresolved does not
/// append one entry per tick, and the same clip played again reports its own
/// gap again (the `AirframeSceneLog` "report the gap, not the frame" rule).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockedTrack {
    /// The playing clip the track belongs to.
    pub clip: ContentId,
    /// The node whose track was refused.
    pub node: ContentId,
    /// Which track kind was refused.
    pub track: TrackKind,
    /// The claim the unknown is recorded under.
    pub claim_id: ClaimId,
    /// Why the reference is unknown.
    pub reason: String,
}

/// Something the advance refused instead of performing.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationRefusal {
    /// The clip head was held: the session tick asked for a position behind
    /// the one the instance already reached, so it did not move — no marker
    /// re-fired and no track changed (F20 non-negotiable behavior 5: a
    /// reversed or restarted session never re-offers a one-shot marker).
    ///
    /// `from` is the clip time the instance sits at and `to` the one the
    /// session tick asked for. Reported once per occurrence, not once per
    /// tick while the session stays behind. The `instance` names *which* live
    /// instance was held: one `animation_track` played by two instances
    /// reports two holds, and a refusal that named only the track could not
    /// tell them apart.
    Held {
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

// ------------------------------------------------------------------ log ---

/// Resource: the append-only record of what the playback published.
///
/// This is the consumer seam of this stage: marker events (gameplay and
/// presentation), markers blocked by an unknown effect, tracks blocked by an
/// unknown reference and the advances that were refused — plus, since F20-C,
/// the attachment transitions that were refused or could not inherit a
/// velocity ([`AttachmentRecord`]), including the ones an instance teardown
/// and a release before a despawn caused. It grows with the number of
/// *published entries* — never one per tick, and never one per teardown pass
/// that changed nothing — and is meant to be drained by the layer that
/// consumes them (the mission marker consumer is F20-C's wiring), so
/// [`Self::drain`] is how that consumer takes its batch.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct AnimationLog {
    events: Vec<AnimationEvent>,
    blocked_markers: Vec<BlockedMarker>,
    blocked_tracks: Vec<BlockedTrack>,
    refused: Vec<AnimationRefusal>,
    attachments: Vec<AttachmentRecord>,
}

impl AnimationLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The fired marker events, oldest first.
    #[must_use]
    pub fn events(&self) -> &[AnimationEvent] {
        &self.events
    }

    /// The marker crossings blocked by an unknown effect.
    #[must_use]
    pub fn blocked_markers(&self) -> &[BlockedMarker] {
        &self.blocked_markers
    }

    /// The track applications blocked by an unknown reference.
    #[must_use]
    pub fn blocked_tracks(&self) -> &[BlockedTrack] {
        &self.blocked_tracks
    }

    /// The advances that were refused instead of performed.
    #[must_use]
    pub fn refusals(&self) -> &[AnimationRefusal] {
        &self.refused
    }

    /// The attachment transitions that were refused, and the detaches that
    /// inherited no velocity (F20-C's consumer).
    #[must_use]
    pub fn attachments(&self) -> &[AttachmentRecord] {
        &self.attachments
    }

    /// Appends what the attachment consumer published this tick.
    ///
    /// `pub(crate)`: the consumer is a sibling module of this record, and
    /// the publications follow the same rule every other collection here
    /// follows — one entry per transition, never one per frame.
    pub(crate) fn push_attachments(&mut self, records: Vec<AttachmentRecord>) {
        self.attachments.extend(records);
    }

    /// How many entries the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
            + self.blocked_markers.len()
            + self.blocked_tracks.len()
            + self.refused.len()
            + self.attachments.len()
    }

    /// Whether nothing has been published since the last drain.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes everything published so far, leaving the log empty.
    ///
    /// The returned log is the consumer's batch; draining is what keeps a
    /// long session from accumulating presentation cues that were already
    /// handed over.
    #[must_use]
    pub fn drain(&mut self) -> Self {
        std::mem::take(self)
    }
}

// ------------------------------------------------------------ playback ----

/// The identity of one live instance: the `animation_track` it plays and the
/// [`AnimationInstance`] within it.
///
/// This is the key of [`AnimationPlayback`]'s live map, and it is what an
/// [`AnimatedNodeBinding`] names when it asks to be driven. Two keys are
/// different instances even when they share a track, which is what keeps two
/// aircraft from sharing one propeller track.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceKey {
    /// The `animation_track` the instance plays.
    pub clip: ContentId,
    /// Which instance of that track.
    pub instance: AnimationInstance,
}

impl InstanceKey {
    /// The key of one instance of `clip`.
    #[must_use]
    pub fn new(clip: &ContentId, instance: AnimationInstance) -> Self {
        Self {
            clip: clip.clone(),
            instance,
        }
    }
}

impl fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} as {}", self.clip, self.instance)
    }
}

/// One live instance of a playing clip: the evaluator plus the identity its
/// events are stamped with.
#[derive(Debug)]
struct PlayingClip {
    /// The scene generation the instance serves; only bindings stamped with
    /// it are driven.
    generation: SceneGeneration,
    /// The session tick the clip started at; clip time is the ticks since.
    started_at: Tick,
    /// The producer serial this instance's event ids carry.
    producer: u32,
    /// The fixed-tick evaluator, held across advances so its per-activation
    /// dedup state survives every loop pass.
    object: AnimatedObject,
    /// The (node, track) applications already reported as blocked.
    blocked_tracks: HashSet<(ContentId, TrackKind)>,
    /// Whether a held (refused) advance is already on record.
    held_reported: bool,
}

/// Resource: the session's animation playback — its session id, the next
/// producer serial, and the live instances of every playing
/// `animation_track`.
///
/// The live map is keyed by [`InstanceKey`], so several entities may play one
/// track as separate instances (one propeller per aircraft); each entry owns
/// its own evaluator, producer serial and applied state, and
/// [`play_animation`] refuses a second instance of the *same* identity instead
/// of silently replacing the first. The producer inserts this resource once
/// per session (`AnimationPlayback::new(session)`); [`play_animation`] refuses
/// to start anything without it, so an animation can never play in no session
/// at all.
///
/// It also holds the two schedule counters the fixed-tick wiring needs:
/// [`Self::advanced_through`] (the session tick the playback was last advanced
/// to — the repeat rule of
/// [`super::schedule::advance_animation_on_session_tick`] reads it) and
/// [`Self::advances`] (how many advance passes ran, so a schedule placement can
/// be observed rather than inferred from a side effect).
#[derive(Resource, Debug)]
pub struct AnimationPlayback {
    session: SessionId,
    next_producer: u32,
    playing: BTreeMap<InstanceKey, PlayingClip>,
    advanced_through: Option<Tick>,
    advances: u64,
}

impl AnimationPlayback {
    /// A playback for `session`: its events are stamped with that shared
    /// nonzero [`SessionId`] generation, so a restarted session can never
    /// collide with the previous one's ids (`IDENTITY-CONTENT` session
    /// generations; F20-A follow-up 1, resolved by task #397).
    ///
    /// The playback starts with no committed tick behind it, so the first tick
    /// a driver commits is always a change and is therefore advanced.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            session,
            next_producer: 0,
            playing: BTreeMap::new(),
            advanced_through: None,
            advances: 0,
        }
    }

    /// The session generation every event id carries.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// Whether this exact instance of this `animation_track` currently plays.
    ///
    /// The identity is the whole question: a *different* instance of the same
    /// track is a different live entry and is never reported here.
    #[must_use]
    pub fn is_playing(&self, clip: &ContentId, instance: AnimationInstance) -> bool {
        self.playing.contains_key(&InstanceKey::new(clip, instance))
    }

    /// The live instances, in stable `(track, instance)` order.
    pub fn playing(&self) -> impl Iterator<Item = &InstanceKey> + '_ {
        self.playing.keys()
    }

    /// The clip time one playing instance has reached; `None` when that
    /// instance is not playing.
    #[must_use]
    pub fn time(&self, clip: &ContentId, instance: AnimationInstance) -> Option<u64> {
        self.get(clip, instance)
            .map(|playing| playing.object.time())
    }

    /// The scene generation one playing instance serves.
    #[must_use]
    pub fn generation(
        &self,
        clip: &ContentId,
        instance: AnimationInstance,
    ) -> Option<SceneGeneration> {
        self.get(clip, instance).map(|playing| playing.generation)
    }

    /// Whether that live instance has a channel on `node`.
    ///
    /// This is the "driven node" half of the verified binding, read the same
    /// way the track application reads it (a node the clip drives is one it
    /// has a channel on), so the F20-C attachment consumer can verify a
    /// binding without reaching into the evaluator. An instance that is not
    /// playing drives nothing and returns `false`.
    #[must_use]
    pub fn drives(&self, clip: &ContentId, instance: AnimationInstance, node: &ContentId) -> bool {
        self.get(clip, instance).is_some_and(|playing| {
            playing
                .object
                .clip()
                .channels()
                .iter()
                .any(|channel| channel.target() == node)
        })
    }

    /// The producer serial one playing instance stamps its event ids with.
    ///
    /// The serial is what keeps two instances of one track from ever sharing
    /// an event id on the same session tick.
    #[must_use]
    pub fn producer(&self, clip: &ContentId, instance: AnimationInstance) -> Option<u32> {
        self.get(clip, instance).map(|playing| playing.producer)
    }

    /// Whether a [`LoopMode::Once`](cs_sim::animated_object::LoopMode)
    /// instance has reached its end; `None` when that instance is not
    /// playing.
    #[must_use]
    pub fn is_finished(&self, clip: &ContentId, instance: AnimationInstance) -> Option<bool> {
        self.get(clip, instance)
            .map(|playing| playing.object.is_finished())
    }

    /// How many instances are playing, across every track.
    ///
    /// This counts **live instances**, not tracks: one track played by two
    /// aircraft is `2`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.playing.len()
    }

    /// Whether no instance is playing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.playing.is_empty()
    }

    /// The session tick this playback was last advanced to; `None` while it
    /// has never been advanced.
    ///
    /// The schedule's repeat rule is a comparison against this: a committed
    /// tick equal to it is not a new tick, so nothing is advanced and nothing
    /// is published. It is written by [`advance_animation`], so a direct call
    /// and the scheduled system agree on what "already advanced" means.
    #[must_use]
    pub const fn advanced_through(&self) -> Option<Tick> {
        self.advanced_through
    }

    /// How many advance passes have run.
    ///
    /// A pass is counted even when it published nothing, so this is the
    /// observable that distinguishes "the schedule advanced" from "the
    /// evaluator had nothing new to say".
    #[must_use]
    pub const fn advances(&self) -> u64 {
        self.advances
    }

    fn get(&self, clip: &ContentId, instance: AnimationInstance) -> Option<&PlayingClip> {
        self.playing.get(&InstanceKey::new(clip, instance))
    }

    /// Takes one live instance out of the map, reporting whether it played.
    ///
    /// `pub(crate)`: removing an instance is only ever half of a teardown, and
    /// the teardown belongs to [`stop_animation`] and
    /// [`super::schedule::release_superseded_instances`], which release the
    /// applied state with it. Keeping it private here is what makes it
    /// impossible to stop an instance and leave its applied components behind.
    pub(crate) fn remove(&mut self, key: &InstanceKey) -> bool {
        self.playing.remove(key).is_some()
    }

    /// Records that an advance pass committed `at`.
    fn record_advance(&mut self, at: Tick) {
        self.advanced_through = Some(at);
        self.advances += 1;
    }

    /// Starts one instance of an already validated runtime clip.
    fn start(
        &mut self,
        clip: &AnimationClip,
        instance: AnimationInstance,
        generation: SceneGeneration,
        at: Tick,
    ) -> Result<(), AnimationPlayError> {
        let runtime = lower_clip(clip).map_err(AnimationPlayError::Lower)?;
        let key = InstanceKey::new(runtime.id(), instance);
        if self.playing.contains_key(&key) {
            return Err(AnimationPlayError::AlreadyPlaying {
                clip: key.clip,
                instance,
            });
        }
        // The serial space stops one short of `u32::MAX`, so a wrap can
        // never hand out an id an earlier event of this session already
        // fired with.
        if self.next_producer == u32::MAX {
            return Err(AnimationPlayError::ProducerExhausted);
        }
        let producer = self.next_producer;
        self.next_producer += 1;
        let object = AnimatedObject::new(runtime, self.session, producer);
        self.playing.insert(
            key,
            PlayingClip {
                generation,
                started_at: at,
                producer,
                object,
                blocked_tracks: HashSet::new(),
                held_reported: false,
            },
        );
        Ok(())
    }

    /// Every live instance whose scene generation is **not** the one the
    /// scene load path currently serves, in stable key order.
    ///
    /// The scene load path is the only writer of
    /// [`SceneGenerations`](crate::scene::SceneGenerations) and generations
    /// only ever count up, so "older than the latest" and "not the latest" are
    /// the same set; the comparison is written as `!=` so a future generation
    /// is not torn down by a rule about superseded ones.
    pub fn superseded(&self, latest: SceneGeneration) -> Vec<InstanceKey> {
        self.playing
            .iter()
            .filter(|(_, playing)| playing.generation != latest)
            .map(|(key, _)| key.clone())
            .collect()
    }
}

/// Why a declared clip was not started.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationPlayError {
    /// The world carries no [`AnimationPlayback`]: there is no session to
    /// play in, so nothing was started.
    NoSession,
    /// That exact instance of that `animation_track` already has a live
    /// instance in this session.
    ///
    /// A second instance of the *same* [`AnimationInstance`] is refused rather
    /// than silently replacing the first; a *different* instance of the same
    /// track is a different live entry and is not this error.
    AlreadyPlaying {
        /// The track that is already playing.
        clip: ContentId,
        /// The instance identity that is already playing it.
        instance: AnimationInstance,
    },
    /// The declared clip did not survive the lowering boundary. Unreachable
    /// for a clip that passed [`AnimationClip::try_new`] (the runtime
    /// re-checks the same invariants), kept so a future drift between the
    /// two boundaries is refused here instead of reaching the evaluator.
    Lower(LowerError),
    /// The session's producer serials are exhausted; start a new session.
    ProducerExhausted,
}

impl std::fmt::Display for AnimationPlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSession => write!(f, "the world holds no animation playback to play in"),
            Self::AlreadyPlaying { clip, instance } => {
                write!(f, "{clip} is already playing as {instance}")
            }
            Self::Lower(source) => write!(f, "the declared clip did not lower: {source}"),
            Self::ProducerExhausted => {
                write!(f, "no producer serial is left in this session")
            }
        }
    }
}

impl std::error::Error for AnimationPlayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lower(source) => Some(source),
            _ => None,
        }
    }
}

// ------------------------------------------------------- entry points -----

/// Starts one declared clip in the world's [`AnimationPlayback`], as one
/// [`AnimationInstance`] of that `animation_track`.
///
/// The clip is lowered and re-validated at the boundary
/// ([`lower::lower_clip`]), so a playback can only ever drive the track
/// kinds, tick ranges and marker keys that both records accept. The
/// instance's event ids are stamped with the playback's session and a fresh
/// producer serial, and its clip time starts at `at`.
///
/// `instance` names *which* live instance of that track this is — one per
/// animated node the spawn wiring spawns, so two aircraft may spin the same
/// propeller track as two independent instances. Reusing a live identity is
/// refused, never substituted.
///
/// # Errors
///
/// [`AnimationPlayError::NoSession`] when the world holds no
/// [`AnimationPlayback`], [`AnimationPlayError::AlreadyPlaying`] when that
/// exact instance is already live, [`AnimationPlayError::Lower`] when the
/// declared clip does not survive the boundary,
/// [`AnimationPlayError::ProducerExhausted`] when no producer serial is left.
pub fn play_animation(
    world: &mut World,
    clip: &AnimationClip,
    instance: AnimationInstance,
    generation: SceneGeneration,
    at: Tick,
) -> Result<(), AnimationPlayError> {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return Err(AnimationPlayError::NoSession);
    };
    let started = playback.start(clip, instance, generation, at);
    world.insert_resource(playback);
    started
}

/// Stops one instance of an `animation_track` and releases what **that
/// instance** applied; reports whether it was playing.
///
/// The teardown is scoped to the entities whose [`AnimatedNodeBinding`] names
/// exactly this `(clip, instance)` pair:
///
/// 1. the animation-managed hierarchy link is released first, by the same
///    rule an authored detach uses ([`super::attachment::release_animated_attachment`]),
///    so the departing parent cannot take the node with it when it is
///    despawned (non-negotiable behavior 4) and the release inherits the
///    parent's velocity exactly once;
/// 2. then the values that instance applied — [`NodeAnimatedPose`],
///    [`NodeAnimatedMaterial`], [`NodeAnimatedAttachment`] — and the
///    bookkeeping the consumers kept ([`AppliedAttachment`](super::attachment::AppliedAttachment),
///    [`RefusedAttachment`](super::attachment::RefusedAttachment)) go;
/// 3. and the binding itself, which named a live instance that no longer
///    exists.
///
/// Nothing else is touched: another instance of the same track, an entity
/// bound to a different track, and an entity carrying an animated component
/// with no binding at all all keep their state. The instance is removed from
/// the live map *before* the release, so the next advance cannot re-attach
/// what the release unparented.
pub fn stop_animation(world: &mut World, clip: &ContentId, instance: AnimationInstance) -> bool {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return false;
    };
    let stopped = playback.remove(&InstanceKey::new(clip, instance));
    world.insert_resource(playback);
    if stopped {
        release_instance(world, clip, instance);
    }
    stopped
}

/// Releases what one instance applied to its own entities.
///
/// The instance is expected to be out of the live map already: releasing an
/// instance that still plays would let the next advance undo the teardown.
pub(crate) fn release_instance(world: &mut World, clip: &ContentId, instance: AnimationInstance) {
    // 1. The bound entities, collected before anything is removed: the
    //    release publishes records stamped with the binding's identity, so
    //    the binding must still be there when it runs.
    let mut query = world.query::<(Entity, &AnimatedNodeBinding)>();
    let bound: Vec<Entity> = query
        .iter(world)
        .filter(|(_, binding)| binding.clip == *clip && binding.instance == instance)
        .map(|(entity, _)| entity)
        .collect();

    let mut records: Vec<AttachmentRecord> = Vec::new();
    for entity in bound {
        // 2. The hierarchy link first, by the authored detach's rule, so a
        //    parent despawned later in this step cannot take the node with
        //    it and the inherited velocity is written once. Everything the
        //    release could not inherit is published, never dropped in
        //    silence — the same rule F20-C.01's review pinned.
        records.extend(
            super::attachment::release_animated_attachment(world, entity).unwrap_or_default(),
        );

        // 3. Then the applied values, the consumers' bookkeeping and the
        //    binding itself.
        world
            .entity_mut(entity)
            .remove::<NodeAnimatedPose>()
            .remove::<NodeAnimatedMaterial>()
            .remove::<NodeAnimatedAttachment>()
            .remove::<NodeAnimatedVisibility>()
            .remove::<super::attachment::AppliedAttachment>()
            .remove::<super::attachment::RefusedAttachment>()
            .remove::<AnimatedNodeBinding>();
    }

    if !records.is_empty() {
        let mut log = world.remove_resource::<AnimationLog>().unwrap_or_default();
        log.push_attachments(records);
        world.insert_resource(log);
    }
}

/// Advances every playing clip to the session tick `at`, publishes what it
/// crossed and applies its tracks to the bound entities.
///
/// The caller is the fixed-tick driver: it calls this **once per committed
/// session tick**, in the F16-C order, with the tick stamped into every event
/// id. Clip time is the ticks since the instance started
/// (`at - started_at`), so the evaluator's head only ever moves forward: a
/// session tick that goes backwards holds the instance where it is and
/// publishes [`AnimationRefusal::Held`] instead of replaying anything (F20
/// non-negotiable behavior 5). An instance whose own start tick has not
/// arrived yet is passed over entirely — it offers no marker, applies no
/// state and reports no hold, so a clip scheduled to start at tick `N` is
/// silent at `N - 1`.
///
/// With no [`AnimationPlayback`] resource the call does nothing — there is
/// no session to advance.
pub fn advance_animation(world: &mut World, at: Tick) {
    let Some(mut playback) = world.remove_resource::<AnimationPlayback>() else {
        return;
    };
    // The pass is recorded whatever it publishes, so a schedule placement can
    // be observed (`advances`) and a repeated committed tick can be refused
    // (`advanced_through`) without inferring either from a side effect.
    playback.record_advance(at);

    // 1. Advance every instance and collect the markers it crossed. The
    //    evaluator keeps the per-activation dedup, so a looping clip keeps
    //    its one-shot gameplay event across passes.
    let mut events: Vec<AnimationEvent> = Vec::new();
    let mut blocked_markers: Vec<BlockedMarker> = Vec::new();
    let mut refused: Vec<AnimationRefusal> = Vec::new();
    for (key, playing) in playback.playing.iter_mut() {
        if at.0 < playing.started_at.0 {
            // The instance's own start tick has not arrived: it has not
            // played a single tick yet, so no marker is offered, no state is
            // applied and there is no head to hold (a clip scheduled to
            // start at tick N must not emit anything at tick N - 1 —
            // non-negotiable behavior 1, markers fire at their authored
            // tick of the fixed-tick simulation).
            continue;
        }
        let target = at.0 - playing.started_at.0;
        if target < playing.object.time() {
            if !playing.held_reported {
                playing.held_reported = true;
                refused.push(AnimationRefusal::Held {
                    clip: key.clip.clone(),
                    instance: key.instance,
                    from: playing.object.time(),
                    to: target,
                });
            }
            continue;
        }
        playing.held_reported = false;
        let outcome = playing.object.advance_to(target, at).expect(
            "the clip time was checked to move forward, and only a backwards head is refused",
        );
        events.extend(outcome.events);
        blocked_markers.extend(outcome.blocked);
    }

    // 2. Snapshot the evaluated state of every instance that has started —
    //    one coherent record per driven node, owned locally so the world's
    //    borrow ends before anything is written — and publish the unknown
    //    references its tracks carry. A gap in the clip's own content is
    //    reported whether or not an entity happens to be bound to that node
    //    (the same way a blocked marker is, without consulting the world);
    //    the binding decides only whether a *value* is applied, never
    //    whether a gap is visible. The map is keyed by instance, so two
    //    instances of one track never read each other's state.
    let mut states: BTreeMap<
        InstanceKey,
        (SceneGeneration, BTreeMap<ContentId, AnimatedNodeState>),
    > = BTreeMap::new();
    let mut blocked_tracks: Vec<BlockedTrack> = Vec::new();
    for (key, playing) in playback.playing.iter_mut() {
        if at.0 < playing.started_at.0 {
            // Not started yet: the instance contributes no state, so its
            // tracks apply to nothing and its gaps are not offered either.
            continue;
        }
        let node_states = playing.object.states();
        for (node, state) in &node_states {
            match state.material() {
                Some(Resolved::Unknown { claim_id, reason }) => publish_blocked(
                    playing,
                    &mut blocked_tracks,
                    &key.clip,
                    node,
                    TrackKind::Material,
                    claim_id,
                    reason,
                ),
                Some(Resolved::Known(_)) | None => {}
            }
            if let Some(AttachmentState {
                parent: Some(Resolved::Unknown { claim_id, reason }),
                ..
            }) = state.attachment()
            {
                publish_blocked(
                    playing,
                    &mut blocked_tracks,
                    &key.clip,
                    node,
                    TrackKind::Attachment,
                    claim_id,
                    reason,
                );
            }
        }
        states.insert(key.clone(), (playing.generation, node_states));
    }

    // 3. The candidate bindings, collected from the world before any write.
    let mut query = world.query::<(Entity, &AnimatedNodeBinding)>();
    let bindings: Vec<(Entity, AnimatedNodeBinding)> = query
        .iter(world)
        .map(|(entity, binding)| (entity, binding.clone()))
        .collect();

    // 4. Decide the writes: a track value reaches an entity only through a
    //    verified binding, and an unknown reference is never written.
    let mut writes: Vec<(Entity, NodeWrite)> = Vec::new();
    for (entity, binding) in bindings {
        let key = InstanceKey::new(&binding.clip, binding.instance);
        let Some((generation, node_states)) = states.get(&key) else {
            // That instance is not playing: the entity keeps its state.
            continue;
        };
        if *generation != binding.generation {
            // A binding stamped by another scene load is never driven.
            continue;
        }
        let Some(state) = node_states.get(&binding.node) else {
            // This clip does not drive that node.
            continue;
        };
        if let Some(pose) = state.pose().copied() {
            writes.push((entity, NodeWrite::Pose(pose)));
        }
        if let Some(visibility) = state.visibility() {
            // The visibility channel carries no `Resolved` reference, so it
            // has no blocked-track case: a reached key is a definite
            // `Visible`/`Hidden`. A node whose channel has reached no key
            // yet contributes nothing, which keeps the previously applied
            // value (the base state the object was spawned with) — the same
            // rule every aspect of a node follows.
            writes.push((entity, NodeWrite::Visibility(visibility)));
        }
        if let Some(Resolved::Known(known)) = state.material() {
            writes.push((entity, NodeWrite::Material(known.value.clone())));
        }
        if let Some(attachment) = state.attachment() {
            match &attachment.parent {
                // `None` is a detach — a definite transition, not an
                // unknown — and a known parent is kind-validated at clip
                // assembly, so both apply. An unknown parent was published
                // above and is never written.
                None | Some(Resolved::Known(_)) => {
                    writes.push((entity, NodeWrite::Attachment(attachment.clone())));
                }
                Some(Resolved::Unknown { .. }) => {}
            }
        }
    }

    // 5. Publish the playback, apply the writes idempotently, and append
    //    everything that was published to the log.
    world.insert_resource(playback);
    for (entity, write) in writes {
        apply_write(world, entity, write);
    }
    if !events.is_empty()
        || !blocked_markers.is_empty()
        || !blocked_tracks.is_empty()
        || !refused.is_empty()
    {
        let mut log = world.remove_resource::<AnimationLog>().unwrap_or_default();
        log.events.extend(events);
        log.blocked_markers.extend(blocked_markers);
        log.blocked_tracks.extend(blocked_tracks);
        log.refused.extend(refused);
        world.insert_resource(log);
    }

    // 6. The attachment records the instances published become hierarchy
    //    changes in the same tick: the parent change with its authored pose
    //    policy, the descendant world poses behind it, and the velocity a
    //    detach inherits from the parent it is leaving — applied once per
    //    change, with every refusal and every missing velocity source
    //    published once instead of guessed (F20-C, AC03).
    super::attachment::apply_attachment_transitions(world);
}

/// Records one unknown track of a playing instance, the first time the
/// evaluated state of that `(node, track)` reaches it — the once-per-instance
/// publication the [`BlockedTrack`] doc promises, independent of any binding.
fn publish_blocked(
    playing: &mut PlayingClip,
    published: &mut Vec<BlockedTrack>,
    clip: &ContentId,
    node: &ContentId,
    track: TrackKind,
    claim_id: &ClaimId,
    reason: &str,
) {
    if playing.blocked_tracks.insert((node.to_owned(), track)) {
        published.push(BlockedTrack {
            clip: clip.to_owned(),
            node: node.to_owned(),
            track,
            claim_id: claim_id.to_owned(),
            reason: reason.to_owned(),
        });
    }
}

/// One track value on its way to one verified entity.
#[derive(Clone, Debug, PartialEq)]
enum NodeWrite {
    /// The transform track's pose.
    Pose(PoseSample),
    /// The visibility channel's verdict.
    Visibility(Visibility),
    /// The material track's resolved material.
    Material(ContentId),
    /// The attachment track's parent change.
    Attachment(AttachmentState),
}

/// Writes one track value onto its entity, only when it changed — running
/// the same advance twice writes the same component twice, and a component
/// that already holds the value is left untouched (idempotence).
fn apply_write(world: &mut World, entity: Entity, write: NodeWrite) {
    match write {
        NodeWrite::Pose(pose) => {
            insert_changed::<NodeAnimatedPose>(world, entity, NodeAnimatedPose(pose))
        }
        NodeWrite::Visibility(visibility) => insert_changed::<NodeAnimatedVisibility>(
            world,
            entity,
            NodeAnimatedVisibility::new(visibility),
        ),
        NodeWrite::Material(material) => {
            insert_changed::<NodeAnimatedMaterial>(world, entity, NodeAnimatedMaterial(material))
        }
        NodeWrite::Attachment(attachment) => insert_changed::<NodeAnimatedAttachment>(
            world,
            entity,
            NodeAnimatedAttachment(attachment),
        ),
    }
}

fn insert_changed<T: Component + PartialEq>(world: &mut World, entity: Entity, value: T) {
    if world.get::<T>(entity) != Some(&value) {
        world.entity_mut(entity).insert(value);
    }
}
