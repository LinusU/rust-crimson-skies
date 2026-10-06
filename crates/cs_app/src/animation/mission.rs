//! The mission animation consumer: a mission's startup world actors and its
//! animation records, joined and refused (task #678,
//! `M01-LC-ACTOR-ANIM-PLAYBACK`).
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-D`, non-negotiable behavior 2 ("Animation unknowns must
//! retain source locator and block affected gameplay transitions. Do not assume
//! MechWarrior animation event semantics apply to CS.").
//! Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//! Findings: `docs/findings/2026-10-05-m01-lc-actor-anim-playback.md` (this
//! stage), `docs/findings/2026-10-04-m01-lc-world-actors.md` (the member →
//! actor binding) and `docs/findings/2026-10-05-m01-lc-anim-records.md` (the
//! record walk).
//!
//! # The gap this closes
//!
//! Three earlier stages measured the halves of a mission's animation data and
//! stopped at each half's edge:
//!
//! * [`super::programs`] reads a mission's `startanims.zrd` startup event table
//!   and resolves each animation name to the `ANIMATION_DEFINITIONS` member that
//!   declares it, with the member's byte span and the object selectors it drives;
//! * [`super::carrier`] binds a startup identity to **one** record of the
//!   scope's mission carrier (`mis_anim.zbd`) or of its world group's camera
//!   carrier (`cam_anim.zbd`);
//! * [`cs_formats::zbd`] walks those records and names every field inside one,
//!   the tables it references (objects, nodes, lights, sounds, animation
//!   references) and its sequence blocks with their raw event bytes.
//!
//! Nothing read any of it **for a mission**: no production consumer joined a
//! startup animation to the record that stores it, none asked which world nodes
//! that record addresses, and none decided — per animation, with a source
//! locator — whether it can be played. This module is that consumer, and
//! [`bind_mission_animation`] is its entry point.
//!
//! # The join, and the two name agreements it checks
//!
//! Every startup identity is joined across three independently measured halves:
//! the declaration ([`super::programs`]), the payload record ([`super::carrier`]
//! plus [`cs_formats::zbd`]) and the group's world container, whose record
//! names are what those names resolve against
//! ([`super::programs::WorldNodeNames`]).
//!
//! Two agreements make the pair a *pair* rather than two unrelated hits on the
//! same spelling, and both are measured over the installation, not assumed:
//!
//! * the record's `object_name` is one of the declaration's object selectors;
//! * the declaration's sequence names, in order, are the record's **ordinary**
//!   sequence block names, in order — a sequence the declaration leaves unnamed
//!   agrees with an empty stored name.
//!
//! Measured over M01's closure (`zbd/c1c/m01`, `zbd/c1c` and `zbd` against
//! `zbd/c1c/m01/mis_anim.zbd` and `zbd/c1c/cam_anim.zbd`): **280** declaration /
//! record pairs, **280** sequence agreements, **279** object agreements and
//! **280** `root_name == object_name` equalities. The single object
//! disagreement is `agyro_rotors`, declared by `zbd/zrdr.zbd::autogyro_bus.zrd`
//! over `agyrobus` and stored over `autogyro`. It is **reported**, never
//! repaired: choosing between two spellings the original stored is a guess, and
//! a consumer that silently repaired it would hide the only interesting record
//! in the closure.
//!
//! # What is played, and what still says why
//!
//! A record's sequence blocks were raw bytes until this module's companion
//! [`super::events`] measured their grammar: an eight-byte tag/length header
//! per event, an opcode vocabulary that joins the installation's own statement
//! spellings, and two timing fields (`START_TIME` at payload word `0`,
//! `RUN_TIME` at the last word where the opcode's position is value-matched).
//! A record whose blocks all walk and whose opcodes all join is **playable**:
//! [`StartupAnimation::playback`] carries its duration and its per-tick pose
//! report. Three refusals keep the honest answer for everything else — bytes
//! this consumer does not hold or a stream that is not the measured shape
//! ([`EVENTS_NOT_DECODED_CLAIM`]), an opcode no declaration joins
//! ([`super::events::OPCODE_NOT_MEASURED_CLAIM`]) and a `RUN_TIME` position
//! nobody value-matched ([`super::events::RUN_TIME_NOT_MEASURED_CLAIM`]) — and
//! each keeps its byte offset. F20 behavior 2 is what makes a refusal, not a
//! guess, the answer for those.
//!
//! # The world-actor half
//!
//! A mission's own archive also states which world actors exist before the
//! mission runs: the `ON_STARTUP` definitions of `placezeps.zrd` are M01's
//! placement statements for its three capital ships, and
//! [`MissionAnimationBinding::placements`] carries them with their object
//! selectors resolved against the same world container. Their *placement* stays
//! refused ([`PLACEMENT_FIELDS_CLAIM`]): the placement member's `node` /
//! `position` / `yaw` / `pitch` / `max_speed` / `max_accel` fields are F33-D's
//! undecoded carrier and the world's own unit is #436's open measurement, so no
//! actor is spawned from a guessed coordinate.

use std::fmt;
use std::path::Path;

use cs_assets::install::{self, Discovery};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_content::textures::WorldTextureLoad;
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discover_container;
use cs_formats::zbd::{
    AnimationRecord, AnimationRecordSequenceKind, AnimationRecordTableKind, AnimationRecords,
    ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::Provenance;
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::install::RelativePath;

use super::carrier::{
    STARTUP_MEMBER, SiblingReader, StartupBinding, StartupOutcome, UNBOUND_REASON_NOT_WALKED,
    bind_animation_carrier, bind_startup_identities, carrier_name,
};
use super::events::{
    DecodedEvent, EventClass, EventStreamError, OPCODE_NOT_MEASURED_CLAIM,
    RUN_TIME_NOT_MEASURED_CLAIM, opcode_info,
};
use super::programs::{
    ANIMATION_DEFINITIONS_RECORD, AnimationDefinitionSite, BindingResolution, ObjectSelector,
    SelectorMatch, StartupAnimationBinding, WorldActorProgramBinding, WorldNodeNames,
    read_animation_definition_member, read_startup_animations,
};
use super::survey::{CarrierKind, MISSION_CARRIER};
use crate::world::retail::read_world_containers;

/// The reader archive every mission-scoped directory carries.
const READER_ARCHIVE: &str = "zrdr.zbd";

/// The world's geometry container: the one that holds the records an animation
/// record's node table names.
const WORLD_CONTAINER: &str = "gamez.zbd";

/// The claim the two name agreements are recorded under.
///
/// **Measured** over M01's closure (280 pairs, 280 sequence agreements, 279
/// object agreements, 280 `root_name == object_name`), never assumed: a pair
/// that disagrees is refused with [`OBJECT_NAME_DISAGREES_REASON`] or
/// [`SEQUENCE_NAMES_DISAGREE_REASON`] rather than repaired.
pub const DECLARATION_MATCH_CLAIM: &str = "f20-anim.startup-record-matches-its-declaring-member";

/// The claim under which "an animation record cannot be played yet" is recorded.
///
/// One value, owned by [`super::events`]: this alias exists so the consumer's
/// public name and the decoder's name cannot drift apart.
pub const EVENTS_NOT_DECODED_CLAIM: &str = super::events::EVENT_STREAM_NOT_DECODED_CLAIM;

/// The claim under which "a placed world actor is not spawned from its
/// declaration yet" is recorded.
pub const PLACEMENT_FIELDS_CLAIM: &str = "f20-anim.placement-member-fields-undecoded";

/// Why a record this consumer holds no event bytes for is not played.
///
/// A record whose blocks **are** held is decoded by [`super::events`]; a
/// structural failure of that walk keeps its own reason (the walk's), and an
/// opcode no declaration joins keeps [`OPCODE_NOT_MEASURED_CLAIM`] instead.
pub const EVENTS_NOT_DECODED_REASON: &str = "the consumer holds no event bytes for this record's \
     sequence blocks (a synthetic or partial row), so nothing is decoded and no statement, tick or \
     pose is recoverable from it";

/// Why a record whose `object_name` is not one of its declaration's selectors is
/// refused instead of matched to the nearest spelling.
pub const OBJECT_NAME_DISAGREES_REASON: &str = "the record's object_name is not one of the declaring member's object selectors; both \
     spellings are kept and neither is rewritten to the other, because choosing between them \
     would be a guess about which one the original meant";

/// Why a record whose ordinary sequence block names differ from the declaring
/// member's sequence names is refused.
pub const SEQUENCE_NAMES_DISAGREE_REASON: &str = "the record's ordinary sequence block names, in order, differ from the declaring member's \
     sequence names, in order; both lists are reported side by side and never paired by \
     position, because a mismatch means the two halves are not established as one animation";

/// Why a startup animation no member declares is refused.
pub const UNDECLARED_REASON: &str = "no member of the mission archive, its world group or the shared root declares this \
     animation name; whether the original engine resolved it elsewhere is unmeasured";

/// Why a startup animation two members declare is refused.
pub const AMBIGUOUS_DECLARATION_REASON: &str = "more than one member of the mission closure declares this animation name, and no measured \
     rule picks one of the declaring sites";

/// Why a placed world actor is not spawned from its declaration.
pub const PLACEMENT_FIELDS_REASON: &str = "the placement member's node, position, yaw, pitch, max_speed and max_accel fields are an \
     undecoded carrier (F33-D) and the world's own unit is unmeasured (#436), so no spawn \
     position, heading or motion limit is read out of them";

/// Why a stored name that is not a selector is kept and reported.
pub const UNREADABLE_TARGET_REASON: &str = "the stored name is empty or spells a node path rather than one node name; it is kept as \
     stored and never replaced by a selector this consumer would have invented";

/// Why a mission scope's animation inputs could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionAnimationError {
    /// The installation could not be inventoried.
    Discovery(String),
    /// A reader archive or one of its members could not be read or decoded.
    Archive {
        /// The archive's logical key.
        container: String,
        /// Why.
        reason: String,
    },
    /// An animation carrier could not be read.
    Carrier {
        /// The carrier's logical key.
        container: String,
        /// Why.
        reason: String,
    },
    /// The scope's world container could not be read.
    World {
        /// The world group the container belongs to.
        group: String,
        /// Why.
        reason: String,
    },
    /// A claim id or source span could not be built.
    Provenance(String),
}

impl fmt::Display for MissionAnimationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => write!(f, "installation discovery failed: {reason}"),
            Self::Archive { container, reason } => write!(f, "{container}: {reason}"),
            Self::Carrier { container, reason } => write!(f, "{container}: {reason}"),
            Self::World { group, reason } => {
                write!(f, "world group {group} could not be read: {reason}")
            }
            Self::Provenance(reason) => write!(f, "provenance: {reason}"),
        }
    }
}

impl std::error::Error for MissionAnimationError {}

/// One sequence block of an animation record, as far as it is measured.
///
/// The block's name and its event-stream length are always measured; the
/// events themselves are decoded by [`super::events`] when this consumer holds
/// the block's bytes, and [`SequenceEvents::Absent`] says when it does not.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordSequence {
    /// Reset, damage or ordinary.
    pub kind: AnimationRecordSequenceKind,
    /// The block's name, verbatim. Empty when the original stored an empty
    /// name, which is what an unnamed `.zrd` sequence pairs with.
    pub name: String,
    /// The block's event stream in bytes — a length, never an interpretation.
    pub event_bytes: u64,
    /// The block's decoded events, or why they are not decoded.
    pub events: SequenceEvents,
}

/// What one sequence block's event bytes became.
#[derive(Clone, Debug, PartialEq)]
pub enum SequenceEvents {
    /// The consumer holds no bytes for this block (a synthetic or partial
    /// row). Refused under [`EVENTS_NOT_DECODED_CLAIM`], never played.
    Absent,
    /// The stream walked and every opcode joined a statement: these are the
    /// measured events, with their statement spellings and timing.
    Decoded(Vec<DecodedEvent>),
    /// The stream did not walk, or an opcode joins no statement. The refusal
    /// keeps its byte offset and its claim, so a reviewer can read exactly
    /// where the block stopped being measured.
    Refused {
        /// The decoder's own stable code.
        code: &'static str,
        /// Why it was refused, in the words of the finding.
        reason: &'static str,
        /// Byte offset inside the block's stream.
        offset: u64,
        /// The claim the refusal travels under.
        claim_id: &'static str,
        /// The opcode the refusal names, when it is an unjoined opcode.
        opcode: Option<u8>,
    },
}

impl SequenceEvents {
    /// The decoded events, when the block walked and every opcode is measured.
    #[must_use]
    pub const fn decoded(&self) -> Option<&Vec<DecodedEvent>> {
        match self {
            Self::Decoded(events) => Some(events),
            Self::Absent | Self::Refused { .. } => None,
        }
    }

    /// Refuses a stream error with its own code, reason, offset and claim.
    #[must_use]
    pub fn refused(error: &EventStreamError) -> Self {
        Self::Refused {
            code: error.code(),
            reason: error.reason(),
            offset: error.offset(),
            claim_id: error.claim_id(),
            opcode: error.opcode(),
        }
    }
}

impl RecordSequence {
    /// Reset, damage or ordinary.
    #[must_use]
    pub const fn kind(&self) -> AnimationRecordSequenceKind {
        self.kind
    }

    /// The block's name, verbatim. Empty when the original stored an empty
    /// name, which is what an unnamed `.zrd` sequence pairs with.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The block's event stream in bytes — a length, never an interpretation.
    #[must_use]
    pub const fn event_bytes(&self) -> u64 {
        self.event_bytes
    }

    /// The block's decoded events, or why they are not decoded.
    #[must_use]
    pub const fn events(&self) -> &SequenceEvents {
        &self.events
    }
}

/// Why a walked record still cannot report a playback.
///
/// Every value keeps a byte offset and a claim, so a refusal is readable
/// against the record's own source locator instead of being a boolean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaybackGap {
    /// The consumer holds no bytes for this record's blocks: nothing is
    /// decoded, so nothing plays.
    Absent,
    /// A block's bytes are not the measured event shape.
    NotDecoded {
        /// The decoder's own stable code.
        code: &'static str,
        /// Why it was refused, in the words of the finding.
        reason: &'static str,
        /// Byte offset inside the block's stream.
        offset: u64,
        /// The claim the refusal travels under.
        claim_id: &'static str,
    },
    /// A block carries an opcode no declaration of the installation joins to a
    /// statement: its class, timing and effect are unmeasured.
    OpcodeNotDecoded {
        /// The stored opcode.
        opcode: u8,
        /// [`super::events::OPCODE_NOT_MEASURED_REASON`], verbatim.
        reason: &'static str,
        /// Byte offset inside the block's stream.
        offset: u64,
        /// [`OPCODE_NOT_MEASURED_CLAIM`].
        claim_id: &'static str,
    },
    /// A block carries a statement whose `RUN_TIME` position has not been
    /// value-matched, so a duration for the record could only be guessed.
    TimingNotDecoded {
        /// The stored opcode.
        opcode: u8,
        /// [`super::events::RUN_TIME_NOT_MEASURED_REASON`], verbatim.
        reason: &'static str,
        /// Byte offset inside the block's stream.
        offset: u64,
        /// [`RUN_TIME_NOT_MEASURED_CLAIM`].
        claim_id: &'static str,
    },
}

impl PlaybackGap {
    /// The stable label a report groups by.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Absent => "events_absent",
            Self::NotDecoded { .. } => "event_stream_not_decoded",
            Self::OpcodeNotDecoded { .. } => "event_opcode_not_decoded",
            Self::TimingNotDecoded { .. } => "event_timing_not_decoded",
        }
    }

    /// Why the record is not played, in the words of the finding.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Absent => EVENTS_NOT_DECODED_REASON,
            Self::NotDecoded { reason, .. }
            | Self::OpcodeNotDecoded { reason, .. }
            | Self::TimingNotDecoded { reason, .. } => reason,
        }
    }

    /// The claim the refusal is recorded under.
    #[must_use]
    pub const fn claim_id(&self) -> &'static str {
        match self {
            Self::Absent | Self::NotDecoded { .. } => EVENTS_NOT_DECODED_CLAIM,
            Self::OpcodeNotDecoded { claim_id, .. } | Self::TimingNotDecoded { claim_id, .. } => {
                claim_id
            }
        }
    }

    /// The opcode the refusal names, when one is named.
    #[must_use]
    pub const fn opcode(&self) -> Option<u8> {
        match self {
            Self::OpcodeNotDecoded { opcode, .. } | Self::TimingNotDecoded { opcode, .. } => {
                Some(*opcode)
            }
            Self::Absent | Self::NotDecoded { .. } => None,
        }
    }

    /// The byte offset the refusal was found at, when the walk named one.
    #[must_use]
    pub const fn offset(&self) -> Option<u64> {
        match self {
            Self::Absent => None,
            Self::NotDecoded { offset, .. }
            | Self::OpcodeNotDecoded { offset, .. }
            | Self::TimingNotDecoded { offset, .. } => Some(*offset),
        }
    }

    /// Converts this gap into the consumer's refusal, keeping its reason, its
    /// claim and its source locator.
    #[must_use]
    pub fn refusal(&self) -> PlayRefusal {
        let claim_id = ClaimId::new(self.claim_id())
            .expect("a gap's claim id is a static, validated constant");
        let reason = self.reason();
        let offset = self.offset();
        match self {
            Self::OpcodeNotDecoded { opcode, .. } => PlayRefusal::EventOpcodeUndecoded {
                reason,
                claim_id,
                opcode: *opcode,
                offset: offset.unwrap_or_default(),
            },
            Self::TimingNotDecoded { opcode, .. } => PlayRefusal::EventTimingUndecoded {
                reason,
                claim_id,
                opcode: *opcode,
                offset: offset.unwrap_or_default(),
            },
            Self::Absent | Self::NotDecoded { .. } => PlayRefusal::EventsNotDecoded {
                reason,
                claim_id,
                offset,
            },
        }
    }

    /// Converts a decoder refusal into this consumer's gap.
    #[must_use]
    pub fn of(error: &EventStreamError) -> Self {
        match error.opcode() {
            Some(opcode) => match opcode_info(opcode).and_then(|info| info.timing_gap_claim()) {
                Some(RUN_TIME_NOT_MEASURED_CLAIM) => Self::TimingNotDecoded {
                    opcode,
                    reason: error.reason(),
                    offset: error.offset(),
                    claim_id: RUN_TIME_NOT_MEASURED_CLAIM,
                },
                _ => Self::OpcodeNotDecoded {
                    opcode,
                    reason: error.reason(),
                    offset: error.offset(),
                    claim_id: OPCODE_NOT_MEASURED_CLAIM,
                },
            },
            None => Self::NotDecoded {
                code: error.code(),
                reason: error.reason(),
                offset: error.offset(),
                claim_id: error.claim_id(),
            },
        }
    }
}

impl SequenceEvents {
    /// The gap that keeps this block's record from a playback, when there is
    /// one. `None` means the block decoded.
    #[must_use]
    pub fn gap(&self) -> Option<PlaybackGap> {
        match self {
            Self::Absent => Some(PlaybackGap::Absent),
            Self::Decoded(_) => None,
            Self::Refused {
                code,
                reason,
                offset,
                claim_id,
                opcode,
            } => Some(match opcode {
                Some(opcode) if *claim_id == RUN_TIME_NOT_MEASURED_CLAIM => {
                    PlaybackGap::TimingNotDecoded {
                        opcode: *opcode,
                        reason,
                        offset: *offset,
                        claim_id,
                    }
                }
                Some(opcode) => PlaybackGap::OpcodeNotDecoded {
                    opcode: *opcode,
                    reason,
                    offset: *offset,
                    claim_id,
                },
                None => PlaybackGap::NotDecoded {
                    code,
                    reason,
                    offset: *offset,
                    claim_id,
                },
            }),
        }
    }
}

/// One decoded sequence block of a playable record.
#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackSequence {
    /// Reset, damage or ordinary.
    pub kind: AnimationRecordSequenceKind,
    /// The block's name, verbatim.
    pub name: String,
    /// The block's decoded events, in stored order.
    pub events: Vec<DecodedEvent>,
}

impl PlaybackSequence {
    /// The block's largest `end_time`, in the original's stored time unit.
    #[must_use]
    pub fn duration_time(&self) -> f32 {
        self.events
            .iter()
            .map(DecodedEvent::end_time)
            .fold(0.0_f32, f32::max)
    }
}

/// A record's playback: its decoded blocks and the duration they measure.
///
/// The duration is the largest `end_time` over every decoded event of every
/// block, in the **original's stored time unit**. The unit is not established
/// (seconds is plausible, unverified), so this value is reported and never
/// multiplied into ticks by this module: [`Self::poses`] takes the caller's
/// tick rate as its own statement.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordPlayback {
    duration_time: f32,
    sequences: Vec<PlaybackSequence>,
}

impl RecordPlayback {
    /// Builds a playback from decoded blocks, measuring the duration.
    #[must_use]
    pub fn new(sequences: Vec<PlaybackSequence>) -> Self {
        let duration_time = sequences
            .iter()
            .map(PlaybackSequence::duration_time)
            .fold(0.0_f32, f32::max);
        Self {
            duration_time,
            sequences,
        }
    }

    /// The largest `end_time` over the record's decoded events, in the
    /// original's stored time unit. Measured from the payloads: `START_TIME`
    /// at word `0` plus `RUN_TIME` at the last word where the opcode's
    /// position is value-matched.
    #[must_use]
    pub const fn duration_time(&self) -> f32 {
        self.duration_time
    }

    /// The record's decoded blocks, in stored order.
    #[must_use]
    pub fn sequences(&self) -> &[PlaybackSequence] {
        &self.sequences
    }

    /// Every decoded event of every block, in stored order.
    pub fn events(&self) -> impl Iterator<Item = &DecodedEvent> {
        self.sequences.iter().flat_map(|sequence| &sequence.events)
    }

    /// The record's **per-tick pose report**: one row per tick of the caller's
    /// timeline, listing every statement the record has *started* by that tick
    /// — its measured configuration at that moment — each keeping its own
    /// `[start_time, end_time]` span.
    ///
    /// What this is, exactly: the event stream stores **statements** — an
    /// opcode, its authored target fields and its timing — and no keyframes
    /// (measured over all 56 994 retail blocks). So a row here is the record's
    /// measured activity at that tick, with the installation's own statement
    /// spelling, and **not** a transform: no `PoseSample` is produced from an
    /// event, because the stored unit (#436), the original's animation tick
    /// rate and the interpolation of a motion statement are all unmeasured.
    ///
    /// The caller states the tick rate: `ticks_per_second` is the caller's own
    /// timeline, not a measurement of the original's (see
    /// `f20-anim.tick-rate-unmeasured`). A rate of zero samples nothing and
    /// returns an empty report.
    #[must_use]
    pub fn poses(&self, ticks_per_second: u32) -> Vec<TickPose> {
        if ticks_per_second == 0 {
            return Vec::new();
        }
        let rate = ticks_per_second as f32;
        let last = (self.duration_time * rate).ceil();
        let last = if last.is_finite() && last >= 0.0 {
            last as u64
        } else {
            0
        };
        (0..=last)
            .map(|tick| {
                let time = tick as f32 / rate;
                let statements = self
                    .events()
                    .filter(|event| event.start_time() <= time)
                    .map(PoseStatement::of)
                    .collect();
                TickPose { tick, statements }
            })
            .collect()
    }
}

/// One statement a [`TickPose`] row holds.
#[derive(Clone, Debug, PartialEq)]
pub struct PoseStatement {
    /// The opcode byte.
    pub opcode: u8,
    /// The installation's own statement spelling.
    pub statement: &'static str,
    /// This project's class of that spelling.
    pub class: EventClass,
    /// When the statement starts, in the original's stored time unit.
    pub start_time: f32,
    /// When it ends, in the original's stored time unit.
    pub end_time: f32,
}

impl PoseStatement {
    /// Reads one decoded event into a pose row's statement.
    #[must_use]
    pub fn of(event: &DecodedEvent) -> Self {
        Self {
            opcode: event.opcode,
            statement: event.statement,
            class: event.class,
            start_time: event.start_time(),
            end_time: event.end_time(),
        }
    }
}

/// The record's decoded activity at one tick of the caller's timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct TickPose {
    /// The tick, counting from the record's start.
    pub tick: u64,
    /// Every statement the record had **started** by this tick, in stored
    /// order. A statement that has already ended stays in the row: the report
    /// is the record's accumulated configuration, not a filter on end times
    /// (see [`RecordPlayback::poses`]).
    pub statements: Vec<PoseStatement>,
}

impl TickPose {
    /// The tick, counting from the record's start.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// Every statement the record had **started** by this tick, in stored
    /// order; each one keeps its own `[start_time, end_time]` span, so a
    /// caller can tell a statement that is running from one that has ended.
    #[must_use]
    pub fn statements(&self) -> &[PoseStatement] {
        &self.statements
    }
}

/// One animation record of a carrier payload, owned and measured.
///
/// Every field here is read verbatim out of the original bytes. The flag word's
/// bits, the status / activation / priority bytes' meanings and the bodies of
/// the table entries after their names are **unmeasured**; the sequence blocks'
/// events are decoded by [`super::events`] and each block keeps whether that
/// happened ([`SequenceEvents`]), and the finding names what stays open.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationRecordFacts {
    /// Which carrier holds the record.
    pub carrier: CarrierKind,
    /// The carrier's logical key.
    pub carrier_key: String,
    /// The record's index in that carrier's record list.
    pub index: usize,
    /// The record's own bytes inside its carrier: the span a reviewer can read.
    pub span: SourceSpan,
    /// Where these facts come from, with the claim they are recorded under.
    pub provenance: Provenance,
    /// The record's identity, as `startanims.zrd` spells it.
    pub anim_name: String,
    /// The record's `object_name` field, verbatim.
    pub object_name: String,
    /// The record's `root_name` field, verbatim.
    ///
    /// Equal to [`Self::object_name`] in every one of the 280 pairs measured
    /// over M01's closure; kept as its own field because the equality is a
    /// measurement, not an identity.
    pub root_name: String,
    /// The record's flag word. Its bits are unmeasured.
    pub flags: u32,
    /// The record's status byte. Its meaning is unmeasured.
    pub status: u8,
    /// The record's activation byte. Its meaning is unmeasured.
    pub activation: u8,
    /// The record's execution priority. Its meaning is unmeasured.
    pub execution_priority: u8,
    /// The record's `reset_time` float, verbatim. Its unit is unmeasured.
    pub reset_time: f32,
    /// The record's `max_health` float, verbatim. Its unit is unmeasured.
    pub max_health: f32,
    /// The object-reference table's names, verbatim and in stored order.
    ///
    /// Measured: the first entry of every non-empty objects or nodes table is
    /// an empty name, so entry `0` is kept here rather than dropped.
    pub objects: Vec<String>,
    /// The node-reference table's names, verbatim and in stored order.
    pub nodes: Vec<String>,
    /// The animation-reference table's names: the animations this record
    /// calls. The `CALL_ANIMATION` statements that make those calls are decoded
    /// as far as their spelling and their timing ([`super::events`]), but no
    /// payload field is read out of an event, so no statement can yet be tied
    /// to one of these names.
    pub animation_refs: Vec<String>,
    /// The record's sequence blocks, in stored order: the reset and damage
    /// blocks first, then the ordinary ones.
    pub sequences: Vec<RecordSequence>,
}

impl AnimationRecordFacts {
    /// Which carrier holds the record.
    #[must_use]
    pub const fn carrier(&self) -> CarrierKind {
        self.carrier
    }

    /// The carrier's logical key.
    #[must_use]
    pub fn carrier_key(&self) -> &str {
        &self.carrier_key
    }

    /// The record's index in that carrier's record list.
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// The record's own bytes inside its carrier: the span a reviewer can read.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// Where these facts come from, with the claim they are recorded under.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The record's identity, as `startanims.zrd` spells it.
    #[must_use]
    pub fn anim_name(&self) -> &str {
        &self.anim_name
    }

    /// The record's `object_name` field, verbatim.
    #[must_use]
    pub fn object_name(&self) -> &str {
        &self.object_name
    }

    /// The record's `root_name` field, verbatim.
    ///
    /// Equal to [`Self::object_name`] in every one of the 280 pairs measured
    /// over M01's closure; kept as its own field because the equality is a
    /// measurement, not an identity.
    #[must_use]
    pub fn root_name(&self) -> &str {
        &self.root_name
    }

    /// The record's flag word. Its bits are unmeasured.
    #[must_use]
    pub const fn flags(&self) -> u32 {
        self.flags
    }

    /// The record's status byte. Its meaning is unmeasured.
    #[must_use]
    pub const fn status(&self) -> u8 {
        self.status
    }

    /// The record's activation byte. Its meaning is unmeasured.
    #[must_use]
    pub const fn activation(&self) -> u8 {
        self.activation
    }

    /// The record's execution priority. Its meaning is unmeasured.
    #[must_use]
    pub const fn execution_priority(&self) -> u8 {
        self.execution_priority
    }

    /// The record's `reset_time` float, verbatim. Its unit is unmeasured.
    #[must_use]
    pub const fn reset_time(&self) -> f32 {
        self.reset_time
    }

    /// The record's `max_health` float, verbatim. Its unit is unmeasured.
    #[must_use]
    pub const fn max_health(&self) -> f32 {
        self.max_health
    }

    /// The object-reference table's names, verbatim and in stored order.
    ///
    /// Measured: the first entry of every non-empty objects or nodes table is
    /// an empty name, so entry `0` is kept here rather than dropped.
    #[must_use]
    pub fn objects(&self) -> &[String] {
        &self.objects
    }

    /// The node-reference table's names, verbatim and in stored order.
    #[must_use]
    pub fn nodes(&self) -> &[String] {
        &self.nodes
    }

    /// The animation-reference table's names: the animations this record
    /// calls. The `CALL_ANIMATION` statements that make those calls are decoded
    /// as far as their spelling and their timing ([`super::events`]), but no
    /// payload field is read out of an event, so no statement can yet be tied
    /// to one of these names.
    #[must_use]
    pub fn animation_refs(&self) -> &[String] {
        &self.animation_refs
    }

    /// The record's sequence blocks, in stored order: the reset and damage
    /// blocks first, then the ordinary ones.
    #[must_use]
    pub fn sequences(&self) -> &[RecordSequence] {
        &self.sequences
    }

    /// The ordinary sequence blocks, in stored order — the ones the declaring
    /// member's `SEQUENCE_DEFINITION` list pairs with.
    pub fn ordinary_sequences(&self) -> impl Iterator<Item = &RecordSequence> {
        self.sequences
            .iter()
            .filter(|block| block.kind == AnimationRecordSequenceKind::Sequence)
    }

    /// The block of a given kind, when the record carries one.
    #[must_use]
    pub fn sequence(&self, kind: AnimationRecordSequenceKind) -> Option<&RecordSequence> {
        self.sequences.iter().find(|block| block.kind == kind)
    }

    /// The record's playback: its decoded blocks and the duration their
    /// measured timing gives, in the original's stored time unit.
    ///
    /// # Errors
    ///
    /// The [`PlaybackGap`] that keeps this record from a playback: bytes this
    /// consumer does not hold, a stream that is not the measured event shape,
    /// an opcode no declaration joins, or a `RUN_TIME` position that was never
    /// value-matched. **One** gap refuses the whole record: partial decoding
    /// never becomes a partial playback.
    pub fn playback(&self) -> Result<RecordPlayback, PlaybackGap> {
        let mut sequences = Vec::with_capacity(self.sequences.len());
        for block in &self.sequences {
            let SequenceEvents::Decoded(events) = &block.events else {
                return Err(block
                    .events
                    .gap()
                    .expect("a refused block always states its gap"));
            };
            // A decoded stream can still carry a statement whose timing was
            // never value-matched (opcode 5): that gap refuses the record
            // before any duration is read, never a shortened duration.
            for event in events {
                let info = opcode_info(event.opcode)
                    .expect("a decoded event's opcode is in the stored table");
                if let Some(claim_id) = info.timing_gap_claim() {
                    let reason = info
                        .timing_gap_reason()
                        .expect("a gap claim always travels with its reason");
                    let offset = event.offset;
                    let opcode = event.opcode;
                    return Err(if claim_id == RUN_TIME_NOT_MEASURED_CLAIM {
                        PlaybackGap::TimingNotDecoded {
                            opcode,
                            reason,
                            offset,
                            claim_id,
                        }
                    } else {
                        PlaybackGap::OpcodeNotDecoded {
                            opcode,
                            reason,
                            offset,
                            claim_id,
                        }
                    });
                }
            }
            sequences.push(PlaybackSequence {
                kind: block.kind,
                name: block.name.clone(),
                events: events.clone(),
            });
        }
        Ok(RecordPlayback::new(sequences))
    }
}

/// Where a startup identity's payload record was found.
#[derive(Clone, Debug, PartialEq)]
pub enum RecordResolution {
    /// Exactly one record of the two carriers carries this identity.
    Bound(Box<AnimationRecordFacts>),
    /// The identity is left open, with the reason and every match found.
    Unbound {
        /// One of the three `UNBOUND_REASON_*` texts of [`super::carrier`],
        /// verbatim.
        reason: &'static str,
        /// The matches that were found, if any.
        matches: Vec<(CarrierKind, usize)>,
    },
}

impl RecordResolution {
    /// The bound record, when there is one.
    #[must_use]
    pub fn bound(&self) -> Option<&AnimationRecordFacts> {
        match self {
            Self::Bound(record) => Some(record.as_ref()),
            Self::Unbound { .. } => None,
        }
    }

    /// Whether a record was found.
    #[must_use]
    pub const fn is_bound(&self) -> bool {
        matches!(self, Self::Bound(_))
    }
}

/// Which half of the join a world-node name came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetSource {
    /// An object selector of the declaring `.zrd` member.
    DeclaredSelector,
    /// The record's `object_name` field.
    RecordObject,
    /// The record's `root_name` field.
    RecordRoot,
    /// One entry of the record's node-reference table.
    RecordNode,
}

impl TargetSource {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DeclaredSelector => "declared_selector",
            Self::RecordObject => "record_object",
            Self::RecordRoot => "record_root",
            Self::RecordNode => "record_node",
        }
    }
}

/// What one stored name selected in the mission's world container.
#[derive(Clone, Debug, PartialEq)]
pub enum TargetResolution {
    /// The name is a selector and it selected these records.
    Selected(SelectorMatch),
    /// The stored name is not a selector, and is kept as it is stored.
    Unreadable {
        /// The name exactly as stored.
        stored: String,
        /// Why it is not a selector.
        reason: &'static str,
    },
}

impl TargetResolution {
    /// How many world records the name selected, when it selected any.
    #[must_use]
    pub const fn occurrences(&self) -> Option<usize> {
        match self {
            Self::Selected(outcome) => outcome.occurrences(),
            Self::Unreadable { .. } => None,
        }
    }

    /// Whether this name's reach is not measured: a node path, a narrowing
    /// wildcard, or a name that is not a selector at all.
    #[must_use]
    pub const fn is_unmeasured(&self) -> bool {
        match self {
            Self::Selected(outcome) => outcome.is_unmeasured(),
            Self::Unreadable { .. } => true,
        }
    }
}

/// One name a startup animation addresses, and what it selected in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationTarget {
    /// Which half of the join the name came from.
    pub source: TargetSource,
    /// The name exactly as stored.
    pub stored: String,
    /// What the name selected.
    pub resolution: TargetResolution,
}

impl AnimationTarget {
    /// Which half of the join the name came from.
    #[must_use]
    pub const fn source(&self) -> TargetSource {
        self.source
    }

    /// The name exactly as stored.
    #[must_use]
    pub fn stored(&self) -> &str {
        &self.stored
    }

    /// What the name selected.
    #[must_use]
    pub const fn resolution(&self) -> &TargetResolution {
        &self.resolution
    }

    /// Resolves one stored name against a world container's record names.
    ///
    /// `world` is `None` when the caller holds no container: the name is then
    /// kept as [`TargetResolution::Unreadable`] with
    /// [`UNREADABLE_TARGET_REASON`], never counted as selecting nothing.
    #[must_use]
    pub fn resolve(
        source: TargetSource,
        stored: impl Into<String>,
        world: Option<&WorldNodeNames>,
    ) -> Self {
        let stored = stored.into();
        let resolution = match ObjectSelector::parse(&stored) {
            Ok(selector) => match world {
                Some(names) => TargetResolution::Selected(names.resolve(&selector)),
                None => TargetResolution::Unreadable {
                    stored: stored.clone(),
                    reason: UNREADABLE_TARGET_REASON,
                },
            },
            Err(_) => TargetResolution::Unreadable {
                stored: stored.clone(),
                reason: UNREADABLE_TARGET_REASON,
            },
        };
        Self {
            source,
            stored,
            resolution,
        }
    }
}

/// Why one startup animation is not played.
///
/// The first six are disagreements or content nobody stored: the record's own
/// halves do not match, or no member declares the name. The three event
/// refusals are about the record's sequence blocks: this consumer holds no
/// bytes for them, the bytes are not the measured event shape, an opcode joins
/// no statement of the installation, or a statement's `RUN_TIME` position was
/// never value-matched. Each keeps its claim and its byte offset, so F20
/// behavior 2's source locator is never dropped.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayRefusal {
    /// No member of the closure declares this animation name.
    Undeclared {
        /// [`UNDECLARED_REASON`], verbatim.
        reason: &'static str,
    },
    /// More than one member declares it.
    AmbiguousDeclaration {
        /// [`AMBIGUOUS_DECLARATION_REASON`], verbatim.
        reason: &'static str,
        /// How many members declare it.
        sites: usize,
    },
    /// No record of either carrier carries this identity, several do, or a
    /// carrier was not walked.
    NoRecord {
        /// One of the three `UNBOUND_REASON_*` texts, verbatim.
        reason: &'static str,
        /// The matches that were found, when the reason is ambiguity.
        matches: Vec<(CarrierKind, usize)>,
    },
    /// The record's `object_name` is not one of the declaration's selectors.
    ObjectNameDisagrees {
        /// [`OBJECT_NAME_DISAGREES_REASON`], verbatim.
        reason: &'static str,
        /// The declaration's selectors, as stored.
        declared: Vec<String>,
        /// The record's stored object name.
        stored: String,
    },
    /// The declaration's sequence names and the record's ordinary sequence
    /// block names are not the same list.
    SequenceNamesDisagree {
        /// [`SEQUENCE_NAMES_DISAGREE_REASON`], verbatim.
        reason: &'static str,
        /// The declaration's sequence names, `None` where it leaves one unnamed.
        declared: Vec<Option<String>>,
        /// The record's ordinary sequence block names, verbatim.
        stored: Vec<String>,
    },
    /// The record is walked and both name agreements hold, but one of its
    /// blocks is not the measured event shape (or holds no bytes here).
    EventsNotDecoded {
        /// [`EVENTS_NOT_DECODED_REASON`] or the walk's own reason, verbatim.
        reason: &'static str,
        /// [`EVENTS_NOT_DECODED_CLAIM`].
        claim_id: ClaimId,
        /// The byte offset the refusal was found at, when the walk named one.
        offset: Option<u64>,
    },
    /// The record is walked, but one of its events carries an opcode no
    /// declaration of the installation joins to a statement.
    EventOpcodeUndecoded {
        /// [`super::events::OPCODE_NOT_MEASURED_REASON`], verbatim.
        reason: &'static str,
        /// [`OPCODE_NOT_MEASURED_CLAIM`].
        claim_id: ClaimId,
        /// The stored opcode that was refused.
        opcode: u8,
        /// The byte offset the refusal was found at inside the block.
        offset: u64,
    },
    /// The record is walked and every opcode joins a statement, but one of
    /// them states a `RUN_TIME` whose payload position has not been
    /// value-matched, so a duration could only be guessed.
    EventTimingUndecoded {
        /// [`super::events::RUN_TIME_NOT_MEASURED_REASON`], verbatim.
        reason: &'static str,
        /// [`RUN_TIME_NOT_MEASURED_CLAIM`].
        claim_id: ClaimId,
        /// The stored opcode whose timing is unmeasured.
        opcode: u8,
        /// The byte offset the refusal was found at inside the block.
        offset: u64,
    },
}

impl PlayRefusal {
    /// The stable label a report groups by.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Undeclared { .. } => "undeclared",
            Self::AmbiguousDeclaration { .. } => "ambiguous_declaration",
            Self::NoRecord { .. } => "no_record",
            Self::ObjectNameDisagrees { .. } => "object_name_disagrees",
            Self::SequenceNamesDisagree { .. } => "sequence_names_disagree",
            Self::EventsNotDecoded { .. } => "events_not_decoded",
            Self::EventOpcodeUndecoded { .. } => "event_opcode_undecoded",
            Self::EventTimingUndecoded { .. } => "event_timing_undecoded",
        }
    }

    /// Why the animation was refused, in the words of the finding.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Undeclared { reason }
            | Self::AmbiguousDeclaration { reason, .. }
            | Self::NoRecord { reason, .. }
            | Self::ObjectNameDisagrees { reason, .. }
            | Self::SequenceNamesDisagree { reason, .. }
            | Self::EventsNotDecoded { reason, .. }
            | Self::EventOpcodeUndecoded { reason, .. }
            | Self::EventTimingUndecoded { reason, .. } => reason,
        }
    }

    /// The claim the refusal is recorded under, when it has one. The three
    /// event refusals carry one each — the stream's shape, an opcode nobody
    /// joins, and a `RUN_TIME` position nobody value-matched — the others
    /// describe content that was read and does not match, or content nobody
    /// stored.
    #[must_use]
    pub const fn claim_id(&self) -> Option<&ClaimId> {
        match self {
            Self::EventsNotDecoded { claim_id, .. }
            | Self::EventOpcodeUndecoded { claim_id, .. }
            | Self::EventTimingUndecoded { claim_id, .. } => Some(claim_id),
            _ => None,
        }
    }

    /// The event the refusal names: the stored opcode for the two event gaps,
    /// `None` for everything else.
    #[must_use]
    pub const fn event_opcode(&self) -> Option<u8> {
        match self {
            Self::EventOpcodeUndecoded { opcode, .. }
            | Self::EventTimingUndecoded { opcode, .. } => Some(*opcode),
            _ => None,
        }
    }

    /// The byte offset the refusal was found at, for the three event refusals.
    #[must_use]
    pub const fn event_offset(&self) -> Option<u64> {
        match self {
            Self::EventsNotDecoded { offset, .. } => *offset,
            Self::EventOpcodeUndecoded { offset, .. }
            | Self::EventTimingUndecoded { offset, .. } => Some(*offset),
            _ => None,
        }
    }
}

impl fmt::Display for PlayRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undeclared { reason } => write!(f, "undeclared: {reason}"),
            Self::AmbiguousDeclaration { reason, sites } => {
                write!(f, "{sites} declaring sites: {reason}")
            }
            Self::NoRecord { reason, matches } => {
                write!(f, "{} record(s) matched: {reason}", matches.len())
            }
            Self::ObjectNameDisagrees {
                reason,
                declared,
                stored,
            } => write!(
                f,
                "declared {declared:?} against the record's {stored:?}: {reason}"
            ),
            Self::SequenceNamesDisagree {
                reason,
                declared,
                stored,
            } => write!(
                f,
                "declared {declared:?} against the record's {stored:?}: {reason}"
            ),
            Self::EventsNotDecoded {
                reason, claim_id, ..
            } => {
                write!(f, "not played ({claim_id}): {reason}")
            }
            Self::EventOpcodeUndecoded {
                reason,
                claim_id,
                opcode,
                offset,
            } => write!(
                f,
                "event {opcode} at +{offset} not played ({claim_id}): {reason}"
            ),
            Self::EventTimingUndecoded {
                reason,
                claim_id,
                opcode,
                offset,
            } => write!(
                f,
                "event {opcode} at +{offset} not played ({claim_id}): {reason}"
            ),
        }
    }
}

/// One startup animation of a mission, joined across the three measurements.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupAnimation {
    /// The `.zrd` member half: which member declares this name, or why none
    /// does.
    pub declaration: StartupAnimationBinding,
    /// The payload half: which carrier record stores this name, or why none
    /// does.
    pub record: RecordResolution,
    /// Every name this animation addresses, in join order: the declaration's
    /// selectors, then the record's object, root and node-table entries.
    pub targets: Vec<AnimationTarget>,
    /// The record's playback, when every sequence block decoded and every
    /// opcode joined a statement. `None` when the record holds no bytes or one
    /// of its blocks refused to decode, in which case a refusal says which and
    /// where. A `Some` can stand beside a refusal of the *name* agreements, so
    /// [`StartupAnimation::is_playable`] — not `playback().is_some()` — is the
    /// play decision.
    pub playback: Option<RecordPlayback>,
    /// Every refusal, in the order the join found them. Empty means playable.
    pub refusals: Vec<PlayRefusal>,
}

impl StartupAnimation {
    /// The startup event that fires this animation.
    #[must_use]
    pub fn event(&self) -> &str {
        self.declaration.event()
    }

    /// The animation name as `startanims.zrd` spells it.
    #[must_use]
    pub fn identity(&self) -> &str {
        self.declaration.animation_name()
    }

    /// Every name this animation addresses, in join order: the declaration's
    /// selectors, then the record's object, root and node-table entries.
    #[must_use]
    pub fn targets(&self) -> &[AnimationTarget] {
        &self.targets
    }

    /// Every refusal, in the order the join found them. Empty means playable.
    #[must_use]
    pub fn refusals(&self) -> &[PlayRefusal] {
        &self.refusals
    }

    /// The `.zrd` member half: which member declares this name, or why none
    /// does.
    #[must_use]
    pub const fn declaration(&self) -> &StartupAnimationBinding {
        &self.declaration
    }

    /// The payload half: which carrier record stores this name, or why none
    /// does.
    #[must_use]
    pub const fn record(&self) -> &RecordResolution {
        &self.record
    }

    /// The bound payload record, when there is one.
    #[must_use]
    pub fn bound_record(&self) -> Option<&AnimationRecordFacts> {
        self.record.bound()
    }

    /// The targets that selected at least one world record.
    pub fn resolved_targets(&self) -> impl Iterator<Item = &AnimationTarget> {
        self.targets
            .iter()
            .filter(|target| target.resolution.occurrences().is_some())
    }

    /// Whether this animation can be played.
    ///
    /// `true` exactly when the join found no refusal: the two name agreements
    /// hold **and** every sequence block decoded, so a duration and a per-tick
    /// pose report exist ([`Self::playback`]). A record carrying an opcode no
    /// declaration joins, or a `RUN_TIME` nobody value-matched, is refused
    /// instead: partial decoding never yields a partial playback.
    #[must_use]
    pub fn is_playable(&self) -> bool {
        self.refusals.is_empty()
    }

    /// The record's playback — its duration and its per-tick pose report —
    /// when every sequence block decoded. `None` when no record is bound or any
    /// block refused to decode, so a caller can never read a duration out of a
    /// record whose events are unknown.
    ///
    /// It can be `Some` for a row that is still **not** playable: a name
    /// disagreement refuses the join, not the decoding. Ask
    /// [`Self::is_playable`] before playing.
    #[must_use]
    pub const fn playback(&self) -> Option<&RecordPlayback> {
        self.playback.as_ref()
    }
}

/// Joins one startup identity's declaration and payload record into the
/// consumer's row.
///
/// This is the whole join rule, exposed so it can be driven without an
/// installation: it checks the two name agreements, collects every stored name
/// the animation addresses, and names every gap that blocks playback. It never
/// repairs a disagreement and never invents a record.
#[must_use]
pub fn join_startup_animation(
    declaration: StartupAnimationBinding,
    record: RecordResolution,
    world: Option<&WorldNodeNames>,
) -> StartupAnimation {
    let mut refusals = Vec::new();
    match declaration.resolution() {
        BindingResolution::Single(site) => {
            if let Some(facts) = record.bound() {
                refusals.extend(check_agreements(site, facts));
            }
        }
        BindingResolution::Ambiguous(sites) => {
            refusals.push(PlayRefusal::AmbiguousDeclaration {
                reason: AMBIGUOUS_DECLARATION_REASON,
                sites: sites.len(),
            });
        }
        BindingResolution::Unresolved => {
            refusals.push(PlayRefusal::Undeclared {
                reason: UNDECLARED_REASON,
            });
        }
    }
    let mut playback = None;
    match &record {
        RecordResolution::Bound(facts) => match facts.playback() {
            Ok(decoded) => playback = Some(decoded),
            Err(gap) => refusals.push(gap.refusal()),
        },
        RecordResolution::Unbound { reason, matches } => {
            refusals.push(PlayRefusal::NoRecord {
                reason,
                matches: matches.clone(),
            });
        }
    }
    let mut targets = Vec::new();
    if let Some(site) = declaration.resolution().single() {
        for selector in site.objects().selectors() {
            targets.push(AnimationTarget::resolve(
                TargetSource::DeclaredSelector,
                selector.stored(),
                world,
            ));
        }
    }
    if let Some(facts) = record.bound() {
        for (source, names) in [
            (
                TargetSource::RecordObject,
                std::slice::from_ref(&facts.object_name),
            ),
            (
                TargetSource::RecordRoot,
                std::slice::from_ref(&facts.root_name),
            ),
            (TargetSource::RecordNode, facts.nodes.as_slice()),
        ] {
            for name in names {
                targets.push(AnimationTarget::resolve(source, name.clone(), world));
            }
        }
    }
    StartupAnimation {
        declaration,
        record,
        targets,
        playback,
        refusals,
    }
}

/// The two name agreements, as refusals. An empty result is the measurement
/// that the record and its declaring member are one animation.
fn check_agreements(
    site: &AnimationDefinitionSite,
    facts: &AnimationRecordFacts,
) -> Vec<PlayRefusal> {
    let mut refusals = Vec::new();
    let declared: Vec<String> = site
        .objects()
        .selectors()
        .map(|selector| selector.stored().to_owned())
        .collect();
    if !declared.iter().any(|name| name == &facts.object_name) {
        refusals.push(PlayRefusal::ObjectNameDisagrees {
            reason: OBJECT_NAME_DISAGREES_REASON,
            declared,
            stored: facts.object_name.clone(),
        });
    }
    let declared_sequences: Vec<Option<String>> = site
        .definition()
        .sequences()
        .iter()
        .map(|sequence| sequence.name().map(str::to_owned))
        .collect();
    let stored_sequences: Vec<String> = facts
        .ordinary_sequences()
        .map(|block| block.name.clone())
        .collect();
    let agrees = declared_sequences.len() == stored_sequences.len()
        && declared_sequences
            .iter()
            .zip(&stored_sequences)
            .all(|(declared, stored)| declared.as_deref().unwrap_or_default() == stored);
    if !agrees {
        refusals.push(PlayRefusal::SequenceNamesDisagree {
            reason: SEQUENCE_NAMES_DISAGREE_REASON,
            declared: declared_sequences,
            stored: stored_sequences,
        });
    }
    refusals
}

/// One world actor the mission's own archive places at startup.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldActorPlacement {
    /// The archive that declares the placement (`zbd/c1c/m01/zrdr.zbd` for
    /// M01's `placezeps.zrd`).
    pub archive: String,
    /// The member that declares it.
    pub member: String,
    /// The definition's index inside that member.
    pub definition: usize,
    /// The animation name the definition declares, when it declares one.
    pub animation_name: Option<String>,
    /// The object selectors the placement states, resolved against the
    /// mission's world container.
    pub targets: Vec<AnimationTarget>,
    /// The claim under which "not spawned yet" is recorded
    /// ([`PLACEMENT_FIELDS_CLAIM`]).
    pub claim_id: ClaimId,
}

impl WorldActorPlacement {
    /// The archive that declares the placement.
    #[must_use]
    pub fn archive(&self) -> &str {
        &self.archive
    }

    /// The member that declares the placement.
    #[must_use]
    pub fn member(&self) -> &str {
        &self.member
    }

    /// The definition's index inside that member.
    #[must_use]
    pub const fn definition(&self) -> usize {
        self.definition
    }

    /// The animation name the definition declares, when it declares one.
    #[must_use]
    pub fn animation_name(&self) -> Option<&str> {
        self.animation_name.as_deref()
    }

    /// The object selectors the placement states, resolved against the
    /// mission's world container.
    #[must_use]
    pub fn targets(&self) -> &[AnimationTarget] {
        &self.targets
    }

    /// The claim under which "not spawned yet" is recorded
    /// ([`PLACEMENT_FIELDS_CLAIM`]).
    #[must_use]
    pub const fn claim_id(&self) -> &ClaimId {
        &self.claim_id
    }

    /// [`PLACEMENT_FIELDS_REASON`], verbatim.
    #[must_use]
    pub const fn unplaced_reason(&self) -> &'static str {
        PLACEMENT_FIELDS_REASON
    }
}

/// One animation carrier the mission's playback inputs were read from.
#[derive(Clone, Debug, PartialEq)]
pub struct CarrierFact {
    /// Mission (`mis_anim.zbd`) or camera (`cam_anim.zbd`).
    pub kind: CarrierKind,
    /// The carrier's logical key.
    pub container_key: String,
    /// How many records the walk reached.
    pub record_count: usize,
    /// The count the carrier's own header declared.
    pub declared_record_count: u16,
    /// Everything that went wrong reading this carrier, as the reader's own
    /// stable codes. A blocked carrier is reported, never dropped.
    pub blockers: Vec<String>,
}

impl CarrierFact {
    /// Mission (`mis_anim.zbd`) or camera (`cam_anim.zbd`).
    #[must_use]
    pub const fn kind(&self) -> CarrierKind {
        self.kind
    }

    /// The carrier's logical key.
    #[must_use]
    pub fn container_key(&self) -> &str {
        &self.container_key
    }

    /// How many records the walk reached.
    #[must_use]
    pub const fn record_count(&self) -> usize {
        self.record_count
    }

    /// The count the carrier's own header declared.
    #[must_use]
    pub const fn declared_record_count(&self) -> u16 {
        self.declared_record_count
    }

    /// Everything that went wrong reading this carrier, as the reader's own
    /// stable codes. A blocked carrier is reported, never dropped.
    #[must_use]
    pub fn blockers(&self) -> &[String] {
        &self.blockers
    }

    /// Whether the carrier's records were walked.
    #[must_use]
    pub const fn is_walked(&self) -> bool {
        self.blockers.is_empty()
    }
}

/// One startup event's answer: what it fires and what became of each animation.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionAnimationRun {
    event: String,
    rows: Vec<StartupAnimation>,
}

impl MissionAnimationRun {
    /// The startup event this run answers (`NEW_GAME_START`,
    /// `LOAD_GAME_START`).
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// One row per identity the event names, in stored order.
    #[must_use]
    pub fn rows(&self) -> &[StartupAnimation] {
        &self.rows
    }

    /// The rows that can be played.
    pub fn playable(&self) -> impl Iterator<Item = &StartupAnimation> {
        self.rows.iter().filter(|row| row.is_playable())
    }

    /// The rows that were refused, with every refusal named.
    pub fn refused(&self) -> impl Iterator<Item = (&StartupAnimation, Vec<&PlayRefusal>)> {
        self.rows
            .iter()
            .filter(|row| !row.refusals.is_empty())
            .map(|row| (row, row.refusals.iter().collect()))
    }

    /// How many identities the event names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the event names nothing. Measured content: a `LOAD_GAME_START`
    /// with no identities is a record, not a failure.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A mission scope's animation playback inputs, joined and refused.
///
/// This is what a mission holds for one scope. It answers three questions from
/// measured facts — which members declare each startup animation and which
/// carrier record stores it ([`StartupAnimation`]); which world nodes those
/// declarations and records address ([`AnimationTarget`]); and which world
/// actors the mission's own archive places at startup
/// ([`WorldActorPlacement`]) — and it refuses to play anything, by name and with
/// a source locator, because no record's event stream is decoded
/// ([`EVENTS_NOT_DECODED_REASON`]).
#[derive(Clone, Debug, PartialEq)]
pub struct MissionAnimationBinding {
    scope: String,
    group: String,
    archives: Vec<String>,
    world_container: String,
    carriers: Vec<CarrierFact>,
    startup: Vec<StartupAnimation>,
    placements: Vec<WorldActorPlacement>,
    provenance: Provenance,
}

impl MissionAnimationBinding {
    /// The mission scope, as `zbd/<group>/<mission>`.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// The scope's world group, as its directory name (`c1c`).
    #[must_use]
    pub fn group(&self) -> &str {
        &self.group
    }

    /// The world container the object selectors are resolved against.
    #[must_use]
    pub fn world_container(&self) -> &str {
        &self.world_container
    }

    /// Every reader archive read for the scope, in search order: the mission's
    /// own, its world group's, then the shared root.
    #[must_use]
    pub fn archives(&self) -> &[String] {
        &self.archives
    }

    /// The carriers the payload records were read from.
    #[must_use]
    pub fn carriers(&self) -> &[CarrierFact] {
        &self.carriers
    }

    /// Every startup animation of the scope, in event order then stored order.
    #[must_use]
    pub fn startup(&self) -> &[StartupAnimation] {
        &self.startup
    }

    /// The startup animations one event fires, in stored order.
    #[must_use]
    pub fn startup_of(&self, event: &str) -> Vec<&StartupAnimation> {
        self.startup
            .iter()
            .filter(|row| row.event() == event)
            .collect()
    }

    /// The world actors the mission's own archive places at startup.
    #[must_use]
    pub fn placements(&self) -> &[WorldActorPlacement] {
        &self.placements
    }

    /// How many startup animations can be played. Zero today, for every record
    /// of the installation.
    #[must_use]
    pub fn playable_count(&self) -> usize {
        self.startup.iter().filter(|row| row.is_playable()).count()
    }

    /// How many startup animations are refused.
    #[must_use]
    pub fn refused_count(&self) -> usize {
        self.startup.iter().filter(|row| !row.is_playable()).count()
    }

    /// One startup animation by identity, when the scope declares it.
    #[must_use]
    pub fn animation(&self, identity: &str) -> Option<&StartupAnimation> {
        self.startup.iter().find(|row| row.identity() == identity)
    }

    /// Every world-node name the scope's startup animations address, in join
    /// order and without dropping a stored entry.
    pub fn world_targets(&self) -> impl Iterator<Item = (&AnimationTarget, &StartupAnimation)> {
        self.startup
            .iter()
            .flat_map(|row| row.targets.iter().map(move |target| (target, row)))
    }

    /// The claim the join itself is recorded under ([`DECLARATION_MATCH_CLAIM`]).
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Answers one startup event: the identities it names and what became of
    /// each.
    ///
    /// An event the scope's `startanims.zrd` does not declare answers an empty
    /// run: an absent event is measured content, never a failure.
    #[must_use]
    pub fn run(&self, event: &str) -> MissionAnimationRun {
        MissionAnimationRun {
            event: event.to_owned(),
            rows: self
                .startup
                .iter()
                .filter(|row| row.event() == event)
                .cloned()
                .collect(),
        }
    }
}

/// Reads one mission scope's animation playback inputs from the installation.
///
/// `scope` is the mission's logical key, e.g. `zbd/c1c/m01`. Everything read
/// here goes through the production readers: `cs_assets::install::discover`,
/// `cs_formats::script_raw::discover_container` and the measured `.zrd` grammar
/// for the three reader archives, `cs_formats::zbd` for the two animation
/// carriers and their records, [`super::carrier`] for the carrier bindings and
/// the identity join, and [`crate::world::retail`] for the group's `gamez.zbd`.
///
/// # Errors
///
/// [`MissionAnimationError`] when the installation cannot be inventoried, a
/// reader archive or carrier cannot be read, the mission archive carries no
/// `startanims.zrd`, or the scope's world container cannot be read. A refusal
/// **about content** — an undeclared name, an unbound identity, a name
/// disagreement — is never an `Err`: it is a row of the returned binding, with
/// its reason and its source.
pub fn bind_mission_animation(
    install_root: &Path,
    scope: &str,
) -> Result<MissionAnimationBinding, MissionAnimationError> {
    let scope = scope.to_ascii_lowercase();
    let components: Vec<&str> = scope.split('/').collect();
    let group = match components.as_slice() {
        ["zbd", group, mission] if !group.is_empty() && !mission.is_empty() => (*group).to_owned(),
        _ => {
            return Err(MissionAnimationError::Archive {
                container: scope.clone(),
                reason: "a mission scope is spelled zbd/<group>/<mission>".to_owned(),
            });
        }
    };
    let found = install::discover(install_root)
        .map_err(|error| MissionAnimationError::Discovery(error.to_string()))?;
    let install_sha256 = install::fingerprint(&found.manifest);

    // The world container the object selectors resolve against. The binding
    // reads the container's `gamez.zbd` node names only, so the texture
    // archive the container opens is the project default's selection.
    let world_container_key = format!("zbd/{group}/{WORLD_CONTAINER}");
    let container = read_world_containers(install_root)
        .and_then(|containers| containers.container(&group, &WorldTextureLoad::project_default()))
        .map_err(|error| MissionAnimationError::World {
            group: group.clone(),
            reason: error.to_string(),
        })?;
    let world = WorldNodeNames::from_gamez(container.container_key().to_owned(), container.nodes());
    let world = &world;

    // The three reader archives: the mission's own, its world group and the
    // shared root, in that search order.
    let mission_key = format!("{scope}/{READER_ARCHIVE}");
    let group_key = format!("zbd/{group}/{READER_ARCHIVE}");
    let root_key = format!("zbd/{READER_ARCHIVE}");
    let mut archives: Vec<String> = Vec::new();
    let mut members: Vec<String> = Vec::new();
    let mut declarations: Vec<AnimationDefinitionSite> = Vec::new();
    let mut startup_table = None;
    let mut reader_bytes: Vec<(String, Vec<u8>)> = Vec::new();
    for key in [mission_key.clone(), group_key.clone(), root_key.clone()] {
        let Some((spelling, bytes)) = read_optional_archive(&found, &key)? else {
            continue;
        };
        archives.push(key.clone());
        let container_sha256 = archive_digest(&found, &key);
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            MissionAnimationError::Archive {
                container: key.clone(),
                reason: error.to_string(),
            }
        })?;
        let discovery = discover_container(&key, &path, &bytes);
        let is_mission = key == mission_key;
        for program in discovery.programs() {
            let locator = program.locator();
            let Some(member) = locator.member().map(str::to_owned) else {
                continue;
            };
            if is_mission {
                members.push(member.clone());
                if member.eq_ignore_ascii_case(STARTUP_MEMBER) {
                    startup_table =
                        Some(read_startup_animations(program.bytes()).map_err(|error| {
                            MissionAnimationError::Archive {
                                container: key.clone(),
                                reason: format!("{member}: {error}"),
                            }
                        })?);
                }
            }
            // Only a member whose root record *is* `ANIMATION_DEFINITIONS` is
            // read as one: the reader refuses anything else, and an archive
            // carries members for other purposes too.
            if !declares_definition_record(program.bytes()) {
                continue;
            }
            let read =
                read_animation_definition_member(&member, program.bytes()).map_err(|error| {
                    MissionAnimationError::Archive {
                        container: key.clone(),
                        reason: format!("{member}: {error}"),
                    }
                })?;
            let span = locator.span();
            let span = SourceSpan::new(
                install_sha256,
                &key,
                Some(&member),
                span.offset,
                span.len,
                container_sha256,
            )
            .map_err(|error| MissionAnimationError::Provenance(error.to_string()))?;
            for definition in read.definitions() {
                declarations.push(AnimationDefinitionSite::new(
                    &key,
                    &member,
                    span.clone(),
                    definition.clone(),
                ));
            }
        }
        reader_bytes.push((key.clone(), bytes));
    }
    let startup_table = startup_table.ok_or_else(|| MissionAnimationError::Archive {
        container: mission_key.clone(),
        reason: format!("the mission archive carries no {STARTUP_MEMBER} member"),
    })?;
    let reader_of = |key: &str| -> SiblingReader<'_> {
        reader_bytes
            .iter()
            .find(|(found_key, _)| found_key == key)
            .map_or(SiblingReader::Absent, |(_, bytes)| {
                SiblingReader::Bytes(bytes.as_slice())
            })
    };

    // The two carriers: the scope's own mission carrier and its world group's
    // camera carrier, each joined with the reader archive that declares it.
    let carrier_keys = [
        (
            CarrierKind::Mission,
            format!("{scope}/{MISSION_CARRIER}"),
            mission_key.clone(),
        ),
        (
            CarrierKind::Camera,
            format!("zbd/{group}/{}", carrier_name(CarrierKind::Camera)),
            group_key.clone(),
        ),
    ];
    let mut bindings = Vec::new();
    let mut carrier_bytes: Vec<Vec<u8>> = Vec::new();
    let mut carrier_facts = Vec::new();
    for (kind, key, reader_key) in &carrier_keys {
        let Some((spelling, bytes)) = read_optional_archive(&found, key)? else {
            continue;
        };
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            MissionAnimationError::Archive {
                container: key.clone(),
                reason: error.to_string(),
            }
        })?;
        let binding =
            bind_animation_carrier(&path, reader_key, *kind, &bytes, reader_of(reader_key));
        carrier_facts.push(CarrierFact {
            kind: *kind,
            container_key: key.clone(),
            record_count: binding
                .payload
                .as_ref()
                .and_then(|payload| payload.records.as_ref())
                .map_or(0, |facts| facts.count),
            declared_record_count: binding
                .payload
                .as_ref()
                .map_or(0, |payload| payload.declared_record_count),
            blockers: binding
                .blockers
                .iter()
                .map(|blocker| blocker.label().to_owned())
                .collect(),
        });
        bindings.push(binding);
        carrier_bytes.push(bytes);
    }
    let mission_binding = bindings
        .iter()
        .find(|binding| binding.kind == CarrierKind::Mission);
    let camera_binding = bindings
        .iter()
        .find(|binding| binding.kind == CarrierKind::Camera);
    let identity_rows: Vec<StartupBinding> = match mission_binding {
        Some(mission) => bind_startup_identities(mission, camera_binding),
        None => Vec::new(),
    };

    // Walk each carrier's records once, so a bound identity can be read in
    // full. The paths come from `carrier_facts`, which the read loop filled in
    // the same order it pushed the bytes: a carrier the installation does not
    // hold made it into neither, so the three lists can never disagree.
    let paths = carrier_facts
        .iter()
        .map(|fact| {
            RelativePath::new(&fact.container_key).map_err(|error| MissionAnimationError::Carrier {
                container: fact.container_key.clone(),
                reason: error.to_string(),
            })
        })
        .collect::<Result<Vec<RelativePath>, MissionAnimationError>>()?;
    let mut walks: Vec<(String, u64, Result<AnimationRecords<'_>, String>)> = Vec::new();
    for ((fact, bytes), path) in carrier_facts.iter().zip(&carrier_bytes).zip(&paths) {
        let (payload_offset, walked) = walk_carrier_records(&fact.container_key, path, bytes);
        walks.push((fact.container_key.clone(), payload_offset, walked));
    }

    // Every startup identity, joined on both halves.
    let program_binding = WorldActorProgramBinding::new(
        format!("{scope}/{READER_ARCHIVE}"),
        archives.clone(),
        members,
        startup_table,
        &declarations,
    );
    let mut startup = Vec::new();
    for declaration in program_binding.startup() {
        let identity = declaration.animation_name();
        let record = match identity_rows
            .iter()
            .find(|row| row.key == declaration.event() && row.identity == identity)
        {
            Some(row) => match &row.outcome {
                StartupOutcome::Bound { carrier, record } => {
                    let walk = carrier_keys
                        .iter()
                        .find(|(kind, _, _)| kind == carrier)
                        .and_then(|(_, key, _)| {
                            walks.iter().find(|(walk_key, _, _)| walk_key == key)
                        });
                    let Some((key, payload_offset, walked)) = walk else {
                        return Err(MissionAnimationError::Carrier {
                            container: format!("a bound {} carrier has no walk", carrier.label()),
                            reason: "the carrier's own binding published a record index this \
                                     consumer cannot look up"
                                .to_owned(),
                        });
                    };
                    let walked = match walked {
                        Ok(walked) => walked,
                        // The identity's index was published by the
                        // carrier's own walk, so a walk that refuses it
                        // here is reported as an unwalked carrier, never
                        // quietly as "no such record".
                        Err(_) => {
                            return Err(MissionAnimationError::Carrier {
                                container: key.clone(),
                                reason: "the carrier's record walk was refused".to_owned(),
                            });
                        }
                    };
                    match walked.get(*record) {
                        None => RecordResolution::Unbound {
                            reason: UNBOUND_REASON_NOT_WALKED,
                            matches: vec![(*carrier, *record)],
                        },
                        Some(walked) => RecordResolution::Bound(Box::new(own_record(
                            walked,
                            *carrier,
                            key.clone(),
                            *payload_offset,
                            &install_sha256,
                            archive_digest(&found, key),
                        )?)),
                    }
                }
                StartupOutcome::Unbound { reason, matches } => RecordResolution::Unbound {
                    reason,
                    matches: matches.clone(),
                },
            },
            None => RecordResolution::Unbound {
                reason: UNBOUND_REASON_NOT_WALKED,
                matches: Vec::new(),
            },
        };
        startup.push(join_startup_animation(
            declaration.clone(),
            record,
            Some(world),
        ));
    }

    // The mission's own startup placements. The world group and the shared root
    // also declare `ON_STARTUP` definitions, but those describe content placed
    // elsewhere, which is not this mission's business.
    let mut placements = Vec::new();
    for site in program_binding.startup_activations() {
        if site.archive() != mission_key {
            continue;
        }
        let targets = site
            .objects()
            .selectors()
            .map(|selector| {
                AnimationTarget::resolve(
                    TargetSource::DeclaredSelector,
                    selector.stored(),
                    Some(world),
                )
            })
            .collect();
        placements.push(WorldActorPlacement {
            archive: site.archive().to_owned(),
            member: site.member().to_owned(),
            definition: site.definition().index(),
            animation_name: site.animation_name().map(str::to_owned),
            targets,
            claim_id: claim_id(PLACEMENT_FIELDS_CLAIM)?,
        });
    }

    let provenance = Provenance::new(
        claim_id(DECLARATION_MATCH_CLAIM)?,
        ClaimStatus::ObservedTool,
        None,
    )
    .map_err(|error| MissionAnimationError::Provenance(error.to_string()))?;
    Ok(MissionAnimationBinding {
        scope,
        group,
        archives,
        world_container: world_container_key,
        carriers: carrier_facts,
        startup,
        placements,
        provenance,
    })
}

/// Copies one walked record into owned, measured facts.
///
/// `payload_offset` is where the carrier's payload begins inside the container,
/// so the record's own span is `payload_offset + record.payload_offset()`, which
/// is what a reviewer needs to read the bytes this row is about.
fn own_record(
    record: &AnimationRecord<'_>,
    carrier: CarrierKind,
    carrier_key: String,
    payload_offset: u64,
    install_sha256: &ContentHash,
    container_sha256: Option<ContentHash>,
) -> Result<AnimationRecordFacts, MissionAnimationError> {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let span = SourceSpan::new(
        *install_sha256,
        &carrier_key,
        None,
        payload_offset + record.payload_offset(),
        record.len() as u64,
        container_sha256,
    )
    .map_err(|error| MissionAnimationError::Provenance(error.to_string()))?;
    Ok(AnimationRecordFacts {
        carrier,
        carrier_key,
        index: record.index(),
        span: span.clone(),
        provenance: Provenance::new(
            claim_id(DECLARATION_MATCH_CLAIM)?,
            ClaimStatus::ObservedTool,
            Some(span),
        )
        .map_err(|error| MissionAnimationError::Provenance(error.to_string()))?,
        anim_name: text(record.anim_name()),
        object_name: text(record.object_name()),
        root_name: text(record.root_name()),
        flags: record.flags(),
        status: record.status(),
        activation: record.activation(),
        execution_priority: record.execution_priority(),
        reset_time: record.reset_time(),
        max_health: record.max_health(),
        objects: table_names(record, AnimationRecordTableKind::Objects),
        nodes: table_names(record, AnimationRecordTableKind::Nodes),
        animation_refs: table_names(record, AnimationRecordTableKind::AnimationRefs),
        sequences: record
            .sequences()
            .iter()
            .map(|block| RecordSequence {
                kind: block.kind(),
                name: text(block.name()),
                event_bytes: block.events().len() as u64,
                events: match super::events::decode_event_stream(block.events()) {
                    Ok(decoded) => SequenceEvents::Decoded(decoded),
                    Err(error) => SequenceEvents::refused(&error),
                },
            })
            .collect(),
    })
}

/// Every name of one of a record's reference tables, verbatim and in order.
fn table_names(record: &AnimationRecord<'_>, kind: AnimationRecordTableKind) -> Vec<String> {
    let Some(table) = record.table(kind) else {
        return Vec::new();
    };
    (0..table.count())
        .filter_map(|index| table.name(index))
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect()
}

/// Walks one carrier's animation records, with the offset the payload starts at
/// so a record's own span can be quoted.
///
/// A refusal is a value, not a panic: the caller reports it against the carrier
/// and every identity bound to that carrier stays unbound with
/// [`UNBOUND_REASON_NOT_WALKED`].
fn walk_carrier_records<'a>(
    key: &'a str,
    path: &'a RelativePath,
    bytes: &'a [u8],
) -> (u64, Result<AnimationRecords<'a>, String>) {
    let mut context = ParseContext::with_defaults(key);
    let decision = match dispatch(ZbdProbe::new(key, path, header_prefix(bytes))) {
        Ok(decision) => decision,
        Err(error) => return (0, Err(error.code().to_owned())),
    };
    let index = match read_animation_index(&mut context, decision, bytes) {
        Ok(index) => index,
        Err(error) => return (0, Err(error.code().to_owned())),
    };
    let payload_offset = index.payload_offset();
    let payload = match index.payload() {
        Ok(payload) => payload,
        Err(error) => return (payload_offset, Err(error.code().to_owned())),
    };
    (
        payload_offset,
        payload.records().map_err(|error| error.code().to_owned()),
    )
}

/// The header bytes the documented animation rule evaluates: the signature and
/// version words, or the whole container when it is shorter than that.
fn header_prefix(bytes: &[u8]) -> &[u8] {
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    &bytes[..needed]
}

/// Reads one archive out of the discovery, when the installation holds it.
fn read_optional_archive(
    found: &Discovery,
    key: &str,
) -> Result<Option<(String, Vec<u8>)>, MissionAnimationError> {
    let Some(record) = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
    else {
        return Ok(None);
    };
    let spelling = record.relative_spelling.as_str().to_owned();
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
        MissionAnimationError::Archive {
            container: key.to_owned(),
            reason: error.to_string(),
        }
    })?;
    Ok(Some((spelling, bytes)))
}

/// An archive's digest out of the discovery, when the manifest carries one.
fn archive_digest(found: &Discovery, key: &str) -> Option<ContentHash> {
    found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
        .map(|record| record.sha256)
}

/// Whether a member's root record is the `ANIMATION_DEFINITIONS` record.
///
/// The production `.zrd` decoder is asked first, so a member that is not a
/// document at all is skipped rather than refused, and a document whose first
/// record names something else is skipped too.
fn declares_definition_record(bytes: &[u8]) -> bool {
    let Ok(document) = decode_zrd(bytes) else {
        return false;
    };
    document
        .as_list()
        .and_then(|children| children.first())
        .and_then(ZrdValue::as_list)
        .and_then(|record| record.first())
        .and_then(ZrdValue::as_text)
        == Some(ANIMATION_DEFINITIONS_RECORD)
}

fn claim_id(id: &str) -> Result<ClaimId, MissionAnimationError> {
    ClaimId::new(id).map_err(|error| MissionAnimationError::Provenance(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::programs::{LOAD_GAME_START, NEW_GAME_START};

    /// A startup row in the smallest shape the binding holds: a declaration
    /// nobody resolved, no record and one stored target name.
    fn startup_row(event: &str, identity: &str, refusals: Vec<PlayRefusal>) -> StartupAnimation {
        StartupAnimation {
            declaration: StartupAnimationBinding::new(
                event,
                identity,
                BindingResolution::Unresolved,
            ),
            record: RecordResolution::Unbound {
                reason: UNBOUND_REASON_NOT_WALKED,
                matches: Vec::new(),
            },
            targets: vec![AnimationTarget::resolve(
                TargetSource::RecordNode,
                format!("{identity}_node"),
                None,
            )],
            playback: None,
            refusals,
        }
    }

    fn binding(rows: Vec<StartupAnimation>) -> MissionAnimationBinding {
        MissionAnimationBinding {
            scope: "zbd/c1c/m01".to_owned(),
            group: "c1c".to_owned(),
            archives: vec!["zbd/c1c/m01/zrdr.zbd".to_owned()],
            world_container: "zbd/c1c/gamez.zbd".to_owned(),
            carriers: Vec::new(),
            startup: rows,
            placements: Vec::new(),
            provenance: Provenance::new(
                claim_id(DECLARATION_MATCH_CLAIM).expect("the static claim id is valid"),
                ClaimStatus::ObservedTool,
                None,
            )
            .expect("an observed-tool claim with no source span is valid"),
        }
    }

    /// The seam a mission asks `run` on answers exactly one event's rows, in
    /// stored order — not every row the scope holds. A refused row reports its
    /// refusals while `playable` answers the row with none, an event the table
    /// never declared is an empty run rather than a failure, and
    /// `world_targets` pairs every row's targets with the row they came from.
    ///
    /// The integration file cannot reach this seam without an installation:
    /// the binding's fields stay private, so this test builds the measured
    /// shapes in place.
    #[test]
    fn accept_m01_lc_actor_anim_playback_a_run_filters_one_event_and_reports_its_rows() {
        let undeclared = || {
            vec![PlayRefusal::Undeclared {
                reason: UNDECLARED_REASON,
            }]
        };
        let binding = binding(vec![
            startup_row(NEW_GAME_START, "first", undeclared()),
            startup_row(LOAD_GAME_START, "other", undeclared()),
            startup_row(NEW_GAME_START, "second", Vec::new()),
        ]);

        let run = binding.run(NEW_GAME_START);
        assert_eq!(run.event(), NEW_GAME_START);
        assert_eq!(run.len(), 2, "only the asked event's rows answer");
        assert!(!run.is_empty());
        assert_eq!(
            run.rows()
                .iter()
                .map(StartupAnimation::identity)
                .collect::<Vec<_>>(),
            vec!["first", "second"],
            "the run keeps the event's stored order"
        );
        assert_eq!(run.playable().count(), 1, "the row with no refusal plays");
        let (row, refusals) = run.refused().next().expect("exactly one refused row");
        assert_eq!(row.identity(), "first");
        assert_eq!(refusals.len(), 1);
        assert_eq!(run.refused().count(), 1);

        // The other event answers its own row; an undeclared event is an
        // empty run, not a failure.
        assert_eq!(binding.run(LOAD_GAME_START).len(), 1);
        assert!(
            binding.run("NO_SUCH_EVENT").is_empty(),
            "an absent startup event is measured content, not an error"
        );

        // `startup_of` answers the same set by borrow, the counts agree with
        // the rows, and `world_targets` joins each target to its own row.
        assert_eq!(binding.startup_of(NEW_GAME_START).len(), 2);
        assert_eq!(binding.playable_count(), 1);
        assert_eq!(binding.refused_count(), 2);
        assert_eq!(
            binding.world_targets().count(),
            3,
            "one stored target per row"
        );
        assert!(
            binding
                .world_targets()
                .all(|(target, row)| target.stored() == format!("{}_node", row.identity())),
            "each target pairs with the row it came from"
        );
    }
}
