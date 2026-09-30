//! Fixed-tick object animation runtime: channels, event markers and the
//! per-tick evaluator (F20-A).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **runtime half** of the animation contract — the typed
//! records the fixed-tick simulation evaluates and the evaluator itself. The
//! provenance-carrying declared form (scene-node ids, origins) lives in
//! `cs_content::animation`; `cs_app::animation` lowers one into the other.
//! `cs_sim` cannot depend on `cs_content` (`docs/01-ARCHITECTURE.md`), so the
//! runtime records here are built from `cs_types` values only and carry
//! [`Resolved`] where an unknown must survive into gameplay (F20
//! non-negotiable behavior 2).
//!
//! # Time and events
//!
//! Clips are measured in whole simulation ticks (F20 non-negotiable behavior
//! 1): a channel key or an event marker sits at an integer clip tick and the
//! evaluator's only motion is [`AnimatedObject::advance_to`]. Interpolation
//! *between* ticks is presentation-only and lives in `cs_app::animation`;
//! nothing here consumes wall time.
//!
//! A marker that reaches its tick produces an [`AnimationEvent`] stamped with
//! an [`AnimationEventId`], the animation-scoped realization of the contract's
//! `EventId(session, tick, producer, sequence)` shape — `cs_types` does not
//! implement the shared `SessionId`/`EventId` types yet (recorded in
//! `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`),
//! so this module carries its own fields rather than guessing a shared one.
//!
//! # Dedup and blocking semantics
//!
//! [`MarkerEffect::Gameplay`] fires **at most once per activation**, even when
//! [`LoopMode::Loop`] wraps the clip — a looping propeller cannot re-emit a
//! one-shot mission event (the AC02 shape), and a destroyed node cannot be
//! respawned by a looping transition because the transition's marker never
//! repeats (non-negotiable behavior 3). [`MarkerEffect::Presentation`] cues
//! may repeat once per pass. A marker whose effect is [`Resolved::Unknown`]
//! is not skipped and not guessed: the crossing surfaces a [`BlockedMarker`]
//! in the outcome instead of an event, so the affected gameplay transition
//! is visibly blocked with its claim id and reason.
//!
//! # Coherent node state
//!
//! [`AnimatedObject::states`] evaluates every channel at the current clip
//! position into one [`AnimatedNodeState`] per touched node. The state holds
//! a single `pose`, and `mesh_pose`/`collider_pose` both read it — the
//! render surface and the collision surface cannot diverge because they are
//! the same evaluated value (the F11 `visual_transform`/`collision_transform`
//! pattern). A node whose visibility channel reports
//! [`Visibility::Hidden`] reports `collider_enabled() == false`, so a
//! visibility swap cannot leave an invisible wall (a designed rule; whether
//! the original couples visibility to collision is unmeasured — see the
//! findings file).
//!
//! # What is measured and what is designed
//!
//! Every record, vocabulary and fixture value here is **newly authored
//! project design**. The original animation container layouts
//! (`mis_anim.zbd`, `cam_anim.zbd`) are located but undecoded (F13), and the
//! spec forbids assuming MechWarrior animation semantics. Nothing here
//! claims to reproduce original behavior; the format and verification
//! stages are F20-B/D and F13's continuation.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Quaternion, SpaceError};

// --------------------------------------------------------------- pose -----

/// One sampled pose of a node in its parent's space: rotation, translation
/// in meters and per-axis scale, all canonical.
///
/// TRS rather than a full affine matrix: rotation, translation and scale are
/// the quantities the authored channels drive. Authored shear has no channel
/// representation in this stage and is a recorded limitation (F20-B decides
/// whether original tracks need it); mirroring survives because a negative
/// scale component is legal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseSample {
    rotation: Quaternion,
    translation_m: [f64; 3],
    scale: [f64; 3],
}

const TRANSLATION_FIELDS: [&str; 3] = ["translation_m[0]", "translation_m[1]", "translation_m[2]"];
const SCALE_FIELDS: [&str; 3] = ["scale[0]", "scale[1]", "scale[2]"];

impl PoseSample {
    /// The identity pose: no rotation, no offset, unit scale.
    pub const IDENTITY: Self = Self {
        rotation: Quaternion::IDENTITY,
        translation_m: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
    };

    /// Validates a pose sample, refusing non-finite translation or scale.
    /// The rotation arrives as a [`Quaternion`], so it is already unit
    /// length by construction.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the offending component.
    pub fn try_new(
        rotation: Quaternion,
        translation_m: [f64; 3],
        scale: [f64; 3],
    ) -> Result<Self, SpaceError> {
        check_finite(translation_m, TRANSLATION_FIELDS)?;
        check_finite(scale, SCALE_FIELDS)?;
        Ok(Self {
            rotation,
            translation_m,
            scale,
        })
    }

    /// The rotation.
    #[must_use]
    pub const fn rotation(&self) -> Quaternion {
        self.rotation
    }

    /// The translation, in meters.
    #[must_use]
    pub const fn translation_m(&self) -> [f64; 3] {
        self.translation_m
    }

    /// The per-axis scale; a negative component mirrors.
    #[must_use]
    pub const fn scale(&self) -> [f64; 3] {
        self.scale
    }

    /// Linear interpolation between two poses at `alpha ∈ [0, 1]`:
    /// componentwise lerp for translation and scale, normalized lerp with a
    /// shortest-path sign fix for the rotation.
    ///
    /// This is the only interpolation the runtime performs — always at an
    /// integer tick. Fractional-alpha resampling is `cs_app::animation`'s
    /// presentation concern and produces exactly this function's output at
    /// whole-tick inputs.
    ///
    /// # Errors
    ///
    /// [`AnimationError::AlphaOutOfRange`] when `alpha` is non-finite or
    /// outside `[0, 1]`, [`AnimationError::DegenerateRotation`] when the
    /// blended rotation has no direction to normalize, and
    /// [`SpaceError::NonFinite`] if a blended component is not finite.
    pub fn interpolate(from: &Self, to: &Self, alpha: f64) -> Result<Self, AnimationError> {
        if !(0.0..=1.0).contains(&alpha) {
            return Err(AnimationError::AlphaOutOfRange { alpha });
        }
        let lerp = |a: [f64; 3], b: [f64; 3]| {
            [
                a[0] + (b[0] - a[0]) * alpha,
                a[1] + (b[1] - a[1]) * alpha,
                a[2] + (b[2] - a[2]) * alpha,
            ]
        };
        let translation_m = lerp(from.translation_m, to.translation_m);
        let scale = lerp(from.scale, to.scale);
        check_finite(translation_m, TRANSLATION_FIELDS).map_err(AnimationError::Pose)?;
        check_finite(scale, SCALE_FIELDS).map_err(AnimationError::Pose)?;

        // nlerp with a hemisphere sign fix: flipping `to` when the dot is
        // negative takes the short arc and never lands on a zero blend for
        // unit inputs (the worst case is |blend| = 1/√2 at a 180° spread).
        let a = from.rotation.components();
        let mut b = to.rotation.components();
        let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        if dot < 0.0 {
            b = [-b[0], -b[1], -b[2], -b[3]];
        }
        let blended = [
            a[0] + (b[0] - a[0]) * alpha,
            a[1] + (b[1] - a[1]) * alpha,
            a[2] + (b[2] - a[2]) * alpha,
            a[3] + (b[3] - a[3]) * alpha,
        ];
        let length = (blended[0] * blended[0]
            + blended[1] * blended[1]
            + blended[2] * blended[2]
            + blended[3] * blended[3])
            .sqrt();
        if !length.is_finite() || length <= f64::EPSILON {
            return Err(AnimationError::DegenerateRotation);
        }
        let rotation = Quaternion::try_new([
            blended[0] / length,
            blended[1] / length,
            blended[2] / length,
            blended[3] / length,
        ])
        .map_err(AnimationError::Pose)?;
        Ok(Self {
            rotation,
            translation_m,
            scale,
        })
    }
}

fn check_finite<const N: usize>(
    values: [f64; N],
    fields: [&'static str; N],
) -> Result<(), SpaceError> {
    for (value, field) in values.into_iter().zip(fields) {
        if !value.is_finite() {
            return Err(SpaceError::NonFinite { field });
        }
    }
    Ok(())
}

// ------------------------------------------------------------- channels ---

/// How a channel moves between two authored keys.
///
/// Both modes are evaluated only at integer ticks; the difference is the
/// semantic pose *at* a tick, not smoothness between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// The channel holds the previous key's value until the next key's tick.
    Step,
    /// The channel linearly interpolates between the surrounding keys.
    Linear,
}

/// Whether a node is drawn.
///
/// This is the runtime's designed vocabulary; it deliberately parallels
/// `cs_content::scene::NodeVisibility` without depending on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// The node is drawn.
    Visible,
    /// The node is not drawn.
    Hidden,
}

/// The explicit pose policy of a parent change (F20 non-negotiable behavior
/// 4): an attach/detach record must say which pose survives, never silently
/// keep one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosePolicy {
    /// The node's world pose is preserved across the parent change (the
    /// local pose is recomputed against the new parent by the consumer).
    KeepWorldPose,
    /// The node's local pose under the new parent is preserved (the world
    /// pose follows the parent).
    KeepLocalPose,
}

/// One attachment operation: attach to a parent node or detach from the
/// current one.
///
/// The parent is [`Resolved`] because a reference whose target cannot be
/// identified is an explicit unknown, not a guess; evaluating an unknown
/// parent keeps the state unknown rather than inventing an attachment.
/// The velocity a detached body inherits is F20-C's consumer concern — the
/// record here only states *that* the parent changed and how the pose is
/// preserved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentOp {
    /// Attach the node under `parent`.
    ///
    /// The resolved parent is boxed so `Attach` does not dwarf `Detach`
    /// (`Resolved` carries a full claim id and reason).
    Attach {
        /// The new parent, a `scene_node` id or an explicit unknown.
        parent: Box<Resolved<ContentId>>,
        /// Which pose is preserved.
        pose: PosePolicy,
    },
    /// Detach the node from its current parent.
    Detach {
        /// Which pose is preserved.
        pose: PosePolicy,
    },
}

/// A transform keyframe: the pose the channel holds (or interpolates
/// toward) from `tick` onward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformKey {
    /// Clip tick this key takes effect at.
    pub tick: u64,
    /// The canonical pose.
    pub pose: PoseSample,
}

/// A visibility keyframe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibilityKey {
    /// Clip tick this key takes effect at.
    pub tick: u64,
    /// The visibility from this tick on.
    pub visibility: Visibility,
}

/// A material-swap keyframe. An unresolved material stays
/// [`Resolved::Unknown`] in the evaluated state rather than inventing one.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialKey {
    /// Clip tick this key takes effect at.
    pub tick: u64,
    /// The material from this tick on (`material` kind when known).
    pub material: Resolved<ContentId>,
}

/// An attachment keyframe.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentKey {
    /// Clip tick this key takes effect at.
    pub tick: u64,
    /// The operation from this tick on.
    pub op: AttachmentOp,
}

/// One channel of a clip: a typed key track bound to one scene node.
///
/// Keys are ordered by strictly increasing `tick` (validated at
/// [`AnimatedClip::try_new`]), so evaluation is a binary-search-free scan.
/// A channel with no key whose tick has been reached contributes nothing —
/// that aspect keeps the object's base state rather than inventing one.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeChannel {
    /// A node-transform channel.
    Transform {
        /// The driven node (`scene_node` kind).
        target: ContentId,
        /// How the pose moves between keys.
        interpolation: Interpolation,
        /// Keys ordered by strictly increasing tick.
        keys: Vec<TransformKey>,
    },
    /// A visibility-swap channel.
    Visibility {
        /// The driven node (`scene_node` kind).
        target: ContentId,
        /// Keys ordered by strictly increasing tick.
        keys: Vec<VisibilityKey>,
    },
    /// A material-swap channel.
    Material {
        /// The driven node (`scene_node` kind).
        target: ContentId,
        /// Keys ordered by strictly increasing tick.
        keys: Vec<MaterialKey>,
    },
    /// An attachment channel.
    Attachment {
        /// The driven node (`scene_node` kind).
        target: ContentId,
        /// Keys ordered by strictly increasing tick.
        keys: Vec<AttachmentKey>,
    },
}

impl NodeChannel {
    /// The node this channel drives.
    #[must_use]
    pub fn target(&self) -> &ContentId {
        match self {
            Self::Transform { target, .. }
            | Self::Visibility { target, .. }
            | Self::Material { target, .. }
            | Self::Attachment { target, .. } => target,
        }
    }

    /// The channel's tick span check: every key sits inside `0..=duration`.
    fn key_ticks(&self) -> Vec<u64> {
        match self {
            Self::Transform { keys, .. } => keys.iter().map(|key| key.tick).collect(),
            Self::Visibility { keys, .. } => keys.iter().map(|key| key.tick).collect(),
            Self::Material { keys, .. } => keys.iter().map(|key| key.tick).collect(),
            Self::Attachment { keys, .. } => keys.iter().map(|key| key.tick).collect(),
        }
    }
}

// ------------------------------------------------------------- markers ----

/// What firing an event marker asks its consumer to do.
///
/// This is the **designed** vocabulary of this engine's animation IR. The
/// original marker semantics are undecoded (F13 classifies `mis_anim.zbd` /
/// `cam_anim.zbd` as script-family programs with no instruction meaning
/// yet), so no variant here claims an original counterpart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerEffect {
    /// A gameplay cue handed to the mission/objective layer: mission events,
    /// state transitions, pickup or destruction triggers.
    ///
    /// Gameplay effects are **one-shot per activation**: they fire once no
    /// matter how many times a looping clip wraps, because they move
    /// simulation state (F20 non-negotiable behavior 3, AC02).
    Gameplay {
        /// The authored cue label the consuming layer binds.
        cue: String,
    },
    /// A presentation-only cue (sound sync, particle bursts, cosmetic
    /// flashes). It cannot move simulation state, so it may fire once per
    /// pass of a looping clip.
    Presentation {
        /// The authored cue label the presentation layer binds.
        cue: String,
    },
}

impl MarkerEffect {
    /// Whether this effect moves gameplay state (and is therefore dedup'd
    /// once per activation).
    #[must_use]
    pub const fn is_gameplay(&self) -> bool {
        matches!(self, Self::Gameplay { .. })
    }

    /// The authored cue label.
    #[must_use]
    pub fn cue(&self) -> &str {
        match self {
            Self::Gameplay { cue } | Self::Presentation { cue } => cue,
        }
    }
}

/// One event marker: a stable key, a clip tick and a resolved effect.
///
/// `key` is the authored identity used for dedup and diagnostics — unique
/// within the clip, stable across re-parses, and never an array index. An
/// unknown `effect` is carried, not dropped: when its tick is reached the
/// evaluator reports a [`BlockedMarker`] instead of emitting an event, so
/// the gameplay transition it gates cannot fire silently (F20
/// non-negotiable behavior 2).
#[derive(Clone, Debug, PartialEq)]
pub struct ClipMarker {
    /// Clip tick this marker fires at (`0..=duration`).
    pub tick: u64,
    /// The stable marker key.
    pub key: String,
    /// What the marker asks for, or an explicit unknown with its claim.
    pub effect: Resolved<MarkerEffect>,
}

// ----------------------------------------------------------------- clip ---

/// Whether a clip stops at its end or wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once; the clip saturates at `duration` and reports
    /// [`TickOutcome::finished`].
    Once,
    /// Loop forever; position wraps modulo `duration`.
    Loop,
}

/// One animation clip in runtime form: the typed input the evaluator steps.
///
/// Key and marker ticks live in `0..=duration`. In [`LoopMode::Loop`] a tick
/// of exactly `duration` coincides with the next pass's tick `0` for channel
/// evaluation (position is `time % duration`), while its marker still fires
/// at the wrap instant — the seam is one point in time, not two.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimatedClip {
    id: ContentId,
    duration_ticks: u64,
    loop_mode: LoopMode,
    channels: Vec<NodeChannel>,
    markers: Vec<ClipMarker>,
}

impl AnimatedClip {
    /// Validates and assembles a clip.
    ///
    /// # Errors
    ///
    /// [`AnimationError::NotAnAnimationTrack`], [`AnimationError::ZeroDuration`],
    /// [`AnimationError::TargetKind`], [`AnimationError::EmptyChannel`],
    /// [`AnimationError::KeyTicks`], [`AnimationError::BeyondDuration`],
    /// [`AnimationError::MaterialKind`], [`AnimationError::AttachmentKind`],
    /// [`AnimationError::MarkerTicks`], [`AnimationError::EmptyMarkerKey`],
    /// [`AnimationError::DuplicateMarkerKey`] and
    /// [`AnimationError::EmptyMarkerCue`].
    pub fn try_new(
        id: ContentId,
        duration_ticks: u64,
        loop_mode: LoopMode,
        channels: Vec<NodeChannel>,
        markers: Vec<ClipMarker>,
    ) -> Result<Self, AnimationError> {
        if id.kind() != ContentKind::AnimationTrack {
            return Err(AnimationError::NotAnAnimationTrack { kind: id.kind() });
        }
        if duration_ticks == 0 {
            return Err(AnimationError::ZeroDuration { clip: id });
        }
        for channel in &channels {
            if channel.target().kind() != ContentKind::SceneNode {
                return Err(AnimationError::TargetKind {
                    target: channel.target().clone(),
                });
            }
            let ticks = channel.key_ticks();
            if ticks.is_empty() {
                return Err(AnimationError::EmptyChannel {
                    target: channel.target().clone(),
                });
            }
            for pair in ticks.windows(2) {
                if pair[0] >= pair[1] {
                    return Err(AnimationError::KeyTicks {
                        target: channel.target().clone(),
                        first: pair[0],
                        second: pair[1],
                    });
                }
            }
            for tick in &ticks {
                if *tick > duration_ticks {
                    return Err(AnimationError::BeyondDuration {
                        tick: *tick,
                        duration: duration_ticks,
                    });
                }
            }
            match channel {
                NodeChannel::Material { keys, target } => {
                    for key in keys {
                        if let Resolved::Known(known) = &key.material
                            && known.value.kind() != ContentKind::Material
                        {
                            return Err(AnimationError::MaterialKind {
                                target: target.clone(),
                                material: known.value.kind(),
                            });
                        }
                    }
                }
                NodeChannel::Attachment { keys, target } => {
                    for key in keys {
                        if let AttachmentOp::Attach { parent, .. } = &key.op
                            && let Resolved::Known(parent) = parent.as_ref()
                            && parent.value.kind() != ContentKind::SceneNode
                        {
                            return Err(AnimationError::AttachmentKind {
                                target: target.clone(),
                                parent: parent.value.kind(),
                            });
                        }
                    }
                }
                NodeChannel::Transform { .. } | NodeChannel::Visibility { .. } => {}
            }
        }
        let mut marker_keys = HashSet::new();
        for (index, marker) in markers.iter().enumerate() {
            if marker.tick > duration_ticks {
                return Err(AnimationError::BeyondDuration {
                    tick: marker.tick,
                    duration: duration_ticks,
                });
            }
            if index > 0 && markers[index - 1].tick > marker.tick {
                return Err(AnimationError::MarkerTicks { tick: marker.tick });
            }
            if marker.key.is_empty() {
                return Err(AnimationError::EmptyMarkerKey);
            }
            if !marker_keys.insert(marker.key.as_str()) {
                return Err(AnimationError::DuplicateMarkerKey {
                    key: marker.key.clone(),
                });
            }
            if let Resolved::Known(effect) = &marker.effect
                && effect.value.cue().is_empty()
            {
                return Err(AnimationError::EmptyMarkerCue {
                    key: marker.key.clone(),
                });
            }
        }
        Ok(Self {
            id,
            duration_ticks,
            loop_mode,
            channels,
            markers,
        })
    }

    /// The clip's `animation_track` id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// The clip length in ticks; positions are `0..=duration` for a
    /// [`LoopMode::Once`] clip and wrap modulo `duration` for
    /// [`LoopMode::Loop`].
    #[must_use]
    pub const fn duration_ticks(&self) -> u64 {
        self.duration_ticks
    }

    /// The loop mode.
    #[must_use]
    pub const fn loop_mode(&self) -> LoopMode {
        self.loop_mode
    }

    /// The channels, in authored order.
    #[must_use]
    pub fn channels(&self) -> &[NodeChannel] {
        &self.channels
    }

    /// The markers, sorted by tick.
    #[must_use]
    pub fn markers(&self) -> &[ClipMarker] {
        &self.markers
    }
}

// -------------------------------------------------------------- events ----

/// The identity of one fired animation event: the animation-scoped
/// realization of `EventId(session, tick, producer, sequence)` from
/// `docs/contracts/IDENTITY-CONTENT.md`.
///
/// `session` is the session generation the playing object belongs to and
/// `tick` the simulation tick the event fired at — both supplied by the
/// caller, so a replayed or restarted session can never collide with the
/// previous one's events. `producer` distinguishes the playing objects of a
/// session and `sequence` orders this object's own events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationEventId {
    /// The session generation (`IDENTITY-CONTENT` session generations).
    pub session: u64,
    /// The simulation tick the event fired at.
    pub tick: Tick,
    /// The playing object's producer serial within the session.
    pub producer: u32,
    /// The event's sequence within this producer.
    pub sequence: u32,
}

/// One event a marker crossing produced.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEvent {
    /// The unique identity of this firing.
    pub id: AnimationEventId,
    /// The clip it came from.
    pub clip: ContentId,
    /// The marker's stable key.
    pub marker: String,
    /// The effect handed to the consumer.
    pub effect: MarkerEffect,
    /// The loop pass the firing belonged to (0 for the first pass).
    pub pass: u64,
}

/// A marker crossing that was **blocked** because its effect is unknown.
///
/// The transition the marker gates does not fire; the claim id and reason
/// of the unknown are surfaced so the consumer can refuse the transition
/// visibly rather than continue with half-applied state (F20 non-negotiable
/// behavior 2). Each marker blocks at most once per activation — after the
/// first report the gap is on record.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockedMarker {
    /// The clip the marker belongs to.
    pub clip: ContentId,
    /// The marker's stable key.
    pub marker: String,
    /// The claim the unknown is recorded under.
    pub claim_id: ClaimId,
    /// Why the effect is unknown.
    pub reason: String,
    /// The loop pass the crossing belonged to.
    pub pass: u64,
}

/// What one [`AnimatedObject::advance_to`] produced.
#[derive(Clone, Debug, PartialEq)]
pub struct TickOutcome {
    /// The simulation tick this advance ran in.
    pub at: Tick,
    /// The absolute clip time reached (monotonic; unwrapped across loops).
    pub clip_time: u64,
    /// The events fired, in firing order.
    pub events: Vec<AnimationEvent>,
    /// The marker crossings blocked by unknown effects.
    pub blocked: Vec<BlockedMarker>,
    /// How many loop passes this advance completed.
    pub passes_completed: u64,
    /// Whether a [`LoopMode::Once`] clip reached its end. Always false for a
    /// looping clip.
    pub finished: bool,
}

// ------------------------------------------------------------- evaluator --

/// One playing instance of an [`AnimatedClip`] — the fixed-tick evaluator.
///
/// The object advances forward only, in whole ticks, and owns the dedup
/// state that makes gameplay markers one-shot per activation. State is a
/// pure function of the clip position, so arriving at the same position by
/// one advance or a skip produces the same channels and the same set of
/// fired gameplay markers exactly once (the AC04 shape at this stage's
/// scope).
#[derive(Clone, Debug)]
pub struct AnimatedObject {
    clip: AnimatedClip,
    session: u64,
    producer: u32,
    time: u64,
    sequence: u32,
    fired_gameplay: HashSet<usize>,
    fired_presentation: BTreeMap<usize, u64>,
    reported_blocked: HashSet<usize>,
}

impl AnimatedObject {
    /// Activates a clip for `session`, producing events under `producer`.
    ///
    /// Activation lands the head at clip time 0 and evaluates that state;
    /// markers at tick 0 fire on the first advance that includes 0 (any
    /// `advance_to`), never spontaneously at construction.
    #[must_use]
    pub fn new(clip: AnimatedClip, session: u64, producer: u32) -> Self {
        Self {
            clip,
            session,
            producer,
            time: 0,
            sequence: 0,
            fired_gameplay: HashSet::new(),
            fired_presentation: BTreeMap::new(),
            reported_blocked: HashSet::new(),
        }
    }

    /// The clip this object plays.
    #[must_use]
    pub fn clip(&self) -> &AnimatedClip {
        &self.clip
    }

    /// The absolute clip time reached (monotonic, unwrapped across loops).
    #[must_use]
    pub const fn time(&self) -> u64 {
        self.time
    }

    /// The clip-local position the state is evaluated at: `time` clamped to
    /// the end for [`LoopMode::Once`], `time % duration` for
    /// [`LoopMode::Loop`].
    #[must_use]
    pub fn position(&self) -> u64 {
        match self.clip.loop_mode() {
            LoopMode::Once => self.time.min(self.clip.duration_ticks()),
            LoopMode::Loop => self.time % self.clip.duration_ticks(),
        }
    }

    /// The loop pass the head is in (`time / duration`).
    #[must_use]
    pub fn pass(&self) -> u64 {
        self.time / self.clip.duration_ticks()
    }

    /// Whether a [`LoopMode::Once`] clip has reached its end.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(self.clip.loop_mode(), LoopMode::Once) && self.time == self.clip.duration_ticks()
    }

    /// Advances the playback head to absolute clip time `clip_time`,
    /// emitting one [`AnimationEvent`] per marker crossing reached and one
    /// [`BlockedMarker`] per newly reached unknown-effect marker.
    ///
    /// `at` is the session simulation tick this advance commits to — the
    /// tick stamped into every event id. Crossing several marker instants in
    /// one advance (a skip, or a long jump through a loop) fires each
    /// gameplay marker exactly once, in tick order.
    ///
    /// # Errors
    ///
    /// [`AnimationError::Regression`] when `clip_time` is behind the current
    /// head. Reversing is refused outright rather than replayed, so a
    /// reversed cinematic can never re-offer a one-shot marker.
    pub fn advance_to(&mut self, clip_time: u64, at: Tick) -> Result<TickOutcome, AnimationError> {
        if clip_time < self.time {
            return Err(AnimationError::Regression {
                from: self.time,
                to: clip_time,
            });
        }
        let duration = self.clip.duration_ticks();
        let reached = match self.clip.loop_mode() {
            LoopMode::Once => clip_time.min(duration),
            LoopMode::Loop => clip_time,
        };
        let previous = self.time;

        // Candidate instants: marker.tick + pass * duration inside
        // [previous, reached]. The left end is inclusive — a marker at the
        // head's current instant still fires the first time an advance
        // reaches it (dedup, not the inequality, prevents a second firing).
        // A `Once` clip has exactly one pass, so pass indices stay at 0 and
        // nothing re-fires at the terminal instant.
        let mut candidates: Vec<(u64, usize, u64)> = Vec::new();
        let first_pass = previous / duration;
        let last_pass = match self.clip.loop_mode() {
            LoopMode::Once => 0,
            LoopMode::Loop => reached / duration,
        };
        for (index, marker) in self.clip.markers().iter().enumerate() {
            for pass in first_pass..=last_pass {
                let Some(instant) = marker.tick.checked_add(pass * duration) else {
                    break;
                };
                if instant > reached {
                    break;
                }
                if instant >= previous {
                    candidates.push((instant, index, pass));
                }
            }
        }
        candidates.sort();

        let mut events = Vec::new();
        let mut blocked = Vec::new();
        for (_instant, index, pass) in candidates {
            let marker = &self.clip.markers()[index];
            match &marker.effect {
                Resolved::Unknown { claim_id, reason } => {
                    if self.reported_blocked.insert(index) {
                        blocked.push(BlockedMarker {
                            clip: self.clip.id().clone(),
                            marker: marker.key.clone(),
                            claim_id: claim_id.clone(),
                            reason: reason.clone(),
                            pass,
                        });
                    }
                }
                Resolved::Known(effect) => {
                    // Dedup retention is bounded by the clip's marker count:
                    // gameplay markers by index, presentation markers by the
                    // last pass each index fired in. No unbounded set grows
                    // with clip time.
                    let fresh = if effect.value.is_gameplay() {
                        self.fired_gameplay.insert(index)
                    } else {
                        self.fired_presentation.insert(index, pass) != Some(pass)
                    };
                    if fresh {
                        events.push(AnimationEvent {
                            id: AnimationEventId {
                                session: self.session,
                                tick: at,
                                producer: self.producer,
                                sequence: self.sequence,
                            },
                            clip: self.clip.id().clone(),
                            marker: marker.key.clone(),
                            effect: effect.value.clone(),
                            pass,
                        });
                        self.sequence += 1;
                    }
                }
            }
        }
        self.time = reached;
        Ok(TickOutcome {
            at,
            clip_time: reached,
            events,
            blocked,
            passes_completed: reached / duration - previous / duration,
            finished: matches!(self.clip.loop_mode(), LoopMode::Once) && reached == duration,
        })
    }

    /// Evaluates every channel at the current position into one coherent
    /// [`AnimatedNodeState`] per touched node.
    ///
    /// An aspect with no reached key stays `None` — it keeps whatever base
    /// state the spawned object has instead of inventing a value.
    #[must_use]
    pub fn states(&self) -> BTreeMap<ContentId, AnimatedNodeState> {
        let position = self.position();
        let mut states: BTreeMap<ContentId, AnimatedNodeState> = BTreeMap::new();
        for channel in self.clip.channels() {
            let state = states
                .entry(channel.target().clone())
                .or_insert_with(|| AnimatedNodeState::new(channel.target().clone()));
            match channel {
                NodeChannel::Transform {
                    interpolation,
                    keys,
                    ..
                } => {
                    state.pose = sample_transform(keys, *interpolation, position);
                }
                NodeChannel::Visibility { keys, .. } => {
                    state.visibility = keys
                        .iter()
                        .rev()
                        .find(|key| key.tick <= position)
                        .map(|key| key.visibility);
                }
                NodeChannel::Material { keys, .. } => {
                    state.material = keys
                        .iter()
                        .rev()
                        .find(|key| key.tick <= position)
                        .map(|key| key.material.clone());
                }
                NodeChannel::Attachment { keys, .. } => {
                    state.attachment =
                        keys.iter()
                            .rev()
                            .find(|key| key.tick <= position)
                            .map(|key| match &key.op {
                                AttachmentOp::Attach { parent, pose } => AttachmentState {
                                    parent: Some(parent.as_ref().clone()),
                                    pose: *pose,
                                },
                                AttachmentOp::Detach { pose } => AttachmentState {
                                    parent: None,
                                    pose: *pose,
                                },
                            });
                }
            }
        }
        states
    }
}

/// Samples a transform channel at `position`: the latest key at or before
/// it for [`Interpolation::Step`], the linear blend of the surrounding keys
/// for [`Interpolation::Linear`].
fn sample_transform(
    keys: &[TransformKey],
    interpolation: Interpolation,
    position: u64,
) -> Option<PoseSample> {
    let previous = keys.iter().rev().find(|key| key.tick <= position)?;
    match interpolation {
        Interpolation::Step => Some(previous.pose),
        Interpolation::Linear => {
            let Some(next) = keys.iter().find(|key| key.tick > position) else {
                return Some(previous.pose);
            };
            let alpha = (position - previous.tick) as f64 / (next.tick - previous.tick) as f64;
            // A blend failure is unreachable for finite inputs (both poses
            // validated, alpha in (0, 1)); hold the last reached pose rather
            // than erase the channel's state.
            Some(
                PoseSample::interpolate(&previous.pose, &next.pose, alpha).unwrap_or(previous.pose),
            )
        }
    }
}

// ------------------------------------------------------------- state -----

/// The evaluated attachment state of one node.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentState {
    /// The current parent (`scene_node` id or explicit unknown); `None`
    /// after a detach.
    pub parent: Option<Resolved<ContentId>>,
    /// The authored pose policy of the last parent change.
    pub pose: PosePolicy,
}

/// The coherent per-tick state of one animated node.
///
/// Every field is `None` until a channel reaches it — `None` means
/// "unchanged from the spawned base state", never a default the animation
/// invented.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimatedNodeState {
    node: ContentId,
    pose: Option<PoseSample>,
    visibility: Option<Visibility>,
    material: Option<Resolved<ContentId>>,
    attachment: Option<AttachmentState>,
}

impl AnimatedNodeState {
    fn new(node: ContentId) -> Self {
        Self {
            node,
            pose: None,
            visibility: None,
            material: None,
            attachment: None,
        }
    }

    /// The node this state belongs to.
    #[must_use]
    pub fn node(&self) -> &ContentId {
        &self.node
    }

    /// The single evaluated pose. This is the *only* pose the node has:
    /// [`Self::mesh_pose`] and [`Self::collider_pose`] are two names for it,
    /// so a pose change cannot reach the render path without reaching the
    /// collision path in the same tick (the F20-A minimum scenario's
    /// coherence rule, stated structurally).
    #[must_use]
    pub const fn pose(&self) -> Option<&PoseSample> {
        self.pose.as_ref()
    }

    /// The pose the mesh presents — the same value as
    /// [`Self::collider_pose`].
    #[must_use]
    pub const fn mesh_pose(&self) -> Option<&PoseSample> {
        self.pose()
    }

    /// The pose the collider evaluates — the same value as
    /// [`Self::mesh_pose`].
    #[must_use]
    pub const fn collider_pose(&self) -> Option<&PoseSample> {
        self.pose()
    }

    /// The evaluated visibility, when a channel has reached one.
    #[must_use]
    pub const fn visibility(&self) -> Option<Visibility> {
        self.visibility
    }

    /// Whether the node's collider participates in collision.
    ///
    /// Designed rule: a [`Visibility::Hidden`] node carries no collider, so
    /// a visibility swap cannot leave an invisible obstacle behind; a node
    /// with no visibility value keeps its authored collider state.
    #[must_use]
    pub fn collider_enabled(&self) -> bool {
        !matches!(self.visibility, Some(Visibility::Hidden))
    }

    /// The evaluated material, when a channel has reached one. An unknown
    /// material stays [`Resolved::Unknown`] — the render gate refuses it
    /// rather than drawing a guess.
    #[must_use]
    pub fn material(&self) -> Option<&Resolved<ContentId>> {
        self.material.as_ref()
    }

    /// The evaluated attachment state, when a channel has reached one.
    #[must_use]
    pub fn attachment(&self) -> Option<&AttachmentState> {
        self.attachment.as_ref()
    }
}

// ------------------------------------------------------------- errors -----

/// Why clip assembly, lowering input or playback was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationError {
    /// The clip id is not in the `animation_track` namespace.
    NotAnAnimationTrack {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A clip with zero duration can never reach a key or a marker.
    ZeroDuration {
        /// The clip that was refused.
        clip: ContentId,
    },
    /// A channel drives a node id that is not a `scene_node`.
    TargetKind {
        /// The offending target id.
        target: ContentId,
    },
    /// A channel carries no keys; it would drive nothing.
    EmptyChannel {
        /// The channel's target.
        target: ContentId,
    },
    /// Channel keys were not in strictly increasing tick order.
    KeyTicks {
        /// The channel's target.
        target: ContentId,
        /// The first out-of-order tick.
        first: u64,
        /// The second.
        second: u64,
    },
    /// A key or marker sits beyond the clip's duration.
    BeyondDuration {
        /// The offending tick.
        tick: u64,
        /// The clip duration.
        duration: u64,
    },
    /// A material key resolved to a content id of the wrong kind.
    MaterialKind {
        /// The channel's target.
        target: ContentId,
        /// The kind the id actually names.
        material: ContentKind,
    },
    /// An attachment parent resolved to a content id of the wrong kind.
    AttachmentKind {
        /// The channel's target.
        target: ContentId,
        /// The kind the parent id actually names.
        parent: ContentKind,
    },
    /// Markers were not sorted by tick.
    MarkerTicks {
        /// The out-of-order marker tick.
        tick: u64,
    },
    /// A marker carried an empty key; it would have no stable identity.
    EmptyMarkerKey,
    /// Two markers share one key; dedup would collapse them.
    DuplicateMarkerKey {
        /// The duplicated key.
        key: String,
    },
    /// A marker's known effect carried an empty cue.
    EmptyMarkerCue {
        /// The marker's key.
        key: String,
    },
    /// A pose interpolation alpha was outside `[0, 1]` or non-finite.
    AlphaOutOfRange {
        /// The rejected alpha.
        alpha: f64,
    },
    /// A rotation blend degenerated to a vector with no direction.
    DegenerateRotation,
    /// A pose component failed validation.
    Pose(SpaceError),
    /// `advance_to` was asked to move the head backwards.
    Regression {
        /// The current clip time.
        from: u64,
        /// The refused clip time.
        to: u64,
    },
}

impl fmt::Display for AnimationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAnimationTrack { kind } => {
                write!(f, "clip id names a {kind}, not an animation_track")
            }
            Self::ZeroDuration { clip } => write!(f, "clip {clip} has zero duration"),
            Self::TargetKind { target } => {
                write!(f, "channel target {target} is not a scene_node")
            }
            Self::EmptyChannel { target } => {
                write!(f, "channel on {target} carries no keys")
            }
            Self::KeyTicks {
                target,
                first,
                second,
            } => write!(
                f,
                "channel on {target} has keys out of order ({first} then {second})"
            ),
            Self::BeyondDuration { tick, duration } => {
                write!(f, "tick {tick} is beyond the clip duration {duration}")
            }
            Self::MaterialKind { target, material } => write!(
                f,
                "material key on {target} resolves to a {material}, not a material"
            ),
            Self::AttachmentKind { target, parent } => write!(
                f,
                "attachment on {target} names a {parent} parent, not a scene_node"
            ),
            Self::MarkerTicks { tick } => {
                write!(f, "markers are not sorted by tick (at {tick})")
            }
            Self::EmptyMarkerKey => write!(f, "a marker key must not be empty"),
            Self::DuplicateMarkerKey { key } => {
                write!(f, "marker key {key:?} is used more than once")
            }
            Self::EmptyMarkerCue { key } => {
                write!(f, "marker {key:?} has an empty cue")
            }
            Self::AlphaOutOfRange { alpha } => {
                write!(f, "interpolation alpha {alpha} is outside [0, 1]")
            }
            Self::DegenerateRotation => {
                write!(f, "the blended rotation has no direction to normalize")
            }
            Self::Pose(source) => write!(f, "invalid pose sample: {source}"),
            Self::Regression { from, to } => {
                write!(f, "cannot rewind a playing clip from {from} to {to}")
            }
        }
    }
}

impl std::error::Error for AnimationError {}

// ----------------------------------------------------------- fixtures -----

/// The tick the synthetic door opens at.
pub const SYNTHETIC_DOOR_OPEN_TICK: u64 = 10;
/// The duration of the synthetic door clip.
pub const SYNTHETIC_DOOR_DURATION: u64 = 30;
/// The marker key of the door's gameplay cue.
pub const SYNTHETIC_DOOR_MARKER: &str = "door_opened";

/// The minimal synthetic fixture: a hangar door that opens at
/// [`SYNTHETIC_DOOR_OPEN_TICK`] of a 30-tick one-shot clip — a quarter turn
/// about its hinge, with a gameplay marker at the same tick.
///
/// Newly authored development content; every id lives under the `synthetic`
/// key so it can never be mistaken for retail content.
#[must_use]
pub fn synthetic_door_clip() -> AnimatedClip {
    let open = PoseSample::try_new(
        Quaternion::from_axis_angle(
            cs_types::space::UnitVec3::UP,
            cs_types::space::Radians(std::f64::consts::FRAC_PI_2),
        )
        .expect("a quarter turn is unit length"),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    )
    .expect("the fixture pose is finite");
    AnimatedClip::try_new(
        ContentId::from_source(ContentKind::AnimationTrack, "synthetic.door_open")
            .expect("fixture id is valid"),
        SYNTHETIC_DOOR_DURATION,
        LoopMode::Once,
        vec![NodeChannel::Transform {
            target: ContentId::from_source(ContentKind::SceneNode, "synthetic.hangar.door")
                .expect("fixture id is valid"),
            interpolation: Interpolation::Step,
            keys: vec![
                TransformKey {
                    tick: 0,
                    pose: PoseSample::IDENTITY,
                },
                TransformKey {
                    tick: SYNTHETIC_DOOR_OPEN_TICK,
                    pose: open,
                },
            ],
        }],
        vec![ClipMarker {
            tick: SYNTHETIC_DOOR_OPEN_TICK,
            key: SYNTHETIC_DOOR_MARKER.to_owned(),
            effect: Resolved::Known(cs_types::content::Known::new(
                MarkerEffect::Gameplay {
                    cue: "synthetic.hangar.door_opened".to_owned(),
                },
                cs_types::content::Provenance::designed(
                    ClaimId::new("f20a.synthetic-door").expect("claim id is valid"),
                ),
            )),
        }],
    )
    .expect("the synthetic door fixture is valid")
}

/// A 4-tick looping propeller: quarter turns of the rotor per tick, a
/// one-shot gameplay marker (`engine_started` at tick 1) and a
/// presentation cue (`blade_pass` at tick 0) that may repeat every pass.
///
/// Newly authored development content, used by the AC02-shaped test to show
/// that looping never re-emits the one-shot gameplay marker.
#[must_use]
pub fn synthetic_propeller_clip() -> AnimatedClip {
    let rotor = ContentId::from_source(ContentKind::SceneNode, "synthetic.plane.prop")
        .expect("fixture id is valid");
    let turn = |quarter: u8| {
        PoseSample::try_new(
            Quaternion::from_axis_angle(
                cs_types::space::UnitVec3::FORWARD,
                cs_types::space::Radians(f64::from(quarter) * std::f64::consts::FRAC_PI_2),
            )
            .expect("a quarter turn is unit length"),
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
        )
        .expect("the fixture pose is finite")
    };
    let designed = || {
        cs_types::content::Provenance::designed(
            ClaimId::new("f20a.synthetic-propeller").expect("claim id is valid"),
        )
    };
    AnimatedClip::try_new(
        ContentId::from_source(ContentKind::AnimationTrack, "synthetic.propeller")
            .expect("fixture id is valid"),
        4,
        LoopMode::Loop,
        vec![NodeChannel::Transform {
            target: rotor,
            interpolation: Interpolation::Step,
            keys: (0..4)
                .map(|tick| TransformKey {
                    tick,
                    pose: turn(tick as u8),
                })
                .collect(),
        }],
        vec![
            ClipMarker {
                tick: 0,
                key: "blade_pass".to_owned(),
                effect: Resolved::Known(cs_types::content::Known::new(
                    MarkerEffect::Presentation {
                        cue: "synthetic.plane.blade_pass".to_owned(),
                    },
                    designed(),
                )),
            },
            ClipMarker {
                tick: 1,
                key: "engine_started".to_owned(),
                effect: Resolved::Known(cs_types::content::Known::new(
                    MarkerEffect::Gameplay {
                        cue: "synthetic.plane.engine_started".to_owned(),
                    },
                    designed(),
                )),
            },
        ],
    )
    .expect("the synthetic propeller fixture is valid")
}
