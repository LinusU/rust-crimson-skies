//! The declared animation IR: channels and event markers with provenance
//! (F20-A), plus the declared fixtures the F20-B playback is driven with.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **content half** of the animation contract — the
//! normalized, provenance-carrying clip record a track importer produces and
//! the catalog consumes. Its runtime counterpart is
//! `cs_sim::animated_object` (numeric records and the fixed-tick evaluator);
//! the conversion boundary between them is `cs_app::animation::lower_clip`.
//! The split mirrors `flight_tuning` ↔ `cs_sim::flight`: this crate cannot
//! depend on `cs_sim`, so the declared record keeps its own typed fields and
//! re-validates them at its boundary.
//!
//! # Records
//!
//! An [`AnimationClip`] carries a stable `animation_track` [`ContentId`], an
//! [`Origin`], a whole-tick duration, a [`LoopMode`], the four channel kinds
//! the deliverable names — node [`TransformChannel`]s, [`VisibilityChannel`]
//! swaps, [`MaterialChannel`] swaps and [`AttachmentChannel`]s — and the
//! [`EventMarker`]s the simulation fires in fixed ticks with event ids
//! (F20 non-negotiable behavior 1).
//!
//! Channel targets are [`SceneNodeId`]s, so a channel can only ever name a
//! real scene node — never a mesh index, a pattern or a guessed name
//! (`IDENTITY-CONTENT`: stable content ids). References that cannot be
//! resolved stay [`Resolved::Unknown`]: an unknown material, an unknown
//! attachment parent and an unknown marker effect are all carried through
//! with their claim id and reason instead of being dropped or guessed (F20
//! non-negotiable behavior 2 — the runtime then *blocks* the transition they
//! gate rather than skipping it).
//!
//! # Designed vocabulary, not original data
//!
//! The original animation container layouts (`mis_anim.zbd`,
//! `cam_anim.zbd`) are located but undecoded — F13 classifies them as
//! script-family programs with no instruction semantics — and the spec
//! forbids importing MechWarrior's animation-event semantics. Every channel
//! kind, marker effect, loop rule and fixture value here is therefore
//! **newly authored project design** carrying `Origin::Designed` /
//! `Origin::SyntheticFixture` provenance, recorded in
//! `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md`.

use std::collections::HashSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Quaternion, SpaceError};

use crate::scene::{NodeVisibility, SceneNodeId};

// --------------------------------------------------------------- keys -----

/// One canonical transform sample: the TRS a transform keyframe holds.
///
/// Canonical space and SI units (`IDENTITY-CONTENT` numeric contract):
/// translation in meters, rotation a unit quaternion, scale per-axis with
/// negative components mirroring. TRS covers the authored door/propeller/
/// turret channels; authored shear has no representation in this stage —
/// recorded as a limitation for F20-B in the findings file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformSample {
    rotation: Quaternion,
    translation_m: [f64; 3],
    scale: [f64; 3],
}

impl TransformSample {
    /// The identity sample.
    pub const IDENTITY: Self = Self {
        rotation: Quaternion::IDENTITY,
        translation_m: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
    };

    /// Validates a sample, refusing non-finite translation or scale.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the offending component.
    pub fn try_new(
        rotation: Quaternion,
        translation_m: [f64; 3],
        scale: [f64; 3],
    ) -> Result<Self, SpaceError> {
        for (value, field) in translation_m.into_iter().zip([
            "translation_m[0]",
            "translation_m[1]",
            "translation_m[2]",
        ]) {
            if !value.is_finite() {
                return Err(SpaceError::NonFinite { field });
            }
        }
        for (value, field) in scale.into_iter().zip(["scale[0]", "scale[1]", "scale[2]"]) {
            if !value.is_finite() {
                return Err(SpaceError::NonFinite { field });
            }
        }
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

    /// The per-axis scale.
    #[must_use]
    pub const fn scale(&self) -> [f64; 3] {
        self.scale
    }
}

/// How a transform channel moves between keys.
///
/// Evaluation happens at integer ticks only (F20 non-negotiable behavior
/// 1); the choice is the semantic pose *at* a tick, not render smoothness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Hold the previous key until the next key's tick.
    Step,
    /// Linearly blend the surrounding keys at each tick.
    Linear,
}

/// Whether a clip stops at its end or wraps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once and hold the terminal state.
    Once,
    /// Loop; gameplay markers still fire at most once per activation (F20
    /// non-negotiable behavior 3, AC02).
    Loop,
}

/// The explicit pose policy of a parent change (F20 non-negotiable behavior
/// 4): the record says which pose survives, never a silent default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosePolicy {
    /// Preserve the node's world pose across the parent change.
    KeepWorldPose,
    /// Preserve the node's local pose under the new parent.
    KeepLocalPose,
}

/// One attachment operation.
///
/// The resolved parent is boxed: `Resolved` carries a full claim id and
/// reason, which would otherwise make `Attach` dwarf `Detach`.
#[derive(Clone, Debug, PartialEq)]
pub enum AttachmentOp {
    /// Attach the node under `parent`.
    Attach {
        /// The new parent node, or an explicit unknown.
        parent: Box<Resolved<SceneNodeId>>,
        /// Which pose is preserved.
        pose: PosePolicy,
    },
    /// Detach the node from its current parent.
    Detach {
        /// Which pose is preserved.
        pose: PosePolicy,
    },
}

/// A transform keyframe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformKey {
    /// Clip tick the key takes effect at (`0..=duration`).
    pub tick: u64,
    /// The canonical pose.
    pub pose: TransformSample,
}

/// A visibility-swap keyframe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibilityKey {
    /// Clip tick the key takes effect at.
    pub tick: u64,
    /// The visibility from this tick on.
    pub visibility: NodeVisibility,
}

/// A material-swap keyframe.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialKey {
    /// Clip tick the key takes effect at.
    pub tick: u64,
    /// The material (`material` kind when known) or an explicit unknown.
    pub material: Resolved<ContentId>,
}

/// An attachment keyframe.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentKey {
    /// Clip tick the key takes effect at.
    pub tick: u64,
    /// The operation from this tick on.
    pub op: AttachmentOp,
}

/// One channel: a typed key track bound to one scene node.
///
/// Keys are in strictly increasing tick order (validated by
/// [`AnimationClip::try_new`]); a channel with no reached key contributes
/// nothing, so untouched aspects keep the object's base state.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationChannel {
    /// A node-transform channel.
    Transform(TransformChannel),
    /// A visibility-swap channel.
    Visibility(VisibilityChannel),
    /// A material-swap channel.
    Material(MaterialChannel),
    /// An attachment channel.
    Attachment(AttachmentChannel),
}

impl AnimationChannel {
    /// The node this channel drives.
    #[must_use]
    pub fn target(&self) -> &SceneNodeId {
        match self {
            Self::Transform(channel) => &channel.target,
            Self::Visibility(channel) => &channel.target,
            Self::Material(channel) => &channel.target,
            Self::Attachment(channel) => &channel.target,
        }
    }
}

/// A node-transform channel.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformChannel {
    /// The driven node.
    pub target: SceneNodeId,
    /// How the pose moves between keys.
    pub interpolation: Interpolation,
    /// Keys ordered by strictly increasing tick.
    pub keys: Vec<TransformKey>,
}

/// A visibility-swap channel.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityChannel {
    /// The driven node.
    pub target: SceneNodeId,
    /// Keys ordered by strictly increasing tick.
    pub keys: Vec<VisibilityKey>,
}

/// A material-swap channel.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialChannel {
    /// The driven node.
    pub target: SceneNodeId,
    /// Keys ordered by strictly increasing tick.
    pub keys: Vec<MaterialKey>,
}

/// An attachment channel.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentChannel {
    /// The driven node.
    pub target: SceneNodeId,
    /// Keys ordered by strictly increasing tick.
    pub keys: Vec<AttachmentKey>,
}

// ------------------------------------------------------------- markers ----

/// What firing an event marker asks its consumer to do — the **designed**
/// effect vocabulary of this IR.
///
/// [`MarkerEffect::Gameplay`] moves simulation state (mission cues, state
/// transitions, destruction triggers), so the runtime fires it at most once
/// per activation even across loops. [`MarkerEffect::Presentation`] is
/// cosmetic (sound sync, particles) and may fire once per pass. No variant
/// claims an original counterpart: the original marker encoding is
/// undecoded (F13) and MechWarrior semantics do not transfer (spec
/// non-negotiable behavior 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerEffect {
    /// A gameplay cue handed to the mission/objective layer.
    Gameplay {
        /// The authored cue label the consumer binds.
        cue: String,
    },
    /// A presentation-only cue.
    Presentation {
        /// The authored cue label the consumer binds.
        cue: String,
    },
}

impl MarkerEffect {
    /// Whether this effect moves gameplay state.
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

/// One event marker: a stable authored key, a clip tick and a resolved
/// effect.
///
/// `key` is the marker's identity inside the clip — unique, stable across
/// re-parses, never an array index — and is what the runtime deduplicates
/// on and reports in fired/blocked records. An unknown `effect` is
/// retained: reaching it blocks the gated gameplay transition with the
/// unknown's claim and reason rather than skipping it.
#[derive(Clone, Debug, PartialEq)]
pub struct EventMarker {
    /// Clip tick the marker fires at (`0..=duration`).
    pub tick: u64,
    /// The stable marker key.
    pub key: String,
    /// What the marker asks for, or an explicit unknown with its claim.
    pub effect: Resolved<MarkerEffect>,
}

// ----------------------------------------------------------------- clip ---

/// One declared animation clip: the normalized record the catalog stores.
///
/// Validation ([`Self::try_new`]) refuses what the runtime cannot honestly
/// evaluate: a zero duration, unsorted or out-of-range key/marker ticks,
/// duplicate marker keys, a resolved material that is not a `material` id or
/// an attachment parent that is not a `scene_node` id. Unknown references
/// are not refused — they are data the runtime must block on — but a
/// *known* reference of the wrong kind is an authoring error, not content.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationClip {
    id: ContentId,
    origin: Origin,
    duration_ticks: u64,
    loop_mode: LoopMode,
    channels: Vec<AnimationChannel>,
    markers: Vec<EventMarker>,
    provenance: Provenance,
}

impl AnimationClip {
    /// Validates and assembles a declared clip.
    ///
    /// # Errors
    ///
    /// [`ClipError::NotAnAnimationTrack`], [`ClipError::ZeroDuration`],
    /// [`ClipError::EmptyChannel`], [`ClipError::KeyTicks`],
    /// [`ClipError::BeyondDuration`], [`ClipError::MaterialKind`],
    /// [`ClipError::MarkerTicks`], [`ClipError::EmptyMarkerKey`],
    /// [`ClipError::DuplicateMarkerKey`] and [`ClipError::EmptyMarkerCue`].
    pub fn try_new(
        id: ContentId,
        origin: Origin,
        duration_ticks: u64,
        loop_mode: LoopMode,
        channels: Vec<AnimationChannel>,
        markers: Vec<EventMarker>,
        provenance: Provenance,
    ) -> Result<Self, ClipError> {
        if id.kind() != ContentKind::AnimationTrack {
            return Err(ClipError::NotAnAnimationTrack { kind: id.kind() });
        }
        if duration_ticks == 0 {
            return Err(ClipError::ZeroDuration);
        }
        for channel in &channels {
            let ticks: Vec<u64> = match channel {
                AnimationChannel::Transform(channel) => {
                    channel.keys.iter().map(|key| key.tick).collect()
                }
                AnimationChannel::Visibility(channel) => {
                    channel.keys.iter().map(|key| key.tick).collect()
                }
                AnimationChannel::Material(channel) => {
                    for key in &channel.keys {
                        if let Resolved::Known(known) = &key.material
                            && known.value.kind() != ContentKind::Material
                        {
                            return Err(ClipError::MaterialKind {
                                target: channel.target.key().to_owned(),
                                material: known.value.kind(),
                            });
                        }
                    }
                    channel.keys.iter().map(|key| key.tick).collect()
                }
                AnimationChannel::Attachment(channel) => {
                    channel.keys.iter().map(|key| key.tick).collect()
                }
            };
            if ticks.is_empty() {
                return Err(ClipError::EmptyChannel {
                    target: channel.target().key().to_owned(),
                });
            }
            for pair in ticks.windows(2) {
                if pair[0] >= pair[1] {
                    return Err(ClipError::KeyTicks {
                        target: channel.target().key().to_owned(),
                        first: pair[0],
                        second: pair[1],
                    });
                }
            }
            for tick in &ticks {
                if *tick > duration_ticks {
                    return Err(ClipError::BeyondDuration {
                        tick: *tick,
                        duration: duration_ticks,
                    });
                }
            }
        }
        let mut marker_keys = HashSet::new();
        for (index, marker) in markers.iter().enumerate() {
            if marker.tick > duration_ticks {
                return Err(ClipError::BeyondDuration {
                    tick: marker.tick,
                    duration: duration_ticks,
                });
            }
            if index > 0 && markers[index - 1].tick > marker.tick {
                return Err(ClipError::MarkerTicks { tick: marker.tick });
            }
            if marker.key.is_empty() {
                return Err(ClipError::EmptyMarkerKey);
            }
            if !marker_keys.insert(marker.key.as_str()) {
                return Err(ClipError::DuplicateMarkerKey {
                    key: marker.key.clone(),
                });
            }
            if let Resolved::Known(effect) = &marker.effect
                && effect.value.cue().is_empty()
            {
                return Err(ClipError::EmptyMarkerCue {
                    key: marker.key.clone(),
                });
            }
        }
        Ok(Self {
            id,
            origin,
            duration_ticks,
            loop_mode,
            channels,
            markers,
            provenance,
        })
    }

    /// The clip's `animation_track` id.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// Where this clip's bytes came from — installation, synthetic fixture
    /// or engine design.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The clip length in ticks.
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
    pub fn channels(&self) -> &[AnimationChannel] {
        &self.channels
    }

    /// The markers, sorted by tick.
    #[must_use]
    pub fn markers(&self) -> &[EventMarker] {
        &self.markers
    }

    /// The provenance of the clip record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why a declared clip was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipError {
    /// The id is not in the `animation_track` namespace.
    NotAnAnimationTrack {
        /// The kind it actually names.
        kind: ContentKind,
    },
    /// A clip with zero duration can never reach a key or a marker.
    ZeroDuration,
    /// A channel carries no keys; it would drive nothing.
    EmptyChannel {
        /// The channel's target node key.
        target: String,
    },
    /// Channel keys were not in strictly increasing tick order.
    KeyTicks {
        /// The channel's target node key.
        target: String,
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
        /// The channel's target node key.
        target: String,
        /// The kind the id actually names.
        material: ContentKind,
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
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnAnimationTrack { kind } => {
                write!(f, "clip id names a {kind}, not an animation_track")
            }
            Self::ZeroDuration => write!(f, "a clip must have a nonzero duration"),
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
        }
    }
}

impl std::error::Error for ClipError {}

// ----------------------------------------------------------- fixture ------

/// The tick the synthetic door opens at (mirrors
/// `cs_sim::animated_object::SYNTHETIC_DOOR_OPEN_TICK`).
pub const SYNTHETIC_DOOR_OPEN_TICK: u64 = 10;
/// The duration of the synthetic door clip.
pub const SYNTHETIC_DOOR_DURATION: u64 = 30;
/// The marker key of the door's gameplay cue.
pub const SYNTHETIC_DOOR_MARKER: &str = "door_opened";

/// The minimal synthetic fixture in declared form: a hangar door that opens
/// at [`SYNTHETIC_DOOR_OPEN_TICK`] of a 30-tick one-shot clip, with a
/// gameplay marker at the same tick.
///
/// Every identity lives under the `synthetic` key and the record carries
/// [`Origin::SyntheticFixture`] plus designed provenance — it can never be
/// mistaken for retail content and cannot stand in for it.
#[must_use]
pub fn declared_synthetic_door_clip() -> AnimationClip {
    let door = SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, "synthetic.hangar.door")
            .expect("fixture id is valid"),
    )
    .expect("fixture id names a scene node");
    let open = TransformSample::try_new(
        Quaternion::from_axis_angle(
            cs_types::space::UnitVec3::UP,
            cs_types::space::Radians(std::f64::consts::FRAC_PI_2),
        )
        .expect("a quarter turn is unit length"),
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
    )
    .expect("the fixture pose is finite");
    let designed =
        || Provenance::designed(ClaimId::new("f20a.synthetic-door").expect("claim id is valid"));
    AnimationClip::try_new(
        ContentId::from_source(ContentKind::AnimationTrack, "synthetic.door_open")
            .expect("fixture id is valid"),
        Origin::SyntheticFixture,
        SYNTHETIC_DOOR_DURATION,
        LoopMode::Once,
        vec![AnimationChannel::Transform(TransformChannel {
            target: door,
            interpolation: Interpolation::Step,
            keys: vec![
                TransformKey {
                    tick: 0,
                    pose: TransformSample::IDENTITY,
                },
                TransformKey {
                    tick: SYNTHETIC_DOOR_OPEN_TICK,
                    pose: open,
                },
            ],
        })],
        vec![EventMarker {
            tick: SYNTHETIC_DOOR_OPEN_TICK,
            key: SYNTHETIC_DOOR_MARKER.to_owned(),
            effect: Resolved::Known(cs_types::content::Known::new(
                MarkerEffect::Gameplay {
                    cue: "synthetic.hangar.door_opened".to_owned(),
                },
                designed(),
            )),
        }],
        designed(),
    )
    .expect("the declared synthetic door fixture is valid")
}

// --------------------------------------------------- propeller fixture ----

/// The rotor node the synthetic propeller drives (`scene_node` kind).
pub const SYNTHETIC_PROPELLER_NODE: &str = "synthetic.plane.prop";
/// The duration of the synthetic propeller clip, in ticks.
pub const SYNTHETIC_PROPELLER_DURATION: u64 = 4;
/// The clip tick of the presentation marker (fires once per loop pass).
pub const SYNTHETIC_PROPELLER_PRESENTATION_TICK: u64 = 0;
/// The clip tick of the one-shot gameplay marker (fires once per
/// activation, whichever pass reaches it).
pub const SYNTHETIC_PROPELLER_GAMEPLAY_TICK: u64 = 1;
/// The stable key of the presentation marker.
pub const SYNTHETIC_PROPELLER_PRESENTATION_MARKER: &str = "blade_pass";
/// The stable key of the one-shot gameplay marker.
pub const SYNTHETIC_PROPELLER_GAMEPLAY_MARKER: &str = "engine_started";

/// The declared twin of `cs_sim::animated_object::synthetic_propeller_clip`:
/// a looping transform track over the rotor node, one presentation marker
/// and one one-shot gameplay marker.
///
/// This is the fixture F20-B's minimum acceptance scenario (AC02) drives
/// through the production path — declared record →
/// `cs_app::animation::lower::lower_clip` → playback — so the scenario is
/// exercised from content form rather than from a runtime-only clip. Lowering
/// it must produce exactly the runtime fixture the evaluator was tested
/// against; the acceptance test asserts that equality.
///
/// Newly authored development content under [`Origin::SyntheticFixture`];
/// every id lives under the `synthetic` key so it can never be mistaken for
/// retail content.
#[must_use]
pub fn declared_synthetic_propeller_clip() -> AnimationClip {
    let rotor = SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, SYNTHETIC_PROPELLER_NODE)
            .expect("fixture id is valid"),
    )
    .expect("fixture id names a scene node");
    let turn = |quarter: u8| {
        TransformSample::try_new(
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
    // The claim id and every value match the F20-A runtime fixture: they
    // describe the same designed clip, so lowering must be lossless.
    let designed = || {
        Provenance::designed(ClaimId::new("f20a.synthetic-propeller").expect("claim id is valid"))
    };
    AnimationClip::try_new(
        ContentId::from_source(ContentKind::AnimationTrack, "synthetic.propeller")
            .expect("fixture id is valid"),
        Origin::SyntheticFixture,
        SYNTHETIC_PROPELLER_DURATION,
        LoopMode::Loop,
        vec![AnimationChannel::Transform(TransformChannel {
            target: rotor,
            interpolation: Interpolation::Step,
            keys: (0..SYNTHETIC_PROPELLER_DURATION)
                .map(|tick| TransformKey {
                    tick,
                    pose: turn(tick as u8),
                })
                .collect(),
        })],
        vec![
            EventMarker {
                tick: SYNTHETIC_PROPELLER_PRESENTATION_TICK,
                key: SYNTHETIC_PROPELLER_PRESENTATION_MARKER.to_owned(),
                effect: Resolved::Known(Known::new(
                    MarkerEffect::Presentation {
                        cue: "synthetic.plane.blade_pass".to_owned(),
                    },
                    designed(),
                )),
            },
            EventMarker {
                tick: SYNTHETIC_PROPELLER_GAMEPLAY_TICK,
                key: SYNTHETIC_PROPELLER_GAMEPLAY_MARKER.to_owned(),
                effect: Resolved::Known(Known::new(
                    MarkerEffect::Gameplay {
                        cue: "synthetic.plane.engine_started".to_owned(),
                    },
                    designed(),
                )),
            },
        ],
        designed(),
    )
    .expect("the declared synthetic propeller fixture is valid")
}

// ----------------------------------------------------- cargo fixture ------

/// The cargo node the synthetic cargo clip drives (`scene_node` kind).
pub const SYNTHETIC_CARGO_NODE: &str = "synthetic.cargo";
/// The cargo bay node the cargo is attached to (`scene_node` kind).
pub const SYNTHETIC_CARGO_BAY_NODE: &str = "synthetic.cargo.bay";
/// The duration of the synthetic cargo clip, in ticks.
pub const SYNTHETIC_CARGO_DURATION: u64 = 10;
/// The clip tick the material track's first key lands at.
pub const SYNTHETIC_CARGO_MATERIAL_TICK: u64 = 0;
/// The clip tick the material track swaps the material at.
pub const SYNTHETIC_CARGO_SCORCH_TICK: u64 = 4;
/// The clip tick the attachment track attaches the cargo at.
pub const SYNTHETIC_CARGO_ATTACH_TICK: u64 = 0;
/// The clip tick the attachment track detaches the cargo at.
pub const SYNTHETIC_CARGO_DETACH_TICK: u64 = 6;
/// The key of the material the cargo starts with (`material` kind).
pub const SYNTHETIC_CARGO_MATERIAL: &str = "synthetic.cargo.canvas";
/// The key of the material the material track swaps in (`material` kind).
pub const SYNTHETIC_CARGO_SCORCHED_MATERIAL: &str = "synthetic.cargo.scorched";

/// The declared fixture that carries the two `Resolved` track kinds this
/// stage applies: a material track and an attachment track on one node.
///
/// The material track swaps a known `material` id at
/// [`SYNTHETIC_CARGO_SCORCH_TICK`]; the attachment track attaches the cargo
/// under the bay with [`PosePolicy::KeepLocalPose`] and later detaches it
/// with [`PosePolicy::KeepWorldPose`]. Both references are known — the
/// *unknown* variants the blocking rule covers are assembled by the
/// acceptance test from the same public record types, because an unknown is
/// a state of the evidence, not a fixture of the content pipeline.
///
/// Newly authored development content under [`Origin::SyntheticFixture`].
#[must_use]
pub fn declared_synthetic_cargo_clip() -> AnimationClip {
    let cargo = SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, SYNTHETIC_CARGO_NODE)
            .expect("fixture id is valid"),
    )
    .expect("fixture id names a scene node");
    let bay = SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, SYNTHETIC_CARGO_BAY_NODE)
            .expect("fixture id is valid"),
    )
    .expect("fixture id names a scene node");
    let material = |key: &str| {
        Resolved::Known(Known::new(
            ContentId::from_source(ContentKind::Material, key).expect("fixture id is valid"),
            Provenance::designed(ClaimId::new("f20b.synthetic-cargo").expect("claim id is valid")),
        ))
    };
    AnimationClip::try_new(
        ContentId::from_source(ContentKind::AnimationTrack, "synthetic.cargo")
            .expect("fixture id is valid"),
        Origin::SyntheticFixture,
        SYNTHETIC_CARGO_DURATION,
        LoopMode::Once,
        vec![
            AnimationChannel::Material(MaterialChannel {
                target: cargo.clone(),
                keys: vec![
                    MaterialKey {
                        tick: SYNTHETIC_CARGO_MATERIAL_TICK,
                        material: material(SYNTHETIC_CARGO_MATERIAL),
                    },
                    MaterialKey {
                        tick: SYNTHETIC_CARGO_SCORCH_TICK,
                        material: material(SYNTHETIC_CARGO_SCORCHED_MATERIAL),
                    },
                ],
            }),
            AnimationChannel::Attachment(AttachmentChannel {
                target: cargo,
                keys: vec![
                    AttachmentKey {
                        tick: SYNTHETIC_CARGO_ATTACH_TICK,
                        op: AttachmentOp::Attach {
                            parent: Box::new(Resolved::Known(Known::new(
                                bay,
                                Provenance::designed(
                                    ClaimId::new("f20b.synthetic-cargo")
                                        .expect("claim id is valid"),
                                ),
                            ))),
                            pose: PosePolicy::KeepLocalPose,
                        },
                    },
                    AttachmentKey {
                        tick: SYNTHETIC_CARGO_DETACH_TICK,
                        op: AttachmentOp::Detach {
                            pose: PosePolicy::KeepWorldPose,
                        },
                    },
                ],
            }),
        ],
        Vec::new(),
        Provenance::designed(ClaimId::new("f20b.synthetic-cargo").expect("claim id is valid")),
    )
    .expect("the declared synthetic cargo fixture is valid")
}

// -------------------------------------------------- breakable fixture -----

/// The node the synthetic breakable clip drives (`scene_node` kind).
pub const SYNTHETIC_BREAKABLE_NODE: &str = "synthetic.plane.hatch";
/// The duration of the synthetic breakable clip, in ticks.
pub const SYNTHETIC_BREAKABLE_DURATION: u64 = 6;
/// The clip tick the node is hidden from.
pub const SYNTHETIC_BREAKABLE_HIDDEN_TICK: u64 = 2;
/// The clip tick the node is shown again from.
pub const SYNTHETIC_BREAKABLE_SHOWN_TICK: u64 = 4;
/// The clip tick of the one-shot gameplay marker that announces the break.
pub const SYNTHETIC_BREAKABLE_BREAK_TICK: u64 = 2;
/// The stable key of that marker.
pub const SYNTHETIC_BREAKABLE_MARKER: &str = "hatch_broken";

/// The declared fixture that carries a **visibility channel**: a breakable
/// node that is hidden from [`SYNTHETIC_BREAKABLE_HIDDEN_TICK`] to
/// [`SYNTHETIC_BREAKABLE_SHOWN_TICK`] of a [`SYNTHETIC_BREAKABLE_DURATION`]-tick
/// clip, with one one-shot gameplay marker at the break tick.
///
/// The clip **loops**, so every pass re-shows the node: that is what makes it
/// the stress case for F20-C's destruction rule — a looping visibility track
/// tries to restore the node on every pass, and a destroyed node must still
/// not come back (F20 non-negotiable behavior 3). The marker is a gameplay
/// cue, so it fires once per activation however often the visibility cycles
/// (AC02).
///
/// It is the only declared fixture with a visibility channel, because the
/// three others exist to pin the transform, material and attachment paths; the
/// visibility path is driven through this one rather than by moving an
/// identity an earlier stage's test already pins.
///
/// Newly authored development content under [`Origin::SyntheticFixture`].
#[must_use]
pub fn declared_synthetic_breakable_clip() -> AnimationClip {
    let hatch = SceneNodeId::from_content_id(
        ContentId::from_source(ContentKind::SceneNode, SYNTHETIC_BREAKABLE_NODE)
            .expect("fixture id is valid"),
    )
    .expect("fixture id names a scene node");
    let designed = || {
        Provenance::designed(ClaimId::new("f20c.synthetic-breakable").expect("claim id is valid"))
    };
    AnimationClip::try_new(
        ContentId::from_source(ContentKind::AnimationTrack, "synthetic.breakable")
            .expect("fixture id is valid"),
        Origin::SyntheticFixture,
        SYNTHETIC_BREAKABLE_DURATION,
        LoopMode::Loop,
        vec![AnimationChannel::Visibility(VisibilityChannel {
            target: hatch,
            keys: vec![
                VisibilityKey {
                    tick: 0,
                    visibility: NodeVisibility::Visible,
                },
                VisibilityKey {
                    tick: SYNTHETIC_BREAKABLE_HIDDEN_TICK,
                    visibility: NodeVisibility::Hidden,
                },
                VisibilityKey {
                    tick: SYNTHETIC_BREAKABLE_SHOWN_TICK,
                    visibility: NodeVisibility::Visible,
                },
            ],
        })],
        vec![EventMarker {
            tick: SYNTHETIC_BREAKABLE_BREAK_TICK,
            key: SYNTHETIC_BREAKABLE_MARKER.to_owned(),
            effect: Resolved::Known(Known::new(
                MarkerEffect::Gameplay {
                    cue: "synthetic.plane.hatch_broken".to_owned(),
                },
                designed(),
            )),
        }],
        designed(),
    )
    .expect("the declared synthetic breakable fixture is valid")
}
