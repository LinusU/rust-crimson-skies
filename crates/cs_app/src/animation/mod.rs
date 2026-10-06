//! The animation application boundary (F20-A, F20-B).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module sits between the declared clip record
//! ([`cs_content::animation`]) and the fixed-tick evaluator
//! ([`cs_sim::animated_object`]), which cannot see each other — `cs_sim`
//! must not depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower::lower_clip`] — the conversion boundary: a validated declared
//!   clip becomes a runtime [`cs_sim::animated_object::AnimatedClip`], with
//!   every [`Resolved::Unknown`] carried through so the runtime still blocks
//!   the transitions it gates (F20 non-negotiable behavior 2);
//! * [`presentation::interpolated_pose`] — the render-side pose sampler.
//!   It runs on fractional alpha between two committed tick poses and
//!   produces *no* events: interpolation changes presentation only, gameplay
//!   markers stay inside the fixed-tick evaluator (non-negotiable behavior
//!   1);
//! * [`AnimatedNodeBinding`] — the ECS binding record tying an entity to one
//!   animated node of one playing clip, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a stale
//!   binding looking live.
//!
//! Stage `### F20-B` adds [`playback`], the fixed-tick playback that drives
//! both ends of that boundary inside a session: [`playback::play_animation`]
//! starts one lowered instance of a declared clip in the
//! [`playback::AnimationPlayback`] resource,
//! [`playback::advance_animation`] advances every instance once per
//! committed session tick, publishes its markers into the
//! [`playback::AnimationLog`] and applies the transform, material and
//! attachment tracks to the entities whose [`AnimatedNodeBinding`] verifies
//! (playing clip, live scene generation, driven node), and
//! [`playback::stop_animation`] ends an instance. The applied values are the
//! components [`playback::NodeAnimatedPose`],
//! [`playback::NodeAnimatedMaterial`] and
//! [`playback::NodeAnimatedAttachment`]; an unknown reference never becomes
//! a component — it is blocked and reported instead (F20 non-negotiable
//! behavior 2).
//!
//! The playback owns the animation state it applies; the records in this
//! module remain the ECS outputs the consumers bind.
//!
//! Stage `### F20-C` adds [`attachment`], the consumer half: the record
//! [`playback::NodeAnimatedAttachment`] becomes a real parent change —
//! [`attachment::apply_attachment_transitions`] inserts or removes `ChildOf`
//! with the authored pose policy, recomposes the world pose of the node's
//! descendants and gives a detached node the world velocity its parent had
//! at that tick, exactly once per change; and
//! [`attachment::release_attachments_before_despawn`] releases an animated
//! attachment before its parent goes away (non-negotiable behavior 4,
//! AC03).
//!
//! Stage `### F20-C.02` adds [`schedule`], the **producer wiring**: the
//! committed session tick arrives as the
//! [`schedule::CommittedSessionTick`] resource the session driver writes, and
//! [`schedule::advance_animation_on_session_tick`] (installed by
//! [`schedule::AnimationSchedulePlugin`], in `FixedPostUpdate` after the
//! physics step) advances the playback **once per committed tick change** —
//! nothing at all without that stamp, and nothing for a repeated one.
//! [`schedule::release_superseded_instances`] is the teardown half: a scene
//! load that superseded an instance's generation releases what that instance
//! applied, for its own entities only.
//!
//! Stage `### F20-C.03` adds [`visibility`], the **consumer half of the
//! visibility channel** and the ownership decision F20-B refused to guess:
//! [`visibility::NodeAnimatedVisibility`] is the clip's evaluated visibility
//! applied through the same verified-binding path as the other three channels,
//! and [`visibility::VisibilityVerdict`] is the single **composed** verdict
//! that [`visibility::composed_visibility_verdict`] reads out of that record,
//! the node's own `NodeDisabled` marker and F11-C's `NodePresentation` — damage
//! outranks LOD, LOD's own cull outranks the clip's reason, and a hidden node
//! carries no collider whatever the draw verdict says. The composition is
//! computed on read and nothing writes `NodePresentation` or `NodeDisabled`, so
//! neither the LOD pass nor an animation pass can silently lose the other's
//! decision, and a destroyed node stays destroyed in the frame its marker
//! appears (F20 non-negotiable behavior 3).
//!
//! The F20-C **integration** step adds the two producers the stage was missing:
//! [`binding::bind_animated_node`] is the spawn-side entry that writes the
//! generation-stamped [`AnimatedNodeBinding`] and starts the
//! `(track, instance)` it names, and [`schedule::AnimationPlugin`] is the
//! one-stop production composition — [`schedule::commit_session_tick`] copies
//! the F23-A physics ledger's committed tick into
//! [`schedule::CommittedSessionTick`] and installs the fixed-tick advance, so
//! a real [`PhysicsSession`](crate::physics::PhysicsSession) that adds the
//! plugin through its `configure` seam drives the whole path. The mission
//! marker consumer is still absent (the mission/objective layers are F37/F39);
//! the [`AnimationLog`](playback::AnimationLog) drain is the seam it will bind
//! to, recorded in
//! `docs/findings/2026-10-02-f20-c-wired-session-integration.md`.
//!
//! Stage `### F20-D` adds the two measured halves of the original-family
//! validation:
//!
//! * [`survey`] — [`survey::survey_animation_families`] walks the original
//!   installation and validates every carrier of the two mission-critical
//!   animation families (`mis_anim.zbd` per launchable scope, `cam_anim.zbd`
//!   per world group): the production two-key dispatch on the carrier's own
//!   header, the manifest fingerprint, and the paired `*.zrd` record plus
//!   every other `*anim*` member of the sibling reader archive. Missing
//!   carriers, refused headers and absent members are per-scope
//!   [`survey::CarrierBlocker`]s — a row is never dropped. The container
//!   payloads stay **undecoded**: the survey validates and fingerprints
//!   them, it does not interpret them;
//!
//! Task #632 (`M01-LC-WORLD-ACTORS`) adds [`programs`], the **member→actor
//! binding** the validation stage above could only validate around: the reader
//! archives' `.zrd` members all decoded as documents and none had a consumer.
//! [`programs::read_startup_animations`] reads a mission's `startanims.zrd`
//! startup event table, [`programs::read_animation_definition_member`] reads an
//! `ANIMATION_DEFINITIONS` member's definitions, and
//! [`programs::WorldActorProgramBinding`] joins the two across a mission's own
//! archive, its world group and the shared root — the archive, the member, the
//! byte span and the object selectors a startup animation resolves to, with
//! [`programs::WorldNodeNames`] resolving those names against a GameZ
//! container's records. It resolves the **binding** and names every field family
//! it did not interpret in [`programs::UnmeasuredFieldFamily`]; it does not
//! produce a `cs_content::world_actors::DeclaredWorldActorProgram`, because the
//! records state no motion, socket, pickup or tick-rate field.
//!
//! * [`capture`] — [`capture::capture_animated_pose`] draws a production
//!   [`RenderMesh`](cs_content::mesh::RenderMesh) at an evaluated
//!   [`PoseSample`](cs_sim::animated_object::PoseSample) on the real GPU and
//!   writes a measured PNG, which is the `gpu` half of the stage: the
//!   evidence that an animation output actually reaches a rendered frame.
//!
//! Task #633 (`M01-LC-ANIM-CARRIERS`) adds [`carrier`], the half of the same
//! question that is about **contents**: [`carrier::survey_animation_bindings`]
//! reads each carrier's own front index through
//! [`cs_formats::zbd::anim::read_animation_index`] — the family has no trailer
//! — reads the scope's paired `mis_anim.zrd`/`cam_anim.zrd` and its
//! `startanims.zrd` through the production `.zrd` reader, and joins the
//! `ANIMATION_DEFINITION_FILE` references to the carrier's member rows by exact
//! path. Every member and every reference gets a disposition; the animation
//! records inside the payload stay undecoded, and the startup identities that
//! would need them are listed as the open input they are.
//!
//! Task #678 (`M01-LC-ACTOR-ANIM-PLAYBACK`) adds [`mission`], the **consumer**
//! the two halves above were measured for: it joins one mission scope's
//! `startanims.zrd` identities to both the `.zrd` member that declares them and
//! the carrier record that stores them, checks the two name agreements that
//! make those halves one animation (the record's `object_name` is one of the
//! declaration's selectors, and the declaration's sequence names are the
//! record's ordinary sequence block names in order), resolves every name the
//! animation addresses against the group's world container, carries the
//! `ON_STARTUP` placements of the mission's own archive as its world actors,
//! and decides per animation — with a source locator — whether it can be
//! played.
//!
//! Task #690 (`F20-EVENT-GRAMMAR`) adds [`events`], the grammar those records'
//! sequence blocks were missing: an eight-byte tag/length header per event, an
//! opcode table whose statement spellings come from the installation's own
//! declarations, and two measured timing fields. With it, [`mission`] reports a
//! record **playable** with its duration and its per-tick pose report, and
//! keeps a refusal — with its byte offset and its claim — for the four opcodes
//! no declaration joins and for the one `RUN_TIME` position nobody
//! value-matched.

use std::fmt;

use bevy::ecs::component::Component;
use cs_types::content::ContentId;

use crate::scene::SceneGeneration;

pub mod attachment;
pub mod binding;
pub mod capture;
pub mod carrier;
pub mod events;
pub mod lower;
pub mod mission;
pub mod playback;
pub mod presentation;
pub mod programs;
pub mod schedule;
pub mod survey;
pub mod visibility;

pub use attachment::{
    AppliedAttachment, AttachmentRecord, AttachmentRefusalReason, RefusedAttachment,
    VelocitySkipReason, apply_attachment_transitions, release_animated_attachment,
    release_attachments_before_despawn,
};
pub use binding::{AnimatedNodeBindError, bind_animated_node};
pub use capture::{
    POSE_CAPTURE_HEIGHT, POSE_CAPTURE_WIDTH, PoseCapture, PoseCaptureError, PoseCaptureRequest,
    capture_animated_pose,
};
pub use carrier::{
    ANIMATION_DEFINITION_FILE_KEY, ANIMATION_DEFINITIONS_KEY, ANIMATION_LIST_KEY,
    ANIMATION_PATH_KEY, AnimationBindingError, AnimationBindingSurvey, AnimationDocument,
    AnimationReference, BindingBlocker, CarrierBinding, CarrierMember, GRAVITY_KEY,
    PATH_COMPONENT_SEPARATOR, PATH_ROOT_SEPARATOR, PayloadFacts, RecordFacts, STARTUP_MEMBER,
    ScopeStartupBinding, SiblingReader, StartupBinding, StartupGroup, StartupIdentities,
    StartupOutcome, UNBOUND_REASON_AMBIGUOUS, UNBOUND_REASON_NO_RECORD, UNBOUND_REASON_NOT_WALKED,
    UNRESOLVED_REASON_NO_MEMBER, UNRESOLVED_REASON_NO_RECORD_NAMES, bind_animation_carrier,
    bind_installation, bind_startup_identities, carrier_name, document_member,
    survey_animation_bindings,
};
pub use events::{
    DecodedEvent, EVENT_GRAMMAR_CLAIM, EVENT_HEADER_BYTES, EVENT_STREAM_NOT_DECODED_CLAIM,
    EventClass, EventStreamError, OPCODE_NOT_MEASURED_CLAIM, RUN_TIME_NOT_MEASURED_CLAIM,
    STORED_OPCODES, decode_event_stream, opcode_info, sequence_duration, walk_event_stream,
};
pub use mission::{
    AMBIGUOUS_DECLARATION_REASON, AnimationRecordFacts, AnimationTarget, CarrierFact,
    DECLARATION_MATCH_CLAIM, EVENTS_NOT_DECODED_CLAIM, EVENTS_NOT_DECODED_REASON,
    MissionAnimationBinding, MissionAnimationError, MissionAnimationRun,
    OBJECT_NAME_DISAGREES_REASON, PLACEMENT_FIELDS_CLAIM, PLACEMENT_FIELDS_REASON, PlayRefusal,
    RecordResolution, RecordSequence, SEQUENCE_NAMES_DISAGREE_REASON, StartupAnimation,
    TargetResolution, TargetSource, UNDECLARED_REASON, UNREADABLE_TARGET_REASON,
    WorldActorPlacement, bind_mission_animation, join_startup_animation,
};
pub use playback::{
    AnimationLog, AnimationPlayError, AnimationPlayback, AnimationRefusal, BlockedTrack,
    InstanceKey, NodeAnimatedAttachment, NodeAnimatedMaterial, NodeAnimatedPose, TrackKind,
    advance_animation, play_animation, stop_animation,
};
pub use programs::{
    ACTIVATION_FIELD, ACTIVATION_PREREQUISITE_FIELD, ACTIVATION_VOCABULARY_CLAIM,
    ANIMATION_DEFINITION_FIELD, ANIMATION_DEFINITION_FILE_FIELD, ANIMATION_DEFINITIONS_RECORD,
    ANIMATION_LIST_FIELD, ANIMATION_NAME_FIELD, Activation, ActivationPrerequisite,
    AnimationDefinitionMember, AnimationDefinitionSite, AnimationProgramError, AnimationSequence,
    BindingResolution, DeclaredAnimationDefinition, DefinitionObjects, LOAD_GAME_START,
    MINIMUM_TO_SATISFY, MeasuredActivation, NAME_ALTERNATE_FIELD, NAME_FIELD, NEW_GAME_START,
    NODE_PATH_SEPARATOR, OBJECT_SELECTOR_CLAIM, OPTIONS_PREREQUISITE, ObjectSelector,
    PrerequisiteCondition, REQUIRED_PREREQUISITE, SEQUENCE_FIELD, SEQUENCE_KINDS_CLAIM,
    SEQUENCE_NAME_FIELD, SelectorMatch, SelectorSegment, StartupAnimationBinding,
    StartupAnimationTable, StartupEvent, StateBinding, UnmeasuredFieldFamily, WILDCARD_CHAR,
    WorldActorProgramBinding, WorldNodeNames, read_animation_definition_member,
    read_startup_animations,
};
pub use schedule::{
    AnimationPlugin, AnimationSchedulePlugin, CommittedSessionTick,
    advance_animation_on_session_tick, commit_session_tick, release_superseded_instances,
};
pub use survey::{
    AnimationFamilySurvey, AnimationSurveyError, CarrierBlocker, CarrierKind, CarrierRecord,
    MemberRecord, PayloadFamily, SurveyedScope, UnpairedMember, survey_animation_families,
};
pub use visibility::{
    ColliderVerdict, DrawVerdict, NodeAnimatedVisibility, VisibilityVerdict,
    composed_visibility_verdict,
};

/// The identity of one live instance of an `animation_track`.
///
/// Several entities may play **one** track as separate instances — two
/// aircraft each spin a propeller hub with the same authored clip — so the
/// track id alone does not name a playback instance, and neither does an
/// entity id (the spawn wiring, not the playback, owns entity identity). The
/// spawn wiring assigns one instance per animated node it spawns and every
/// [`AnimatedNodeBinding`] it writes names that instance, so
/// [`AnimationPlayback`](playback::AnimationPlayback) can key its live map by
/// (track, instance) and give each instance its own evaluator, its own applied
/// state and its own event ids.
///
/// A validated nonzero number, like the `SessionId`/`PeerId` of
/// `docs/contracts/IDENTITY-CONTENT.md`: zero never names a live instance, so a
/// default-constructed identity cannot alias one. **Designed** — no original
/// data carries an instance identity; see
/// `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationInstance(u32);

impl AnimationInstance {
    /// Wraps an assigned instance number; zero is refused.
    #[must_use]
    pub const fn new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The assigned number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AnimationInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "instance {}", self.0)
    }
}

/// Component: marks an entity as presenting one node of one playing clip.
///
/// `node` is the channel target's stable `scene_node` id, `clip` the
/// `animation_track` it is driven by and `instance` which live instance of
/// that track it belongs to; `generation` is the scene generation the binding
/// was spawned under, so a reload stamps new bindings and stale ones are
/// identified by mismatch rather than surviving pointers (F11/F20
/// session-generation ownership).
///
/// The instance is what keeps one track's instances apart: two entities bound
/// to the same track under different instances each receive their own
/// evaluated state, and a teardown of one instance leaves the other untouched.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct AnimatedNodeBinding {
    /// The playing clip (`animation_track` id).
    pub clip: ContentId,
    /// The animated node (`scene_node` id).
    pub node: ContentId,
    /// Which live instance of `clip` this entity presents.
    pub instance: AnimationInstance,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}
